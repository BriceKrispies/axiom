//! A deterministic in-memory [`TelemetrySink`] that retains samples.

use crate::telemetry_metric::TelemetryMetric;
use crate::telemetry_sink::TelemetrySink;

/// A [`TelemetrySink`] that stores received samples in order.
///
/// No I/O is performed: samples are appended to a `Vec` in arrival order, so a
/// replay produces an identical, assertable telemetry stream. A small
/// [`Self::counter_total`] helper sums integer counters of a given name, which
/// is the most common deterministic check callers want.
#[derive(Debug, Clone, Default)]
pub struct InMemoryTelemetrySink {
    metrics: Vec<TelemetryMetric>,
}

impl InMemoryTelemetrySink {
    /// Create an empty sink.
    pub fn new() -> Self {
        InMemoryTelemetrySink {
            metrics: Vec::new(),
        }
    }

    /// The captured samples, in arrival order.
    pub fn metrics(&self) -> &[TelemetryMetric] {
        &self.metrics
    }

    /// The number of captured samples.
    pub fn len(&self) -> usize {
        self.metrics.len()
    }

    /// Whether no samples have been captured.
    pub fn is_empty(&self) -> bool {
        self.metrics.is_empty()
    }

    /// Sum of the integer values of all counter samples named `name`.
    pub fn counter_total(&self, name: &str) -> i64 {
        self.metrics
            .iter()
            .filter(|m| m.name() == name)
            .filter_map(|m| m.value().as_integer())
            .sum()
    }

    /// Discard all captured samples.
    pub fn clear(&mut self) {
        self.metrics.clear();
    }
}

impl TelemetrySink for InMemoryTelemetrySink {
    fn record(&mut self, metric: TelemetryMetric) {
        self.metrics.push(metric);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_value::MetricValue;
    use crate::tick::Tick;

    #[test]
    fn new_and_default_sinks_are_empty() {
        assert!(InMemoryTelemetrySink::new().is_empty());
        assert!(InMemoryTelemetrySink::default().is_empty());
        assert_eq!(InMemoryTelemetrySink::new().len(), 0);
    }

    #[test]
    fn samples_are_captured_in_order() {
        let mut sink = InMemoryTelemetrySink::new();
        sink.record(TelemetryMetric::counter("frames", 1, Some(Tick::new(1))));
        sink.record(TelemetryMetric::gauge(
            "load",
            MetricValue::float(0.25),
            None,
        ));
        assert_eq!(sink.len(), 2);
        assert_eq!(sink.metrics()[0].name(), "frames");
        assert_eq!(sink.metrics()[1].name(), "load");
    }

    #[test]
    fn counter_total_sums_only_matching_counters() {
        let mut sink = InMemoryTelemetrySink::new();
        sink.record(TelemetryMetric::counter("hits", 2, None));
        sink.record(TelemetryMetric::counter("hits", 3, None));
        sink.record(TelemetryMetric::counter("misses", 5, None));
        assert_eq!(sink.counter_total("hits"), 5);
        assert_eq!(sink.counter_total("misses"), 5);
        assert_eq!(sink.counter_total("absent"), 0);
    }

    #[test]
    fn capture_is_deterministic_across_runs() {
        let build = || {
            let mut sink = InMemoryTelemetrySink::new();
            sink.record(TelemetryMetric::counter("c", 7, None));
            sink
        };
        assert_eq!(build().metrics(), build().metrics());
    }

    #[test]
    fn populated_sink_is_not_empty() {
        let mut sink = InMemoryTelemetrySink::new();
        sink.record(TelemetryMetric::counter("c", 1, None));
        assert!(!sink.is_empty());
        assert_eq!(sink.len(), 1);
    }

    #[test]
    fn clear_empties_the_sink() {
        let mut sink = InMemoryTelemetrySink::new();
        sink.record(TelemetryMetric::counter("c", 1, None));
        sink.clear();
        assert!(sink.is_empty());
    }
}
