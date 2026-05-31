//! A deterministic in-memory [`LogSink`] that retains records for inspection.

use crate::log_record::LogRecord;
use crate::log_sink::LogSink;

/// A [`LogSink`] that stores received records in order.
///
/// It performs no I/O: records are appended to a `Vec` in the exact order they
/// arrive, making it ideal for tests and for deterministic replay where the log
/// stream itself is an assertable artifact.
#[derive(Debug, Clone, Default)]
pub struct InMemoryLogSink {
    records: Vec<LogRecord>,
}

impl InMemoryLogSink {
    /// Create an empty sink.
    pub fn new() -> Self {
        InMemoryLogSink {
            records: Vec::new(),
        }
    }

    /// The captured records, in arrival order.
    pub fn records(&self) -> &[LogRecord] {
        &self.records
    }

    /// The number of captured records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether no records have been captured.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Discard all captured records.
    pub fn clear(&mut self) {
        self.records.clear();
    }
}

impl LogSink for InMemoryLogSink {
    fn record(&mut self, record: LogRecord) {
        self.records.push(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_level::LogLevel;

    fn rec(code: u32) -> LogRecord {
        LogRecord::new(LogLevel::Info, "kernel.test", code, "msg")
    }

    #[test]
    fn new_and_default_sinks_are_empty() {
        assert!(InMemoryLogSink::new().is_empty());
        assert!(InMemoryLogSink::default().is_empty());
        assert_eq!(InMemoryLogSink::new().len(), 0);
    }

    #[test]
    fn records_are_captured_in_order() {
        let mut sink = InMemoryLogSink::new();
        sink.record(rec(1));
        sink.record(rec(2));
        sink.record(rec(3));

        let codes: Vec<u32> = sink.records().iter().map(LogRecord::message_code).collect();
        assert_eq!(codes, vec![1, 2, 3]);
        assert_eq!(sink.len(), 3);
    }

    #[test]
    fn capture_is_deterministic_across_runs() {
        let build = || {
            let mut sink = InMemoryLogSink::new();
            sink.record(rec(10));
            sink.record(rec(20));
            sink
        };
        assert_eq!(build().records(), build().records());
    }

    #[test]
    fn populated_sink_is_not_empty() {
        let mut sink = InMemoryLogSink::new();
        sink.record(rec(1));
        // Distinguishes `is_empty -> true`: a sink with a record is NOT empty.
        assert!(!sink.is_empty());
        assert_eq!(sink.len(), 1);
    }

    #[test]
    fn clear_empties_the_sink() {
        let mut sink = InMemoryLogSink::new();
        sink.record(rec(1));
        sink.clear();
        assert!(sink.is_empty());
    }
}
