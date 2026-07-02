//! [`ConsoleLogViewerState`] — the typed placeholder state of the Console Log
//! Viewer panel: an ordered list of **structured** log records.
//!
//! Records are stored as structured [`ConsoleRecord`] values (level, message,
//! tick), never as pre-formatted text. These are placeholder logs; a future
//! integration wires the kernel's real `LogRecord` stream into this panel. Pure
//! value data — the panel simulates nothing.

use axiom_kernel::Tick;

/// The severity level of a [`ConsoleRecord`]. A placeholder vocabulary mirroring
/// the shape of the kernel's log levels until the real stream is wired in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    /// Finest-grained diagnostic detail.
    Trace,
    /// Debugging detail.
    Debug,
    /// Informational message.
    Info,
    /// A warning.
    Warn,
    /// An error.
    Error,
}

/// One structured, placeholder console record: a level, a message, and the tick
/// it was emitted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleRecord {
    /// The record severity.
    pub level: ConsoleLevel,
    /// The record message.
    pub message: String,
    /// The tick the record was emitted on.
    pub tick: Tick,
}

impl ConsoleRecord {
    /// Build a placeholder structured console record.
    #[must_use]
    pub fn new(level: ConsoleLevel, message: &str, tick: Tick) -> Self {
        ConsoleRecord {
            level,
            message: message.to_string(),
            tick,
        }
    }
}

/// The Console Log Viewer panel state: an ordered list of structured placeholder
/// records. `Default` is empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConsoleLogViewerState {
    records: Vec<ConsoleRecord>,
}

impl ConsoleLogViewerState {
    /// Append a structured record, preserving insertion order exactly.
    pub fn record(&mut self, record: ConsoleRecord) {
        self.records.push(record);
    }

    /// The records, in insertion order.
    #[must_use]
    pub fn records(&self) -> &[ConsoleRecord] {
        &self.records
    }
}
