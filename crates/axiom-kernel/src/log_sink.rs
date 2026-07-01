//! The trait through which structured log records are emitted.

use crate::log_record::LogRecord;

/// A destination that receives structured [`LogRecord`]s.
///
/// This is the kernel's only logging output path: records are *handed to a
/// sink*, never printed. The kernel ships one deterministic, in-memory
/// implementation ([`crate::in_memory_log_sink::InMemoryLogSink`]); higher
/// layers may implement sinks that forward records onward (still as data).
pub trait LogSink {
    /// Receive a record. Implementations must not perform ambient I/O.
    fn record(&mut self, record: LogRecord);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_level::LogLevel;

    #[derive(Default)]
    struct CountingSink {
        count: usize,
        last_code: u32,
    }

    impl LogSink for CountingSink {
        fn record(&mut self, record: LogRecord) {
            self.count += 1;
            self.last_code = record.message_code();
        }
    }

    #[test]
    fn records_are_delivered_to_the_sink() {
        let mut sink = CountingSink::default();
        sink.record(LogRecord::new(LogLevel::Info, "test", 11, "a"));
        sink.record(LogRecord::new(LogLevel::Info, "test", 22, "b"));
        assert_eq!(sink.count, 2);
        assert_eq!(sink.last_code, 22);
    }
}
