use std::path::Path;

use chrono::{DateTime, Local};
use rusqlite::{Connection, params};
use thiserror::Error;

use crate::model::{ActivitySample, PressureReading};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite 操作失败：{0}")]
    Sqlite(#[from] rusqlite::Error),
}

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
                pressure_level TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS self_reports (
                recorded_at TEXT NOT NULL,
                value INTEGER NOT NULL CHECK(value BETWEEN 1 AND 5)
            );
            ",
        )?;
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
                recorded_at, keys_per_minute, backspace_ratio, app_switches,
                continuous_active_seconds, agent_context_percent, active_agents,
                recent_failures, pressure_score, pressure_level
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                recorded_at.to_rfc3339(),
                sample.keys_per_minute,
                sample.backspace_ratio,
                sample.app_switches,
                sample.continuous_active_seconds,
                sample.agent_context_percent,
                sample.active_agents,
                sample.recent_failures,
                reading.score,
                format!("{:?}", reading.level).to_lowercase(),
            ],
        )?;
        Ok(())
    }

    pub fn save_self_report(
        &self,
        recorded_at: DateTime<Local>,
        value: u8,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO self_reports (recorded_at, value) VALUES (?1, ?2)",
            params![recorded_at.to_rfc3339(), value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::PressureLevel, pressure::calculate_pressure};

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

        let sample = ActivitySample::default();
        let reading = calculate_pressure(&sample);
        assert_eq!(reading.level, PressureLevel::Calm);
        storage
            .save_sample(Local::now(), &sample, &reading)
            .expect("聚合样本应可保存");
    }
}
