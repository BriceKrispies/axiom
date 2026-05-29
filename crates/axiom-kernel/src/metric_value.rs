//! The value carried by a telemetry metric.

/// A telemetry value: either an exact integer or a 32-bit float.
///
/// Integers suit counters; floats suit gauges of fractional quantities. Only
/// `PartialEq` is derived (not `Eq`) because `f32` is not totally ordered; the
/// kernel never relies on float equality for control flow, only for value
/// inspection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricValue {
    Integer(i64),
    Float(f32),
}

impl MetricValue {
    /// Construct an integer value.
    pub const fn integer(value: i64) -> Self {
        MetricValue::Integer(value)
    }

    /// Construct a float value.
    pub const fn float(value: f32) -> Self {
        MetricValue::Float(value)
    }

    /// The integer payload, if this is an `Integer`.
    pub const fn as_integer(self) -> Option<i64> {
        match self {
            MetricValue::Integer(v) => Some(v),
            MetricValue::Float(_) => None,
        }
    }

    /// The float payload, if this is a `Float`.
    pub const fn as_float(self) -> Option<f32> {
        match self {
            MetricValue::Float(v) => Some(v),
            MetricValue::Integer(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_value_inspection() {
        let v = MetricValue::integer(-5);
        assert_eq!(v.as_integer(), Some(-5));
        assert_eq!(v.as_float(), None);
    }

    #[test]
    fn float_value_inspection() {
        let v = MetricValue::float(1.5);
        assert_eq!(v.as_float(), Some(1.5));
        assert_eq!(v.as_integer(), None);
    }

    #[test]
    fn equality_is_structural() {
        assert_eq!(MetricValue::integer(3), MetricValue::integer(3));
        assert_ne!(MetricValue::integer(3), MetricValue::float(3.0));
    }
}
