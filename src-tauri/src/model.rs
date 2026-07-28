use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentMetricQuality {
    Exact,
    Estimated,
    ActivityOnly,
    #[default]
    Unavailable,
}

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
    pub agent_metric_quality: AgentMetricQuality,
    pub window_coverage_seconds: u64,
    pub collection_paused: bool,
    pub keyboard_hook_ready: bool,
    pub keyboard_hook_error: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PressureLevel {
    Calm,
    Elevated,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualState {
    Calm,
    Focused,
    Overloaded,
    Uncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionAdvice {
    pub title: String,
    pub detail: String,
    pub action: String,
    pub suggested_minutes: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PressureReading {
    pub score: f64,
    pub raw_score: f64,
    pub level: PressureLevel,
    pub reasons: Vec<String>,
    pub confidence: f64,
    pub confidence_level: ConfidenceLevel,
    pub calibration_adjustment: f64,
    pub calibration_reports: u32,
    pub visual_state: VisualState,
    pub advice: Option<ActionAdvice>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceHealthStatus {
    Healthy,
    Limited,
    Stale,
    Paused,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataSourceHealth {
    pub id: String,
    pub label: String,
    pub status: SourceHealthStatus,
    pub detail: String,
    pub last_success_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryPoint {
    pub recorded_at: String,
    pub score: f64,
    pub raw_score: f64,
    pub confidence: f64,
    pub level: PressureLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaySummary {
    pub date: String,
    pub sample_count: u32,
    pub covered_minutes: u32,
    pub average_score: f64,
    pub peak_score: f64,
    pub elevated_minutes: u32,
    pub high_minutes: u32,
    pub self_report_count: u32,
    pub average_self_report: Option<f64>,
    pub headline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardSnapshot {
    pub recorded_at: String,
    pub sample: ActivitySample,
    pub pressure: PressureReading,
    pub source_health: Vec<DataSourceHealth>,
    pub quiet_hours_active: bool,
    /// 上一次未正常退出时只提示恢复事实，不包含崩溃现场或用户内容。
    pub recovery_notice: Option<String>,
}
