//! An owned, serializable picture of one engine frame.

use axiom_frame::{EngineFrame, FrameLifecycleState};
use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    SchemaVersion,
};

use crate::metric_report::MetricReport;
use crate::system_report::SystemReport;

/// The wire schema version of a [`FrameReport`]. Bumped on incompatible
/// layout changes; the major component gates compatibility.
const SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// The answerable picture of one engine frame: identity, lifecycle, viewport
/// size, skip status, and the ordered per-system reports recovered from the
/// frame contract.
///
/// Plain, owned data with no floating-point fields, so two reports built from
/// equal frames are equal and serialize to identical bytes — the property that
/// lets an agent diff "tick N vs tick M" exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameReport {
    engine_frame_index: u64,
    host_frame_sequence: u64,
    runtime_step_count: u32,
    skipped: bool,
    lifecycle: FrameLifecycleState,
    viewport_width: u32,
    viewport_height: u32,
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
        FrameReport {
            engine_frame_index: frame.engine_frame_index(),
            host_frame_sequence: frame.host_frame_sequence(),
            runtime_step_count: frame.runtime_step_count(),
            skipped: frame.is_skipped(),
            lifecycle: frame.lifecycle(),
            viewport_width: frame.viewport().logical_width(),
            viewport_height: frame.viewport().logical_height(),
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

    fn write_to(&self, writer: &mut BinaryWriter) {
        SCHEMA.write_to(writer);
        writer.write_u64(self.engine_frame_index);
        writer.write_u64(self.host_frame_sequence);
        writer.write_u32(self.runtime_step_count);
        writer.write_bool(self.skipped);
        writer.write_u8(lifecycle_to_u8(self.lifecycle));
        writer.write_u32(self.viewport_width);
        writer.write_u32(self.viewport_height);
        writer.write_u32(self.systems.len() as u32);
        for system in &self.systems {
            system.write_to(writer);
        }
        writer.write_u32(self.metrics.len() as u32);
        for metric in &self.metrics {
            metric.write_to(writer);
        }
    }

    fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let version = SchemaVersion::read_from(reader)?;
        if !SCHEMA.is_compatible_with(version) {
            return Err(KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::SchemaVersionMismatch,
                "FrameReport schema major version is incompatible",
            ));
        }
        let engine_frame_index = reader.read_u64()?;
        let host_frame_sequence = reader.read_u64()?;
        let runtime_step_count = reader.read_u32()?;
        let skipped = reader.read_bool()?;
        let lifecycle = lifecycle_from_u8(reader.read_u8()?)?;
        let viewport_width = reader.read_u32()?;
        let viewport_height = reader.read_u32()?;
        let count = reader.read_u32()?;
        let mut systems = Vec::new();
        for _ in 0..count {
            systems.push(SystemReport::read_from(reader)?);
        }
        let metric_count = reader.read_u32()?;
        let mut metrics = Vec::new();
        for _ in 0..metric_count {
            metrics.push(MetricReport::read_from(reader)?);
        }
        Ok(FrameReport {
            engine_frame_index,
            host_frame_sequence,
            runtime_step_count,
            skipped,
            lifecycle,
            viewport_width,
            viewport_height,
            systems,
            metrics,
        })
    }
}

/// Stable wire encoding of the frame lifecycle state.
fn lifecycle_to_u8(state: FrameLifecycleState) -> u8 {
    match state {
        FrameLifecycleState::Active => 0,
        FrameLifecycleState::Hidden => 1,
        FrameLifecycleState::Suspended => 2,
        FrameLifecycleState::ShutdownRequested => 3,
    }
}

/// Inverse of [`lifecycle_to_u8`]; rejects an unknown code.
fn lifecycle_from_u8(code: u8) -> KernelResult<FrameLifecycleState> {
    match code {
        0 => Ok(FrameLifecycleState::Active),
        1 => Ok(FrameLifecycleState::Hidden),
        2 => Ok(FrameLifecycleState::Suspended),
        3 => Ok(FrameLifecycleState::ShutdownRequested),
        _ => Err(KernelError::new(
            KernelErrorScope::Binary,
            KernelErrorCode::InvalidId,
            "unknown frame lifecycle code",
        )),
    }
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
