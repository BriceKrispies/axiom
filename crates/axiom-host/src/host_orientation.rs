//! The coarse display orientation derived from a viewport's pixel extents.

/// Which way round the host surface currently is.
///
/// This is a **derived** fact, not an externally-supplied one: it is a pure
/// function of a viewport's physical pixel extents (see
/// [`crate::HostViewport::orientation`]). Deriving it from the dimensions that
/// already exist keeps the host boundary from having to trust a *separate*,
/// possibly-inconsistent orientation signal — the surface extent is the single
/// source of truth, so the orientation can never disagree with the size the
/// engine actually renders into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Width and height are equal.
    Square,
    /// Width exceeds height (the classic desktop / landscape-phone case).
    Landscape,
    /// Height exceeds width (a phone held upright).
    Portrait,
}

impl Orientation {
    /// Derive the orientation from physical width and height, branchlessly.
    ///
    /// The two total comparisons select a table index — `0 = Square`,
    /// `1 = Landscape`, `2 = Portrait` — with no control flow:
    /// `index = (w > h) + 2·(w < h)`. Exactly one of the comparisons can be
    /// true, so the index is always in range.
    pub(crate) fn from_extents(width: u32, height: u32) -> Self {
        let index = (width > height) as usize + 2 * ((width < height) as usize);
        [
            Orientation::Square,
            Orientation::Landscape,
            Orientation::Portrait,
        ][index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_extents_are_square() {
        assert_eq!(Orientation::from_extents(800, 800), Orientation::Square);
    }

    #[test]
    fn wider_than_tall_is_landscape() {
        assert_eq!(Orientation::from_extents(1600, 900), Orientation::Landscape);
    }

    #[test]
    fn taller_than_wide_is_portrait() {
        assert_eq!(Orientation::from_extents(1080, 1920), Orientation::Portrait);
    }

    #[test]
    fn one_pixel_wider_flips_to_landscape() {
        // Boundary: a single pixel of difference is enough. A `>=` mutant of
        // the inner comparison would misclassify the equal case, which the
        // `equal_extents_are_square` test already pins.
        assert_eq!(Orientation::from_extents(801, 800), Orientation::Landscape);
        assert_eq!(Orientation::from_extents(800, 801), Orientation::Portrait);
    }

    #[test]
    fn variants_are_distinct() {
        assert_ne!(Orientation::Square, Orientation::Landscape);
        assert_ne!(Orientation::Landscape, Orientation::Portrait);
        assert_ne!(Orientation::Portrait, Orientation::Square);
    }

    #[test]
    fn orientation_is_copy() {
        let a = Orientation::Portrait;
        let b = a;
        assert_eq!(a, b);
    }
}
