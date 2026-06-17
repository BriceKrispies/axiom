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

use crate::{build_doom_app, DoomGame, Intent};

#[cfg(feature = "agent-render")]
mod render;

/// One action from the agent: hold these `keys` this step, apply a mouse-look
/// `yaw`/`pitch` delta, optionally `fire`, advance `steps` ticks, and optionally
/// `render` an image. All fields default, so `{}` is a valid idle step.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Action {
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
pub struct AgentSession {
    game: DoomGame,
    app: RunningApp,
    tick: u64,
    last: FrameOutcome,
}

impl AgentSession {
    /// Start a fresh game and advance one idle tick so there is a frame to read.
    pub fn new() -> Self {
        let mut game = DoomGame::new();
        let mut app = build_doom_app();
        let cmd = game.step(Intent::default());
        let last = app.tick_with_controls(0, &cmd.enemies, &[cmd.control]);
        AgentSession {
            game,
            app,
            tick: 1,
            last,
        }
    }

    /// Reset to a fresh game.
    pub fn reset(&mut self) {
        *self = AgentSession::new();
    }

    /// Apply `action` for `steps` ticks and return the resulting observation.
    pub fn step(&mut self, action: &Action) -> Observation {
        let intent = action_to_intent(action);
        let steps = action.steps.unwrap_or(1).max(1);
        for _ in 0..steps {
            let cmd = self.game.step(intent);
            self.last = self
                .app
                .tick_with_controls(self.tick, &cmd.enemies, &[cmd.control]);
            self.tick += 1;
        }
        self.observe(action.render)
    }

    /// The current observation without stepping.
    pub fn observe(&self, render: bool) -> Observation {
        let hud = self.game.hud();
        let image = if render { self.render_image() } else { None };
        Observation {
            tick: self.tick.saturating_sub(1),
            hud: HudView {
                hp: hud.health.max(0),
                score: hud.score,
                ammo: hud.ammo,
                enemies: hud.enemies_alive,
            },
            draw_count: self.last.draws().len(),
            state_hash: frame_hash(&self.last.instance_floats()),
            image,
        }
    }

    /// Render the current frame to a PNG and return its path. Without the
    /// `agent-render` feature there is no renderer, so this is always `None`.
    #[cfg(feature = "agent-render")]
    fn render_image(&self) -> Option<String> {
        render::render_frame(&self.app, &self.last, self.tick.saturating_sub(1))
    }

    #[cfg(not(feature = "agent-render"))]
    fn render_image(&self) -> Option<String> {
        None
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
        // Walk forward into the room and fire down the lane a few times; with
        // enemies chasing, some shot lands and the score climbs.
        let mut s = AgentSession::new();
        let start = s.step(&Action::default()).hud.score;
        let mut best = start;
        for _ in 0..240 {
            let obs = s.step(&Action {
                keys: vec!["forward".into()],
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
        let json = serde_json::to_string(&s.observe(false)).unwrap();
        assert!(json.contains("state_hash"));
        assert!(!json.contains("image"), "image omitted when not rendered");
    }
}
