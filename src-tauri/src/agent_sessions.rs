use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde_json::Value;

const MAX_SESSION_TAIL_BYTES: u64 = 512 * 1024;
const INVENTORY_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub struct AgentObservation {
    pub source: String,
    pub active_sessions: u32,
    pub current_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
}

pub struct CodexSessionCollector {
    sessions_root: PathBuf,
    active_window: Duration,
    cache: HashMap<PathBuf, CachedTokenUsage>,
    known_files: Vec<PathBuf>,
    last_inventory_at: Option<SystemTime>,
}

struct CachedTokenUsage {
    file_length: u64,
    modified_at: SystemTime,
    usage: Option<(u64, u64)>,
}

impl CodexSessionCollector {
    pub fn new(sessions_root: impl Into<PathBuf>, active_window: Duration) -> Self {
        Self {
            sessions_root: sessions_root.into(),
            active_window,
            cache: HashMap::new(),
            known_files: Vec::new(),
            last_inventory_at: None,
        }
    }

    pub fn poll(&mut self, now: SystemTime) -> AgentObservation {
        let mut observation = AgentObservation {
            source: "Codex".to_string(),
            active_sessions: 0,
            current_tokens: None,
            context_window: None,
            context_percent: None,
        };

        let should_refresh_inventory = self
            .last_inventory_at
            .and_then(|last| now.duration_since(last).ok())
            .is_none_or(|elapsed| elapsed >= INVENTORY_REFRESH_INTERVAL);
        if should_refresh_inventory {
            // 全目录发现降为 30 秒一次；高频轮询只检查已知候选文件的 metadata。
            self.known_files = find_session_files(&self.sessions_root);
            self.last_inventory_at = Some(now);
        }

        let mut active_paths = HashSet::new();
        for session_file in &self.known_files {
            let Ok(metadata) = session_file.metadata() else {
                continue;
            };
            let Ok(modified_at) = metadata.modified() else {
                continue;
            };
            let age = now.duration_since(modified_at).unwrap_or_default();
            if age > self.active_window {
                continue;
            }

            active_paths.insert(session_file.clone());
            observation.active_sessions = observation.active_sessions.saturating_add(1);
            let cached = self.cache.get(session_file);
            let usage = if cached.is_some_and(|entry| {
                entry.file_length == metadata.len() && entry.modified_at == modified_at
            }) {
                cached.and_then(|entry| entry.usage)
            } else {
                let usage = read_latest_token_usage(session_file);
                self.cache.insert(
                    session_file.clone(),
                    CachedTokenUsage {
                        file_length: metadata.len(),
                        modified_at,
                        usage,
                    },
                );
                usage
            };
            let Some((current_tokens, context_window)) = usage else {
                continue;
            };
            if context_window == 0 {
                continue;
            }
            let context_percent =
                ((current_tokens as f64 / context_window as f64) * 1_000.0).round() / 10.0;
            if observation
                .context_percent
                .is_none_or(|current| context_percent > current)
            {
                // 多个会话并行时，以占用最高的会话驱动黑洞，避免平均值掩盖即将满载的会话。
                observation.current_tokens = Some(current_tokens);
                observation.context_window = Some(context_window);
                observation.context_percent = Some(context_percent.clamp(0.0, 100.0));
            }
        }

        self.cache.retain(|path, _| active_paths.contains(path));
        observation
    }
}

fn find_session_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(find_session_files(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            files.push(path);
        }
    }
    files
}

fn read_latest_token_usage(path: &Path) -> Option<(u64, u64)> {
    let mut file = File::open(path).ok()?;
    let file_length = file.metadata().ok()?.len();
    let tail_start = file_length.saturating_sub(MAX_SESSION_TAIL_BYTES);
    file.seek(SeekFrom::Start(tail_start)).ok()?;

    let mut tail = String::new();
    file.read_to_string(&mut tail).ok()?;
    let lines = if tail_start == 0 {
        tail.as_str()
    } else {
        // 从文件中部开始时，首行可能是不完整 JSON，直接跳过。
        tail.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    };

    for line in lines.lines().rev() {
        // 先做轻量字段过滤，绝不解析或保留普通用户/Agent 消息正文。
        if !line.contains("\"token_count\"") {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event.pointer("/payload/type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let Some(current_tokens) = event
            .pointer("/payload/info/last_token_usage/total_tokens")
            .and_then(Value::as_u64)
        else {
            continue;
        };
        let Some(context_window) = event
            .pointer("/payload/info/model_context_window")
            .and_then(Value::as_u64)
        else {
            continue;
        };
        return Some((current_tokens, context_window));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime},
    };

    use super::CodexSessionCollector;

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn create_test_directory(name: &str) -> PathBuf {
        let sequence = TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "pressure-lens-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("测试会话目录应可创建");
        directory
    }

    #[test]
    fn codex_context_uses_the_latest_turn_tokens_instead_of_cumulative_usage() {
        let sessions = create_test_directory("codex-context");
        let session_file = sessions.join("active-session.jsonl");
        fs::write(
            &session_file,
            concat!(
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
                "\"last_token_usage\":{\"input_tokens\":111666,\"output_tokens\":814,",
                "\"total_tokens\":112480},",
                "\"total_token_usage\":{\"total_tokens\":49821856},",
                "\"model_context_window\":258400}}}\n"
            ),
        )
        .expect("测试会话应可写入");

        let mut collector = CodexSessionCollector::new(&sessions, Duration::from_secs(120));
        let observation = collector.poll(SystemTime::now());

        assert_eq!(observation.source, "Codex");
        assert_eq!(observation.active_sessions, 1);
        assert_eq!(observation.current_tokens, Some(112_480));
        assert_eq!(observation.context_window, Some(258_400));
        assert_eq!(observation.context_percent, Some(43.5));

        fs::remove_dir_all(sessions).expect("测试目录应可清理");
    }

    #[test]
    fn codex_context_keeps_the_last_complete_event_while_a_new_line_is_being_written() {
        let sessions = create_test_directory("codex-partial-line");
        let session_file = sessions.join("active-session.jsonl");
        fs::write(
            &session_file,
            concat!(
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
                "\"last_token_usage\":{\"total_tokens\":50000},",
                "\"model_context_window\":200000}}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\""
            ),
        )
        .expect("测试会话应可写入");

        let mut collector = CodexSessionCollector::new(&sessions, Duration::from_secs(120));
        let observation = collector.poll(SystemTime::now());

        assert_eq!(observation.context_percent, Some(25.0));
        assert_eq!(observation.current_tokens, Some(50_000));

        fs::remove_dir_all(sessions).expect("测试目录应可清理");
    }

    #[test]
    fn parallel_codex_sessions_use_the_highest_context_pressure() {
        let sessions = create_test_directory("codex-parallel");
        for (name, tokens) in [("session-a.jsonl", 40_000), ("session-b.jsonl", 160_000)] {
            let event = serde_json::json!({
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "last_token_usage": { "total_tokens": tokens },
                        "model_context_window": 200_000
                    }
                }
            });
            fs::write(sessions.join(name), format!("{event}\n")).expect("并行测试会话应可写入");
        }

        let mut collector = CodexSessionCollector::new(&sessions, Duration::from_secs(120));
        let observation = collector.poll(SystemTime::now());

        assert_eq!(observation.active_sessions, 2);
        assert_eq!(observation.current_tokens, Some(160_000));
        assert_eq!(observation.context_percent, Some(80.0));

        fs::remove_dir_all(sessions).expect("测试目录应可清理");
    }
}
