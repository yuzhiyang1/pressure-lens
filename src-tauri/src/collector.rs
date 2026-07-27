use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    model::{ActivitySample, AgentMetricQuality, DataSourceHealth},
    providers::{AgentAggregate, ProviderRegistry},
    settings::AppSettings,
    signal_timeline::{CounterTotals, SignalTimeline},
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
    quality: AgentMetricQuality,
    health: Vec<DataSourceHealth>,
}

pub struct ActivityCollector {
    keypresses: AtomicU64,
    backspaces: AtomicU64,
    app_switches: AtomicU64,
    continuous_active_seconds: AtomicU64,
    started: AtomicBool,
    started_at: Instant,
    collection_paused: AtomicBool,
    collect_keyboard: AtomicBool,
    collect_app_switches: AtomicBool,
    collect_agents: AtomicBool,
    keyboard_hook_ready: AtomicBool,
    keyboard_hook_error: AtomicU32,
    timeline: Mutex<SignalTimeline>,
    agent: Mutex<AgentMetrics>,
}

impl Default for ActivityCollector {
    fn default() -> Self {
        Self {
            keypresses: AtomicU64::new(0),
            backspaces: AtomicU64::new(0),
            app_switches: AtomicU64::new(0),
            continuous_active_seconds: AtomicU64::new(0),
            started: AtomicBool::new(false),
            started_at: Instant::now(),
            collection_paused: AtomicBool::new(false),
            collect_keyboard: AtomicBool::new(true),
            collect_app_switches: AtomicBool::new(true),
            collect_agents: AtomicBool::new(true),
            keyboard_hook_ready: AtomicBool::new(false),
            keyboard_hook_error: AtomicU32::new(0),
            timeline: Mutex::new(SignalTimeline::new(
                Duration::ZERO,
                CounterTotals::default(),
            )),
            agent: Mutex::new(AgentMetrics::default()),
        }
    }
}

impl ActivityCollector {
    pub fn apply_settings(&self, settings: &AppSettings) {
        self.collection_paused
            .store(settings.collection_paused, Ordering::Relaxed);
        self.collect_keyboard
            .store(settings.collect_keyboard, Ordering::Relaxed);
        self.collect_app_switches
            .store(settings.collect_app_switches, Ordering::Relaxed);
        self.collect_agents
            .store(settings.collect_agents, Ordering::Relaxed);
        if settings.collection_paused {
            self.continuous_active_seconds.store(0, Ordering::Relaxed);
        }
        if !settings.collect_agents {
            let mut agent = self.agent.lock().expect("Agent 指标互斥锁不应中毒");
            agent.context_percent = None;
            agent.context_tokens = None;
            agent.context_window = None;
            agent.source = None;
            agent.automatic = false;
            agent.active_agents = 0;
            agent.recent_failures = 0;
            agent.quality = AgentMetricQuality::Unavailable;
        }
    }

    fn keyboard_enabled(&self) -> bool {
        !self.collection_paused.load(Ordering::Relaxed)
            && self.collect_keyboard.load(Ordering::Relaxed)
    }

    fn activity_enabled(&self) -> bool {
        !self.collection_paused.load(Ordering::Relaxed)
    }

    fn agents_enabled(&self) -> bool {
        !self.collection_paused.load(Ordering::Relaxed)
            && self.collect_agents.load(Ordering::Relaxed)
    }

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

        if let Some(home) = user_home() {
            let agent_collector = Arc::clone(self);
            thread::spawn(move || collect_agent_providers(agent_collector, home));
        }
    }

    fn update_agent_observation(&self, observation: AgentAggregate) {
        let mut agent = self.agent.lock().expect("Agent 指标互斥锁不应中毒");
        agent.context_percent = observation.context_percent;
        agent.context_tokens = observation.current_tokens;
        agent.context_window = observation.context_window;
        agent.source = observation.source;
        agent.automatic = true;
        agent.active_agents = observation.active_sessions;
        agent.quality = observation.quality;
        agent.health = observation.health;
        // Codex 目前没有稳定的结构化失败计数，保留为 0，避免猜测消息正文。
        agent.recent_failures = 0;
    }

    pub fn provider_health(&self) -> Vec<DataSourceHealth> {
        self.agent
            .lock()
            .expect("Agent 指标互斥锁不应中毒")
            .health
            .clone()
    }

    pub fn snapshot(&self) -> ActivitySample {
        let totals = CounterTotals {
            keys: self.keypresses.load(Ordering::Relaxed),
            backspaces: self.backspaces.load(Ordering::Relaxed),
            app_switches: self.app_switches.load(Ordering::Relaxed),
        };
        let rolling = self
            .timeline
            .lock()
            .expect("滚动窗口互斥锁不应中毒")
            .snapshot(self.started_at.elapsed(), totals);
        let keyboard_enabled = self.keyboard_enabled();
        let keys = if keyboard_enabled { rolling.keys } else { 0 };
        let backspaces = if keyboard_enabled {
            rolling.backspaces
        } else {
            0
        };
        let app_switches = if self.collect_app_switches.load(Ordering::Relaxed) {
            rolling.app_switches
        } else {
            0
        };
        let agent = self.agent.lock().expect("Agent 指标互斥锁不应中毒");

        ActivitySample {
            keys_per_minute: keys,
            backspace_ratio: if keys == 0 {
                0.0
            } else {
                (backspaces as f64 / keys as f64).clamp(0.0, 1.0)
            },
            app_switches,
            continuous_active_seconds: self.continuous_active_seconds.load(Ordering::Relaxed),
            agent_context_percent: agent.context_percent,
            agent_context_tokens: agent.context_tokens,
            agent_context_window: agent.context_window,
            agent_source: agent.source.clone(),
            agent_automatic: agent.automatic,
            active_agents: agent.active_agents,
            recent_failures: agent.recent_failures,
            agent_metric_quality: agent.quality,
            window_coverage_seconds: self.started_at.elapsed().as_secs().min(60),
            collection_paused: self.collection_paused.load(Ordering::Relaxed),
            keyboard_hook_ready: keyboard_enabled
                && self.keyboard_hook_ready.load(Ordering::Relaxed),
            keyboard_hook_error: if keyboard_enabled {
                match self.keyboard_hook_error.load(Ordering::Relaxed) {
                    0 => None,
                    code => Some(code),
                }
            } else {
                None
            },
        }
    }
}

fn user_home() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

fn collect_agent_providers(collector: Arc<ActivityCollector>, home: std::path::PathBuf) {
    let mut providers = ProviderRegistry::for_home(&home);
    loop {
        if collector.agents_enabled() {
            collector.update_agent_observation(providers.poll(std::time::SystemTime::now()));
        }
        thread::sleep(Duration::from_secs(5));
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
    let mut last_activity_tick = std::time::Instant::now();

    loop {
        if !collector.activity_enabled() {
            collector
                .continuous_active_seconds
                .store(0, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        let foreground = unsafe { GetForegroundWindow() };
        if collector.collect_app_switches.load(Ordering::Relaxed)
            && !foreground.is_null()
            && !previous_window.is_null()
            && foreground != previous_window
        {
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

        // 前台窗口与空闲状态不需要 10Hz 采样；4Hz 足以保持交互感知并显著减少唤醒。
        thread::sleep(Duration::from_millis(250));
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
        if code >= 0
            && (wparam as u32 == WM_KEYDOWN || wparam as u32 == WM_SYSKEYDOWN)
            && let Some(collector) = KEYBOARD_COLLECTOR.get()
        {
            if !collector.keyboard_enabled() {
                return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
            }
            // 虚拟键值只在当前回调中判断是否为删除键，随后立即丢弃。
            let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            collector.keypresses.fetch_add(1, Ordering::Relaxed);
            if event.vkCode == 0x08 || event.vkCode == 0x2E {
                collector.backspaces.fetch_add(1, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::{ActivityCollector, Ordering};
    use crate::{model::AgentMetricQuality, providers::AgentAggregate, settings::AppSettings};

    #[test]
    fn disabling_agent_collection_immediately_removes_the_stale_signal() {
        let collector = ActivityCollector::default();
        collector.update_agent_observation(AgentAggregate {
            source: Some("Codex".to_string()),
            active_sessions: 2,
            current_tokens: Some(180_000),
            context_window: Some(200_000),
            context_percent: Some(90.0),
            quality: AgentMetricQuality::Exact,
            health: Vec::new(),
        });
        let settings = AppSettings {
            collect_agents: false,
            collect_keyboard: false,
            ..AppSettings::default()
        };
        collector.keyboard_hook_ready.store(true, Ordering::Relaxed);

        collector.apply_settings(&settings);
        let sample = collector.snapshot();

        assert_eq!(sample.agent_context_percent, None);
        assert_eq!(sample.active_agents, 0);
        assert_eq!(sample.agent_metric_quality, AgentMetricQuality::Unavailable);
        assert!(!sample.keyboard_hook_ready);
    }
}
