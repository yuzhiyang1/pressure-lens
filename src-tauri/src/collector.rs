use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use crate::{
    agent_sessions::{AgentObservation, CodexSessionCollector},
    model::ActivitySample,
};

#[derive(Default)]
struct AgentMetrics {
    context_percent: Option<f64>,
    context_tokens: Option<u64>,
    context_window: Option<u64>,
    source: Option<String>,
    automatic: bool,
    active_agents: u32,
    recent_failures: u32,
}

#[derive(Default)]
pub struct ActivityCollector {
    keypresses: AtomicU32,
    backspaces: AtomicU32,
    app_switches: AtomicU32,
    continuous_active_seconds: AtomicU64,
    started: AtomicBool,
    keyboard_hook_ready: AtomicBool,
    keyboard_hook_error: AtomicU32,
    agent: Mutex<AgentMetrics>,
}

impl ActivityCollector {
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }

        let activity_collector = Arc::clone(self);
        thread::spawn(move || collect_windows_activity(activity_collector));

        #[cfg(windows)]
        {
            let keyboard_collector = Arc::clone(self);
            thread::spawn(move || collect_windows_keyboard(keyboard_collector));
        }

        if let Some(sessions_root) = codex_sessions_root() {
            let agent_collector = Arc::clone(self);
            thread::spawn(move || collect_codex_sessions(agent_collector, sessions_root));
        }
    }

    pub fn update_agent_metrics(
        &self,
        context_percent: f64,
        active_agents: u32,
        recent_failures: u32,
    ) {
        let mut agent = self.agent.lock().expect("Agent 指标互斥锁不应中毒");
        agent.context_percent = Some(context_percent.clamp(0.0, 100.0));
        agent.context_tokens = None;
        agent.context_window = None;
        agent.source = Some("外部接入".to_string());
        agent.automatic = false;
        agent.active_agents = active_agents;
        agent.recent_failures = recent_failures;
    }

    fn update_agent_observation(&self, observation: AgentObservation) {
        let mut agent = self.agent.lock().expect("Agent 指标互斥锁不应中毒");
        agent.context_percent = observation.context_percent;
        agent.context_tokens = observation.current_tokens;
        agent.context_window = observation.context_window;
        agent.source = Some(observation.source);
        agent.automatic = true;
        agent.active_agents = observation.active_sessions;
        // Codex 目前没有稳定的结构化失败计数，保留为 0，避免猜测消息正文。
        agent.recent_failures = 0;
    }

    pub fn snapshot(&self) -> ActivitySample {
        let keys = self.keypresses.load(Ordering::Relaxed);
        let backspaces = self.backspaces.load(Ordering::Relaxed);
        let agent = self.agent.lock().expect("Agent 指标互斥锁不应中毒");

        ActivitySample {
            keys_per_minute: keys,
            backspace_ratio: if keys == 0 {
                0.0
            } else {
                backspaces as f64 / keys as f64
            },
            app_switches: self.app_switches.load(Ordering::Relaxed),
            continuous_active_seconds: self.continuous_active_seconds.load(Ordering::Relaxed),
            agent_context_percent: agent.context_percent,
            agent_context_tokens: agent.context_tokens,
            agent_context_window: agent.context_window,
            agent_source: agent.source.clone(),
            agent_automatic: agent.automatic,
            active_agents: agent.active_agents,
            recent_failures: agent.recent_failures,
            keyboard_hook_ready: self.keyboard_hook_ready.load(Ordering::Relaxed),
            keyboard_hook_error: match self.keyboard_hook_error.load(Ordering::Relaxed) {
                0 => None,
                code => Some(code),
            },
        }
    }
}

fn codex_sessions_root() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .map(|home| home.join(".codex").join("sessions"))
        .filter(|path| path.is_dir())
}

fn collect_codex_sessions(collector: Arc<ActivityCollector>, sessions_root: std::path::PathBuf) {
    let mut sessions = CodexSessionCollector::new(sessions_root, Duration::from_secs(120));
    loop {
        collector.update_agent_observation(sessions.poll(std::time::SystemTime::now()));
        thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(windows)]
fn collect_windows_activity(collector: Arc<ActivityCollector>) {
    use windows_sys::Win32::{
        Foundation::HWND,
        System::SystemInformation::GetTickCount,
        UI::{
            Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
            WindowsAndMessaging::GetForegroundWindow,
        },
    };

    let mut previous_window: HWND = std::ptr::null_mut();
    let mut last_minute = std::time::Instant::now();
    let mut last_activity_tick = std::time::Instant::now();

    loop {
        let foreground = unsafe { GetForegroundWindow() };
        if !foreground.is_null() && !previous_window.is_null() && foreground != previous_window {
            collector.app_switches.fetch_add(1, Ordering::Relaxed);
        }
        previous_window = foreground;

        if last_activity_tick.elapsed() >= Duration::from_secs(1) {
            let mut input = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if unsafe { GetLastInputInfo(&mut input) } != 0 {
                // 每秒最多累计一次，避免采集循环频率放大活跃时长。
                let idle_ms = unsafe { GetTickCount() }.wrapping_sub(input.dwTime);
                if idle_ms < 60_000 {
                    collector
                        .continuous_active_seconds
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    collector
                        .continuous_active_seconds
                        .store(0, Ordering::Relaxed);
                }
            }
            last_activity_tick = std::time::Instant::now();
        }

        if last_minute.elapsed() >= Duration::from_secs(60) {
            collector.keypresses.store(0, Ordering::Relaxed);
            collector.backspaces.store(0, Ordering::Relaxed);
            collector.app_switches.store(0, Ordering::Relaxed);
            last_minute = std::time::Instant::now();
        }

        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(windows)]
fn collect_windows_keyboard(collector: Arc<ActivityCollector>) {
    use std::sync::OnceLock;
    use windows_sys::Win32::{
        Foundation::{GetLastError, LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW,
            UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
        },
    };

    static KEYBOARD_COLLECTOR: OnceLock<Arc<ActivityCollector>> = OnceLock::new();

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && (wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN) {
            if let Some(collector) = KEYBOARD_COLLECTOR.get() {
                // 虚拟键值只在当前回调中判断是否为删除键，随后立即丢弃。
                let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
                collector.keypresses.fetch_add(1, Ordering::Relaxed);
                if event.vkCode == 0x08 || event.vkCode == 0x2E {
                    collector.backspaces.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    if KEYBOARD_COLLECTOR.set(collector).is_err() {
        return;
    }

    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0) };
    if hook.is_null() {
        if let Some(collector) = KEYBOARD_COLLECTOR.get() {
            collector
                .keyboard_hook_error
                .store(unsafe { GetLastError() }, Ordering::Relaxed);
        }
        return;
    }
    if let Some(collector) = KEYBOARD_COLLECTOR.get() {
        collector.keyboard_hook_ready.store(true, Ordering::Relaxed);
    }

    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {}
    unsafe {
        UnhookWindowsHookEx(hook);
    }
}

#[cfg(not(windows))]
fn collect_windows_activity(_collector: Arc<ActivityCollector>) {
    // 非 Windows 构建保留空采集器，便于运行核心逻辑测试。
}
