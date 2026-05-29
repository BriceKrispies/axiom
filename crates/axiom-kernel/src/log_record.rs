//! A structured, deterministic log record — data, never a printed side effect.

use crate::frame_index::FrameIndex;
use crate::log_field::LogField;
use crate::log_level::LogLevel;
use crate::tick::Tick;

/// A single structured log entry.
///
/// A record is **pure data**: building one prints nothing and touches no
/// ambient environment. It carries a machine-readable `message_code` as its
/// primary identity (a static message string is metadata only), an optional
/// deterministic tick/frame, a static scope name, and structured fields. Two
/// records built from the same inputs are equal, which makes log assertions in
/// deterministic replays exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    level: LogLevel,
    scope: &'static str,
    message_code: u32,
    message: &'static str,
    tick: Option<Tick>,
    frame: Option<FrameIndex>,
    fields: Vec<LogField>,
}

impl LogRecord {
    /// Begin a record with its level, static scope, machine message code and
    /// static human message. Tick/frame are absent and fields empty until added.
    pub fn new(
        level: LogLevel,
        scope: &'static str,
        message_code: u32,
        message: &'static str,
    ) -> Self {
        LogRecord {
            level,
            scope,
            message_code,
            message,
            tick: None,
            frame: None,
            fields: Vec::new(),
        }
    }

    /// Attach a deterministic tick and frame index.
    pub fn at(mut self, tick: Tick, frame: FrameIndex) -> Self {
        self.tick = Some(tick);
        self.frame = Some(frame);
        self
    }

    /// Append a structured field.
    pub fn with_field(mut self, field: LogField) -> Self {
        self.fields.push(field);
        self
    }

    /// The severity level.
    pub fn level(&self) -> LogLevel {
        self.level
    }

    /// The static scope name.
    pub fn scope(&self) -> &'static str {
        self.scope
    }

    /// The machine-readable message code (primary identity).
    pub fn message_code(&self) -> u32 {
        self.message_code
    }

    /// The static human-readable message (metadata only).
    pub fn message(&self) -> &'static str {
        self.message
    }

    /// The tick this record was emitted at, if any.
    pub fn tick(&self) -> Option<Tick> {
        self.tick
    }

    /// The frame this record was emitted at, if any.
    pub fn frame(&self) -> Option<FrameIndex> {
        self.frame
    }

    /// The structured fields, in attachment order.
    pub fn fields(&self) -> &[LogField] {
        &self.fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_record_has_no_tick_or_fields() {
        let r = LogRecord::new(LogLevel::Info, "kernel.boot", 1, "kernel started");
        assert_eq!(r.level(), LogLevel::Info);
        assert_eq!(r.scope(), "kernel.boot");
        assert_eq!(r.message_code(), 1);
        assert_eq!(r.message(), "kernel started");
        assert!(r.tick().is_none());
        assert!(r.frame().is_none());
        assert!(r.fields().is_empty());
    }

    #[test]
    fn builder_attaches_tick_frame_and_fields() {
        let r = LogRecord::new(LogLevel::Warn, "kernel.clock", 42, "step skipped")
            .at(Tick::new(10), FrameIndex::new(10))
            .with_field(LogField::u64("skipped", 1))
            .with_field(LogField::str("reason", "overflow"));

        assert_eq!(r.tick(), Some(Tick::new(10)));
        assert_eq!(r.frame(), Some(FrameIndex::new(10)));
        assert_eq!(r.fields().len(), 2);
        assert_eq!(r.fields()[0].as_u64(), Some(1));
        assert_eq!(r.fields()[1].as_str(), Some("overflow"));
    }

    #[test]
    fn identical_inputs_produce_equal_records() {
        let build = || {
            LogRecord::new(LogLevel::Error, "kernel.binary", 7, "decode failed")
                .at(Tick::new(3), FrameIndex::new(3))
                .with_field(LogField::i64("offset", -1))
        };
        assert_eq!(build(), build());
    }
}
