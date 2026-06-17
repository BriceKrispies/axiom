//! One system's outcome inside a recorded frame.

use axiom_frame::FrameSystemReport;
use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

/// An owned, serializable summary of one system that ran during a frame.
///
/// The frame layer carries per-system detail as [`FrameSystemReport`] (whose
/// name borrows a `'static str`). Introspection needs an *owned*, serializable
/// form so a report can outlive the frame and cross the serialization boundary
/// to an agent, so the name is copied into a `String`.
///
/// `error_code` is `None` for a system that succeeded and `Some(code)` — the
/// raw runtime error code — for one that failed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SystemReport {
    system_id: u64,
    name: String,
    order: i32,
    succeeded: bool,
    error_code: Option<u16>,
}

impl SystemReport {
    /// Project a frame's per-system report into an owned report.
    pub fn from_frame(report: &FrameSystemReport) -> Self {
        SystemReport {
            system_id: report.system_id(),
            name: report.name().to_string(),
            order: report.order(),
            succeeded: report.succeeded(),
            error_code: report.error_code(),
        }
    }

    /// The stable kernel handle id of the system, as a raw `u64`.
    pub const fn system_id(&self) -> u64 {
        self.system_id
    }

    /// The system's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The order value the system was registered with.
    pub const fn order(&self) -> i32 {
        self.order
    }

    /// Whether the system succeeded.
    pub const fn succeeded(&self) -> bool {
        self.succeeded
    }

    /// The raw runtime error code if the system failed, else `None`.
    pub const fn error_code(&self) -> Option<u16> {
        self.error_code
    }

    /// Append this report to a writer. The name is length-prefixed; the
    /// optional error code is a `bool` presence tag followed by the `u16`.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u64(self.system_id);
        writer.write_byte_slice(self.name.as_bytes());
        writer.write_i32(self.order);
        writer.write_bool(self.succeeded);
        writer.write_bool(self.error_code.is_some());
        self.error_code
            .iter()
            .for_each(|code| writer.write_u16(*code));
    }

    /// Read a report previously written with [`Self::write_to`]. The name is
    /// decoded lossily, so any byte sequence yields a valid `String` (system
    /// names are ASCII in practice, so round trips are exact).
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let system_id = reader.read_u64()?;
        let name = String::from_utf8_lossy(reader.read_byte_slice()?).into_owned();
        let order = reader.read_i32()?;
        let succeeded = reader.read_bool()?;
        let error_code = reader
            .read_bool()?
            .then(|| reader.read_u16())
            .transpose()?;
        Ok(SystemReport {
            system_id,
            name,
            order,
            succeeded,
            error_code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use axiom_frame::EngineFrame;

    fn only_system(frame: &EngineFrame) -> SystemReport {
        let summary = &frame.runtime_step_summaries()[0];
        SystemReport::from_frame(&summary.systems()[0])
    }

    #[test]
    fn from_frame_copies_every_field() {
        let frame = fixtures::failing_engine_frame();
        let report = only_system(&frame);
        assert_eq!(report.system_id(), 1);
        assert_eq!(report.name(), "fail");
        assert_eq!(report.order(), 1);
        assert!(!report.succeeded());
        assert!(report.error_code().is_some());
    }

    #[test]
    fn failed_system_round_trips_with_error_code() {
        let report = only_system(&fixtures::failing_engine_frame());
        let mut w = BinaryWriter::new();
        report.write_to(&mut w);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(SystemReport::read_from(&mut r).unwrap(), report);
    }

    #[test]
    fn succeeded_system_round_trips_without_error_code() {
        // A hand-built success report exercises the `None` arm of both
        // write_to and read_from (frame fixtures register only failing
        // systems).
        let report = SystemReport {
            system_id: 9,
            name: "physics".to_string(),
            order: -3,
            succeeded: true,
            error_code: None,
        };
        let mut w = BinaryWriter::new();
        report.write_to(&mut w);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        let decoded = SystemReport::read_from(&mut r).unwrap();
        assert_eq!(decoded, report);
        assert_eq!(decoded.error_code(), None);
        assert_eq!(decoded.name(), "physics");
        assert_eq!(decoded.system_id(), 9);
        assert_eq!(decoded.order(), -3);
        assert!(decoded.succeeded());
    }

    #[test]
    fn truncation_at_every_prefix_is_err() {
        let report = only_system(&fixtures::failing_engine_frame());
        let mut w = BinaryWriter::new();
        report.write_to(&mut w);
        let bytes = w.into_bytes();
        for len in 0..bytes.len() {
            let mut r = BinaryReader::new(&bytes[..len]);
            assert!(
                SystemReport::read_from(&mut r).is_err(),
                "truncated read at len {len} must fail"
            );
        }
    }
}
