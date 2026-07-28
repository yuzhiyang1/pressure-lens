use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use crate::{
    model::AgentMetricQuality,
    providers::{AgentProvider, ProviderObservation},
};

const ACTIVE_WINDOW: Duration = Duration::from_secs(120);
const SCAN_INTERVAL: Duration = Duration::from_secs(15);

pub struct CursorProvider {
    workspace_storage: PathBuf,
    last_scan_at: Option<SystemTime>,
    cached_active_sessions: u32,
}

impl CursorProvider {
    pub fn new(workspace_storage: impl Into<PathBuf>) -> Self {
        Self {
            workspace_storage: workspace_storage.into(),
            last_scan_at: None,
            cached_active_sessions: 0,
        }
    }
}

impl AgentProvider for CursorProvider {
    fn poll(&mut self, now: SystemTime) -> ProviderObservation {
        let available = self.workspace_storage.is_dir();
        let should_scan = self
            .last_scan_at
            .and_then(|last| now.duration_since(last).ok())
            .is_none_or(|elapsed| elapsed >= SCAN_INTERVAL);
        if should_scan {
            self.cached_active_sessions =
                count_recent_workspace_databases(&self.workspace_storage, now, ACTIVE_WINDOW);
            self.last_scan_at = Some(now);
        }

        ProviderObservation {
            id: "cursor",
            label: "Cursor",
            available,
            active_sessions: self.cached_active_sessions,
            current_tokens: None,
            context_window: None,
            context_percent: None,
            quality: if self.cached_active_sessions > 0 {
                AgentMetricQuality::ActivityOnly
            } else {
                AgentMetricQuality::Unavailable
            },
            detail: if !available {
                "未发现 Cursor workspaceStorage".to_string()
            } else if self.cached_active_sessions > 0 {
                "仅根据 workspace 元数据判断活跃，不读取聊天正文".to_string()
            } else {
                "监听中，最近两分钟没有活跃工作区".to_string()
            },
        }
    }
}

fn count_recent_workspace_databases(root: &Path, now: SystemTime, window: Duration) -> u32 {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let database = entry.path().join("state.vscdb");
            let modified = database.metadata().ok()?.modified().ok()?;
            (now.duration_since(modified).unwrap_or_default() <= window).then_some(())
        })
        .count()
        .min(u32::MAX as usize) as u32
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::CursorProvider;
    use crate::{model::AgentMetricQuality, providers::AgentProvider};

    #[test]
    fn cursor_detects_activity_without_opening_the_message_database() {
        let root = std::env::temp_dir().join(format!(
            "pressure-lens-cursor-provider-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("workspace")).expect("测试目录应可创建");
        fs::write(root.join("workspace").join("state.vscdb"), b"metadata-only")
            .expect("测试数据库占位应可写入");
        let mut provider = CursorProvider::new(&root);

        let observation = provider.poll(SystemTime::now());

        assert_eq!(observation.active_sessions, 1);
        assert_eq!(observation.context_percent, None);
        assert_eq!(observation.quality, AgentMetricQuality::ActivityOnly);
        fs::remove_dir_all(root).expect("测试目录应可清理");
    }
}
