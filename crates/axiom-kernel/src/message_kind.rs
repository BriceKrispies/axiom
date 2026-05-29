//! An opaque, layer-defined message classification code.

/// A classification code for a message.
///
/// The kernel deliberately knows **no** concrete message kinds — gameplay,
/// rendering and other meanings belong to higher layers. `MessageKind` is just
/// an opaque, totally-ordered `u32` tag those layers assign meaning to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageKind(u32);

impl MessageKind {
    /// Construct a kind from a raw code.
    pub const fn new(raw: u32) -> Self {
        MessageKind(raw)
    }

    /// The raw code.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_raw_round_trip() {
        assert_eq!(MessageKind::new(7).raw(), 7);
    }

    #[test]
    fn equality_and_ordering_are_numeric() {
        assert_eq!(MessageKind::new(3), MessageKind::new(3));
        assert!(MessageKind::new(1) < MessageKind::new(2));
    }
}
