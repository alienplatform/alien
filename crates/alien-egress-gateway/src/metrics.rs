//! Fixed-size, unlabeled DNS and connection latency histograms.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

// Exclusive internal bins are accumulated only when rendering Prometheus buckets.
const BOUNDS_MICROS: [u64; 8] = [
    1_000, 5_000, 10_000, 50_000, 100_000, 500_000, 1_000_000, 5_000_000,
];

#[derive(Default)]
pub struct Histogram {
    bins: [AtomicU64; 9],
    sum_micros: AtomicU64,
}

pub struct Timer<'a> {
    histogram: &'a Histogram,
    start: Instant,
}

impl Histogram {
    pub fn start(&self) -> Timer<'_> {
        Timer {
            histogram: self,
            start: Instant::now(),
        }
    }

    fn observe(&self, duration: Duration) {
        let micros = duration.as_micros().min(u64::MAX as u128) as u64;
        let index = BOUNDS_MICROS
            .iter()
            .position(|bound| micros <= *bound)
            .unwrap_or(8);
        self.bins[index].fetch_add(1, Ordering::Relaxed);
        self.sum_micros.fetch_add(micros, Ordering::Relaxed);
    }

    pub fn render(&self, name: &str, output: &mut String) {
        output.push_str(&format!("# TYPE {name} histogram\n"));
        let mut count = 0;
        for (index, bin) in self.bins.iter().enumerate() {
            count += bin.load(Ordering::Relaxed);
            let bound = BOUNDS_MICROS.get(index).map_or_else(
                || "+Inf".to_string(),
                |micros| (*micros as f64 / 1_000_000.0).to_string(),
            );
            output.push_str(&format!("{name}_bucket{{le=\"{bound}\"}} {count}\n"));
        }
        output.push_str(&format!(
            "{name}_count {count}\n{name}_sum {}\n",
            self.sum_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0
        ));
    }
}

impl Drop for Timer<'_> {
    fn drop(&mut self) {
        // Includes failed and cancelled operations, not just successful requests.
        self.histogram.observe(self.start.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_buckets_include_boundaries_and_overflow() {
        let histogram = Histogram::default();
        for micros in [0, 1_000, 1_001, 5_000_001] {
            histogram.observe(Duration::from_micros(micros));
        }
        let mut output = String::new();
        histogram.render("duration_seconds", &mut output);
        assert!(output.contains("duration_seconds_bucket{le=\"0.001\"} 2\n"));
        assert!(output.contains("duration_seconds_bucket{le=\"0.005\"} 3\n"));
        assert!(output.contains("duration_seconds_bucket{le=\"+Inf\"} 4\n"));
        assert!(output.contains("duration_seconds_count 4\n"));
        assert!(output.contains("duration_seconds_sum 5.002002\n"));
    }
}
