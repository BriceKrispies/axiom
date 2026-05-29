//! The kind of a telemetry metric.

/// The kind of a [`crate::telemetry_metric::TelemetryMetric`].
///
/// - `Counter`: a monotonically accumulating total.
/// - `Gauge`: a point-in-time value that may rise or fall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum MetricKind {
    Counter = 0,
    Gauge = 1,
}

impl MetricKind {
    /// The stable numeric discriminant of this kind.
    pub const fn raw(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(MetricKind::Counter.raw(), 0);
        assert_eq!(MetricKind::Gauge.raw(), 1);
    }

    #[test]
    fn kinds_are_distinct() {
        assert_ne!(MetricKind::Counter, MetricKind::Gauge);
    }
}
