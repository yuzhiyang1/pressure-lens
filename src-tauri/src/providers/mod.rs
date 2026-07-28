mod claude_code;
mod codex;
mod cursor;

use std::{path::Path, time::SystemTime};

use chrono::{DateTime, Local};

use crate::model::{AgentMetricQuality, DataSourceHealth, SourceHealthStatus};

pub use claude_code::ClaudeCodeProvider;
pub use codex::CodexProvider;
pub use cursor::CursorProvider;

#[derive(Debug, Clone)]
pub struct ProviderObservation {
    pub id: &'static str,
    pub label: &'static str,
    pub available: bool,
    pub active_sessions: u32,
    pub current_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
    pub quality: AgentMetricQuality,
    pub detail: String,
}

impl ProviderObservation {
    fn health(&self, now: SystemTime) -> DataSourceHealth {
        let status = if !self.available {
            SourceHealthStatus::Unavailable
        } else {
            match self.quality {
                AgentMetricQuality::Exact => SourceHealthStatus::Healthy,
                AgentMetricQuality::Estimated | AgentMetricQuality::ActivityOnly => {
                    SourceHealthStatus::Limited
                }
                AgentMetricQuality::Unavailable => SourceHealthStatus::Healthy,
            }
        };
        DataSourceHealth {
            id: format!("agent_{}", self.id),
            label: self.label.to_string(),
            status,
            detail: self.detail.clone(),
            last_success_at: self
                .available
                .then(|| DateTime::<Local>::from(now).to_rfc3339()),
        }
    }
}

pub trait AgentProvider: Send {
    fn poll(&mut self, now: SystemTime) -> ProviderObservation;
}

#[derive(Debug, Clone)]
pub struct AgentAggregate {
    pub source: Option<String>,
    pub active_sessions: u32,
    pub current_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
    pub quality: AgentMetricQuality,
    pub health: Vec<DataSourceHealth>,
}

pub struct ProviderRegistry {
    providers: Vec<Box<dyn AgentProvider>>,
}

impl ProviderRegistry {
    pub fn for_home(home: &Path) -> Self {
        Self {
            providers: vec![
                Box::new(CodexProvider::new(home.join(".codex").join("sessions"))),
                Box::new(ClaudeCodeProvider::new(
                    home.join(".claude").join("projects"),
                )),
                Box::new(CursorProvider::new(
                    home.join("AppData")
                        .join("Roaming")
                        .join("Cursor")
                        .join("User")
                        .join("workspaceStorage"),
                )),
            ],
        }
    }

    #[cfg(test)]
    pub fn from_providers(providers: Vec<Box<dyn AgentProvider>>) -> Self {
        Self { providers }
    }

    pub fn poll(&mut self, now: SystemTime) -> AgentAggregate {
        let observations = self
            .providers
            .iter_mut()
            .map(|provider| provider.poll(now))
            .collect::<Vec<_>>();
        let active_sessions = observations
            .iter()
            .map(|observation| observation.active_sessions)
            .sum();
        let selected = observations
            .iter()
            .filter(|observation| observation.context_percent.is_some())
            .max_by(|left, right| {
                left.context_percent
                    .partial_cmp(&right.context_percent)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .or_else(|| {
                observations
                    .iter()
                    .find(|observation| observation.active_sessions > 0)
            });
        let active_labels = observations
            .iter()
            .filter(|observation| observation.active_sessions > 0)
            .map(|observation| observation.label)
            .collect::<Vec<_>>();

        AgentAggregate {
            source: (!active_labels.is_empty()).then(|| active_labels.join(" + ")),
            active_sessions,
            current_tokens: selected.and_then(|item| item.current_tokens),
            context_window: selected.and_then(|item| item.context_window),
            context_percent: selected.and_then(|item| item.context_percent),
            quality: selected
                .map(|item| item.quality)
                .unwrap_or(AgentMetricQuality::Unavailable),
            health: observations
                .iter()
                .map(|observation| observation.health(now))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::{AgentProvider, ProviderObservation, ProviderRegistry};
    use crate::model::AgentMetricQuality;

    struct FixedProvider(ProviderObservation);

    impl AgentProvider for FixedProvider {
        fn poll(&mut self, _now: SystemTime) -> ProviderObservation {
            self.0.clone()
        }
    }

    #[test]
    fn registry_uses_the_highest_context_pressure_across_providers() {
        let mut registry = ProviderRegistry::from_providers(vec![
            Box::new(FixedProvider(ProviderObservation {
                id: "codex",
                label: "Codex",
                available: true,
                active_sessions: 1,
                current_tokens: Some(80_000),
                context_window: Some(200_000),
                context_percent: Some(40.0),
                quality: AgentMetricQuality::Exact,
                detail: "fixture".to_string(),
            })),
            Box::new(FixedProvider(ProviderObservation {
                id: "claude_code",
                label: "Claude Code",
                available: true,
                active_sessions: 2,
                current_tokens: Some(160_000),
                context_window: Some(200_000),
                context_percent: Some(80.0),
                quality: AgentMetricQuality::Estimated,
                detail: "fixture".to_string(),
            })),
        ]);

        let aggregate = registry.poll(SystemTime::now());

        assert_eq!(aggregate.active_sessions, 3);
        assert_eq!(aggregate.context_percent, Some(80.0));
        assert_eq!(aggregate.quality, AgentMetricQuality::Estimated);
        assert_eq!(aggregate.source.as_deref(), Some("Codex + Claude Code"));
    }
}
