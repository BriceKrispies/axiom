//! An angle value, constructed in degrees or radians.

/// An angle. Constructed in whichever unit reads best at the call site; the
/// engine reads it back in radians (the unit its math layer uses).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Angle {
    radians: f32,
}

impl Angle {
    /// An angle given in degrees.
    pub fn degrees(degrees: f32) -> Self {
        Angle {
            radians: degrees.to_radians(),
        }
    }

    /// An angle given in radians.
    pub const fn radians(radians: f32) -> Self {
        Angle { radians }
    }

    /// The angle in radians.
    pub const fn as_radians(self) -> f32 {
        self.radians
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_convert_to_radians() {
        assert!((Angle::degrees(180.0).as_radians() - std::f32::consts::PI).abs() < 1.0e-6);
    }

    #[test]
    fn radians_round_trip() {
        assert_eq!(Angle::radians(1.5).as_radians(), 1.5);
    }
}
