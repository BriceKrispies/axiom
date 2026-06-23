//! The overlay's three display densities and how they cycle.
//!
//! A density chooses *how much* the overlay shows; which concrete rows each
//! density renders lives in [`crate::debug_overlay_state::OverlayState::visible_rows`],
//! next to the state it reads. Cycling is a fixed ring: `compact → normal →
//! verbose → compact`.

/// How much detail the overlay renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayDensity {
    /// Title + fps + frame time + renderer backend + fallback count.
    Compact,
    /// The core diagnostics read-out (the default).
    #[default]
    Normal,
    /// Everything in `Normal`, plus a command-history preview and the raw
    /// backend-selection / overlay debug state.
    Verbose,
}

impl OverlayDensity {
    /// The next density in the ring `compact → normal → verbose → compact`.
    pub fn cycle(self) -> Self {
        match self {
            OverlayDensity::Compact => OverlayDensity::Normal,
            OverlayDensity::Normal => OverlayDensity::Verbose,
            OverlayDensity::Verbose => OverlayDensity::Compact,
        }
    }

    /// A stable lowercase label, used as a CSS modifier and in the header.
    pub fn label(self) -> &'static str {
        match self {
            OverlayDensity::Compact => "compact",
            OverlayDensity::Normal => "normal",
            OverlayDensity::Verbose => "verbose",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_compact_normal_verbose_compact() {
        // The exact ring the Shift+Backquote shortcut walks.
        assert_eq!(OverlayDensity::Compact.cycle(), OverlayDensity::Normal);
        assert_eq!(OverlayDensity::Normal.cycle(), OverlayDensity::Verbose);
        assert_eq!(OverlayDensity::Verbose.cycle(), OverlayDensity::Compact);
    }

    #[test]
    fn cycling_three_times_returns_to_start() {
        let start = OverlayDensity::Compact;
        assert_eq!(start.cycle().cycle().cycle(), start);
    }

    #[test]
    fn default_is_normal() {
        assert_eq!(OverlayDensity::default(), OverlayDensity::Normal);
    }

    #[test]
    fn labels_are_stable_and_distinct() {
        assert_eq!(OverlayDensity::Compact.label(), "compact");
        assert_eq!(OverlayDensity::Normal.label(), "normal");
        assert_eq!(OverlayDensity::Verbose.label(), "verbose");
    }
}
