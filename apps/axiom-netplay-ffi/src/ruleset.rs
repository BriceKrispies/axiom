//! The v1 game ruleset — the **only** place game-specific schema lives.
//!
//! It maps an opaque intent payload to an engine move and decides movement
//! legality. This is deliberately in the app, never in `axiom-net-protocol` or
//! any layer/module: the protocol carries opaque bounded bytes, and the worker's
//! ruleset is what gives those bytes meaning. All functions are pure and never
//! panic.

use axiom::prelude::{PlayerInput, Vec3};

use crate::status::{REASON_IMPOSSIBLE_MOVEMENT, REASON_MALFORMED};

/// A well-formed move-intent payload is two little-endian `f32`: `(dx, dy)`.
pub const MOVE_PAYLOAD_LEN: usize = 8;

/// The largest per-intent move magnitude the ruleset permits on either axis. An
/// intent beyond this is "impossible movement" — rejected, not clamped — so a
/// cheating client cannot teleport by inflating a single delta.
pub const MAX_INTENT_DELTA: f32 = 1.0;

/// The half-extent of the square play field, in world units. A player's
/// authoritative position is clamped to `[-FIELD_BOUND, FIELD_BOUND]` on each
/// axis: the server is the wall, so a player cannot walk out of the arena no
/// matter how long they hold a key. The browser predicts this *same* bound (its
/// `LIMIT`), so client prediction converges with authority at the wall instead
/// of drifting past it. Unlike [`MAX_INTENT_DELTA`] (a per-intent legality check
/// that *rejects*), this is a positional bound the worker *enforces* by trimming
/// the applied delta.
pub const FIELD_BOUND: f32 = 3.5;

/// Given a player's current position component on one axis and the net delta it
/// would move this tick, return the *effective* delta that keeps the resulting
/// position inside `[-FIELD_BOUND, FIELD_BOUND]`. Moving back toward the centre
/// is never restricted; only the component that would cross a wall is trimmed.
pub fn clamp_axis(pos: f32, delta: f32) -> f32 {
    (pos + delta).clamp(-FIELD_BOUND, FIELD_BOUND) - pos
}

/// Decode an opaque payload into a validated `(dx, dy)` move, or an
/// `REASON_*` code describing why it is illegal:
/// - [`REASON_MALFORMED`] if the payload is the wrong length, or
/// - [`REASON_IMPOSSIBLE_MOVEMENT`] if a component is non-finite or exceeds the
///   per-axis bound.
pub fn decode_move(payload: &[u8]) -> Result<(f32, f32), u32> {
    let head = payload.get(..MOVE_PAYLOAD_LEN).ok_or(REASON_MALFORMED)?;
    let bytes = <[u8; MOVE_PAYLOAD_LEN]>::try_from(head).map_err(|_| REASON_MALFORMED)?;
    let dx = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let dy = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let legal = dx.is_finite()
        && dy.is_finite()
        && dx.abs() <= MAX_INTENT_DELTA
        && dy.abs() <= MAX_INTENT_DELTA;
    legal.then_some((dx, dy)).ok_or(REASON_IMPOSSIBLE_MOVEMENT)
}

/// The engine input for an accepted move (a world-space translation on the XY
/// plane of the addressed player's node).
pub fn player_move(player_id: u32, dx: f32, dy: f32) -> PlayerInput {
    PlayerInput::new(player_id, Vec3::new(dx, dy, 0.0))
}

/// Encode a `(dx, dy)` move into the canonical wire payload. Used by tests and by
/// any host that wants to build a payload through the one ruleset.
pub fn encode_move(dx: f32, dy: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(MOVE_PAYLOAD_LEN);
    out.extend_from_slice(&dx.to_le_bytes());
    out.extend_from_slice(&dy.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_move_decodes() {
        let payload = encode_move(0.25, -0.5);
        assert_eq!(decode_move(&payload), Ok((0.25, -0.5)));
    }

    #[test]
    fn a_short_payload_is_malformed() {
        assert_eq!(decode_move(&[0, 1, 2]), Err(REASON_MALFORMED));
    }

    #[test]
    fn clamp_axis_keeps_a_player_inside_the_field() {
        // Near the right wall, a big push only reaches the wall.
        assert_eq!(clamp_axis(3.4, 1.0), 3.5_f32 - 3.4);
        // Comfortably inside, the full delta passes through untouched.
        assert_eq!(clamp_axis(0.0, 0.5), 0.5);
        // Already at the wall, moving back toward centre is unrestricted.
        assert_eq!(clamp_axis(FIELD_BOUND, -1.0), -1.0);
        // The bound is symmetric on the left wall too.
        assert_eq!(clamp_axis(-3.4, -1.0), -3.5_f32 - -3.4);
    }

    #[test]
    fn an_oversized_delta_is_impossible() {
        let payload = encode_move(99.0, 0.0);
        assert_eq!(decode_move(&payload), Err(REASON_IMPOSSIBLE_MOVEMENT));
    }

    #[test]
    fn a_non_finite_delta_is_impossible() {
        let payload = encode_move(f32::NAN, 0.0);
        assert_eq!(decode_move(&payload), Err(REASON_IMPOSSIBLE_MOVEMENT));
    }

    #[test]
    fn player_move_addresses_the_player_on_the_xy_plane() {
        let input = player_move(3, 0.1, 0.2);
        assert_eq!(input.player, 3);
        assert_eq!(input.delta, Vec3::new(0.1, 0.2, 0.0));
    }
}
