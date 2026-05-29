//! The trait through which telemetry samples are recorded.

use crate::telemetry_metric::TelemetryMetric;

/// A destination that receives [`TelemetryMetric`] samples.
///
/// As with logging, telemetry is recorded, never exported by the kernel:
/// samples are handed to a sink as structured data. The kernel ships one
/// deterministic in-memory implementation
/// ([`crate::in_memory_telemetry_sink::InMemoryTelemetrySink`]); higher layers
/// may add sinks that forward samples to an exporter.
pub trait TelemetrySink {
    /// Receive a sample. Implementations must not perform ambient I/O.
    fn record(&mut self, metric: TelemetryMetric);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal sink proving the trait contract.
    #[derive(Default)]
    struct SummingSink {
        total: i64,
    }

    impl TelemetrySink for SummingSink {
        fn record(&mut self, metric: TelemetryMetric) {
            if let Some(v) = metric.value().as_integer() {
                self.total += v;
            }
        }
    }

    #[test]
    fn samples_are_delivered_to_the_sink() {
        let mut sink = SummingSink::default();
        sink.record(TelemetryMetric::counter("a", 3, None));
        sink.record(TelemetryMetric::counter("a", 4, None));
        assert_eq!(sink.total, 7);
    }
}
