use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use crate::{
    agent_sessions::CodexSessionCollector,
    model::AgentMetricQuality,
    providers::{AgentProvider, ProviderObservation},
};

pub struct CodexProvider {
    root: PathBuf,
    collector: CodexSessionCollector,
}

impl CodexProvider {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            collector: CodexSessionCollector::new(&root, Duration::from_secs(120)),
            root,
        }
    }
}

impl AgentProvider for CodexProvider {
    fn poll(&mut self, now: SystemTime) -> ProviderObservation {
        let available = Path::new(&self.root).is_dir();
        let observation = self.collector.poll(now);
        ProviderObservation {
            id: "codex",
            label: "Codex",
            available,
            active_sessions: observation.active_sessions,
            current_tokens: observation.current_tokens,
            context_window: observation.context_window,
            context_percent: observation.context_percent,
            quality: if observation.context_percent.is_some() {
                AgentMetricQuality::Exact
            } else {
                AgentMetricQuality::Unavailable
            },
            detail: if !available {
                "未发现 .codex/sessions".to_string()
            } else if observation.active_sessions == 0 {
                "监听中，最近两分钟没有活跃会话".to_string()
            } else {
                "读取 token_count 结构化事件".to_string()
            },
        }
    }
}
