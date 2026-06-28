//! An owned, serializable picture of one engine frame.

use axiom_frame::{EngineFrame, FrameLifecycleState};
use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    SchemaVersion,
};

use crate::metric_report::MetricReport;
use crate::system_report::SystemReport;

/// The wire schema version of a [`FrameReport`]. Bumped on incompatible
/// layout changes; the major component gates compatibility. Bumped to 2.0 when
/// the deterministic frame timing was added to the layout.
const SCHEMA: SchemaVersion = SchemaVersion::new(2, 0);

/// The answerable picture of one engine frame: identity, lifecycle, viewport
/// size, skip status, the deterministic frame timing, and the ordered per-system
/// reports and telemetry metrics recovered from the frame contract.
///
/// Almost all fields are integers, and the one float — a metric value inside
/// [`MetricReport`] — is encoded through its fixed [`BinaryWriter::write_f32`]
/// bit pattern, so two reports built from equal frames serialize to identical
/// bytes. That byte-stability is the property that lets an agent diff "tick N vs
/// tick M" exactly. The type is `PartialEq` (not `Eq`) precisely because of that
/// float: value equality follows IEEE semantics even though the *bytes* are
/// stable.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameReport {
    engine_frame_index: u64,
    host_frame_sequence: u64,
    runtime_step_count: u32,
    skipped: bool,
    lifecycle: FrameLifecycleState,
    viewport_width: u32,
    viewport_height: u32,
    host_elapsed_nanos: u64,
    consumed_nanos: u64,
    retained_nanos: u64,
    fixed_step_nanos: u64,
    systems: Vec<SystemReport>,
    metrics: Vec<MetricReport>,
}

impl FrameReport {
    /// Project an engine frame into an owned report. Flattens every step
    /// summary's per-system detail into a single ordered list.
    pub fn from_frame(frame: &EngineFrame) -> Self {
        let systems = frame
            .runtime_step_summaries()
            .iter()
            .flat_map(|summary| summary.systems().iter().map(SystemReport::from_frame))
            .collect();
        let metrics = frame
            .runtime_step_summaries()
            .iter()
            .flat_map(|summary| summary.metrics().iter().map(MetricReport::from_metric))
            .collect();
        let timing = frame.timing();
        FrameReport {
            engine_frame_index: frame.engine_frame_index(),
            host_frame_sequence: frame.host_frame_sequence(),
            runtime_step_count: frame.runtime_step_count(),
            skipped: frame.is_skipped(),
            lifecycle: frame.lifecycle(),
            viewport_width: frame.viewport().logical_width(),
            viewport_height: frame.viewport().logical_height(),
            host_elapsed_nanos: timing.host_elapsed_nanos(),
            consumed_nanos: timing.consumed_nanos(),
            retained_nanos: timing.retained_nanos(),
            fixed_step_nanos: timing.fixed_step_nanos(),
            systems,
            metrics,
        }
    }

    /// The engine frame index (the layer-04 monotonic counter).
    pub const fn engine_frame_index(&self) -> u64 {
        self.engine_frame_index
    }

    /// The host frame sequence this frame was adapted from.
    pub const fn host_frame_sequence(&self) -> u64 {
        self.host_frame_sequence
    }

    /// The number of runtime steps executed for this frame.
    pub const fn runtime_step_count(&self) -> u32 {
        self.runtime_step_count
    }

    /// Whether the host frame was a lifecycle skip.
    pub const fn skipped(&self) -> bool {
        self.skipped
    }

    /// The frame-level lifecycle state.
    pub const fn lifecycle(&self) -> FrameLifecycleState {
        self.lifecycle
    }

    /// The frame viewport's logical width.
    pub const fn viewport_width(&self) -> u32 {
        self.viewport_width
    }

    /// The frame viewport's logical height.
    pub const fn viewport_height(&self) -> u32 {
        self.viewport_height
    }

    /// The host-supplied elapsed nanoseconds this frame was driven with — the
    /// deterministic timing the host injected as data (never a wall-clock read).
    pub const fn host_elapsed_nanos(&self) -> u64 {
        self.host_elapsed_nanos
    }

    /// The nanoseconds actually consumed by this frame's fixed runtime steps.
    pub const fn consumed_nanos(&self) -> u64 {
        self.consumed_nanos
    }

    /// The leftover nanoseconds carried forward to the next frame.
    pub const fn retained_nanos(&self) -> u64 {
        self.retained_nanos
    }

    /// The fixed runtime step duration in nanoseconds.
    pub const fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }

    /// The ordered per-system reports for this frame.
    pub fn systems(&self) -> &[SystemReport] {
        &self.systems
    }

    /// The telemetry metrics emitted across this frame's steps.
    pub fn metrics(&self) -> &[MetricReport] {
        &self.metrics
    }

    /// Serialize this report to bytes — the snapshot an external agent reads.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.write_to(&mut writer);
        writer.into_bytes()
    }

    /// Decode a report previously produced by [`Self::to_bytes`]. Fails with
    /// [`KernelErrorCode::SchemaVersionMismatch`] for an incompatible major
    /// version, or a binary error for truncated/invalid data.
    pub fn from_bytes(bytes: &[u8]) -> KernelResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        Self::read_from(&mut reader)
    }

    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        SCHEMA.write_to(writer);
        writer.write_u64(self.engine_frame_index);
        writer.write_u64(self.host_frame_sequence);
        writer.write_u32(self.runtime_step_count);
        writer.write_bool(self.skipped);
        writer.write_u8(lifecycle_to_u8(self.lifecycle));
        writer.write_u32(self.viewport_width);
        writer.write_u32(self.viewport_height);
        writer.write_u64(self.host_elapsed_nanos);
        writer.write_u64(self.consumed_nanos);
        writer.write_u64(self.retained_nanos);
        writer.write_u64(self.fixed_step_nanos);
        writer.write_u32(self.systems.len() as u32);
        self.systems
            .iter()
            .for_each(|system| system.write_to(writer));
        writer.write_u32(self.metrics.len() as u32);
        self.metrics
            .iter()
            .for_each(|metric| metric.write_to(writer));
    }

    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        // A fully branchless sequential decode: each field's read is chained
        // through `and_then`, so the first error short-circuits the rest and
        // the reader advances field-by-field exactly as `write_to` laid them
        // down. The schema guard becomes a `then_some(...).ok_or_else(...)`
        // expression threaded into the same chain.
        SchemaVersion::read_from(reader)
            .and_then(|version| {
                SCHEMA
                    .is_compatible_with(version)
                    .then_some(())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorScope::Binary,
                            KernelErrorCode::SchemaVersionMismatch,
                            "FrameReport schema major version is incompatible",
                        )
                    })
            })
            .and_then(|()| reader.read_u64())
            .and_then(|engine_frame_index| {
                reader.read_u64().and_then(|host_frame_sequence| {
                    reader.read_u32().and_then(|runtime_step_count| {
                        reader.read_bool().and_then(|skipped| {
                            reader
                                .read_u8()
                                .and_then(lifecycle_from_u8)
                                .and_then(|lifecycle| {
                                    reader.read_u32().and_then(|viewport_width| {
                                        reader.read_u32().and_then(|viewport_height| {
                                            reader.read_u64().and_then(|host_elapsed_nanos| {
                                            reader.read_u64().and_then(|consumed_nanos| {
                                            reader.read_u64().and_then(|retained_nanos| {
                                            reader.read_u64().and_then(|fixed_step_nanos| {
                                            reader.read_u32().and_then(|count| {
                                                (0..count)
                                                    .map(|_| SystemReport::read_from(reader))
                                                    .collect::<KernelResult<Vec<_>>>()
                                                    .and_then(|systems| {
                                                        reader.read_u32().and_then(|metric_count| {
                                                            (0..metric_count)
                                                                .map(|_| {
                                                                    MetricReport::read_from(reader)
                                                                })
                                                                .collect::<KernelResult<Vec<_>>>()
                                                                .map(|metrics| FrameReport {
                                                                    engine_frame_index,
                                                                    host_frame_sequence,
                                                                    runtime_step_count,
                                                                    skipped,
                                                                    lifecycle,
                                                                    viewport_width,
                                                                    viewport_height,
                                                                    host_elapsed_nanos,
                                                                    consumed_nanos,
                                                                    retained_nanos,
                                                                    fixed_step_nanos,
                                                                    systems,
                                                                    metrics,
                                                                })
                                                        })
                                                    })
                                            })
                                            })})})})
                                        })
                                    })
                                })
                        })
                    })
                })
            })
    }
}

/// The wire codes, in code order. `FrameLifecycleState` is a fieldless enum
/// whose default discriminants (`Active = 0` .. `ShutdownRequested = 3`) are
/// the wire codes, so the table doubles as both the encode discriminant set and
/// the decode lookup — keeping the two directions provably in lock-step.
const LIFECYCLE_VARIANTS: [FrameLifecycleState; 4] = [
    FrameLifecycleState::Active,
    FrameLifecycleState::Hidden,
    FrameLifecycleState::Suspended,
    FrameLifecycleState::ShutdownRequested,
];

/// Stable wire encoding of the frame lifecycle state. The fieldless enum's
/// discriminants are the wire codes, so the cast is the encoding.
fn lifecycle_to_u8(state: FrameLifecycleState) -> u8 {
    state as u8
}

/// Inverse of [`lifecycle_to_u8`]; rejects an unknown code by indexing the
/// variant table, whose miss is the only failure arm.
fn lifecycle_from_u8(code: u8) -> KernelResult<FrameLifecycleState> {
    LIFECYCLE_VARIANTS
        .get(code as usize)
        .copied()
        .ok_or_else(|| {
            KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::InvalidId,
                "unknown frame lifecycle code",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn report_with(lifecycle: FrameLifecycleState, skipped: bool) -> FrameReport {
        FrameReport {
            engine_frame_index: 3,
            host_frame_sequence: 5,
            runtime_step_count: 2,
            skipped,
            lifecycle,
            viewport_width: 320,
            viewport_height: 200,
            host_elapsed_nanos: 2_000,
            consumed_nanos: 2_000,
            retained_nanos: 0,
            fixed_step_nanos: 1_000,
            systems: Vec::new(),
            metrics: Vec::new(),
        }
    }

    #[test]
    fn from_active_frame_has_no_systems() {
        let frame = &fixtures::active_engine_frames(1)[0];
        let report = FrameReport::from_frame(frame);
        assert_eq!(report.runtime_step_count(), 1);
        assert!(!report.skipped());
        assert_eq!(report.lifecycle(), FrameLifecycleState::Active);
        assert_eq!(report.viewport_width(), 320);
        assert_eq!(report.viewport_height(), 200);
        assert!(report.systems().is_empty());
    }

    #[test]
    fn from_frame_captures_the_deterministic_timing() {
        // The single fixed-step active frame: one step consumed the fixed step
        // duration, nothing retained. The timing is host-injected data, so it is
        // exact and replay-stable — never a wall-clock read.
        let frame = &fixtures::active_engine_frames(1)[0];
        let report = FrameReport::from_frame(frame);
        let timing = frame.timing();
        assert_eq!(report.host_elapsed_nanos(), timing.host_elapsed_nanos());
        assert_eq!(report.consumed_nanos(), timing.consumed_nanos());
        assert_eq!(report.retained_nanos(), timing.retained_nanos());
        assert_eq!(report.fixed_step_nanos(), timing.fixed_step_nanos());
        // The fixed step is genuinely non-zero, so the accessor proves a value,
        // not a default.
        assert!(report.fixed_step_nanos() > 0);
    }

    #[test]
    fn equal_float_metric_frames_serialize_byte_identically() {
        // The failing fixture carries a float metric (`cube.angle_deg`). Two
        // reports built from independent-but-equal frames must serialize to the
        // exact same bytes — the float rides through its fixed bit pattern, so
        // the agent byte channel is replay-diffable even with floats present.
        let a = FrameReport::from_frame(&fixtures::failing_engine_frame());
        let b = FrameReport::from_frame(&fixtures::failing_engine_frame());
        assert_eq!(a, b);
        assert!(!a.metrics().is_empty(), "the fixture carries a float metric");
        assert_eq!(a.to_bytes(), b.to_bytes());
    }

    #[test]
    fn from_failing_frame_carries_the_system() {
        let frame = fixtures::failing_engine_frame();
        let report = FrameReport::from_frame(&frame);
        assert_eq!(report.systems().len(), 1);
        assert_eq!(report.systems()[0].name(), "fail");
        assert!(!report.systems()[0].succeeded());
        assert!(report.systems()[0].error_code().is_some());
        // The same step emitted a metric, which must ride along.
        assert_eq!(report.metrics().len(), 1);
        assert_eq!(report.metrics()[0].name(), "cube.angle_deg");
    }

    #[test]
    fn accessors_return_distinct_constructed_values() {
        let report = report_with(FrameLifecycleState::Active, false);
        assert_eq!(report.engine_frame_index(), 3);
        assert_eq!(report.host_frame_sequence(), 5);
        assert_eq!(report.runtime_step_count(), 2);
        assert_eq!(report.viewport_width(), 320);
        assert_eq!(report.viewport_height(), 200);
    }

    #[test]
    fn round_trips_each_lifecycle_and_skip_flag() {
        for lifecycle in [
            FrameLifecycleState::Active,
            FrameLifecycleState::Hidden,
            FrameLifecycleState::Suspended,
            FrameLifecycleState::ShutdownRequested,
        ] {
            for skipped in [false, true] {
                let report = report_with(lifecycle, skipped);
                let decoded = FrameReport::from_bytes(&report.to_bytes()).unwrap();
                assert_eq!(decoded, report);
            }
        }
    }

    #[test]
    fn round_trips_a_frame_with_systems() {
        let report = FrameReport::from_frame(&fixtures::failing_engine_frame());
        assert!(!report.systems().is_empty());
        let decoded = FrameReport::from_bytes(&report.to_bytes()).unwrap();
        assert_eq!(decoded, report);
    }

    #[test]
    fn incompatible_schema_major_is_rejected() {
        let mut writer = BinaryWriter::new();
        SchemaVersion::new(SCHEMA.major() + 1, 0).write_to(&mut writer);
        let bytes = writer.into_bytes();
        let err = FrameReport::from_bytes(&bytes).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::SchemaVersionMismatch);
    }

    #[test]
    fn truncation_at_every_prefix_is_err() {
        let report = FrameReport::from_frame(&fixtures::failing_engine_frame());
        let bytes = report.to_bytes();
        for len in 0..bytes.len() {
            assert!(
                FrameReport::from_bytes(&bytes[..len]).is_err(),
                "truncated decode at len {len} must fail"
            );
        }
    }

    #[test]
    fn invalid_lifecycle_code_in_buffer_is_rejected() {
        // A structurally valid buffer whose lifecycle byte is an unknown code
        // must fail decode (the read_u8 succeeds, so this exercises the `?`
        // propagation of lifecycle_from_u8's error at the call site, not just
        // the helper itself).
        let mut bytes = report_with(FrameLifecycleState::Active, false).to_bytes();
        // Layout: schema(4) + u64 + u64 + u32 + bool(1) => lifecycle byte at 25.
        let lifecycle_offset = 4 + 8 + 8 + 4 + 1;
        bytes[lifecycle_offset] = 99;
        assert_eq!(
            FrameReport::from_bytes(&bytes).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }

    #[test]
    fn lifecycle_codes_round_trip_and_reject_unknown() {
        assert_eq!(lifecycle_to_u8(FrameLifecycleState::Active), 0);
        assert_eq!(lifecycle_to_u8(FrameLifecycleState::Hidden), 1);
        assert_eq!(lifecycle_to_u8(FrameLifecycleState::Suspended), 2);
        assert_eq!(lifecycle_to_u8(FrameLifecycleState::ShutdownRequested), 3);
        assert_eq!(lifecycle_from_u8(0).unwrap(), FrameLifecycleState::Active);
        assert_eq!(lifecycle_from_u8(1).unwrap(), FrameLifecycleState::Hidden);
        assert_eq!(
            lifecycle_from_u8(2).unwrap(),
            FrameLifecycleState::Suspended
        );
        assert_eq!(
            lifecycle_from_u8(3).unwrap(),
            FrameLifecycleState::ShutdownRequested
        );
        assert_eq!(
            lifecycle_from_u8(4).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }
}
