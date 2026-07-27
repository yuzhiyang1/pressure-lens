use serde::{Deserialize, Serialize};

/// 一分钟级聚合样本。这里有意不包含原始按键、窗口标题或文本内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ActivitySample {
    pub keys_per_minute: u32,
    pub backspace_ratio: f64,
    pub app_switches: u32,
    pub continuous_active_seconds: u64,
    pub agent_context_percent: Option<f64>,
    pub agent_context_tokens: Option<u64>,
    pub agent_context_window: Option<u64>,
    pub agent_source: Option<String>,
    pub agent_automatic: bool,
    pub active_agents: u32,
    pub recent_failures: u32,
    pub keyboard_hook_ready: bool,
    pub keyboard_hook_error: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PressureLevel {
    Calm,
    Elevated,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PressureReading {
    pub score: f64,
    pub level: PressureLevel,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub sample: ActivitySample,
    pub pressure: PressureReading,
}
