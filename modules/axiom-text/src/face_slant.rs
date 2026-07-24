//! The upright/italic/oblique presentation of a font face.

/// How a face slants. Stored in a compiled font as a single validated byte; the
/// three-state vocabulary keeps italic and (synthetic) oblique distinct without
/// a boolean pair that can express a fourth, meaningless state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceSlant {
    /// Normal, non-slanted.
    #[default]
    Upright,
    /// A true italic face.
    Italic,
    /// A slanted (obliqued) presentation of an upright design.
    Oblique,
}

impl FaceSlant {
    /// The stable byte discriminant serialized into a compiled font.
    pub const fn raw(self) -> u8 {
        // A fixed table read (no branch): the enum's declaration order is its
        // wire order.
        [0u8, 1, 2][self as usize]
    }

    /// Recover a slant from its byte, or `None` for an unknown discriminant.
    pub fn from_raw(raw: u8) -> Option<FaceSlant> {
        [Self::Upright, Self::Italic, Self::Oblique]
            .get(raw as usize)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_round_trips_every_slant() {
        [FaceSlant::Upright, FaceSlant::Italic, FaceSlant::Oblique]
            .into_iter()
            .for_each(|s| assert_eq!(FaceSlant::from_raw(s.raw()), Some(s)));
    }

    #[test]
    fn default_is_upright() {
        assert_eq!(FaceSlant::default(), FaceSlant::Upright);
    }

    #[test]
    fn unknown_discriminant_is_none() {
        assert_eq!(FaceSlant::from_raw(3), None);
        assert_eq!(FaceSlant::from_raw(255), None);
    }
}
