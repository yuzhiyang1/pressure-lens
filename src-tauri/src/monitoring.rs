use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{
    assessment::{AssessmentContext, assess},
    calibration::CalibrationProfile,
    collector::ActivityCollector,
    model::{
        ConfidenceLevel, DashboardSnapshot, DataSourceHealth, DaySummary, HistoryPoint,
        SourceHealthStatus, VisualState,
    },
    settings::AppSettings,
    storage::Storage,
};

#[derive(Debug, Clone, Serialize)]
pub struct DashboardData {
    pub snapshot: DashboardSnapshot,
    pub history: Vec<HistoryPoint>,
    pub summary: DaySummary,
    pub settings: AppSettings,
}

/// 后台评估与 Journal 写入运行时；WebView 是否可见不会改变它的生命周期。
pub struct MonitoringCore {
    collector: Arc<ActivityCollector>,
    storage: Mutex<Storage>,
    settings: RwLock<AppSettings>,
    calibration: RwLock<CalibrationProfile>,
    latest: RwLock<DashboardSnapshot>,
    storage_healthy: AtomicBool,
    persisted_samples_today: AtomicU32,
    current_date: Mutex<String>,
    recovery_notice: RwLock<Option<String>>,
    shutdown: AtomicBool,
}

impl MonitoringCore {
    pub fn new(
        collector: Arc<ActivityCollector>,
        storage: Storage,
    ) -> Result<Arc<Self>, Box<dyn std::error::Error>> {
        let settings = storage.load_settings()?;
        let previous_lifecycle = storage.runtime_state("lifecycle")?;
        storage.set_runtime_state("lifecycle", "running")?;
        let recovery_notice = (previous_lifecycle.as_deref() == Some("running"))
            .then(|| "检测到上次未正常退出；本地 Journal 已自动恢复，采集已继续。".to_string());
        storage.prune_history(settings.retention_days)?;
        let calibration = storage.calibration_profile()?;
        let now = Local::now();
        let date = local_date(now);
        let persisted_samples_today = storage.sample_count_for_date(&date)?;
        collector.apply_settings(&settings);
        let sample = collector.snapshot();
        let pressure = assess(
            &sample,
            AssessmentContext {
                calibration,
                persisted_samples_today,
                storage_healthy: true,
            },
        );
        let latest = DashboardSnapshot {
            recorded_at: now.to_rfc3339(),
            source_health: source_health(
                &sample,
                &settings,
                true,
                now,
                collector.provider_health(),
            ),
            quiet_hours_active: settings.quiet_hours.is_active(now),
            recovery_notice: recovery_notice.clone(),
            sample,
            pressure,
        };

        Ok(Arc::new(Self {
            collector,
            storage: Mutex::new(storage),
            settings: RwLock::new(settings),
            calibration: RwLock::new(calibration),
            latest: RwLock::new(latest),
            storage_healthy: AtomicBool::new(true),
            persisted_samples_today: AtomicU32::new(persisted_samples_today),
            current_date: Mutex::new(date),
            recovery_notice: RwLock::new(recovery_notice),
            shutdown: AtomicBool::new(false),
        }))
    }

    pub fn snapshot(&self) -> DashboardSnapshot {
        self.latest.read().expect("最新评估读锁不应中毒").clone()
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.read().expect("设置读锁不应中毒").clone()
    }

    pub fn refresh(&self, now: DateTime<Local>) -> DashboardSnapshot {
        let settings = self.settings();
        let storage_healthy = self.storage_healthy.load(Ordering::Relaxed);
        let snapshot = if settings.collection_paused {
            let mut frozen = self.snapshot();
            frozen.recorded_at = now.to_rfc3339();
            frozen.sample.collection_paused = true;
            frozen.pressure.confidence = 0.2;
            frozen.pressure.confidence_level = ConfidenceLevel::Low;
            frozen.pressure.visual_state = VisualState::Uncertain;
            frozen.source_health = source_health(
                &frozen.sample,
                &settings,
                storage_healthy,
                now,
                self.collector.provider_health(),
            );
            frozen.quiet_hours_active = settings.quiet_hours.is_active(now);
            frozen.recovery_notice = self
                .recovery_notice
                .read()
                .expect("恢复提示读锁不应中毒")
                .clone();
            frozen
        } else {
            let sample = self.collector.snapshot();
            let calibration = *self.calibration.read().expect("校准档案读锁不应中毒");
            let pressure = assess(
                &sample,
                AssessmentContext {
                    calibration,
                    persisted_samples_today: self.persisted_samples_today.load(Ordering::Relaxed),
                    storage_healthy,
                },
            );
            DashboardSnapshot {
                recorded_at: now.to_rfc3339(),
                source_health: source_health(
                    &sample,
                    &settings,
                    storage_healthy,
                    now,
                    self.collector.provider_health(),
                ),
                quiet_hours_active: settings.quiet_hours.is_active(now),
                recovery_notice: self
                    .recovery_notice
                    .read()
                    .expect("恢复提示读锁不应中毒")
                    .clone(),
                sample,
                pressure,
            }
        };
        *self.latest.write().expect("最新评估写锁不应中毒") = snapshot.clone();
        snapshot
    }

    pub fn persist_minute(&self, now: DateTime<Local>) -> Result<(), String> {
        let snapshot = self.snapshot();
        if snapshot.sample.collection_paused {
            return Ok(());
        }
        let date = local_date(now);
        {
            let mut current_date = self.current_date.lock().map_err(|_| "日期锁不可用")?;
            if *current_date != date {
                *current_date = date.clone();
                self.persisted_samples_today.store(0, Ordering::Relaxed);
                let retention_days = self.settings().retention_days;
                self.storage
                    .lock()
                    .map_err(|_| "数据库锁不可用")?
                    .prune_history(retention_days)
                    .map_err(|error| error.to_string())?;
            }
        }
        let result = self
            .storage
            .lock()
            .map_err(|_| "数据库锁不可用")?
            .save_sample(now, &snapshot.sample, &snapshot.pressure)
            .map_err(|error| error.to_string());
        self.storage_healthy
            .store(result.is_ok(), Ordering::Relaxed);
        if result.is_ok() {
            self.persisted_samples_today.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    pub fn record_self_report(&self, value: u8) -> Result<DashboardSnapshot, String> {
        if !(1..=5).contains(&value) {
            return Err("自评值必须在 1 到 5 之间".to_string());
        }
        let raw_score = self.snapshot().pressure.raw_score;
        let calibration = {
            let storage = self.storage.lock().map_err(|_| "数据库锁不可用")?;
            storage
                .save_self_report(Local::now(), value, raw_score)
                .map_err(|error| error.to_string())?;
            storage
                .calibration_profile()
                .map_err(|error| error.to_string())?
        };
        *self.calibration.write().map_err(|_| "校准档案锁不可用")? = calibration;
        Ok(self.refresh(Local::now()))
    }

    pub fn update_settings(&self, settings: AppSettings) -> Result<DashboardSnapshot, String> {
        let normalized = settings.normalized();
        self.storage
            .lock()
            .map_err(|_| "数据库锁不可用")?
            .save_settings(&normalized)
            .map_err(|error| error.to_string())?;
        self.collector.apply_settings(&normalized);
        *self.settings.write().map_err(|_| "设置锁不可用")? = normalized;
        Ok(self.refresh(Local::now()))
    }

    pub fn dashboard_data(&self) -> Result<DashboardData, String> {
        let date = local_date(Local::now());
        let storage = self.storage.lock().map_err(|_| "数据库锁不可用")?;
        Ok(DashboardData {
            snapshot: self.snapshot(),
            history: storage
                .history_for_date(&date)
                .map_err(|error| error.to_string())?,
            summary: storage
                .day_summary(&date)
                .map_err(|error| error.to_string())?,
            settings: self.settings(),
        })
    }

    pub fn clear_history(&self, today_only: bool) -> Result<DashboardData, String> {
        let storage = self.storage.lock().map_err(|_| "数据库锁不可用")?;
        if today_only {
            storage
                .clear_today(&local_date(Local::now()))
                .map_err(|error| error.to_string())?;
        } else {
            storage
                .clear_all_history()
                .map_err(|error| error.to_string())?;
        }
        self.persisted_samples_today.store(0, Ordering::Relaxed);
        drop(storage);
        *self.calibration.write().map_err(|_| "校准档案锁不可用")? = CalibrationProfile::default();
        self.dashboard_data()
    }

    pub fn set_runtime_state(&self, key: &str, value: &str) -> Result<(), String> {
        self.storage
            .lock()
            .map_err(|_| "数据库锁不可用")?
            .set_runtime_state(key, value)
            .map_err(|error| error.to_string())
    }

    pub fn runtime_state(&self, key: &str) -> Result<Option<String>, String> {
        self.storage
            .lock()
            .map_err(|_| "数据库锁不可用")?
            .runtime_state(key)
            .map_err(|error| error.to_string())
    }

    pub fn finish_session(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Err(error) = self.set_runtime_state("lifecycle", "clean") {
            log::error!("清洁退出状态写入失败：{error}");
        }
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        let core = Arc::clone(self);
        thread::spawn(move || {
            let mut last_persisted_at = Instant::now();
            while !core.shutdown.load(Ordering::Relaxed) {
                let snapshot = core.refresh(Local::now());
                let _ = app.emit("snapshot-updated", &snapshot);
                if last_persisted_at.elapsed() >= Duration::from_secs(60) {
                    if let Err(error) = core.persist_minute(Local::now()) {
                        log::error!("分钟样本写入失败：{error}");
                    }
                    last_persisted_at = Instant::now();
                }
                thread::sleep(Duration::from_secs(2));
            }
        });
    }
}

fn source_health(
    sample: &crate::model::ActivitySample,
    settings: &AppSettings,
    storage_healthy: bool,
    now: DateTime<Local>,
    mut provider_health: Vec<DataSourceHealth>,
) -> Vec<DataSourceHealth> {
    let timestamp = now.to_rfc3339();
    let globally_paused = settings.collection_paused;
    let keyboard_status = if globally_paused || !settings.collect_keyboard {
        SourceHealthStatus::Paused
    } else if sample.keyboard_hook_ready {
        SourceHealthStatus::Healthy
    } else if sample.keyboard_hook_error.is_some() {
        SourceHealthStatus::Error
    } else {
        SourceHealthStatus::Limited
    };
    if provider_health.is_empty() {
        provider_health.push(DataSourceHealth {
            id: "agents".to_string(),
            label: "Agent Provider".to_string(),
            status: if globally_paused || !settings.collect_agents {
                SourceHealthStatus::Paused
            } else {
                SourceHealthStatus::Limited
            },
            detail: "Provider 正在初始化".to_string(),
            last_success_at: None,
        });
    }
    if globally_paused || !settings.collect_agents {
        for health in &mut provider_health {
            health.status = SourceHealthStatus::Paused;
            health.detail = "已按隐私设置暂停".to_string();
        }
    }
    let mut health = vec![
        DataSourceHealth {
            id: "keyboard".to_string(),
            label: "键盘节奏".to_string(),
            status: keyboard_status,
            detail: match keyboard_status {
                SourceHealthStatus::Healthy => "只保留滚动聚合计数".to_string(),
                SourceHealthStatus::Paused => "已按隐私设置暂停".to_string(),
                SourceHealthStatus::Error => format!(
                    "Windows Hook 错误 {}",
                    sample.keyboard_hook_error.unwrap_or_default()
                ),
                _ => "正在等待 Windows Hook".to_string(),
            },
            last_success_at: (keyboard_status == SourceHealthStatus::Healthy)
                .then_some(timestamp.clone()),
        },
        DataSourceHealth {
            id: "activity".to_string(),
            label: "活跃与应用切换".to_string(),
            status: if globally_paused {
                SourceHealthStatus::Paused
            } else {
                SourceHealthStatus::Healthy
            },
            detail: if globally_paused {
                "已暂停".to_string()
            } else if settings.collect_app_switches {
                "4Hz 低频前台状态采样".to_string()
            } else {
                "只计算连续活跃，应用切换已关闭".to_string()
            },
            last_success_at: (!globally_paused).then_some(timestamp.clone()),
        },
        DataSourceHealth {
            id: "history".to_string(),
            label: "本地历史".to_string(),
            status: if storage_healthy {
                SourceHealthStatus::Healthy
            } else {
                SourceHealthStatus::Error
            },
            detail: if storage_healthy {
                "SQLite WAL · 每分钟独立写入".to_string()
            } else {
                "最近一次分钟写入失败".to_string()
            },
            last_success_at: storage_healthy.then_some(timestamp),
        },
    ];
    health.extend(provider_health);
    health
}

fn local_date(now: DateTime<Local>) -> String {
    now.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::MonitoringCore;
    use crate::{collector::ActivityCollector, storage::Storage};

    fn temporary_database() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间有效")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pressure-lens-recovery-{}-{nonce}.sqlite3",
            std::process::id()
        ))
    }

    #[test]
    fn an_unclean_previous_session_is_reported_and_the_next_exit_is_marked_clean() {
        let path = temporary_database();
        {
            let storage = Storage::open(&path).expect("测试 Journal 可创建");
            storage
                .set_runtime_state("lifecycle", "running")
                .expect("可模拟未正常退出");
        }

        let collector = Arc::new(ActivityCollector::default());
        let core = MonitoringCore::new(collector, Storage::open(&path).expect("可重新打开"))
            .expect("监控核心可恢复");
        assert!(core.snapshot().recovery_notice.is_some());

        core.finish_session();
        assert_eq!(
            core.runtime_state("lifecycle").expect("可读取退出状态"),
            Some("clean".to_string())
        );
        drop(core);

        // 测试数据库不包含真实用户数据，可以在句柄释放后安全清理。
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
