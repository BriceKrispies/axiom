//! Opaque reference to a material resource owned outside the scene module.

/// An opaque, stable reference to a material resource.
///
/// `axiom-scene` does not own materials; a [`MaterialRef`] is just a
/// `u64` identity that future resource/render modules (or apps) will
/// resolve to actual material data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialRef(u64);

impl MaterialRef {
    pub const INVALID: MaterialRef = MaterialRef(0);

    pub const fn from_raw(raw: u64) -> Self {
        MaterialRef(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero() {
        assert!(!MaterialRef::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid_and_stable() {
        let a = MaterialRef::from_raw(99);
        let b = MaterialRef::from_raw(99);
        assert!(a.is_valid());
        assert_eq!(a, b);
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(MaterialRef::from_raw(1) < MaterialRef::from_raw(2));
    }
}
