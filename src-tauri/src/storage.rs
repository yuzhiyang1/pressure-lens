use std::path::Path;

use chrono::{DateTime, Days, Local};
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::{
    calibration::CalibrationProfile,
    model::{ActivitySample, DaySummary, HistoryPoint, PressureLevel, PressureReading},
    settings::AppSettings,
};

const ASSESSMENT_MODEL_VERSION: i64 = 2;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite 操作失败：{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("本地设置序列化失败：{0}")]
    Serialization(#[from] serde_json::Error),
}

/// Journal Module：隐藏迁移、WAL、分钟样本、自评配对、设置和日汇总。
pub struct Storage {
    connection: Connection,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let connection = Connection::open(path)?;
        let storage = Self { connection };
        storage.migrate()?;
        Ok(storage)
    }

    #[cfg(test)]
    fn in_memory() -> Result<Self, StorageError> {
        let storage = Self {
            connection: Connection::open_in_memory()?,
        };
        storage.migrate()?;
        Ok(storage)
    }

    fn migrate(&self) -> Result<(), StorageError> {
        self.connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 1000;

            CREATE TABLE IF NOT EXISTS activity_samples (
                recorded_at TEXT NOT NULL,
                keys_per_minute INTEGER NOT NULL,
                backspace_ratio REAL NOT NULL,
                app_switches INTEGER NOT NULL,
                continuous_active_seconds INTEGER NOT NULL,
                agent_context_percent REAL,
                active_agents INTEGER NOT NULL,
                recent_failures INTEGER NOT NULL,
                pressure_score REAL NOT NULL,
                pressure_level TEXT NOT NULL,
                recorded_at_unix INTEGER NOT NULL DEFAULT 0,
                local_date TEXT NOT NULL DEFAULT '',
                raw_score REAL NOT NULL DEFAULT 0,
                confidence REAL NOT NULL DEFAULT 0,
                calibration_adjustment REAL NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS self_reports (
                recorded_at TEXT NOT NULL,
                value INTEGER NOT NULL CHECK(value BETWEEN 1 AND 5),
                raw_score REAL,
                model_version INTEGER NOT NULL DEFAULT 2
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS runtime_state (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            ",
        )?;

        // 早期 MVP 数据库没有以下列；逐列检查保证用户可原地升级。
        self.ensure_column(
            "activity_samples",
            "recorded_at_unix",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("activity_samples", "local_date", "TEXT NOT NULL DEFAULT ''")?;
        self.ensure_column("activity_samples", "raw_score", "REAL NOT NULL DEFAULT 0")?;
        self.ensure_column("activity_samples", "confidence", "REAL NOT NULL DEFAULT 0")?;
        self.ensure_column(
            "activity_samples",
            "calibration_adjustment",
            "REAL NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("self_reports", "raw_score", "REAL")?;
        self.ensure_column(
            "self_reports",
            "model_version",
            "INTEGER NOT NULL DEFAULT 2",
        )?;
        self.connection.execute_batch(
            "
            UPDATE activity_samples
            SET local_date = substr(recorded_at, 1, 10)
            WHERE local_date = '';

            CREATE INDEX IF NOT EXISTS idx_activity_samples_local_date_time
            ON activity_samples(local_date, recorded_at);

            CREATE INDEX IF NOT EXISTS idx_self_reports_recorded_at
            ON self_reports(recorded_at);

            PRAGMA user_version = 2;
            ",
        )?;
        Ok(())
    }

    fn ensure_column(
        &self,
        table: &str,
        column: &str,
        declaration: &str,
    ) -> Result<(), StorageError> {
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if !columns.iter().any(|name| name == column) {
            self.connection.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {declaration};"
            ))?;
        }
        Ok(())
    }

    pub fn save_sample(
        &self,
        recorded_at: DateTime<Local>,
        sample: &ActivitySample,
        reading: &PressureReading,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO activity_samples (
                recorded_at, recorded_at_unix, local_date,
                keys_per_minute, backspace_ratio, app_switches,
                continuous_active_seconds, agent_context_percent, active_agents,
                recent_failures, pressure_score, raw_score, confidence,
                calibration_adjustment, pressure_level
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
            )",
            params![
                recorded_at.to_rfc3339(),
                recorded_at.timestamp(),
                recorded_at.format("%Y-%m-%d").to_string(),
                sample.keys_per_minute,
                sample.backspace_ratio,
                sample.app_switches,
                sample.continuous_active_seconds,
                sample.agent_context_percent,
                sample.active_agents,
                sample.recent_failures,
                reading.score,
                reading.raw_score,
                reading.confidence,
                reading.calibration_adjustment,
                level_name(reading.level),
            ],
        )?;
        Ok(())
    }

    pub fn save_self_report(
        &self,
        recorded_at: DateTime<Local>,
        value: u8,
        raw_score: f64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO self_reports (
                recorded_at, value, raw_score, model_version
            ) VALUES (?1, ?2, ?3, ?4)",
            params![
                recorded_at.to_rfc3339(),
                value,
                raw_score,
                ASSESSMENT_MODEL_VERSION
            ],
        )?;
        Ok(())
    }

    pub fn calibration_profile(&self) -> Result<CalibrationProfile, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT value, raw_score
             FROM self_reports
             WHERE raw_score IS NOT NULL AND model_version = ?1
             ORDER BY recorded_at DESC
             LIMIT 30",
        )?;
        let mut reports = statement
            .query_map(params![ASSESSMENT_MODEL_VERSION], |row| {
                Ok((row.get::<_, u8>(0)?, row.get::<_, f64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // CalibrationProfile 期望旧到新的顺序，内部再给予较新反馈更高权重。
        reports.reverse();
        Ok(CalibrationProfile::from_reports(&reports))
    }

    pub fn history_for_date(&self, date: &str) -> Result<Vec<HistoryPoint>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT recorded_at, pressure_score, raw_score, confidence, pressure_level
             FROM activity_samples
             WHERE local_date = ?1
             ORDER BY recorded_at",
        )?;
        let history = statement
            .query_map(params![date], |row| {
                Ok(HistoryPoint {
                    recorded_at: row.get(0)?,
                    score: row.get(1)?,
                    raw_score: row.get(2)?,
                    confidence: row.get(3)?,
                    level: parse_level(&row.get::<_, String>(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(history)
    }

    pub fn sample_count_for_date(&self, date: &str) -> Result<u32, StorageError> {
        let count = self.connection.query_row(
            "SELECT count(*) FROM activity_samples WHERE local_date = ?1",
            params![date],
            |row| row.get::<_, u32>(0),
        )?;
        Ok(count)
    }

    pub fn day_summary(&self, date: &str) -> Result<DaySummary, StorageError> {
        let history = self.history_for_date(date)?;
        let sample_count = history.len().min(u32::MAX as usize) as u32;
        let average_score = if history.is_empty() {
            0.0
        } else {
            history.iter().map(|point| point.score).sum::<f64>() / history.len() as f64
        };
        let peak_score = history
            .iter()
            .map(|point| point.score)
            .fold(0.0_f64, f64::max);
        let elevated_minutes = history
            .iter()
            .filter(|point| point.level == PressureLevel::Elevated)
            .count()
            .min(u32::MAX as usize) as u32;
        let high_minutes = history
            .iter()
            .filter(|point| point.level == PressureLevel::High)
            .count()
            .min(u32::MAX as usize) as u32;
        let (self_report_count, average_self_report) = self.connection.query_row(
            "SELECT count(*), avg(value)
                 FROM self_reports
                 WHERE substr(recorded_at, 1, 10) = ?1",
            params![date],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, Option<f64>>(1)?)),
        )?;

        Ok(DaySummary {
            date: date.to_string(),
            sample_count,
            covered_minutes: sample_count,
            average_score: round_one_decimal(average_score),
            peak_score: round_one_decimal(peak_score),
            elevated_minutes,
            high_minutes,
            self_report_count,
            average_self_report: average_self_report.map(round_one_decimal),
            headline: summary_headline(sample_count, average_score, peak_score),
        })
    }

    pub fn load_settings(&self) -> Result<AppSettings, StorageError> {
        let json = self
            .connection
            .query_row("SELECT json FROM app_settings WHERE id = 1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        match json {
            Some(json) => Ok(serde_json::from_str::<AppSettings>(&json)?.normalized()),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), StorageError> {
        let normalized = settings.clone().normalized();
        self.connection.execute(
            "INSERT INTO app_settings (id, json, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
            params![
                serde_json::to_string(&normalized)?,
                Local::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn set_runtime_state(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO runtime_state (key, value, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, Local::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn runtime_state(&self, key: &str) -> Result<Option<String>, StorageError> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM runtime_state WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn clear_today(&self, date: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "DELETE FROM activity_samples WHERE local_date = ?1",
            params![date],
        )?;
        self.connection.execute(
            "DELETE FROM self_reports WHERE substr(recorded_at, 1, 10) = ?1",
            params![date],
        )?;
        Ok(())
    }

    pub fn clear_all_history(&self) -> Result<(), StorageError> {
        self.connection
            .execute("DELETE FROM activity_samples", [])?;
        self.connection.execute("DELETE FROM self_reports", [])?;
        Ok(())
    }

    pub fn prune_history(&self, retention_days: u32) -> Result<(), StorageError> {
        let cutoff = Local::now()
            .date_naive()
            .checked_sub_days(Days::new(retention_days.clamp(1, 365) as u64))
            .unwrap_or_else(|| Local::now().date_naive())
            .format("%Y-%m-%d")
            .to_string();
        self.connection.execute(
            "DELETE FROM activity_samples WHERE local_date < ?1",
            params![cutoff],
        )?;
        self.connection.execute(
            "DELETE FROM self_reports WHERE substr(recorded_at, 1, 10) < ?1",
            params![cutoff],
        )?;
        Ok(())
    }
}

fn level_name(level: PressureLevel) -> &'static str {
    match level {
        PressureLevel::Calm => "calm",
        PressureLevel::Elevated => "elevated",
        PressureLevel::High => "high",
    }
}

fn parse_level(value: &str) -> PressureLevel {
    match value {
        "high" => PressureLevel::High,
        "elevated" => PressureLevel::Elevated,
        _ => PressureLevel::Calm,
    }
}

fn summary_headline(sample_count: u32, average: f64, peak: f64) -> String {
    if sample_count == 0 {
        "今天还没有完整分钟样本".to_string()
    } else if peak >= 70.0 {
        "今天出现过高负荷时段，优先安排一次真正恢复".to_string()
    } else if average >= 40.0 {
        "今天整体负荷偏高，减少切换会更有帮助".to_string()
    } else {
        "今天整体节奏平稳".to_string()
    }
}

fn round_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use super::Storage;
    use crate::{
        assessment::{AssessmentContext, assess},
        calibration::CalibrationProfile,
        model::{ActivitySample, PressureLevel},
        settings::AppSettings,
    };

    #[test]
    fn persisted_schema_contains_only_aggregated_keyboard_fields() {
        let storage = Storage::in_memory().expect("内存数据库应可创建");
        let columns: Vec<String> = storage
            .connection
            .prepare("PRAGMA table_info(activity_samples)")
            .expect("应可读取表结构")
            .query_map([], |row| row.get(1))
            .expect("应可枚举字段")
            .collect::<Result<_, _>>()
            .expect("字段读取应成功");

        assert!(columns.contains(&"keys_per_minute".to_string()));
        assert!(columns.contains(&"backspace_ratio".to_string()));
        assert!(!columns.iter().any(|name| {
            ["key", "character", "text", "window_title"]
                .iter()
                .any(|forbidden| name == forbidden)
        }));
    }

    #[test]
    fn today_history_and_summary_come_from_persisted_minute_samples() {
        let storage = Storage::in_memory().expect("内存数据库应可创建");
        let date = "2026-07-27";
        for (minute, keys) in [(10, 80), (11, 230)] {
            let recorded_at = Local
                .with_ymd_and_hms(2026, 7, 27, 9, minute, 0)
                .single()
                .expect("测试时间有效");
            let sample = ActivitySample {
                keys_per_minute: keys,
                backspace_ratio: if keys > 200 { 0.24 } else { 0.05 },
                app_switches: if keys > 200 { 16 } else { 3 },
                continuous_active_seconds: minute as u64 * 60,
                window_coverage_seconds: 60,
                keyboard_hook_ready: true,
                ..ActivitySample::default()
            };
            let reading = assess(
                &sample,
                AssessmentContext {
                    calibration: CalibrationProfile::default(),
                    persisted_samples_today: 2,
                    storage_healthy: true,
                },
            );
            storage
                .save_sample(recorded_at, &sample, &reading)
                .expect("分钟样本应可保存");
        }
        storage
            .save_self_report(
                Local
                    .with_ymd_and_hms(2026, 7, 27, 9, 12, 0)
                    .single()
                    .expect("测试时间有效"),
                4,
                60.0,
            )
            .expect("自评应可保存");

        let history = storage.history_for_date(date).expect("今日历史应可读取");
        let summary = storage.day_summary(date).expect("日总结应可读取");

        assert_eq!(history.len(), 2);
        assert_eq!(summary.sample_count, 2);
        assert!(summary.peak_score >= summary.average_score);
        assert_eq!(summary.self_report_count, 1);
        assert_eq!(summary.average_self_report, Some(4.0));
    }

    #[test]
    fn settings_round_trip_through_the_journal() {
        let storage = Storage::in_memory().expect("内存数据库应可创建");
        let settings = AppSettings {
            collection_paused: true,
            retention_days: 45,
            ..AppSettings::default()
        };

        storage.save_settings(&settings).expect("设置应可持久化");
        let loaded = storage.load_settings().expect("设置应可读取");

        assert!(loaded.collection_paused);
        assert_eq!(loaded.retention_days, 45);
    }

    #[test]
    fn high_pressure_level_survives_history_round_trip() {
        let storage = Storage::in_memory().expect("内存数据库应可创建");
        let recorded_at = Local
            .with_ymd_and_hms(2026, 7, 27, 10, 0, 0)
            .single()
            .expect("测试时间有效");
        let sample = ActivitySample {
            keys_per_minute: 240,
            backspace_ratio: 0.26,
            app_switches: 18,
            continuous_active_seconds: 90 * 60,
            agent_context_percent: Some(95.0),
            active_agents: 5,
            recent_failures: 8,
            ..ActivitySample::default()
        };
        let reading = assess(
            &sample,
            AssessmentContext {
                calibration: CalibrationProfile::default(),
                persisted_samples_today: 0,
                storage_healthy: true,
            },
        );
        storage
            .save_sample(recorded_at, &sample, &reading)
            .expect("高压样本应可保存");

        let history = storage
            .history_for_date("2026-07-27")
            .expect("历史应可读取");
        assert_eq!(history[0].level, PressureLevel::High);
    }
}
