//! The movement state machine.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/player/movement.js:1-996` — the
//! whole file.
//!
//! Runs at the fixed 120 Hz step so the feel is framerate-independent and
//! reproducible in capture mode. Collision is *entirely* delegated to the
//! [`CharacterController`] seam — this file only ever owns velocity and asks
//! the controller to resolve a displacement.
//!
//! States: stand · crouch · prone · sprint · tacsprint · slide · jump · fall ·
//! mantle · vault (+ lean, an additive modifier on any grounded state).
//!
//! Every transition is interruptible. Nothing here waits for an animation to
//! finish except the rooted mantle, and even that can be cut short by taking
//! damage or by control being disabled.
//!
//! ## Seams (see `crate::player` module doc comment)
//!
//! [`CharacterController`] stands in for `physics.createCharacter()`'s return
//! value (`this.character` in the source) — it is a supertrait of
//! [`crate::player::mantle::LedgeCharacter`], so one implementation of
//! `CharacterController` also satisfies `LedgeProbe::probe`. [`PlayerInput`]
//! stands in for `ctx.input` (`src/core/input.js`, not ported). `Time` and
//! `Config` are **not** re-seamed — `crate::engine::Time` already carries
//! every `ctx.time.*` field this file reads, and `movement.js` never reads
//! `ctx.config`.
//!
//! ## Deliberate shape divergence: `this.character`
//!
//! The source holds `this.character` as a field and every private method
//! reaches into it directly. Rust cannot borrow `self.character` mutably
//! while also calling `&mut self` helper methods that touch other fields, so
//! [`Movement::step`] (and every other public entry point that touches the
//! controller) temporarily *takes* `self.character` out of the struct,
//! threads it through the step as an explicit `&mut dyn CharacterController`
//! parameter, and restores it before returning. Behaviourally this is
//! identical to the source; it is purely how Rust has to hold the reference.

use crate::world::palette::Surface;
use crate::engine::Time;
use crate::player::mantle::{self, LedgeCharacter, LedgeKind, LedgeProbe, MantleMotion, WorldProbe};
use crate::player::springs;
use crate::player::tuning::{Stance, CROUCH, FOOTSTEP, MOVE, STAND};
use crate::player::Vec3;

use super::tuning::GRAVITY;
use super::tuning::JUMP_SPEED;

/// `STATES`. `movement.js:22-25`.
pub const STATES: [MovementState; 10] = [
    MovementState::Stand,
    MovementState::Crouch,
    MovementState::Prone,
    MovementState::Sprint,
    MovementState::TacSprint,
    MovementState::Slide,
    MovementState::Jump,
    MovementState::Fall,
    MovementState::Mantle,
    MovementState::Vault,
];

/// One of the ten authored movement states. `movement.js:22-25`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementState {
    Stand,
    Crouch,
    Prone,
    Sprint,
    TacSprint,
    Slide,
    Jump,
    Fall,
    Mantle,
    Vault,
}

/// The character-controller facts `movement.js` reads and writes on
/// `this.character`, beyond the three [`LedgeCharacter`] already names.
/// `physics.createCharacter({...})`'s returned shape.
pub trait CharacterController: LedgeCharacter {
    fn height(&self) -> f64;
    fn set_height(&mut self, h: f64);
    fn step_height(&self) -> f64;
    fn set_step_height(&mut self, h: f64);
    fn grounded(&self) -> bool;
    fn set_grounded(&mut self, g: bool);
    fn velocity(&self) -> Vec3;
    fn set_velocity(&mut self, v: Vec3);
    /// `c.canFit(height)`.
    fn can_fit(&self, height: f64) -> bool;
    /// `c.lastMoveBlocked`.
    fn last_move_blocked(&self) -> bool;
    /// `c.touchingCeiling`.
    fn touching_ceiling(&self) -> bool;
    /// `c.groundNormal`.
    fn ground_normal(&self) -> Vec3;
    /// `c.groundFriction`.
    fn ground_friction(&self) -> f64;
    /// `c.groundSurfaceName`.
    fn ground_surface(&self) -> Surface;
    /// `c.landingSpeed`.
    fn landing_speed(&self) -> f64;
    /// `c.move(dx, dy, dz)` — attempt the displacement, resolve collision
    /// (updating velocity/position/`grounded`/`lastMoveBlocked` as a side
    /// effect, exactly as the source's controller does), and return the
    /// distance actually travelled.
    fn move_by(&mut self, dx: f64, dy: f64, dz: f64) -> f64;
    /// `c.teleport(x, y, z)`.
    fn teleport_to(&mut self, x: f64, y: f64, z: f64);
    /// `c.setPosition(x, y, z)`.
    fn set_position(&mut self, x: f64, y: f64, z: f64);
    /// `c.depenetrate(iterations)`.
    fn depenetrate(&mut self, iterations: u32);
    /// `c.probeGround()`.
    fn probe_ground(&mut self);
}

/// The six named actions `ctx.input.action(name)` is queried with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    Jump,
    Crouch,
    Prone,
    Sprint,
    LeanLeft,
    LeanRight,
}

/// The `ctx.input` duck-type `latchInput` reads from. `movement.js:182-232`.
pub trait PlayerInput {
    /// `input.moveVector(cmd)`, folded with the immediate `cmd.moveX = cmd.x;
    /// cmd.moveY = cmd.y;` copy (`movement.js:200-202`) into one return value —
    /// the source's `cmd.x`/`cmd.y` never survive past that copy, so there is
    /// nothing lost by not naming them separately here.
    fn move_vector(&self) -> (f64, f64);
    fn action(&self, action: InputAction) -> bool;
    /// `ctx.input.stick.moveY` — raw analog Y before deadzone, used only for
    /// the tactical-sprint stick-flick shortcut (`movement.js:207`).
    fn stick_move_y(&self) -> f64;
    fn ads(&self) -> bool;
}

/// The latched per-rendered-frame input snapshot — the source's `this.cmd`.
/// `movement.js:100-107`. Does **not** carry `cronePressed`: a field present
/// in the source's object literal that is never read or written anywhere else
/// in `movement.js` (a `crouchPressed`/`pronePressed` near-duplicate, dead on
/// arrival) — dropped rather than ported inert.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerCommand {
    pub move_x: f64,
    pub move_y: f64,
    pub jump: bool,
    pub jump_held: bool,
    pub crouch_pressed: bool,
    pub prone_pressed: bool,
    pub sprint_held: bool,
    pub sprint_pressed: bool,
    pub lean_l: bool,
    pub lean_r: bool,
    pub ads: bool,
}

/// `this._prevHeld`. `movement.js:109-111`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct PrevHeld {
    jump: bool,
    crouch: bool,
    prone: bool,
    sprint: bool,
}

/// `this.landEvent`. `movement.js:121`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandEvent {
    pub pending: bool,
    pub speed: f64,
    pub surface: Surface,
}

impl Default for LandEvent {
    fn default() -> Self {
        LandEvent {
            pending: false,
            speed: 0.0,
            surface: Surface::Concrete,
        }
    }
}

/// `this.stepEvent`. `movement.js:122`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepEvent {
    pub pending: bool,
    pub running: bool,
    pub surface: Surface,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub left: bool,
}

impl Default for StepEvent {
    fn default() -> Self {
        StepEvent {
            pending: false,
            running: false,
            surface: Surface::Concrete,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            left: false,
        }
    }
}

/// `this.mantleEvent`. `movement.js:123`. `kind` is [`LedgeKind`] rather than
/// the source's `'none' | 'vault' | 'mantle'` string — the same information,
/// typed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MantleEvent {
    pub pending: bool,
    pub kind: LedgeKind,
    pub height: f64,
}

impl Default for MantleEvent {
    fn default() -> Self {
        MantleEvent {
            pending: false,
            kind: LedgeKind::None,
            height: 0.0,
        }
    }
}

/// The movement state machine. `movement.js:27-995`.
pub struct Movement {
    character: Option<Box<dyn CharacterController>>,
    probe: LedgeProbe,
    pub mantle_motion: MantleMotion,

    pub state: MovementState,
    pub prev_state: MovementState,
    pub state_time: f64,
    pub stance: Stance,
    pub stance_want: Stance,
    pub sprinting: bool,
    pub tactical_sprint: bool,
    pub sliding: bool,
    pub grounded: bool,
    pub was_grounded: bool,
    pub air_time: f64,
    pub ground_time: f64,
    pub speed: f64,
    pub horizontal_speed: f64,
    pub blocked: bool,

    pub yaw: f64,
    pub pitch: f64,
    pub yaw_rate: f64,

    /// 0..1 aim-down-sight blend. `weapons` may drive this directly, exactly
    /// as the source's `m.adsAmount = ...`.
    pub ads_amount: f64,
    pub control_enabled: bool,

    pub lean_input: f64,
    pub lean_amount: f64,
    pub lean_allowed: f64,
    pub lean_offset_x: f64,
    pub lean_offset_z: f64,
    lean_probe_timer: f64,

    coyote: f64,
    jump_buffer: f64,
    jump_cooldown: f64,
    sprint_hold_time: f64,
    last_sprint_press: f64,
    tac_sprint_time: f64,
    tac_sprint_lock: f64,
    slide_time: f64,
    slide_cooldown: f64,
    slide_dir_x: f64,
    slide_dir_z: f64,
    /// `this._slideSide` — which shoulder the slide leans over. Public because
    /// the frame driver reads it to give the camera rig its slide roll
    /// (`player/index.js:360-363`'s `this.rig.onSlideStart(m._slideSide)`).
    pub slide_side: f64,
    mantle_cooldown: f64,
    ledge_probe_timer: f64,
    step_distance: f64,
    bob_distance: f64,
    bob_phase: f64,
    foot_left: bool,
    foot_hold: f64,
    tac_sprint_requested: bool,
    edge_frame: Option<u64>,

    /// One-shot flags — consumed (and cleared) by the caller each frame,
    /// exactly as the source documents.
    pub jumped: bool,
    pub slide_started: bool,
    pub slide_ended: bool,

    pub cmd: PlayerCommand,
    cmd_frame: Option<u64>,
    prev_held: PrevHeld,

    pub prev_position: Vec3,
    pub position: Vec3,
    pub velocity: Vec3,
    pub render_position: Vec3,

    pub land_event: LandEvent,
    pub step_event: StepEvent,
    pub mantle_event: MantleEvent,

    // Basis for the current step, recomputed every `step()` call
    // (`movement.js:263-265`) and read by several helpers thereafter.
    fwd: Vec3,
    right: Vec3,
    prev_vy: f64,
}

impl Default for Movement {
    fn default() -> Self {
        Movement::new()
    }
}

impl Movement {
    pub fn new() -> Self {
        Movement {
            character: None,
            probe: LedgeProbe::new(),
            mantle_motion: MantleMotion::new(),

            state: MovementState::Stand,
            prev_state: MovementState::Stand,
            state_time: 0.0,
            stance: Stance::Stand,
            stance_want: Stance::Stand,
            sprinting: false,
            tactical_sprint: false,
            sliding: false,
            grounded: true,
            was_grounded: true,
            air_time: 0.0,
            ground_time: 0.0,
            speed: 0.0,
            horizontal_speed: 0.0,
            blocked: false,

            yaw: 0.0,
            pitch: 0.0,
            yaw_rate: 0.0,

            ads_amount: 0.0,
            control_enabled: true,

            lean_input: 0.0,
            lean_amount: 0.0,
            lean_allowed: 0.0,
            lean_offset_x: 0.0,
            lean_offset_z: 0.0,
            lean_probe_timer: 0.0,

            coyote: 0.0,
            jump_buffer: 0.0,
            jump_cooldown: 0.0,
            sprint_hold_time: 0.0,
            last_sprint_press: -10.0,
            tac_sprint_time: 0.0,
            tac_sprint_lock: 0.0,
            slide_time: 0.0,
            slide_cooldown: 0.0,
            slide_dir_x: 0.0,
            slide_dir_z: 1.0,
            slide_side: 1.0,
            mantle_cooldown: 0.0,
            ledge_probe_timer: 0.0,
            step_distance: 0.0,
            bob_distance: 0.0,
            bob_phase: 0.0,
            foot_left: false,
            foot_hold: 0.0,
            tac_sprint_requested: false,
            edge_frame: None,

            jumped: false,
            slide_started: false,
            slide_ended: false,

            cmd: PlayerCommand::default(),
            cmd_frame: None,
            prev_held: PrevHeld::default(),

            prev_position: [0.0, 0.0, 0.0],
            position: [0.0, 0.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            render_position: [0.0, 0.0, 0.0],

            land_event: LandEvent::default(),
            step_event: StepEvent::default(),
            mantle_event: MantleEvent::default(),

            fwd: [0.0, 0.0, -1.0],
            right: [1.0, 0.0, 0.0],
            prev_vy: 0.0,
        }
    }

    /* ==================================================================== */
    /* setup                                                                */
    /* ==================================================================== */

    /// `init(physics, spawn)`. `movement.js:138-157`. Narrower than the
    /// source: constructing the controller itself (`physics.createCharacter`
    /// with the radius/height/stepHeight/slopeLimit/snapDistance config at
    /// `movement.js:141-149`) is the physics seam's job, not this port's —
    /// `init` takes the already-built controller and does only the
    /// teleport-and-sync the source does with it.
    /* -------------------------------------------------------------- */
    /* Doors for the facade                                             */
    /*                                                                  */
    /* `player/index.js` reaches into `this.movement`'s private state    */
    /* directly. These four accessors are those exact reaches, named —   */
    /* not a widened API. Each cites the source line that writes the     */
    /* field, so the facade's transcription stays diffable.              */
    /* -------------------------------------------------------------- */

    /// `this.movement._footHold = FOOTSTEP.landHold` (`player/index.js:338`).
    pub fn set_foot_hold(&mut self, v: f64) {
        self.foot_hold = v;
    }

    /// `this.movement._cmdFrame = -1` (`player/index.js:630`) — a frame number
    /// no real frame equals, so the next latch always runs.
    pub fn invalidate_cmd_frame(&mut self) {
        self.cmd_frame = None;
    }

    /// `this.movement.latchInput(-2)` (`player/index.js:621`), whose only
    /// caller has already set `controlEnabled = false`, so it takes the flush
    /// branch.
    pub fn flush_latched_input(&mut self) {
        self.cmd = PlayerCommand::default();
        self.prev_held = PrevHeld::default();
        self.cmd_frame = None;
    }

    /// `m._beginSlide(m.cmd, m._wish.set(...), 1, MOVE.sprintSpeed)`
    /// (`player/index.js:692`) — `debugState('slide')` reaches into the private
    /// slide entry, so the facade needs a door.
    pub fn debug_begin_slide(&mut self, wish: Vec3, wish_len: f64, current_speed: f64) {
        let Some(mut character) = self.character.take() else {
            return;
        };
        let cmd = self.cmd;
        self.begin_slide(character.as_mut(), cmd, wish, wish_len, current_speed);
        self.character = Some(character);
    }

    pub fn init(&mut self, mut character: Box<dyn CharacterController>, spawn: Option<Vec3>) {
        if let Some(spawn) = spawn {
            character.teleport_to(spawn[0], spawn[1], spawn[2]);
        }
        self.position = character.position();
        self.prev_position = self.position;
        self.render_position = self.position;
        self.character = Some(character);
    }

    /// `dispose()`. `movement.js:159-162`. `physics.removeCharacter(c)` has no
    /// port equivalent (no physics registry exists yet); dropping the boxed
    /// controller is the Rust-native analogue of releasing it.
    pub fn dispose(&mut self) {
        self.character = None;
    }

    /// `get stanceDef()`. `movement.js:164-166`.
    pub fn stance_def(&self) -> crate::player::tuning::StanceDef {
        self.stance.def()
    }

    /// `get eyeHeight()`. Eye height for the *current* stance, before camera
    /// smoothing. `movement.js:168-171`.
    pub fn eye_height(&self) -> f64 {
        self.stance.def().eye
    }

    /* ==================================================================== */
    /* input                                                                */
    /* ==================================================================== */

    /// `latchInput(frame)`. `movement.js:182-232`. Latches the input snapshot
    /// for this rendered frame, so edge detection is exact regardless of how
    /// many fixed substeps run within it.
    pub fn latch_input(&mut self, time: &Time, input: &dyn PlayerInput) {
        let frame = time.frame;
        if self.cmd_frame == Some(frame) {
            return;
        }
        self.cmd_frame = Some(frame);

        if !self.control_enabled {
            self.cmd = PlayerCommand::default();
            self.prev_held = PrevHeld::default();
            return;
        }

        let (mx, my) = input.move_vector();
        self.cmd.move_x = mx;
        self.cmd.move_y = my;

        let jump = input.action(InputAction::Jump);
        let crouch = input.action(InputAction::Crouch);
        let prone = input.action(InputAction::Prone);
        let sprint = input.action(InputAction::Sprint) || input.stick_move_y().abs() > 0.92;

        let prev = self.prev_held;
        self.cmd.jump = jump && !prev.jump;
        self.cmd.jump_held = jump;
        self.cmd.crouch_pressed = crouch && !prev.crouch;
        self.cmd.prone_pressed = prone && !prev.prone;
        self.cmd.sprint_held = sprint;
        self.cmd.sprint_pressed = sprint && !prev.sprint;
        self.cmd.lean_l = input.action(InputAction::LeanLeft);
        self.cmd.lean_r = input.action(InputAction::LeanRight);
        self.cmd.ads = input.ads();

        self.prev_held = PrevHeld {
            jump,
            crouch,
            prone,
            sprint,
        };

        if self.cmd.jump {
            self.jump_buffer = MOVE.jump_buffer;
        }
        if self.cmd.sprint_pressed {
            let now = time.elapsed;
            if now - self.last_sprint_press < MOVE.tac_sprint_tap_window && self.tac_sprint_lock <= 0.0 {
                self.tac_sprint_requested = true;
            }
            self.last_sprint_press = now;
        }
    }

    /* ==================================================================== */
    /* the fixed step                                                       */
    /* ==================================================================== */

    /// `step(h)`. `movement.js:238-340`. `h` is `time.fixed`, matching the
    /// source's caller (the fixed loop always steps with `ctx.time.fixed`).
    pub fn step(&mut self, time: &Time, world: Option<&dyn WorldProbe>) {
        let Some(mut character) = self.character.take() else {
            return;
        };
        let c = character.as_mut();
        let h = time.fixed;

        // A rendered frame contains 0..N fixed steps but only ever *one* key
        // press. Edge flags are therefore consumed by the first substep of the
        // frame; the rest see them cleared.
        let frame = time.frame;
        if self.edge_frame != Some(frame) {
            self.edge_frame = Some(frame);
        } else {
            self.cmd.jump = false;
            self.cmd.crouch_pressed = false;
            self.cmd.prone_pressed = false;
            self.cmd.sprint_pressed = false;
        }
        let cmd = self.cmd;

        self.prev_position = self.position;
        self.state_time += h;
        self.tick_timers(h);

        // Basis for this step.
        let sy = self.yaw.sin();
        let cy = self.yaw.cos();
        self.fwd = [-sy, 0.0, -cy];
        self.right = [cy, 0.0, -sy];

        if self.mantle_motion.active {
            self.step_mantle(c, h);
            self.publish(c);
            self.character = Some(character);
            return;
        }

        // ---- wish direction, with directional speed weighting -------------
        let mx = cmd.move_x;
        let my = cmd.move_y;
        let raw_input = mx.hypot(my);
        let sx = mx * MOVE.strafe_scale;
        let sz = if my >= 0.0 { my } else { my * MOVE.back_scale };
        let mut wish_len = sx.hypot(sz);
        let wish: Vec3 = if wish_len > 1e-5 {
            let wx = self.fwd[0] * sz + self.right[0] * sx;
            let wz = self.fwd[2] * sz + self.right[2] * sx;
            let l = wx.hypot(wz);
            wish_len = wish_len.min(1.0);
            [wx / l, 0.0, wz / l]
        } else {
            wish_len = 0.0;
            [0.0, 0.0, 0.0]
        };
        let forward_intent = if raw_input > 1e-4 { my / raw_input } else { 0.0 };

        // ---- discrete decisions, in priority order -------------------------
        self.update_stance(c, cmd, raw_input);
        self.update_sprint(c, cmd, raw_input, forward_intent, h);
        self.update_slide(c, cmd, h, wish, wish_len);
        let jumped = self.update_jump(c);

        // ---- integrate velocity ---------------------------------------------
        if self.sliding {
            self.accelerate_slide(c, h, wish, wish_len);
        } else if c.grounded() && !jumped {
            self.accelerate_ground(c, h, wish, wish_len, raw_input);
        } else {
            self.accelerate_air(h, wish, wish_len);
        }

        if c.grounded() && !jumped && self.velocity[1] < 0.0 {
            self.velocity[1] = 0.0;
        }
        self.velocity[1] += GRAVITY * h;
        if self.velocity[1] < -MOVE.terminal_speed {
            self.velocity[1] = -MOVE.terminal_speed;
        }

        // ---- ledge detection (before the move, so we never fight the wall) -
        if self.try_ledge(c, world, wish, wish_len, cmd, forward_intent) {
            self.publish(c);
            self.character = Some(character);
            return;
        }

        // ---- resolve ---------------------------------------------------------
        self.prev_vy = self.velocity[1];
        c.set_velocity(self.velocity);
        let _travelled = c.move_by(self.velocity[0] * h, self.velocity[1] * h, self.velocity[2] * h);
        self.velocity = c.velocity();
        self.blocked = c.last_move_blocked();

        self.was_grounded = self.grounded;
        self.grounded = c.grounded();
        self.position = c.position();

        if c.touching_ceiling() && self.velocity[1] > 0.0 {
            self.velocity[1] = 0.0;
        }

        // ---- post-move bookkeeping ------------------------------------------
        self.post_move(c, world);
        self.update_lean(c, world, h, cmd);
        self.resolve_state();
        self.publish(c);
        self.character = Some(character);
    }

    fn tick_timers(&mut self, h: f64) {
        self.jump_buffer = (self.jump_buffer - h).max(0.0);
        self.jump_cooldown = (self.jump_cooldown - h).max(0.0);
        self.slide_cooldown = (self.slide_cooldown - h).max(0.0);
        self.mantle_cooldown = (self.mantle_cooldown - h).max(0.0);
        self.tac_sprint_lock = (self.tac_sprint_lock - h).max(0.0);
        self.foot_hold = (self.foot_hold - h).max(0.0);
        self.ledge_probe_timer = (self.ledge_probe_timer - h).max(0.0);
        self.lean_probe_timer = (self.lean_probe_timer - h).max(0.0);
        if self.grounded {
            self.coyote = MOVE.coyote_time;
            self.ground_time += h;
            self.air_time = 0.0;
        } else {
            self.coyote = (self.coyote - h).max(0.0);
            self.air_time += h;
            self.ground_time = 0.0;
        }
    }

    /* ==================================================================== */
    /* stance                                                               */
    /* ==================================================================== */

    /// `_updateStance`. `movement.js:366-397`.
    fn update_stance(&mut self, c: &mut dyn CharacterController, cmd: PlayerCommand, raw_input: f64) {
        if self.sliding {
            self.stance_want = Stance::Crouch;
        } else {
            if cmd.crouch_pressed {
                self.stance_want = if self.stance_want == Stance::Crouch {
                    Stance::Stand
                } else {
                    Stance::Crouch
                };
            }
            if cmd.prone_pressed {
                self.stance_want = if self.stance_want == Stance::Prone {
                    Stance::Crouch
                } else {
                    Stance::Prone
                };
            }
            // Sprinting always stands you up — CoD does not let you sprint
            // crouched.
            if cmd.sprint_held && raw_input > 0.5 && self.stance_want != Stance::Stand && cmd.move_y > 0.5 {
                self.stance_want = Stance::Stand;
            }
            if cmd.jump && self.stance_want != Stance::Stand {
                self.stance_want = Stance::Stand;
            }
        }

        if self.stance_want == self.stance {
            return;
        }
        let target = self.stance_want.def();
        if target.height <= self.stance_def().height {
            // Shrinking always succeeds.
            c.set_height(target.height);
            c.set_step_height(target.step_height);
            self.stance = self.stance_want;
        } else if c.can_fit(target.height) {
            c.set_height(target.height);
            c.set_step_height(target.step_height);
            self.stance = self.stance_want;
        }
        // else: blocked by a ceiling — keep asking every step until it clears.
    }

    /* ==================================================================== */
    /* sprint / tactical sprint                                             */
    /* ==================================================================== */

    /// `_updateSprint`. `movement.js:403-439`. `h` stands in for
    /// `this.ctx.time.fixed` (the same value `step`'s `h` already is).
    fn update_sprint(
        &mut self,
        c: &dyn CharacterController,
        cmd: PlayerCommand,
        raw_input: f64,
        forward_intent: f64,
        h: f64,
    ) {
        let want_sprint = cmd.sprint_held
            && raw_input > 0.45
            && forward_intent > MOVE.sprint_forward_dot
            && self.stance == Stance::Stand
            && !self.sliding
            && self.ads_amount < 0.3
            && (c.grounded() || self.sprinting);

        if want_sprint {
            self.sprint_hold_time += h;
            if self.sprint_hold_time >= MOVE.sprint_start_delay {
                self.sprinting = true;
            }
        } else {
            self.sprint_hold_time = 0.0;
            self.sprinting = false;
            self.tactical_sprint = false;
            self.tac_sprint_time = 0.0;
            self.tac_sprint_requested = false;
        }

        if self.sprinting {
            if self.tac_sprint_requested && !self.tactical_sprint {
                self.tactical_sprint = true;
                self.tac_sprint_time = 0.0;
            }
            self.tac_sprint_requested = false;
            if self.tactical_sprint {
                self.tac_sprint_time += h;
                if self.tac_sprint_time > MOVE.tac_sprint_max_time {
                    self.tactical_sprint = false;
                    self.tac_sprint_lock = MOVE.tac_sprint_recovery;
                }
            }
        }
    }

    /* ==================================================================== */
    /* slide                                                                */
    /* ==================================================================== */

    /// `_updateSlide`. `movement.js:445-479`.
    fn update_slide(&mut self, c: &mut dyn CharacterController, cmd: PlayerCommand, h: f64, wish: Vec3, wish_len: f64) {
        if !self.sliding {
            let fast = self.velocity[0].hypot(self.velocity[2]);
            let can_start = cmd.crouch_pressed
                && self.sprinting
                && c.grounded()
                && fast >= MOVE.slide.min_speed_to_start
                && self.slide_cooldown <= 0.0
                && self.mantle_cooldown <= 0.0;
            if can_start {
                self.begin_slide(c, cmd, wish, wish_len, fast);
            }
            return;
        }

        self.slide_time += h;

        // Slide-cancel: a jump out of a slide is the signature CoD movement
        // tech.
        if self.jump_buffer > 0.0 && c.grounded() && self.jump_cooldown <= 0.0 {
            self.end_slide(c, true);
            return;
        }
        // Standing up mid-slide, or losing the floor, or bleeding out of
        // speed.
        let sp = self.velocity[0].hypot(self.velocity[2]);
        if cmd.crouch_pressed
            || self.slide_time > MOVE.slide.duration
            || sp < MOVE.slide.exit_speed
            || (!c.grounded() && self.air_time > 0.14)
        {
            self.end_slide(c, false);
        }
    }

    /// `_beginSlide`. `movement.js:481-508`.
    fn begin_slide(
        &mut self,
        c: &mut dyn CharacterController,
        cmd: PlayerCommand,
        wish: Vec3,
        wish_len: f64,
        current_speed: f64,
    ) {
        let mut dx = self.velocity[0];
        let mut dz = self.velocity[2];
        let mut l = dx.hypot(dz);
        if l < 0.4 && wish_len > 0.1 {
            dx = wish[0];
            dz = wish[2];
            l = 1.0;
        }
        if l < 1e-4 {
            return;
        }
        dx /= l;
        dz /= l;

        let target = MOVE.slide.min_entry.max(MOVE.slide.entry_speed.min(current_speed * 1.3));
        self.velocity[0] = dx * target;
        self.velocity[2] = dz * target;
        self.slide_dir_x = dx;
        self.slide_dir_z = dz;
        self.slide_side = if cmd.move_x >= 0.0 { 1.0 } else { -1.0 };
        self.slide_time = 0.0;
        self.sliding = true;
        self.sprinting = false;
        self.tactical_sprint = false;
        self.stance_want = Stance::Crouch;
        // Force the capsule down immediately; a slide is a commitment.
        c.set_height(CROUCH.height);
        c.set_step_height(CROUCH.step_height);
        self.stance = Stance::Crouch;
        self.set_state(MovementState::Slide);
        self.slide_started = true;
    }

    /// `_endSlide`. `movement.js:510-534`.
    fn end_slide(&mut self, c: &mut dyn CharacterController, into_jump: bool) {
        self.sliding = false;
        self.slide_cooldown = MOVE.slide.cooldown;
        self.slide_time = 0.0;
        if into_jump {
            // Preserve the burst but never let it compound into a speed
            // exploit.
            let sp = self.velocity[0].hypot(self.velocity[2]);
            let cap = MOVE.sprint_speed * 1.06;
            if sp > cap {
                let s = cap / sp;
                self.velocity[0] *= s;
                self.velocity[2] *= s;
            }
            self.stance_want = Stance::Stand;
            if c.can_fit(STAND.height) {
                c.set_height(STAND.height);
                c.set_step_height(STAND.step_height);
                self.stance = Stance::Stand;
            }
            self.do_jump(c);
        } else {
            self.stance_want = Stance::Crouch;
        }
        self.slide_ended = true;
    }

    /// `_accelerateSlide`. `movement.js:536-572`.
    fn accelerate_slide(&mut self, c: &dyn CharacterController, h: f64, wish: Vec3, wish_len: f64) {
        let s = &MOVE.slide;
        let sp0 = self.velocity[0].hypot(self.velocity[2]);
        if sp0 < 1e-5 {
            return;
        }
        let mut dx = self.velocity[0] / sp0;
        let mut dz = self.velocity[2] / sp0;

        // Steering: lateral authority only, so the slide curves but never
        // pivots.
        if wish_len > 0.05 {
            let lat = wish[0] * -dz + wish[2] * dx; // wish . right(dir)
            let steer = lat * s.steer * h;
            let nx = dx - dz * steer; // dir + right(dir) * steer
            let nz = dz + dx * steer;
            let m = nx.hypot(nz);
            let l = if m.is_finite() && m != 0.0 { m } else { 1.0 };
            dx = nx / l;
            dz = nz / l;
        }

        // Downhill keeps a slide alive; uphill kills it fast.
        let gn = c.ground_normal();
        let slope = -(gn[0] * dx + gn[2] * dz);
        let mut sp = sp0 + slope * s.slope_assist * h;

        // Exponential drag plus a linear brake — the tail must actually
        // terminate.
        sp = sp * (-s.drag * h).exp() - s.brake * h;
        if sp < 0.0 {
            sp = 0.0;
        }

        // Surface friction as a *rate*, not a per-step multiplier: sand eats a
        // slide, sheet metal barely touches it.
        sp -= sp * springs::clamp(c.ground_friction() - 0.55, 0.0, 0.8) * 0.62 * h;
        if sp < 0.0 {
            sp = 0.0;
        }

        self.velocity[0] = dx * sp;
        self.velocity[2] = dz * sp;
        self.slide_dir_x = dx;
        self.slide_dir_z = dz;
    }

    /// `get slideProgress()`. `movement.js:574-576`.
    pub fn slide_progress(&self) -> f64 {
        if self.sliding {
            springs::clamp01(self.slide_time / MOVE.slide.duration)
        } else {
            0.0
        }
    }

    /* ==================================================================== */
    /* jump                                                                 */
    /* ==================================================================== */

    /// `_updateJump`. `movement.js:582-599`. Drops the source's `cmd`
    /// parameter: it is never read anywhere in the method body.
    fn update_jump(&mut self, c: &mut dyn CharacterController) -> bool {
        if self.sliding {
            return false;
        }
        if self.jump_buffer <= 0.0 {
            return false;
        }
        if self.jump_cooldown > 0.0 {
            return false;
        }
        if !c.grounded() && self.coyote <= 0.0 {
            return false;
        }

        // You stand up before you jump; if a ceiling forbids it, you do not
        // jump.
        if self.stance != Stance::Stand {
            if !c.can_fit(STAND.height) {
                return false;
            }
            c.set_height(STAND.height);
            c.set_step_height(STAND.step_height);
            self.stance = Stance::Stand;
            self.stance_want = Stance::Stand;
        }
        self.do_jump(c);
        true
    }

    /// `_doJump`. `movement.js:601-611`.
    fn do_jump(&mut self, c: &mut dyn CharacterController) {
        self.velocity[1] = *JUMP_SPEED;
        self.jump_buffer = 0.0;
        self.jump_cooldown = MOVE.jump_cooldown;
        self.coyote = 0.0;
        self.grounded = false;
        c.set_grounded(false);
        self.jumped = true;
        self.set_state(MovementState::Jump);
    }

    /* ==================================================================== */
    /* acceleration                                                         */
    /* ==================================================================== */

    /// `targetSpeed()`. `movement.js:617-627`.
    pub fn target_speed(&self) -> f64 {
        let mut base = if self.sprinting {
            if self.tactical_sprint {
                MOVE.tac_sprint_speed
            } else {
                MOVE.sprint_speed
            }
        } else {
            let mut b = self.stance.def().speed;
            b *= springs::lerp(1.0, MOVE.ads_scale, springs::clamp01(self.ads_amount));
            b
        };
        base *= springs::lerp(1.0, 0.6, springs::clamp01(self.lean_amount.abs()));
        base
    }

    /// `_accelerateGround`. `movement.js:629-672`.
    fn accelerate_ground(&mut self, c: &dyn CharacterController, h: f64, wish: Vec3, wish_len: f64, raw_input: f64) {
        let speed = self.target_speed() * wish_len;

        let mut tx = wish[0] * speed;
        let mut tz = wish[2] * speed;

        // Walk along the ground plane rather than into it, so slopes do not
        // steal speed and ramps do not launch you.
        let gn = c.ground_normal();
        if gn[1] > 0.1 && gn[1] < 0.999 && (tx != 0.0 || tz != 0.0) {
            let d = tx * gn[0] + tz * gn[2];
            let px = tx - gn[0] * d;
            let pz = tz - gn[2] * d;
            let l = px.hypot(pz);
            if l > 1e-5 {
                let want = tx.hypot(tz);
                tx = (px / l) * want;
                tz = (pz / l) * want;
            }
        }

        let dx = tx - self.velocity[0];
        let dz = tz - self.velocity[2];
        let dl = dx.hypot(dz);
        if dl < 1e-6 {
            return;
        }

        let cur = self.velocity[0].hypot(self.velocity[2]);
        let mut rate = if raw_input < 0.02 {
            MOVE.stop_decel
        } else if speed < cur * 0.92 {
            MOVE.ground_decel
        } else {
            MOVE.ground_accel
        };
        // Rough ground (sand, dirt) responds a little more sluggishly.
        rate *= springs::clamp(c.ground_friction() + 0.08, 0.75, 1.05);

        let step = rate * h;
        if dl <= step {
            self.velocity[0] = tx;
            self.velocity[2] = tz;
        } else {
            let s = step / dl;
            self.velocity[0] += dx * s;
            self.velocity[2] += dz * s;
        }
    }

    /// Air control: a quarter of ground authority, and it may only add speed
    /// along the wish direction up to `airSpeedCap`. Existing momentum (a
    /// slide-cancel launch, say) is preserved — you can steer it but not
    /// amplify it. `_accelerateAir`. `movement.js:679-690`.
    fn accelerate_air(&mut self, h: f64, wish: Vec3, wish_len: f64) {
        if wish_len < 1e-4 {
            return;
        }
        let cap = MOVE.air_speed_cap * wish_len;
        let along = self.velocity[0] * wish[0] + self.velocity[2] * wish[2];
        let add = cap - along;
        if add <= 0.0 {
            return;
        }
        let accel = MOVE.ground_accel * MOVE.air_accel_scale * wish_len * h;
        let gain = accel.min(add);
        self.velocity[0] += wish[0] * gain;
        self.velocity[2] += wish[2] * gain;
    }

    /* ==================================================================== */
    /* mantle / vault                                                       */
    /* ==================================================================== */

    /// `_tryLedge`. `movement.js:696-760`.
    fn try_ledge(
        &mut self,
        c: &mut dyn CharacterController,
        world: Option<&dyn WorldProbe>,
        wish: Vec3,
        wish_len: f64,
        cmd: PlayerCommand,
        forward_intent: f64,
    ) -> bool {
        if self.mantle_cooldown > 0.0 || self.sliding {
            return false;
        }
        if wish_len < 0.35 || forward_intent < 0.4 {
            return false;
        }
        if self.stance == Stance::Prone {
            return false;
        }
        if self.ledge_probe_timer > 0.0 {
            return false;
        }

        let sp = self.velocity[0].hypot(self.velocity[2]);

        // Cheap gate: only probe when something is actually in the way, when
        // we are descending onto a lip, or when the player asked for it with
        // jump.
        let pressing = cmd.jump_held || cmd.jump;
        let blocked_now = c.last_move_blocked() && sp > 0.3;
        let descending = !c.grounded() && self.velocity[1] < 1.0;
        // The character controller's step offset happily *lifts* the capsule
        // onto a knee-high box without ever reporting a blocked move, so
        // waiting to be blocked means you float up low walls instead of
        // vaulting them.
        let closing = c.grounded() && sp >= MOVE.mantle.auto_speed;
        if !(blocked_now || descending || closing || (pressing && c.grounded())) {
            return false;
        }
        // Probe rate scales with speed. At 7 m/s a fixed 20 Hz probe travels
        // 0.35 m between samples and skips clean over the narrow window in
        // which a vault is still possible.
        self.ledge_probe_timer = springs::clamp(0.1 / 1.5_f64.max(sp), 0.008, 0.05);

        let Some(world) = world else {
            return false;
        };
        let kind = self.probe.probe(world, &*c, wish[0], wish[2], STAND.height);
        if kind == LedgeKind::None {
            return false;
        }

        let r = self.probe.result;
        let auto = r.fast && sp >= MOVE.mantle.auto_speed;

        // A proactive vault (nothing blocked us, no jump pressed) has to be
        // certain, or a staircase turns into a series of animations.
        if closing && !blocked_now && !pressing {
            let reach = MOVE.mantle.proactive_distance + sp * MOVE.mantle.proactive_lookahead;
            if r.distance > reach {
                return false;
            }
            if r.obstacle_height < c.step_height() + 0.07 {
                return false;
            }
        }

        // Low obstacles are cleared automatically at speed; anything taller
        // is an explicit action so you never get yanked up a wall by
        // accident.
        if !auto && !pressing {
            return false;
        }

        let side = if cmd.move_x >= 0.0 { 1.0 } else { -1.0 };
        self.mantle_motion.begin(&r, &*c, wish[0], wish[2], side, sp);
        self.velocity = [0.0, 0.0, 0.0];
        c.set_velocity([0.0, 0.0, 0.0]);
        self.jump_buffer = 0.0;
        self.sprinting = false;
        self.tactical_sprint = false;
        self.mantle_event.pending = true;
        self.mantle_event.kind = kind;
        self.mantle_event.height = r.obstacle_height;
        self.set_state(if kind == LedgeKind::Vault {
            MovementState::Vault
        } else {
            MovementState::Mantle
        });
        true
    }

    /// `_stepMantle`. `movement.js:762-791`.
    fn step_mantle(&mut self, c: &mut dyn CharacterController, h: f64) {
        // You cannot peek round a corner with both hands on a ledge.
        self.lean_amount = springs::approach(self.lean_amount, 0.0, MOVE.lean.rate, h);
        self.lean_offset_x = self.right[0] * self.lean_amount * MOVE.lean.offset;
        self.lean_offset_z = self.right[2] * self.lean_amount * MOVE.lean.offset;
        let alive = self.mantle_motion.step(h);
        let (px, py, pz) = (self.mantle_motion.px, self.mantle_motion.py, self.mantle_motion.pz);
        c.set_position(px, py, pz);
        self.position = [px, py, pz];
        self.was_grounded = self.grounded;
        self.grounded = false;
        if !alive {
            let (lx, ly, lz) = (
                self.mantle_motion.land_x,
                self.mantle_motion.land_y,
                self.mantle_motion.land_z,
            );
            let (fx, fz, exit_speed) = (
                self.mantle_motion.fx,
                self.mantle_motion.fz,
                self.mantle_motion.exit_speed,
            );
            self.mantle_motion.end();
            c.set_position(lx, ly, lz);
            self.position = [lx, ly, lz];
            c.depenetrate(4);
            c.probe_ground();
            self.grounded = c.grounded();
            self.was_grounded = true; // suppress a bogus landing event
            self.velocity[0] = fx * exit_speed;
            self.velocity[2] = fz * exit_speed;
            self.velocity[1] = 0.0;
            self.mantle_cooldown = MOVE.mantle.cooldown;
            self.step_distance = 0.0;
            self.foot_hold = FOOTSTEP.land_hold;
            self.resolve_state();
        }
    }

    /// `cancelMantle()`. `movement.js:793-803`. The source assumes
    /// `this.character` is non-null here (it would throw otherwise); this
    /// port degrades to a no-op on a missing controller instead, a documented
    /// divergence in favour of not panicking a public API.
    pub fn cancel_mantle(&mut self) {
        if !self.mantle_motion.active {
            return;
        }
        let (lx, ly, lz) = (
            self.mantle_motion.land_x,
            self.mantle_motion.land_y,
            self.mantle_motion.land_z,
        );
        self.mantle_motion.end();
        if let Some(c) = self.character.as_mut() {
            c.set_position(lx, ly, lz);
            self.position = [lx, ly, lz];
            c.depenetrate(4);
            c.probe_ground();
        }
        self.mantle_cooldown = MOVE.mantle.cooldown;
        self.resolve_state();
    }

    /* ==================================================================== */
    /* lean                                                                 */
    /* ==================================================================== */

    /// `_updateLean`. `movement.js:809-827`.
    fn update_lean(&mut self, c: &dyn CharacterController, world: Option<&dyn WorldProbe>, h: f64, cmd: PlayerCommand) {
        let mut want = f64::from(cmd.lean_r) - f64::from(cmd.lean_l);
        if self.sprinting || self.sliding || !self.grounded || self.stance == Stance::Prone {
            want = 0.0;
        }
        self.lean_input = want;

        // Validate against the world at ~30 Hz — the camera must never poke
        // through a wall, so we shorten the lean until the probe capsule is
        // clear.
        if self.lean_probe_timer <= 0.0 {
            self.lean_probe_timer = 1.0 / 30.0;
            self.lean_allowed = if want == 0.0 {
                0.0
            } else {
                self.probe_lean(c, world, want)
            };
        }
        let target = want * self.lean_allowed;
        self.lean_amount = springs::approach(self.lean_amount, target, MOVE.lean.rate, h);
        if self.lean_amount.abs() < 1e-4 {
            self.lean_amount = 0.0;
        }

        let off = self.lean_amount * MOVE.lean.offset;
        self.lean_offset_x = self.right[0] * off;
        self.lean_offset_z = self.right[2] * off;
    }

    /// `_probeLean`. `movement.js:829-844`.
    fn probe_lean(&self, c: &dyn CharacterController, world: Option<&dyn WorldProbe>, side: f64) -> f64 {
        let Some(world) = world else {
            return 0.0;
        };
        let pos = c.position();
        let eye = pos[1] + self.eye_height();
        let l = &MOVE.lean;
        for i in 0..3_i32 {
            let amt = 1.0 - f64::from(i) * 0.33;
            let dx = self.right[0] * side * l.offset * amt;
            let dz = self.right[2] * side * l.offset * amt;
            let p0 = [pos[0] + dx, eye - 0.22, pos[2] + dz];
            let p1 = [pos[0] + dx, eye + 0.06, pos[2] + dz];
            if world.check_capsule_segment(p0, p1, l.probe_radius, mantle::ProbeMask::World) {
                return amt;
            }
        }
        0.0
    }

    /* ==================================================================== */
    /* post-move                                                            */
    /* ==================================================================== */

    /// `_postMove(h, travelled)`. `movement.js:850-889`. Drops both
    /// parameters: neither `h` nor `travelled` is read anywhere in the
    /// source's method body.
    fn post_move(&mut self, c: &mut dyn CharacterController, world: Option<&dyn WorldProbe>) {
        let v = self.velocity;
        self.speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        self.horizontal_speed = v[0].hypot(v[2]);

        // ---- landing ---------------------------------------------------------
        if self.grounded && !self.was_grounded {
            let impact = c.landing_speed().max(-(0.0_f64.min(self.prev_vy)));
            self.land_event.pending = true;
            self.land_event.speed = impact;
            self.land_event.surface = c.ground_surface();
            self.foot_hold = FOOTSTEP.land_hold;
            self.step_distance = 0.0;
            if self.sliding {
                self.end_slide(c, false);
            }
        }

        // ---- footstep cadence -------------------------------------------------
        let dx = self.position[0] - self.prev_position[0];
        let dz = self.position[2] - self.prev_position[2];
        let moved = dx.hypot(dz);
        if self.grounded && !self.sliding {
            self.step_distance += moved;
            self.bob_distance += moved;
            let stride = self.stance.def().stride_length * if self.sprinting { 1.28 } else { 1.0 };
            // One footfall = pi of bob phase, so the camera's horizontal
            // extreme and the footstep event are the same event by
            // construction.
            self.bob_phase += (moved / stride) * std::f64::consts::PI;
            if self.bob_phase > std::f64::consts::PI * 4.0 {
                self.bob_phase -= std::f64::consts::PI * 4.0;
            }
            if self.step_distance >= stride && self.horizontal_speed > 0.55 && self.foot_hold <= 0.0 {
                self.step_distance -= stride;
                self.foot_left = !self.foot_left;
                self.emit_footstep(c, world);
            }
        } else {
            self.bob_distance += moved * 0.25;
            if !self.grounded {
                self.step_distance = self.stance.def().stride_length * 0.55;
            }
        }
    }

    /// `_emitFootstep`. `movement.js:891-915`.
    fn emit_footstep(&mut self, c: &dyn CharacterController, world: Option<&dyn WorldProbe>) {
        let lateral = if self.foot_left { -FOOTSTEP.lateral } else { FOOTSTEP.lateral };
        let pos = c.position();
        let fx = pos[0] + self.right[0] * lateral;
        let fz = pos[2] + self.right[2] * lateral;

        // Query the surface *under the foot*, not under the capsule centre —
        // a step that lands half on a kerb should sound like the kerb.
        let mut y = pos[1];
        let mut surface = c.ground_surface();
        if let Some(world) = world {
            if let Some(hit) = world.raycast(
                [fx, pos[1] + 0.35, fz],
                [0.0, -1.0, 0.0],
                FOOTSTEP.probe,
                mantle::ProbeMask::World,
            ) {
                y = hit.point[1];
                surface = hit.surface;
            }
        }
        self.step_event.pending = true;
        self.step_event.running = self.horizontal_speed >= FOOTSTEP.run_speed;
        self.step_event.surface = surface;
        self.step_event.x = fx;
        self.step_event.y = y;
        self.step_event.z = fz;
        self.step_event.left = self.foot_left;
    }

    /* ==================================================================== */
    /* state resolution                                                     */
    /* ==================================================================== */

    /// `_resolveState`. `movement.js:921-931`.
    fn resolve_state(&mut self) {
        if self.mantle_motion.active {
            return;
        }
        let next = if self.sliding {
            MovementState::Slide
        } else if !self.grounded {
            if self.velocity[1] > 0.35 {
                MovementState::Jump
            } else {
                MovementState::Fall
            }
        } else if self.stance == Stance::Prone {
            MovementState::Prone
        } else if self.stance == Stance::Crouch {
            MovementState::Crouch
        } else if self.sprinting {
            if self.tactical_sprint {
                MovementState::TacSprint
            } else {
                MovementState::Sprint
            }
        } else {
            MovementState::Stand
        };
        self.set_state(next);
    }

    /// `_setState`. `movement.js:933-938`.
    fn set_state(&mut self, next: MovementState) {
        if next == self.state {
            return;
        }
        self.prev_state = self.state;
        self.state = next;
        self.state_time = 0.0;
    }

    /// `_publish`. `movement.js:940-946`.
    fn publish(&mut self, c: &dyn CharacterController) {
        self.position = c.position();
        let v = self.velocity;
        self.speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        self.horizontal_speed = v[0].hypot(v[2]);
    }

    /// Interpolated feet position for rendering. `sampleRender(alpha)`.
    /// `movement.js:949-952`.
    pub fn sample_render(&mut self, alpha: f64) -> Vec3 {
        let t = springs::clamp01(alpha);
        self.render_position = [
            springs::lerp(self.prev_position[0], self.position[0], t),
            springs::lerp(self.prev_position[1], self.position[1], t),
            springs::lerp(self.prev_position[2], self.position[2], t),
        ];
        self.render_position
    }

    /* ==================================================================== */
    /* external control                                                     */
    /* ==================================================================== */

    /// `teleport(x, y, z)`. `movement.js:958-985`.
    pub fn teleport(&mut self, x: f64, y: f64, z: f64) {
        let Some(mut character) = self.character.take() else {
            return;
        };
        let c = character.as_mut();
        self.mantle_motion.end();
        self.sliding = false;
        self.sprinting = false;
        self.tactical_sprint = false;
        self.stance = Stance::Stand;
        self.stance_want = Stance::Stand;
        c.set_height(STAND.height);
        c.set_step_height(STAND.step_height);
        c.teleport_to(x, y, z);
        self.velocity = [0.0, 0.0, 0.0];
        self.position = c.position();
        self.prev_position = self.position;
        self.render_position = self.position;
        self.grounded = c.grounded();
        self.was_grounded = self.grounded;
        self.lean_amount = 0.0;
        self.lean_offset_x = 0.0;
        self.lean_offset_z = 0.0;
        self.step_distance = 0.0;
        self.bob_distance = 0.0;
        self.bob_phase = 0.0;
        self.foot_hold = 0.0;
        self.jump_buffer = 0.0;
        self.land_event.pending = false;
        self.step_event.pending = false;
        self.set_state(MovementState::Stand);
        self.character = Some(character);
    }

    /// `get bobDistance()`. `movement.js:987-989`.
    pub fn bob_distance(&self) -> f64 {
        self.bob_distance
    }

    /// Radians of gait phase; pi per footfall. Drives the camera's
    /// figure-eight. `get stepPhase()`. `movement.js:991-994`.
    pub fn step_phase(&self) -> f64 {
        self.bob_phase
    }
}
