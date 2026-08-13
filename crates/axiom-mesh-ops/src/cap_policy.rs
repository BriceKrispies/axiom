//! Which ends of a swept, extruded, or revolved surface get closed off.

/// Whether an operator closes the start and/or end of the surface it generates.
///
/// The discriminant is a two-bit set — bit 0 is the start cap, bit 1 the end cap
/// — so asking about one end is a mask, not a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CapPolicy {
    /// Leave both ends open. The surface is a shell.
    None = 0,
    /// Close the start only.
    Start = 1,
    /// Close the end only.
    End = 2,
    /// Close both ends, producing a solid.
    #[default]
    Both = 3,
}

impl CapPolicy {
    /// Whether the start of the surface is closed.
    pub const fn caps_start(self) -> bool {
        (self as u8 & 1) != 0
    }

    /// Whether the end of the surface is closed.
    pub const fn caps_end(self) -> bool {
        (self as u8 & 2) != 0
    }

    /// How many caps this policy adds — 0, 1, or 2.
    pub const fn cap_count(self) -> usize {
        (self as u8 & 1) as usize + ((self as u8 >> 1) & 1) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_caps_neither_end() {
        assert!(!CapPolicy::None.caps_start());
        assert!(!CapPolicy::None.caps_end());
        assert_eq!(CapPolicy::None.cap_count(), 0);
    }

    #[test]
    fn start_caps_only_the_start() {
        assert!(CapPolicy::Start.caps_start());
        assert!(!CapPolicy::Start.caps_end());
        assert_eq!(CapPolicy::Start.cap_count(), 1);
    }

    #[test]
    fn end_caps_only_the_end() {
        assert!(!CapPolicy::End.caps_start());
        assert!(CapPolicy::End.caps_end());
        assert_eq!(CapPolicy::End.cap_count(), 1);
    }

    #[test]
    fn both_caps_both_ends_and_is_the_default() {
        assert!(CapPolicy::Both.caps_start());
        assert!(CapPolicy::Both.caps_end());
        assert_eq!(CapPolicy::Both.cap_count(), 2);
        assert_eq!(CapPolicy::default(), CapPolicy::Both);
    }
}
