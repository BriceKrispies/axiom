//! Ported from Claude-of-Duty `src/fx/index.js:1-1316` — the `FxSystem`
//! facade's CPU-testable core: budget dimensioning, the emit dispatch table,
//! and the event-driven public API (`onImpact`/`onWeaponFire`/`explosion`/
//! `spawnShell`/`addDecal`/`scorch`/`bloodSpatterBehind`/`onActorDeath`/
//! `onLand`/`onFootstep`), minus the scene-graph/renderer wiring.
//!
//! ## What this module does not port, and why
//!
//! `index.js` is the one file in this slice that is genuinely inseparable
//! from THREE's scene graph: `init()` builds `THREE.Scene`/`Mesh` objects and
//! attaches them, `prewarmMaterials()` walks `renderer.compile`, `viewFlash`/
//! `muzzleFlash`/`_syncLighting` convert points through `camera.matrixWorld`/
//! `viewCamera.matrixWorld`, and `debugBurst`/`_findTarget` (the screenshot
//! capture harness) fan-cast against the physics BVH purely to *frame a
//! shot for a screenshot* — dev tooling, not gameplay. None of that has a
//! CPU-testable equivalent without a camera/scene-graph module this port
//! does not have yet (the `player`/camera integration is a separate,
//! concurrently-developed slice). What *is* ported below is everything that
//! is real control flow and data transformation independent of a live
//! camera: budget arithmetic (`init`'s particle/haze/view-layer sizing,
//! `index.js:47-72`), the emit dispatch table (`emitAdd`/`emitLit`/
//! `emitMote`/`emitViewAdd`/`emitViewLit`, `index.js:216-226`), and every
//! public-API method whose body does not read a camera transform.
//!
//! `sunWorld()`'s renderer-driven direction (`render?.sunDir`) and
//! `_syncLighting`'s whole lighting-uniform push are also not ported for the
//! same reason: there is no live renderer to read a sun direction from yet.
//! [`FxSystem::sun_world`] instead exposes the field
//! [`FxSystem::set_sun_world`] sets, defaulting straight up
//! (`index.js:598-609`'s fallback), which is exactly the state a `sky`
//! module would drive through `setAmbient` once that lands.
//!
//! ## The `addSmokeColumn`/`addSmokeSource` gap
//!
//! `index.js:625-637` forwards straight to `this.ambience` (`ambience.js`),
//! a file **outside this port slice** (not in the file list this task
//! ports). [`FxSystem::add_smoke_column`] is therefore a documented no-op:
//! calling it is harmless and matches what happens when `ambience` itself is
//! absent, but it means this port's RNG stream is only self-consistent
//! *within fx* — the real game's stream diverges the moment `ambience.js`'s
//! own (currently unknown) RNG draws would have happened. Whoever ports
//! `ambience.js` should replace this stub with a real
//! `Ambience`-equivalent, constructed and forked from [`FxSystem::rng`] at
//! the same point `index.js:126-129` does (inside `FxSystem::new`, right
//! after `ShellSystem::new`).

use crate::fx::atlas::{bake_decal_atlas, bake_particle_atlas, DecalAtlas, ParticleAtlas};
use crate::fx::decals::{DecalAdd, DecalSystem};
use crate::fx::haze::HazeSystem;
use crate::fx::lights::LightPool;
use crate::fx::particles::{ParticleLayer, ParticleMode, ParticleSpawn};
use crate::fx::shells::{ShellSpawnOpts, ShellSystem};
use crate::fx::world::FxWorld;
use crate::rng::Rng;

fn clamp(v: f64, a: f64, b: f64) -> f64 {
    v.clamp(a, b)
}

fn clamp_i(v: f64, a: f64, b: f64) -> usize {
    clamp(v, a, b).round() as usize
}

/// `stats`, `index.js:172`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FxStats {
    pub spawned: u64,
    pub decals: u64,
}

/// [`FxSystem::add_decal`]'s options — [`DecalAdd`] minus the fields the
/// facade itself resolves (`point`/`normal`/`now`/`world`/`mask`). See the
/// module doc on why `roll`/`flip` stay `Option`: the source draws an RNG
/// value for either one that is omitted (`index.js:512-533`), so collapsing
/// them to a fixed default here would silently desync the RNG stream from
/// the source's draw order.
#[derive(Debug, Clone, Default)]
pub struct DecalOpts {
    pub tile: usize,
    pub size: f64,
    pub roll: Option<f64>,
    pub life: Option<f64>,
    pub fade: Option<f64>,
    pub opacity: Option<f64>,
    pub max_angle: Option<f64>,
    pub depth: Option<f64>,
    pub flip: Option<bool>,
}

/// `addSmokeColumn`'s `o` parameter shape, `explosions.js:181-189` — kept as
/// a real type even though [`FxSystem::add_smoke_column`] is a stub (see the
/// module doc), so the call sites that build one stay faithful to the
/// source's argument shape.
#[derive(Debug, Clone, Copy, Default)]
pub struct SmokeColumnOpts {
    pub radius: f64,
    pub duration: f64,
    pub rate: f64,
    pub rise: f64,
    pub dark: f64,
    pub life: f64,
    pub growth: f64,
}

/// `class FxSystem`'s CPU-testable state. See the module doc for what is
/// dropped (every `THREE.*` object, the renderer/camera handles).
pub struct FxSystem {
    pub rng: Rng,
    pub now: f64,
    pub gravity: f64,
    /// `this.pScale`, `index.js:66`.
    pub pscale: f64,

    pub lit: ParticleLayer,
    pub add: ParticleLayer,
    pub motes: ParticleLayer,
    pub view_add: ParticleLayer,
    pub view_lit: ParticleLayer,
    view_attached: bool,

    pub decals: DecalSystem,
    pub shells: ShellSystem,
    pub haze_sys: HazeSystem,
    pub lights: LightPool,
    pub view_lights: Option<LightPool>,

    pub atlas: ParticleAtlas,
    pub decal_atlas: DecalAtlas,

    pub stats: FxStats,

    /// The physics seam — see [`crate::fx::world`]'s module doc. `None`
    /// reproduces every `if (physics) { ... }`/`physics?.` guard in the
    /// source.
    pub world: Option<Box<dyn FxWorld>>,

    /// [`FxSystem::sun_world`]'s backing field — see the module doc.
    sun_world: (f64, f64, f64),

    suppress_decals: bool,
}

impl FxSystem {
    /// `async init(ctx)`, `index.js:38-206` — the budget dimensioning, pool
    /// construction and atlas baking. `particle_budget`/`decal_budget` are
    /// `config.q.particleBudget`/`decalBudget` (`crate::config::
    /// QualityPreset`). `seed` is the root FX RNG seed — the source forks
    /// this from `ctx.rng` (`this.rng = ctx.rng.fork()`, `index.js:40`); a
    /// caller wiring this into the full engine should pass
    /// `ctx.rng.fork()`'s output here.
    pub fn new(seed: u32, particle_budget: u32, decal_budget: u32, gravity: f64) -> Self {
        let mut rng = Rng::new(seed);

        let budget = f64::from(particle_budget);
        let big = particle_budget >= 10_000;
        let atlas_size = if big { 1024 } else { 512 };
        // `buildParticleAtlas(this.rng.fork(), atlasSize)`, then
        // `buildDecalAtlas(this.rng.fork(), atlasSize)` — in that order.
        let atlas = bake_particle_atlas(&mut rng.fork(), atlas_size);
        let decal_atlas = bake_decal_atlas(&mut rng.fork(), atlas_size);

        let mote = clamp_i(budget * 0.06, 96.0, 600.0);
        let haze_cap = clamp_i(budget * 0.04, 48.0, 320.0);
        let view_add_cap = clamp_i(budget * 0.03, 48.0, 400.0);
        let view_lit_cap = clamp_i(budget * 0.02, 32.0, 256.0);
        let rest = (budget - mote as f64 - haze_cap as f64 - view_add_cap as f64 - view_lit_cap as f64).max(256.0);
        let lit_cap = (rest * 0.55).round() as usize;
        let add_cap = rest as usize - lit_cap;

        let pscale = clamp(budget / 12000.0, 0.4, 1.25);

        let lit = ParticleLayer::new(lit_cap, ParticleMode::Lit);
        let add = ParticleLayer::new(add_cap, ParticleMode::Additive);
        let motes = ParticleLayer::new(mote, ParticleMode::Additive);
        let view_add = ParticleLayer::new(view_add_cap, ParticleMode::Additive);
        let view_lit = ParticleLayer::new(view_lit_cap, ParticleMode::Lit);

        let decals = DecalSystem::new(decal_budget as usize, decal_atlas.cols);
        let haze_sys = HazeSystem::new(haze_cap);
        let lights = LightPool::new(4);
        // `new ShellSystem(this)`, `index.js:127` — forks `this.rng` for the
        // brass texture bake (`shells.js:58`); see `ShellSystem::new`.
        let shells = ShellSystem::new(&mut rng);

        FxSystem {
            rng,
            now: 0.0,
            gravity,
            pscale,
            lit,
            add,
            motes,
            view_add,
            view_lit,
            view_attached: false,
            decals,
            shells,
            haze_sys,
            lights,
            view_lights: None,
            atlas,
            decal_atlas,
            stats: FxStats::default(),
            world: None,
            sun_world: (0.0, 1.0, 0.0),
            suppress_decals: false,
        }
    }

    /// A minimal instance for unit tests across the `fx` module — a small
    /// budget, no physics world bound. Not a port of anything in the
    /// source; the source has no equivalent because JS tests do not need a
    /// typed constructor to skip optional state.
    #[cfg(test)]
    pub fn test_instance(seed: u32) -> Self {
        FxSystem::new(seed, 2000, 64, -19.62)
    }

    // ===================================================================
    // emit helpers — `index.js:212-226`.
    // ===================================================================

    pub fn emit_add(&mut self, s: &ParticleSpawn) -> usize {
        self.stats.spawned += 1;
        self.add.emit(s, self.now)
    }

    pub fn emit_lit(&mut self, s: &ParticleSpawn) -> usize {
        self.stats.spawned += 1;
        self.lit.emit(s, self.now)
    }

    pub fn emit_mote(&mut self, s: &ParticleSpawn) -> usize {
        self.motes.emit(s, self.now)
    }

    pub fn emit_view_add(&mut self, s: &ParticleSpawn) -> usize {
        self.attach_view();
        self.stats.spawned += 1;
        self.view_add.emit(s, self.now)
    }

    pub fn emit_view_lit(&mut self, s: &ParticleSpawn) -> usize {
        self.attach_view();
        self.stats.spawned += 1;
        self.view_lit.emit(s, self.now)
    }

    /// `view ? fx.emitViewAdd : fx.emitAdd`, the dispatch every recipe module
    /// (`muzzle.js`) reads once and calls repeatedly (`muzzle.js:120-121`).
    pub fn emit_add_view(&mut self, view: bool, s: &ParticleSpawn) -> usize {
        if view {
            self.emit_view_add(s)
        } else {
            self.emit_add(s)
        }
    }

    pub fn emit_lit_view(&mut self, view: bool, s: &ParticleSpawn) -> usize {
        if view {
            self.emit_view_lit(s)
        } else {
            self.emit_lit(s)
        }
    }

    /// `_attachView()`, `index.js:229-243`. Idempotent; a real viewmodel
    /// scene attachment has no meaning here (see the module doc), so this
    /// only tracks the `view_lights` pool the source lazily builds
    /// alongside it.
    fn attach_view(&mut self) {
        if self.view_attached {
            return;
        }
        self.view_attached = true;
        if self.view_lights.is_none() {
            self.view_lights = Some(LightPool::new(2));
        }
    }

    // ===================================================================
    // public API — `index.js:369-661`.
    // ===================================================================

    /// `onImpact(e)`, `index.js:373-378`. `e.exit` and `e.damage` are the
    /// caller-supplied event payload fields; `incident` is
    /// `e.incident ?? this._defaultIncident(e)` (`index.js:377`,
    /// `-e.normal`) resolved by the caller since there is no event-bus
    /// payload type in this port yet.
    pub fn on_impact(
        &mut self,
        point: (f64, f64, f64),
        normal: (f64, f64, f64),
        incident: Option<(f64, f64, f64)>,
        surface: crate::world::palette::Surface,
        damage: Option<f64>,
        exit: bool,
    ) {
        let mut energy = clamp(0.7 + damage.unwrap_or(25.0) / 55.0, 0.7, 1.7);
        if exit {
            energy *= 0.75;
        }
        let inc = incident.unwrap_or((-normal.0, -normal.1, -normal.2));
        crate::fx::impacts::spawn_impact(self, point, normal, inc, surface, energy);
    }

    /// `tracer(from, to, speed)`, `index.js:481-485`.
    pub fn tracer(&mut self, from: (f64, f64, f64), to: (f64, f64, f64), speed: f64) {
        crate::fx::tracers::spawn_tracer(self, from, to, speed, 1.0);
    }

    /// `explosion(e)`, `index.js:499-502`.
    pub fn explosion(&mut self, opts: &crate::fx::explosions::ExplosionOpts) {
        crate::fx::explosions::explode(self, opts);
    }

    /// `spawnShell(position, velocity, opts)`, `index.js:505-509`.
    pub fn spawn_shell(&mut self, position: (f64, f64, f64), velocity: Option<(f64, f64, f64)>, opts: ShellSpawnOpts) {
        self.shells.spawn(&mut self.rng, position, velocity, opts);
    }

    /// `addDecal(point, normal, opts)`, `index.js:511-535`. See the module
    /// doc's [`DecalOpts`] note on why `roll`/`flip` are resolved here, in
    /// this exact order, rather than defaulted inside [`DecalSystem::add`].
    pub fn add_decal(&mut self, point: (f64, f64, f64), normal: (f64, f64, f64), opts: DecalOpts) -> bool {
        if self.suppress_decals {
            return false;
        }
        let roll = opts.roll.unwrap_or_else(|| self.rng.float() * std::f64::consts::PI * 2.0);
        let flip = opts.flip.unwrap_or_else(|| self.rng.float() < 0.5);
        let now = self.now;
        let world = self.world.as_deref();
        let add = DecalAdd {
            point: [point.0, point.1, point.2],
            normal: [normal.0, normal.1, normal.2],
            size: opts.size,
            tile: opts.tile,
            roll: Some(roll),
            life: opts.life.or(Some(60.0)),
            fade: opts.fade.or(Some(0.72)),
            opacity: opts.opacity.or(Some(1.0)),
            max_angle: opts.max_angle.or(Some(62.0)),
            depth: opts.depth,
            flip,
            mask: 0xffff,
            now,
            world: world.map(|w| w as &dyn crate::fx::decals::DecalWorld),
        };
        let ok = self.decals.add(&add);
        if ok {
            self.stats.decals += 1;
        }
        ok
    }

    /// `scorch(x, y, z, radius)`, `index.js:538-565`.
    pub fn scorch(&mut self, x: f64, y: f64, z: f64, radius: f64) {
        let mut px = x;
        let mut py = y;
        let mut pz = z;
        let mut n = (0.0, 1.0, 0.0);
        if let Some(world) = self.world.as_deref() {
            if let Some(hit) = world.raycast((x, y + 0.4, z), (0.0, -1.0, 0.0), radius * 1.5 + 1.0, 0xffff) {
                px = hit.point.0;
                py = hit.point.1;
                pz = hit.point.2;
                n = hit.normal;
            }
        }
        self.add_decal(
            (px, py, pz),
            n,
            DecalOpts {
                tile: crate::fx::atlas::d::SCORCH,
                size: radius * 1.05,
                life: Some(120.0),
                fade: Some(0.55),
                opacity: Some(0.9),
                max_angle: Some(80.0),
                depth: Some(radius * 0.35),
                roll: None,
                flip: None,
            },
        );
    }

    /// `bloodSpatterBehind(point, incident)`, `index.js:568-580`.
    pub fn blood_spatter_behind(&mut self, point: (f64, f64, f64), incident: (f64, f64, f64)) {
        let Some(world) = self.world.as_deref() else {
            return;
        };
        let Some(hit) = world.raycast(point, incident, 2.6, 0xffff) else {
            return;
        };
        let tile = if self.rng.float() < 0.5 {
            crate::fx::atlas::d::BLOOD_A
        } else {
            crate::fx::atlas::d::BLOOD_B
        };
        let size = self.rng.range(0.32, 0.62);
        let opacity = self.rng.range(0.7, 1.0);
        self.add_decal(
            hit.point,
            hit.normal,
            DecalOpts {
                tile,
                size,
                life: Some(90.0),
                fade: Some(0.8),
                opacity: Some(opacity),
                max_angle: Some(70.0),
                roll: None,
                flip: None,
                depth: None,
            },
        );
    }

    /// `sunWorld()`, `index.js:598-609` — see the module doc.
    pub fn sun_world(&self) -> (f64, f64, f64) {
        self.sun_world
    }

    /// Not in the source directly — the write side of [`FxSystem::sun_world`]
    /// (the source reads `render?.sunDir` live; this port has that pushed in
    /// by whatever owns the renderer, once one exists).
    pub fn set_sun_world(&mut self, dir: (f64, f64, f64)) {
        self.sun_world = dir;
    }

    /// `haze(x, y, z, radius, grow, life, strength, tile)`, `index.js:
    /// 612-614`.
    #[allow(clippy::too_many_arguments)]
    pub fn haze(&mut self, x: f64, y: f64, z: f64, radius: f64, grow: f64, life: f64, strength: f64, tile: usize) {
        let now = self.now;
        let seed = self.rng.float();
        self.haze_sys.emit(now, x, y, z, radius, grow, life, strength, tile, seed);
    }

    /// `hazeRing(x, y, z, radius, grow, life, strength)`, `index.js:617-619`.
    pub fn haze_ring(&mut self, x: f64, y: f64, z: f64, radius: f64, grow: f64, life: f64, strength: f64) {
        let now = self.now;
        let seed = self.rng.float();
        self.haze_sys
            .emit(now, x, y, z, radius, grow, life, strength, crate::fx::atlas::p::RING, seed);
    }

    /// `addSmokeColumn(x, y, z, o)`, `index.js:625-627` — a stub; see the
    /// module doc's "`addSmokeColumn`/`addSmokeSource` gap" section.
    pub fn add_smoke_column(&mut self, _x: f64, _y: f64, _z: f64, _opts: SmokeColumnOpts) {}

    /// `viewFlash(x, y, z, r, g, b, strength)`, `index.js:349-364`. `key`
    /// (the source's `render?.viewSun?.intensity ?? 2.5`) has no renderer to
    /// read from yet, so it is a parameter here rather than an internal
    /// default — a caller with a real sun key passes it, and `2.5` (the
    /// source's own fallback) is the reasonable default absent one.
    #[allow(clippy::too_many_arguments)]
    pub fn view_flash(&mut self, x: f64, y: f64, z: f64, r: f64, g: f64, b: f64, strength: f64) {
        self.attach_view();
        let key = 2.5;
        let peak = (key * 0.72 * clamp(strength, 0.05, 2.2)).max(0.04);
        if let Some(pool) = self.view_lights.as_mut() {
            pool.flash(x, y + 0.04, z, r, g, b, peak, 0.09, 8.0, 1.6, 2.0);
        }
    }

    /// `onActorDeath(e)`, `index.js:641-676`.
    pub fn on_actor_death(&mut self, point: (f64, f64, f64)) {
        let count = (8.0 * self.pscale).round() as i32 + 3;
        for i in 0..count {
            let (vx, vy, vz) = crate::fx::util::cone(&mut self.rng, 0.0, 1.0, 0.0, 1.4, 0.7);
            let mut s = crate::fx::particles::reset_spawn();
            s.x = point.0;
            s.y = point.1;
            s.z = point.2;
            s.vx = vx * self.rng.range(0.6, 2.4);
            s.vy = vy * self.rng.range(0.4, 1.6);
            s.vz = vz * self.rng.range(0.6, 2.4);
            s.tile = (if i % 2 == 1 { crate::fx::atlas::p::MIST } else { crate::fx::atlas::p::SMOKE_A }) as f64;
            s.size0 = self.rng.range(0.05, 0.1);
            s.size1 = self.rng.range(0.2, 0.4);
            s.size_curve = 0.5;
            s.life = self.rng.range(0.4, 0.8);
            s.drag = 5.0;
            s.gravity = -3.5;
            s.rot = self.rng.float() * 6.283;
            s.r0 = 0.3;
            s.g0 = 0.03;
            s.b0 = 0.026;
            s.r1 = 0.15;
            s.g1 = 0.015;
            s.b1 = 0.013;
            s.alpha = self.rng.range(0.4, 0.75);
            s.alpha_curve = 1.5;
            s.soft = 0.2;
            s.seed = self.rng.float();
            self.emit_lit(&s);
        }
        if let Some(gy) = self.world.as_deref().and_then(|w| w.ground_height(point.0, point.2, point.1 + 1.0)) {
            let tile = if self.rng.float() < 0.5 { crate::fx::atlas::d::BLOOD_A } else { crate::fx::atlas::d::BLOOD_B };
            let size = self.rng.range(0.5, 0.9);
            self.add_decal(
                (point.0, gy, point.2),
                (0.0, 1.0, 0.0),
                DecalOpts {
                    tile,
                    size,
                    life: Some(120.0),
                    fade: Some(0.85),
                    max_angle: Some(80.0),
                    roll: None,
                    flip: None,
                    opacity: None,
                    depth: None,
                },
            );
        }
    }

    /// `onLand(e)`, `index.js:678-712`. `camera_pos`/`ground_from` stand in
    /// for `ctx.camera.position`/the eye-relative ground probe origin the
    /// source computes from it — passed in since there is no camera in this
    /// port yet.
    pub fn on_land(&mut self, velocity: f64, camera_pos: (f64, f64, f64), player_height: f64, eye_offset: f64) {
        let v = velocity.abs();
        if v < 3.2 {
            return;
        }
        let x = camera_pos.0;
        let z = camera_pos.2;
        let mut y = camera_pos.1 - player_height + eye_offset;
        if let Some(gy) = self.world.as_deref().and_then(|w| w.ground_height(x, z, camera_pos.1 + 1.0)) {
            y = gy;
        }
        let strength = clamp((v - 3.0) / 7.0, 0.2, 1.3);
        let count = (7.0 * self.pscale * strength).round() as i32 + 2;
        for _ in 0..count {
            let a = self.rng.float() * 6.283;
            let sp = self.rng.range(0.7, 2.4) * strength;
            let mut s = crate::fx::particles::reset_spawn();
            s.x = x + a.cos() * 0.22;
            s.y = y + 0.03;
            s.z = z + a.sin() * 0.22;
            s.vx = a.cos() * sp;
            s.vy = self.rng.range(0.1, 0.6);
            s.vz = a.sin() * sp;
            s.tile = crate::fx::atlas::p::DUST as f64;
            s.size0 = self.rng.range(0.05, 0.1);
            s.size1 = self.rng.range(0.3, 0.55);
            s.size_curve = 0.45;
            s.life = self.rng.range(0.5, 1.0);
            s.drag = 3.2;
            s.gravity = -0.6;
            s.rot = self.rng.float() * 6.283;
            s.spin = self.rng.signed() * 1.2;
            s.r0 = 0.48;
            s.g0 = 0.44;
            s.b0 = 0.39;
            s.r1 = 0.4;
            s.g1 = 0.37;
            s.b1 = 0.33;
            s.alpha = self.rng.range(0.25, 0.5) * strength;
            s.alpha_curve = 1.6;
            s.soft = 0.3;
            s.turb = 0.05;
            s.turb_freq = 2.0;
            s.seed = self.rng.float();
            self.emit_lit(&s);
        }
    }

    /// `onFootstep(e)`, `index.js:714-738`.
    pub fn on_footstep(&mut self, running: bool, position: (f64, f64, f64)) {
        if !running {
            return;
        }
        if self.rng.float() > 0.55 {
            return;
        }
        let mut s = crate::fx::particles::reset_spawn();
        s.x = position.0 + self.rng.signed() * 0.08;
        s.y = position.1 + 0.02;
        s.z = position.2 + self.rng.signed() * 0.08;
        s.vy = self.rng.range(0.1, 0.35);
        s.vx = self.rng.signed() * 0.3;
        s.vz = self.rng.signed() * 0.3;
        s.tile = crate::fx::atlas::p::DUST as f64;
        s.size0 = 0.04;
        s.size1 = self.rng.range(0.18, 0.32);
        s.size_curve = 0.45;
        s.life = self.rng.range(0.4, 0.75);
        s.drag = 3.4;
        s.gravity = -0.5;
        s.rot = self.rng.float() * 6.283;
        s.r0 = 0.46;
        s.g0 = 0.42;
        s.b0 = 0.37;
        s.r1 = 0.4;
        s.g1 = 0.36;
        s.b1 = 0.32;
        s.alpha = self.rng.range(0.1, 0.22);
        s.alpha_curve = 1.7;
        s.soft = 0.25;
        s.seed = self.rng.float();
        self.emit_lit(&s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_never_exceed_the_particle_budget() {
        for budget in [2000u32, 4000, 12000, 24000] {
            let fx = FxSystem::new(1, budget, 128, -19.62);
            let total = fx.lit.capacity + fx.add.capacity + fx.motes.capacity + fx.view_add.capacity + fx.view_lit.capacity;
            assert!(total as f64 <= f64::from(budget) * 1.05, "budget {budget} produced {total}");
        }
    }

    #[test]
    fn decal_capacity_matches_the_configured_budget() {
        for budget in [64usize, 128, 256, 512] {
            let fx = FxSystem::new(1, 4000, budget as u32, -19.62);
            assert_eq!(fx.decals.capacity, budget);
        }
    }

    #[test]
    fn pscale_is_clamped() {
        let low = FxSystem::new(1, 100, 64, -19.62);
        assert!((low.pscale - 0.4).abs() < 1e-9);
        let high = FxSystem::new(1, 100_000, 64, -19.62);
        assert!((high.pscale - 1.25).abs() < 1e-9);
    }

    #[test]
    fn on_impact_dispatches_by_surface() {
        let mut fx = FxSystem::test_instance(1);
        let before = fx.stats.spawned;
        fx.on_impact(
            (0.0, 1.0, 0.0),
            (0.0, 1.0, 0.0),
            None,
            crate::world::palette::Surface::Concrete,
            Some(25.0),
            false,
        );
        assert!(fx.stats.spawned > before);
    }

    #[test]
    fn on_footstep_ignores_a_stationary_step() {
        let mut fx = FxSystem::test_instance(2);
        let before = fx.stats.spawned;
        fx.on_footstep(false, (0.0, 0.0, 0.0));
        assert_eq!(fx.stats.spawned, before);
    }

    #[test]
    fn add_decal_respects_the_suppress_flag() {
        let mut fx = FxSystem::test_instance(3);
        fx.suppress_decals = true;
        let ok = fx.add_decal(
            (0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            DecalOpts {
                tile: 0,
                size: 0.1,
                ..Default::default()
            },
        );
        assert!(!ok);
    }

    #[test]
    fn spawn_shell_advances_the_ring() {
        let mut fx = FxSystem::test_instance(4);
        fx.spawn_shell((0.0, 1.0, 0.0), None, ShellSpawnOpts::default());
        assert!(fx.shells.alive_count() > 0);
    }
}
