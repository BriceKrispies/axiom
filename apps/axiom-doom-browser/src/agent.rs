//! The native agent bridge: drive the real DOOM game from JSON and read back
//! structured state (and, with `agent-render`, images). Native-only and gated
//! behind the `agent` feature, so the wasm build and the default workspace gates
//! never compile it.
//!
//! The engine is a pure function of `(tick, inputs)`, so this just feeds the same
//! `Intent` the keyboard would and reports the resulting [`FrameOutcome`] / HUD —
//! no engine changes, the agent's events are indistinguishable from a player's.

use axiom::prelude::{FrameOutcome, RunningApp};
use serde::{Deserialize, Serialize};

use crate::{build_doom_app, DoomAssets, DoomGame, Intent};

/// An absolute player pose: world position `(x, z)`, look `yaw`/`pitch` (radians).
/// Sent in an [`Action`]'s `teleport` to stand the player somewhere exactly, and
/// returned in every [`Observation`] so a session always knows where it is and
/// which way it looks — the readout that makes a view-dependent artifact
/// reproducible at an exact pose.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pose {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
}

/// One action from the agent: optionally `teleport` to an absolute pose first,
/// then hold these `keys` this step, apply a mouse-look `yaw`/`pitch` delta,
/// optionally `fire`, advance `steps` ticks, and optionally `render` an image. All
/// fields default, so `{}` is a valid idle step.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Action {
    /// Teleport to this absolute pose before stepping (a debug "stand here, look
    /// there" — ignores walls). `None` leaves the player where they are.
    #[serde(default)]
    pub teleport: Option<Pose>,
    /// How many ticks to advance with this action (default 1).
    pub steps: Option<u32>,
    /// Held movement keys: `forward`, `backward`, `left`, `right`,
    /// `strafe_left`, `strafe_right`, `fire`.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Mouse-look yaw delta (radians, +left) applied this step.
    #[serde(default)]
    pub yaw: f32,
    /// Mouse-look pitch delta (radians, +up) applied this step.
    #[serde(default)]
    pub pitch: f32,
    /// Fire this step (same as listing `fire` in `keys`).
    #[serde(default)]
    pub fire: bool,
    /// Render an image of the resulting frame (needs the `agent-render` feature).
    #[serde(default)]
    pub render: bool,
}

/// The HUD slice an observation carries.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HudView {
    pub hp: i32,
    pub score: u32,
    pub ammo: u32,
    pub enemies: u32,
}

/// What the agent gets back: the structured state of the resulting frame, plus an
/// optional image path. `state_hash` is a deterministic fingerprint of the frame
/// (for replay/diffing).
#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub tick: u64,
    /// Where the player stands + looks after this step (the reproducibility readout).
    pub pose: Pose,
    pub hud: HudView,
    pub draw_count: usize,
    pub state_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// Build the `Intent` the engine consumes from an agent action.
fn action_to_intent(action: &Action) -> Intent {
    let mut intent = Intent {
        look_yaw: action.yaw,
        look_pitch: action.pitch,
        fire: action.fire,
        ..Intent::default()
    };
    for key in &action.keys {
        match key.as_str() {
            "forward" | "up" => intent.forward = true,
            "backward" | "back" | "down" => intent.backward = true,
            "left" | "turn_left" => intent.turn_left = true,
            "right" | "turn_right" => intent.turn_right = true,
            "strafe_left" => intent.strafe_left = true,
            "strafe_right" => intent.strafe_right = true,
            "fire" => intent.fire = true,
            _ => {}
        }
    }
    intent
}

/// A deterministic FNV-1a fingerprint of the frame's packed instance floats —
/// stable across runs of the same build, so a fixed action script always hashes
/// the same (the basis for replay/diffing).
fn frame_hash(floats: &[f32]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in floats {
        for b in f.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// A live agent session: the real DOOM game plus its engine app, driven one
/// action at a time. The agent's authority is identical to the browser's — same
/// `DoomGame::step` → `tick_with_controls` path as `web.rs`.
#[derive(Debug)]
pub struct AgentSession {
    game: DoomGame,
    app: RunningApp,
    assets: DoomAssets,
    tick: u64,
    last: FrameOutcome,
}

impl AgentSession {
    /// Start a fresh game and advance one idle tick so there is a frame to read.
    pub fn new() -> Self {
        let mut game = DoomGame::new();
        let (mut app, assets) = build_doom_app(&crate::level::LevelDoc::default());
        // Bind the enemies to their engine Entities so hits classify from tick 0.
        game.bind_entities(&app);
        let cmd = game.step(Intent::default(), &app);
        crate::apply_lifecycle(&mut game, &mut app, &assets, &cmd);
        let last = app.tick_with_controls(0, &cmd.enemies, &[cmd.control]);
        AgentSession {
            game,
            app,
            assets,
            tick: 1,
            last,
        }
    }

    /// Reset to a fresh game.
    pub fn reset(&mut self) {
        *self = AgentSession::new();
    }

    /// Apply `action` for `steps` ticks and return the resulting observation
    /// (structured only; the bin adds an image when asked and able).
    pub fn step(&mut self, action: &Action) -> Observation {
        // A teleport is a debug "stand here, look there" snap: set the player pose
        // and apply the one corrective control that jumps the engine camera there,
        // idling a tick (no enemy moves, no movement keys) so the frame reflects
        // the new pose. Render the result to screenshot an exact view.
        if let Some(p) = action.teleport {
            let control = self.game.teleport(p.x, p.z, p.yaw, p.pitch);
            self.last = self.app.tick_with_controls(self.tick, &[], &[control]);
            self.tick += 1;
            return self.observe();
        }
        let intent = action_to_intent(action);
        let steps = action.steps.unwrap_or(1).max(1);
        for _ in 0..steps {
            let cmd = self.game.step(intent, &self.app);
            crate::apply_lifecycle(&mut self.game, &mut self.app, &self.assets, &cmd);
            self.last = self
                .app
                .tick_with_controls(self.tick, &cmd.enemies, &[cmd.control]);
            self.tick += 1;
        }
        self.observe()
    }

    /// The current structured observation (no image).
    pub fn observe(&self) -> Observation {
        let hud = self.game.hud();
        let (x, z, yaw, pitch) = self.game.pose();
        Observation {
            tick: self.tick.saturating_sub(1),
            pose: Pose { x, z, yaw, pitch },
            hud: HudView {
                hp: hud.health.max(0),
                score: hud.score,
                ammo: hud.ammo,
                enemies: hud.enemies_alive,
            },
            draw_count: self.last.draws().len(),
            state_hash: frame_hash(&self.last.instance_floats()),
            image: None,
        }
    }

    /// The current tick (the last advanced).
    pub fn tick(&self) -> u64 {
        self.tick.saturating_sub(1)
    }

    /// The most recent frame, for the bin's offscreen renderer.
    pub fn frame(&self) -> &FrameOutcome {
        &self.last
    }

    /// The cube vertex stream, its indices, and the per-frame instance capacity —
    /// the geometry the offscreen renderer uploads. (Kept in the lib so the bin's
    /// wgpu code, which the cdylib must not link, stays out of the lib.)
    pub fn geometry(&self) -> (Vec<f32>, Vec<u32>, u32) {
        let (vertices, indices) = self.app.mesh_vertex_stream();
        (vertices, indices, self.app.renderable_count() as u32)
    }
}

impl Default for AgentSession {
    fn default() -> Self {
        AgentSession::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_keys_and_look_map_to_intent() {
        let action = Action {
            keys: vec!["forward".into(), "left".into()],
            yaw: 0.2,
            pitch: -0.1,
            fire: true,
            ..Action::default()
        };
        let intent = action_to_intent(&action);
        assert!(intent.forward && intent.turn_left && intent.fire);
        assert_eq!(intent.look_yaw, 0.2);
        assert_eq!(intent.look_pitch, -0.1);
        assert!(!intent.backward);
    }

    #[test]
    fn a_fixed_action_script_replays_to_the_same_state_hash() {
        let script = [
            Action {
                keys: vec!["forward".into()],
                ..Action::default()
            },
            Action {
                yaw: 0.1,
                keys: vec!["forward".into()],
                ..Action::default()
            },
            Action {
                fire: true,
                ..Action::default()
            },
        ];
        let run = || {
            let mut s = AgentSession::new();
            let mut last = String::new();
            for a in &script {
                last = s.step(a).state_hash;
            }
            last
        };
        assert_eq!(run(), run(), "determinism: same actions -> same frame hash");
    }

    #[test]
    fn firing_at_a_lined_up_enemy_raises_the_score() {
        // Spin in place and fire: the enemies chase in to melee range and the
        // sweeping aim cone catches them, so the score climbs. (Deterministic, so
        // the budget is fixed; kept under the ~300-tick death window so a respawn
        // can't reset the score mid-test.)
        let mut s = AgentSession::new();
        let start = s.step(&Action::default()).hud.score;
        let mut best = start;
        for _ in 0..250 {
            let obs = s.step(&Action {
                keys: vec!["left".into()],
                fire: true,
                ..Action::default()
            });
            best = best.max(obs.hud.score);
        }
        assert!(best > start, "an agent playing DOOM can score kills");
    }

    #[test]
    fn observation_serializes_without_an_image_by_default() {
        let s = AgentSession::new();
        let json = serde_json::to_string(&s.observe()).unwrap();
        assert!(json.contains("state_hash"));
        assert!(!json.contains("image"), "image omitted when not rendered");
    }
}
