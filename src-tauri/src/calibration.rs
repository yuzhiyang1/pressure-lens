#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CalibrationProfile {
    pub adjustment: f64,
    pub report_count: u32,
}

impl CalibrationProfile {
    pub fn from_reports(reports: &[(u8, f64)]) -> Self {
        if reports.is_empty() {
            return Self::default();
        }

        let mut weighted_delta = 0.0;
        let mut total_weight = 0.0;
        for (index, (value, raw_score)) in reports.iter().rev().enumerate() {
            let target = report_target(*value);
            let weight = 0.9_f64.powi(index as i32);
            weighted_delta += (target - raw_score) * weight;
            total_weight += weight;
        }
        let desired = (weighted_delta / total_weight).clamp(-15.0, 15.0);
        // 前五次反馈逐步建立可信度，避免一次极端感受永久改写个人模型。
        let reliability = (reports.len() as f64 / 5.0).clamp(0.0, 1.0);
        Self {
            adjustment: round_one_decimal(desired * reliability),
            report_count: reports.len().min(u32::MAX as usize) as u32,
        }
    }

    pub fn apply(&self, raw_score: f64) -> f64 {
        round_one_decimal((raw_score + self.adjustment).clamp(0.0, 100.0))
    }
}

fn report_target(value: u8) -> f64 {
    match value.clamp(1, 5) {
        1 => 12.0,
        2 => 32.0,
        3 => 52.0,
        4 => 74.0,
        _ => 92.0,
    }
}

fn round_one_decimal(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::CalibrationProfile;

    #[test]
    fn one_self_report_only_nudges_the_score() {
        let profile = CalibrationProfile::from_reports(&[(5, 40.0)]);

        assert_eq!(profile.report_count, 1);
        assert!(profile.adjustment > 0.0);
        assert!(profile.adjustment <= 3.0);
    }

    #[test]
    fn repeated_self_reports_build_a_bounded_personal_adjustment() {
        let reports = vec![(5, 40.0); 8];
        let profile = CalibrationProfile::from_reports(&reports);

        assert_eq!(profile.report_count, 8);
        assert!(profile.adjustment > 10.0);
        assert!(profile.adjustment <= 15.0);
        assert_eq!(profile.apply(40.0), 55.0);
    }
}
