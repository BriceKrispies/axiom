//! **The game, as one object.** Ported from Claude-of-Duty
//! `src/player/index.js:143-345` — `PlayerSystem`'s init, `_resolveSpawn`,
//! `_consumeLook`, `fixedUpdate`, `update` and `_updateAds` — plus the frame
//! ordering `src/core/engine.js:239-318` gives them.
//!
//! This is the file where the ported subsystems stop being isolated. It owns:
//!
//! | held here          | ported in |
//! |--------------------|-----------|
//! | the level + its BVH | [`crate::scene::level`] over `crate::world` / `crate::physics` |
//! | the character       | [`crate::physics::character::Character`] |
//! | the movement machine | [`crate::player::movement::Movement`] |
//! | the camera feel      | [`crate::player::camera::CameraRig`] |
//! | the HUD model        | [`crate::ui::Hud`] |
//! | the sky's frame terms | [`crate::scene::sky_look`] |
//!
//! and nothing else: it decides no behaviour of its own beyond the seven lines
//! the source's `PlayerSystem` decides.
//!
//! ## What is honestly not connected
//!
//! * **`crate::weapons`** — the geometry kit is built and placed (see
//!   [`crate::scene::app`]), but there is no viewmodel rig (`rig.js`, unported)
//!   and no firing: `weapons/index.js`, `viewmodel.js` and `fire.js` are not
//!   ported, and `ballistics::RaycastWorld::fire_bullet` needs the unported
//!   penetration solver.
//! * **`crate::fx`** — [`crate::physics::probe::PhysicsWorld`] implements
//!   `FxWorld`, so the seam is closed, but `FxSystem`'s output is particle and
//!   decal *geometry*, which needs the unported render frame graph (instanced,
//!   additively-blended, atlas-sampled quads) to reach a pixel. Driving it
//!   would produce state nothing can draw.
//! * **`crate::audio`** — likewise: `AudioCore::set_world_probe` takes the same
//!   `PhysicsWorld`, but realising the graph needs `web_audio`'s bridge running
//!   on a user-gesture-unlocked `AudioContext`, which is a separate arm.
//! * **`crate::materials`** — the 19 surface generators bake CPU textures the
//!   port has no texture-upload path for yet; the level is flat-lit from the
//!   palette tints (see [`crate::scene::level::LevelBatch`]).
//!
//! Each of those is a missing *arm*, not a missing binding. Where a seam exists
//! it is bound; where the consumer does not exist, nothing is faked.

use crate::config::{Config, FIXED_DT, MAX_SUBSTEPS};
use crate::engine::{Time, CAPTURE_SEED};
use crate::input::{Action, Input};
use crate::physics::character::{Character, CharacterOpts};
use crate::physics::probe::PhysicsWorld;
use crate::physics::surfaces::mask;
use crate::player::camera::{CameraRig, Euler, HealthView};
use crate::events::EventBus;
use crate::player::mantle::{LedgeKind, WorldProbe};
use crate::player::movement::Movement;
use crate::player::springs::{approach, clamp, clamp01, lerp};
use crate::player::tuning::{CAMERA, MOVE, STAND};
use crate::rng::Rng;
use crate::scene::level::{build_level, Level, SpawnPoint};
use crate::scene::sky_look::{self, SkyLook};
use crate::ui::{CameraBasis, FramePull, Hud, HudFrame, PlayerPull};

/// `MAX_DT` — the frame delta clamp (`config.js`'s `MAX_DT`, applied at
/// `engine.js`'s step). A tab that was backgrounded must not resolve a
/// twenty-second step in one frame.
pub const MAX_DT: f64 = 0.1;

/// Gamepad look rate at full deflection, rad/s. `player/index.js:240`.
const STICK_LOOK_RATE: f64 = 3.1;

/// The camera pose one frame resolved to: where the eye is, how it is
/// oriented, and the vertical field of view in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub eye: [f64; 3],
    pub rotation: Euler,
    pub fov_degrees: f64,
}

/// The whole running game.
pub struct Game {
    pub config: Config,
    pub level: Level,
    pub physics: PhysicsWorld,
    pub movement: Movement,
    pub rig: CameraRig,
    pub hud: Hud,
    pub sky: SkyLook,
    pub time: Time,

    /// The fixed-step accumulator (`engine.js:271`).
    accumulator: f64,
    /// `this.controlEnabled` — false while paused.
    pub control_enabled: bool,
    /// `this.adsAmount`.
    pub ads_amount: f64,
    /// `this.adsRequested`.
    pub ads_requested: bool,
    /// The pause menu's state; `Escape` toggles it.
    pub paused: bool,
    /// `this._lookFrame` — look is consumed once per rendered frame.
    look_frame: Option<u64>,
    /// The spawn the player started at, kept for the report line the source
    /// logs (`player/index.js:193-198`).
    pub spawn: SpawnPoint,
    /// `ctx.events` — the pause menu emits through it. Nothing subscribes yet
    /// (the subsystems that would are unported), but the menu's API takes one
    /// and swallowing its emissions into a bus is honest where inventing a
    /// no-op is not.
    pub events: EventBus,
    /// The frame's *unscaled* delta. `Time` carries the scaled `dt`; the HUD's
    /// damped channels want the raw one (`index.js`'s `rawDt`).
    raw_dt: f64,
}

impl Game {
    /// Build the level, bind every seam, and put the player on the ground.
    ///
    /// `seed` is the engine root seed — [`CAPTURE_SEED`] is the source's own
    /// deterministic value (`engine.js:26`).
    pub fn new(seed: u32) -> Self {
        let config = Config::default();
        let mut root = Rng::new(seed);

        let level = build_level(&mut root);
        let physics = PhysicsWorld::new(level.world.clone());

        // `physics.createCharacter({...})` — `movement.js:141-149`'s dimensions.
        let mut character = Character::new(
            physics.world(),
            CharacterOpts {
                height: STAND.height,
                step_height: STAND.step_height,
                mask: mask::CHARACTER,
                ..CharacterOpts::default()
            },
        );

        // `_resolveSpawn()` — `player/index.js:199-211`. Physics owns the exact
        // floor; drop onto it so we never start embedded.
        let spawn = level.spawn(0);
        let feet_y = physics
            .ground_height(spawn.position[0], spawn.position[2], spawn.position[1] + 6.0)
            .map_or(spawn.position[1] + 0.2, |gy| gy + 0.03);
        let feet = [spawn.position[0], feet_y, spawn.position[2]];
        character.teleport(feet[0], feet[1], feet[2]);

        let mut movement = Movement::new();
        movement.init(Box::new(character), Some(feet));
        movement.yaw = spawn.yaw;
        movement.pitch = 0.0;

        let mut rig = CameraRig::new(f64::from(config.fov));
        rig.reset(STAND.eye);

        let sky = sky_look::resolve(sky_look::HOUR);

        let mut time = Time::default();
        time.fixed = FIXED_DT;
        time.scale = 1.0;

        let mut game = Game {
            config,
            level,
            physics,
            movement,
            rig,
            hud: Hud::new(root.fork()),
            sky,
            time,
            accumulator: 0.0,
            control_enabled: true,
            ads_amount: 0.0,
            ads_requested: false,
            paused: false,
            look_frame: None,
            events: EventBus::new(),
            raw_dt: 1.0 / 60.0,
            spawn: SpawnPoint {
                position: feet,
                yaw: spawn.yaw,
                tag: spawn.tag,
            },
        };
        // `rig.update(1/60, ...)` then `applyTo` — `player/index.js:157-159`,
        // so frame zero already has a settled camera rather than the origin.
        game.rig.update(
            1.0 / 60.0,
            &mut game.movement,
            health_view(),
            &game.config,
            &game.time,
        );
        game
    }

    /// Advance one rendered frame. `engine.js:239-318`'s ordering, with the
    /// player's `fixedUpdate`/`update` in their places:
    ///
    /// 1. `input.beginFrame()`
    /// 2. N × `fixedUpdate(FIXED_DT)` — the movement machine
    /// 3. `update(dt)` — ADS blend, camera rig, HUD
    ///
    /// Returns the camera pose the frame should be drawn from.
    pub fn frame(&mut self, raw_dt: f64, input: &mut Input) -> CameraPose {
        let dt = raw_dt.clamp(0.0, MAX_DT);
        self.raw_dt = dt;
        self.time.frame += 1;
        self.time.raw += dt;
        self.time.dt = dt * self.time.scale;
        self.time.elapsed += self.time.dt;
        self.time.fixed = FIXED_DT;

        let pad = None;
        input.begin_frame(&self.config, pad);
        self.handle_pause(input);

        // ---- fixed steps ----------------------------------------------------
        self.accumulator += self.time.dt;
        let mut steps = 0u32;
        while self.accumulator >= FIXED_DT && steps < MAX_SUBSTEPS {
            self.fixed_update(input);
            self.accumulator -= FIXED_DT;
            steps += 1;
        }
        if steps == MAX_SUBSTEPS {
            // Shed the backlog rather than spiral (`engine.js:281-285`).
            self.accumulator = 0.0;
        }
        self.time.alpha = self.accumulator / FIXED_DT;

        // ---- the rendered update -------------------------------------------
        self.consume_look(self.time.dt, input);
        self.movement.latch_input(&self.time, input);
        self.update_ads(self.time.dt, input);
        self.drain_movement_events();
        self.rig.update(
            self.time.dt,
            &mut self.movement,
            health_view(),
            &self.config,
            &self.time,
        );

        self.pose()
    }

    /// `fixedUpdate(h, ctx)`. `player/index.js:266-273`.
    fn fixed_update(&mut self, input: &Input) {
        let look_dt = if self.time.dt > 1e-5 {
            self.time.dt
        } else {
            FIXED_DT
        };
        self.consume_look(look_dt, input);
        self.movement.latch_input(&self.time, input);
        if !self.control_enabled {
            return;
        }
        self.movement.ads_amount = self.ads_amount;
        // The lean/ledge probes are the physics seam; `Movement` calls them
        // through `mantle::WorldProbe`, which `PhysicsWorld` implements.
        let probe: &dyn WorldProbe = &self.physics;
        self.movement.step(&self.time, Some(probe));
    }

    /// `_consumeLook(dt)`. `player/index.js:222-258`.
    fn consume_look(&mut self, dt: f64, input: &Input) {
        if self.look_frame == Some(self.time.frame) {
            return;
        }
        self.look_frame = Some(self.time.frame);
        if !self.control_enabled {
            self.movement.yaw_rate = 0.0;
            return;
        }
        let sens = lerp(
            1.0,
            f64::from(self.config.ads_sens_scale.get()),
            clamp01(self.ads_amount),
        );

        let mut d_yaw = -input.look.x * sens;
        let mut d_pitch = -input.look.y * sens;

        // Gamepad: rate-based, already curved by `Input`.
        let stick = input.stick;
        if stick.look_x != 0.0 || stick.look_y != 0.0 {
            let rate = STICK_LOOK_RATE * sens;
            d_yaw -= stick.look_x * rate * dt;
            d_pitch -= stick.look_y * rate * dt;
        }
        // Mantles are rooted: you keep your head, but the shoulders are
        // committed.
        if self.movement.mantle_motion.active {
            d_yaw *= 0.55;
            d_pitch *= 0.55;
        }

        self.movement.yaw += d_yaw;
        self.movement.pitch = clamp(
            self.movement.pitch + d_pitch,
            -CAMERA.pitch_limit,
            CAMERA.pitch_limit,
        );
        // Keep yaw bounded so long sessions never lose float precision.
        if self.movement.yaw > std::f64::consts::PI {
            self.movement.yaw -= std::f64::consts::PI * 2.0;
        } else if self.movement.yaw < -std::f64::consts::PI {
            self.movement.yaw += std::f64::consts::PI * 2.0;
        }

        self.movement.yaw_rate = if dt > 1e-5 { d_yaw / dt } else { 0.0 };
    }

    /// `_updateAds(dt)`. `player/index.js:304-319`, minus the `_adsExternal`
    /// arm (`weapons` drives that, and `weapons/index.js` is not ported).
    fn update_ads(&mut self, dt: f64, input: &Input) {
        self.ads_requested = self.control_enabled
            && input.ads()
            && !self.movement.mantle_motion.active
            && !self.movement.sliding;
        let target = if self.ads_requested { 1.0 } else { 0.0 };
        self.ads_amount = approach(self.ads_amount, target, 0.075, dt);
        self.movement.ads_amount = self.ads_amount;
    }

    /// `_drainMovementEvents()`. `player/index.js:322-360`, reduced to the two
    /// consumers this port has: the camera rig's landing dip and its footstep
    /// bob. The event bus emissions (`player:land`, `player:step`) are the
    /// audio/fx arms' inputs and have no listener here — see the module doc.
    fn drain_movement_events(&mut self) {
        if self.movement.land_event.pending {
            self.movement.land_event.pending = false;
            // The source's `if (mag > 0.35) m._footHold = FOOTSTEP.landHold`
            // is redundant here: `post_move` already sets `foot_hold` to
            // exactly that on the landing frame (`movement.js:865`), so the
            // rig's dip magnitude has nothing left to add.
            self.rig.on_land(self.movement.land_event.speed);
        }
        if self.movement.step_event.pending {
            self.movement.step_event.pending = false;
            self.rig
                .on_footstep(self.movement.sprinting, self.movement.stance);
        }
        if self.movement.slide_started {
            self.movement.slide_started = false;
            self.rig.on_slide_start(self.movement.slide_side);
        }
        self.movement.slide_ended = false;
        if self.movement.mantle_event.pending {
            self.movement.mantle_event.pending = false;
            self.rig.add_trauma(
                if self.movement.mantle_event.kind == LedgeKind::Vault {
                    0.08
                } else {
                    0.14
                },
            );
        }
        self.movement.jumped = false;
    }

    /// `Escape` toggles the pause menu, which is what releases pointer lock in
    /// the browser (the DOM does that itself) and what stops the movement
    /// machine from being driven. `ui/menu.js`'s `open`/`close`.
    fn handle_pause(&mut self, input: &Input) {
        if input.action_pressed(Action::Pause) {
            self.paused = !self.paused;
            self.control_enabled = !self.paused;
            if self.paused {
                self.hud.menu.show(None, &self.events);
            } else {
                self.hud.menu.close(None, &self.events);
            }
        }
    }

    /// The frame's camera pose — `rig.applyTo(camera)` (`camera.js:346-355`),
    /// as a value rather than a mutation of a `THREE.Camera`.
    pub fn pose(&self) -> CameraPose {
        CameraPose {
            eye: self.rig.eye_position,
            rotation: self.rig.rotation,
            fov_degrees: self.rig.fov,
        }
    }

    /// The camera's XZ basis, which the HUD's damage arcs and compass need —
    /// `index.js:486-496`'s read of `camera.matrixWorld`'s columns 0 and 2.
    /// Yaw alone determines it: pitch and roll do not change the XZ heading in
    /// the source either, because both columns are re-normalised after the XZ
    /// projection.
    pub fn camera_basis(&self) -> CameraBasis {
        let yaw = self.movement.yaw;
        CameraBasis {
            right_x: yaw.cos(),
            right_z: -yaw.sin(),
            forward_x: -yaw.sin(),
            forward_z: -yaw.cos(),
        }
    }

    /// Drive the HUD with this frame's real state — the `FramePull` seam bound
    /// to the movement machine. `weapon` stays `None`: no weapons subsystem is
    /// ported, and the HUD's own defaults are what the source shows when
    /// `ctx.peek('weapons')` is absent.
    pub fn hud_frame(&mut self) -> HudFrame {
        let basis = self.camera_basis();
        let position = self.movement.render_position;
        let pull = FramePull {
            weapon: None,
            player: Some(PlayerPull {
                health: Some(100.0),
                max_health: Some(100.0),
                armour: Some(0.0),
                regen: Some(false),
                move_amount: Some(clamp01(self.movement.horizontal_speed / MOVE.sprint_speed)),
                sprint: Some(self.movement.sprinting),
                crouch: Some(self.movement.stance != crate::player::tuning::Stance::Stand),
                ads: Some(self.ads_requested),
                airborne: Some(!self.movement.grounded),
                position: Some([position[0] as f32, position[1] as f32, position[2] as f32]),
            }),
            blips: &[],
            objectives: &[],
        };
        self.hud
            .late_update(self.time.dt, self.raw_dt, basis, pull)
    }
}

impl Default for Game {
    fn default() -> Self {
        Game::new(CAPTURE_SEED)
    }
}

/// The health subsystem's view of itself. `health.js` is not ported (it is the
/// damage/regen model, which needs the damage events no subsystem here emits),
/// so the rig is handed a full-health, unsuppressed reading — the same value
/// `Health` reports on a frame where nothing has happened.
fn health_view() -> HealthView {
    HealthView {
        fraction: 1.0,
        suppression: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::movement::MovementState;

    fn game() -> Game {
        Game::new(CAPTURE_SEED)
    }

    /// Run `frames` frames at 60 Hz with a given input state already latched.
    fn run(game: &mut Game, input: &mut Input, frames: usize) -> CameraPose {
        let mut pose = game.pose();
        for _ in 0..frames {
            pose = game.frame(1.0 / 60.0, input);
        }
        pose
    }

    #[test]
    fn the_player_spawns_standing_on_the_ground_at_eye_height() {
        let game = game();
        let feet = game.movement.position;
        assert!(
            (feet[1] - game.spawn.position[1]).abs() < 0.2,
            "feet at {}, spawn at {}",
            feet[1],
            game.spawn.position[1]
        );
        let eye = game.pose().eye;
        // The eye rides ~1.66 m over the feet at the standing stance.
        assert!(
            (eye[1] - feet[1] - STAND.eye).abs() < 0.15,
            "eye at {}, feet at {}",
            eye[1],
            feet[1]
        );
        assert!(
            eye[1] > feet[1],
            "the camera is above the floor, not inside it"
        );
    }

    #[test]
    fn an_idle_player_does_not_fall_through_the_world() {
        let mut game = game();
        let mut input = Input::new();
        let y0 = game.movement.position[1];
        run(&mut game, &mut input, 240);
        assert!(
            (game.movement.position[1] - y0).abs() < 0.2,
            "drifted from {} to {}",
            y0,
            game.movement.position[1]
        );
        assert!(game.movement.grounded);
    }

    #[test]
    fn holding_forward_walks_the_player_along_its_facing() {
        let mut game = game();
        let mut input = Input::new();
        let start = game.movement.position;
        input.key_down("KeyW");
        run(&mut game, &mut input, 120);
        let end = game.movement.position;
        let travelled = (end[0] - start[0]).hypot(end[2] - start[2]);
        assert!(travelled > 3.0, "walked only {travelled} m in two seconds");
        // And it is genuinely in the facing direction, not sideways.
        let fwd = (-game.movement.yaw.sin(), -game.movement.yaw.cos());
        let dot = ((end[0] - start[0]) * fwd.0 + (end[2] - start[2]) * fwd.1) / travelled;
        assert!(dot > 0.9, "moved off-axis, dot = {dot}");
    }

    #[test]
    fn mouse_look_turns_the_camera_and_clamps_the_pitch() {
        let mut game = game();
        let mut input = Input::new();
        input.pointer_locked = true;
        let yaw0 = game.movement.yaw;
        input.mouse_move(200.0, 0.0);
        game.frame(1.0 / 60.0, &mut input);
        assert!(game.movement.yaw < yaw0, "moving right yaws right");

        // Slam the mouse up for a while; the pitch must stop at the limit.
        for _ in 0..200 {
            input.mouse_move(0.0, -400.0);
            game.frame(1.0 / 60.0, &mut input);
        }
        assert!((game.movement.pitch - CAMERA.pitch_limit).abs() < 1e-9);
    }

    /// Pins the mouse-look roll (`CameraRig::turn_roll` et al., feeding
    /// `rig.rotation.roll`) down to and stable at zero once input stops. This
    /// is the decay half of the "camera tilts on its own" report: `yaw_rate`
    /// is recomputed fresh every frame from this frame's `input.look` (zero
    /// once the mouse stops), so `turn_roll`'s target collapses to zero and
    /// `approach` decays it there — never a runaway or a stuck bank. See
    /// `scene::app::combined_yaw_and_pitch_introduce_no_roll` for the other,
    /// larger half of that bug: a wrong Euler composition order in
    /// `write_camera` baked a *permanent*, non-decaying tilt into the
    /// rendered camera any time yaw and pitch were both nonzero, entirely
    /// independent of this `roll` channel.
    #[test]
    fn roll_settles_back_to_zero_after_look_input_stops() {
        let mut game = game();
        let mut input = Input::new();
        input.pointer_locked = true;

        // Pan at a jittery, non-60hz frame rate (like a real high-refresh
        // monitor) so both the `steps == 0` and `steps >= 1` paths through
        // `consume_look` run during the pan.
        let dts = [1.0 / 144.0, 1.0 / 120.0, 1.0 / 165.0, 1.0 / 90.0, 1.0 / 240.0];
        for (i, _) in (0..400).enumerate() {
            input.mouse_move(3.0, 0.0);
            game.frame(dts[i % dts.len()], &mut input);
        }
        assert!(
            game.rig.rotation.roll.abs() > 1e-4,
            "the pan should have banked the camera at all"
        );

        // Release the mouse; input.look is zero every frame from here on, so
        // yaw_rate must be exactly zero every frame and the roll must decay
        // to (and stay at) zero — not hunt, not plateau non-zero.
        for (i, _) in (0..300).enumerate() {
            game.frame(dts[i % dts.len()], &mut input);
            assert_eq!(
                game.movement.yaw_rate, 0.0,
                "yaw_rate must be exactly zero the instant look input stops"
            );
        }
        assert!(
            game.rig.rotation.roll.abs() < 1e-6,
            "roll never settled: {}",
            game.rig.rotation.roll
        );

        // And it stays there — not merely transiently near zero.
        for (i, _) in (0..60).enumerate() {
            game.frame(dts[i % dts.len()], &mut input);
            assert!(
                game.rig.rotation.roll.abs() < 1e-6,
                "roll drifted back off zero while idle: {}",
                game.rig.rotation.roll
            );
        }
    }

    #[test]
    fn crouching_lowers_the_eye_and_standing_raises_it_again() {
        let mut game = game();
        let mut input = Input::new();
        let standing = run(&mut game, &mut input, 10).eye[1];
        // Crouch is a TOGGLE in the source (`update_stance` reads
        // `cmd.crouchPressed`, the press edge), not a hold — so standing up
        // needs a second press, not a release.
        input.key_down("KeyC");
        let crouched = run(&mut game, &mut input, 90).eye[1];
        assert!(
            crouched < standing - 0.3,
            "eye went {standing} -> {crouched}"
        );
        assert_eq!(game.movement.stance, crate::player::tuning::Stance::Crouch);
        input.key_up("KeyC");
        run(&mut game, &mut input, 2);
        input.key_down("KeyC");
        let restood = run(&mut game, &mut input, 150).eye[1];
        assert_eq!(game.movement.stance, crate::player::tuning::Stance::Stand);
        assert!(restood > crouched + 0.3, "stood back up to {restood}");
    }

    #[test]
    fn jumping_leaves_the_ground_and_lands_again() {
        let mut game = game();
        let mut input = Input::new();
        run(&mut game, &mut input, 10);
        input.key_down("Space");
        run(&mut game, &mut input, 6);
        input.key_up("Space");
        let mut airborne = false;
        for _ in 0..120 {
            game.frame(1.0 / 60.0, &mut input);
            airborne |= !game.movement.grounded;
        }
        assert!(airborne, "the jump never left the floor");
        assert!(game.movement.grounded, "and it landed again");
    }

    #[test]
    fn escape_pauses_and_un_pauses_the_movement_machine() {
        let mut game = game();
        let mut input = Input::new();
        assert!(game.control_enabled);
        input.key_down("Escape");
        game.frame(1.0 / 60.0, &mut input);
        assert!(game.paused && !game.control_enabled);
        input.key_up("Escape");
        game.frame(1.0 / 60.0, &mut input);
        // While paused, forward does nothing.
        let before = game.movement.position;
        input.key_down("KeyW");
        run(&mut game, &mut input, 60);
        assert_eq!(game.movement.position, before);
        // Un-pause and it moves again.
        input.key_up("KeyW");
        input.key_down("Escape");
        game.frame(1.0 / 60.0, &mut input);
        input.key_up("Escape");
        input.key_down("KeyW");
        run(&mut game, &mut input, 60);
        assert!(game.control_enabled);
        assert_ne!(game.movement.position, before);
    }

    #[test]
    fn ads_blends_in_while_the_right_button_is_held() {
        let mut game = game();
        let mut input = Input::new();
        input.mouse_down(2);
        run(&mut game, &mut input, 60);
        assert!(game.ads_requested);
        assert!(game.ads_amount > 0.5, "got {}", game.ads_amount);
        assert!(game.rig.fov < f64::from(game.config.fov), "the FOV narrowed");
        input.mouse_up(2);
        run(&mut game, &mut input, 90);
        assert!(game.ads_amount < 0.1);
    }

    #[test]
    fn a_huge_frame_delta_is_clamped_rather_than_simulated_whole() {
        let mut game = game();
        let mut input = Input::new();
        let before = game.time.elapsed;
        game.frame(20.0, &mut input);
        assert!(
            game.time.elapsed - before <= MAX_DT + 1e-12,
            "a backgrounded tab resolved {} s in one frame",
            game.time.elapsed - before
        );
    }

    #[test]
    fn the_hud_frame_reads_the_real_movement_state() {
        let mut game = game();
        let mut input = Input::new();
        game.hud.resize(1280.0, 720.0);
        input.key_down("KeyW");
        input.key_down("ShiftLeft");
        run(&mut game, &mut input, 90);
        let frame = game.hud_frame();
        assert!(game.movement.sprinting, "sprinting with W + Shift held");
        assert!(game.hud.state.sprint, "and the HUD knows it");
        assert!(game.hud.state.move_amount > 0.0);
        assert!(frame.hud_visible > 0.0);
        // The compass heading follows the camera basis.
        let basis = game.camera_basis();
        assert!((basis.forward_x.hypot(basis.forward_z) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_frame_is_deterministic_for_the_same_inputs() {
        let script = |game: &mut Game, input: &mut Input| {
            input.key_down("KeyW");
            input.pointer_locked = true;
            for i in 0..120 {
                input.mouse_move(f64::from(i % 7) - 3.0, 1.0);
                game.frame(1.0 / 60.0, input);
            }
        };
        let mut a = game();
        let mut b = game();
        script(&mut a, &mut Input::new());
        script(&mut b, &mut Input::new());
        assert_eq!(a.pose(), b.pose());
        assert_eq!(a.movement.position, b.movement.position);
    }

    #[test]
    fn the_movement_state_machine_is_actually_being_driven() {
        let mut game = game();
        let mut input = Input::new();
        run(&mut game, &mut input, 30);
        assert_eq!(game.movement.state, MovementState::Stand);
        input.key_down("KeyW");
        input.key_down("ShiftLeft");
        run(&mut game, &mut input, 60);
        assert_eq!(game.movement.state, MovementState::Sprint);
    }
}
