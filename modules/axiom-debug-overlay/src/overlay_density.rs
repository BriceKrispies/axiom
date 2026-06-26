//! The overlay's three display densities and how they cycle — branchless.
//!
//! `OverlayDensity` is a fieldless enum, so its discriminant indexes a `const`
//! table for both the cycle ring and the label (the sanctioned branchless form
//! for a fieldless-enum match).

/// How much detail the overlay renders. Discriminants are explicit so they can
/// index the tables below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum OverlayDensity {
    Compact = 0,
    #[default]
    Normal = 1,
    Verbose = 2,
}

impl OverlayDensity {
    /// The next density in the ring `compact → normal → verbose → compact`.
    pub(crate) fn cycle(self) -> Self {
        const NEXT: [OverlayDensity; 3] = [
            OverlayDensity::Normal,
            OverlayDensity::Verbose,
            OverlayDensity::Compact,
        ];
        NEXT[self as usize]
    }

    /// A stable lowercase label, used as a CSS modifier and in the header.
    pub(crate) fn label(self) -> &'static str {
        const LABELS: [&str; 3] = ["compact", "normal", "verbose"];
        LABELS[self as usize]
    }

    /// Resolve a density from its label, or `None` for an unknown label.
    pub(crate) fn from_label(label: &str) -> Option<OverlayDensity> {
        const TABLE: [(&str, OverlayDensity); 3] = [
            ("compact", OverlayDensity::Compact),
            ("normal", OverlayDensity::Normal),
            ("verbose", OverlayDensity::Verbose),
        ];
        TABLE
            .iter()
            .find(|(name, _)| *name == label)
            .map(|(_, density)| *density)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_compact_normal_verbose_compact() {
        assert_eq!(OverlayDensity::Compact.cycle(), OverlayDensity::Normal);
        assert_eq!(OverlayDensity::Normal.cycle(), OverlayDensity::Verbose);
        assert_eq!(OverlayDensity::Verbose.cycle(), OverlayDensity::Compact);
    }

    #[test]
    fn labels_are_stable_and_distinct() {
        assert_eq!(OverlayDensity::Compact.label(), "compact");
        assert_eq!(OverlayDensity::Normal.label(), "normal");
        assert_eq!(OverlayDensity::Verbose.label(), "verbose");
    }

    #[test]
    fn from_label_round_trips_every_density_and_rejects_unknown() {
        assert_eq!(
            OverlayDensity::from_label("compact"),
            Some(OverlayDensity::Compact)
        );
        assert_eq!(
            OverlayDensity::from_label("normal"),
            Some(OverlayDensity::Normal)
        );
        assert_eq!(
            OverlayDensity::from_label("verbose"),
            Some(OverlayDensity::Verbose)
        );
        assert_eq!(OverlayDensity::from_label("nope"), None);
    }

    #[test]
    fn default_is_normal() {
        assert_eq!(OverlayDensity::default(), OverlayDensity::Normal);
    }
}
