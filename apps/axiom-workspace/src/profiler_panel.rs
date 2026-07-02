//! [`ProfilerPanelState`] — the typed placeholder state of the Profiler panel: an
//! ordered list of timing samples.
//!
//! Samples are stored as structured [`ProfilerSample`] values (label, integer
//! microseconds, tick). Timings are integer microseconds — the workspace carries
//! no naked floats. These are placeholder frame/system samples; a future
//! integration wires the engine's real per-phase timings into this panel.

use axiom_kernel::Tick;

/// One placeholder profiler sample: a label, an elapsed duration in integer
/// microseconds, and the tick it was measured on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilerSample {
    /// The label of the measured span (e.g. a placeholder phase name).
    pub label: String,
    /// The elapsed time in integer microseconds (no naked floats).
    pub micros: u64,
    /// The tick this sample was measured on.
    pub tick: Tick,
}

impl ProfilerSample {
    /// Build a placeholder profiler sample.
    #[must_use]
    pub fn new(label: &str, micros: u64, tick: Tick) -> Self {
        ProfilerSample {
            label: label.to_string(),
            micros,
            tick,
        }
    }
}

/// The Profiler panel state: an ordered list of placeholder samples. `Default` is
/// empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfilerPanelState {
    samples: Vec<ProfilerSample>,
}

impl ProfilerPanelState {
    /// Append a sample, preserving insertion order exactly.
    pub fn record_sample(&mut self, sample: ProfilerSample) {
        self.samples.push(sample);
    }

    /// The samples, in insertion order.
    #[must_use]
    pub fn samples(&self) -> &[ProfilerSample] {
        &self.samples
    }
}
