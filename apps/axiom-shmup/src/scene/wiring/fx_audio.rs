//! **The FX + audio seam** — what constructs [`FxSystem`] and [`AudioSystem`],
//! and what steps them from the running game's real state.
//!
//! Both subsystems were ported in full (`fx/index.js:1-1316`,
//! `audio/index.js:1-868`) and, until this file, nothing anywhere constructed
//! either of them. This module is the composition step, in the tier that owns
//! composition ([`crate::scene`]), following the precedent
//! [`crate::scene::app::drive_viewmodel`] set: construct once, step from real
//! state, invent nothing.
//!
//! ## Where these go in `Game::new`'s ordering — and why it matters
//!
//! The source registers eleven subsystems and the registry **topologically
//! sorts them on `static deps`** (`core/registry.js:46-63`;
//! `main.js:36` says outright that registration order is irrelevant). Resolving
//! the real `deps` gives exactly one init order:
//!
//! ```text
//! render → materials → sky → physics → world → player → weapons → fx → ai → ui → audio
//! ```
//!
//! Each subsystem takes `ctx.rng.fork()` as the *first* statement of its
//! `init`, so the root stream is consumed in that order. Nine of the eleven
//! fork (`materials` and `sky` do not):
//!
//! | # | subsystem | source line | in this port |
//! |---|-----------|-------------|--------------|
//! | 1 | render    | `render/index.js:134`   | — (unported) |
//! | 2 | physics   | `physics/index.js:244`  | — (`PhysicsWorld` is query-only and draws nothing) |
//! | 3 | world     | `world/index.js:91`     | `build_level(&mut root)` |
//! | 4 | player    | `player/index.js:147`   | — (the source's own comment says it is never read) |
//! | 5 | weapons   | `weapons/index.js:134`  | — (unconstructed) |
//! | 6 | **fx**    | `fx/index.js:42`        | **[`build_fx`]** |
//! | 7 | ai        | `ai/index.js:55`        | — (unconstructed) |
//! | 8 | ui        | `ui/index.js:69`        | `Hud::new(root.fork())` |
//! | 9 | **audio** | `audio/index.js:130`    | **[`build_audio`]** |
//!
//! So, concretely, inside `Game::new`:
//!
//! * [`build_fx`] must be called **after** `build_level(&mut root)` and
//!   **strictly before** `Hud::new(root.fork())`.
//! * [`build_audio`] must be called **after** the `Hud`'s fork. Audio is last
//!   in the whole graph (`static deps = []`, and it sorts last only because it
//!   is registered last), so nothing may take a root fork after it.
//!
//! **What this ordering does and does not buy.** It preserves the *relative*
//! order the source has, which is the property that stops adding FX from
//! reshuffling the HUD's stream. It does **not** make the port's root stream
//! identical to the source's, and it cannot today: rows 1, 2, 4, 5 and 7 take
//! no fork here because those subsystems are unconstructed, and
//! [`crate::scene::level::build_level`] takes **two** forks where the source's
//! `world` takes one (a documented borrow-checker split). Every fork below the
//! first divergence therefore lands on a different seed than the JavaScript's.
//! That is a pre-existing, documented property of this port, not something this
//! file introduces — but it means an `fx`/`audio` RNG golden captured from the
//! original can only be compared by seeding [`FxSystem::new`] /
//! [`AudioSystem::new`] directly, never by running `Game::new`.
//!
//! ## The physics seam was already closed
//!
//! [`crate::physics::probe::PhysicsWorld`] implements
//! [`crate::fx::world::FxWorld`] (`physics/probe.rs:278`) *and*
//! [`crate::audio::spatial::WorldProbe`], and it is `Clone` over an
//! `Rc<StaticWorld>` — so binding both is a handle copy, not a rebuild.
//! [`build_fx`] and [`build_audio`] do exactly that; nothing was missing.
//!
//! ## What still cannot be drawn or heard, and why
//!
//! * **Rendering.** FX output is particles, decals, tracers, shells and pooled
//!   lights. Of those, only *lights* and *shells* are expressible with what the
//!   engine gives an app today: `RunningApp::add_point_light`/`despawn` (16
//!   lights total, directional included, silently truncated past that) and
//!   ordinary instanced meshes moved by `RunningApp::set`. Particles, decals,
//!   tracers and haze need additive blending and camera-facing quads: the GPU
//!   backend *has* an additive `BlendState` but every call site passes
//!   `false`, there is no billboard path in 3D at all, and mesh geometry is
//!   upload-once through the facade. [`FxAudio::particle_points`] is the CPU
//!   readback that a renderer would consume the day one exists; it runs the
//!   ported vertex-shader integration ([`crate::fx::particles::integrate`]) and
//!   yields world-space points. Building the pass itself is not this file's
//!   job and is not attempted here.
//! * **Audio output.** Everything below [`AudioCore`] is arithmetic over a
//!   recorded [`crate::audio::graph::AudioGraph`]; no sample reaches a speaker
//!   until [`crate::audio::web_audio::WebAudioBridge`] (`wasm32`-only) walks
//!   that graph into real `web_sys` nodes. See [`FxAudio::start_audio`] for the
//!   exact browser call sequence and the user-gesture requirement.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ai::animator::quat_from_axis_angle;
use crate::audio::foley::Gait;
use crate::audio::system::{
    AudioCore, AudioSystem, BulletImpact as AudioImpact, ExplosionEvent,
    PlayerFootstep as AudioFootstep, PlayerLand, PlayerState, WeaponFire as AudioFire,
    WeaponShell as AudioShell,
};
use crate::config::{Config, UNITS};
use crate::engine::Ctx;
use crate::registry::{Phase, Subsystem};
use crate::fx::explosions::ExplosionOpts;
use crate::fx::particles::{self, ParticleLayer};
use crate::fx::system::{
    BulletImpact as FxImpact, CameraFrame, FxFrame, FxStats, FxSystem,
    PlayerFootstep as FxFootstep, WeaponFire as FxFire, WeaponShell as FxShell,
};
use crate::physics::probe::PhysicsWorld;
use crate::player::camera::Euler;
use crate::player::movement::{LandEvent, StepEvent};
use crate::player::tuning::Stance;
use crate::rng::Rng;
use crate::scene::game::{CameraPose, Game};

use crate::weapons::rig_math::{M4, Q, V3};

/// `_syncLighting`'s `sunI` fallback (`fx/index.js`, `let sunI = 4.3`) — the
/// intensity a `THREE.DirectionalLight` carries in the source at full sun, and
/// the divisor `this._sunFactor = clamp(sunI / 4.3, 0, 1.6)` normalises by.
///
/// [`SkyLook::sun_intensity`] is a 0..1 *relative* term (the engine's
/// `DirectionalLight::intensity` is a `Ratio`), so it is scaled by this to land
/// on the source's units. Midday then reads `sun_factor == 1.0`, which is what
/// the ambience's density term expects.
pub const SOURCE_SUN_INTENSITY: f64 = 4.3;

/// The one-shot movement flags a frame produced, handed in explicitly.
///
/// [`crate::scene::game::Game`] latches `movement.land_event` /
/// `movement.step_event` and clears `pending` inside `drain_movement_events`
/// before `frame` returns, so by the time this seam runs the edge is gone. The
/// source does not have that problem because `_drainMovementEvents`
/// (`player/index.js:322-360`) *emits* `player:land` and `player:footstep` on
/// the bus as it clears them; this port's `drain_movement_events` drops both
/// emissions, which its own doc calls out.
///
/// Rather than re-derive the edges from `grounded`/`foot_hold` — which would be
/// inventing behaviour — the caller passes what it saw. `Default` (both `None`)
/// is a frame in which neither fired.
#[derive(Debug, Clone, Copy, Default)]
pub struct MovementPulse {
    /// `m.landEvent` on the frame `pending` was true.
    pub land: Option<LandEvent>,
    /// `m.stepEvent` on the frame `pending` was true.
    pub step: Option<StepEvent>,
}

/// Everything one stepped frame needs off the running game, as an owned value.
///
/// Taking a value rather than `&Game` is deliberate and load-bearing: it lets
/// [`FxAudio`] be a **field of `Game`** as well as of
/// [`crate::scene::app::Scene`]. `let state = FrameState::of(self);` ends the
/// `&self` borrow before `self.fx_audio.frame(&state, …)` takes `&mut` — a
/// method taking `&Game` could never be called that way.
#[derive(Debug, Clone, Copy)]
pub struct FrameState {
    /// `ctx.time.dt` — the frame's scaled, clamped delta.
    pub dt: f64,
    /// `ctx.time.elapsed` — what `fx/index.js:781` assigns to `this.now`.
    pub now: f64,
    pub pose: CameraPose,
    /// `r?.sunDir`, in world space.
    pub sun_dir: V3,
    /// `r?.activeSun` — `(colour, intensity)`, in the source's units.
    pub active_sun: (V3, f64),
    pub sprinting: bool,
    pub stance: Stance,
    /// `this.adsRequested`.
    pub ads: bool,
}

impl FrameState {
    /// Read this frame's terms off the running game.
    pub fn of(game: &Game) -> Self {
        FrameState {
            dt: game.time.dt,
            now: game.time.elapsed,
            pose: game.pose(),
            sun_dir: sun_direction(&game.sky),
            active_sun: active_sun(&game.sky),
            sprinting: game.movement.sprinting,
            stance: game.movement.stance,
            ads: game.ads_requested,
        }
    }
}

/// What one stepped frame produced, for a caller that needs to react to it.
///
/// Everything else FX holds is reachable directly and is deliberately not
/// duplicated here: `fx.lights.slots`, `fx.shells.slots`, `fx.decals.raw_*()`
/// and the five [`ParticleLayer`]s are all public.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FxAudioReport {
    /// [`FxSystem::late_update`]'s return — the frame the source would have
    /// called `prewarmMaterials()` on (`fx/index.js:820`). Fires exactly once,
    /// on the second stepped frame.
    pub prewarm_due: bool,
    /// `fx.stats` after the step.
    pub stats: FxStats,
    /// Pooled lights with a non-zero intensity this frame — the muzzle-flash
    /// and explosion lights a renderer would realise. `fx.lights.slots` carries
    /// their positions and colours.
    pub live_lights: usize,
    /// Brass in flight; `fx.shells.slots` carries the transforms.
    pub live_shells: usize,
    /// Whether the audio graph exists and is being driven.
    pub audio_running: bool,
}

/// One integrated particle, in world space — [`FxAudio::particle_points`]'s
/// output and the render seam's input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticlePoint {
    pub position: (f64, f64, f64),
    /// Colour already multiplied by intensity, as the vertex shader leaves it.
    pub color: (f64, f64, f64),
    pub alpha: f64,
    pub size: f64,
    /// Atlas tile index the spawn chose.
    pub tile: f64,
    /// `true` for the additive layers (`add`, `motes`), `false` for `lit`.
    pub additive: bool,
}

/// `fx`'s `init` — position 6 in the source's init order. Call **after**
/// `build_level(&mut root)` and **before** `Hud::new(root.fork())`.
///
/// `root.u32()` rather than `root.fork()` because [`FxSystem::new`] takes a
/// seed and immediately does `Rng::new(seed)` — and `Rng::fork` *is*
/// `Rng::new(self.u32())` (`rng.rs:178`). The two are the same draw and the
/// same resulting stream; this spelling is the one the signature accepts.
///
/// The budgets are `ctx.config.q.particleBudget`/`decalBudget` and the gravity
/// is `UNITS.gravity`, exactly as `fx/index.js:46-50` reads them.
pub fn build_fx(root: &mut Rng, config: &Config, physics: &PhysicsWorld) -> FxSystem {
    let mut fx = FxSystem::new(
        root.u32(),
        config.q.particle_budget,
        config.q.decal_budget,
        UNITS.gravity,
    );
    // `this._physics = ctx.peek('physics')` (`fx/index.js:44`). A handle copy:
    // `PhysicsWorld` is `Clone` over an `Rc<StaticWorld>`.
    fx.world = Some(Box::new(physics.clone()));
    fx
}

/// `audio`'s `init` — **last** in the source's init order. Call after the
/// `Hud`'s `root.fork()`; nothing may take a root fork after this.
///
/// `AudioSystem::new` takes the forked stream directly (`audio/index.js:130`).
pub fn build_audio(root: &mut Rng, physics: &PhysicsWorld) -> AudioSystem {
    let audio = AudioSystem::new(root.fork());
    // `ctx.peek('physics')?.raycast` — the occlusion / space-classifier probe.
    audio
        .core()
        .borrow_mut()
        .set_world_probe(Some(Rc::new(physics.clone())));
    audio
}

/// The two subsystems, stepped together.
///
/// They are paired because every game event feeds *both* — a bullet impact is a
/// spark burst and a ricochet, a shot is a muzzle flash and a report — and the
/// three ported subsystems each declared their own payload type for the same
/// event names precisely because no one had written this file yet (see the
/// "NOTE FOR THE INTEGRATION PASS" comment in [`crate::fx::system`]). The
/// forwarders below are that convergence: one call per game event, fanned out
/// to each subsystem's own payload.
pub struct FxAudio {
    pub fx: FxSystem,
    /// Held so the registered facade genuinely exists; its handlers are reached
    /// through [`FxAudio::core`].
    pub audio: AudioSystem,
    core: Rc<RefCell<AudioCore>>,
    /// `ctx.scene.fog` — `(colour, density)`. The source resolves
    /// `fog.density ?? 1 / max(1, fog.far ?? 400)` at the emitter, so a caller
    /// that has the engine's `FrameDepthFog` converts it once and sets this.
    /// `None` clears only the density term, which is what `_syncLighting` does.
    pub fog: Option<(V3, f64)>,
}

impl FxAudio {
    /// Pair two already-constructed subsystems. Separate from [`build_fx`] /
    /// [`build_audio`] on purpose: the two constructors must sit on *opposite*
    /// sides of the HUD's root fork, so they cannot be taken in one call.
    pub fn new(fx: FxSystem, audio: AudioSystem) -> Self {
        let core = audio.core();
        FxAudio {
            fx,
            audio,
            core,
            fog: None,
        }
    }

    /// The shared audio guts — `window.__AUDIO__` in the source.
    pub fn core(&self) -> Rc<RefCell<AudioCore>> {
        Rc::clone(&self.core)
    }

    /// Build the audio graph. Returns `true` once it is live.
    ///
    /// **Native / headless:** call with any sample rate (48000.0 is the
    /// browser's usual) and drive the clock with [`FxAudio::frame`], which
    /// advances it by `dt`. Everything is recorded into an
    /// [`crate::audio::graph::AudioGraph`] and nothing is heard, which is the
    /// property the golden tests rely on.
    ///
    /// **Browser — what the orchestrator must actually do for sound:**
    ///
    /// 1. **A user gesture is required.** `AudioContext` construction is
    ///    allowed at any time but the context starts `suspended` under Chrome's
    ///    and Safari's autoplay policy, and `resume()` only succeeds from
    ///    inside a user-gesture handler. The source arms exactly that
    ///    (`audio/index.js`'s gesture latch); this port's
    ///    [`crate::audio::web_audio`] deliberately does not, and its
    ///    `WebAudioBridge::new` doc says "the caller arms one". So: on the
    ///    first `pointerdown`/`keydown` on the canvas — the same gesture that
    ///    already requests pointer lock — construct
    ///    `WebAudioBridge::new()`, call `bridge.context().resume()`, then call
    ///    this with `bridge.sample_rate()`.
    /// 2. **Push the device clock in, every frame**, before
    ///    [`FxAudio::frame`]: `fx_audio.set_context_time(bridge.current_time())`.
    ///    Without it the graph's clock is a simulated accumulator and every
    ///    scheduled event drifts off the audio device.
    /// 3. **Flush, every frame**, after [`FxAudio::frame`]:
    ///    `bridge.flush(core.borrow().graph().unwrap())`. The bridge is
    ///    append-only, so this realises exactly that frame's new voices.
    ///
    /// The graph is only *recorded* until step 3 runs; a page that never
    /// flushes is silent no matter how much synthesis happened.
    pub fn start_audio(&mut self, sample_rate: f64) -> bool {
        self.core.borrow_mut().start(sample_rate)
    }

    /// Push the audio device's absolute clock into the graph.
    ///
    /// See [`FxAudio::start_audio`] step 2. Native callers do not call this —
    /// [`FxAudio::frame`] advances the clock by `dt` instead.
    pub fn set_context_time(&mut self, t: f64) {
        self.core.borrow_mut().set_context_time(t);
    }

    /// Advance both subsystems one rendered frame, from the game's real state.
    ///
    /// The order is the source's: `update` for every subsystem in dependency
    /// order (fx before audio — audio has no deps and sorts last), then
    /// `lateUpdate`. Build `state` with [`FrameState::of`].
    ///
    /// `advance_clock` is `false` in the browser, where the audio device owns
    /// the clock and [`FxAudio::set_context_time`] pushes it in instead.
    pub fn frame(
        &mut self,
        state: &FrameState,
        pulse: &MovementPulse,
        advance_clock: bool,
    ) -> FxAudioReport {
        let dt = state.dt;
        let now = state.now;
        let camera = camera_frame(state.pose);

        // ---- the one-shots, before the frame updates ----------------------
        // `_drainMovementEvents` emits inside the player's `update`, ahead of
        // fx's and audio's, so a landing is heard and sprayed on the frame it
        // happened rather than the next one.
        self.drain_pulse(state, pulse);

        // ---- fx: update, then lateUpdate ----------------------------------
        let frame = FxFrame {
            camera,
            // The source's `ctx.viewCamera` is the weapon layer's own camera.
            // This port renders the viewmodel in world space
            // (`scene::app::drive_viewmodel` composes the rig into the world),
            // so the view camera *is* the camera. It is only read once
            // `view_attached` becomes true, which needs a `view: true` muzzle
            // flash — nothing emits one yet.
            view_camera: camera,
            // `ctx.peek('weapons')?.muzzleWorld(v)`. No weapons subsystem is
            // constructed, so `_stageMuzzle` takes its eye-relative fallback.
            muzzle_world: None,
            sun_dir: Some(state.sun_dir),
            active_sun: Some(state.active_sun),
            fog: self.fog,
            // `ctx.scene`, asked one question: is a followed prop still
            // attached. Nothing here detaches props, and `None` is the
            // "still attached" reading.
            scene: None,
        };
        self.fx.update(dt, now, &frame);
        let prewarm_due = self.fx.late_update(dt, now);

        // ---- audio --------------------------------------------------------
        let mut core = self.core.borrow_mut();
        // `cam.matrixWorld.elements` at `audio/index.js:222-227`: position is
        // column 3, forward is the negated column 2, up is column 1.
        let e = &camera.matrix_world.e;
        core.set_listener_basis(
            [e[12], e[13], e[14]],
            [-e[8], -e[9], -e[10]],
            [e[4], e[5], e[6]],
        );
        // `player:state` — the cloth rustle on a stance or ADS change. The
        // handler dedupes against its own last value, so pushing it every
        // frame is what the source's per-change emit resolves to.
        core.on_player_state(&PlayerState {
            stance: Some(stance_name(state.stance).to_string()),
            ads: Some(state.ads),
        });
        core.update(dt);
        // Native only: in the browser the device clock is authoritative and
        // arrives through `set_context_time`.
        if advance_clock {
            core.advance(dt);
        }
        let audio_running = core.running;
        drop(core);

        FxAudioReport {
            prewarm_due,
            stats: self.fx.stats,
            live_lights: self
                .fx
                .lights
                .slots
                .iter()
                .filter(|s| s.intensity > 0.0)
                .count(),
            live_shells: self.fx.shells.alive_count(),
            audio_running,
        }
    }

    /// Turn this frame's movement one-shots into the source's two emissions.
    fn drain_pulse(&mut self, state: &FrameState, pulse: &MovementPulse) {
        let (now, pose) = (state.now, state.pose);
        if let Some(land) = pulse.land {
            // `fx.onLand(e)` (`fx/index.js:702-712`) reads `ctx.camera.position`
            // and drops to the ground plane by `UNITS.playerHeight -
            // UNITS.eyeOffset`; the eye pose is exactly that camera.
            self.fx.on_land(
                now,
                land.speed,
                (pose.eye[0], pose.eye[1], pose.eye[2]),
                UNITS.player_height,
                UNITS.eye_offset,
            );
            self.core.borrow_mut().on_land(&PlayerLand {
                velocity: Some(land.speed),
                surface: Some(land.surface),
            });
        }

        if let Some(step) = pulse.step {
            self.fx.handle_footstep(
                now,
                &FxFootstep {
                    running: step.running,
                    position: Some((step.x, step.y, step.z)),
                },
            );
            self.core.borrow_mut().on_footstep(&AudioFootstep {
                position: Some([step.x, step.y, step.z]),
                surface: Some(step.surface),
                // `p?.running ? 'run' : p?.crouched ? 'crouch' : 'walk'`
                // (`audio/index.js`'s gait resolve), with the source's sprint
                // tier taken from the movement machine rather than re-derived.
                gait: Some(gait_of(step.running, state.sprinting, state.stance)),
                level: None,
            });
        }
    }

    /* ================================================================ */
    /* Event fan-out — one game event, both subsystems                  */
    /* ================================================================ */

    /// `weapon:fire`. Returns FX's muzzle profile so a caller can place the
    /// flash light it implies.
    ///
    /// The two subsystems disagree on the payload (FX wants `dir`/`intensity`/
    /// `flashScale`, audio wants `suppressed`/`empty`/`firstPerson`), which is
    /// exactly why this lives here and not in either module.
    pub fn weapon_fire(
        &mut self,
        state: &FrameState,
        fx_event: &FxFire,
        audio_event: &AudioFire,
    ) -> Option<crate::fx::muzzle::MuzzleProfile> {
        let camera = camera_frame(state.pose);
        let frame = FxFrame {
            camera,
            view_camera: camera,
            muzzle_world: None,
            sun_dir: Some(state.sun_dir),
            active_sun: Some(state.active_sun),
            fog: self.fog,
            scene: None,
        };
        let profile = self.fx.on_weapon_fire(state.now, &frame, fx_event);
        self.core.borrow_mut().on_fire(audio_event);
        profile
    }

    /// `bullet:impact` — the spark/debris burst and the ricochet.
    pub fn bullet_impact(&mut self, now: f64, e: &FxImpact) {
        self.fx.handle_impact(now, e);
        self.core.borrow_mut().on_impact(&AudioImpact {
            point: e.point.map(|p| [p.0, p.1, p.2]),
            surface: Some(e.surface),
            damage: e.damage,
            exit: e.exit,
        });
    }

    /// `weapon:shell` — the brass, and its bounce.
    pub fn weapon_shell(&mut self, now: f64, e: &FxShell) {
        self.fx.handle_weapon_shell(now, e);
        self.core.borrow_mut().on_shell(&AudioShell {
            position: e.position.map(|p| [p.0, p.1, p.2]),
        });
    }

    /// `explosion`.
    pub fn explosion(&mut self, now: f64, opts: &ExplosionOpts) {
        self.fx.explosion_at(now, opts);
        self.core.borrow_mut().on_explosion(&ExplosionEvent {
            position: [opts.position.0, opts.position.1, opts.position.2],
            radius: Some(opts.radius),
        });
    }

    /* ================================================================ */
    /* Render readback                                                  */
    /* ================================================================ */

    /// Integrate every live particle in the three world-space layers and append
    /// it to `out` (which is cleared first).
    ///
    /// This is the render seam, and it is the *whole* of it that this port can
    /// honestly provide: [`crate::fx::particles::integrate`] is a faithful port
    /// of `PARTICLE_VERT`'s `main()`, so what comes out is precisely what the
    /// source's GPU would have positioned, sized and coloured. Turning those
    /// points into pixels needs an additively-blended camera-facing quad pass
    /// the engine does not expose — see the module doc.
    ///
    /// The view-space layers (`view_add`, `view_lit`) are skipped: nothing
    /// attaches the view scene, so they are always empty.
    pub fn particle_points(&self, now: f64, out: &mut Vec<ParticlePoint>) {
        out.clear();
        let layers: [(&ParticleLayer, bool); 3] = [
            (&self.fx.lit, false),
            (&self.fx.add, true),
            (&self.fx.motes, true),
        ];
        for (layer, additive) in layers {
            for slot in 0..layer.capacity {
                if let Some(s) = particles::integrate(layer, slot, now) {
                    out.push(ParticlePoint {
                        position: s.pos,
                        color: s.color,
                        alpha: s.alpha,
                        size: s.size,
                        tile: layer.tile_at(slot),
                        additive,
                    });
                }
            }
        }
    }
}

/* ==================================================================== */
/* Frame-term derivations                                               */
/* ==================================================================== */

/// The camera's `matrixWorld` and its inverse, from the pose the game resolved.
///
/// The rotation is composed **YXZ** — yaw, then pitch, then roll — because the
/// source overrides Three's default Euler order and
/// [`crate::scene::app::write_camera`] composes it the same way. Composing it
/// any other way here would light every particle from a rotated sun and put the
/// audio listener's ears in the wrong place, both of which are silent-wrong
/// rather than visibly-wrong.
pub fn camera_frame(pose: CameraPose) -> CameraFrame {
    let matrix_world = M4::compose(
        V3::new(pose.eye[0], pose.eye[1], pose.eye[2]),
        camera_quat(pose.rotation),
        V3::new(1.0, 1.0, 1.0),
    );
    CameraFrame {
        matrix_world,
        matrix_world_inverse: matrix_world.invert(),
    }
}

/// `write_camera`'s composition, in `rig_math`'s quaternion.
fn camera_quat(rotation: Euler) -> Q {
    quat_from_axis_angle(V3::new(0.0, 1.0, 0.0), rotation.yaw)
        .multiply(quat_from_axis_angle(V3::new(1.0, 0.0, 0.0), rotation.pitch))
        .multiply(quat_from_axis_angle(V3::new(0.0, 0.0, 1.0), rotation.roll))
}

/// `r?.sunDir` — the direction the renderer decided the sun is in.
/// [`SkyLook::sun_direction`] is documented as pointing *at* the sun, which is
/// the same convention `_syncLighting` transforms into view space.
fn sun_direction(sky: &crate::scene::wiring::look::SkyDriver) -> V3 {
    let d = sky.sun_direction();
    V3::new(f64::from(d.x), f64::from(d.y), f64::from(d.z))
}

/// `r?.activeSun` — `(colour, intensity)`. See [`SOURCE_SUN_INTENSITY`] for the
/// unit conversion.
fn active_sun(sky: &crate::scene::wiring::look::SkyDriver) -> (V3, f64) {
    let key = sky.key_light();
    let c = key.color.to_array();
    (
        V3::new(f64::from(c[0]), f64::from(c[1]), f64::from(c[2])),
        f64::from(key.intensity.get()) * SOURCE_SUN_INTENSITY,
    )
}

/// `p.stance` — the source's string, which `_onPlayerState` only ever compares
/// for inequality.
fn stance_name(stance: Stance) -> &'static str {
    ["stand", "crouch", "prone"][stance as usize]
}

/// `p?.running ? 'run' : p?.crouched ? 'crouch' : 'walk'`, plus the sprint tier
/// [`Gait`] carries and the movement machine already knows.
fn gait_of(running: bool, sprinting: bool, stance: Stance) -> Gait {
    let crouched = stance != Stance::Stand;
    [
        [[Gait::Walk, Gait::Run][usize::from(running)], Gait::Crouch][usize::from(crouched)],
        Gait::Sprint,
    ][usize::from(sprinting & !crouched)]
}

// NOTE: `Surface` is ONE type across the whole port — `crate::audio::foley`
// re-exports `crate::world::palette::Surface` (`foley.rs:25`), and
// `crate::fx::world::FxHit` names the same one. So the movement machine's
// `LandEvent::surface` crosses into both subsystems unconverted. There is no
// missing translation here; do not go looking for it.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::AudioGraph;
    use crate::engine::CAPTURE_SEED;
    use crate::input::Input;
    use crate::physics::surfaces::mask;
    use crate::world::palette::Surface;

    /// Build a `Game` and the pair off the same root, in the source's order.
    /// This is the shape `Game::new` should take once the orchestrator wires
    /// it; here it is reconstructed so the seam is exercised without editing
    /// the shared file.
    fn rig() -> (Game, FxAudio) {
        let game = Game::new(CAPTURE_SEED);
        let config = Config::default();
        // The forks are taken off a stream seeded the same way; see the module
        // doc on why this cannot be byte-identical to the source's yet.
        let mut root = Rng::new(CAPTURE_SEED);
        let fx = build_fx(&mut root, &config, &game.physics);
        let audio = build_audio(&mut root, &game.physics);
        (game, FxAudio::new(fx, audio))
    }

    #[test]
    fn build_fx_binds_the_physics_seam_and_the_quality_budgets() {
        let game = Game::new(CAPTURE_SEED);
        let config = Config::default();
        let mut root = Rng::new(CAPTURE_SEED);
        let fx = build_fx(&mut root, &config, &game.physics);
        assert!(fx.world.is_some(), "the FxWorld seam is bound");
        // The seam really reaches the level's BVH, not an empty world: the
        // spawn point stands on something.
        let spawn = game.spawn.position;
        let gy = fx
            .world
            .as_deref()
            .and_then(|w| w.ground_height(spawn[0], spawn[2], spawn[1] + 6.0));
        assert!(gy.is_some(), "the bound world found no ground under spawn");
        // And a raycast through the same handle hits the same floor.
        let hit = fx.world.as_deref().and_then(|w| {
            w.raycast(
                (spawn[0], spawn[1] + 4.0, spawn[2]),
                (0.0, -1.0, 0.0),
                20.0,
                mask::CHARACTER,
            )
        });
        assert!(hit.is_some(), "the FxWorld raycast seam is dead");
        // `pscale` is derived from the ultra preset's 24k budget: clamped 1.25.
        assert!((fx.pscale - 1.25).abs() < 1e-12, "pscale = {}", fx.pscale);
    }

    #[test]
    fn the_fork_order_is_fx_before_ui_before_audio() {
        // The property the module doc claims: taking fx's fork first and
        // audio's last yields a *different* pair than the reverse, so the
        // ordering is load-bearing rather than decorative.
        let game = Game::new(CAPTURE_SEED);
        let config = Config::default();

        let mut a = Rng::new(CAPTURE_SEED);
        let fx_a = build_fx(&mut a, &config, &game.physics);
        let _hud_fork = a.fork();
        let audio_a = build_audio(&mut a, &game.physics);

        let mut b = Rng::new(CAPTURE_SEED);
        let _hud_fork = b.fork();
        let fx_b = build_fx(&mut b, &config, &game.physics);
        let audio_b = build_audio(&mut b, &game.physics);

        // The atlases are baked off forks of each system's own stream, so a
        // different seed moves every byte.
        assert_ne!(
            fx_a.atlas.data, fx_b.atlas.data,
            "the particle atlas did not move when fx's fork moved"
        );
        drop((audio_a, audio_b));
    }

    #[test]
    fn a_stepped_frame_drives_both_subsystems_from_real_state() {
        let (mut game, mut pair) = rig();
        let mut input = Input::new();
        assert!(pair.start_audio(48_000.0));

        let mut report = FxAudioReport {
            prewarm_due: false,
            stats: FxStats::default(),
            live_lights: 0,
            live_shells: 0,
            audio_running: false,
        };
        let mut prewarms = 0u32;
        for _ in 0..10 {
            game.frame(1.0 / 60.0, &mut input);
            report = pair.frame(&FrameState::of(&game), &MovementPulse::default(), true);
            prewarms += u32::from(report.prewarm_due);
        }
        assert!(report.audio_running, "the audio graph never came up");
        assert_eq!(prewarms, 1, "the pre-warm gate is a one-shot");

        // `_syncLighting` really ran against the sky rather than keeping its
        // constructed defaults: `sunFactor` is `clamp(sunI / 4.3, 0, 1.6)` and
        // `SOURCE_SUN_INTENSITY` is exactly that divisor, so it comes back as
        // the sky's own 0..1 intensity.
        let expected = f64::from(game.sky.key_light().intensity.get()).clamp(0.0, 1.6);
        assert!(
            (pair.fx.sun_factor - expected).abs() < 1e-9,
            "sun_factor {} did not come from the sky's {expected}",
            pair.fx.sun_factor
        );
        // And the derived ambient left `index.js:127-133`'s literals behind.
        assert_ne!(
            (pair.fx.amb_top.x, pair.fx.amb_top.y, pair.fx.amb_top.z),
            (0.42, 0.5, 0.66),
            "the ambient is still the constructor's, so _syncLighting did not run"
        );
        // The frame clock reached the subsystem.
        assert!((pair.fx.now - game.time.elapsed).abs() < 1e-12);
    }

    #[test]
    fn the_listener_basis_follows_the_camera_the_game_resolved() {
        let (mut game, mut pair) = rig();
        let mut input = Input::new();
        input.pointer_locked = true;
        // Turn ninety degrees and check the listener turned with it.
        for _ in 0..60 {
            input.mouse_move(20.0, 0.0);
            game.frame(1.0 / 60.0, &mut input);
            pair.frame(&FrameState::of(&game), &MovementPulse::default(), true);
        }
        let pose = game.pose();
        let cam = camera_frame(pose);
        let e = &cam.matrix_world.e;
        // The composed matrix's translation IS the eye.
        assert!((e[12] - pose.eye[0]).abs() < 1e-12);
        assert!((e[13] - pose.eye[1]).abs() < 1e-12);
        assert!((e[14] - pose.eye[2]).abs() < 1e-12);
        // Forward is unit, and it is the yaw the movement machine holds.
        let fwd = (-e[8], -e[9], -e[10]);
        let len = (fwd.0 * fwd.0 + fwd.1 * fwd.1 + fwd.2 * fwd.2).sqrt();
        assert!((len - 1.0).abs() < 1e-9, "forward is not unit: {len}");
        let yaw_fwd = (-game.movement.yaw.sin(), -game.movement.yaw.cos());
        let dot = (fwd.0 * yaw_fwd.0 + fwd.2 * yaw_fwd.1) / fwd.0.hypot(fwd.2);
        assert!(dot > 0.99, "the listener faces the wrong way, dot = {dot}");
    }

    #[test]
    fn a_landing_pulse_sprays_dust_and_plays_a_footfall() {
        let (mut game, mut pair) = rig();
        let mut input = Input::new();
        pair.start_audio(48_000.0);
        game.frame(1.0 / 60.0, &mut input);

        let before = pair.fx.lit.spawned();
        let pulse = MovementPulse {
            land: Some(LandEvent {
                pending: true,
                speed: 7.5,
                surface: Surface::Concrete,
            }),
            step: None,
        };
        pair.frame(&FrameState::of(&game), &pulse, true);
        assert!(
            pair.fx.lit.spawned() > before,
            "a hard landing emitted no dust"
        );
        // Under the source's 3.2 m/s gate a soft landing emits nothing.
        let quiet = pair.fx.lit.spawned();
        let soft = MovementPulse {
            land: Some(LandEvent {
                pending: true,
                speed: 1.0,
                surface: Surface::Concrete,
            }),
            step: None,
        };
        pair.frame(&FrameState::of(&game), &soft, true);
        assert_eq!(pair.fx.lit.spawned(), quiet, "a soft landing sprayed dust");
    }

    #[test]
    fn a_running_footstep_pulse_reaches_fx_and_audio() {
        let (mut game, mut pair) = rig();
        let mut input = Input::new();
        pair.start_audio(48_000.0);
        game.frame(1.0 / 60.0, &mut input);
        // One quiet frame so the listener is at the player's ears before the
        // first footstep is placed — `on_footstep` gates on distance-to-
        // listener, and the spatial field starts at the origin.
        pair.frame(&FrameState::of(&game), &MovementPulse::default(), true);
        let before = pair.core().borrow().stats;

        let step = StepEvent {
            pending: true,
            running: true,
            surface: Surface::Concrete,
            x: game.movement.position[0],
            y: game.movement.position[1],
            z: game.movement.position[2],
            left: true,
        };
        // The FX half is a coin flip (`rng.float() > 0.55`), so drive enough
        // steps that both arms are taken.
        let fx_before = pair.fx.lit.spawned() + pair.fx.add.spawned();
        for _ in 0..12 {
            pair.frame(
                &FrameState::of(&game),
                &MovementPulse {
                    land: None,
                    step: Some(step),
                },
                true,
            );
        }
        let after = pair.core().borrow().stats;
        assert!(
            after.events > before.events,
            "twelve footsteps scheduled no audio voice ({} -> {})",
            before.events,
            after.events
        );
        assert!(
            pair.fx.lit.spawned() + pair.fx.add.spawned() > fx_before,
            "twelve running footsteps kicked up no dust at all"
        );
    }

    #[test]
    fn the_particle_readback_reports_live_world_particles() {
        let (mut game, mut pair) = rig();
        let mut input = Input::new();
        game.frame(1.0 / 60.0, &mut input);
        pair.frame(&FrameState::of(&game), &MovementPulse::default(), true);

        let pulse = MovementPulse {
            land: Some(LandEvent {
                pending: true,
                speed: 9.0,
                surface: Surface::Concrete,
            }),
            step: None,
        };
        pair.frame(&FrameState::of(&game), &pulse, true);

        let mut points = Vec::new();
        pair.particle_points(game.time.elapsed, &mut points);
        assert!(!points.is_empty(), "the landing spray read back as nothing");
        // Every point is finite, alive and near the player's feet.
        let feet = game.movement.position;
        points.iter().for_each(|p| {
            assert!(p.position.0.is_finite() && p.alpha > 0.0 && p.size > 0.0);
        });
        let near = points
            .iter()
            .filter(|p| (p.position.0 - feet[0]).hypot(p.position.2 - feet[2]) < 3.0)
            .count();
        assert!(near > 0, "no dust landed anywhere near the player");
    }

    /// The browser clock path: `advance_clock == false` leaves the graph's
    /// clock alone, and `set_context_time` sets it absolutely. Before
    /// `AudioCore::set_context_time` existed there was no way to do the second,
    /// so a browser caller had to accumulate deltas and drift off the device.
    #[test]
    fn the_device_clock_can_be_pushed_in_absolutely() {
        let (mut game, mut pair) = rig();
        let mut input = Input::new();
        pair.start_audio(48_000.0);
        game.frame(1.0 / 60.0, &mut input);

        let t0 = pair.core().borrow().graph().map(AudioGraph::current_time);
        // A frame that does NOT advance the clock leaves it exactly where the
        // device left it.
        pair.frame(&FrameState::of(&game), &MovementPulse::default(), false);
        assert_eq!(
            pair.core().borrow().graph().map(AudioGraph::current_time),
            t0,
            "the browser path advanced a clock it does not own"
        );

        // And an absolute push lands exactly, not relatively.
        pair.set_context_time(12.5);
        assert_eq!(
            pair.core().borrow().graph().map(AudioGraph::current_time),
            Some(12.5)
        );
        pair.set_context_time(12.5 + 1.0 / 60.0);
        assert_eq!(
            pair.core().borrow().graph().map(AudioGraph::current_time),
            Some(12.5 + 1.0 / 60.0)
        );

        // The native path still advances by dt.
        pair.frame(&FrameState::of(&game), &MovementPulse::default(), true);
        let after = pair
            .core()
            .borrow()
            .graph()
            .map(AudioGraph::current_time)
            .unwrap();
        assert!(after > 12.5 + 1.0 / 60.0, "the native clock stalled");
    }

    #[test]
    fn gait_resolves_the_source_ladder() {
        assert_eq!(gait_of(false, false, Stance::Stand), Gait::Walk);
        assert_eq!(gait_of(true, false, Stance::Stand), Gait::Run);
        assert_eq!(gait_of(true, true, Stance::Stand), Gait::Sprint);
        assert_eq!(gait_of(false, false, Stance::Crouch), Gait::Crouch);
        // Crouching wins over sprinting: you cannot sprint crouched.
        assert_eq!(gait_of(true, true, Stance::Crouch), Gait::Crouch);
        assert_eq!(stance_name(Stance::Prone), "prone");
    }
}

/// The registry face of the fx system — `fx/index.js:37`.
///
/// Same two-phase shape as [`crate::world::system::WorldSubsystem`]; see that
/// type for why construction must be empty and the fork must happen in
/// [`Subsystem::init`].
///
/// **This one does fork**, and it is slot 8 in the pinned root sequence
/// (`world, weapons, fx, ai, ui, audio`). `build_fx` draws from the root and
/// `build_audio` draws again afterwards, so a registry driving these two has to
/// call them in that order and no other —
/// `crate::scene::game::tests::the_root_stream_is_consumed_in_the_registrys_order`
/// is what fails if it does not.
///
/// It needs the physics world at init, which `Ctx` does not carry. That is the
/// same shortfall `ai::system::AiSystem::update` records, and it is handed in
/// here explicitly rather than degraded around: a system that cannot see
/// physics builds no impact decals and no ground splash, and would look like a
/// working fx system producing almost nothing.
pub struct FxSubsystem {
    built: Option<FxSystem>,
    config: Config,
    physics: Option<Rc<PhysicsWorld>>,
}

impl FxSubsystem {
    /// An unbuilt fx system. `physics` is the world its impact and splash
    /// queries trace against; without it the system builds, and produces almost
    /// nothing.
    pub fn new(config: Config, physics: Option<Rc<PhysicsWorld>>) -> Self {
        FxSubsystem {
            built: None,
            config,
            physics,
        }
    }

    /// The built system, or `None` before the registry has run `init`.
    pub const fn get(&self) -> Option<&FxSystem> {
        self.built.as_ref()
    }

    /// The built system, mutably.
    pub const fn get_mut(&mut self) -> Option<&mut FxSystem> {
        self.built.as_mut()
    }
}

impl Subsystem for FxSubsystem {
    fn id(&self) -> &'static str {
        "fx"
    }

    /// `static deps = ['render', 'materials']` (`fx/index.js:38`).
    fn deps(&self) -> &'static [&'static str] {
        &["render", "materials"]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Update]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Slot 8: the fork the pinned sequence expects between `weapons` and `ai`.
    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), crate::error::CoreError> {
        let mut root = ctx.rng.borrow_mut();
        self.built = self
            .physics
            .as_ref()
            .map(|physics| build_fx(&mut root, &self.config, physics));
        Ok(())
    }
}

#[cfg(test)]
mod fx_subsystem_tests {
    use super::*;

    #[test]
    fn it_answers_to_the_id_and_the_sources_deps() {
        let fx = FxSubsystem::new(Config::default(), None);
        assert_eq!(fx.id(), "fx");
        assert_eq!(fx.deps(), &["render", "materials"]);
        assert!(fx.get().is_none());
    }

    /// **Construction is free; `init` is where the stream moves.** If this ever
    /// forks at construction, the fork lands in registration order and the
    /// registry re-orders init around a draw already spent.
    #[test]
    fn construction_draws_nothing_from_the_root_stream() {
        let rng = crate::rng::Rng::new(11);
        let before = rng.state();
        let _fx = FxSubsystem::new(Config::default(), None);
        assert_eq!(rng.state(), before);
    }
}
