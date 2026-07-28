mod agent_sessions;
mod assessment;
mod calibration;
mod collector;
mod desktop_capture;
mod model;
mod monitoring;
mod overlay_movement;
mod pressure;
mod providers;
mod settings;
mod signal_timeline;
mod storage;
mod updates;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use collector::ActivityCollector;
use desktop_capture::{CaptureRect, capture};
use model::DashboardSnapshot;
use monitoring::{DashboardData, MonitoringCore};
use overlay_movement::OverlayMoveState;
use serde::{Deserialize, Serialize};
use settings::AppSettings;
use storage::Storage;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
    ipc::Response,
    menu::{Menu, MenuItemBuilder},
    tray::TrayIconBuilder,
};
use tauri_plugin_autostart::ManagerExt as _;
use updates::PendingUpdate;

struct AppState {
    monitoring: Arc<MonitoringCore>,
    overlay_move: OverlayMoveState,
}

#[derive(Serialize, Deserialize)]
struct SavedOverlayPosition {
    x: i32,
    y: i32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlayHoverPayload {
    progress: f32,
    ready: bool,
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> DashboardSnapshot {
    // 纯读 Interface：持久化由 MonitoringCore 的后台分钟节拍负责。
    state.monitoring.snapshot()
}

#[tauri::command]
fn get_dashboard_data(state: State<'_, AppState>) -> Result<DashboardData, String> {
    state.monitoring.dashboard_data()
}

#[tauri::command]
fn record_self_report(
    value: u8,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DashboardSnapshot, String> {
    let snapshot = state.monitoring.record_self_report(value)?;
    let _ = app.emit("snapshot-updated", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.monitoring.settings()
}

#[tauri::command]
fn update_settings(
    settings: AppSettings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DashboardSnapshot, String> {
    let normalized = settings.normalized();
    apply_autostart(&app, normalized.launch_at_startup)?;
    let snapshot = state.monitoring.update_settings(normalized.clone())?;
    app.emit("settings-updated", &normalized)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("snapshot-updated", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn clear_history(today_only: bool, state: State<'_, AppState>) -> Result<DashboardData, String> {
    state.monitoring.clear_history(today_only)
}

#[tauri::command]
fn set_overlay_visible(visible: bool, app: AppHandle) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "桌面黑洞窗口不存在".to_string())?;
    if visible {
        overlay.show().map_err(|error| error.to_string())?;
    } else {
        overlay.hide().map_err(|error| error.to_string())?;
    }
    // 覆盖层 WebView 收到事件后会真正停止/恢复渲染和桌面捕获。
    app.emit_to("overlay", "overlay-visibility", visible)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_overlay_visible(app: AppHandle) -> Result<bool, String> {
    app.get_webview_window("overlay")
        .ok_or_else(|| "桌面黑洞窗口不存在".to_string())?
        .is_visible()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_overlay_move_mode(state: State<'_, AppState>) -> bool {
    state.overlay_move.is_enabled()
}

#[tauri::command]
fn start_overlay_dragging(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if !state.overlay_move.is_enabled() {
        return Err("请悬停 2 秒或按 Ctrl+Alt+M 解锁桌面黑洞".to_string());
    }
    app.get_webview_window("overlay")
        .ok_or_else(|| "桌面黑洞窗口不存在".to_string())?
        .start_dragging()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn finish_overlay_dragging(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.overlay_move.lock_after_drag() {
        apply_overlay_move_mode(&app, false)?;
    }
    Ok(())
}

#[tauri::command]
async fn capture_overlay_background(app: AppHandle) -> Result<Response, String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "桌面黑洞窗口不存在".to_string())?;
    let position = overlay
        .outer_position()
        .map_err(|error| format!("无法读取悬浮窗位置：{error}"))?;
    let size = overlay
        .inner_size()
        .map_err(|error| format!("无法读取悬浮窗尺寸：{error}"))?;
    let rect = CaptureRect::new(position.x, position.y, size.width, size.height)?;

    // GDI 复制和 JPEG 压缩都放入阻塞线程；二进制帧不转 Base64，也不会写入磁盘。
    let payload = tauri::async_runtime::spawn_blocking(move || {
        // 以低质量损失换取约一个数量级的 IPC 体积下降，避免 WebView 长期保留大块 RGBA 消息。
        capture(rect).and_then(|frame| frame.into_jpeg(72))
    })
    .await
    .map_err(|error| format!("桌面捕获线程异常：{error}"))??;

    Ok(Response::new(payload))
}

fn initialize_state(app: &AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_local_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let storage = Storage::open(&data_dir.join("pressure-lens.sqlite3"))?;
    let collector = Arc::new(ActivityCollector::default());
    let monitoring = MonitoringCore::new(Arc::clone(&collector), storage)?;
    collector.start();

    Ok(AppState {
        monitoring,
        overlay_move: OverlayMoveState::default(),
    })
}

fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let currently_enabled = manager.is_enabled().map_err(|error| error.to_string())?;
    if enabled && !currently_enabled {
        manager.enable().map_err(|error| error.to_string())?;
    } else if !enabled && currently_enabled {
        manager.disable().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn install_panic_marker(path: PathBuf) {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |information| {
        // 只记录时间和代码位置，不写按键、窗口或 Agent 会话内容。
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
        {
            let location = information
                .location()
                .map(|value| format!("{}:{}", value.file(), value.line()))
                .unwrap_or_else(|| "unknown".to_string());
            let _ = writeln!(
                file,
                "{} Pressure Lens panic at {}",
                chrono::Local::now().to_rfc3339(),
                location
            );
            let _ = file.sync_all();
        }
        previous_hook(information);
    }));
}

fn overlay_position_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|directory| directory.join("overlay-position.json"))
        .map_err(|error| error.to_string())
}

fn save_overlay_position(app: &AppHandle) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "桌面黑洞窗口不存在".to_string())?;
    let position = overlay
        .outer_position()
        .map_err(|error| error.to_string())?;
    let saved = SavedOverlayPosition {
        x: position.x,
        y: position.y,
    };
    let json = serde_json::to_string(&saved).map_err(|error| error.to_string())?;
    app.state::<AppState>()
        .monitoring
        .set_runtime_state("overlay_position", &json)
}

fn load_visible_overlay_position(
    app: &AppHandle,
    overlay: &WebviewWindow,
) -> Result<Option<PhysicalPosition<i32>>, Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let stored = state.monitoring.runtime_state("overlay_position")?;
    let saved = if let Some(json) = stored {
        serde_json::from_str(&json)?
    } else {
        // 仅为旧版本迁移读取一次 JSON；新版本全部使用 SQLite 事务写入。
        let path = overlay_position_path(app)?;
        let Ok(json) = fs::read(path) else {
            return Ok(None);
        };
        let migrated: SavedOverlayPosition = serde_json::from_slice(&json)?;
        state
            .monitoring
            .set_runtime_state("overlay_position", &serde_json::to_string(&migrated)?)?;
        migrated
    };
    let overlay_size = overlay.outer_size()?;
    let center_x = saved.x as i64 + overlay_size.width as i64 / 2;
    let center_y = saved.y as i64 + overlay_size.height as i64 / 2;

    // 只有窗口中心仍位于任一显示器内才恢复，避免显示器布局变化后窗口掉到屏幕外。
    let visible = overlay.available_monitors()?.iter().any(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        center_x >= position.x as i64
            && center_x < position.x as i64 + size.width as i64
            && center_y >= position.y as i64
            && center_y < position.y as i64 + size.height as i64
    });
    Ok(visible.then_some(PhysicalPosition::new(saved.x, saved.y)))
}

fn configure_overlay(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("桌面黑洞窗口创建失败")?;

    overlay.set_ignore_cursor_events(true)?;
    overlay.set_always_on_top(true)?;
    overlay.set_skip_taskbar(true)?;
    // 桌面折射已关闭，不再存在递归采样风险；允许截图和录屏保留悬浮黑洞。
    overlay.set_content_protected(false)?;

    if let Some(position) = load_visible_overlay_position(app, &overlay)? {
        overlay.set_position(position)?;
    } else if let Some(monitor) = overlay.primary_monitor()? {
        // 首次启动放在主显示器右上角，之后恢复用户拖动后的位置。
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let overlay_size = overlay.outer_size()?;
        let x = monitor_position.x + monitor_size.width as i32 - overlay_size.width as i32 - 28;
        let y = monitor_position.y + 36;
        overlay.set_position(PhysicalPosition::new(x, y))?;
    }
    Ok(())
}

fn apply_overlay_move_mode(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "桌面黑洞窗口不存在".to_string())?;
    overlay
        .set_ignore_cursor_events(!enabled)
        .map_err(|error| error.to_string())?;
    if !enabled {
        // 锁定时记录位置；屏幕捕获帧和会话内容都不会进入这个设置文件。
        save_overlay_position(app)?;
    }
    app.emit_to("overlay", "overlay-move-mode", enabled)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn toggle_overlay_move_mode(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let enabled = state.overlay_move.toggle();
    if let Err(error) = apply_overlay_move_mode(app, enabled) {
        state.overlay_move.toggle();
        return Err(error);
    }
    Ok(enabled)
}

fn emit_overlay_hover(app: &AppHandle, progress: f32, ready: bool) {
    let _ = app.emit_to(
        "overlay",
        "overlay-hover-progress",
        OverlayHoverPayload { progress, ready },
    );
}

#[cfg(windows)]
fn start_overlay_hover_watcher(app: AppHandle) {
    use windows_sys::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

    const HOVER_DURATION: Duration = Duration::from_secs(2);
    const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(50);
    const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
    const INTERACTION_RADIUS_RATIO: f64 = 0.30;

    thread::spawn(move || {
        let mut hovered_since: Option<Instant> = None;
        let mut last_progress_step: i32 = -1;

        loop {
            let Some(overlay) = app.get_webview_window("overlay") else {
                return;
            };
            let visible = overlay.is_visible().unwrap_or(false);
            let position = overlay.outer_position();
            let size = overlay.outer_size();
            let mut cursor = POINT { x: 0, y: 0 };
            let cursor_available = unsafe { GetCursorPos(&mut cursor) } != 0;

            let inside = match (visible, cursor_available, position, size) {
                (true, true, Ok(position), Ok(size)) => {
                    let center_x = position.x as f64 + size.width as f64 / 2.0;
                    let center_y = position.y as f64 + size.height as f64 / 2.0;
                    let delta_x = cursor.x as f64 - center_x;
                    let delta_y = cursor.y as f64 - center_y;
                    let radius = size.width.min(size.height) as f64 * INTERACTION_RADIUS_RATIO;
                    delta_x * delta_x + delta_y * delta_y <= radius * radius
                }
                _ => false,
            };

            let state = app.state::<AppState>();
            if !inside {
                hovered_since = None;
                state.overlay_move.reset_after_cursor_leave();
                if state.overlay_move.lock_hover() {
                    let _ = apply_overlay_move_mode(&app, false);
                }
                if last_progress_step != 0 {
                    emit_overlay_hover(&app, 0.0, false);
                    last_progress_step = 0;
                }
                // 鼠标远离黑洞时降到 4Hz；只有真正悬停时才用 20Hz 驱动蓄力环。
                thread::sleep(IDLE_POLL_INTERVAL);
                continue;
            }

            // 快捷键模式由用户显式控制，不叠加自动悬停倒计时。
            if state.overlay_move.is_enabled() && !state.overlay_move.is_hover_armed() {
                hovered_since = None;
                if last_progress_step != 0 {
                    emit_overlay_hover(&app, 0.0, false);
                    last_progress_step = 0;
                }
                thread::sleep(IDLE_POLL_INTERVAL);
                continue;
            }

            let started = *hovered_since.get_or_insert_with(Instant::now);
            let progress =
                (started.elapsed().as_secs_f32() / HOVER_DURATION.as_secs_f32()).clamp(0.0, 1.0);
            let progress_step = (progress * 40.0).round() as i32;
            if progress_step != last_progress_step {
                emit_overlay_hover(&app, progress, state.overlay_move.is_hover_armed());
                last_progress_step = progress_step;
            }

            if progress >= 1.0 && state.overlay_move.arm_from_hover() {
                if apply_overlay_move_mode(&app, true).is_ok() {
                    emit_overlay_hover(&app, 1.0, true);
                    last_progress_step = 40;
                } else {
                    state.overlay_move.lock_hover();
                    hovered_since = None;
                    emit_overlay_hover(&app, 0.0, false);
                    last_progress_step = 0;
                }
            }

            thread::sleep(ACTIVE_POLL_INTERVAL);
        }
    });
}

#[cfg(not(windows))]
fn start_overlay_hover_watcher(_app: AppHandle) {}

#[cfg(windows)]
fn start_overlay_move_hotkey(app: AppHandle) {
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{
            MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
        },
        WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY},
    };

    const HOTKEY_ID: i32 = 0x504C;
    const VIRTUAL_KEY_M: u32 = 0x4D;

    thread::spawn(move || unsafe {
        let registered = RegisterHotKey(
            std::ptr::null_mut(),
            HOTKEY_ID,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            VIRTUAL_KEY_M,
        );
        if registered == 0 {
            return;
        }

        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            if message.message == WM_HOTKEY && message.wParam == HOTKEY_ID as usize {
                let _ = toggle_overlay_move_mode(&app);
            }
        }
        UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID);
    });
}

#[cfg(not(windows))]
fn start_overlay_move_hotkey(_app: AppHandle) {}

fn configure_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let open_dashboard = MenuItemBuilder::with_id("open-dashboard", "打开仪表盘").build(app)?;
    let toggle_overlay =
        MenuItemBuilder::with_id("toggle-overlay", "显示 / 隐藏桌面黑洞").build(app)?;
    let move_overlay =
        MenuItemBuilder::with_id("move-overlay", "移动桌面黑洞 (Ctrl+Alt+M)").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出 Pressure Lens").build(app)?;
    let menu = Menu::with_items(
        app,
        &[&open_dashboard, &toggle_overlay, &move_overlay, &quit],
    )?;

    let mut tray = TrayIconBuilder::with_id("pressure-lens")
        .menu(&menu)
        .tooltip("Pressure Lens · 本地认知负荷");
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.on_menu_event(|app, event| match event.id().as_ref() {
        "open-dashboard" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "toggle-overlay" => {
            if let Some(window) = app.get_webview_window("overlay") {
                let visible = window.is_visible().unwrap_or(false);
                let _ = set_overlay_visible(!visible, app.clone());
            }
        }
        "move-overlay" => {
            let _ = toggle_overlay_move_mode(app);
        }
        "quit" => app.exit(0),
        _ => {}
    })
    .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 单实例必须最先注册，确保第二次启动在其他插件初始化前就被接管。
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _cwd| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            },
        ))
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("pressure-lens".to_string()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                ])
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            install_panic_marker(data_dir.join("last-crash.txt"));
            let state = initialize_state(app.handle())?;
            let monitoring = Arc::clone(&state.monitoring);
            let launch_at_startup = state.monitoring.settings().launch_at_startup;
            app.manage(state);
            app.manage(PendingUpdate::default());
            apply_autostart(app.handle(), launch_at_startup)?;
            configure_overlay(app.handle())?;
            configure_tray(app.handle())?;
            start_overlay_move_hotkey(app.handle().clone());
            start_overlay_hover_watcher(app.handle().clone());
            monitoring.start(app.handle().clone());
            log::info!("Pressure Lens {} 已启动", app.package_info().version);
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭主窗口时继续在托盘和桌面覆盖层中运行。
            if window.label() == "main"
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_dashboard_data,
            record_self_report,
            get_settings,
            update_settings,
            clear_history,
            set_overlay_visible,
            get_overlay_visible,
            get_overlay_move_mode,
            start_overlay_dragging,
            finish_overlay_dragging,
            capture_overlay_background,
            updates::check_for_update,
            updates::install_update
        ]);

    if updates::is_configured() {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    let app = builder
        .build(tauri::generate_context!())
        .expect("Pressure Lens 构建失败");
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            app_handle.state::<AppState>().monitoring.finish_session();
            log::info!("Pressure Lens 已正常退出");
        }
    });
}
