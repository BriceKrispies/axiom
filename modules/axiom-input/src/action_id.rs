//! [`ActionId`] — the opaque, author-defined action identity the binding table
//! and the per-tick reads are keyed on.

/// An author-defined action, interned to a stable id the simulation reads by.
///
/// It is *opaque*: gameplay names an action (`ActionId::new(JUMP)`) and asks the
/// snapshot about it (`is_down`, `pressed`, `axis`, …); it never names a physical
/// key. The author picks the stable raw id once — the same role the interface
/// layer's keymap gives its neutral `u32` action — and remaps which keys fire it
/// with [`crate::InputState::bind_action`] without changing the id gameplay reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActionId(u32);

impl ActionId {
    /// Intern a raw action id. Two `ActionId`s are equal iff their raw ids are.
    pub const fn new(raw: u32) -> Self {
        ActionId(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_ids_compare_equal_distinct_ids_do_not() {
        assert_eq!(ActionId::new(7), ActionId::new(7));
        assert_ne!(ActionId::new(7), ActionId::new(8));
    }
}
