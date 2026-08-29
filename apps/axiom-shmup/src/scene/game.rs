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
//! | the HUD              | [`crate::scene::wiring::hud::HudRig`] (`ui/index.js`) |
//! | the sky's frame terms | [`crate::scene::wiring::look`] (`sky/index.js`) |
//!
//! and nothing else: it decides no behaviour of its own beyond the seven lines
//! the source's `PlayerSystem` decides.
//!
//! ## What is honestly not connected
//!
//! * **`crate::weapons`** — the geometry kit is built and placed, and the
//!   **viewmodel rig now drives it** (`crate::scene::app::drive_viewmodel`):
//!   sway, breathing, the lag layer and the ADS transition all run against real
//!   player state.
//!
//!   Still not connected: **firing**. `weapons::system` is a complete port of
//!   `weapons/index.js:1-843` and nothing constructs it, so `trigger` is always
//!   false and no recoil, no muzzle flash and no shell ejection reach the frame.
//!   Per-part animation is also absent for a structural reason rather than a
//!   missing port — the rifle's buckets are merged **per material**, so there is
//!   no bolt node to slide.
//!
//!   Every claim in this list is a dated one. The line that used to sit here
//!   said `viewmodel.js` was unported; it had been ported for some time, and
//!   the rifle lay in the road on the strength of it.
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

use crate::scene::wiring::hud::{HudPull, HudRig};
use crate::ui::system::{UiClock, UiFrame};
use crate::ui::PlayerPull;

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
    /// `ui/index.js` — the whole HUD: eleven widgets, their DOM views, the
    /// seven event subscriptions and the effect journal. This used to be
    /// `ui::Hud`, a model-only second port of the same file with no view.
    pub hud: HudRig,
    /// `sky/index.js` — the real ephemeris, settable hour, weather and the
    /// moon key-handover. This replaced `scene::sky_look`, which computed the
    /// same sun from a FROZEN `HOUR` constant and omitted the aureole exponent,
    /// the beam floor, the cloud dimmer and the moon. Keeping both would have
    /// been two suns, one of which could not move.
    pub sky: crate::scene::wiring::look::SkyDriver,
    /// `materials/index.js` — the per-key parameter merge. Held for the frame
    /// rather than dropped, which is why all 46 keys used to share one
    /// hand-authored surface.
    pub materials: crate::scene::wiring::look::MaterialLook,
    /// **The three subsystem streams this port forks but does not yet spend.**
    ///
    /// `core/engine.js:143-145` runs one prepare pass over `registry.resolve()`
    /// order and every subsystem forks the root stream there. Three of those
    /// subsystems have no live consumer inside `Game`, and their forks are held
    /// here rather than dropped: the *position* of a draw in the root sequence
    /// is what seeds everything after it, so a fork nobody spends is still part
    /// of the level. `crate::player::system::PlayerCore::rng` documents the same
    /// contract one level down.
    ///
    /// `render/index.js:142-145`, the FIRST draw of the sequence. The source
    /// spends it on `new RenderProbeScene(this.rng.fork())`
    /// (`render/index.js:515`); this port has no probe scene.
    pub render_rng: Rng,
    /// `physics/index.js:252-255`, handed straight to `this.ballistics.rng`
    /// (`index.js:260`) and read by every spread and ricochet draw
    /// (`index.js:724`, `:816`, `:1029`).
    /// [`crate::physics::system::PhysicsCore::set_rng`] is where this belongs;
    /// `Game` drives [`PhysicsWorld`] -- the BVH query facade -- instead, so the
    /// stream waits here until the ballistics arm is wired.
    pub physics_rng: Rng,
    /// `player/index.js:153-156`. Never read anywhere in `player/index.js`;
    /// dead computation in the source is still part of the source.
    pub player_rng: Rng,

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

    // ---- the ported subsystem facades ------------------------------------
    //
    // Each of these is a complete port of a `<name>/index.js` that, until now,
    // nothing constructed. Their construction ORDER in `Game::new` is not a
    // style choice — see `crate::scene::wiring`'s module doc.
    /// `fx/index.js` + `audio/index.js`.
    pub fx_audio: crate::scene::wiring::fx_audio::FxAudio,
    /// Land/step events the source emits on the bus and this port only cleared.
    pub pulse: crate::scene::wiring::fx_audio::MovementPulse,
    /// `weapons/index.js` — the firing machine. **It owns the viewmodel**, so
    /// nothing else may construct one: `scene::app` briefly held a second, which
    /// rendered a rifle that could not recoil because the recoil lands on the
    /// core's copy.
    pub weapons: crate::scene::wiring::weapons::WeaponsRig,
    /// What the weapons produced this frame, for the HUD and the renderer.
    pub weapons_frame: crate::scene::wiring::weapons::WeaponsFrame,
    /// `ai/index.js` — nav, squads, soldiers.
    pub ai: crate::scene::wiring::ai::AiWiring,
    /// The AI frustum needs the viewport aspect and nothing else holds it.
    pub aspect: f64,
}

impl Game {
    /// Build the level, bind every seam, and put the player on the ground.
    ///
    /// `seed` is the engine root seed — [`CAPTURE_SEED`] is the source's own
    /// deterministic value (`engine.js:26`).
    pub fn new(seed: u32) -> Self {
        Game::new_observed(seed, &mut |_, _| {})
    }

    /// [`Game::new`] with a per-slot observer: `checkpoint(slot, state)` is
    /// called with the ROOT stream's four state words immediately after each
    /// subsystem has finished drawing from it, in construction order.
    ///
    /// Not in the source. It exists because the construction order **is the
    /// level**: `core/registry.js` topologically sorts the subsystems, every one
    /// of them forks the root stream once at init, and `registry.rs` records
    /// that the sort's tie-breaking — and so the order of two independent
    /// systems — falls back to insertion order. A reordering therefore changes
    /// the world while compiling, running and failing no existing test.
    ///
    /// That is the failure mode a move onto [`crate::registry::Registry`] walks
    /// into, so the order is pinned here BEFORE the move: whatever composition
    /// root drives these systems, it must reproduce this sequence exactly. The
    /// same reason `crate::world::system::WorldSystem::init_observed` exists,
    /// one level up.
    pub fn new_observed(seed: u32, checkpoint: &mut dyn FnMut(&str, [u32; 4])) -> Self {
        let config = Config::default();
        let mut root = Rng::new(seed);
        checkpoint("start", root.state());

        // **THE PREPARE PASS - `core/engine.js:143-145`, draw for draw.**
        //
        // ```js
        // boot.time('engine.prepare', () => {
        //   for (const sys of order) sys.prepare?.(this.ctx);
        // });
        // ```
        //
        // Every subsystem's `prepare(ctx)` is the same two lines - fork the root
        // stream once, keep the fork - and `core/registry.js`'s topological sort
        // fixes the order they run in:
        //
        // | slot | name      | source                     | draws |
        // |------|-----------|----------------------------|-------|
        // |  1   | render    | `render/index.js:144`      | 1 |
        // |  2   | materials | *(no `prepare`)*           | 0 |
        // |  3   | sky       | *(no `prepare`)*           | 0 |
        // |  4   | physics   | `physics/index.js:254`     | 1 |
        // |  5   | world     | `world/index.js:126`       | 1 |
        // |  6   | player    | `player/index.js:155`      | 1 |
        // |  7   | weapons   | `weapons/index.js:147,158` | 2 |
        // |  8   | fx        | `fx/index.js:53`           | 1 |
        // |  9   | ai        | `ai/index.js:68`           | 1 |
        // | 10   | ui        | `ui/index.js:78`           | 1 |
        // | 11   | audio     | `audio/index.js:139`       | 1 |
        //
        // Ten draws, in that sequence. `apps/shmup/tools/rngprobe.mjs --trace`
        // prints exactly this list off the running original, and
        // `apps/shmup/tools/rng-golden.json` pins where the root stream lands
        // afterwards.
        //
        // **The POSITION of a draw is the whole seed contract** - see
        // `apps/shmup/ARCHITECTURE.md`, "prepare(ctx) - claim your seed", and
        // the long comment at `core/engine.js:120-142` explaining why hoisting
        // the forks out of `init()` was only legal because the *sequence* came
        // with them. A subsystem this port has not wired still has to take its
        // draw, or every seed after it shifts and the port grows a different
        // town. Until this pass existed the port's world was fork #1 where the
        // source's is #3, so no screenshot of the two could be compared.
        //
        // The source splits the fork from the construction (a prepare pass, then
        // an init pass) and this follows it: where a fork and the object it
        // belongs to cannot be built at the same moment - `physics` needs the
        // BVH the `world` slot has not produced yet - the fork is taken here at
        // its slot and the object is built below.

        // 1 - render. `render/index.js:142-145`.
        let render_rng = root.fork();
        checkpoint("render", root.state());

        // 4 - physics. `physics/index.js:252-255`. Slots 2 (`materials`) and 3
        // (`sky`) have no `prepare` and draw nothing, so nothing sits between.
        let physics_rng = root.fork();
        checkpoint("physics", root.state());

        // 5 - world. `world/index.js:124-127`; the fork itself is taken inside
        // `WorldSystem::init`, which `build_level` delegates to.
        let level = build_level(&mut root);
        checkpoint("world", root.state());

        // 6 - player. `player/index.js:153-156`. Before `weapons`, and that is
        // load-bearing: it is what puts the two weapon forks at #5 and #6 of the
        // root sequence.
        let player_rng = root.fork();
        checkpoint("player", root.state());

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

        let mut rig = CameraRig::new(config.fov);
        rig.reset(STAND.eye);

        // Slots 3 (`sky`) and 2 (`materials`) draw no RNG, so their position
        // here is free; they sit before the forking slots to match the source.
        let sky = crate::scene::wiring::look::SkyDriver::new(
            config.quality,
            crate::scene::wiring::look::HOUR,
        );
        let materials = crate::scene::wiring::look::MaterialLook::new(config.quality, 0.0);

        // weapons (slot 7) forks before fx.
        let mut weapons = crate::scene::wiring::weapons::WeaponsRig::new(&mut root);
        checkpoint("weapons", root.state());
        // fx (slot 8), then ai (slot 9), then the HUD (slot 10), then audio
        // (slot 11, last).
        let fx = crate::scene::wiring::fx_audio::build_fx(&mut root, &config, &physics);
        checkpoint("fx", root.state());
        let ai = crate::scene::wiring::ai::AiWiring::new(root.fork(), &config, &level, &physics, feet);
        checkpoint("ai", root.state());
        let hud = HudRig::new(root.fork());
        checkpoint("ui", root.state());

        let audio = crate::scene::wiring::fx_audio::build_audio(&mut root, &physics);
        checkpoint("audio", root.state());
        let fx_audio = crate::scene::wiring::fx_audio::FxAudio::new(fx, audio);

        let mut time = Time::default();
        time.fixed = FIXED_DT;
        time.scale = 1.0;

        let mut game = Game {
            config,
            level,
            physics,
            render_rng,
            physics_rng,
            player_rng,
            movement,
            rig,
            hud,
            sky,
            materials,
            weapons,
            weapons_frame: crate::scene::wiring::weapons::WeaponsFrame::default(),
            fx_audio,
            pulse: crate::scene::wiring::fx_audio::MovementPulse::default(),
            ai,
            // Overwritten at bind from the real surface (`scene::boot`). This
            // default is only what a native test sees; a browser frame that kept
            // it would shear the moment the window was not 16:9.
            aspect: 1280.0 / 720.0,
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
        // `WeaponCore::init` needs the bus and the clock, which only exist once
        // the struct is built.
        game.weapons.init(game.events.clone(), game.time);
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

        // No pad from here on purpose: `Game` sits above the browser and cannot
        // poll one. The frame loop installs it with `Input::set_pad` and
        // `begin_frame` reads it, which is where the source polls it too.
        input.begin_frame(&self.config, None);
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

        // The subsystems that read the finished frame: fx + audio off the
        // movement pulse, then the AI off the camera the rig just settled.
        // The sky and the material cache, before anything reads their output.
        let eye = self.rig.eye_position;
        self.sky.frame(self.time.dt, self.time.elapsed, (eye[0], eye[2]));
        self.materials.frame(self.time.dt);

        let pose = self.pose();
        // The weapons before the fx that consume their events.
        let mut link = crate::scene::wiring::weapons::PlayerLink::new(
            &mut self.rig,
            &self.movement,
            self.ads_requested,
        );
        self.weapons_frame = self.weapons.frame(
            self.time.dt,
            self.time,
            &*input,
            pose,
            Some(&mut link),
            None,
        );
        let state = crate::scene::wiring::fx_audio::FrameState::of(self);
        // The weapons' two rich payloads, into the subsystems built to consume
        // them. `WeaponsFrame` has carried `fire` and `shell` since the weapons
        // seam landed and NOTHING read either, so fx's only input was footsteps:
        // no muzzle flash, no tracers, no brass. The comment one call above —
        // "the weapons before the fx that consume their events" — described an
        // intent, not a wire.
        //
        // `bullet:impact` and `explosion` are deliberately NOT bridged here:
        // impacts are raised through `RaycastWorld::fire_bullet`, which has no
        // `WeaponPhysics` impl, so no round hits anything yet. Faking one here
        // would put sparks and decals on collisions that never happened.
        let fired = self.weapons_frame.fire;
        let ejected = self.weapons_frame.shell;
        let now = self.time.elapsed;
        if let Some(f) = fired {
            self.fx_audio.weapon_fire(
                &state,
                &crate::fx::system::WeaponFire {
                    origin: Some((f.origin.x, f.origin.y, f.origin.z)),
                    dir: Some((f.dir.x, f.dir.y, f.dir.z)),
                    weapon: f.weapon.map(str::to_owned),
                    ..Default::default()
                },
                &crate::audio::system::WeaponFire {
                    weapon: f.weapon.map(str::to_owned),
                    origin: Some([f.origin.x, f.origin.y, f.origin.z]),
                    ..Default::default()
                },
            );
        }
        if let Some(sh) = ejected {
            self.fx_audio.weapon_shell(
                now,
                &crate::fx::system::WeaponShell {
                    position: Some((sh.position.x, sh.position.y, sh.position.z)),
                    velocity: Some((sh.velocity.x, sh.velocity.y, sh.velocity.z)),
                },
            );
        }
        let pulse = self.pulse;
        self.fx_audio.frame(&state, &pulse, true);
        self.ai.frame(
            self.time.dt,
            self.time.frame,
            self.time.elapsed,
            pose,
            self.aspect,
            &self.sky,
            self.movement.position,
            None,
            None,
        );
        pose
    }

    /// `fixedUpdate(h, ctx)`. `player/index.js:266-273`.
    fn fixed_update(&mut self, input: &Input) {
        // The source ticks weapons on the fixed step even while paused
        // (reload timers and bullet flight do not stop for a menu), so this
        // sits ahead of the `control_enabled` return below.
        self.weapons.fixed_step(FIXED_DT, None);
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
        let sens = lerp(1.0, self.config.ads_sens_scale, clamp01(self.ads_amount));

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
        // The source EMITS `player:land` / `player:footstep` here
        // (`player/index.js:332,351`); this port only cleared the flags, so
        // footstep and landing audio, and the dust puff, never fired. The pulse
        // carries them to `fx_audio` for this frame.
        self.pulse = crate::scene::wiring::fx_audio::MovementPulse::default();
        if self.movement.land_event.pending {
            self.pulse.land = Some(self.movement.land_event);
            self.movement.land_event.pending = false;
            // The source's `if (mag > 0.35) m._footHold = FOOTSTEP.landHold`
            // is redundant here: `post_move` already sets `foot_hold` to
            // exactly that on the landing frame (`movement.js:865`), so the
            // rig's dip magnitude has nothing left to add.
            self.rig.on_land(self.movement.land_event.speed);
            self.weapons.on_land(self.movement.land_event.speed);
        }
        if self.movement.step_event.pending {
            self.pulse.step = Some(self.movement.step_event);
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
        if self.movement.jumped {
            self.weapons.on_jump();
        }
        self.movement.jumped = false;
    }

    /// `Escape` toggles the pause menu, which is what releases pointer lock in
    /// the browser (the DOM does that itself) and what stops the movement
    /// machine from being driven. `ui/menu.js`'s `open`/`close`.
    fn handle_pause(&mut self, input: &Input) {
        if input.action_pressed(Action::Pause) {
            self.hud.toggle_menu(&self.events);
        }
        // `PauseMenu` is the single owner of open/closed; these two mirror it,
        // so the pointer-lock-loss path inside `UiCore::late_update` reaches
        // them too. `UiInput::pause_pressed` is left false in the wiring for
        // exactly this reason — the edge arrives here and only here.
        self.paused = self.hud.menu_open();
        self.control_enabled = !self.paused;
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

    /// Drive the HUD with this frame's real state — the seams [`HudPull`]
    /// names, bound to the movement machine, the weapon rig and the AI. Call it
    /// after the camera has reached its final transform.
    pub fn hud_frame(&mut self, input: &Input) -> UiFrame {
        let pose = self.pose();
        let position = self.movement.render_position;
        let weapon = self.weapons.hud_pull();
        let actors = self.ai.actor_poses();
        let pull = HudPull {
            dt: self.time.dt,
            clock: UiClock {
                raw: self.time.raw,
                elapsed: self.time.elapsed,
            },
            pose,
            aspect: self.aspect,
            input,
            weapon: Some(weapon),
            player: PlayerPull {
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
            },
            player_position: position,
            actors: &actors,
        };
        self.hud.frame(pull, &self.events)
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
        assert!(game.rig.fov < game.config.fov, "the FOV narrowed");
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

    /// **The root stream is pinned against the ORIGINAL, not against itself.**
    ///
    /// This test used to assert the port's own numbers, which made it a
    /// regression guard and nothing more: the port forked `world` first where
    /// the source forks it third, and a test written from the port's output
    /// agreed with the defect. The oracle is
    /// `apps/shmup/tools/rng-golden.json` and the live trace that produced it
    /// (`node tools/rngprobe.mjs --trace`, run from `apps/shmup`).
    ///
    /// The values below are the root stream's four state words after each of
    /// the source's ten prepare-pass draws, computed from `Rng::new(0x5eed1234)`
    /// - the seed `engine.js:35` uses when `config.deterministic`, which is
    /// `?capture=1` only (`main.js:38`, `:71`). They are anchored at both ends:
    /// `start` is the seeded state before any draw, and the final `audio` row is
    /// **literally `rng-golden.json`'s `root` key**,
    /// `302843209,3700148001,543641753,221195564` - the state the original's
    /// root is left in once its boot has taken all ten forks. A port that
    /// reaches that value has drawn the same number of times, in the same
    /// places, as the original.
    #[test]
    fn the_root_stream_is_consumed_in_the_sources_order() {
        let mut seen: Vec<(String, [u32; 4])> = Vec::new();
        Game::new_observed(CAPTURE_SEED, &mut |slot, state| {
            seen.push((slot.to_owned(), state));
        });
        let order: Vec<&str> = seen.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "start", "render", "physics", "world", "player", "weapons", "fx", "ai", "ui",
                "audio"
            ],
            "the root stream was consumed in a different order - the level moved"
        );
        // The states themselves, not only the order: a subsystem that draws a
        // different NUMBER of values leaves the sequence of names intact and the
        // world different, and the names alone would not see it. `weapons` is
        // the one slot that advances by two (`weapons/index.js:147` and `:158`).
        let states: Vec<[u32; 4]> = seen.iter().map(|(_, s)| *s).collect();
        assert_eq!(
            states,
            vec![
                // draw 0 - `new Rng(0x5eed1234)`, before the prepare pass.
                [1408721277, 2042729634, 265063393, 2021161881],
                // 1 - render   (`render/index.js:144`)
                [1380879942, 637173310, 3742543516, 3480868877],
                // 2 - physics  (`physics/index.js:254`)
                [3100452981, 2829475556, 2040552666, 489791316],
                // 3 - world    (`world/index.js:126`)
                [224061893, 1774922315, 2379267247, 3188557228],
                // 4 - player   (`player/index.js:155`)
                [3667674658, 3913383713, 383697770, 897531582],
                // 5,6 - weapons + viewmodel (`weapons/index.js:147`, `:158`)
                [3517776693, 1830632092, 1340740341, 2015643320],
                // 7 - fx       (`fx/index.js:53`)
                [3298066193, 4082772828, 2754750912, 3420528809],
                // 8 - ai       (`ai/index.js:68`)
                [4230968548, 2482797965, 3572559569, 3630148037],
                // 9 - ui       (`ui/index.js:78`)
                [3079446700, 3139692472, 3492053045, 513950301],
                // 10 - audio   (`audio/index.js:139`) == rng-golden.json `root`
                [302843209, 3700148001, 543641753, 221195564],
            ],
            "a subsystem drew a different number of values from the root stream"
        );
    }

    /// The last row above, stated on its own against the golden file's `root`
    /// key, because it is the single assertion that is an ORACLE rather than a
    /// derivation: `apps/shmup/tools/rng-golden.json` is a committed capture of
    /// the original's boot, and this is the number in it.
    #[test]
    fn the_root_stream_ends_where_the_sources_golden_says_it_does() {
        let mut last = [0u32; 4];
        Game::new_observed(CAPTURE_SEED, &mut |_, state| last = state);
        assert_eq!(
            last,
            [302_843_209, 3_700_148_001, 543_641_753, 221_195_564],
            "apps/shmup/tools/rng-golden.json, key `root`"
        );
    }

    /// The world's own stream, which is the point of the whole ordering: the
    /// level is generated from the root's THIRD fork. The four words are
    /// `new Rng(root.u32())` taken after two prior draws - the state
    /// `WorldSystem` starts from in the original.
    #[test]
    fn the_world_generator_starts_from_the_sources_third_fork() {
        let mut root = Rng::new(CAPTURE_SEED);
        root.u32(); // render
        root.u32(); // physics
        let world = root.fork();
        assert_eq!(
            world.state(),
            [2_835_107_428, 3_288_565_564, 3_792_338_184, 2_967_788_734],
            "the world fork moved"
        );
        // And `Game` really reaches that stream position before building the
        // level: its `world` checkpoint is the root state after three draws.
        let mut at_world = None;
        Game::new_observed(CAPTURE_SEED, &mut |slot, state| {
            if slot == "world" {
                at_world = Some(state);
            }
        });
        assert_eq!(
            at_world,
            Some([224_061_893, 1_774_922_315, 2_379_267_247, 3_188_557_228])
        );
    }

    /// **The witness: does this port generate the ORIGINAL town?**
    ///
    /// `apps/shmup/tools/rng-golden.json`, key `witness`, is a committed
    /// snapshot of what the source produced in capture mode:
    /// `staticTris 585630`, `instances 308`, `drawCalls 62`. It is the only
    /// end-to-end oracle for the seeding: the fork order can be right and the
    /// geometry still wrong, and only these counts tell the two apart.
    ///
    /// Two of the three are asserted here because two of the three are exact.
    /// Measured in one binary, building the same level from two stream
    /// positions:
    ///
    /// | root fork | staticTris | instances | drawCalls |
    /// |-----------|-----------|-----------|-----------|
    /// | #1 (the old, wrong slot) | 584465 | 295 | 56 |
    /// | #3 (the source's slot)   | 585336 | **308** | **62** |
    /// | `rng-golden.json`        | 585630 | **308** | **62** |
    ///
    /// The placement counts landed on the golden the moment the world generator
    /// drew from the right fork. `staticTris` did not, and that residue was a
    /// *geometry* defect rather than a seeding one; it is closed - see
    /// `the_static_triangle_count_matches_the_golden` below for the two causes.
    #[test]
    fn the_generated_level_matches_the_sources_witness() {
        let game = Game::new(CAPTURE_SEED);
        assert_eq!(
            (game.level.instances, game.level.draw_calls),
            (308, 62),
            "witness drift: instances/drawCalls against rng-golden.json"
        );
    }

    /// **The third witness number, and the two defects it caught.**
    ///
    /// With the fork order corrected the port's merged static geometry came to
    /// `585336` triangles against the golden's `585630`: a shortfall of 294,
    /// 0.05%, with every instanced placement already exact. Two causes, found
    /// by diffing this port's per-`Assembler::add` emit trace against the same
    /// trace taken from the ORIGINAL JavaScript run headless under Node:
    ///
    /// * **230** in `buildGround`'s material-seam scatter. The source's
    ///   `for (let k = 0; k < sr.int(1, 3); k++)` (`ground.js:205`) re-draws
    ///   its bound before every iteration *including the failing one*; this
    ///   port had hoisted it to a single draw, which desynchronised the seam
    ///   pass's private `sr` stream and emitted the wrong mix of `sand` /
    ///   `road_dust` / `concrete` / `dirt` / `gravel` patches.
    ///   `world::ground::seam` now uses `int_loop_continues`, as the other 24
    ///   sites of that idiom already did.
    /// * **64** in four buildings' jagged parapets. `Math.round(w / 1.2)`
    ///   (`util.js:480`) is an integer derived from a length, and at
    ///   `w = 11.4` f64 rounds to 10 where f32 rounds to 9. `wall_panel` now
    ///   takes the width at the source's precision; see
    ///   `world::buildings::floor_footprint_exact`.
    ///
    /// The number is asserted rather than described so neither can come back.
    #[test]
    fn the_static_triangle_count_matches_the_golden() {
        let game = Game::new(CAPTURE_SEED);
        assert_eq!(
            game.level.static_tris, 585_630,
            "apps/shmup/tools/rng-golden.json, witness.staticTris"
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
        let frame = game.hud_frame(&input);
        assert!(game.movement.sprinting, "sprinting with W + Shift held");
        assert!(game.hud.core().borrow().state.sprint, "and the HUD knows it");
        assert!(game.hud.core().borrow().state.move_amount > 0.0);
        assert!(frame.hud_visible > 0.0);
        // The compass heading follows the camera basis, which `UiCore` now
        // derives from `camera.matrixWorld` itself rather than from yaw — so
        // this reads the frame the HUD produced, not a second computation of it.
        assert!((frame.basis.forward_x.hypot(frame.basis.forward_z) - 1.0).abs() < 1e-12);
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


    /// **What one frame produces, per subsystem — the invariant a composition
    /// root move has to preserve.**
    ///
    /// `the_root_stream_is_consumed_in_the_registrys_order` pins *init*. This
    /// pins *running*, and the two guard different failures. A subsystem can
    /// keep its slot in the init order and still do less work per frame, which
    /// is not hypothetical here: `ai::system::AiSystem::update` — the ported
    /// `Subsystem` impl, as distinct from the wiring this drives — runs the
    /// source's `if (!phys)` path, because `Ctx` carries no physics facade and
    /// so it steps the AI with no gravity and no ballistics. Registering it and
    /// driving the frame from `registry::Registry` would compile, run, and
    /// quietly give a different game.
    ///
    /// So each subsystem contributes one number that its own work moves. The
    /// values are recorded from the wiring path as it stands, and a registry
    /// -driven frame has to reproduce them.
    ///
    /// Deliberately NOT a screenshot or a pose hash: those move for a hundred
    /// reasons and say nothing about which subsystem stopped working. One
    /// observable per system is what makes a failure name its own cause.
    #[test]
    fn one_frame_of_work_per_subsystem_is_pinned() {
        let mut game = game();
        let mut input = Input::new();
        input.pointer_locked = true;
        input.key_down("KeyW");
        input.mouse_down(0);
        for i in 0..90 {
            input.mouse_move(f64::from(i % 5) - 2.0, 0.5);
            game.frame(1.0 / 60.0, &mut input);
            // The HUD is NOT driven by `Game::frame` — `scene::app::frame` calls
            // it separately, and so must this. That split is itself part of
            // what the registry move fixes: `ui` is a subsystem with an
            // `Update` phase, and a registry drives every phase from one list
            // rather than leaving one system to a caller who has to remember.
            game.hud_frame(&input);
        }

        // player / physics — the capsule actually moved and is standing on the
        // world, not falling through it or stuck at the spawn.
        let moved = (game.movement.position[0] - game.spawn.position[0]).hypot(
            game.movement.position[2] - game.spawn.position[2],
        );
        assert!(moved > 1.0, "the player did not move: {moved}");
        assert!(
            game.movement.position[1] > -1.0 && game.movement.position[1] < 5.0,
            "the capsule left the world: y = {}",
            game.movement.position[1]
        );
        assert!(game.movement.grounded, "the player is not standing on anything");

        // weapons — the trigger was held, so rounds left the magazine.
        let mag = game.weapons.core().ammo().mag;
        assert!(mag < 30, "the magazine never drained: {mag}");

        // ai — the actors exist and are being posed.
        let actors = game.ai.actor_poses();
        assert!(!actors.is_empty(), "no AI actors were posed");
        assert!(
            actors.iter().all(|a| a.position[1].is_finite()),
            "an actor pose went non-finite"
        );

        // fx — the frame produced live particles (muzzle flash, at minimum).
        let mut points = Vec::new();
        game.fx_audio.particle_points(game.time.elapsed, &mut points);
        assert!(!points.is_empty(), "the fx system produced no particles");

        // ui — the HUD read the weapon it is drawing.
        assert!(
            game.hud.core().borrow().state.move_amount > 0.0,
            "the HUD did not see the player move"
        );
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
