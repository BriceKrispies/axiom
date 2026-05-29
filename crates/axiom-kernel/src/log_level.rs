//! The severity level of a log record.

/// The severity of a [`crate::log_record::LogRecord`].
///
/// Levels are totally ordered by increasing severity, so sinks can filter with
/// a simple comparison (`level >= LogLevel::Warn`). Discriminants are stable
/// `#[repr(u8)]` values for deterministic serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl LogLevel {
    /// The stable numeric discriminant of this level.
    pub const fn raw(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(LogLevel::Trace.raw(), 0);
        assert_eq!(LogLevel::Error.raw(), 4);
    }

    #[test]
    fn severity_is_ordered() {
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }
}
