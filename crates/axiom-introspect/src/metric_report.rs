//! One telemetry metric a step's systems emitted, in owned serializable form.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    MetricKind, MetricValue, TelemetryMetric,
};

/// An owned, serializable telemetry sample recovered from a frame.
///
/// The kernel's [`TelemetryMetric`] borrows a `'static` name and is `Copy`;
/// introspection needs an owned, serializable form so a sample can outlive the
/// frame and cross the byte channel to an agent. The kind collapses to
/// `is_counter` (counter vs gauge); the value keeps the kernel's
/// integer-or-float [`MetricValue`]; the optional tick is the deterministic
/// sample time.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricReport {
    name: String,
    is_counter: bool,
    value: MetricValue,
    tick: Option<u64>,
}

impl MetricReport {
    /// Project a kernel telemetry metric into an owned report.
    pub fn from_metric(metric: &TelemetryMetric) -> Self {
        MetricReport {
            name: metric.name().to_string(),
            is_counter: metric.kind().raw() == MetricKind::Counter.raw(),
            value: metric.value(),
            tick: metric.tick().map(|t| t.raw()),
        }
    }

    /// The metric name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the metric is a counter (else a gauge).
    pub const fn is_counter(&self) -> bool {
        self.is_counter
    }

    /// The metric value (integer or float).
    pub const fn value(&self) -> MetricValue {
        self.value
    }

    /// The deterministic tick the sample was taken at, if any.
    pub const fn tick(&self) -> Option<u64> {
        self.tick
    }

    /// Append this report to a writer. The value is a `u8` tag (`0` integer,
    /// `1` float) followed by the payload; an `i64` integer is reinterpreted
    /// through its `u64` bit pattern (the writer has no signed-64 primitive).
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_byte_slice(self.name.as_bytes());
        writer.write_bool(self.is_counter);
        match self.value {
            MetricValue::Integer(i) => {
                writer.write_u8(0);
                writer.write_u64(i as u64);
            }
            MetricValue::Float(f) => {
                writer.write_u8(1);
                writer.write_f32(f);
            }
        }
        writer.write_bool(self.tick.is_some());
        self.tick.iter().for_each(|t| writer.write_u64(*t));
    }

    /// Read a report previously written with [`Self::write_to`]. Rejects an
    /// unknown value tag.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        // Branchless sequential decode: each field threads through `and_then`,
        // so the first failure short-circuits and the reader advances exactly
        // as `write_to` laid the fields down.
        reader.read_byte_slice().and_then(|name_bytes| {
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            reader.read_bool().and_then(|is_counter| {
                // Value-tag dispatch on a `u8` (not an enum discriminant):
                // `.then` runs only the selected read, so the reader advances
                // by exactly the bytes the chosen branch consumes. An
                // unrecognized tag falls through both guards to the invalid-tag
                // error.
                reader
                    .read_u8()
                    .and_then(|tag| {
                        (tag == 0)
                            .then(|| reader.read_u64().map(|i| MetricValue::integer(i as i64)))
                            .or_else(|| (tag == 1).then(|| reader.read_f32().map(MetricValue::float)))
                            .unwrap_or_else(|| {
                                Err(KernelError::new(
                                    KernelErrorScope::Binary,
                                    KernelErrorCode::InvalidId,
                                    "unknown metric value tag",
                                ))
                            })
                    })
                    .and_then(|value| {
                        // Optional tick: the presence-tag pattern, with
                        // `transpose` folded into the chain to leave no `?`.
                        reader.read_bool().and_then(|has_tick| {
                            has_tick
                                .then(|| reader.read_u64())
                                .transpose()
                                .map(|tick| MetricReport {
                                    name,
                                    is_counter,
                                    value,
                                    tick,
                                })
                        })
                    })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Tick;

    fn round_trip(report: &MetricReport) -> MetricReport {
        let mut w = BinaryWriter::new();
        report.write_to(&mut w);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        MetricReport::read_from(&mut r).unwrap()
    }

    #[test]
    fn counter_with_tick_round_trips() {
        let report = MetricReport::from_metric(&TelemetryMetric::counter(
            "frame.draws",
            3,
            Some(Tick::new(7)),
        ));
        assert_eq!(report.name(), "frame.draws");
        assert!(report.is_counter());
        assert_eq!(report.value(), MetricValue::integer(3));
        assert_eq!(report.tick(), Some(7));
        assert_eq!(round_trip(&report), report);
    }

    #[test]
    fn gauge_without_tick_round_trips() {
        let report = MetricReport::from_metric(&TelemetryMetric::gauge(
            "cube.angle_deg",
            MetricValue::float(2.5),
            None,
        ));
        assert_eq!(report.name(), "cube.angle_deg");
        assert!(!report.is_counter());
        assert_eq!(report.value(), MetricValue::float(2.5));
        assert_eq!(report.tick(), None);
        assert_eq!(round_trip(&report), report);
    }

    #[test]
    fn negative_integer_round_trips_through_u64_bits() {
        let report = MetricReport::from_metric(&TelemetryMetric::counter("delta", -42, None));
        assert_eq!(round_trip(&report).value(), MetricValue::integer(-42));
    }

    #[test]
    fn truncation_at_every_prefix_is_err() {
        let report =
            MetricReport::from_metric(&TelemetryMetric::counter("c", 1, Some(Tick::new(1))));
        let mut w = BinaryWriter::new();
        report.write_to(&mut w);
        let bytes = w.into_bytes();
        for len in 0..bytes.len() {
            let mut r = BinaryReader::new(&bytes[..len]);
            assert!(
                MetricReport::read_from(&mut r).is_err(),
                "len {len} must fail"
            );
        }
    }

    #[test]
    fn unknown_value_tag_is_rejected() {
        // name "" (len 0) + is_counter(1 byte) + tag 9.
        let mut w = BinaryWriter::new();
        w.write_byte_slice(b"");
        w.write_bool(true);
        w.write_u8(9);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(
            MetricReport::read_from(&mut r).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }
}
