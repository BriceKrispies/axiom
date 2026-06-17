//! The value carried by a telemetry metric.

/// A telemetry value: either an exact integer or a 32-bit float.
///
/// Integers suit counters; floats suit gauges of fractional quantities. Only
/// `PartialEq` is derived (not `Eq`) because `f32` is not totally ordered; the
/// kernel never relies on float equality for control flow, only for value
/// inspection.
///
/// Represented as a tagged struct rather than an enum so payload extraction is
/// branchless: `kind` selects which field is live, and the unused field is
/// always its default (`0` / `0.0`). Derived `PartialEq` compares every field,
/// which is correct precisely because the inactive field is fixed at its
/// default for a given `kind`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricValue {
    kind: u8,
    int: i64,
    float: f32,
}

impl MetricValue {
    const KIND_INTEGER: u8 = 0;
    const KIND_FLOAT: u8 = 1;

    /// Construct an integer value.
    pub const fn integer(value: i64) -> Self {
        MetricValue {
            kind: Self::KIND_INTEGER,
            int: value,
            float: 0.0,
        }
    }

    /// Construct a float value.
    pub const fn float(value: f32) -> Self {
        MetricValue {
            kind: Self::KIND_FLOAT,
            int: 0,
            float: value,
        }
    }

    /// The integer payload, if this is an `Integer`.
    pub fn as_integer(self) -> Option<i64> {
        (self.kind == Self::KIND_INTEGER).then_some(self.int)
    }

    /// The float payload, if this is a `Float`.
    pub fn as_float(self) -> Option<f32> {
        (self.kind == Self::KIND_FLOAT).then_some(self.float)
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
