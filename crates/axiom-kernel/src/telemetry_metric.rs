//! A single structured telemetry sample — data, never an external send.

use crate::metric_kind::MetricKind;
use crate::metric_value::MetricValue;
use crate::tick::Tick;

/// One telemetry sample: a named metric of a given kind, value and optional tick.
///
/// Like log records, metrics are pure data. The kernel records them; it never
/// exports them anywhere. The [`Self::counter`] and [`Self::gauge`]
/// constructors pair each kind with its conventional value type (counters carry
/// an integer, gauges carry any [`MetricValue`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TelemetryMetric {
    name: &'static str,
    kind: MetricKind,
    value: MetricValue,
    tick: Option<Tick>,
}

impl TelemetryMetric {
    /// A counter sample: an accumulating integer total at an optional tick.
    pub const fn counter(name: &'static str, value: i64, tick: Option<Tick>) -> Self {
        TelemetryMetric {
            name,
            kind: MetricKind::Counter,
            value: MetricValue::integer(value),
            tick,
        }
    }

    /// A gauge sample: a point-in-time value at an optional tick.
    pub const fn gauge(name: &'static str, value: MetricValue, tick: Option<Tick>) -> Self {
        TelemetryMetric {
            name,
            kind: MetricKind::Gauge,
            value,
            tick,
        }
    }

    /// The metric name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The metric kind.
    pub const fn kind(&self) -> MetricKind {
        self.kind
    }

    /// The metric value.
    pub const fn value(&self) -> MetricValue {
        self.value
    }

    /// The tick this sample was taken at, if any.
    pub const fn tick(&self) -> Option<Tick> {
        self.tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_has_counter_kind_and_integer_value() {
        let m = TelemetryMetric::counter("frames", 60, Some(Tick::new(60)));
        assert_eq!(m.name(), "frames");
        assert_eq!(m.kind(), MetricKind::Counter);
        assert_eq!(m.value(), MetricValue::integer(60));
        assert_eq!(m.tick(), Some(Tick::new(60)));
    }

    #[test]
    fn gauge_carries_given_value_and_kind() {
        let m = TelemetryMetric::gauge("load", MetricValue::float(0.5), None);
        assert_eq!(m.kind(), MetricKind::Gauge);
        assert_eq!(m.value(), MetricValue::float(0.5));
        assert!(m.tick().is_none());
    }

    #[test]
    fn identical_inputs_produce_equal_metrics() {
        let a = TelemetryMetric::counter("c", 1, Some(Tick::new(1)));
        let b = TelemetryMetric::counter("c", 1, Some(Tick::new(1)));
        assert_eq!(a, b);
    }
}
