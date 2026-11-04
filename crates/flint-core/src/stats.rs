use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RATE_WINDOW: Duration = Duration::from_secs(1);
const KEEP_WINDOW: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default)]
pub struct StatsSnapshot {
    pub frames: u64,
    pub dropped: u64,
    pub bytes: u64,
    pub actual_fps: f64,
    pub bitrate_mbps: f64,
    pub decode_ms: f32,
    pub drop_percent: f64,
}

#[derive(Default)]
struct Window {
    arrivals: VecDeque<Instant>,
    byte_samples: VecDeque<(Instant, u64)>,
    decode_samples: VecDeque<f32>,
}

pub struct Stats {
    frames: AtomicU64,
    dropped: AtomicU64,
    bytes: AtomicU64,
    window: Mutex<Window>,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

impl Stats {
    pub fn new() -> Self {
        Self {
            frames: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            window: Mutex::new(Window::default()),
        }
    }

    pub fn record_frame(&self, at: Instant, decode_ms: f32) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.arrivals.push_back(at);
        window
            .decode_samples
            .push_back(decode_ms.clamp(0.0, 10_000.0));
        if window.decode_samples.len() > 64 {
            window.decode_samples.pop_front();
        }
        while window
            .arrivals
            .front()
            .map(|oldest| at.saturating_duration_since(*oldest) > KEEP_WINDOW)
            .unwrap_or(false)
        {
            window.arrivals.pop_front();
        }
    }

    pub fn record_bytes(&self, at: Instant, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.byte_samples.push_back((at, bytes));
        while window
            .byte_samples
            .front()
            .map(|(oldest, _)| at.saturating_duration_since(*oldest) > KEEP_WINDOW)
            .unwrap_or(false)
        {
            window.byte_samples.pop_front();
        }
    }

    pub fn record_drop(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reset(&self) {
        self.frames.store(0, Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *window = Window::default();
    }

    pub fn snapshot(&self, now: Instant) -> StatsSnapshot {
        let frames = self.frames.load(Ordering::Relaxed);
        let dropped = self.dropped.load(Ordering::Relaxed);
        let bytes = self.bytes.load(Ordering::Relaxed);
        let window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let recent: Vec<&Instant> = window
            .arrivals
            .iter()
            .filter(|at| now.saturating_duration_since(**at) <= RATE_WINDOW)
            .collect();
        let actual_fps = match (recent.first(), recent.last()) {
            (Some(first), Some(last)) if recent.len() >= 2 => {
                let span = last.saturating_duration_since(**first).as_secs_f64();
                if span > 0.02 {
                    (recent.len() - 1) as f64 / span
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };

        let recent_bytes: u64 = window
            .byte_samples
            .iter()
            .filter(|(at, _)| now.saturating_duration_since(*at) <= RATE_WINDOW)
            .map(|(_, count)| *count)
            .sum();
        let bitrate_mbps = recent_bytes as f64 * 8.0 / 1_000_000.0;

        let decode_ms = if window.decode_samples.is_empty() {
            0.0
        } else {
            window.decode_samples.iter().sum::<f32>() / window.decode_samples.len() as f32
        };

        let drop_percent = if frames == 0 {
            0.0
        } else {
            (dropped as f64 * 100.0 / frames as f64).min(100.0)
        };

        StatsSnapshot {
            frames,
            dropped,
            bytes,
            actual_fps,
            bitrate_mbps,
            decode_ms,
            drop_percent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_fps_over_one_second() {
        let stats = Stats::new();
        let start = Instant::now();
        for index in 0..31 {
            stats.record_frame(start + Duration::from_millis(index * 33), 2.0);
        }
        let snapshot = stats.snapshot(start + Duration::from_secs(1));
        assert!(
            (snapshot.actual_fps - 30.0).abs() < 1.5,
            "{}",
            snapshot.actual_fps
        );
        assert_eq!(snapshot.frames, 31);
    }

    #[test]
    fn sums_bitrate_in_window() {
        let stats = Stats::new();
        let start = Instant::now();
        stats.record_bytes(start, 1_000_000);
        stats.record_bytes(start + Duration::from_millis(500), 500_000);
        stats.record_bytes(start + Duration::from_millis(1_500), 9_000_000);
        let snapshot = stats.snapshot(start + Duration::from_millis(1_600));
        assert!(
            (snapshot.bitrate_mbps - 72.0).abs() < 0.001,
            "{}",
            snapshot.bitrate_mbps
        );
    }

    #[test]
    fn reports_drop_ratio_and_latency() {
        let stats = Stats::new();
        let now = Instant::now();
        stats.record_frame(now, 4.0);
        stats.record_frame(now, 8.0);
        stats.record_drop();
        let snapshot = stats.snapshot(now);
        assert_eq!(snapshot.frames, 2);
        assert_eq!(snapshot.dropped, 1);
        assert!((snapshot.decode_ms - 6.0).abs() < 0.001);
        assert!((snapshot.drop_percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn drop_ratio_uses_published_frames_as_total() {
        let stats = Stats::new();
        let now = Instant::now();
        for _ in 0..100 {
            stats.record_frame(now, 1.0);
        }
        for _ in 0..70 {
            stats.record_drop();
        }
        let snapshot = stats.snapshot(now);
        assert_eq!(snapshot.frames, 100);
        assert_eq!(snapshot.dropped, 70);
        assert!((snapshot.drop_percent - 70.0).abs() < 0.001);
    }
}
