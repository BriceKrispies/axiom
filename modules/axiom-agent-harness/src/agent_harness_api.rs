//! [`AgentHarnessApi`] — the module's single public facade.
//!
//! It composes [`axiom_agent::AgentApi`] into the reusable first-person
//! observe → decide → lower pipeline. Everything crosses the boundary as
//! primitives and tuples (Module Law: one facade, no foreign types in the public
//! surface), so an app drives an agent without naming an `axiom-agent` type.
//!
//! The code is branchless (the engine spine invariant): conditional control is
//! expressed as bitmask arithmetic and iterator/`Option` combinators, never
//! `if`/`match`/`&&`.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Meters, Tick};
use axiom_runtime::RuntimeStep;

/// The reusable first-person agent-driving facade.
///
/// Stateless: every method runs one self-contained `observe → decide → lower`
/// cycle through [`axiom_agent`] and returns the result as plain numbers.
#[derive(Debug)]
pub struct AgentHarnessApi;

impl AgentHarnessApi {
    // --- first-person held-control vocabulary (the neutral `control_code` bits) ---

    /// Hold "move forward".
    pub const FORWARD: u32 = 1 << 0;
    /// Hold "move backward".
    pub const BACKWARD: u32 = 1 << 1;
    /// Hold "turn left" (the game decides which yaw sign that is).
    pub const TURN_LEFT: u32 = 1 << 2;
    /// Hold "turn right".
    pub const TURN_RIGHT: u32 = 1 << 3;
    /// Hold "strafe left".
    pub const STRAFE_LEFT: u32 = 1 << 4;
    /// Hold "strafe right".
    pub const STRAFE_RIGHT: u32 = 1 << 5;
    /// Hold the primary action button (e.g. fire / interact).
    pub const ACTION_PRIMARY: u32 = 1 << 6;
    /// Hold the secondary action button.
    pub const ACTION_SECONDARY: u32 = 1 << 7;

    // --- observation fact-kind vocabulary (so a future scripted brain can read them) ---

    /// The agent's own pose fact: `x`/`y`/`z` carry world position **with the
    /// player's height in `y`**, and `value` carries the yaw.
    pub const FACT_SELF_POSE: u16 = 1;
    /// The goal point fact: `x`/`y`/`z` carry the target position.
    pub const FACT_GOAL_POINT: u16 = 2;

    /// Subject code for the agent itself.
    const SUBJECT_SELF: u32 = 0;
    /// Subject code for the goal.
    const SUBJECT_GOAL: u32 = 1;

    /// A fixed ~60 Hz tick delta (nanoseconds) for the `RuntimeStep` that stamps a
    /// decision. The harness drives discrete decisions, so the exact delta is only
    /// bookkeeping; it is fixed for determinism.
    const FIXED_DELTA_NANOS: u64 = 16_666_667;

    /// Tangent of the "close enough to straight ahead" cone (~8°): inside it the
    /// seek policy walks forward without turning.
    const AHEAD_TAN: f64 = 0.14;

    /// Half-angle (radians, ~1.7°) of the "aimed at the target" cone for
    /// [`Self::decide_look_at`]: inside it the yaw is considered on-target.
    const AIMED_CONE_RADIANS: f64 = 0.03;

    /// One micro-unit per millionth of a world unit — the fixed-point convention
    /// for the integer observation coordinates.
    const MICRO: f64 = 1_000_000.0;

    /// Encode a world-space length as fixed-point **micro-units** (millionths of a
    /// world unit) — the harness's integer observation-coordinate convention. The
    /// `decide_*` methods speak this encoding in their `(x, y, z, yaw)` tuples, so
    /// this is the single source of truth for the convention every caller (and the
    /// inverse [`Self::metres`]) must agree on.
    pub fn micro(m: Meters) -> i64 {
        (f64::from(m.get()) * Self::MICRO) as i64
    }

    /// Decode fixed-point micro-units back to a world-space length — the inverse of
    /// [`Self::micro`]. Total: dividing a finite integer by the fixed scale is
    /// always finite, so the length is always valid (any non-finite arithmetic
    /// result would sanitize to zero via [`Meters::finite_or_zero`]).
    pub fn metres(u: i64) -> Meters {
        Meters::finite_or_zero((u as f64 / Self::MICRO) as f32)
    }

    /// Decide a held-control bitmask by **holding `held_control_code`** this tick.
    ///
    /// The harness packs a neutral observation (the agent's pose — including its
    /// height in `y` — and the goal point), runs a one-shot replay of
    /// `held_control_code` through [`axiom_agent`] so the decision produces a real
    /// report, and lowers the emitted intent back to a held-control bitmask.
    ///
    /// Returns `(control_code, reason_code, brain_kind_code, emitted_action_count)`.
    /// A literal "hold forward" is `decide_hold(.., Self::FORWARD)`.
    pub fn decide_hold(
        agent_raw_id: u64,
        tick: u64,
        self_pose_micro: (i64, i64, i64, i64),
        goal_point_micro: (i64, i64, i64),
        held_control_code: u32,
    ) -> (u32, u16, u16, usize) {
        Self::decide_with_control(
            agent_raw_id,
            tick,
            self_pose_micro,
            goal_point_micro,
            held_control_code,
        )
    }

    /// Decide a held-control bitmask by **seeking the goal point**: turn toward it
    /// and walk, stopping within `arrive_radius_micro`.
    ///
    /// The policy is deterministic and game-agnostic — the game passes its own
    /// current forward direction `self_forward_micro` (the world-space `(x, z)` it
    /// considers "forward"), so no yaw convention is baked in. The harness routes
    /// the computed control through [`axiom_agent`] for the canonical report, the
    /// same path as [`Self::decide_hold`].
    ///
    /// Returns `(control_code, reason_code, brain_kind_code, emitted_action_count)`.
    pub fn decide_seek(
        agent_raw_id: u64,
        tick: u64,
        self_pose_micro: (i64, i64, i64, i64),
        self_forward_micro: (i64, i64),
        goal_point_micro: (i64, i64, i64),
        arrive_radius_micro: i64,
    ) -> (u32, u16, u16, usize) {
        let (control, _arrived) = Self::seek_control_code(
            self_pose_micro,
            self_forward_micro,
            goal_point_micro,
            arrive_radius_micro,
        );
        Self::decide_with_control(
            agent_raw_id,
            tick,
            self_pose_micro,
            goal_point_micro,
            control,
        )
    }

    /// Decide a held-control bitmask by **going to** a target point — the
    /// lowering of the agent's high-level `move_toward_point` verb. Identical
    /// motion to [`Self::decide_seek`] (turn-toward + forward), but it also
    /// returns an `arrived` flag so a directive runner knows when this verb is
    /// complete without re-deriving the arrival test in the app.
    ///
    /// Returns `(control_code, reason_code, brain_kind_code, emitted_action_count,
    /// arrived)` where `arrived` is `1` once within `arrive_radius_micro`.
    pub fn decide_goto(
        agent_raw_id: u64,
        tick: u64,
        self_pose_micro: (i64, i64, i64, i64),
        self_forward_micro: (i64, i64),
        goal_point_micro: (i64, i64, i64),
        arrive_radius_micro: i64,
    ) -> (u32, u16, u16, usize, u32) {
        let (control, arrived) = Self::seek_control_code(
            self_pose_micro,
            self_forward_micro,
            goal_point_micro,
            arrive_radius_micro,
        );
        let (control, reason, brain, emitted) = Self::decide_with_control(
            agent_raw_id,
            tick,
            self_pose_micro,
            goal_point_micro,
            control,
        );
        (control, reason, brain, emitted, arrived)
    }

    /// Lower the agent's high-level `look_at_point` verb to an orientation
    /// command toward `target_point_micro`, given the agent's current `(x, z)`
    /// forward direction `self_forward_micro` (no yaw convention is baked in, the
    /// same contract as [`Self::decide_seek`]).
    ///
    /// Returns `(yaw_turn_micro, pitch_target_micro, aimed)`:
    /// - `yaw_turn_micro` — signed micro-radians to rotate the current forward
    ///   toward the target horizontally; **positive turns left** (toward the side
    ///   `decide_seek` calls `TURN_LEFT`).
    /// - `pitch_target_micro` — the look pitch in micro-radians; **positive looks
    ///   up**, negative looks down (so a target below the eye, "look at the
    ///   ground", yields a negative pitch).
    /// - `aimed` — `1` once `yaw_turn_micro` is within the alignment cone.
    ///
    /// A live game feeds the yaw turn as look input and sets the pitch; a capture
    /// path can build a view straight from the target point instead.
    pub fn decide_look_at(
        self_pose_micro: (i64, i64, i64, i64),
        self_forward_micro: (i64, i64),
        target_point_micro: (i64, i64, i64),
    ) -> (i64, i64, u32) {
        let dx = (target_point_micro.0 - self_pose_micro.0) as f64 / Self::MICRO;
        let dy = (target_point_micro.1 - self_pose_micro.1) as f64 / Self::MICRO;
        let dz = (target_point_micro.2 - self_pose_micro.2) as f64 / Self::MICRO;
        let fx = self_forward_micro.0 as f64 / Self::MICRO;
        let fz = self_forward_micro.1 as f64 / Self::MICRO;

        // Signed horizontal turn: atan2(cross, dot) of forward against the target
        // direction. cross > 0 ⇒ target on the left ⇒ positive (TURN_LEFT side).
        let cross = fx * dz - fz * dx;
        let dot = fx * dx + fz * dz;
        let yaw_turn = cross.atan2(dot);

        // Look pitch from the vertical rise over the horizontal run to the target.
        let horizontal = (dx * dx + dz * dz).sqrt();
        let pitch = dy.atan2(horizontal);

        let aimed = u32::from(yaw_turn.abs() <= Self::AIMED_CONE_RADIANS);
        (
            (yaw_turn * Self::MICRO) as i64,
            (pitch * Self::MICRO) as i64,
            aimed,
        )
    }

    /// The branchless seek policy: turn-toward + forward as a held-control bitmask.
    ///
    /// Uses the 2-D cross/dot of `forward` against `goal - self` (no trig, no yaw
    /// convention): `cross` sign picks the turn direction, `dot > 0` with a small
    /// `cross` is the "ahead" cone, and within `arrive_radius` it stops.
    fn seek_control_code(
        self_pose_micro: (i64, i64, i64, i64),
        self_forward_micro: (i64, i64),
        goal_point_micro: (i64, i64, i64),
        arrive_radius_micro: i64,
    ) -> (u32, u32) {
        let dx = (goal_point_micro.0 - self_pose_micro.0) as f64 / Self::MICRO;
        let dz = (goal_point_micro.2 - self_pose_micro.2) as f64 / Self::MICRO;
        let fx = self_forward_micro.0 as f64 / Self::MICRO;
        let fz = self_forward_micro.1 as f64 / Self::MICRO;
        let arrive = arrive_radius_micro as f64 / Self::MICRO;

        let dist = (dx * dx + dz * dz).sqrt();
        let cross = fx * dz - fz * dx; // sign: which side the goal is on
        let dot = fx * dx + fz * dz; // > 0 when the goal is in front

        let arrived = u32::from(dist <= arrive);
        let active = 1 - arrived; // 0 once we have arrived (stop)
                                  // "Ahead": in front and within the cone. `dot.max(0.0)` keeps the cone
                                  // test meaningful only when the goal is actually in front.
        let ahead = u32::from((dot > 0.0) & (cross.abs() <= Self::AHEAD_TAN * dot.max(0.0)));
        let turning = (1 - ahead) & active;
        let turn_left = u32::from(cross > 0.0) & turning;
        let turn_right = u32::from(cross <= 0.0) & turning;

        let control = (Self::FORWARD * active)
            | (Self::TURN_LEFT * turn_left)
            | (Self::TURN_RIGHT * turn_right);
        (control, arrived)
    }

    /// The shared `observe → decide → lower` cycle: pack the neutral observation
    /// (self pose with height + goal), replay `control_code` through the substrate
    /// for a real report, and lower the emitted intent back to a control bitmask.
    fn decide_with_control(
        agent_raw_id: u64,
        tick: u64,
        self_pose_micro: (i64, i64, i64, i64),
        goal_point_micro: (i64, i64, i64),
        control_code: u32,
    ) -> (u32, u16, u16, usize) {
        let agent_id = AgentApi::create_agent_id(agent_raw_id);
        let profile = AgentApi::debug_perfect_profile();
        let mut memory = AgentApi::empty_memory(1);

        // Observe: pack the agent's pose (height in `y`) and the goal point.
        let mut builder = AgentApi::observation_builder(agent_id, Tick::new(tick), 1, 2, 0);
        builder
            .add_channel(AgentApi::channel_semantic())
            .expect("one channel within the channel bound");
        builder
            .add_fact(AgentApi::observation_fact(
                Self::FACT_SELF_POSE,
                Self::SUBJECT_SELF,
                self_pose_micro.0,
                self_pose_micro.1,
                self_pose_micro.2,
                self_pose_micro.3,
            ))
            .expect("self-pose fact within the fact bound");
        builder
            .add_fact(AgentApi::observation_fact(
                Self::FACT_GOAL_POINT,
                Self::SUBJECT_GOAL,
                goal_point_micro.0,
                goal_point_micro.1,
                goal_point_micro.2,
                0,
            ))
            .expect("goal fact within the fact bound");
        let observation = builder.build();

        // Decide + emit through the substrate (a one-shot replay of the control).
        let mut brain = AgentApi::replay_brain(vec![AgentApi::press_control_intent(control_code)]);
        let step = RuntimeStep::new(
            FrameIndex::new(0),
            Tick::new(tick),
            Self::FIXED_DELTA_NANOS,
            0,
        );
        let (report, mut queue) = AgentApi::step(
            agent_id,
            profile,
            &mut brain,
            &observation,
            &mut memory,
            step,
        );

        // Lower the emitted neutral intents back to a held-control bitmask: the
        // OR of every intent this tick emitted, so a multi-intent decision holds
        // all its controls (not just the first).
        let control = queue.combined_control_code();
        (
            control,
            report.reason_code(),
            report.selected_brain_kind_code(),
            report.emitted_action_count(),
        )
    }
}
