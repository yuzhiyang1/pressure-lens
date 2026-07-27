use crate::{
    calibration::CalibrationProfile,
    model::{
        ActionAdvice, ActivitySample, AgentMetricQuality, ConfidenceLevel, PressureLevel,
        PressureReading, VisualState,
    },
    pressure::calculate_pressure,
};

#[derive(Debug, Clone, Copy)]
pub struct AssessmentContext {
    pub calibration: CalibrationProfile,
    pub persisted_samples_today: u32,
    pub storage_healthy: bool,
}

pub fn assess(sample: &ActivitySample, context: AssessmentContext) -> PressureReading {
    let raw = calculate_pressure(sample);
    let score = context.calibration.apply(raw.raw_score);
    let level = level_for_score(score);
    let confidence = calculate_confidence(sample, context);
    let confidence_level = if confidence >= 0.75 {
        ConfidenceLevel::High
    } else if confidence >= 0.45 {
        ConfidenceLevel::Medium
    } else {
        ConfidenceLevel::Low
    };
    let visual_state = if confidence < 0.35 {
        VisualState::Uncertain
    } else {
        match level {
            PressureLevel::Calm => VisualState::Calm,
            PressureLevel::Elevated => VisualState::Focused,
            PressureLevel::High => VisualState::Overloaded,
        }
    };

    PressureReading {
        score,
        raw_score: raw.raw_score,
        level,
        reasons: raw.reasons,
        confidence,
        confidence_level,
        calibration_adjustment: context.calibration.adjustment,
        calibration_reports: context.calibration.report_count,
        visual_state,
        advice: advice_for(sample, level),
    }
}

fn calculate_confidence(sample: &ActivitySample, context: AssessmentContext) -> f64 {
    if sample.collection_paused {
        return 0.2;
    }

    let mut confidence = 0.15;
    confidence += (sample.window_coverage_seconds.min(60) as f64 / 60.0) * 0.15;
    if sample.keyboard_hook_ready {
        confidence += 0.20;
    }
    // 连续活跃和前台应用来自同一个 Windows 活动采集器。
    confidence += 0.10;
    confidence += match sample.agent_metric_quality {
        AgentMetricQuality::Exact => 0.20,
        AgentMetricQuality::Estimated => 0.12,
        AgentMetricQuality::ActivityOnly => 0.05,
        AgentMetricQuality::Unavailable => 0.0,
    };
    confidence += (context.persisted_samples_today.min(10) as f64 / 10.0) * 0.10;
    confidence += (context.calibration.report_count.min(5) as f64 / 5.0) * 0.10;
    if context.storage_healthy {
        confidence += 0.10;
    }
    round_two_decimals(confidence.clamp(0.0, 1.0))
}

fn level_for_score(score: f64) -> PressureLevel {
    if score >= 70.0 {
        PressureLevel::High
    } else if score >= 40.0 {
        PressureLevel::Elevated
    } else {
        PressureLevel::Calm
    }
}

fn advice_for(sample: &ActivitySample, level: PressureLevel) -> Option<ActionAdvice> {
    if level != PressureLevel::High {
        return None;
    }
    if sample.agent_context_percent.unwrap_or_default() >= 80.0 {
        return Some(ActionAdvice {
            title: "先降低 Agent 上下文负担".to_string(),
            detail: "保存当前结论，压缩或新建会话，再继续下一步。".to_string(),
            action: "reset_agent_context".to_string(),
            suggested_minutes: Some(3),
        });
    }
    if sample.continuous_active_seconds >= 45 * 60 {
        return Some(ActionAdvice {
            title: "离开屏幕五分钟".to_string(),
            detail: "站起来、喝水，让持续注意力真正中断一次。".to_string(),
            action: "take_break".to_string(),
            suggested_minutes: Some(5),
        });
    }
    Some(ActionAdvice {
        title: "只保留一个下一步".to_string(),
        detail: "暂停新输入两分钟，写下当前最小可完成动作。".to_string(),
        action: "reduce_scope".to_string(),
        suggested_minutes: Some(2),
    })
}

fn round_two_decimals(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::{AssessmentContext, assess};
    use crate::{
        calibration::CalibrationProfile,
        model::{ActivitySample, AgentMetricQuality, ConfidenceLevel, PressureLevel, VisualState},
    };

    #[test]
    fn healthy_covered_sources_produce_a_high_confidence_assessment() {
        let sample = ActivitySample {
            keys_per_minute: 120,
            backspace_ratio: 0.08,
            app_switches: 5,
            continuous_active_seconds: 25 * 60,
            agent_context_percent: Some(45.0),
            agent_metric_quality: AgentMetricQuality::Exact,
            active_agents: 1,
            window_coverage_seconds: 60,
            keyboard_hook_ready: true,
            ..ActivitySample::default()
        };

        let reading = assess(
            &sample,
            AssessmentContext {
                calibration: CalibrationProfile {
                    adjustment: 3.0,
                    report_count: 5,
                },
                persisted_samples_today: 15,
                storage_healthy: true,
            },
        );

        assert_eq!(reading.confidence_level, ConfidenceLevel::High);
        assert!(reading.confidence >= 0.75);
        assert_eq!(reading.calibration_adjustment, 3.0);
        assert_eq!(reading.score, reading.raw_score + 3.0);
    }

    #[test]
    fn high_pressure_includes_a_concrete_action_and_stable_visual_meaning() {
        let sample = ActivitySample {
            keys_per_minute: 230,
            backspace_ratio: 0.24,
            app_switches: 16,
            continuous_active_seconds: 78 * 60,
            agent_context_percent: Some(91.0),
            agent_metric_quality: AgentMetricQuality::Exact,
            active_agents: 4,
            recent_failures: 7,
            window_coverage_seconds: 60,
            keyboard_hook_ready: true,
            ..ActivitySample::default()
        };

        let reading = assess(
            &sample,
            AssessmentContext {
                calibration: CalibrationProfile::default(),
                persisted_samples_today: 10,
                storage_healthy: true,
            },
        );

        assert_eq!(reading.level, PressureLevel::High);
        assert_eq!(reading.visual_state, VisualState::Overloaded);
        assert!(reading.advice.is_some());
    }
}
