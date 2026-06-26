//! [`TimelineMode`] — whether the recorder is following live frames or paused on
//! a scrubbed frame.
//!
//! This is a pure **output** value type. It carries the *selected* frame while
//! scrubbing but owns no captures and mutates no timeline — scrubbing is a
//! read-only view over already-recorded frames. A consumer reads this to decide
//! whether to keep appending captures (live) or to display a held frame
//! (scrubbing); the consumer destructures the `Scrubbing` payload itself.
//!
//! Inside this module the mode is only *constructed* and *discriminated* (never
//! destructured), so the module stays branchless — `RecordingApi` tracks the
//! selected frame in its own field rather than reading it back out of the enum.

use axiom_kernel::FrameIndex;

/// The recorder's playback mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineMode {
    /// Following the newest recorded frame; new captures are appended.
    Live,
    /// Paused on `selected_frame`; the live timeline is not mutated.
    Scrubbing {
        /// The frame currently being inspected.
        selected_frame: FrameIndex,
    },
}

impl TimelineMode {
    /// The live (append) mode.
    pub(crate) fn live() -> Self {
        TimelineMode::Live
    }

    /// A scrubbing mode paused on `selected_frame`.
    pub(crate) fn scrubbing(selected_frame: FrameIndex) -> Self {
        TimelineMode::Scrubbing { selected_frame }
    }

    /// Whether the recorder is in live (append) mode. Discriminates via derived
    /// equality rather than a `match`/`matches!` arm, keeping it branchless.
    pub fn is_live(self) -> bool {
        self == TimelineMode::Live
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_mode_is_live() {
        let m = TimelineMode::live();
        assert!(m.is_live());
        assert_eq!(m, TimelineMode::Live);
    }

    #[test]
    fn scrubbing_mode_is_not_live_and_carries_its_selection() {
        let m = TimelineMode::scrubbing(FrameIndex::new(12));
        assert!(!m.is_live());
        // The selection is carried in the constructed variant; comparing against
        // the expected variant verifies it without an (uncoverable) panic arm.
        assert_eq!(
            m,
            TimelineMode::Scrubbing {
                selected_frame: FrameIndex::new(12)
            }
        );
        assert_ne!(m, TimelineMode::scrubbing(FrameIndex::new(13)));
    }

    #[test]
    fn mode_is_copy_and_comparable() {
        let a = TimelineMode::scrubbing(FrameIndex::new(3));
        let b = a;
        assert_eq!(a, b);
        assert_ne!(TimelineMode::Live, a);
        assert!(format!("{a:?}").contains("Scrubbing"));
    }
}
