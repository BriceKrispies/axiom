//! [`DeviceFrame`] — the neutral, recordable event bundle the platform edge hands
//! to [`crate::InputState::sample`], and [`Pointer`] — one contact's resolved state.

use axiom_math::Vec2;

use crate::key_token::KeyToken;

/// One contact's resolved state for a tick: where it is and whether it is down.
///
/// A *pointer* is a single contact — a mouse with its primary button down, a
/// finger, or a pen tip — all reduced to the same neutral shape by the platform
/// edge. Returned by [`crate::InputState::pointer`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    /// The contact position in surface pixels (`+x` right, `+y` down).
    pub pos: Vec2,
    /// Whether the contact is pressed this tick.
    pub down: bool,
}

/// The neutral bundle of one frame's raw device activity, decoded by the host
/// into layout-stable tokens and neutral pointer samples before it crosses into
/// the sampling boundary.
///
/// It references no browser type: the set of [`KeyToken`]s down this frame, the
/// pointer `(position, is_down)` samples, and the surface they live on. The
/// platform edge (the windowing module) produces it; this module folds it into a
/// per-tick intent snapshot. Being neutral data, a `DeviceFrame` stream is
/// recordable and replayable.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceFrame {
    surface: Vec2,
    keys_down: Vec<KeyToken>,
    pointers: Vec<(Vec2, bool)>,
}

impl DeviceFrame {
    /// Assemble a frame from the keys down this frame, the pointer samples, and
    /// the `surface` (device pixels, `+x` right, `+y` down) they are measured in.
    pub fn new(surface: Vec2, keys_down: &[KeyToken], pointers: &[(Vec2, bool)]) -> Self {
        DeviceFrame {
            surface,
            keys_down: keys_down.to_vec(),
            pointers: pointers.to_vec(),
        }
    }

    /// The surface the samples are measured in.
    pub(crate) fn surface(&self) -> Vec2 {
        self.surface
    }

    /// The pointer samples this frame.
    pub(crate) fn pointers(&self) -> &[(Vec2, bool)] {
        &self.pointers
    }

    /// Whether `token` is among the keys down this frame.
    pub(crate) fn is_token_down(&self, token: &KeyToken) -> bool {
        self.keys_down.contains(token)
    }

    /// The frame's primary contact as a [`Pointer`], or `None` when no pointer was
    /// sampled. The first sample is the primary contact.
    pub(crate) fn primary_pointer(&self) -> Option<Pointer> {
        self.pointers
            .first()
            .map(|&(pos, down)| Pointer { pos, down })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_pointer_is_the_first_sample_or_none() {
        let empty = DeviceFrame::new(Vec2::ONE, &[], &[]);
        assert_eq!(empty.primary_pointer(), None);

        let frame = DeviceFrame::new(
            Vec2::ONE,
            &[],
            &[(Vec2::new(3.0, 4.0), true), (Vec2::new(9.0, 9.0), false)],
        );
        assert_eq!(
            frame.primary_pointer(),
            Some(Pointer {
                pos: Vec2::new(3.0, 4.0),
                down: true,
            })
        );
    }

    #[test]
    fn reports_surface_pointers_and_token_membership() {
        let frame = DeviceFrame::new(
            Vec2::new(800.0, 600.0),
            &[KeyToken::new("KeyW")],
            &[(Vec2::new(1.0, 2.0), true)],
        );
        assert_eq!(frame.surface(), Vec2::new(800.0, 600.0));
        assert_eq!(frame.pointers(), &[(Vec2::new(1.0, 2.0), true)]);
        assert!(frame.is_token_down(&KeyToken::new("KeyW")));
        assert!(!frame.is_token_down(&KeyToken::new("KeyA")));
    }

    #[test]
    fn frame_clones_equal() {
        let frame = DeviceFrame::new(Vec2::ONE, &[KeyToken::new("KeyW")], &[]);
        assert_eq!(frame.clone(), frame);
    }
}
