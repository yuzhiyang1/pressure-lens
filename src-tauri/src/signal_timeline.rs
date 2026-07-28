use std::{collections::VecDeque, time::Duration};

/// 从进程启动开始只增不减的事件计数。采集回调不再负责分钟归零。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CounterTotals {
    pub keys: u64,
    pub backspaces: u64,
    pub app_switches: u64,
}

/// 最近 60 秒内的一致事件切片。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RollingSignals {
    pub keys: u32,
    pub backspaces: u32,
    pub app_switches: u32,
}

#[derive(Debug, Clone, Copy)]
struct CounterPoint {
    at: Duration,
    totals: CounterTotals,
}

/// 隐藏滚动窗口的时间推进和累计值差分，调用者只需提交当前累计计数。
pub struct SignalTimeline {
    points: VecDeque<CounterPoint>,
    window: Duration,
}

impl SignalTimeline {
    pub fn new(started_at: Duration, initial: CounterTotals) -> Self {
        Self {
            points: VecDeque::from([CounterPoint {
                at: started_at,
                totals: initial,
            }]),
            window: Duration::from_secs(60),
        }
    }

    pub fn snapshot(&mut self, at: Duration, totals: CounterTotals) -> RollingSignals {
        // 单调时钟理论上不会回拨；若测试或平台异常传入更早时间，则沿用最后时间点，
        // 避免窗口顺序被破坏。
        let effective_at = self
            .points
            .back()
            .map(|point| at.max(point.at))
            .unwrap_or(at);
        let current = CounterPoint {
            at: effective_at,
            totals,
        };
        if self
            .points
            .back()
            .is_some_and(|point| point.at == effective_at)
        {
            self.points.pop_back();
        }
        self.points.push_back(current);

        let cutoff = effective_at.saturating_sub(self.window);
        // 保留截止点之前最近的一个累计值作为差分锚点。这样事件会随时间逐步滑出，
        // 而不是在第 60 秒把整个计数桶清零。
        while self.points.len() > 1 && self.points.get(1).is_some_and(|point| point.at <= cutoff) {
            self.points.pop_front();
        }

        let baseline = self
            .points
            .front()
            .map(|point| point.totals)
            .unwrap_or_default();
        RollingSignals {
            keys: as_u32(totals.keys.saturating_sub(baseline.keys)),
            backspaces: as_u32(totals.backspaces.saturating_sub(baseline.backspaces)),
            app_switches: as_u32(totals.app_switches.saturating_sub(baseline.app_switches)),
        }
    }
}

fn as_u32(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::{CounterTotals, SignalTimeline};
    use std::time::Duration;

    #[test]
    fn rolling_minute_slides_across_the_old_reset_boundary_without_dropping_to_zero() {
        let mut timeline = SignalTimeline::new(Duration::ZERO, CounterTotals::default());

        let before_boundary = timeline.snapshot(
            Duration::from_millis(59_900),
            CounterTotals {
                keys: 120,
                backspaces: 12,
                app_switches: 8,
            },
        );
        let after_boundary = timeline.snapshot(
            Duration::from_millis(60_100),
            CounterTotals {
                keys: 121,
                backspaces: 12,
                app_switches: 8,
            },
        );

        assert_eq!(before_boundary.keys, 120);
        assert_eq!(after_boundary.keys, 121);
        assert_eq!(after_boundary.app_switches, 8);
    }

    #[test]
    fn expired_events_leave_the_window_incrementally() {
        let mut timeline = SignalTimeline::new(Duration::ZERO, CounterTotals::default());
        timeline.snapshot(
            Duration::from_secs(10),
            CounterTotals {
                keys: 50,
                ..CounterTotals::default()
            },
        );
        timeline.snapshot(
            Duration::from_secs(30),
            CounterTotals {
                keys: 90,
                ..CounterTotals::default()
            },
        );

        let at_seventy = timeline.snapshot(
            Duration::from_secs(70),
            CounterTotals {
                keys: 100,
                ..CounterTotals::default()
            },
        );
        let at_ninety_one = timeline.snapshot(
            Duration::from_secs(91),
            CounterTotals {
                keys: 102,
                ..CounterTotals::default()
            },
        );

        assert_eq!(at_seventy.keys, 50);
        assert_eq!(at_ninety_one.keys, 12);
    }
}
