use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde_json::Value;

use crate::{
    model::AgentMetricQuality,
    providers::{AgentProvider, ProviderObservation},
};

const ACTIVE_WINDOW: Duration = Duration::from_secs(120);
const INVENTORY_INTERVAL: Duration = Duration::from_secs(30);
const MAX_TAIL_BYTES: u64 = 512 * 1024;

pub struct ClaudeCodeProvider {
    root: PathBuf,
    known_files: Vec<PathBuf>,
    last_inventory_at: Option<SystemTime>,
    cache: HashMap<PathBuf, CachedUsage>,
}

#[derive(Clone, Copy)]
struct CachedUsage {
    length: u64,
    modified_at: SystemTime,
    usage: Option<(u64, u64)>,
}

impl ClaudeCodeProvider {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            known_files: Vec::new(),
            last_inventory_at: None,
            cache: HashMap::new(),
        }
    }
}

impl AgentProvider for ClaudeCodeProvider {
    fn poll(&mut self, now: SystemTime) -> ProviderObservation {
        let available = self.root.is_dir();
        let refresh = self
            .last_inventory_at
            .and_then(|last| now.duration_since(last).ok())
            .is_none_or(|elapsed| elapsed >= INVENTORY_INTERVAL);
        if refresh {
            self.known_files = find_jsonl_files(&self.root);
            self.last_inventory_at = Some(now);
        }

        let mut active_sessions = 0_u32;
        let mut selected: Option<(u64, u64, f64)> = None;
        for path in &self.known_files {
            let Ok(metadata) = path.metadata() else {
                continue;
            };
            let Ok(modified_at) = metadata.modified() else {
                continue;
            };
            if now.duration_since(modified_at).unwrap_or_default() > ACTIVE_WINDOW {
                continue;
            }
            active_sessions = active_sessions.saturating_add(1);
            let cached = self.cache.get(path).copied();
            let usage = if cached.is_some_and(|entry| {
                entry.length == metadata.len() && entry.modified_at == modified_at
            }) {
                cached.and_then(|entry| entry.usage)
            } else {
                let usage = read_latest_usage(path);
                self.cache.insert(
                    path.clone(),
                    CachedUsage {
                        length: metadata.len(),
                        modified_at,
                        usage,
                    },
                );
                usage
            };
            let Some((tokens, window)) = usage else {
                continue;
            };
            let percent = ((tokens as f64 / window as f64) * 1_000.0).round() / 10.0;
            if selected.is_none_or(|(_, _, current)| percent > current) {
                selected = Some((tokens, window, percent.clamp(0.0, 100.0)));
            }
        }

        ProviderObservation {
            id: "claude_code",
            label: "Claude Code",
            available,
            active_sessions,
            current_tokens: selected.map(|item| item.0),
            context_window: selected.map(|item| item.1),
            context_percent: selected.map(|item| item.2),
            quality: if selected.is_some() {
                AgentMetricQuality::Estimated
            } else if active_sessions > 0 {
                AgentMetricQuality::ActivityOnly
            } else {
                AgentMetricQuality::Unavailable
            },
            detail: if !available {
                "未发现 .claude/projects".to_string()
            } else if selected.is_some() {
                "根据最新 assistant usage 估算上下文".to_string()
            } else if active_sessions > 0 {
                "会话活跃，但没有可用 usage".to_string()
            } else {
                "监听中，最近两分钟没有活跃会话".to_string()
            },
        }
    }
}

fn read_latest_usage(path: &Path) -> Option<(u64, u64)> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(MAX_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = String::new();
    file.read_to_string(&mut tail).ok()?;
    let lines = if start == 0 {
        tail.as_str()
    } else {
        tail.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    };
    for line in lines.lines().rev() {
        if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
            continue;
        }
        let event = serde_json::from_str::<Value>(line).ok()?;
        if event.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let usage = event.pointer("/message/usage")?;
        let input = usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cache_create = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let tokens = input
            .saturating_add(cache_create)
            .saturating_add(cache_read);
        if tokens == 0 {
            continue;
        }
        // Claude Code 日志不持久化服务端实际窗口；采用保守 200k 估算并明确标低质量。
        return Some((tokens, 200_000));
    }
    None
}

fn find_jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(find_jsonl_files(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            files.push(path);
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::ClaudeCodeProvider;
    use crate::{model::AgentMetricQuality, providers::AgentProvider};

    #[test]
    fn claude_code_estimates_context_from_structured_usage_only() {
        let root = std::env::temp_dir().join(format!(
            "pressure-lens-claude-provider-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("project")).expect("测试目录应可创建");
        fs::write(
            root.join("project").join("session.jsonl"),
            concat!(
                "{\"type\":\"user\",\"message\":{\"content\":\"不会被解析\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",",
                "\"usage\":{\"input_tokens\":6000,\"cache_creation_input_tokens\":0,",
                "\"cache_read_input_tokens\":144000,\"output_tokens\":1000}}}\n"
            ),
        )
        .expect("测试会话应可写入");
        let mut provider = ClaudeCodeProvider::new(&root);

        let observation = provider.poll(SystemTime::now());

        assert_eq!(observation.active_sessions, 1);
        assert_eq!(observation.current_tokens, Some(150_000));
        assert_eq!(observation.context_percent, Some(75.0));
        assert_eq!(observation.quality, AgentMetricQuality::Estimated);
        fs::remove_dir_all(root).expect("测试目录应可清理");
    }
}
