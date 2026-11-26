use std::time::{Duration, Instant};

use crate::FrameStats;

const HISTORY: usize = 120;
const ADAPTIVE_WINDOW: usize = 30;
const RESOLUTION_PERCENT: [u32; 3] = [100, 83, 67];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct FrameTiming {
    pub frame_number: u64,
    /// Wall-time interval between presented frames, with suspended intervals excluded.
    pub frame_interval_ns: u64,
    pub prepare_ns: u64,
    /// CPU time spent encoding, submitting, presenting or reading back the frame.
    pub render_ns: u64,
    pub triangles: u64,
    pub chunks_drawn: u32,
    pub chunks_culled: u32,
    pub instances_drawn: u32,
    pub resolution_percent: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct FramePercentiles {
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub samples: u32,
}

pub(crate) struct FrameTelemetry {
    history: [FrameTiming; HISTORY],
    count: usize,
    next: usize,
    serial: u64,
    last_presented: Option<Instant>,
    resolution_tier: usize,
    adaptive_sample_count: usize,
    consecutive_fast: usize,
}

impl Default for FrameTelemetry {
    fn default() -> Self {
        Self {
            history: [FrameTiming::default(); HISTORY],
            count: 0,
            next: 0,
            serial: 0,
            last_presented: None,
            resolution_tier: 0,
            adaptive_sample_count: 0,
            consecutive_fast: 0,
        }
    }
}

impl FrameTelemetry {
    /// Mark a known suspension or clock reset so the next presentation starts a new interval.
    pub fn interrupt(&mut self) {
        self.last_presented = None;
        self.adaptive_sample_count = 0;
        self.consecutive_fast = 0;
    }

    pub fn latest(&self) -> Option<FrameTiming> {
        (self.count != 0).then(|| self.history[(self.next + HISTORY - 1) % HISTORY])
    }

    pub fn resolution_percent(&self) -> u32 {
        RESOLUTION_PERCENT[self.resolution_tier]
    }

    pub fn record(
        &mut self,
        started: Instant,
        preparation: Duration,
        render: Duration,
        stats: FrameStats,
        adaptive: bool,
    ) {
        let interval = self.last_presented.replace(started).map_or(0, |previous| {
            started.saturating_duration_since(previous).as_nanos() as u64
        });
        self.serial += 1;
        self.history[self.next] = FrameTiming {
            frame_number: self.serial,
            frame_interval_ns: interval,
            prepare_ns: preparation.as_nanos() as u64,
            render_ns: render.as_nanos() as u64,
            triangles: stats.triangles_submitted,
            chunks_drawn: stats.chunks_drawn,
            chunks_culled: stats.chunks_culled,
            instances_drawn: stats.instances_drawn,
            resolution_percent: if adaptive {
                self.resolution_percent()
            } else {
                100
            },
        };
        self.next = (self.next + 1) % HISTORY;
        self.count = (self.count + 1).min(HISTORY);
        if adaptive {
            self.adjust(interval);
        }
    }

    fn adjust(&mut self, interval: u64) {
        if interval == 0 {
            self.adaptive_sample_count = 0;
            self.consecutive_fast = 0;
            return;
        }
        self.adaptive_sample_count = (self.adaptive_sample_count + 1).min(ADAPTIVE_WINDOW);
        if self.adaptive_sample_count < ADAPTIVE_WINDOW {
            return;
        }
        let p95 = self.percentiles(ADAPTIVE_WINDOW).p95_ns;
        if p95 > 17_000_000 && self.resolution_tier + 1 < RESOLUTION_PERCENT.len() {
            self.resolution_tier += 1;
            self.adaptive_sample_count = 0;
            self.consecutive_fast = 0;
        } else if p95 < 12_000_000 && self.resolution_tier > 0 {
            self.consecutive_fast += 1;
            if self.consecutive_fast >= 90 {
                self.resolution_tier -= 1;
                self.adaptive_sample_count = 0;
                self.consecutive_fast = 0;
            }
        } else {
            self.consecutive_fast = 0;
        }
    }

    pub fn percentiles(&self, limit: usize) -> FramePercentiles {
        let mut intervals = [0; HISTORY];
        let mut len = 0;
        for offset in 1..=self.count.min(limit) {
            let elapsed = self.history[(self.next + HISTORY - offset) % HISTORY].frame_interval_ns;
            if elapsed > 0 {
                intervals[len] = elapsed;
                len += 1;
            }
        }
        if len == 0 {
            return FramePercentiles::default();
        }
        intervals[..len].sort_unstable();
        let at = |percent: usize| intervals[(len * percent).div_ceil(100) - 1];
        FramePercentiles {
            p50_ns: at(50),
            p95_ns: at(95),
            p99_ns: at(99),
            samples: len as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_resolution_has_distinct_step_down_and_recovery_windows() {
        let mut history = FrameTelemetry::default();
        let mut at = Instant::now();
        let mut record = |history: &mut FrameTelemetry, ms| {
            at += Duration::from_millis(ms);
            history.record(
                at,
                Duration::ZERO,
                Duration::ZERO,
                FrameStats::default(),
                true,
            );
        };
        for _ in 0..31 {
            record(&mut history, 20);
        }
        assert_eq!(history.resolution_percent(), 83);
        assert_eq!(history.percentiles(HISTORY).p95_ns, 20_000_000);
        for _ in 0..100 {
            record(&mut history, 10);
        }
        assert_eq!(history.resolution_percent(), 83);
        for _ in 0..21 {
            record(&mut history, 10);
        }
        assert_eq!(history.resolution_percent(), 100);
    }

    #[test]
    fn explicit_interrupt_excludes_suspend_gap_but_keeps_long_stutters() {
        let mut history = FrameTelemetry::default();
        let start = Instant::now();
        let record_at = |history: &mut FrameTelemetry, at| {
            history.record(
                at,
                Duration::ZERO,
                Duration::ZERO,
                FrameStats::default(),
                false,
            );
        };

        record_at(&mut history, start);
        record_at(&mut history, start + Duration::from_millis(300));
        assert_eq!(history.latest().unwrap().frame_interval_ns, 300_000_000);

        history.interrupt();
        record_at(&mut history, start + Duration::from_secs(30));
        assert_eq!(history.latest().unwrap().frame_interval_ns, 0);
        record_at(
            &mut history,
            start + Duration::from_secs(30) + Duration::from_millis(400),
        );
        assert_eq!(history.latest().unwrap().frame_interval_ns, 400_000_000);
    }
}
