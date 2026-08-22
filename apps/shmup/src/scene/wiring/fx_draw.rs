//! **The FX render seam** — what turns [`FxSystem`]'s particles, decals,
//! shells and pooled lights into engine nodes.
//!
//! [`crate::scene::wiring::fx_audio`] constructs and steps the FX system, and
//! its own module doc says outright what was missing: *"Particles, decals,
//! tracers and haze need additive blending and camera-facing quads … Building
//! the pass itself is not this file's job and is not attempted here."* This is
//! that file. 8,886 lines of ported FX had, until now, exactly one consumer —
//! [`FxAudio::particle_points`] — and nothing consumed *that*.
//!
//! Nothing here re-implements FX. Every number drawn comes out of the ported
//! simulation: [`crate::fx::particles::integrate`] (the port of
//! `PARTICLE_VERT`'s `main()`), `fx.decals.placements()`, `fx.shells.slots`,
//! `fx.lights.slots`. Tracers, explosions and the muzzle flash need no code of
//! their own here: `fx/tracers.rs`, `fx/explosions.rs` and `fx/muzzle.rs` are
//! *particle recipes* — they emit into the same five layers everything else
//! does — so drawing the layers draws them.
//!
//! # The shape the engine forces
//!
//! Three engine facts decide everything below, and all three were checked
//! against the engine rather than assumed:
//!
//! 1. **There is no billboard primitive and no per-frame mesh geometry
//!    update.** A mesh is uploaded once, at registration. So a sprite is a
//!    pre-spawned quad node whose `Transform` is rewritten each frame with a
//!    CPU-computed camera-facing rotation.
//! 2. **`Transform`, `Bounds` and `Visible` are the only `Component`s** (the
//!    only `impl Component for` in `modules/axiom`). In particular a node's
//!    **material cannot be changed after it is spawned** — `Renderable` is not
//!    a component, and there is no per-node colour lane in `axiom-scene`. A
//!    particle's colour and alpha therefore have to be chosen from a **fixed
//!    palette of materials decided at install**, and the pool is *partitioned*
//!    by that palette. That single fact is why this file is a grid of cells and
//!    not one flat array.
//! 3. **Alpha comes out as `albedo.a * material.opacity`.** The main pass reads
//!    `material_alpha = in.color.w` (`base_color.w * opacity`) and, when that is
//!    `< 1`, takes `surface.opacity` — which the default surface program
//!    defines as `in.albedo.w`, the sampled texture's alpha times the material
//!    alpha (`scene_wgsl.rs:626-652`, `wgsl_template.rs`'s
//!    `out.opacity = in.albedo.w`). So a **soft, round** particle is reachable:
//!    bind the baked atlas tile (whose alpha channel is the painter's coverage)
//!    and give the material an opacity below one. An *opaque* material
//!    deliberately ignores the map's alpha, so `with_opacity(1.0)` would render
//!    every sprite as a hard square.
//!
//! # What this deliberately does not use
//!
//! * **A runtime `Surface` with `alpha_mask: true`.** That path also multiplies
//!   the albedo alpha in, and it looks like the tidier answer. It is a trap:
//!   `SurfaceKind::code()` excludes `MaterialParams` from the digest, and the
//!   program cache is keyed on that digest
//!   (`modules/axiom-gpu-backend/src/surface_program/cache.rs`), so **every
//!   runtime material in the process shares one compiled program and one
//!   parameter block**. Authoring a particle surface would race the street's 46
//!   parameter sets for the same block. The built-in fixed-material path has no
//!   such coupling. (That collision is a pre-existing engine defect this slice
//!   found and did not fix — see the note.)
//! * **`CAP_ALPHAMASK`.** It is a *frame* capability, not a material flag, and
//!   it `discard`s any texel under `albedo.a < 0.5` on **every** textured draw.
//!   The street's bake packs its height field in albedo alpha, so switching it
//!   on would punch holes in the whole level.
//! * **Additive blending.** The GPU backend has an additive `BlendState` and no
//!   call site reaches it. Additive layers are drawn as *emissive*
//!   alpha-blended quads instead: `emissive` is a real per-instance lane
//!   (`axiom_host::FrameDrawItem::with_emissive`, carried through
//!   `frame_packet_adapter`'s `emissive(3) + specular(1)`), so a spark is a
//!   self-illuminating sprite that blooms rather than one that literally adds.
//!
//! # Budgets, and what happens when they are exceeded
//!
//! Stated here because a silent drop is a finding, not an implementation
//! detail. See [`PARTICLE_CLASSES`], [`DECAL_CLASSES`], [`PARTICLE_ALPHAS`] and
//! [`DECAL_ALPHAS`] for the exact numbers, and [`FxDrawReport`] for what a frame
//! reports.
//!
//! | pool | nodes | materials |
//! |---|---|---|
//! | particles | 292 per alpha tier x 4 tiers = **1,168** | 36 |
//! | decals    | 88 per alpha tier x 3 tiers = **264**   | 18 |
//! | shells    | [`crate::fx::shells::CAPACITY`] = **14** | 1 |
//! | flash lights | [`FX_LIGHT_TIERS`] = **3** | — (a `PointLight` bundle) |
//!
//! The FX system's own capacity at the `Ultra` preset is ~23,000 particle slots
//! and 512 decal slots, so the draw pool is roughly **5% of the simulation**.
//! That is the point: the simulation is a ring buffer sized for a worst case, a
//! frame draws what is alive, and alive is normally a few hundred.
//!
//! **When a cell is full the sprite is dropped for that frame** — not from the
//! simulation, which keeps integrating it, and not permanently. Three
//! properties make that honest rather than lossy:
//!
//! * **It is counted.** [`FxDrawReport::dropped_particles`] /
//!   [`FxDrawReport::dropped_decals`] are the per-frame drop counts, and
//!   [`FxDrawReport::peak_cell_pressure`] is the fullest any cell got. A frame
//!   that drops is a frame that says so.
//! * **It does not flicker.** Cells fill in a fixed order — layer order, then
//!   ascending slot index — so the same sprites win the same cells every frame.
//!   An overfull cell loses a *stable* tail, not a random one.
//! * **It degrades where it is least visible.** The tail of a cell is the
//!   highest-numbered live ring slots, which are the *most recently emitted*
//!   particles of that class — the ones that have had the least time on screen.
//!
//! # Fidelity this cannot reach, stated rather than faked
//!
//! * **Colour is quantised.** Nine appearance classes x four alpha tiers. A
//!   particle whose spawn colour is far from its class tint is drawn in the
//!   class tint. The engine gives an app no per-node colour, so the only way to
//!   do better is more materials, i.e. more draw calls and a more fragmented
//!   pool.
//! * **Velocity stretch is dropped.** `ParticleSpawn::stretch` is a
//!   *screen-space* smear the source applies to the quad's corners in clip
//!   space; there are no clip-space corners here.
//! * **Decals are flat.** [`crate::fx::decals::DecalPlacement`]'s doc has the
//!   whole argument: the clipped triangle soup cannot be uploaded per frame, so
//!   what is drawn is the projector's own face quad — exactly the fallback the
//!   source lays down when the BVH is empty under an impact. Decals will not
//!   wrap a corner.
//! * **A flash light cannot decay smoothly.** `PointLight` is a `Bundle`, not a
//!   `Component`, so intensity is fixed at spawn. [`FX_LIGHT_TIERS`] is three
//!   pre-spawned lights at three fixed intensities and the frame lights the one
//!   nearest the brightest live pooled slot, parking the rest below the world.
//!   That is a three-step ramp for **one** flash at a time, not four smooth
//!   ones.
//! * **The frame's punctual light budget is 16 including the sun.** These three
//!   have to come out of `install_practicals`' fifteen — see the report.

use axiom::prelude::*;
use axiom_math::Quat;

use crate::fx::atlas::{d, p};
use crate::fx::particles::{self, ParticleLayer};
use crate::fx::shells;
use crate::fx::system::FxSystem;
use crate::scene::game::CameraPose;

/* ==================================================================== */
/* The palette — what the pool is partitioned by                        */
/* ==================================================================== */

/// One drawn particle appearance: an atlas tile, a tint, whether it is
/// self-illuminating, and how many nodes each alpha tier gets.
///
/// The tint is linear RGB, in the engine's units. For an emissive class it is
/// also the emissive radiance and may exceed `1.0` — the scene target is
/// `Rgba16Float` under `FrameTonemap::filmic()` (`scene::app::shmup_start`), so
/// an over-one value is a real HDR value that reaches the bloom, not a clip.
#[derive(Debug, Clone, Copy)]
pub struct ParticleClass {
    /// The [`crate::fx::atlas::p`] tile whose RGBA is uploaded for this class.
    pub tile: usize,
    /// Linear RGB tint (and, when `emissive`, the emissive radiance).
    pub tint: [f32; 3],
    /// `true` for what the source draws additively.
    pub emissive: bool,
    /// Nodes this class gets **per alpha tier**.
    pub per_tier: usize,
}

/// The nine drawn particle classes.
///
/// The atlas has sixteen tiles; nine are uploaded and the other seven fold onto
/// the nearest cousin via [`particle_class_of`]. Nine rather than sixteen
/// because every extra class costs a whole column of the pool (four alpha
/// tiers' worth of nodes) and one more potential draw call, and the seven that
/// fold are near-duplicates of the ones they fold into (`SMOKE_B` and `WISP`
/// are both grey soft blobs; `FLASH_LOBE` and `FLASH_CORE` are both white-hot).
///
/// `per_tier` is weighted by how many of each class a firefight actually has
/// live at once — smoke lingers for seconds and is the biggest column; a muzzle
/// flash is six frames of a dozen sprites and is the smallest.
pub const PARTICLE_CLASSES: [ParticleClass; 9] = [
    // Smoke: grey, soft, long-lived, the largest population.
    ParticleClass { tile: p::SMOKE_A, tint: [0.34, 0.34, 0.36], emissive: false, per_tier: 64 },
    // Dust / concrete puff / splash.
    ParticleClass { tile: p::DUST, tint: [0.52, 0.45, 0.34], emissive: false, per_tier: 48 },
    // Debris chips and splinters.
    ParticleClass { tile: p::CHIP, tint: [0.20, 0.19, 0.18], emissive: false, per_tier: 24 },
    // Blood droplets and mist.
    ParticleClass { tile: p::DROPLET, tint: [0.36, 0.045, 0.035], emissive: false, per_tier: 24 },
    // Sparks and impact rings — hot, additive.
    ParticleClass { tile: p::SPARK, tint: [2.40, 1.30, 0.42], emissive: true, per_tier: 48 },
    // Tracer streaks. `tracers.rs` emits three sprites per round.
    ParticleClass { tile: p::STREAK, tint: [2.20, 0.95, 0.30], emissive: true, per_tier: 16 },
    // Muzzle-flash lobes and core — the brightest thing in the frame.
    ParticleClass { tile: p::FLASH_CORE, tint: [4.00, 3.40, 2.60], emissive: true, per_tier: 12 },
    // Fire, from `explosions.rs`.
    ParticleClass { tile: p::FIRE, tint: [2.00, 0.82, 0.24], emissive: true, per_tier: 24 },
    // Ambient motes — `ambience.rs`, the only class that is always populated.
    ParticleClass { tile: p::MOTE, tint: [0.72, 0.74, 0.80], emissive: true, per_tier: 32 },
];

/// Which [`PARTICLE_CLASSES`] entry an atlas tile is drawn as, indexed by the
/// [`crate::fx::atlas::p`] tile constant.
///
/// A table and not a `match` so the fold is visible as data: the seven entries
/// that do not point at their own class are exactly the seven folded tiles
/// named in [`PARTICLE_CLASSES`]' doc.
const PARTICLE_TILE_CLASS: [usize; 16] = [
    0, // SMOKE_A    -> smoke
    0, // SMOKE_B    -> smoke
    0, // WISP       -> smoke
    1, // DUST       -> dust
    4, // SPARK      -> spark
    5, // STREAK     -> streak
    6, // FLASH_LOBE -> flash
    6, // FLASH_CORE -> flash
    2, // CHIP       -> debris
    2, // SPLINTER   -> debris
    3, // DROPLET    -> blood
    3, // MIST       -> blood
    1, // SPLASH     -> dust
    4, // RING       -> spark
    7, // FIRE       -> fire
    8, // MOTE       -> mote
];

/// The four particle opacity tiers a material can carry.
///
/// Biased low because particles are mostly translucent: an even 0.25/0.5/0.75/1
/// split would spend half the palette on opacities the FX system almost never
/// asks for. `0.94` rather than `1.0` because an opacity of exactly one makes
/// the material *opaque*, and an opaque material ignores its albedo map's
/// alpha — which would render every sprite as a hard square (see the module
/// doc).
pub const PARTICLE_ALPHAS: [f64; 4] = [0.16, 0.38, 0.64, 0.94];

/// A sprite fainter than this is not drawn at all. Half the lowest tier: below
/// it, quantising *up* to `PARTICLE_ALPHAS[0]` would make a dying particle
/// brighter than it is, and quantising down is zero anyway.
const PARTICLE_ALPHA_FLOOR: f64 = 0.08;

/// One drawn decal appearance. Same shape as [`ParticleClass`]; decals are
/// never emissive, so there is no flag.
#[derive(Debug, Clone, Copy)]
pub struct DecalClass {
    /// The [`crate::fx::atlas::d`] tile whose RGBA is uploaded.
    pub tile: usize,
    /// Linear RGB tint.
    pub tint: [f32; 3],
    /// Nodes this class gets **per alpha tier**.
    pub per_tier: usize,
}

/// The six drawn decal classes; the other ten decal tiles fold onto them via
/// [`decal_class_of`].
pub const DECAL_CLASSES: [DecalClass; 6] = [
    DecalClass { tile: d::HOLE_CONCRETE, tint: [0.10, 0.10, 0.10], per_tier: 24 },
    DecalClass { tile: d::HOLE_METAL, tint: [0.14, 0.13, 0.12], per_tier: 12 },
    DecalClass { tile: d::BLOOD_A, tint: [0.24, 0.02, 0.02], per_tier: 16 },
    DecalClass { tile: d::SCORCH, tint: [0.05, 0.05, 0.05], per_tier: 12 },
    DecalClass { tile: d::IMPACT_DIRT, tint: [0.26, 0.21, 0.15], per_tier: 12 },
    DecalClass { tile: d::SCRAPE, tint: [0.30, 0.29, 0.28], per_tier: 12 },
];

/// Which [`DECAL_CLASSES`] entry a decal-atlas tile is drawn as.
const DECAL_TILE_CLASS: [usize; 16] = [
    0, // HOLE_CONCRETE
    0, // HOLE_CONCRETE_B
    1, // HOLE_METAL
    0, // HOLE_WOOD     -> concrete hole
    0, // HOLE_PLASTER  -> concrete hole
    1, // GLASS_CRACK   -> metal hole
    2, // BLOOD_A
    2, // BLOOD_B
    3, // SCORCH
    4, // IMPACT_DIRT
    4, // IMPACT_SAND   -> dirt
    5, // SCRAPE
    5, // RIPPLE        -> scrape
    1, // HOLE_GLASS    -> metal hole
    5, // SMUDGE        -> scrape
    5, // TEAR          -> scrape
];

/// The three decal opacity tiers. Fewer than the particle palette because a
/// decal sits at its authored opacity for most of its life and only ramps at
/// the very end, so the tiers are doing less work.
pub const DECAL_ALPHAS: [f64; 3] = [0.30, 0.62, 0.92];

/// A decal fainter than this is not drawn.
const DECAL_ALPHA_FLOOR: f64 = 0.15;

/// The three fixed intensities a pooled flash light can take, in **engine**
/// units (see [`fit_point_intensity`]).
///
/// Three pre-spawned lights, not one: `PointLight` is a `Bundle`, so intensity
/// is frozen at spawn and the only way to vary it is to have more than one
/// light and pick. The ramp is geometric, and covers the two flashes the FX
/// system actually raises — a rifle muzzle flash peaks at ~4.7 engine units
/// (`muzzle.rs`'s `prof.light * gain * 0.18` at `distance = 5`), and an
/// explosion at ~70, which clamps to the top tier.
pub const FX_LIGHT_TIERS: [f32; 3] = [1.2, 4.0, 12.0];

/// Where a parked flash light is put. Far enough below the street that the
/// engine's fixed `1/(1 + 0.09d + 0.032d^2)` falloff — which never reaches zero
/// — contributes under `10^-6` of its peak at ground level.
const LIGHT_PARK_Y: f32 = -1000.0;

/// The distance `install_practicals` fits a `three` point light's windowed
/// `1/d^2` against the engine's fixed curve at. Kept identical, deliberately:
/// two different fits would make a muzzle flash and a street lamp disagree
/// about what "intensity 5" means.
const FIT_DISTANCE: f64 = 3.0;

/// A source-side `three` `PointLight` intensity, re-fitted into the engine's
/// point-light units.
///
/// The engine's point light carries neither `distance` nor `decay`; the main
/// pass attenuates every one by a fixed `1/(1 + 0.09d + 0.032d^2)` that never
/// reaches zero, while `three` uses `1/d^2` windowed to zero at `distance`. The
/// two curves cannot be reconciled by choosing an intensity, so they are made
/// to agree at one distance — a room's own scale.
///
/// This is `scene::app::install_practicals`' arithmetic, hoisted so the FX
/// flash lights and the world's practicals cannot drift apart. `app.rs` should
/// call it too; see the note.
pub fn fit_point_intensity(source_intensity: f64, distance: f64) -> f32 {
    let engine_falloff = 1.0 / (1.0 + 0.09 * FIT_DISTANCE + 0.032 * FIT_DISTANCE * FIT_DISTANCE);
    let window = (1.0 - (FIT_DISTANCE / distance.max(1e-3)).powi(4)).max(0.0);
    let three = source_intensity / (FIT_DISTANCE * FIT_DISTANCE) * window * window;
    (three / engine_falloff) as f32
}

/* ==================================================================== */
/* The report                                                           */
/* ==================================================================== */

/// What one drawn frame did, and what it could not fit.
///
/// Every count here is per frame. `dropped_*` being non-zero is not an error —
/// it is the pool doing its job — but a frame that drops steadily is a frame
/// asking for a bigger `per_tier` on some class, and this is how that gets
/// noticed instead of guessed at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FxDrawReport {
    /// Particle sprites that reached a node this frame.
    pub particles_drawn: usize,
    /// Particle sprites the pool had no cell room for. See the module doc's
    /// budget section for what "dropped" means and does not mean.
    pub dropped_particles: usize,
    /// Decal quads drawn.
    pub decals_drawn: usize,
    /// Decals the pool had no cell room for.
    pub dropped_decals: usize,
    /// Brass casings drawn (never dropped — the pool is
    /// [`crate::fx::shells::CAPACITY`], the simulation's own capacity).
    pub shells_drawn: usize,
    /// Flash lights lit this frame: `0` or `1`, because the three light nodes
    /// are three *intensities* of one flash, not three flashes.
    pub lights_lit: usize,
    /// The fullest any one cell got, as a fraction of its capacity, times 1000.
    /// An integer so the report stays `Eq`. `1000` means at least one cell ran
    /// out.
    pub peak_cell_pressure: u32,
}

/* ==================================================================== */
/* The pool                                                             */
/* ==================================================================== */

/// One material's worth of pre-spawned quads.
struct Cell {
    nodes: Vec<Entity>,
    /// How many were claimed this frame. Reset at the top of [`FxDraw::frame`].
    used: usize,
    /// How many were visible at the end of the *previous* frame, so the hide
    /// pass only touches the nodes that actually need hiding rather than
    /// writing `Visible` to every node in the pool every frame.
    shown: usize,
}

impl Cell {
    fn claim(&mut self) -> Option<Entity> {
        let node = self.nodes.get(self.used).copied();
        self.used += usize::from(node.is_some());
        node
    }
}

/// The FX draw pool: the quads, the brass, the flash lights, and the frame that
/// drives them.
///
/// Built once, inside `App::install` — **not** afterwards. The live windowing
/// backend sizes its vertex and instance buffers once at startup from
/// `RunningApp::mesh_set()` / `renderable_count()`
/// (`app/authoring.rs`'s module doc says so outright), so a node spawned after
/// `build()` exists for the deterministic headless path and is invisible in the
/// browser.
pub struct FxDraw {
    /// `PARTICLE_CLASSES.len() * PARTICLE_ALPHAS.len()` cells, class-major.
    particles: Vec<Cell>,
    /// `DECAL_CLASSES.len() * DECAL_ALPHAS.len()` cells, class-major.
    decals: Vec<Cell>,
    /// One node per [`crate::fx::shells::ShellSlot`].
    shells: Vec<Entity>,
    /// One node per [`FX_LIGHT_TIERS`] entry.
    lights: Vec<Entity>,
}

/// The atlas tiles the pool uploads, cut out of the FX system's two bakes.
///
/// A separate step from [`FxDraw::install`] for a borrow reason that is real,
/// not cosmetic: `App::install` takes a `move` closure, and the `Game` that owns
/// the FX system is still needed after `build()` returns. So the pixels the pool
/// needs are lifted out **before** the closure is authored, and the closure owns
/// those and nothing else. Exactly the shape `scene::app::build` already uses for
/// the level batches and the practicals (`std::mem::take`).
///
/// Only the fifteen tiles [`PARTICLE_CLASSES`] and [`DECAL_CLASSES`] name are
/// cut, not the whole 1024² atlases, so this holds ~3.9 MB rather than ~8 MB.
pub struct FxAtlasTiles {
    /// `(side, rgba8)` per [`PARTICLE_CLASSES`] entry, in order.
    particle: Vec<(u32, Vec<u8>)>,
    /// `(side, rgba8)` per [`DECAL_CLASSES`] entry, in order.
    decal: Vec<(u32, Vec<u8>)>,
}

impl FxAtlasTiles {
    /// Cut the tiles out of a constructed [`FxSystem`]'s baked atlases. Call
    /// before authoring the `App::install` closure.
    pub fn of(fx: &FxSystem) -> FxAtlasTiles {
        FxAtlasTiles {
            particle: PARTICLE_CLASSES
                .iter()
                .map(|c| cut_tile(&fx.atlas.data, fx.atlas.size, fx.atlas.cols, c.tile))
                .collect(),
            decal: DECAL_CLASSES
                .iter()
                .map(|c| {
                    cut_tile(
                        &fx.decal_atlas.albedo,
                        fx.decal_atlas.size,
                        fx.decal_atlas.cols,
                        c.tile,
                    )
                })
                .collect(),
        }
    }
}

impl FxDraw {
    /// Register the meshes, textures and materials, and spawn every pooled
    /// node. Call from inside `App::install`.
    ///
    /// The pool's size is a property of this file, not of the FX system's
    /// capacities — see the module doc's budget table.
    /// Every renderable node the pool holds — particle cells, decal cells and
    /// the brass ring, but not the three flash lights (a point light is not a
    /// renderable).
    ///
    /// The pool spawns its whole budget at install and hides what a frame does
    /// not need, so a hidden slot is *installed and not drawn by design*. Any
    /// invariant relating installed nodes to drawn ones has to subtract this or
    /// it is measuring the pool rather than the scene.
    pub fn pool_len(&self) -> usize {
        let cells = |cs: &Vec<Cell>| cs.iter().map(|c| c.nodes.len()).sum::<usize>();
        cells(&self.particles) + cells(&self.decals) + self.shells.len()
    }

    pub fn install(app: &mut RunningApp, tiles: &FxAtlasTiles) -> FxDraw {
        let quad = app
            .add_mesh_data(unit_quad())
            .expect("a unit quad is valid renderable geometry");

        // Plain loops, not iterator chains: `app` is borrowed mutably by every
        // step (upload, register, spawn) and two chained closures cannot each
        // hold that borrow. Apps sit outside the Branchless Law, and
        // `scene::app::install_level` sets the same precedent.
        let mut particles = Vec::with_capacity(PARTICLE_CLASSES.len() * PARTICLE_ALPHAS.len());
        for (index, class) in PARTICLE_CLASSES.into_iter().enumerate() {
            let texture = upload_tile(app, &tiles.particle[index]);
            for alpha in PARTICLE_ALPHAS {
                // An emissive class is drawn BLACK-albedo + emissive: its light
                // is self-illumination, not reflectance, so a lamp shining on a
                // spark must not brighten it and a shadow must not dim it. A lit
                // class is the opposite — its tint IS a reflectance.
                let tint = tint_color(class.tint);
                let base = if class.emissive { Color::BLACK } else { tint };
                let mut material = Material::lit(base)
                    .with_custom_texture(texture)
                    .with_texture_sampling(TextureSampling::Anisotropic)
                    .with_opacity(Ratio::finite_or_zero(alpha as f32));
                if class.emissive {
                    material = material.with_emissive(tint);
                }
                let handle = app.add_material(material);
                particles.push(spawn_cell(app, quad, handle, class.per_tier));
            }
        }

        let mut decals = Vec::with_capacity(DECAL_CLASSES.len() * DECAL_ALPHAS.len());
        for (index, class) in DECAL_CLASSES.into_iter().enumerate() {
            let texture = upload_tile(app, &tiles.decal[index]);
            for alpha in DECAL_ALPHAS {
                let material = Material::lit(tint_color(class.tint))
                    .with_custom_texture(texture)
                    .with_texture_sampling(TextureSampling::Anisotropic)
                    .with_opacity(Ratio::finite_or_zero(alpha as f32));
                let handle = app.add_material(material);
                decals.push(spawn_cell(app, quad, handle, class.per_tier));
            }
        }

        // Brass. A real solid, not a sprite: a casing tumbles, and a
        // camera-facing quad cannot tumble. `Mesh::cylinder` is the engine's
        // own primitive; the case profile (`shells.js:26-40`'s lathe) is GPU
        // presentation the port explicitly did not bring over, and inventing a
        // lathe here would be porting, not wiring.
        let brass = app.add_material(
            Material::lit(tint_color([0.62, 0.44, 0.16]))
                .with_roughness(Ratio::finite_or_zero(0.35))
                .with_metallic(Ratio::finite_or_zero(1.0)),
        );
        let casing = app.add_mesh(Mesh::cylinder());
        let shells = (0..shells::CAPACITY)
            .map(|_| {
                let node = app.spawn(Spawn::new(Transform::IDENTITY, casing, brass));
                app.set(node, Visible(false));
                node
            })
            .collect();

        // The flash lights. Parked at spawn; a frame raises at most one.
        let lights = FX_LIGHT_TIERS
            .iter()
            .map(|intensity| {
                app.add_point_light(
                    PointLight {
                        // A muzzle flash and an explosion are both warm-white.
                        // The pooled slot's own `(r, g, b)` cannot be honoured —
                        // a `PointLight`'s colour is as frozen as its intensity.
                        color: tint_color([1.0, 0.86, 0.62]),
                        intensity: Ratio::finite_or_zero(*intensity),
                    },
                    Transform::from_translation(Vec3::new(0.0, LIGHT_PARK_Y, 0.0)),
                )
            })
            .collect();

        FxDraw {
            particles,
            decals,
            shells,
            lights,
        }
    }

    /// Draw one frame's FX. Call after the game has stepped and after
    /// `write_camera`, with the same `pose` the camera was written from and the
    /// same `now` the FX system was stepped at (`game.time.elapsed`).
    pub fn frame(
        &mut self,
        app: &mut RunningApp,
        fx: &FxSystem,
        pose: CameraPose,
        now: f64,
    ) -> FxDrawReport {
        let eye = Vec3::new(pose.eye[0] as f32, pose.eye[1] as f32, pose.eye[2] as f32);
        let camera = camera_rotation(pose);

        self.particles.iter_mut().for_each(|cell| cell.used = 0);
        self.decals.iter_mut().for_each(|cell| cell.used = 0);

        let mut report = FxDrawReport::default();
        self.draw_particles(app, fx, eye, camera, now, &mut report);
        self.draw_decals(app, fx, now, &mut report);
        self.draw_shells(app, fx, &mut report);
        self.draw_lights(app, fx, &mut report);
        self.hide_unused(app);
        report.peak_cell_pressure = self
            .particles
            .iter()
            .chain(self.decals.iter())
            .map(|c| (c.used * 1000 / c.nodes.len().max(1)) as u32)
            .max()
            .unwrap_or(0);
        report
    }

    /// The three world-space layers plus the two view-space ones.
    ///
    /// The view layers are drawn, and that matters: `muzzle.rs` emits the whole
    /// muzzle flash through `emit_add_view(view, ..)` with `view = true`
    /// whenever the shot came from within 1.5 m of the camera
    /// (`system.rs:835`), which is every shot the player fires. Skipping them —
    /// which `FxAudio::particle_points` does, on the grounds that "nothing
    /// attaches the view scene" — means skipping the muzzle flash. This port
    /// composes the viewmodel in world space
    /// (`scene::app::drive_viewmodel`), so the view camera *is* the camera and a
    /// view-space point becomes a world-space one by the camera's own
    /// transform.
    fn draw_particles(
        &mut self,
        app: &mut RunningApp,
        fx: &FxSystem,
        eye: Vec3,
        camera: Quat,
        now: f64,
        report: &mut FxDrawReport,
    ) {
        let layers: [(&ParticleLayer, bool); 5] = [
            (&fx.lit, false),
            (&fx.add, false),
            (&fx.motes, false),
            (&fx.view_lit, true),
            (&fx.view_add, true),
        ];
        for (layer, view_space) in layers {
            for slot in 0..layer.capacity {
                let Some(sample) = particles::integrate(layer, slot, now) else {
                    continue;
                };
                if sample.alpha < PARTICLE_ALPHA_FLOOR {
                    continue;
                }
                let class = particle_class_of(layer.tile_at(slot));
                let tier = nearest(&PARTICLE_ALPHAS, sample.alpha);
                let cell = &mut self.particles[class * PARTICLE_ALPHAS.len() + tier];
                let Some(node) = cell.claim() else {
                    report.dropped_particles += 1;
                    continue;
                };

                let local = Vec3::new(sample.pos.0 as f32, sample.pos.1 as f32, sample.pos.2 as f32);
                // A view-layer particle is authored in camera-local space.
                let world = [local, eye.add(camera.rotate(local))][usize::from(view_space)];
                // The billboard is view-plane aligned — the camera's own
                // rotation, not a per-sprite look-at. That is what a point
                // sprite is, it is one quaternion for the whole frame, and it
                // has no degenerate case when a particle passes through the eye.
                let roll = Quat::from_axis_angle(Vec3::UNIT_Z, layer.roll_at(slot, now) as f32)
                    .unwrap_or(Quat::IDENTITY);
                // `uPScale`: the quality preset's sprite scale (`index.js:66`).
                // `size` is the sprite's half-extent, and the quad is a unit
                // square, so the scale is twice it.
                let extent = (sample.size * fx.pscale * 2.0) as f32;
                app.set(
                    node,
                    Transform::new(
                        world,
                        camera.multiply(roll),
                        Vec3::new(extent, extent, extent),
                    ),
                );
                app.set(node, Visible(true));
                report.particles_drawn += 1;
            }
        }
    }

    /// One oriented quad per live decal placement.
    fn draw_decals(
        &mut self,
        app: &mut RunningApp,
        fx: &FxSystem,
        now: f64,
        report: &mut FxDrawReport,
    ) {
        for placement in fx.decals.placements() {
            if !placement.occupied || !placement.wrote {
                continue;
            }
            let age = (now - placement.birth) / placement.life;
            if !(0.0..1.0).contains(&age) {
                continue;
            }
            // The source's decal fade lives in a GLSL string the port did not
            // bring over; what it DID bring over is the four lanes the shader
            // reads — `birth, 1/life, fade, opacity`. The reading here is the
            // only one those four names admit: hold `opacity` until the
            // normalised age reaches `fade`, then ramp linearly to zero at the
            // end of life. If the shader turns out to curve that ramp, this is
            // the one line that changes.
            let hold = placement.fade.clamp(0.0, 0.999);
            let alpha = placement.opacity * ((1.0 - age) / (1.0 - hold)).min(1.0).max(0.0);
            if alpha < DECAL_ALPHA_FLOOR {
                continue;
            }

            let class = decal_class_of(placement.tile);
            let tier = nearest(&DECAL_ALPHAS, alpha);
            let cell = &mut self.decals[class * DECAL_ALPHAS.len() + tier];
            let Some(node) = cell.claim() else {
                report.dropped_decals += 1;
                continue;
            };

            let n = vec3_of(placement.normal);
            let b = vec3_of(placement.bitangent);
            // `look_rotation(f, up)` maps local `+Z -> -f` and `+X -> f x up`
            // normalised. With `f = -n` and `up = b` that is exactly
            // `(+X -> tangent, +Y -> bitangent, +Z -> normal)` — the projector's
            // own basis, so the quad's UVs land the way `write_uv` wrote them.
            let Ok(rotation) = Quat::look_rotation(n.mul_scalar(-1.0), b) else {
                // A degenerate basis means `add` was handed a zero normal; it
                // wrote no usable geometry either.
                report.dropped_decals += 1;
                continue;
            };
            // Off the surface by the same lift `add`'s own fallback quad uses
            // (`decals.js:336`), so the decal does not z-fight the wall.
            let lift = 0.004 + placement.half_size * 2.0 * 0.01;
            let centre = vec3_of(placement.point).add(n.mul_scalar(lift as f32));
            let extent = (placement.half_size * 2.0) as f32;
            app.set(
                node,
                Transform::new(centre, rotation, Vec3::new(extent, extent, extent)),
            );
            app.set(node, Visible(true));
            report.decals_drawn += 1;
        }
    }

    /// Brass. One node per simulation slot, so nothing is ever dropped.
    fn draw_shells(&mut self, app: &mut RunningApp, fx: &FxSystem, report: &mut FxDrawReport) {
        for (node, slot) in self.shells.iter().zip(fx.shells.slots.iter()) {
            app.set(*node, Visible(slot.alive));
            if !slot.alive {
                continue;
            }
            // `ShellSlot::scale` is a multiplier on `CASE_LEN`, and
            // `Mesh::cylinder` is the unit primitive it scales. The casing is
            // slim: `ShellPayload::case_radius` defaults to 4.95 mm against a
            // 44.6 mm case, so the girth is the length times 0.22.
            let length = (slot.scale * shells::CASE_LEN) as f32;
            let girth = length * 0.22;
            app.set(
                *node,
                Transform::new(
                    Vec3::new(slot.pos.0 as f32, slot.pos.1 as f32, slot.pos.2 as f32),
                    slot.quat,
                    Vec3::new(girth, length, girth),
                ),
            );
            report.shells_drawn += 1;
        }
    }

    /// The flash light: at most one, at the nearest of [`FX_LIGHT_TIERS`].
    fn draw_lights(&mut self, app: &mut RunningApp, fx: &FxSystem, report: &mut FxDrawReport) {
        let brightest = fx
            .lights
            .slots
            .iter()
            .filter(|s| s.intensity > 0.0)
            .map(|s| (fit_point_intensity(s.intensity, s.distance), s))
            .max_by(|a, b| a.0.total_cmp(&b.0));

        let chosen = brightest
            .filter(|(fitted, _)| *fitted >= FX_LIGHT_TIERS[0] * 0.5)
            .map(|(fitted, slot)| (nearest_f32(&FX_LIGHT_TIERS, fitted), slot));

        for (tier, node) in self.lights.iter().enumerate() {
            let at = chosen
                .filter(|(lit, _)| *lit == tier)
                .map(|(_, slot)| Vec3::new(slot.x as f32, slot.y as f32, slot.z as f32))
                .unwrap_or(Vec3::new(0.0, LIGHT_PARK_Y, 0.0));
            app.set(*node, Transform::from_translation(at));
        }
        report.lights_lit = usize::from(chosen.is_some());
    }

    /// Retire every node a cell did not claim this frame.
    ///
    /// Only the nodes between this frame's high-water mark and the previous
    /// one's are touched, so an idle pool costs nothing: a frame that draws 40
    /// smoke sprites out of 64 writes 24 `Visible(false)`s the first time and
    /// zero on every frame after that holds at 40.
    fn hide_unused(&mut self, app: &mut RunningApp) {
        for cell in self.particles.iter_mut().chain(self.decals.iter_mut()) {
            for node in &cell.nodes[cell.used..cell.shown.max(cell.used)] {
                app.set(*node, Visible(false));
            }
            cell.shown = cell.used;
        }
    }
}

/* ==================================================================== */
/* Helpers                                                              */
/* ==================================================================== */

/// The unit sprite quad: 1 m square in the XY plane, normal `+Z`, UVs spanning
/// the whole texture with `v` increasing upward (so a tile's `y = +1` row, which
/// the bake writes last, lands at the top of the quad).
fn unit_quad() -> MeshData {
    MeshData::new(
        vec![
            Vec3::new(-0.5, -0.5, 0.0),
            Vec3::new(0.5, -0.5, 0.0),
            Vec3::new(0.5, 0.5, 0.0),
            Vec3::new(-0.5, 0.5, 0.0),
        ],
        vec![Vec3::new(0.0, 0.0, 1.0); 4],
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ],
        vec![0, 1, 2, 0, 2, 3],
    )
}

/// Cut one `side x side` tile out of a baked `cols x cols` atlas.
fn cut_tile(data: &[u8], size: u32, cols: u32, tile: usize) -> (u32, Vec<u8>) {
    let side = size / cols;
    let ox = (tile as u32 % cols) * side;
    let oy = (tile as u32 / cols) * side;
    let row_bytes = (side * 4) as usize;
    let pixels = (0..side)
        .flat_map(|y| {
            let start = (((oy + y) * size + ox) * 4) as usize;
            data[start..start + row_bytes].iter().copied()
        })
        .collect();
    (side, pixels)
}

/// Register one cut tile as its own texture and return the id
/// [`Material::with_custom_texture`] takes.
///
/// Its own texture rather than per-tile UVs into the whole atlas, because the
/// alternative multiplies the *mesh* axis by sixteen: draw batches key on
/// `(mesh, material)`, so sixteen tile-quads times the alpha palette would be
/// sixteen times the draw calls for the same sprites. One quad and N textures
/// keeps the mesh axis at one.
fn upload_tile(app: &mut RunningApp, tile: &(u32, Vec<u8>)) -> u64 {
    app.add_texture_data(tile.0, tile.0, tile.1.clone())
        .expect("an atlas tile is side * side * 4 bytes")
        .id()
}

/// Spawn `count` hidden quads sharing one mesh and one material.
fn spawn_cell(
    app: &mut RunningApp,
    mesh: Handle<Mesh>,
    material: Handle<Material>,
    count: usize,
) -> Cell {
    let nodes = (0..count)
        .map(|_| {
            let node = app.spawn(Spawn::new(Transform::IDENTITY, mesh, material));
            app.set(node, Visible(false));
            node
        })
        .collect();
    Cell {
        nodes,
        used: 0,
        shown: 0,
    }
}

/// A linear-RGB triple as a [`Color`]. `finite_or_zero` rather than `new`
/// because these are authored constants and an HDR emissive above `1.0` is a
/// value, not an error.
fn tint_color(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(
        Ratio::finite_or_zero(rgb[0]),
        Ratio::finite_or_zero(rgb[1]),
        Ratio::finite_or_zero(rgb[2]),
    )
}

fn vec3_of(v: [f64; 3]) -> Vec3 {
    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)
}

/// The camera's rotation, composed **YXZ** — yaw, then pitch, then roll.
///
/// Identical to `scene::app::write_camera`'s composition, and it has to be: a
/// billboard built from a differently-composed camera rotation is a sprite that
/// is not facing the camera, which reads as a subtle skew rather than as an
/// obvious bug.
fn camera_rotation(pose: CameraPose) -> Quat {
    let axis = |a: Vec3, angle: f64| {
        Quat::from_axis_angle(a, angle as f32).expect("an authored camera angle is finite")
    };
    axis(Vec3::UNIT_Y, pose.rotation.yaw)
        .multiply(axis(Vec3::UNIT_X, pose.rotation.pitch))
        .multiply(axis(Vec3::UNIT_Z, pose.rotation.roll))
}

/// Which [`PARTICLE_CLASSES`] entry an integrated particle's atlas tile draws
/// as. Out-of-range tiles fold onto smoke — `tile` comes out of a `f32` lane in
/// the interleaved record, so it is a number, not an enum.
fn particle_class_of(tile: f64) -> usize {
    PARTICLE_TILE_CLASS
        .get(tile.max(0.0) as usize)
        .copied()
        .unwrap_or(0)
}

/// Which [`DECAL_CLASSES`] entry a decal tile draws as.
fn decal_class_of(tile: usize) -> usize {
    DECAL_TILE_CLASS.get(tile).copied().unwrap_or(0)
}

/// The index of the tier nearest `value`.
fn nearest(tiers: &[f64], value: f64) -> usize {
    tiers
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - value).abs().total_cmp(&(b.1 - value).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// [`nearest`], for the `f32` light ramp.
fn nearest_f32(tiers: &[f32], value: f32) -> usize {
    tiers
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - value).abs().total_cmp(&(b.1 - value).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pool's size is a stated budget, so it is pinned. A change here is a
    /// change to the module doc's table and to the app's instance count.
    #[test]
    fn the_stated_budgets_are_the_real_ones() {
        let per_tier: usize = PARTICLE_CLASSES.iter().map(|c| c.per_tier).sum();
        assert_eq!(per_tier, 292);
        assert_eq!(per_tier * PARTICLE_ALPHAS.len(), 1168);

        let decal_per_tier: usize = DECAL_CLASSES.iter().map(|c| c.per_tier).sum();
        assert_eq!(decal_per_tier, 88);
        assert_eq!(decal_per_tier * DECAL_ALPHAS.len(), 264);

        // One material per cell, and a cell is one potential draw call.
        assert_eq!(PARTICLE_CLASSES.len() * PARTICLE_ALPHAS.len(), 36);
        assert_eq!(DECAL_CLASSES.len() * DECAL_ALPHAS.len(), 18);
    }

    /// Every atlas tile folds onto a real class, and every class is reachable —
    /// a class nothing maps to is a column of the pool that can never fill.
    #[test]
    fn every_tile_folds_onto_a_reachable_class() {
        for tile in 0..16usize {
            assert!(PARTICLE_TILE_CLASS[tile] < PARTICLE_CLASSES.len());
            assert!(DECAL_TILE_CLASS[tile] < DECAL_CLASSES.len());
        }
        for class in 0..PARTICLE_CLASSES.len() {
            assert!(
                PARTICLE_TILE_CLASS.contains(&class),
                "particle class {class} is unreachable"
            );
        }
        for class in 0..DECAL_CLASSES.len() {
            assert!(
                DECAL_TILE_CLASS.contains(&class),
                "decal class {class} is unreachable"
            );
        }
    }

    /// A class's own tile must fold onto itself, or the class is drawing with
    /// somebody else's texture.
    #[test]
    fn a_class_tile_folds_onto_its_own_class() {
        for (index, class) in PARTICLE_CLASSES.iter().enumerate() {
            assert_eq!(PARTICLE_TILE_CLASS[class.tile], index);
        }
        for (index, class) in DECAL_CLASSES.iter().enumerate() {
            assert_eq!(DECAL_TILE_CLASS[class.tile], index);
        }
    }

    /// No alpha tier may be exactly `1.0`: an opaque material ignores its
    /// albedo map's alpha, which would square off every sprite.
    #[test]
    fn no_alpha_tier_is_opaque() {
        assert!(PARTICLE_ALPHAS.iter().all(|a| *a < 1.0));
        assert!(DECAL_ALPHAS.iter().all(|a| *a < 1.0));
    }

    #[test]
    fn nearest_picks_the_closest_tier() {
        assert_eq!(nearest(&PARTICLE_ALPHAS, 0.0), 0);
        assert_eq!(nearest(&PARTICLE_ALPHAS, 1.0), PARTICLE_ALPHAS.len() - 1);
        assert_eq!(nearest(&PARTICLE_ALPHAS, 0.38), 1);
        // Exactly between 0.38 and 0.64 -> the lower one wins, because `min_by`
        // keeps the first of two equal keys.
        assert_eq!(nearest(&PARTICLE_ALPHAS, 0.51), 1);
    }

    #[test]
    fn a_full_cell_hands_back_nothing_instead_of_panicking() {
        let mut cell = Cell {
            nodes: Vec::new(),
            used: 0,
            shown: 0,
        };
        assert!(cell.claim().is_none());
        assert_eq!(cell.used, 0, "a refused claim must not advance the cursor");
    }

    /// The muzzle-flash fit is the number the light tiers were chosen around;
    /// if it moves, the ramp is wrong.
    #[test]
    fn a_rifle_muzzle_flash_lands_inside_the_light_ramp() {
        // `muzzle.rs`: peak = prof.light (200) * gain (1) * 0.18, distance 5*sc.
        let fitted = fit_point_intensity(200.0 * 0.18, 5.0);
        assert!((4.0..5.5).contains(&fitted), "muzzle flash fitted to {fitted}");
        assert_eq!(nearest_f32(&FX_LIGHT_TIERS, fitted), 1);
    }

    /// An explosion is far brighter than the ramp's top and must clamp there
    /// rather than wrapping to a dim tier.
    #[test]
    fn an_explosion_clamps_to_the_brightest_tier() {
        let fitted = fit_point_intensity(420.0, 32.0);
        assert_eq!(nearest_f32(&FX_LIGHT_TIERS, fitted), FX_LIGHT_TIERS.len() - 1);
    }

    /// A practical's fit must be unchanged from `install_practicals`', which is
    /// the whole reason the arithmetic was hoisted here.
    #[test]
    fn the_practical_fit_is_the_one_app_rs_already_uses() {
        // `install_practicals`, inlined: intensity 5 at distance 13.
        let window: f64 = 1.0 - (3.0f64 / 13.0).powi(4);
        let three = 5.0 / 9.0 * window.max(0.0) * window.max(0.0);
        let expected = (three / (1.0 / (1.0 + 0.09 * 3.0 + 0.032 * 9.0))) as f32;
        assert!((fit_point_intensity(5.0, 13.0) - expected).abs() < 1e-6);
    }

    #[test]
    fn the_sprite_quad_is_a_unit_square_facing_positive_z() {
        let quad = unit_quad();
        assert_eq!(quad.positions().len(), 4);
        assert_eq!(quad.indices().len(), 6);
        assert!(quad.normals().iter().all(|n| n.z == 1.0));
        // UV v increases with local y, so a tile's last-baked row is on top.
        let uvs = quad.uvs();
        assert_eq!(uvs[0].y, 0.0);
        assert_eq!(uvs[3].y, 1.0);
    }
}
