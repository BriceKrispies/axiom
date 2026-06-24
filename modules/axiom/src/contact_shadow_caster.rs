//! `ContactShadowCaster`: opt-in marker that a renderable grounds itself with a
//! contact shadow.
//!
//! Spawn it alongside a [`crate::prelude::Renderable`] on a node that is a
//! discrete, dynamic object (an enemy, a prop) you want grounded with a shadow.
//! Level geometry (walls, floors) simply omits it. The mark rides the per-draw
//! data all the way to the presentation boundary, where a grounding backend (the
//! software Canvas 2D path) projects each marked object onto the ground; backends
//! with real shadows (the GPU path) ignore it. It carries no data — its presence
//! is the whole signal.

/// Marks the node's renderable as a contact-shadow caster (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContactShadowCaster;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_a_zero_data_value() {
        // It is a pure marker: all instances are equal and it is `Default`.
        assert_eq!(ContactShadowCaster, ContactShadowCaster::default());
        assert_eq!(core::mem::size_of::<ContactShadowCaster>(), 0);
        assert!(format!("{:?}", ContactShadowCaster).contains("ContactShadowCaster"));
    }
}
