use chrono::{DateTime, Local, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    Eco,
    #[default]
    Balanced,
    Vivid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct QuietHours {
    pub enabled: bool,
    pub start: String,
    pub end: String,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            start: "22:00".to_string(),
            end: "08:00".to_string(),
        }
    }
}

impl QuietHours {
    pub fn is_active(&self, now: DateTime<Local>) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(start) = parse_minutes(&self.start) else {
            return false;
        };
        let Some(end) = parse_minutes(&self.end) else {
            return false;
        };
        let current = now.hour() as u16 * 60 + now.minute() as u16;
        if start == end {
            return true;
        }
        if start < end {
            current >= start && current < end
        } else {
            current >= start || current < end
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub collection_paused: bool,
    pub collect_keyboard: bool,
    pub collect_app_switches: bool,
    pub collect_agents: bool,
    pub animation_intensity: f64,
    pub lens_intensity: f64,
    pub performance_mode: PerformanceMode,
    pub quiet_hours: QuietHours,
    pub retention_days: u32,
    pub launch_at_startup: bool,
    pub decorative_shape_tour: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            collection_paused: false,
            collect_keyboard: true,
            collect_app_switches: true,
            collect_agents: true,
            animation_intensity: 0.65,
            lens_intensity: 0.55,
            performance_mode: PerformanceMode::Balanced,
            quiet_hours: QuietHours::default(),
            retention_days: 30,
            launch_at_startup: false,
            decorative_shape_tour: true,
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.schema_version = 1;
        self.animation_intensity = finite_clamp(self.animation_intensity, 0.0, 1.0, 0.65);
        self.lens_intensity = finite_clamp(self.lens_intensity, 0.0, 1.0, 0.55);
        self.retention_days = self.retention_days.clamp(1, 365);
        if parse_minutes(&self.quiet_hours.start).is_none() {
            self.quiet_hours.start = "22:00".to_string();
        }
        if parse_minutes(&self.quiet_hours.end).is_none() {
            self.quiet_hours.end = "08:00".to_string();
        }
        self
    }
}

fn parse_minutes(value: &str) -> Option<u16> {
    let (hour, minute) = value.split_once(':')?;
    let hour: u16 = hour.parse().ok()?;
    let minute: u16 = minute.parse().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

fn finite_clamp(value: f64, minimum: f64, maximum: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, QuietHours};
    use chrono::{Local, TimeZone};

    #[test]
    fn overnight_quiet_hours_cover_late_evening_and_early_morning() {
        let quiet = QuietHours {
            enabled: true,
            start: "22:00".to_string(),
            end: "08:00".to_string(),
        };

        assert!(
            quiet.is_active(
                Local
                    .with_ymd_and_hms(2026, 7, 27, 23, 0, 0)
                    .single()
                    .expect("测试时间有效")
            )
        );
        assert!(
            quiet.is_active(
                Local
                    .with_ymd_and_hms(2026, 7, 28, 7, 59, 0)
                    .single()
                    .expect("测试时间有效")
            )
        );
        assert!(
            !quiet.is_active(
                Local
                    .with_ymd_and_hms(2026, 7, 28, 12, 0, 0)
                    .single()
                    .expect("测试时间有效")
            )
        );
    }

    #[test]
    fn settings_are_normalized_before_they_reach_collectors_or_renderers() {
        let settings = AppSettings {
            animation_intensity: 4.0,
            lens_intensity: f64::NAN,
            retention_days: 0,
            quiet_hours: QuietHours {
                enabled: true,
                start: "99:70".to_string(),
                end: "bad".to_string(),
            },
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(settings.animation_intensity, 1.0);
        assert_eq!(settings.lens_intensity, 0.55);
        assert_eq!(settings.retention_days, 1);
        assert_eq!(settings.quiet_hours.start, "22:00");
        assert_eq!(settings.quiet_hours.end, "08:00");
    }

    #[test]
    fn older_settings_json_gets_new_privacy_and_performance_defaults() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"schema_version":1,"collection_paused":true}"#)
                .expect("旧版设置应当可以原地升级");

        assert!(settings.collection_paused);
        assert!(settings.collect_keyboard);
        assert_eq!(settings.retention_days, 30);
        assert!(settings.decorative_shape_tour);
    }
}
