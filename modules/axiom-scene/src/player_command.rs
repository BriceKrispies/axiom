//! The wire shape of a per-tick player-move input, as a [`FrameCommand`].
//!
//! A controllable node (a [`crate::scene_storage::PlayerMoveSystem`] target) is
//! addressed by a small player index. Each tick an app submits a move delta for
//! its player as a frame command; [`crate::SceneApi::move_command`] encodes one
//! and [`crate::scene::Scene::advance`] decodes them into the move queue. The
//! encode/decode pair lives here so the format has a single source of truth.

use axiom_frame::FrameCommand;
use axiom_kernel::{BinaryReader, BinaryWriter};
use axiom_math::Vec3;

/// The `FrameCommand::kind` tag identifying a player-move command.
pub(crate) const MOVE_KIND: u32 = 1;

/// Encode a move for `player` by `delta` (a translation delta) as a frame
/// command. `sequence` is frame bookkeeping and is not interpreted by the scene.
pub(crate) fn encode_move(sequence: u64, player: u32, delta: Vec3) -> FrameCommand {
    let mut w = BinaryWriter::new();
    w.write_u32(player);
    w.write_f32(delta.x);
    w.write_f32(delta.y);
    w.write_f32(delta.z);
    FrameCommand::new(sequence, MOVE_KIND, w.into_bytes())
}

/// Decode a move command into `(player, delta)`, or `None` if it is not a
/// well-formed move (wrong kind, or a truncated payload).
pub(crate) fn decode_move(command: &FrameCommand) -> Option<(u32, Vec3)> {
    // Reads are sequential on a stateful reader, so each field read nests inside
    // the previous one's success arm (`and_then`); a wrong kind or any truncated
    // field collapses the whole chain to `None`.
    (command.kind() == MOVE_KIND)
        .then(|| BinaryReader::new(command.payload()))
        .and_then(|mut r| {
            r.read_u32().ok().and_then(|player| {
                r.read_f32().ok().and_then(|x| {
                    r.read_f32()
                        .ok()
                        .and_then(|y| r.read_f32().ok().map(|z| (player, Vec3::new(x, y, z))))
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_round_trips() {
        let cmd = encode_move(0, 2, Vec3::new(0.5, -1.5, 0.0));
        assert_eq!(cmd.kind(), MOVE_KIND);
        assert_eq!(decode_move(&cmd), Some((2, Vec3::new(0.5, -1.5, 0.0))));
    }

    #[test]
    fn non_move_kind_decodes_to_none() {
        let other = FrameCommand::new(0, MOVE_KIND + 1, vec![0; 16]);
        assert_eq!(decode_move(&other), None);
    }

    #[test]
    fn every_truncated_prefix_decodes_to_none() {
        // Walks the `?`/`ok()?` error arm of each field read (player, x, y, z).
        let full = encode_move(0, 1, Vec3::new(0.5, 0.25, -0.5));
        let bytes = full.payload().to_vec();
        for k in 0..bytes.len() {
            let cmd = FrameCommand::new(0, MOVE_KIND, bytes[..k].to_vec());
            assert_eq!(decode_move(&cmd), None, "prefix len {k} must fail");
        }
        // The full payload decodes.
        assert!(decode_move(&full).is_some());
    }
}
