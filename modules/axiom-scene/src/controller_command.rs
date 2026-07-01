//! The wire shape of a per-tick first-person controller input, as a
//! [`FrameCommand`].
//!
//! Mirrors [`crate::player_command`] but carries an orientation + local-move
//! triple instead of a world translation: a controller-marked node (a
//! [`crate::scene_storage::ControllerSystem`] target) is addressed by a small
//! index, and each tick an app submits `(forward, strafe, turn)`.
//! [`crate::SceneApi::controller_command`] encodes one and
//! [`crate::scene::Scene::advance`] decodes them into the controller queue. The
//! encode/decode pair lives here so the format has a single source of truth.

use axiom_frame::FrameCommand;
use axiom_kernel::{BinaryReader, BinaryWriter};
use axiom_math::Vec3;

/// The `FrameCommand::kind` tag identifying a first-person controller command.
/// Distinct from [`crate::player_command::MOVE_KIND`] so the two input kinds
/// coexist on one frame.
pub(crate) const CONTROLLER_KIND: u32 = 2;

/// Encode a first-person input for controller `index`: a `yaw`/`pitch` look
/// delta (radians; yaw about +Y, pitch about local +X) plus a `move_local`
/// translation in the node's own frame (local -Z is forward, local +X is right),
/// as a frame command. `sequence` is frame bookkeeping and is not interpreted by
/// the scene.
pub(crate) fn encode_controller(
    sequence: u64,
    index: u32,
    move_local: Vec3,
    yaw: f32,
    pitch: f32,
) -> FrameCommand {
    let mut w = BinaryWriter::new();
    w.write_u32(index);
    w.write_f32(move_local.x);
    w.write_f32(move_local.y);
    w.write_f32(move_local.z);
    w.write_f32(yaw);
    w.write_f32(pitch);
    FrameCommand::new(sequence, CONTROLLER_KIND, w.into_bytes())
}

/// Decode a controller command into `(index, move_local, yaw, pitch)`, or `None`
/// if it is not a well-formed controller input (wrong kind, or a truncated
/// payload).
pub(crate) fn decode_controller(command: &FrameCommand) -> Option<(u32, Vec3, f32, f32)> {
    // Reads are sequential on a stateful reader, so each field read nests inside
    // the previous one's success arm (`and_then`); a wrong kind or any truncated
    // field collapses the whole chain to `None`.
    (command.kind() == CONTROLLER_KIND)
        .then(|| BinaryReader::new(command.payload()))
        .and_then(|mut r| {
            r.read_u32().ok().and_then(|index| {
                r.read_f32().ok().and_then(|x| {
                    r.read_f32().ok().and_then(|y| {
                        r.read_f32().ok().and_then(|z| {
                            r.read_f32().ok().and_then(|yaw| {
                                r.read_f32()
                                    .ok()
                                    .map(|pitch| (index, Vec3::new(x, y, z), yaw, pitch))
                            })
                        })
                    })
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_round_trips() {
        let cmd = encode_controller(0, 2, Vec3::new(0.5, 0.0, -0.25), 0.1, -0.2);
        assert_eq!(cmd.kind(), CONTROLLER_KIND);
        assert_eq!(
            decode_controller(&cmd),
            Some((2, Vec3::new(0.5, 0.0, -0.25), 0.1, -0.2))
        );
    }

    #[test]
    fn non_controller_kind_decodes_to_none() {
        let other = FrameCommand::new(0, CONTROLLER_KIND + 1, vec![0; 16]);
        assert_eq!(decode_controller(&other), None);
    }

    #[test]
    fn every_truncated_prefix_decodes_to_none() {
        // Walks the `ok()?` error arm of each field read (index, forward, strafe,
        // turn).
        let full = encode_controller(0, 1, Vec3::new(0.5, 0.0, 0.25), -0.5, 0.3);
        let bytes = full.payload().to_vec();
        for k in 0..bytes.len() {
            let cmd = FrameCommand::new(0, CONTROLLER_KIND, bytes[..k].to_vec());
            assert_eq!(decode_controller(&cmd), None, "prefix len {k} must fail");
        }
        assert!(decode_controller(&full).is_some());
    }
}
