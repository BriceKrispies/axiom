//! The players-and-controllers arm of the [`SceneApi`] facade: marking nodes as
//! command-driven players or first-person controllers and encoding their
//! per-tick input commands. A child module so neither `impl SceneApi` block
//! exceeds the engine's impl-block size budget.

use axiom_frame::FrameCommand;
use axiom_kernel::{Meters, Radians};
use axiom_math::Vec3;

use super::SceneApi;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;

impl SceneApi {
    /// Mark `node` as the controllable node for `player` index. Per-tick move
    /// commands addressed to that index translate it during [`Self::advance`].
    pub fn add_player(&mut self, node: SceneNodeId, player: u32) -> SceneResult<()> {
        self.scene.add_player(node, player)
    }

    /// The world-space translation of the node marked with `player` index, if
    /// any. A read-only projection of authoritative scene state — the value an
    /// authoritative server reads to broadcast a renderable view to clients,
    /// without keeping a parallel position mirror.
    pub fn player_translation(&self, player: u32) -> Option<Vec3> {
        self.scene.player_translation(player)
    }

    /// Encode a per-tick move for `player` by `delta` (a translation delta) as a
    /// [`FrameCommand`] to hand to the frame builder. The scene decodes these in
    /// [`Self::advance`] and applies them to the addressed player's node.
    pub fn move_command(&self, sequence: u64, player: u32, delta: Vec3) -> FrameCommand {
        crate::player_command::encode_move(sequence, player, delta)
    }

    /// Mark `node` as the first-person controller for `index`. Per-tick
    /// controller commands addressed to that index yaw it about +Y and move it
    /// along its own facing during [`Self::advance`].
    pub fn add_controller(&mut self, node: SceneNodeId, index: u32) -> SceneResult<()> {
        self.scene.add_controller(node, index)
    }

    /// Apply one first-person input to controller `index` **immediately** — yaw by
    /// `yaw` and pitch by `pitch` (clamped), then move by `move_local` in the
    /// node's yaw-only frame — and recompute world transforms now. The zero-lag
    /// counterpart to staging a [`Self::controller_command`] for [`Self::advance`]:
    /// a host that owns its own loop drives the camera with this between ticks. An
    /// unknown index is a no-op. When `seat_y` is present the eye is seated at that
    /// absolute height instead of taking `move_local`'s vertical component.
    pub fn control_now(
        &mut self,
        index: u32,
        move_local: Vec3,
        yaw: Radians,
        pitch: Radians,
        seat_y: Option<Meters>,
    ) {
        self.scene
            .control_now(index, move_local, yaw.get(), pitch.get(), seat_y);
    }

    /// Encode a per-tick first-person input for controller `index`: a `yaw`/`pitch`
    /// look delta (yaw about +Y, pitch about local +X, clamped by the scene) plus
    /// a `move_local` translation in the node's own frame (local -Z is forward,
    /// local +X is right) and an optional absolute vertical `seat_y` (metres), as a
    /// [`FrameCommand`] to hand to the frame builder. The scene decodes these in
    /// [`Self::advance`] and applies them to the addressed controller's node —
    /// moving in the yaw-only horizontal frame, seating the eye when `seat_y` is
    /// present.
    pub fn controller_command(
        &self,
        sequence: u64,
        index: u32,
        move_local: Vec3,
        yaw: Radians,
        pitch: Radians,
        seat_y: Option<Meters>,
    ) -> FrameCommand {
        crate::controller_command::encode_controller(
            sequence,
            index,
            move_local,
            yaw.get(),
            pitch.get(),
            seat_y,
        )
    }
}
