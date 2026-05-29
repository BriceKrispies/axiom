//! An opaque capability a layer declares it provides.

/// A capability a layer declares it provides, identified by an opaque code.
///
/// The kernel attaches no meaning to specific codes; it only uses equality to
/// reject a layer declaring the same capability twice. Higher layers assign and
/// interpret the codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerCapability(u32);

impl LayerCapability {
    /// Construct a capability from its opaque code.
    pub const fn new(code: u32) -> Self {
        LayerCapability(code)
    }

    /// The opaque capability code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_code_round_trip() {
        assert_eq!(LayerCapability::new(101).code(), 101);
    }

    #[test]
    fn equality_detects_duplicates() {
        assert_eq!(LayerCapability::new(5), LayerCapability::new(5));
        assert_ne!(LayerCapability::new(5), LayerCapability::new(6));
    }
}
