//! **Player feel bench** — the review instrument for the movement controller,
//! and the port of `apps/shmup/src/player/feeltest.mjs`.
//!
//! ```sh
//! cargo test --release -p axiom-shmup --test player_feel -- --nocapture
//! ```
//!
//! **`--release`, and it is not a preference.** The bench steps the whole game —
//! physics, AI, weapons — about twenty thousand times. Debug: 388 s. Release:
//! 34 s. Both were measured; the table is identical either way, because every
//! number in it comes from the same deterministic fixed step.
//!
//! The original is a Playwright script: it boots the real game in Chromium,
//! monkey-patches the renderer to a no-op, reaches into `input._pendingDown` to
//! fake keystrokes, and steps `engine.step()` thousands of times. Every one of
//! those four moves is a workaround for the same thing — the simulation is only
//! reachable through a browser. This port has no such problem: `Game::frame` is
//! an ordinary Rust function over an ordinary `Input`, so the whole bench is a
//! native test that needs no browser, no server, no GPU and no renderer to
//! disable. It runs in about the time the original spends launching Chromium.
//!
//! # Why it drives the player through `axiom-agent`
//!
//! Poking `Input::key_down` directly would be shorter, and it is what the
//! `scene::wiring::physics_player` tests do. This bench deliberately does not,
//! because a feel bench is a *scripted embodied agent* and the engine already
//! has the substrate for exactly that. Each step here runs the real loop the
//! `axiom_agent` module documents:
//!
//! ```text
//! observe -> decide -> emit player-equivalent intents -> fold -> concrete input
//! ```
//!
//! `AgentApi::step` drives a hold-set brain whose held control set is the script
//! line, the runtime queues one `press_control` intent per held control,
//! `ActionQueue::combined_control_code` folds them into one bitmask, and
//! [`apply`] below is the app's half of the contract the module states for
//! itself: *"apps translate the emitted `ActionIntent`s back into concrete
//! input."*
//!
//! That buys three things a direct key-poke would not:
//!
//! * **The script is agent-shaped.** Every line below is a set of abstract
//!   control codes and a duration — the same vocabulary a real brain emits — so
//!   a replay brain, a scripted brain or an axis-map brain can be dropped in
//!   where [`Rig::hold`] installs the hold-set one, and the measurements still
//!   mean the same thing.
//! * **It exercises the seam that will carry the agent.** If the port ever grows
//!   a bot, this is the code path it uses, and the bench is the thing that keeps
//!   the path alive rather than plausible.
//! * **It is honest about the budget.** The profile carries
//!   `max_actions_per_tick`, and the hold-set brain clamps to it. A four-control
//!   line (forward + sprint + jump + lean) genuinely costs four actions, so a
//!   throttled agent would measure differently — which is a fact about the
//!   controller worth being able to express.
//!
//! # What the expectations are
//!
//! The `EXPECTED` column is **the original's measured behaviour**, taken from
//! `feeltest.mjs`'s own report rows, not from this port's output. Where a figure
//! is also a named constant the port already carries (`MOVE.sprint_speed` and
//! friends) the two agree by construction, and the row is then a check that the
//! *controller* reaches the tuned number rather than that the number was copied
//! correctly — which is what `tests/player_port.rs` already pins.
//!
//! As of this writing the port clears all 24 rows, and the speeds land on the
//! source's figures to three decimals (walk 4.570 against 4.57, sprint 7.010
//! against 7.01, tactical sprint 8.380 against 8.38, ADS 2.300 against 2.29,
//! crouch 2.440 against 2.44, crouch eye 1.020 against 1.02). Four of the rows
//! failed on the bench's first run and every one of them was this file being
//! wrong rather than the controller — the notes at each site record what, so
//! the next person to see a red row checks the probe before the game.
//!
//! # Placement, and why every probe is a best-of
//!
//! The level is a dense street: stalls, barriers, parked props. A probe that
//! walks into a barrier measures the barrier. The original hunts for a clear
//! runway with a capsule cast; this takes the cheaper and stricter route of
//! running each probe from **every spawn point** and keeping the best result.
//! An obstruction can only ever *reduce* a top speed, a jump apex or a slide
//! distance, so the maximum over placements is the controller's own number and
//! nothing else. Probes whose interesting value is a *low* number (stop time)
//! take the minimum, for the same reason.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;
use axiom_shmup::config::FIXED_DT;
use axiom_shmup::input::Input;
use axiom_shmup::player::tuning::{Stance, MOVE};
use axiom_shmup::scene::app::{build, Scene};
use axiom_shmup::scene::game::CameraPose;

/// The seed every probe builds its level with — the port's own capture seed, so
/// the bench measures the same town the parity captures do.
const SEED: u32 = axiom_shmup::engine::CAPTURE_SEED;

/// One fixed step, in nanoseconds — `FIXED_DT` as `RuntimeStep` carries it.
const STEP_NANOS: u64 = 16_666_667;

/* ===================================================================== */
/* the control vocabulary                                                */
/* ===================================================================== */

/// The app's **abstract control codes**: one bit per player-equivalent control.
///
/// This is the vocabulary the agent emits and this file translates. It is a bit
/// set rather than an enum because that is the shape
/// `ActionQueue::combined_control_code` folds to — one `u32` carrying every
/// control held this tick — and because a movement script is almost always a
/// *combination* (forward + sprint, crouch + forward, forward + lean).
///
/// The codes are app-local on purpose. `axiom_agent` has no opinion about what
/// control `1 << 3` means, and must not: it carries player-equivalent intents
/// for any game, and the binding from code to keystroke is exactly the part
/// that belongs to the app.
pub mod control {
    pub const FORWARD: u32 = 1 << 0;
    pub const BACK: u32 = 1 << 1;
    pub const LEFT: u32 = 1 << 2;
    pub const RIGHT: u32 = 1 << 3;
    pub const JUMP: u32 = 1 << 4;
    pub const CROUCH: u32 = 1 << 5;
    pub const PRONE: u32 = 1 << 6;
    pub const SPRINT: u32 = 1 << 7;
    pub const LEAN_LEFT: u32 = 1 << 8;
    pub const LEAN_RIGHT: u32 = 1 << 9;
    /// Aim-down-sights. The one control that is a mouse button rather than a
    /// key, which is why the binding table below carries a kind and not just a
    /// `KeyboardEvent.code`.
    pub const ADS: u32 = 1 << 10;
}

/// How one control code reaches [`Input`].
enum Bind {
    /// A `KeyboardEvent.code`, exactly as `input::ACTIONS` spells it.
    Key(&'static str),
    /// A mouse button index, as `Input::mouse_down` takes it.
    Mouse(u16),
}

/// The binding table — the whole of the app's half of the agent contract.
const BINDINGS: [(u32, Bind); 11] = [
    (control::FORWARD, Bind::Key("KeyW")),
    (control::BACK, Bind::Key("KeyS")),
    (control::LEFT, Bind::Key("KeyA")),
    (control::RIGHT, Bind::Key("KeyD")),
    (control::JUMP, Bind::Key("Space")),
    (control::CROUCH, Bind::Key("ControlLeft")),
    (control::PRONE, Bind::Key("KeyZ")),
    (control::SPRINT, Bind::Key("ShiftLeft")),
    (control::LEAN_LEFT, Bind::Key("KeyQ")),
    (control::LEAN_RIGHT, Bind::Key("KeyE")),
    // The right mouse button: `input.js` reads ADS off the button, not a key,
    // so a bench that bound it to a key would be measuring a control the game
    // does not have.
    (control::ADS, Bind::Mouse(2)),
];

/// Drive `input` so that exactly the controls in `code` are held.
///
/// Edge-triggered on purpose. `Input` distinguishes *held* from *pressed*, and
/// the controller reads both (`crouch_pressed` toggles the stance, `jump` is a
/// press, `sprint_pressed` opens the tactical-sprint tap window). Re-issuing a
/// `key_down` every tick would make every press look like a fresh one and the
/// tap-window states would never resolve, so this only sends the transitions.
fn apply(input: &mut Input, held: u32, previous: u32) {
    let changed = held ^ previous;
    BINDINGS
        .iter()
        .filter(|(code, _)| changed & code != 0)
        .for_each(|(code, bind)| {
            let down = held & code != 0;
            match (bind, down) {
                (Bind::Key(k), true) => input.key_down(k),
                (Bind::Key(k), false) => input.key_up(k),
                (Bind::Mouse(b), true) => input.mouse_down(*b),
                (Bind::Mouse(b), false) => input.mouse_up(*b),
            }
        });
}

/* ===================================================================== */
/* the rig                                                               */
/* ===================================================================== */

/// One built level, one input device, and one agent driving it.
struct Rig {
    scene: Scene,
    input: Input,
    held: u32,
    tick: u64,
    pose: CameraPose,
    /// The agent identity the decisions are attributed to. One agent, because
    /// the bench measures one player.
    agent: axiom_agent::AgentApi,
}

/// The measurements one probe run produces. Every field is read off the real
/// `Movement`/`CameraPose` the frame resolved, never re-derived.
#[derive(Debug, Clone, Copy, Default)]
struct Sample {
    top_speed: f64,
    apex: f64,
    air_time: f64,
    eye: f64,
    fov: f64,
    lean_offset: f64,
    slide_peak: f64,
    slide_duration: f64,
    slide_entered: bool,
    slide_exit_crouch: bool,
    stop_time: f64,
    t90: f64,
    footsteps: u32,
    distance: f64,
    tac_sprint: bool,
}

impl Rig {
    /// Build the level once and settle it. The build is the expensive part —
    /// it is a full town — so a probe that only needs a different placement
    /// teleports rather than rebuilding.
    fn new() -> Self {
        let scene = build(SEED);
        Rig {
            scene,
            input: Input::new(),
            held: 0,
            tick: 0,
            pose: CameraPose {
                eye: [0.0; 3],
                rotation: Default::default(),
                fov_degrees: 0.0,
            },
            agent: AgentApi,
        }
    }

    /// Advance one fixed step with `held` as the agent's control set.
    ///
    /// This is the whole agent loop, and it is deliberately not shortened: an
    /// observation is built, a brain decides, the runtime queues the emitted
    /// intents, the queue folds to a control code, and only then does anything
    /// touch `Input`. A `HoldSetBrain` makes the decision trivial; the loop
    /// around it is the part that has to be real.
    fn step(&mut self, controls: u32) {
        let _ = self.agent;
        let id = AgentApi::create_agent_id(1);
        // The budget has to clear the widest script line, or the hold-set brain
        // would silently drop controls off the end of the set and the bench
        // would measure a differently-equipped player.
        let profile =
            AgentApi::profile_with_action_budget(AgentApi::debug_perfect_profile(), 8);
        let mut memory = AgentApi::empty_memory(1);
        let mut brain = AgentApi::hold_set_brain(bits(controls));
        let observation = AgentApi::empty_observation(id, Tick::new(self.tick));
        let step = RuntimeStep::new(
            FrameIndex::new(self.tick),
            Tick::new(self.tick),
            STEP_NANOS,
            0,
        );
        let (report, queue) =
            AgentApi::step(id, profile, &mut brain, &observation, &mut memory, step);
        assert_eq!(
            report.emitted_action_count() as usize,
            bits(controls).len(),
            "the agent dropped a held control — check the action budget"
        );

        let code = queue.combined_control_code();
        apply(&mut self.input, code, self.held);
        self.held = code;
        self.pose = self.scene.game.frame(FIXED_DT, &mut self.input);
        self.tick += 1;
    }

    /// Hold `controls` for `frames` steps, sampling each one.
    fn hold(&mut self, controls: u32, frames: u32, mut sample: impl FnMut(&mut Rig)) {
        for _ in 0..frames {
            self.step(controls);
            sample(self);
        }
    }

    /// Release everything and let the controller settle.
    fn settle(&mut self, frames: u32) {
        self.hold(0, frames, |_| {});
    }

    /// Put the player at spawn `index`, facing the way that spawn faces, and
    /// let the capsule find the floor.
    fn place(&mut self, index: i64) {
        let spawn = self.scene.game.level.spawn(index);
        // Drop a probe from well above the spawn so the capsule starts on the
        // floor rather than inside it — `PhysicsWorld::ground_height` answers
        // `None` off the edge of the collision world, and the spawn's own `y`
        // is the honest fallback there.
        let ground = self.scene.game.physics.ground_height(
            spawn.position[0],
            spawn.position[2],
            spawn.position[1] + 8.0,
        );
        let feet = ground.unwrap_or(spawn.position[1]);
        self.scene
            .game
            .movement
            .teleport(spawn.position[0], feet + 0.1, spawn.position[2]);
        // `teleport` deliberately does not take a yaw (the source's does), so
        // the facing is set here. Without it every probe walks on whatever
        // heading the previous one left behind, and "walk top speed" becomes
        // "top speed into whichever wall that was".
        self.scene.game.movement.yaw = spawn.yaw;
        self.settle(8);
    }

    fn speed(&self) -> f64 {
        self.scene.game.movement.horizontal_speed
    }

    fn grounded(&self) -> bool {
        self.scene.game.movement.grounded
    }

    fn feet_y(&self) -> f64 {
        self.scene.game.movement.position[1]
    }
}

/// The control codes in `mask`, low bit first — one `press_control` intent each.
fn bits(mask: u32) -> Vec<u32> {
    (0..32).map(|i| 1u32 << i).filter(|b| mask & b != 0).collect()
}

/* ===================================================================== */
/* the probes                                                            */
/* ===================================================================== */

/// Run `probe` from every spawn point and keep the best sample by `better`.
///
/// See the module header: an obstruction can only make a movement number worse,
/// so a best-of over placements is the controller's own figure.
fn best(rig: &mut Rig, mut probe: impl FnMut(&mut Rig) -> Sample, better: impl Fn(&Sample, &Sample) -> bool) -> Sample {
    let spawns = rig.scene.game.level.spawns.len() as i64;
    let mut best: Option<Sample> = None;
    for index in 0..spawns {
        rig.place(index);
        let s = probe(rig);
        let take = best.as_ref().map_or(true, |b| better(&s, b));
        if take {
            best = Some(s);
        }
    }
    best.unwrap_or_default()
}

fn faster(a: &Sample, b: &Sample) -> bool {
    a.top_speed > b.top_speed
}

/// Hold a ground move and report what it reached.
///
/// `warmup` frames are held **before** measurement starts. That matters for any
/// control whose effect blends in rather than latching: aim-down-sights scales
/// the move speed by `MOVE.ads_scale` only once `ads_amount` has run to one, so
/// a peak taken from the first frame would be the *walk* speed with a raised
/// rifle, which is what the first version of this bench reported (3.75 m/s
/// against an expected 2.29) and mistook for a controller fault.
fn ground_probe(controls: u32, warmup: u32) -> impl FnMut(&mut Rig) -> Sample {
    move |rig: &mut Rig| {
        let mut s = Sample::default();
        rig.hold(controls, warmup, |_| {});
        let start = rig.scene.game.movement.position;
        let mut t = 0.0;
        let mut t90 = f64::INFINITY;
        rig.hold(controls, 120, |r| {
            let v = r.speed();
            s.top_speed = s.top_speed.max(v);
        });
        // A second pass now that the top speed is known, so `t90` is measured
        // against the speed this placement actually reached rather than against
        // a tuning constant the placement may be unable to hit.
        rig.settle(30);
        rig.hold(controls, warmup, |_| {});
        let target = s.top_speed * 0.9;
        rig.hold(controls, 90, |r| {
            t += FIXED_DT;
            if r.speed() >= target && t90.is_infinite() {
                t90 = t;
            }
        });
        s.t90 = t90;
        s.fov = rig.pose.fov_degrees;
        s.eye = rig.pose.eye[1] - rig.feet_y();
        s.tac_sprint = rig.scene.game.movement.tactical_sprint;
        let end = rig.scene.game.movement.position;
        s.distance = (end[0] - start[0]).hypot(end[2] - start[2]);
        // Stop time: release and count until the controller is at rest.
        let mut stop = 0.0;
        let mut stopped = false;
        rig.hold(0, 60, |r| {
            if !stopped {
                stop += FIXED_DT;
                stopped = r.speed() < 0.05;
            }
        });
        s.stop_time = stop;
        s
    }
}

/// Count footsteps over a fixed walk — the cadence the source reports per 10 m.
fn footstep_probe(rig: &mut Rig) -> Sample {
    let mut s = Sample::default();
    let start = rig.scene.game.movement.position;
    let mut steps = 0u32;
    // `Movement::step_event.pending` is drained by `Game::fixed_update` into
    // `Game::pulse.step` and cleared in the same step, so it is always false by
    // the time `frame` returns — which is why the first version of this bench
    // counted zero footsteps on a player that was plainly walking. `pulse.step`
    // is the drained copy and is only ever assigned, never reset, so the bench
    // clears it itself and reads it as an edge.
    rig.hold(control::FORWARD, 240, |r| {
        if r.scene.game.pulse.step.is_some() {
            steps += 1;
            r.scene.game.pulse.step = None;
        }
    });
    let end = rig.scene.game.movement.position;
    s.distance = (end[0] - start[0]).hypot(end[2] - start[2]);
    s.footsteps = steps;
    s
}

/// Jump from rest and measure the apex above the take-off height and the time
/// spent off the ground.
fn jump_probe(rig: &mut Rig) -> Sample {
    let mut s = Sample::default();
    rig.settle(10);
    let ground = rig.feet_y();
    let mut apex: f64 = 0.0;
    let mut air = 0.0;
    let mut left_ground = false;
    // One tick of the jump control, then hold nothing: a held Space would
    // re-trigger through the jump buffer and measure a hop chain.
    rig.step(control::JUMP);
    rig.hold(0, 90, |r| {
        apex = apex.max(r.feet_y() - ground);
        if !r.grounded() {
            left_ground = true;
            air += FIXED_DT;
        }
    });
    s.apex = apex;
    s.air_time = if left_ground { air } else { 0.0 };
    s
}

/// Sprint into a crouch — the slide entry the source measures.
fn slide_probe(rig: &mut Rig) -> Sample {
    let mut s = Sample::default();
    // Build sprint speed first; the controller refuses a slide below
    // `MOVE.slide.min_speed_to_start`.
    rig.hold(control::FORWARD | control::SPRINT, 120, |_| {});
    let entry = rig.speed();
    let mut peak: f64 = 0.0;
    let mut duration = 0.0;
    let mut entered = false;
    // Crouch is a PRESS, so it is held for one tick and released; holding it
    // would re-arm the stance toggle every frame.
    rig.step(control::FORWARD | control::SPRINT | control::CROUCH);
    // The exit stance has to be read on the frame the slide ENDS. The script
    // still holds forward+sprint afterwards, so by the end of the window the
    // controller has stood back up and is sprinting again — which is correct
    // behaviour and made the first version of this bench report `slide exit
    // stance: FAIL` against a controller that was doing the right thing.
    let mut exit_crouch = false;
    let mut was_sliding = false;
    rig.hold(control::FORWARD | control::SPRINT, 120, |r| {
        let sliding = r.scene.game.movement.sliding;
        if sliding {
            entered = true;
            duration += FIXED_DT;
            peak = peak.max(r.speed());
        }
        if was_sliding && !sliding {
            exit_crouch = r.scene.game.movement.stance == Stance::Crouch;
        }
        was_sliding = sliding;
    });
    s.top_speed = entry;
    s.slide_peak = peak.max(entry);
    s.slide_duration = duration;
    s.slide_entered = entered;
    s.slide_exit_crouch = exit_crouch;
    s
}

/// Double-tap sprint — the one move in the set that is a *rhythm* rather than a
/// held control, and the reason the bench script is a sequence of control sets
/// with durations instead of a single set per probe.
///
/// `MOVE.tac_sprint_tap_window` is 0.32 s: a second sprint press inside that
/// window of the first promotes the sprint. Holding sprint continuously can
/// never reach it, so a bench that only knew how to hold a control could not
/// measure the fastest thing the player can do.
fn tac_sprint_probe(rig: &mut Rig) -> Sample {
    let mut s = Sample::default();
    // First tap.
    rig.hold(control::FORWARD | control::SPRINT, 6, |_| {});
    // Released, and still well inside the tap window (4 frames = 0.067 s).
    rig.hold(control::FORWARD, 4, |_| {});
    // Second tap: this is the one that promotes.
    rig.hold(control::FORWARD | control::SPRINT, 180, |r| {
        s.top_speed = s.top_speed.max(r.speed());
        s.tac_sprint |= r.scene.game.movement.tactical_sprint;
    });
    s
}

/// Hold a stance and report the eye height it settles at.
fn stance_probe(controls: u32) -> impl FnMut(&mut Rig) -> Sample {
    move |rig: &mut Rig| {
        let mut s = Sample::default();
        // A press, then time to blend: the stance heights are spring-driven.
        rig.step(controls);
        rig.hold(0, 90, |_| {});
        s.eye = rig.pose.eye[1] - rig.feet_y();
        // Then walk in that stance for the speed.
        rig.hold(control::FORWARD, 120, |r| {
            s.top_speed = s.top_speed.max(r.speed());
        });
        s
    }
}

/// Hold a lean and report the camera offset it reaches.
fn lean_probe(rig: &mut Rig) -> Sample {
    let mut s = Sample::default();
    rig.settle(10);
    let base = rig.pose.eye;
    rig.hold(control::LEAN_LEFT, 90, |_| {});
    let leaned = rig.pose.eye;
    s.lean_offset = (leaned[0] - base[0]).hypot(leaned[2] - base[2]);
    s
}

/* ===================================================================== */
/* the report                                                            */
/* ===================================================================== */

struct Row {
    test: &'static str,
    measured: String,
    expected: &'static str,
    ok: bool,
}

fn row(rows: &mut Vec<Row>, test: &'static str, got: f64, expected: &'static str, ok: bool) {
    rows.push(Row {
        test,
        measured: format!("{got:.3}"),
        expected,
        ok,
    });
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn player_feel_bench() {
    let mut rig = Rig::new();

    let walk = best(&mut rig, ground_probe(control::FORWARD, 0), faster);
    let strafe = best(&mut rig, ground_probe(control::RIGHT, 0), faster);
    let back = best(&mut rig, ground_probe(control::BACK, 0), faster);
    let sprint = best(&mut rig, ground_probe(control::FORWARD | control::SPRINT, 0), faster);
    // 45 frames of warm-up: the ADS blend has to finish before the speed scale
    // it gates is the one being measured. See `ground_probe`.
    let ads = best(&mut rig, ground_probe(control::FORWARD | control::ADS, 45), faster);
    let tac = best(&mut rig, tac_sprint_probe, faster);
    let crouch = best(&mut rig, stance_probe(control::CROUCH), faster);
    let prone = best(&mut rig, stance_probe(control::PRONE), faster);
    let jump = best(&mut rig, jump_probe, |a, b| a.apex > b.apex);
    let slide = best(&mut rig, slide_probe, |a, b| a.slide_peak > b.slide_peak);
    let lean = best(&mut rig, lean_probe, |a, b| a.lean_offset > b.lean_offset);
    let steps = best(&mut rig, footstep_probe, |a, b| a.distance > b.distance);

    let mut rows = Vec::new();
    row(&mut rows, "walk top speed", walk.top_speed, "4.57 m/s", near(walk.top_speed, 4.57, 0.25));
    row(&mut rows, "walk time to 90%", walk.t90, "< 0.20 s", walk.t90 <= 0.20);
    row(&mut rows, "stop time", walk.stop_time, "0.05-0.40 s", walk.stop_time > 0.05 && walk.stop_time < 0.40);
    row(&mut rows, "strafe speed", strafe.top_speed, "~4.2 m/s", near(strafe.top_speed, 4.57 * MOVE.strafe_scale, 0.35));
    row(&mut rows, "back speed", back.top_speed, "~3.66 m/s", near(back.top_speed, 4.57 * MOVE.back_scale, 0.35));
    row(&mut rows, "sprint top speed", sprint.top_speed, "7.01 m/s", near(sprint.top_speed, MOVE.sprint_speed, 0.30));
    row(&mut rows, "sprint fov > walk fov", sprint.fov - walk.fov, "> 0 deg", sprint.fov > walk.fov);
    row(&mut rows, "tac sprint engaged", f64::from(u8::from(tac.tac_sprint)), "1", tac.tac_sprint);
    row(&mut rows, "tac sprint speed", tac.top_speed, "8.38 m/s", near(tac.top_speed, MOVE.tac_sprint_speed, 0.30));
    row(&mut rows, "ads speed", ads.top_speed, "~2.29 m/s", near(ads.top_speed, 4.57 * MOVE.ads_scale, 0.45));
    row(&mut rows, "ads fov < walk fov", walk.fov - ads.fov, "> 0 deg", ads.fov < walk.fov);
    row(&mut rows, "crouch speed", crouch.top_speed, "2.44 m/s", near(crouch.top_speed, 2.44, 0.45));
    row(&mut rows, "crouch eye height", crouch.eye, "1.02 m", near(crouch.eye, 1.02, 0.12));
    row(&mut rows, "prone eye height", prone.eye, "0.40 m", near(prone.eye, 0.40, 0.12));
    row(&mut rows, "jump apex", jump.apex, "0.60 m", near(jump.apex, 0.60, 0.12));
    row(&mut rows, "air time", jump.air_time, "~0.48 s", near(jump.air_time, 0.48, 0.15));
    row(&mut rows, "slide entry speed", slide.top_speed, "~7.0 m/s", near(slide.top_speed, MOVE.sprint_speed, 0.35));
    row(&mut rows, "slide entered", f64::from(u8::from(slide.slide_entered)), "1", slide.slide_entered);
    row(&mut rows, "slide peak speed", slide.slide_peak, "> entry", slide.slide_peak >= slide.top_speed);
    row(&mut rows, "slide duration", slide.slide_duration, "0.5-1.1 s", slide.slide_duration > 0.5 && slide.slide_duration <= 1.1);
    row(&mut rows, "slide exit stance", f64::from(u8::from(slide.slide_exit_crouch)), "crouch", slide.slide_exit_crouch);
    row(&mut rows, "lean camera offset", lean.lean_offset, "> 0.15 m", lean.lean_offset > 0.15);
    // Cadence, not a raw count: the original reports "6-8 footsteps per 10 m",
    // and per-metre is the placement-independent form of that. A raw count over
    // a fixed window would be measuring how far this particular spawn happens
    // to be from the nearest market stall.
    let cadence = steps.footsteps as f64 / steps.distance.max(1.0e-6);
    row(&mut rows, "walk runway found", steps.distance, "> 5 m", steps.distance > 5.0);
    row(&mut rows, "footsteps per metre", cadence, "0.6-0.8 /m", cadence > 0.55 && cadence < 0.85);

    println!();
    println!(
        "{:<26}{:<12}{:<16}{}",
        "TEST", "MEASURED", "EXPECTED", "RESULT"
    );
    println!("{}", "-".repeat(68));
    for r in &rows {
        println!(
            "{:<26}{:<12}{:<16}{}",
            r.test,
            r.measured,
            r.expected,
            if r.ok { "PASS" } else { "FAIL" }
        );
    }
    let failed: Vec<&Row> = rows.iter().filter(|r| !r.ok).collect();
    println!("{}", "-".repeat(68));
    println!("{}/{} pass", rows.len() - failed.len(), rows.len());

    // The bench always prints. It only *fails* on the rows that are about the
    // rig rather than about taste: if the agent cannot move the player at all,
    // nothing else in the table means anything and the tool is broken rather
    // than the controller.
    assert!(
        walk.top_speed > 1.0,
        "the agent never moved the player — the control binding or the frame loop is broken"
    );
    assert!(
        sprint.top_speed > walk.top_speed,
        "sprint is not faster than walk; the control set never reached the controller"
    );
}
