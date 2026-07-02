//! [`InputDebuggerState`] — the typed placeholder state of the Input Debugger
//! panel: an ordered list of observed input events.
//!
//! This is the debugger's **own** placeholder view of input, deliberately
//! distinct from the session-recording [`crate::session_record::RecordedInput`]
//! artifact: it carries a human-facing label alongside the raw code so the panel
//! can render events without decoding them. Pure value data — the panel simulates
//! nothing. A future integration wires the real input stream in.

use axiom_kernel::Tick;

/// One placeholder input event as seen by the debugger: the tick it occurred on,
/// a raw input code, and a human-facing label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEventRecord {
    /// The tick this event occurred on.
    pub tick: Tick,
    /// The raw input code.
    pub code: u32,
    /// A human-facing placeholder label for the event.
    pub label: String,
}

impl InputEventRecord {
    /// Build a placeholder input event record.
    #[must_use]
    pub fn new(tick: Tick, code: u32, label: &str) -> Self {
        InputEventRecord {
            tick,
            code,
            label: label.to_string(),
        }
    }
}

/// The Input Debugger panel state: an ordered list of placeholder input events.
/// `Default` is empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputDebuggerState {
    inputs: Vec<InputEventRecord>,
}

impl InputDebuggerState {
    /// Append an input event, preserving insertion order exactly.
    pub fn record_input(&mut self, input: InputEventRecord) {
        self.inputs.push(input);
    }

    /// The input events, in insertion order.
    #[must_use]
    pub fn inputs(&self) -> &[InputEventRecord] {
        &self.inputs
    }
}
