//! The DOOM agent — driving the real game through the reusable `axiom-agent`
//! module. Native-only and gated behind the `agent` feature, so the wasm build
//! and the default workspace gates never compile it.
//! There is **no hand-rolled decision logic** here. Every tick the app:
//! 1. **observes** the live DOOM state into `axiom-agent`'s neutral observation,
//! 2. lets the agent substrate **decide** and emit player-equivalent intents
//!    (`AgentApi::step`, producing a real `DecisionReport`), and
//! 3. **lowers** the emitted neutral intent back into the DOOM [`Intent`] the
//!    engine consumes.
//! Per the Module Law the module never learns a game noun: the app owns both ends
//! of the translation, here. The agent expresses play as *discrete
//! player-equivalent controls* (forward / back / turn / strafe / fire), packed
//! into one neutral `control_code`. Continuous mouse-look and the debug teleport
//! are **not** agent concepts and live where they belong — `web.rs` drives
//! mouse-look for the live browser player, and `DoomGame::teleport` (used by
//! `axiom-shot --pose`) owns the debug pose snap.

use axiom::prelude::{FrameOutcome, RunningApp};
use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;
use serde::Serialize;

use crate::doom::{build_doom_app, DoomAssets, DoomGame, Hud, Intent};

/// An absolute player pose: world position `(x, z)`, look `yaw`/`pitch` (radians).
/// Returned in every [`Observation`] so a session always knows where it stands and
/// which way it looks — the readout that makes a view-dependent artifact
/// reproducible at an exact pose.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Pose {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
}

/// One action from the agent: hold these `keys` this step, optionally `fire`,
/// advance `steps` ticks, and optionally `render` an image. All fields default, so
/// `{}` is a valid idle step. The agent plays with discrete controls only; any
/// other JSON fields (e.g. a legacy `yaw`/`teleport`) are ignored.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Action {
    /// How many ticks to advance with this action (default 1).
    pub steps: Option<u32>,
    /// Held controls: `forward`, `backward`, `left`/`turn_left`,
    /// `right`/`turn_right`, `strafe_left`, `strafe_right`, `fire`.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Fire this step (same as listing `fire` in `keys`).
    #[serde(default)]
    pub fire: bool,
    /// Render an image of the resulting frame (needs the `agent-render` feature).
    #[serde(default)]
    pub render: bool,
}

/// The HUD slice an observation carries.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HudView {
    pub hp: i32,
    pub score: u32,
    pub ammo: u32,
    pub enemies: u32,
}

/// What the agent gets back: the structured state of the resulting frame, plus an
/// optional image path. `state_hash` is a deterministic fingerprint of the frame
/// (for replay/diffing).
#[derive(Debug, Clone, PartialEq, Serialize)]
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

/// The app's DOOM control bitmask: the meaning the app assigns to a neutral
/// `ActionIntent` `control_code`. `axiom-agent` carries the `u32` opaquely; this
/// is the app-side convention that packs DOOM's discrete held controls into it,
/// so a single `press_control` intent encodes a whole tick of input.
const CONTROL_FORWARD: u32 = 1 << 0;
const CONTROL_BACKWARD: u32 = 1 << 1;
const CONTROL_TURN_LEFT: u32 = 1 << 2;
const CONTROL_TURN_RIGHT: u32 = 1 << 3;
const CONTROL_STRAFE_LEFT: u32 = 1 << 4;
const CONTROL_STRAFE_RIGHT: u32 = 1 << 5;
const CONTROL_FIRE: u32 = 1 << 6;

/// Every DOOM control bit, in stable order — used to split a combined control
/// bitmask into one held control per set bit, so the agent emits a genuine
/// *multi-intent* decision (one `press_control` per active control) that the
/// agent substrate carries together and the queue recombines.
const CONTROL_BITS: [u32; 7] = [
    CONTROL_FORWARD,
    CONTROL_BACKWARD,
    CONTROL_TURN_LEFT,
    CONTROL_TURN_RIGHT,
    CONTROL_STRAFE_LEFT,
    CONTROL_STRAFE_RIGHT,
    CONTROL_FIRE,
];

/// Neutral observation-fact kinds the app fills (its own code vocabulary). The
/// agent's brain ignores the observation content, but the app builds a real
/// observation each tick so the `observe → decide` half of the loop is genuinely
/// exercised.
const FACT_PLAYER_POSE: u16 = 1;
const FACT_HUD: u16 = 2;

/// The stable agent id this single-agent session uses. `pub(crate)` so the
/// perception adapter ([`crate::doom::perception`]) drives the same agent identity.
pub(crate) const AGENT_RAW_ID: u64 = 1;

/// The engine's fixed 60 Hz step delta in integer nanoseconds (1/60 s). Stamps
/// the `RuntimeStep` that drives one decision; it does not affect the DOOM
/// simulation (which advances by whole ticks). `pub(crate)` for the perception
/// adapter's own `observe → decide` step.
pub(crate) const FIXED_DELTA_NANOS: u64 = 16_666_667;

/// Translate an agent action's `keys` + `fire` into a DOOM [`Intent`]'s discrete
/// held controls — the app's JSON-control vocabulary. (Engine-agnostic app I/O;
/// the resulting `Intent` carries no mouse-look, only discrete controls.)
fn action_to_intent(action: &Action) -> Intent {
    let mut intent = Intent {
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

/// Pack a DOOM [`Intent`]'s discrete held controls into the neutral control-code
/// bitmask — the app's encoding of one tick of input into a single
/// `press_control` intent. `pub(crate)` so the perception adapter encodes its
/// reactive intent through the same vocabulary.
pub(crate) fn control_code_of(intent: &Intent) -> u32 {
    (u32::from(intent.forward) * CONTROL_FORWARD)
        | (u32::from(intent.backward) * CONTROL_BACKWARD)
        | (u32::from(intent.turn_left) * CONTROL_TURN_LEFT)
        | (u32::from(intent.turn_right) * CONTROL_TURN_RIGHT)
        | (u32::from(intent.strafe_left) * CONTROL_STRAFE_LEFT)
        | (u32::from(intent.strafe_right) * CONTROL_STRAFE_RIGHT)
        | (u32::from(intent.fire) * CONTROL_FIRE)
}

/// Lower a neutral control-code bitmask back into the concrete DOOM [`Intent`]
/// the engine consumes — the inverse of [`control_code_of`]. A code of `0` (a
/// no-op intent past the end of a recording) decodes to an idle `Intent`.
/// `pub(crate)` so the perception adapter lowers its reactive intent identically.
pub(crate) fn intent_of_control_code(code: u32) -> Intent {
    Intent {
        forward: code & CONTROL_FORWARD != 0,
        backward: code & CONTROL_BACKWARD != 0,
        turn_left: code & CONTROL_TURN_LEFT != 0,
        turn_right: code & CONTROL_TURN_RIGHT != 0,
        strafe_left: code & CONTROL_STRAFE_LEFT != 0,
        strafe_right: code & CONTROL_STRAFE_RIGHT != 0,
        fire: code & CONTROL_FIRE != 0,
        look_yaw: 0.0,
        look_pitch: 0.0,
    }
}

/// A world-unit `f32` as fixed-point micro-units — the neutral observation-fact
/// coordinate convention (`axiom-agent` facts are integer/fixed-point only).
fn micro(value: f32) -> i64 {
    (f64::from(value) * 1_000_000.0) as i64
}

/// Run one `observe → decide → emit` cycle through `axiom-agent` and return the
/// lowered DOOM [`Intent`] to apply this tick.
/// All of `axiom-agent`'s neutral contracts (id, profile, observation, brain,
/// memory, queue, intent) are created and consumed here, held only by type
/// inference — the app never names a sealed `axiom-agent` type. The brain is a
/// one-shot replay of the action the controller commanded: the external action
/// *is* the decision, run through the substrate so it produces a real report and
/// emits a player-equivalent intent the app then lowers.
fn decide_intent(px: f32, pz: f32, yaw: f32, hud: Hud, control_code: u32, tick: u64) -> Intent {
    let agent_id = AgentApi::create_agent_id(AGENT_RAW_ID);
    let profile = AgentApi::debug_perfect_profile();
    // Split the commanded control bitmask into one held control per set bit, so
    // the agent emits a real multi-intent decision: `{"keys":["forward","left"]}`
    // becomes two `press_control` intents this tick, not one pre-combined intent.
    let held: Vec<u32> = CONTROL_BITS
        .iter()
        .copied()
        .filter(|bit| control_code & bit != 0)
        .collect();
    let mut brain = AgentApi::hold_set_brain(held);
    let mut memory = AgentApi::empty_memory(1);

    // Observe: translate the live DOOM state into a neutral observation.
    let mut builder = AgentApi::observation_builder(agent_id, Tick::new(tick), 1, 2, 0);
    builder
        .add_channel(AgentApi::channel_semantic())
        .expect("one channel within the channel bound");
    builder
        .add_fact(AgentApi::observation_fact(
            FACT_PLAYER_POSE,
            0,
            micro(px),
            0,
            micro(pz),
            micro(yaw),
        ))
        .expect("pose fact within the fact bound");
    builder
        .add_fact(AgentApi::observation_fact(
            FACT_HUD,
            0,
            i64::from(hud.health),
            i64::from(hud.score),
            i64::from(hud.ammo),
            i64::from(hud.enemies_alive),
        ))
        .expect("hud fact within the fact bound");
    let observation = builder.build();

    // Decide + emit: the substrate runs the brain and hands back a queue of
    // player-equivalent intents, stamped with this tick.
    let step = RuntimeStep::new(FrameIndex::new(0), Tick::new(tick), FIXED_DELTA_NANOS, 0);
    let (_report, mut queue) =
        AgentApi::step(agent_id, profile, &mut brain, &observation, &mut memory, step);

    // Recombine the tick's emitted intents into one held-control bitmask and lower
    // it back into the concrete DOOM Intent — N simultaneous controls applied
    // together (the OR is identical to the commanded `control_code`).
    intent_of_control_code(queue.combined_control_code())
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

/// Start a fresh DOOM game + engine app and advance one idle tick (tick 0), so
/// there is a frame to read.
fn doom_session_start() -> (DoomGame, RunningApp, DoomAssets, FrameOutcome) {
    let mut game = DoomGame::new();
    let (mut app, assets) = build_doom_app(&crate::doom::level::LevelDoc::default());
    // Bind the enemies to their engine Entities so hits classify from tick 0.
    game.bind_entities(&app);
    let last = doom_drive_tick(&mut game, &mut app, &assets, 0, Intent::default());
    (game, app, assets, last)
}

/// Drive one real DOOM frame from a concrete [`Intent`]: step the game against
/// the engine, apply its lifecycle commands (despawn killed / spawn revived
/// enemies), then tick the engine so its world tracks the game — the same path
/// `web.rs` drives for the live browser player. `pub(crate)` so the perception
/// adapter drives the identical game→engine path.
pub(crate) fn doom_drive_tick(
    game: &mut DoomGame,
    app: &mut RunningApp,
    assets: &DoomAssets,
    tick: u64,
    intent: Intent,
) -> FrameOutcome {
    let cmd = game.step(intent, &*app);
    crate::doom::apply_lifecycle(game, app, assets, &cmd);
    app.tick_with_controls(tick, &cmd.enemies, &[cmd.control])
}

/// The structured observation for a game + its most recent frame, reported as of
/// `tick` (the session's post-increment tick; the observation is for `tick - 1`).
fn doom_observation(game: &DoomGame, last: &FrameOutcome, tick: u64) -> Observation {
    let hud = game.hud();
    let (x, z, yaw, pitch) = game.pose();
    Observation {
        tick: tick.saturating_sub(1),
        pose: Pose { x, z, yaw, pitch },
        hud: HudView {
            hp: hud.health.max(0),
            score: hud.score,
            ammo: hud.ammo,
            enemies: hud.enemies_alive,
        },
        draw_count: last.draws().len(),
        state_hash: frame_hash(&last.instance_floats()),
        image: None,
    }
}

/// A live agent session: the real DOOM game plus its engine app, driven one
/// action at a time through `axiom-agent`. Its authority is identical to the
/// browser player's — the same `DoomGame::step` → `tick_with_controls` path — and
/// every tick's decision flows through `AgentApi::step`.
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
        let (game, app, assets, last) = doom_session_start();
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
    /// (structured only; the bin adds an image when asked and able). Each tick is
    /// a full `observe → decide → emit` cycle through `axiom-agent`.
    pub fn step(&mut self, action: &Action) -> Observation {
        let control_code = control_code_of(&action_to_intent(action));
        let steps = action.steps.unwrap_or(1).max(1);
        for _ in 0..steps {
            let (px, pz, yaw, _pitch) = self.game.pose();
            let intent = decide_intent(px, pz, yaw, self.game.hud(), control_code, self.tick);
            self.last =
                doom_drive_tick(&mut self.game, &mut self.app, &self.assets, self.tick, intent);
            self.tick += 1;
        }
        self.observe()
    }

    /// The current structured observation (no image).
    pub fn observe(&self) -> Observation {
        doom_observation(&self.game, &self.last, self.tick)
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
    fn action_keys_map_to_discrete_controls() {
        let action = Action {
            keys: vec!["forward".into(), "left".into()],
            fire: true,
            ..Action::default()
        };
        let intent = action_to_intent(&action);
        assert!(intent.forward && intent.turn_left && intent.fire);
        assert!(!intent.backward);
        // The agent carries no mouse-look — it turns via discrete controls.
        assert_eq!(intent.look_yaw, 0.0);
        assert_eq!(intent.look_pitch, 0.0);
    }

    #[test]
    fn control_code_round_trips_every_discrete_intent() {
        // Exhaustively round-trip all 2^7 discrete control combinations through
        // the bitmask: decode(encode(intent)) == intent for the discrete fields.
        for bits in 0u32..128 {
            let original = Intent {
                forward: bits & 1 != 0,
                backward: bits & 2 != 0,
                turn_left: bits & 4 != 0,
                turn_right: bits & 8 != 0,
                strafe_left: bits & 16 != 0,
                strafe_right: bits & 32 != 0,
                fire: bits & 64 != 0,
                ..Intent::default()
            };
            let decoded = intent_of_control_code(control_code_of(&original));
            assert_eq!(decoded.forward, original.forward);
            assert_eq!(decoded.backward, original.backward);
            assert_eq!(decoded.turn_left, original.turn_left);
            assert_eq!(decoded.turn_right, original.turn_right);
            assert_eq!(decoded.strafe_left, original.strafe_left);
            assert_eq!(decoded.strafe_right, original.strafe_right);
            assert_eq!(decoded.fire, original.fire);
        }
    }

    #[test]
    fn a_fixed_action_script_replays_to_the_same_state_hash() {
        let script = [
            Action {
                keys: vec!["forward".into()],
                ..Action::default()
            },
            Action {
                keys: vec!["turn_left".into(), "forward".into()],
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
        assert!(best > start, "an agent playing DOOM through axiom-agent scores kills");
    }

    #[test]
    fn observation_serializes_without_an_image_by_default() {
        let s = AgentSession::new();
        let json = serde_json::to_string(&s.observe()).unwrap();
        assert!(json.contains("state_hash"));
        assert!(!json.contains("image"), "image omitted when not rendered");
    }
}
