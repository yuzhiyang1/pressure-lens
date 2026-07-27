use crate::model::{ActivitySample, PressureLevel, PressureReading};

/// 根据可解释规则计算认知负荷。每一项先归一化，最终分数限制在 0..100。
pub fn calculate_pressure(sample: &ActivitySample) -> PressureReading {
    let typing = normalize(sample.keys_per_minute as f64, 40.0, 260.0);
    let correction = normalize(sample.backspace_ratio, 0.04, 0.28);
    let switching = normalize(sample.app_switches as f64, 2.0, 18.0);
    let duration = normalize(
        sample.continuous_active_seconds as f64,
        15.0 * 60.0,
        90.0 * 60.0,
    );
    let context = sample
        .agent_context_percent
        .map(|value| normalize(value, 35.0, 95.0))
        .unwrap_or(0.0);
    let agents = normalize(sample.active_agents as f64, 1.0, 5.0);
    let failures = normalize(sample.recent_failures as f64, 0.0, 8.0);

    // 没有 Agent 数据时，权重自然落在本机行为信号上，应用仍可独立工作。
    let agent_weight = if sample.agent_context_percent.is_some() {
        0.45
    } else {
        0.10
    };
    let local_weight = 1.0 - agent_weight;
    let local = typing * 0.30 + correction * 0.25 + switching * 0.20 + duration * 0.25;
    let agent = context * 0.55 + agents * 0.20 + failures * 0.25;
    let score = ((local * local_weight + agent * agent_weight) * 100.0).clamp(0.0, 100.0);

    let level = if score >= 70.0 {
        PressureLevel::High
    } else if score >= 40.0 {
        PressureLevel::Elevated
    } else {
        PressureLevel::Calm
    };

    let mut reasons = Vec::new();
    if sample.continuous_active_seconds >= 45 * 60 {
        reasons.push(format!(
            "已经连续活跃 {} 分钟",
            sample.continuous_active_seconds / 60
        ));
    }
    if sample.backspace_ratio >= 0.18 {
        reasons.push("删除与重写明显增多".to_string());
    }
    if sample.app_switches >= 12 {
        reasons.push("应用切换频繁，注意力可能被打断".to_string());
    }
    if sample.agent_context_percent.unwrap_or_default() >= 80.0 {
        reasons.push("Agent 上下文接近容量上限".to_string());
    }
    if sample.recent_failures >= 4 {
        reasons.push("近期失败与重试较集中".to_string());
    }

    PressureReading {
        score: round_one_decimal(score),
        level,
        reasons,
    }
}

fn normalize(value: f64, low: f64, high: f64) -> f64 {
    ((value - low) / (high - low)).clamp(0.0, 1.0)
}

fn round_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calm_local_activity_produces_a_low_explainable_score() {
        let sample = ActivitySample {
            keys_per_minute: 55,
            continuous_active_seconds: 10 * 60,
            ..Default::default()
        };

        let reading = calculate_pressure(&sample);

        assert_eq!(reading.level, PressureLevel::Calm);
        assert!(reading.score < 40.0);
        assert!(reading.reasons.is_empty());
    }

    #[test]
    fn overloaded_agent_session_surfaces_reasons_and_high_pressure() {
        let sample = ActivitySample {
            keys_per_minute: 230,
            backspace_ratio: 0.24,
            app_switches: 16,
            continuous_active_seconds: 78 * 60,
            agent_context_percent: Some(91.0),
            active_agents: 4,
            recent_failures: 7,
            keyboard_hook_ready: true,
            keyboard_hook_error: None,
            ..Default::default()
        };

        let reading = calculate_pressure(&sample);

        assert_eq!(reading.level, PressureLevel::High);
        assert!(reading.score >= 70.0);
        assert!(
            reading
                .reasons
                .iter()
                .any(|reason| reason.contains("上下文"))
        );
        assert!(reading.reasons.iter().any(|reason| reason.contains("失败")));
    }

    #[test]
    fn severe_agent_load_is_not_reported_as_calm_when_local_activity_is_low() {
        let sample = ActivitySample {
            agent_context_percent: Some(92.0),
            active_agents: 4,
            recent_failures: 7,
            ..Default::default()
        };

        let reading = calculate_pressure(&sample);

        assert_eq!(reading.level, PressureLevel::Elevated);
        assert!(reading.score >= 40.0);
    }
}
