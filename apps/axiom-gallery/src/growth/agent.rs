//! The growth **agent driver** — walking the headless [`GroundSim`] through the
//! reusable [`axiom_agent_harness::AgentHarnessApi`]. Native-only and gated behind
//! the `agent` feature, so the wasm build and the default workspace gates never
//! compile it.
//! There is no hand-rolled decision logic here. Every tick the driver:
//! 1. **observes** the live ground state (the player's pose **and height**, and the
//!    summit as the goal) into the harness as fixed-point micro-units,
//! 2. lets the harness **decide** through `axiom-agent` (hold the requested
//!    controls, or seek the summit) and hand back a held-control bitmask, and
//! 3. **lowers** that bitmask into ground-sim movement axes and steps the sim.
//! The same reusable harness drives any first-person Axiom game; this module is
//! the growth-specific ends of the translation (game state → neutral numbers, and
//! neutral controls → ground-sim axes), which is exactly where the Module Law
//! keeps it.

use axiom_agent_harness::AgentHarnessApi;
use axiom_introspect::IntrospectApi;
use serde::{Deserialize, Serialize};

use crate::growth::ground::{CaptureInputs, GroundSim, MOVE_SPEED};
use crate::growth::presets::PlanetPreset;
use crate::growth::world_tags;

/// The agent's stable id ("growth" in ASCII) — deterministic, like everything else.
const AGENT_RAW_ID: u64 = 0x67_72_6f_77_74_68;

/// A world-unit `f32` as fixed-point micro-units — the harness's integer
/// observation-coordinate convention.
fn micro(value: f32) -> i64 {
    (f64::from(value) * 1_000_000.0) as i64
}

/// One action the driver applies: which neutral held controls to drive (by name)
/// and for how many ticks, or — if `seek` — let the agent navigate to the summit.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Action {
    /// How many ticks to advance with this action (default 1).
    pub steps: Option<u32>,
    /// Held controls: `forward`, `backward`, `turn_left`/`left`,
    /// `turn_right`/`right`, `strafe_left`, `strafe_right`.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Ignore `keys` and let the agent **seek** the summit (turn-toward + forward).
    #[serde(default)]
    pub seek: bool,
}

impl Action {
    /// Hold "forward" for one tick — the literal climb input.
    pub fn forward() -> Self {
        Action {
            steps: Some(1),
            keys: vec!["forward".to_string()],
            seek: false,
        }
    }

    /// Seek the summit for one tick.
    pub fn seek() -> Self {
        Action {
            steps: Some(1),
            keys: Vec::new(),
            seek: true,
        }
    }
}

/// What the driver reports after a step (serde for the HTTP / bridge JSON). The
/// **height** read-outs are the answer to "can the agent report player height":
/// they are filled from the agent's own observation of the player.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Observation {
    pub tick: u64,
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    /// Absolute terrain height under the player (metres).
    pub ground_height_m: f32,
    /// Absolute eye height (metres).
    pub eye_height_m: f32,
    /// Height above the flat spawn shelf (metres) — 0 at the spawn, ≈ prominence
    /// at the summit.
    pub height_above_spawn_m: f32,
    /// Horizontal distance to the summit (metres).
    pub distance_to_peak_m: f32,
    /// Absolute summit altitude (metres).
    pub peak_height_m: f32,
    /// The mountain prominence / relief (metres).
    pub prominence_m: f32,
    /// Whether the player has effectively reached the top.
    pub reached_summit: bool,
    /// The held-control bitmask the agent decided this tick.
    pub control_code: u32,
    /// The agent decision reason code (`axiom-agent` vocabulary).
    pub reason_code: u16,
}

/// A live agent session: the headless ground sim, driven one action at a time
/// through `axiom-agent-harness`. Its authority is identical to the browser
/// player's — the same `step_first_person` → `tick_with_controls` path.
#[derive(Debug)]
pub struct AgentSession {
    sim: GroundSim,
    /// The agent-interrogable surface holding the world's semantic tags (the
    /// nouns). Directive targets and the seek goal are resolved by name through
    /// it — "use more introspect".
    introspect: IntrospectApi,
    /// The accumulated tag set (runtime + any authored), kept so authored tags
    /// can be merged in and re-observed without losing the runtime ones.
    tags: Vec<axiom_introspect::WorldTag>,
    last_control: u32,
    last_reason: u16,
}

impl AgentSession {
    /// Start a session on a fresh planet at the map pick `(u, v)`. Registers the
    /// runtime tags the generated vista yields (`mountaintop`, `ground`) into the
    /// introspection surface so commands can be resolved by name.
    pub fn new(seed: &str, preset: PlanetPreset, sites: u32, u: f32, v: f32) -> Self {
        let sim = GroundSim::new(seed, preset, sites, u, v);
        let tags = world_tags::runtime_tags(&sim);
        let mut introspect = IntrospectApi::new(1);
        introspect.observe_tags(&tags);
        AgentSession {
            sim,
            introspect,
            tags,
            last_control: 0,
            last_reason: 0,
        }
    }

    /// Merge **authored** TOML tags (the static-world source) on top of the
    /// runtime tags and re-observe the union, so both sources answer queries.
    pub fn register_toml_tags(&mut self, toml_str: &str) {
        self.tags.extend(world_tags::toml_tags(&self.sim, toml_str));
        self.introspect.observe_tags(&self.tags);
    }

    /// The seek/observation goal point in micro-units, resolved from the
    /// `mountaintop` tag through introspect (falling back to the sim's peak if the
    /// tag is somehow absent).
    fn goal_micro(&self) -> (i64, i64, i64) {
        self.introspect
            .tag_by_name("mountaintop")
            .map(|tag| (tag.x(), tag.y(), tag.z()))
            .unwrap_or_else(|| {
                let (px, pz) = self.sim.peak_xz();
                (micro(px), micro(self.sim.peak_height_m()), micro(pz))
            })
    }

    /// A default Earthlike session at a mid-latitude land pick — the climb demo's
    /// starting point.
    pub fn earthlike() -> Self {
        Self::new("growth-agent", PlanetPreset::Earthlike, 4096, 0.5, 0.42)
    }

    /// Apply `action` for its `steps` ticks (each a full observe → decide → lower
    /// → step cycle through the harness) and return the resulting observation.
    pub fn step(&mut self, action: &Action) -> Observation {
        let steps = action.steps.unwrap_or(1).max(1);
        for _ in 0..steps {
            let (x, z, yaw, _pitch) = self.sim.pose();
            let self_pose = (micro(x), micro(self.sim.ground_height_m()), micro(z), micro(yaw));
            let goal = self.goal_micro();

            let (control, reason) = if action.seek {
                let (fx, fz) = self.sim.forward_xz();
                let (control, reason, _brain, _emitted) = AgentHarnessApi::decide_seek(
                    AGENT_RAW_ID,
                    self.sim.tick(),
                    self_pose,
                    (micro(fx), micro(fz)),
                    goal,
                    micro(2.0),
                );
                (control, reason)
            } else {
                let held = control_of_keys(&action.keys);
                let (control, reason, _brain, _emitted) =
                    AgentHarnessApi::decide_hold(AGENT_RAW_ID, self.sim.tick(), self_pose, goal, held);
                (control, reason)
            };

            let (forward_axis, strafe_axis, turn_axis) = axes_of_control(control);
            self.sim.step(forward_axis, strafe_axis, turn_axis);
            self.last_control = control;
            self.last_reason = reason;
        }
        self.observe()
    }

    /// What the agent **perceives** of the live world this tick — the slope ahead
    /// (with a real distance) and the landmarks (the summit, the spawn) in view,
    /// sensed through the reusable, game-agnostic `axiom-perception` model. The
    /// same module the DOOM agent uses; growth only supplies the heightfield probe
    /// (see [`crate::growth::perception`]).
    pub fn sight(&self) -> crate::growth::perception::Sight {
        crate::growth::perception::sense_sim(&self.sim)
    }

    /// The current observation without stepping.
    pub fn observe(&self) -> Observation {
        let (x, z, yaw, _pitch) = self.sim.pose();
        Observation {
            tick: self.sim.tick(),
            x,
            z,
            yaw,
            ground_height_m: self.sim.ground_height_m(),
            eye_height_m: self.sim.eye_height_m(),
            height_above_spawn_m: self.sim.height_above_spawn_m(),
            distance_to_peak_m: self.sim.distance_to_peak_m(),
            peak_height_m: self.sim.peak_height_m(),
            prominence_m: self.sim.prominence_m(),
            reached_summit: self.sim.reached_summit(),
            control_code: self.last_control,
            reason_code: self.last_reason,
        }
    }

    /// Restart the session on the same planet/pick (a fresh climb). Re-registers
    /// the runtime tags via [`Self::new`].
    pub fn reset(&mut self) {
        *self = Self::new("growth-agent", PlanetPreset::Earthlike, 4096, 0.5, 0.42);
    }

    /// Whether the player has reached the top.
    pub fn reached_summit(&self) -> bool {
        self.sim.reached_summit()
    }

    /// Gather the render inputs for a portrait of the mountain from a vantage
    /// `distance` m out along the outward unit direction `(dir_x, dir_z)`.
    pub fn capture_portrait(
        &mut self,
        dir_x: f32,
        dir_z: f32,
        distance: f32,
    ) -> CaptureInputs {
        self.sim.capture_portrait(dir_x, dir_z, distance)
    }

    /// Gather the render inputs for the legacy summit look-down toward the spawn.
    pub fn capture_summit_lookdown(&mut self) -> CaptureInputs {
        self.sim.capture_summit_lookdown()
    }

    /// The generated planet's seed (telemetry).
    pub fn seed(&self) -> u64 {
        self.sim.seed()
    }

    /// Whether the player has reached the top (passthrough).
    pub fn reached_summit_now(&self) -> bool {
        self.sim.reached_summit()
    }


    /// Run a parsed directive **script** — the data form of a command like "walk
    /// to the mountaintop, look at the ground, take a screenshot". Each directive
    /// is resolved against the world's tags (the nouns) through introspect and
    /// executed via the reusable agent verbs; the only growth-specific ends are
    /// stepping the sim and producing the capture pixels.
    /// Returns one [`CaptureRequest`] per `capture` directive — the label to save
    /// under plus the neutral render inputs the bin renders (rendering is a GPU
    /// concern kept out of the lib, exactly like the other capture paths).
    pub fn run_directives(&mut self, script: &DirectiveFile) -> Vec<CaptureRequest> {
        let mut captures = Vec::new();
        // The point the next capture aims at, set by a `look_at` directive.
        let mut look_at: Option<(f32, f32, f32)> = None;
        for directive in &script.directive {
            match directive.verb.as_str() {
                "goto" => self.run_goto(directive.required_target()),
                "look_at" | "lookat" => {
                    look_at = Some(self.resolve_point(directive.required_target()));
                }
                "capture" | "screenshot" => {
                    let (tx, ty, tz) = look_at.unwrap_or_else(|| self.resolve_point("mountaintop"));
                    captures.push(CaptureRequest {
                        label: directive.label.clone().unwrap_or_else(|| "capture".to_string()),
                        inputs: self.sim.capture_lookat(tx, ty, tz),
                    });
                }
                "wait" => {
                    for _ in 0..directive.steps.unwrap_or(1).max(1) {
                        self.sim.step(0.0, 0.0, 0.0);
                    }
                }
                other => panic!("unknown directive verb: {other:?}"),
            }
        }
        captures
    }

    /// Drive the agent's `move_toward` verb to a named target until the harness
    /// reports it has arrived (or a tick cap / the summit backstop trips).
    fn run_goto(&mut self, target: &str) {
        let goal = {
            let tag = self
                .introspect
                .tag_by_name(target)
                .unwrap_or_else(|| panic!("goto: no world tag named {target:?}"));
            (tag.x(), tag.y(), tag.z())
        };
        let arrive = micro(MOVE_SPEED * 1.5);
        let mut steps = 0u64;
        loop {
            let (x, z, yaw, _pitch) = self.sim.pose();
            let self_pose = (micro(x), micro(self.sim.ground_height_m()), micro(z), micro(yaw));
            let (fx, fz) = self.sim.forward_xz();
            let (control, reason, _brain, _emitted, arrived) = AgentHarnessApi::decide_goto(
                AGENT_RAW_ID,
                self.sim.tick(),
                self_pose,
                (micro(fx), micro(fz)),
                goal,
                arrive,
            );
            self.last_control = control;
            self.last_reason = reason;
            if arrived == 1 || self.sim.reached_summit() || steps >= GOTO_TICK_CAP {
                break;
            }
            let (forward_axis, strafe_axis, turn_axis) = axes_of_control(control);
            self.sim.step(forward_axis, strafe_axis, turn_axis);
            steps += 1;
        }
    }

    /// Resolve a tag name to a world point in metres `(x, y_abs, z)` through
    /// introspect (f64 intermediate so the micro→metre cast keeps precision).
    fn resolve_point(&self, name: &str) -> (f32, f32, f32) {
        let tag = self
            .introspect
            .tag_by_name(name)
            .unwrap_or_else(|| panic!("look_at: no world tag named {name:?}"));
        let to_m = |micro: i64| (micro as f64 / 1_000_000.0) as f32;
        (to_m(tag.x()), to_m(tag.y()), to_m(tag.z()))
    }
}

/// Hard cap on `goto` ticks so a degenerate plan can never spin forever.
const GOTO_TICK_CAP: u64 = 20_000;

/// One produced capture: the label to save under and the neutral render inputs.
#[derive(Debug, Clone)]
pub struct CaptureRequest {
    pub label: String,
    pub inputs: CaptureInputs,
}

/// A parsed directive **script** (the data form of a high-level command).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DirectiveFile {
    #[serde(default)]
    pub directive: Vec<Directive>,
}

/// One directive: a closed verb (`goto` / `look_at` / `capture` / `wait`), an
/// optional tag target (the noun), an optional capture label, and an optional
/// step count (for `wait`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Directive {
    pub verb: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub steps: Option<u32>,
}

impl Directive {
    /// The target tag name, panicking with a clear message if this verb needs one
    /// and the data omitted it.
    fn required_target(&self) -> &str {
        self.target
            .as_deref()
            .unwrap_or_else(|| panic!("directive {:?} requires a `target` tag name", self.verb))
    }
}

/// Parse a directive script from TOML — the entry point the bin uses so it never
/// needs the `toml` dependency directly.
pub fn parse_directives(toml_str: &str) -> DirectiveFile {
    toml::from_str(toml_str).expect("growth directive script parses")
}

/// Translate the action's named keys into a held-control bitmask (the harness's
/// first-person vocabulary).
fn control_of_keys(keys: &[String]) -> u32 {
    keys.iter().fold(0u32, |acc, key| {
        acc | match key.as_str() {
            "forward" => AgentHarnessApi::FORWARD,
            "backward" => AgentHarnessApi::BACKWARD,
            "turn_left" | "left" => AgentHarnessApi::TURN_LEFT,
            "turn_right" | "right" => AgentHarnessApi::TURN_RIGHT,
            "strafe_left" => AgentHarnessApi::STRAFE_LEFT,
            "strafe_right" => AgentHarnessApi::STRAFE_RIGHT,
            _ => 0,
        }
    })
}

/// Decode a held-control bitmask back into the named controls — the inverse of
/// [`control_of_keys`], used by the live browser bridge to push the agent's
/// decision to the viewer as `{"keys":[…]}`.
pub fn control_to_keys(code: u32) -> Vec<String> {
    let table = [
        (AgentHarnessApi::FORWARD, "forward"),
        (AgentHarnessApi::BACKWARD, "backward"),
        (AgentHarnessApi::TURN_LEFT, "turn_left"),
        (AgentHarnessApi::TURN_RIGHT, "turn_right"),
        (AgentHarnessApi::STRAFE_LEFT, "strafe_left"),
        (AgentHarnessApi::STRAFE_RIGHT, "strafe_right"),
    ];
    table
        .iter()
        .filter(|(flag, _)| code & flag != 0)
        .map(|(_, name)| (*name).to_string())
        .collect()
}

/// Decode a held-control bitmask into ground-sim movement axes
/// `(forward, strafe, turn)`, each in `{-1, 0, 1}`.
fn axes_of_control(code: u32) -> (f32, f32, f32) {
    let bit = |flag: u32| f32::from(u8::from(code & flag != 0));
    let forward = bit(AgentHarnessApi::FORWARD) - bit(AgentHarnessApi::BACKWARD);
    let strafe = bit(AgentHarnessApi::STRAFE_RIGHT) - bit(AgentHarnessApi::STRAFE_LEFT);
    let turn = bit(AgentHarnessApi::TURN_LEFT) - bit(AgentHarnessApi::TURN_RIGHT);
    (forward, strafe, turn)
}
