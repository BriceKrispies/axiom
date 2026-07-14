//! VistaDirector — deterministic "Everest-scale mountain vista" scenic generation.
//! When the player picks a spot on the overworld map and descends into the
//! first-person world, the raw noise field puts them on arbitrary ground facing
//! an arbitrary direction. The [`VistaDirector`] turns that descent into a
//! *deliberate composition*: it produces a [`MountainVistaPlan`] describing a
//! flat, safe landing shelf, an enormous far-off mountain framed on the horizon,
//! a readable winding route up the mountain's side, and the atmospheric band
//! altitudes (cloud band, snow/rock/vegetation lines) that sell the scale.
//! ## Where it sits in the pipeline
//! The whole ground world flows from one pure height function in
//! [`crate::gameworld`]. The plan is *composited into that function* via
//! [`vista_height_m`], so the mountain and the flat shelf become genuinely part
//! of the world — walkable, collidable, and identical across every LOD and the
//! collision sampler — with no special-casing in the streaming machinery. The
//! director itself only *reads* the base terrain (the un-composited height) to
//! score where the composition reads best; it never mutates engine state.
//! ## Determinism
//! Everything is a pure function of the world `seed` plus the picked direction
//! (already baked into the anchored [`GameWorldLocalMap`]). The only randomness
//! is an explicit `axiom_entropy::EntropyStream` fork of the worldgen root
//! (`worldgen_stream(seed).fork(VISTA_SALT)`); there is no wall-clock, no global
//! state, no unseeded sampling. The same input always yields the same plan
//! (proved by [`tests::plan_is_deterministic`]).
//! ## Authoring convention (why the mountain sits on local −Z)
//! The engine's first-person controller always *rebuilds* the camera rotation
//! from an accumulated yaw that starts at `0` (local forward is −Z). Setting an
//! initial transform rotation would be overwritten on tick 0. Rather than fight
//! that with a one-frame snap, the director authors the mountain along the
//! camera's default forward (local −Z) from the spawn, so the player begins
//! already facing the base. The local tangent azimuth is a free authoring choice
//! anyway — the map pick fixes *where on the planet* the anchor is (and thus the
//! surrounding terrain), not which tangent heading the player looks down.

use crate::curves::{lerp3, smoothstep01};
use crate::distributions;
use crate::gameworld::sample_height_m;
use crate::model_planet::PlanetSurfaceAtlas;
use crate::model_world::GameWorldLocalMap;
use crate::seed::worldgen_stream;

/// Salt forked off the worldgen root for the vista's deterministic sub-stream, so
/// vista selection never shares a sequence with worldgen or chunk detail.
const VISTA_SALT: u64 = 0x5713_A115_7A00_0000;

/// Eye height (m) used when reasoning about base/silhouette visibility — matches
/// the viewer's `EYE_HEIGHT_M` so the scoring sightlines agree with what renders.
const EYE_HEIGHT_M: f32 = 1.7;


/// Tunable thresholds and band altitudes that shape the vista. All distances are
/// in metres in the local world-metre frame; slopes are dimensionless rise/run.
#[derive(Clone, Copy, Debug)]
pub struct VistaConfig {
    /// Allowed peak distance from spawn `(min, max)` — far enough to feel huge,
    /// near enough to be a destination.
    pub distance_range_m: (f32, f32),
    /// The preferred peak distance the scorer pulls toward.
    pub ideal_distance_m: f32,
    /// Minimum prominence (peak altitude − base altitude) for an acceptable
    /// mountain. Everest-scale relief.
    pub min_prominence_m: f32,
    /// Relief (m) the analytic massif adds at its summit above the base ground —
    /// the constructed prominence.
    pub peak_relief_m: f32,
    /// Footprint radius (m) of the massif; broad so the average flank is gentle
    /// and a switchback route fits.
    pub massif_base_radius_m: f32,
    /// Radius (m) of the fully-flattened landing disk around the spawn.
    pub flat_radius_m: f32,
    /// Width (m) of the smooth feather blending the shelf edge into terrain.
    pub flat_blend_m: f32,
    /// Maximum slope tolerated on the spawn shelf (the flattened disk is ~0).
    pub max_spawn_slope: f32,
    /// Maximum slope tolerated along the carved route.
    pub max_path_slope: f32,
    /// Slope the route is actually built at (kept under `max_path_slope`).
    pub path_grade: f32,
    /// Radius (m) searched for a flatter nearby spawn if the pick is rough.
    pub spawn_search_radius_m: f32,
    /// Cloud band absolute altitude range `(low, high)` above spawn ground; the
    /// upper mountain fades into and beyond it.
    pub cloud_band_m: (f32, f32),
    /// Top of the green vegetation band (m above spawn ground).
    pub vegetation_line_m: f32,
    /// Altitude (m) by which bare rock fully dominates.
    pub rockline_m: f32,
    /// Snowline (m above spawn ground); permanent snow above it.
    pub snowline_m: f32,
    /// How many distance candidates the director scores.
    pub candidate_count: u32,
    /// Fraction of the relief the route's high endpoint reaches (a ledge high on
    /// the flank, not the geometric summit).
    pub high_endpoint_fraction: f32,
    /// Half-width (m) of the carved route corridor (a trail groove).
    pub route_half_width_m: f32,
    /// Number of switchback loops the upper route spirals through.
    pub route_turns: f32,
}

impl Default for VistaConfig {
    fn default() -> Self {
        Self {
            // Peak distance: a third of the original ~6 km framing, so the
            // mountain reads as a near, looming destination. The footprint radius
            // is scaled down with it (it must stay < the distance or the massif
            // would engulf the spawn), so the composition geometry is unchanged —
            // just closer. The 8200 m relief is kept, so the closer mountain is
            // even more towering (~76° to the summit from the spawn).
            distance_range_m: (1600.0, 2900.0),
            ideal_distance_m: 2000.0,
            min_prominence_m: 5000.0,
            peak_relief_m: 8200.0,
            massif_base_radius_m: 1000.0,
            flat_radius_m: 45.0,
            flat_blend_m: 60.0,
            max_spawn_slope: 0.35,
            max_path_slope: 0.50,
            path_grade: 0.32,
            spawn_search_radius_m: 60.0,
            cloud_band_m: (3400.0, 5200.0),
            vegetation_line_m: 1100.0,
            rockline_m: 1700.0,
            snowline_m: 4300.0,
            candidate_count: 24,
            high_endpoint_fraction: 0.60,
            // More switchback loops to cover the tighter (1 km) footprint, so the
            // carved route still climbs high and reads as a winding trail.
            route_turns: 5.0,
            route_half_width_m: 12.0,
        }
    }
}


/// A reference to the target mountain landform the vista is composed around.
/// The massif is generated (not found in noise), so the id distinguishes the
/// chosen candidate deterministically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountainId(pub u32);

/// A secondary ridge radiating from the summit — gives the massif a readable
/// large-scale silhouette instead of a smooth cone.
#[derive(Clone, Copy, Debug)]
struct Ridge {
    /// Azimuth (radians) of the ridge crest, measured from +X about +Y.
    angle: f32,
    /// Added crest height (m) at the summit, tapering out along the flank.
    amplitude_m: f32,
    /// Angular half-width (radians) of the crest's influence.
    width: f32,
}

/// The coherent analytic mountain landform: a broad massif base, a dominant
/// summit, secondary ridges, and a carved route. Its height contribution at any
/// `(x, z)` is a pure function, so it composites cleanly into the terrain
/// sampler. This is **not** noise×height — it is a constructed massif.
#[derive(Clone, Debug)]
pub struct MountainMassif {
    /// Horizontal centre of the base footprint (m).
    pub base_xz: (f32, f32),
    /// Horizontal position of the summit (m); near the base centre.
    pub peak_xz: (f32, f32),
    /// Footprint radius (m): the raise tapers to zero at this radius.
    pub base_radius_m: f32,
    /// Relief (m) added at the summit above the base ground.
    pub peak_relief_m: f32,
    /// Absolute altitude (m) of the surrounding base ground the massif rises from.
    pub shelf_height_m: f32,
    ridges: Vec<Ridge>,
    /// Carved route waypoints `(x, z, target_absolute_height_m)` from the spawn,
    /// across to the base, then spiralling up the flank.
    route_pts: Vec<(f32, f32, f32)>,
    /// Half-width (m) of the carved corridor.
    route_half_width_m: f32,
}

impl MountainMassif {
    /// The relief (m) the massif adds above the base ground at `(x, z)` — a broad
    /// base, a dominant summit, plus secondary ridge crests. Zero outside the
    /// footprint, so the contribution is fully localized.
    pub fn massif_raise(&self, x: f32, z: f32) -> f32 {
        let dx = x - self.peak_xz.0;
        let dz = z - self.peak_xz.1;
        let r = (dx * dx + dz * dz).sqrt();
        if r >= self.base_radius_m {
            return 0.0;
        }
        let u = (r / self.base_radius_m).clamp(0.0, 1.0);
        let core = radial_core(u);
        let mut raise = self.peak_relief_m * core;

        // Secondary ridges: a crest along each ridge azimuth, tapering with the
        // same radial core so it sits on the flank, narrowing away from its line.
        let phi = dz.atan2(dx);
        for ridge in &self.ridges {
            let mut d = (phi - ridge.angle).abs();
            // Wrap the angular difference into [0, PI].
            if d > core::f32::consts::PI {
                d = core::f32::consts::TAU - d;
            }
            let along = (-(d / ridge.width) * (d / ridge.width)).exp();
            raise += ridge.amplitude_m * core * along;
        }
        raise
    }

    /// The carved route's influence at `(x, z)`: a `(weight, target_height)` pair
    /// where `weight` in `[0, 1]` is 1 on the trail centreline and falls to 0 at
    /// the corridor edge, and `target_height` is the trail's absolute altitude
    /// there. Off the corridor the weight is 0 (the target is unused).
    pub fn route_override(&self, x: f32, z: f32) -> (f32, f32) {
        let half = self.route_half_width_m.max(0.001);
        let mut best_d2 = f32::INFINITY;
        let mut best_target = self.shelf_height_m;
        for seg in self.route_pts.windows(2) {
            let (ax, az, ah) = seg[0];
            let (bx, bz, bh) = seg[1];
            let abx = bx - ax;
            let abz = bz - az;
            let len2 = abx * abx + abz * abz;
            let t = if len2 <= f32::EPSILON {
                0.0
            } else {
                (((x - ax) * abx + (z - az) * abz) / len2).clamp(0.0, 1.0)
            };
            let px = ax + t * abx;
            let pz = az + t * abz;
            let d2 = (x - px) * (x - px) + (z - pz) * (z - pz);
            if d2 < best_d2 {
                best_d2 = d2;
                best_target = ah + t * (bh - ah);
            }
        }
        let d = best_d2.sqrt();
        let w = (1.0 - d / half).clamp(0.0, 1.0);
        (smoothstep01(w), best_target)
    }
}

/// Per-candidate scoring data, surfaced for debug/telemetry so a reviewer can
/// see *why* the chosen vista was accepted.
#[derive(Clone, Copy, Debug)]
pub struct VistaScore {
    /// Spawn-shelf flatness score in `[0, 1]` (1 = dead flat).
    pub flatness: f32,
    /// Prominence (m): peak altitude − base altitude.
    pub prominence: f32,
    /// Distance fit in `[0, 1]` (1 = at the ideal distance).
    pub distance_score: f32,
    /// Whether the lower mountain / base region clears intervening terrain.
    pub base_visibility: bool,
    /// Whether the summit silhouette rises clear above the horizon.
    pub silhouette_visibility: bool,
    /// Whether every carved route segment is within the path-slope budget.
    pub path_walkability: bool,
    /// Whether all hard requirements held (the candidate is acceptable).
    pub accept: bool,
    /// A stable hash of the accepted plan's defining numbers, for the
    /// determinism test and console telemetry.
    pub fingerprint: u64,
}

/// The composed scenic plan that drives spawn, orientation, mountain, route, and
/// atmosphere. Produced by [`VistaDirector::plan`] and composited into terrain by
/// [`vista_height_m`].
#[derive(Clone, Debug)]
pub struct MountainVistaPlan {
    /// Final spawn position (m) — a flattened landing shelf near the map pick.
    pub spawn_xz: (f32, f32),
    /// Radius (m) of the flat landing disk.
    pub flat_radius_m: f32,
    /// Width (m) of the smooth feather blending the shelf edge into terrain.
    pub flat_blend_m: f32,
    /// Absolute altitude (m) of the spawn shelf (the surrounding ground level).
    pub shelf_height_m: f32,
    /// Initial horizontal view direction (unit) — toward the mountain base.
    pub view_dir_xz: (f32, f32),
    /// Initial camera yaw (radians) consistent with `view_dir_xz` (0 = facing −Z).
    pub view_yaw: f32,
    /// The target mountain.
    pub mountain: MountainId,
    /// Mountain base (near rim) position (m).
    pub base_xz: (f32, f32),
    /// Absolute altitude (m) at the base.
    pub base_height_m: f32,
    /// Summit position (m).
    pub peak_xz: (f32, f32),
    /// Absolute altitude (m) at the summit.
    pub peak_height_m: f32,
    /// Peak distance (m) from spawn.
    pub distance_m: f32,
    /// The configured acceptable distance range `(min, max)`.
    pub distance_range_m: (f32, f32),
    /// Prominence / relief score (m): peak altitude − base altitude.
    pub prominence_m: f32,
    /// Base-visibility requirement result.
    pub base_visible: bool,
    /// Silhouette-visibility requirement result.
    pub silhouette_visible: bool,
    /// Path-to-base requirement result.
    pub path_to_base: bool,
    /// Path-to-summit (high endpoint) requirement result.
    pub path_to_summit: bool,
    /// Max spawn slope budget.
    pub max_spawn_slope: f32,
    /// Max path slope budget.
    pub max_path_slope: f32,
    /// Cloud band absolute altitude range `(low, high)`.
    pub cloud_band_m: (f32, f32),
    /// Top of the green vegetation band (m).
    pub vegetation_line_m: f32,
    /// Altitude (m) by which rock fully dominates.
    pub rockline_m: f32,
    /// Snowline (m).
    pub snowline_m: f32,
    /// The route polyline (xz) from spawn → base → up the flank, for readability
    /// markers and debug.
    pub route: Vec<(f32, f32)>,
    /// The analytic massif the plan is built around.
    pub massif: MountainMassif,
    /// Scoring/debug data for the accepted candidate.
    pub debug: VistaScore,
}


/// Composite the vista into a base terrain height at `(x, z)`: flatten the spawn
/// shelf, raise the analytic massif, then carve the route ledge. `base_h` is the
/// un-composited terrain height (macro + detail) at the same point. Pure and
/// fully localized — far from the shelf, massif, and route it returns `base_h`
/// unchanged. This is the single function both the ground sampler and the far
/// scenic mesh use, so collision and rendering see exactly the same world.
pub fn vista_height_m(plan: &MountainVistaPlan, base_h: f32, x: f32, z: f32) -> f32 {
    // 1. Flatten the landing shelf, feathering smoothly into surrounding terrain.
    let shelf = shelf_blend(plan, base_h, x, z);
    // 2. Raise the massif (relief above the local ground).
    let raised = shelf + plan.massif.massif_raise(x, z);
    // 3. Carve the route corridor toward its gentle target altitude.
    let (w, target) = plan.massif.route_override(x, z);
    raised * (1.0 - w) + target * w
}

/// Flatten a disk of radius `flat_radius_m` around the spawn to the shelf level,
/// then feather to `base_h` over `flat_blend_m`. Continuous everywhere (smoothstep
/// blend), so the shelf edge is never a cliff.
fn shelf_blend(plan: &MountainVistaPlan, base_h: f32, x: f32, z: f32) -> f32 {
    let dx = x - plan.spawn_xz.0;
    let dz = z - plan.spawn_xz.1;
    let d = (dx * dx + dz * dz).sqrt();
    let r = plan.flat_radius_m;
    let b = plan.flat_blend_m.max(0.001);
    if d <= r {
        plan.shelf_height_m
    } else if d >= r + b {
        base_h
    } else {
        let s = smoothstep01((d - r) / b);
        plan.shelf_height_m * (1.0 - s) + base_h * s
    }
}


/// The deterministic scenic generator. Stateless; [`Self::plan`] is the entry.
#[derive(Debug)]
pub struct VistaDirector;

impl VistaDirector {
    /// Generate (or select) the [`MountainVistaPlan`] for a descent. Reads the
    /// base terrain via [`sample_height_m`], scores distance candidates around
    /// the local −Z heading, and returns the best acceptable composition — always
    /// producing a valid plan (the massif is analytic, so a valid candidate is
    /// guaranteed even if the sampled ones are rejected).
    pub fn plan(
        atlas: &PlanetSurfaceAtlas,
        localmap: &GameWorldLocalMap,
        seed: u64,
        cfg: VistaConfig,
    ) -> MountainVistaPlan {
        let mut rng = worldgen_stream(seed).fork(VISTA_SALT);

        // 1. Choose the flattest nearby spawn (the "nudge"), so the shelf blend is
        //    gentle and we never seat the player on rough ground.
        let spawn = choose_spawn(atlas, localmap, seed, cfg);
        let shelf_height = sample_height_m(atlas, localmap, seed, spawn.0, spawn.1);

        // 2. Score distance candidates; keep the best acceptable one. The massif
        //    sits on local −Z from the spawn (see module docs).
        let mut best: Option<(f32, f32)> = None; // (score, distance)
        for _ in 0..cfg.candidate_count.max(1) {
            let dist =
                distributions::range(&mut rng, cfg.distance_range_m.0, cfg.distance_range_m.1);
            let eval = evaluate(atlas, localmap, seed, spawn, shelf_height, dist, cfg);
            if eval.accept {
                let s = composite_score(&eval, dist, cfg);
                if best.map(|(bs, _)| s > bs).unwrap_or(true) {
                    best = Some((s, dist));
                }
            }
        }
        // Guarantee: if every sampled candidate was rejected, fall back to the
        // ideal distance — the analytic massif makes it a valid composition.
        let distance = best.map(|(_, d)| d).unwrap_or(cfg.ideal_distance_m);

        build_plan(atlas, localmap, seed, spawn, shelf_height, distance, cfg)
    }
}

/// A candidate evaluation: the derived geometry plus its [`VistaScore`].
struct Eval {
    base_xz: (f32, f32),
    base_height: f32,
    peak_xz: (f32, f32),
    peak_height: f32,
    prominence: f32,
    distance_score: f32,
    flatness: f32,
    base_visible: bool,
    silhouette_visible: bool,
    path_walkable: bool,
    accept: bool,
}

/// Score one candidate distance without building the full plan (cheap enough to
/// run for every candidate). Hard requirements gate `accept`.
fn evaluate(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    spawn: (f32, f32),
    shelf_height: f32,
    distance: f32,
    cfg: VistaConfig,
) -> Eval {
    // Peak on local −Z; base is the near rim facing the spawn.
    let peak_xz = (spawn.0, spawn.1 - distance);
    let rim = cfg.massif_base_radius_m * 0.9;
    let base_xz = (peak_xz.0, peak_xz.1 + rim);

    let base_ground = sample_height_m(atlas, localmap, seed, base_xz.0, base_xz.1);
    let peak_ground = sample_height_m(atlas, localmap, seed, peak_xz.0, peak_xz.1);
    let base_height = base_ground; // rim taper -> raise ~0 at the rim
    let peak_height = peak_ground + cfg.peak_relief_m;
    let prominence = peak_height - base_height;

    let distance_in_range =
        distance >= cfg.distance_range_m.0 && distance <= cfg.distance_range_m.1;
    let distance_score = distance_fit(distance, cfg);
    let flatness = flatness_at(atlas, localmap, seed, spawn);

    // Base visibility: the lower flank (a reference point ~15% up the relief)
    // must clear intervening base terrain along the sightline from the eye.
    let base_visible = lower_flank_visible(
        atlas, localmap, seed, spawn, shelf_height, peak_xz, cfg,
    );
    // Silhouette: the summit must rise clear above the eye's horizon line.
    let silhouette_visible =
        (peak_height - (shelf_height + EYE_HEIGHT_M)) > cfg.min_prominence_m * 0.5;
    // Walkability: the constructed route stays within the path-slope budget by
    // construction; verify the built waypoints respect it.
    let route = build_route(spawn, shelf_height, base_xz, peak_xz, cfg);
    let path_walkable = route_within_slope(&route, cfg.max_path_slope);

    let accept = distance_in_range
        && prominence >= cfg.min_prominence_m
        && base_visible
        && silhouette_visible
        && path_walkable;

    Eval {
        base_xz,
        base_height,
        peak_xz,
        peak_height,
        prominence,
        distance_score,
        flatness,
        base_visible,
        silhouette_visible,
        path_walkable,
        accept,
    }
}

/// A scalar used to rank accepted candidates: favour the ideal distance and a
/// flatter spawn, both already in `[0, 1]`.
fn composite_score(e: &Eval, _distance: f32, _cfg: VistaConfig) -> f32 {
    e.distance_score * 0.7 + e.flatness * 0.3
}

/// Build the full plan for the chosen distance.
fn build_plan(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    spawn: (f32, f32),
    shelf_height: f32,
    distance: f32,
    cfg: VistaConfig,
) -> MountainVistaPlan {
    let e = evaluate(atlas, localmap, seed, spawn, shelf_height, distance, cfg);

    let ridges = build_ridges(seed);
    let route_pts = build_route(spawn, shelf_height, e.base_xz, e.peak_xz, cfg);
    let route: Vec<(f32, f32)> = route_pts.iter().map(|&(x, z, _)| (x, z)).collect();

    let massif = MountainMassif {
        base_xz: e.base_xz,
        peak_xz: e.peak_xz,
        base_radius_m: cfg.massif_base_radius_m,
        peak_relief_m: cfg.peak_relief_m,
        shelf_height_m: shelf_height,
        ridges,
        route_pts,
        route_half_width_m: cfg.route_half_width_m_or_default(),
    };

    // The route's high endpoint reaches a ledge `high_endpoint_fraction` up the
    // relief — substantially above the base, the "path to summit" target.
    let high_endpoint_h = shelf_height + cfg.peak_relief_m * cfg.high_endpoint_fraction;
    let path_to_summit = massif
        .route_pts
        .last()
        .map(|&(_, _, h)| h >= shelf_height + cfg.peak_relief_m * 0.25)
        .unwrap_or(false)
        && high_endpoint_h > e.base_height;

    let debug = VistaScore {
        flatness: e.flatness,
        prominence: e.prominence,
        distance_score: e.distance_score,
        base_visibility: e.base_visible,
        silhouette_visibility: e.silhouette_visible,
        path_walkability: e.path_walkable,
        accept: e.accept,
        fingerprint: 0,
    };

    let mut plan = MountainVistaPlan {
        spawn_xz: spawn,
        flat_radius_m: cfg.flat_radius_m,
        flat_blend_m: cfg.flat_blend_m,
        shelf_height_m: shelf_height,
        view_dir_xz: (0.0, -1.0),
        view_yaw: 0.0,
        mountain: MountainId(mountain_id(seed, distance)),
        base_xz: e.base_xz,
        base_height_m: e.base_height,
        peak_xz: e.peak_xz,
        peak_height_m: e.peak_height,
        distance_m: distance,
        distance_range_m: cfg.distance_range_m,
        prominence_m: e.prominence,
        base_visible: e.base_visible,
        silhouette_visible: e.silhouette_visible,
        path_to_base: !massif.route_pts.is_empty(),
        path_to_summit,
        max_spawn_slope: cfg.max_spawn_slope,
        max_path_slope: cfg.max_path_slope,
        cloud_band_m: cfg.cloud_band_m,
        vegetation_line_m: cfg.vegetation_line_m,
        rockline_m: cfg.rockline_m,
        snowline_m: cfg.snowline_m,
        route,
        massif,
        debug,
    };
    plan.debug.fingerprint = fingerprint(&plan);
    plan
}

impl VistaConfig {
    /// The route corridor half-width, defaulting if a caller leaves it at zero.
    fn route_half_width_m_or_default(&self) -> f32 {
        if self.route_half_width_m > 0.0 {
            self.route_half_width_m
        } else {
            12.0
        }
    }
}


/// Deterministically pick the flattest spot within `spawn_search_radius_m` of
/// the local origin (the map pick), so the shelf blends gently and the player
/// never lands on rough ground. Evaluates the origin plus a fixed ring of
/// offsets and keeps the flattest.
fn choose_spawn(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    cfg: VistaConfig,
) -> (f32, f32) {
    let r = cfg.spawn_search_radius_m;
    let mut best = (0.0_f32, 0.0_f32);
    let mut best_flat = flatness_at(atlas, localmap, seed, best);
    // Two rings of eight directions each — a small, deterministic search.
    for ring in 1..=2 {
        let rad = r * ring as f32 / 2.0;
        for k in 0..8 {
            let a = core::f32::consts::TAU * k as f32 / 8.0;
            let p = (rad * a.cos(), rad * a.sin());
            let f = flatness_at(atlas, localmap, seed, p);
            if f > best_flat {
                best_flat = f;
                best = p;
            }
        }
    }
    best
}

/// Flatness score in `[0, 1]` at `(x, z)`: 1 when the local 4-neighbour slope is
/// zero, decaying as the maximum adjacent slope grows.
fn flatness_at(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    p: (f32, f32),
) -> f32 {
    const D: f32 = 4.0;
    let h = |x: f32, z: f32| sample_height_m(atlas, localmap, seed, x, z);
    let c = h(p.0, p.1);
    let sx = (h(p.0 + D, p.1) - c).abs().max((c - h(p.0 - D, p.1)).abs()) / D;
    let sz = (h(p.0, p.1 + D) - c).abs().max((c - h(p.0, p.1 - D)).abs()) / D;
    let slope = sx.max(sz);
    1.0 / (1.0 + slope * 4.0)
}


/// Whether a reference point ~15% up the relief (representing the visible lower
/// mountain) clears intervening base terrain on the sightline from the eye.
fn lower_flank_visible(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    spawn: (f32, f32),
    shelf_height: f32,
    peak_xz: (f32, f32),
    cfg: VistaConfig,
) -> bool {
    let eye_h = shelf_height + EYE_HEIGHT_M;
    // Reference point: on the near flank, at ~15% of the relief.
    let rim = cfg.massif_base_radius_m * 0.55;
    let ref_xz = (peak_xz.0, peak_xz.1 + rim);
    let ref_h = shelf_height + cfg.peak_relief_m * 0.15;
    let total = (ref_xz.1 - spawn.1).hypot(ref_xz.0 - spawn.0).max(1.0);

    // March the sightline; reject if intervening terrain pokes above it.
    let steps = 24;
    let mut visible = true;
    for i in 1..steps {
        let t = i as f32 / steps as f32;
        let x = spawn.0 + (ref_xz.0 - spawn.0) * t;
        let z = spawn.1 + (ref_xz.1 - spawn.1) * t;
        // Skip samples inside the massif footprint — that ground *is* the
        // mountain, not an occluder.
        let dr = (x - peak_xz.0).hypot(z - peak_xz.1);
        if dr < cfg.massif_base_radius_m {
            continue;
        }
        let line_h = eye_h + (ref_h - eye_h) * t;
        let ground = sample_height_m(atlas, localmap, seed, x, z);
        // A small tolerance so minor roughness near the line doesn't reject.
        let clearance = 0.02 * total * (1.0 - t);
        if ground > line_h + clearance {
            visible = false;
        }
    }
    visible
}


/// Build the carved route as `(x, z, target_absolute_height)` waypoints: a flat
/// leg from the spawn to the near rim, then a switchback spiral up the flank to
/// the high endpoint. Target altitude rises at `path_grade` along cumulative
/// path length, so every segment is within the slope budget by construction.
fn build_route(
    spawn: (f32, f32),
    shelf_height: f32,
    base_xz: (f32, f32),
    peak_xz: (f32, f32),
    cfg: VistaConfig,
) -> Vec<(f32, f32, f32)> {
    let mut pts: Vec<(f32, f32)> = Vec::new();

    // Leg 1: spawn -> base rim, a straight approach (a few points).
    let approach_steps = 6;
    for i in 0..=approach_steps {
        let t = i as f32 / approach_steps as f32;
        pts.push((
            spawn.0 + (base_xz.0 - spawn.0) * t,
            spawn.1 + (base_xz.1 - spawn.1) * t,
        ));
    }

    // Leg 2: spiral from the rim toward the summit, shrinking radius while
    // winding `route_turns` loops — the switchbacks.
    let r_outer = cfg.massif_base_radius_m * 0.9;
    let r_top = cfg.massif_base_radius_m * 0.15;
    // Start angle so the spiral begins at the near rim (toward the spawn, +Z).
    let start_angle = core::f32::consts::FRAC_PI_2; // +Z direction in atan2(dz,dx)
    let spiral_steps = 160;
    for i in 1..=spiral_steps {
        let t = i as f32 / spiral_steps as f32;
        let r = r_outer + (r_top - r_outer) * t;
        let ang = start_angle + cfg.route_turns * core::f32::consts::TAU * t;
        pts.push((peak_xz.0 + r * ang.cos(), peak_xz.1 + r * ang.sin()));
    }

    // Assign target altitudes: flat (shelf) until we reach the rim, then climb at
    // `path_grade` along cumulative horizontal length, capped at the high
    // endpoint.
    let rim_index = approach_steps;
    let cap = shelf_height + cfg.peak_relief_m * cfg.high_endpoint_fraction;
    let mut out: Vec<(f32, f32, f32)> = Vec::with_capacity(pts.len());
    let mut climb = 0.0_f32;
    for (i, &(x, z)) in pts.iter().enumerate() {
        if i == 0 {
            out.push((x, z, shelf_height));
            continue;
        }
        let (px, pz, _) = out[i - 1];
        let seg = (x - px).hypot(z - pz);
        if i > rim_index {
            climb = (climb + cfg.path_grade * seg).min(cap - shelf_height);
        }
        out.push((x, z, shelf_height + climb));
    }
    out
}

/// Whether every consecutive route segment's slope is within `max_slope`.
fn route_within_slope(route: &[(f32, f32, f32)], max_slope: f32) -> bool {
    route.windows(2).all(|w| {
        let (ax, az, ah) = w[0];
        let (bx, bz, bh) = w[1];
        let run = (bx - ax).hypot(bz - az);
        if run <= f32::EPSILON {
            return (bh - ah).abs() <= 1.0e-3;
        }
        (bh - ah).abs() / run <= max_slope + 1.0e-4
    })
}


/// The base material colour (linear RGB) for terrain at `altitude_m` above the
/// spawn ground: green vegetation low, grey-brown rock mid, white snow high.
pub fn band_color(altitude_m: f32, plan: &MountainVistaPlan) -> [f32; 3] {
    let veg = [0.24, 0.40, 0.20];
    let rock = [0.42, 0.39, 0.36];
    let snow = [0.95, 0.96, 0.99];
    let a = altitude_m - plan.shelf_height_m;
    if a <= plan.vegetation_line_m {
        veg
    } else if a <= plan.rockline_m {
        let t = smoothstep01((a - plan.vegetation_line_m) / (plan.rockline_m - plan.vegetation_line_m).max(1.0));
        lerp3(veg, rock, t)
    } else if a <= plan.snowline_m {
        let t = smoothstep01((a - plan.rockline_m) / (plan.snowline_m - plan.rockline_m).max(1.0));
        lerp3(rock, snow, t * 0.5)
    } else {
        let t = smoothstep01((a - plan.snowline_m) / 600.0);
        lerp3(lerp3(rock, snow, 0.5), snow, t)
    }
}

/// Apply atmospheric perspective + cloud-band fade to a base colour, returning a
/// final `[r, g, b, a]`. Distant geometry fades toward `sky`; geometry in the
/// cloud band fades strongly toward cloud-white (partial occlusion); above the
/// band a faint residual haze keeps the summit reading as "taller than visible".
pub fn apply_atmosphere(
    base: [f32; 3],
    distance_m: f32,
    altitude_m: f32,
    plan: &MountainVistaPlan,
    sky: [f32; 3],
) -> [f32; 4] {
    // Aerial perspective: exponential fade toward the sky with distance.
    const ATMO_SCALE_M: f32 = 9000.0;
    let aerial = 1.0 - (-distance_m / ATMO_SCALE_M).exp();
    let mut c = lerp3(base, sky, aerial.clamp(0.0, 0.85));

    // Cloud band: a soft white veil where the mountain crosses the band, plus a
    // residual summit haze above it.
    let cloud = [0.90, 0.93, 0.97];
    let a = altitude_m - plan.shelf_height_m;
    let (lo, hi) = plan.cloud_band_m;
    let band = if a < lo {
        0.0
    } else if a > hi {
        // Residual haze that grows again toward the very top.
        0.22 + 0.15 * smoothstep01((a - hi) / 2500.0)
    } else {
        // Densest in the middle of the band.
        let m = (a - lo) / (hi - lo).max(1.0);
        0.65 * (1.0 - (2.0 * m - 1.0).abs()) + 0.2
    };
    c = lerp3(c, cloud, band.clamp(0.0, 0.85));
    [c[0], c[1], c[2], 1.0]
}

/// A pale trail tint blended into a colour, for baking the readable switchback
/// line into the scenic mesh near the route.
pub fn trail_tint(base: [f32; 4], weight: f32) -> [f32; 4] {
    let pale = [0.78, 0.74, 0.66];
    let w = weight.clamp(0.0, 1.0);
    [
        base[0] * (1.0 - w) + pale[0] * w,
        base[1] * (1.0 - w) + pale[1] * w,
        base[2] * (1.0 - w) + pale[2] * w,
        base[3],
    ]
}


/// Smooth radial massif profile: 1 at the centre (`u = 0`), 0 at the rim
/// (`u = 1`), with a broad base and a sharpened, dominant summit. Smooth (no
/// kink) so normals are well-behaved.
fn radial_core(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    let raised_cos = 0.5 * (1.0 + (core::f32::consts::PI * u).cos());
    raised_cos.powf(1.3)
}

/// Distance fit in `[0, 1]`: 1 at the ideal distance, decaying toward the range
/// edges.
fn distance_fit(distance: f32, cfg: VistaConfig) -> f32 {
    let span = (cfg.distance_range_m.1 - cfg.distance_range_m.0).max(1.0);
    let off = (distance - cfg.ideal_distance_m).abs() / span;
    (1.0 - off).clamp(0.0, 1.0)
}

/// A stable hash of the plan's defining numbers (FNV-1a over their bit patterns)
/// for the determinism test and telemetry.
fn fingerprint(plan: &MountainVistaPlan) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    let mut mix = |v: f32| {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    };
    mix(plan.spawn_xz.0);
    mix(plan.spawn_xz.1);
    mix(plan.shelf_height_m);
    mix(plan.distance_m);
    mix(plan.peak_xz.0);
    mix(plan.peak_xz.1);
    mix(plan.peak_height_m);
    mix(plan.prominence_m);
    mix(plan.base_xz.0);
    mix(plan.base_xz.1);
    h ^ (plan.route.len() as u64).wrapping_mul(0x100_0000_01B3)
}

/// A deterministic mountain id from the seed and chosen distance.
fn mountain_id(seed: u64, distance: f32) -> u32 {
    let mut r = worldgen_stream(seed).fork((distance.to_bits() as u64).wrapping_shl(8));
    (r.next_u64() >> 32) as u32
}

/// Three secondary ridges at deterministic azimuths from the seed.
fn build_ridges(seed: u64) -> Vec<Ridge> {
    let mut r = worldgen_stream(seed).fork(0x21D6_E5A7);
    (0..3)
        .map(|_| Ridge {
            angle: distributions::range(&mut r, 0.0, core::f32::consts::TAU),
            amplitude_m: distributions::range(&mut r, 500.0, 1100.0),
            width: distributions::range(&mut r, 0.25, 0.5),
        })
        .collect()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_planet::{PlanetSurfaceAtlas, RegionGraph};
    use axiom_math::Vec3;

    /// A small synthetic atlas (mirrors `gameworld::tests`): a central land
    /// region plus four neighbours with gentle elevations, so base terrain is
    /// smooth enough to read the composition.
    fn synthetic_atlas() -> PlanetSurfaceAtlas {
        let sites = vec![
            Vec3::new(0.0, 1.0, 0.0).normalize().unwrap_or(Vec3::UNIT_Y),
            Vec3::new(0.1, 1.0, 0.0).normalize().unwrap_or(Vec3::UNIT_Y),
            Vec3::new(-0.1, 1.0, 0.0).normalize().unwrap_or(Vec3::UNIT_Y),
            Vec3::new(0.0, 1.0, 0.1).normalize().unwrap_or(Vec3::UNIT_Y),
            Vec3::new(0.0, 1.0, -0.1).normalize().unwrap_or(Vec3::UNIT_Y),
        ];
        let region_elevation = vec![0.30, 0.33, 0.28, 0.31, 0.29];
        let offsets = vec![0u32, 4, 5, 6, 7, 8];
        let neighbours = vec![1, 2, 3, 4, 0, 0, 0, 0];
        let graph = RegionGraph { offsets, neighbours };
        PlanetSurfaceAtlas {
            sites,
            graph,
            region_plate: vec![0; 5],
            plate_oceanic: vec![false; 5],
            region_elevation,
            region_moisture: vec![0.5; 5],
            planet_radius_m: axiom_kernel::Meters::finite_or_zero(6_000_000.0),
            locator: Default::default(),
        }
    }

    fn fixture() -> (PlanetSurfaceAtlas, GameWorldLocalMap, u64) {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        (atlas, localmap, 0x5CE1_70A1u64)
    }

    fn plan() -> MountainVistaPlan {
        let (atlas, localmap, seed) = fixture();
        VistaDirector::plan(&atlas, &localmap, seed, VistaConfig::default())
    }

    /// 1. Same seed/state ⇒ identical plan.
    #[test]
    fn plan_is_deterministic() {
        let (atlas, localmap, seed) = fixture();
        let cfg = VistaConfig::default();
        let a = VistaDirector::plan(&atlas, &localmap, seed, cfg);
        let b = VistaDirector::plan(&atlas, &localmap, seed, cfg);
        assert_eq!(a.debug.fingerprint, b.debug.fingerprint, "fingerprints differ");
        assert_eq!(a.spawn_xz, b.spawn_xz);
        assert_eq!(a.distance_m, b.distance_m);
        assert_eq!(a.peak_xz, b.peak_xz);
        assert_eq!(a.route.len(), b.route.len());
    }

    /// 2. Spawn shelf slope is within the configured budget.
    #[test]
    fn spawn_slope_within_threshold() {
        let p = plan();
        let (sx, sz) = p.spawn_xz;
        let base = |x: f32, z: f32| {
            // base terrain irrelevant inside the flat disk, but supply a real one.
            let (atlas, localmap, seed) = fixture();
            let b = sample_height_m(&atlas, &localmap, seed, x, z);
            vista_height_m(&p, b, x, z)
        };
        let step = 2.0;
        let mut max_slope = 0.0_f32;
        let mut dz = -p.flat_radius_m * 0.8;
        while dz <= p.flat_radius_m * 0.8 {
            let mut dx = -p.flat_radius_m * 0.8;
            while dx <= p.flat_radius_m * 0.8 {
                let h = base(sx + dx, sz + dz);
                let hx = base(sx + dx + step, sz + dz);
                let hzz = base(sx + dx, sz + dz + step);
                max_slope = max_slope.max((hx - h).abs() / step).max((hzz - h).abs() / step);
                dx += step;
            }
            dz += step;
        }
        assert!(
            max_slope <= p.max_spawn_slope,
            "spawn slope {max_slope} exceeds budget {}",
            p.max_spawn_slope
        );
    }

    /// 3. The flat landing radius is at least the configured radius.
    #[test]
    fn spawn_flat_radius_within_threshold() {
        let p = plan();
        assert!(p.flat_radius_m >= 30.0, "flat radius too small: {}", p.flat_radius_m);
        // The disk is genuinely flat: centre and a point near the rim agree.
        let (atlas, localmap, seed) = fixture();
        let (sx, sz) = p.spawn_xz;
        let c = vista_height_m(&p, sample_height_m(&atlas, &localmap, seed, sx, sz), sx, sz);
        let edge_r = p.flat_radius_m * 0.9;
        let e = vista_height_m(
            &p,
            sample_height_m(&atlas, &localmap, seed, sx + edge_r, sz),
            sx + edge_r,
            sz,
        );
        assert!((c - e).abs() < 1.0e-3, "shelf not flat to its radius: {c} vs {e}");
    }

    /// 4. Mountain distance is within the configured range.
    #[test]
    fn mountain_distance_in_range() {
        let p = plan();
        assert!(
            p.distance_m >= p.distance_range_m.0 && p.distance_m <= p.distance_range_m.1,
            "distance {} out of range {:?}",
            p.distance_m,
            p.distance_range_m
        );
    }

    /// 5. Prominence is above the configured threshold.
    #[test]
    fn prominence_above_threshold() {
        let p = plan();
        assert!(
            p.prominence_m >= VistaConfig::default().min_prominence_m,
            "prominence {} below threshold",
            p.prominence_m
        );
    }

    /// 6. Initial view direction points at the mountain base.
    #[test]
    fn view_dir_points_at_base() {
        let p = plan();
        let dx = p.base_xz.0 - p.spawn_xz.0;
        let dz = p.base_xz.1 - p.spawn_xz.1;
        let len = (dx * dx + dz * dz).sqrt().max(1.0e-6);
        let dot = (p.view_dir_xz.0 * dx + p.view_dir_xz.1 * dz) / len;
        assert!(dot > 0.999, "view dir not aimed at base: dot {dot}");
    }

    /// 7. A route exists from the spawn to the base.
    #[test]
    fn route_exists_spawn_to_base() {
        let p = plan();
        assert!(p.path_to_base, "path_to_base must hold");
        assert!(!p.route.is_empty());
        let first = p.route[0];
        assert!(
            (first.0 - p.spawn_xz.0).abs() < 1.0 && (first.1 - p.spawn_xz.1).abs() < 1.0,
            "route must start at spawn"
        );
        // The base rim appears among the early (approach) waypoints.
        let reaches_base = p
            .route
            .iter()
            .any(|&(x, z)| (x - p.base_xz.0).abs() < 1.0 && (z - p.base_xz.1).abs() < 1.0);
        assert!(reaches_base, "route must reach the base rim");
    }

    /// 8. A route exists from the base up toward the summit (a high endpoint).
    #[test]
    fn route_exists_base_to_summit() {
        let p = plan();
        assert!(p.path_to_summit, "path_to_summit must hold");
        let (_, _, top_h) = *p.massif.route_pts.last().unwrap();
        assert!(
            top_h > p.base_height_m + p.prominence_m * 0.2,
            "route high endpoint {top_h} not substantially above base {}",
            p.base_height_m
        );
    }

    /// 9. Every route segment respects the path-slope budget.
    #[test]
    fn path_slope_constraints_enforced() {
        let p = plan();
        assert!(
            route_within_slope(&p.massif.route_pts, p.max_path_slope),
            "a route segment exceeds the path-slope budget"
        );
        // And the composited height at each waypoint matches the carved target
        // (so the slope the player actually walks is the controlled one).
        let (atlas, localmap, seed) = fixture();
        for &(x, z, target) in &p.massif.route_pts {
            let b = sample_height_m(&atlas, &localmap, seed, x, z);
            let h = vista_height_m(&p, b, x, z);
            assert!((h - target).abs() < 1.0, "waypoint height {h} != target {target}");
        }
    }

    /// 10. A bad candidate (out-of-range distance) is rejected by the scorer.
    #[test]
    fn bad_candidate_is_rejected() {
        let (atlas, localmap, seed) = fixture();
        let cfg = VistaConfig::default();
        let spawn = choose_spawn(&atlas, &localmap, seed, cfg);
        let shelf = sample_height_m(&atlas, &localmap, seed, spawn.0, spawn.1);
        // Far beyond the allowed range.
        let bad = cfg.distance_range_m.1 + 4000.0;
        let e = evaluate(&atlas, &localmap, seed, spawn, shelf, bad, cfg);
        assert!(!e.accept, "an out-of-range candidate must be rejected");
    }

    /// 11. A good candidate (ideal distance) is accepted.
    #[test]
    fn good_candidate_is_selected() {
        let (atlas, localmap, seed) = fixture();
        let cfg = VistaConfig::default();
        let spawn = choose_spawn(&atlas, &localmap, seed, cfg);
        let shelf = sample_height_m(&atlas, &localmap, seed, spawn.0, spawn.1);
        let e = evaluate(&atlas, &localmap, seed, spawn, shelf, cfg.ideal_distance_m, cfg);
        assert!(e.accept, "the ideal-distance candidate must be accepted");
        // And the produced plan is itself an accepted composition.
        let p = plan();
        assert!(p.debug.accept, "selected plan must be accepted");
    }

    /// 12. Flattening blends at the edge — adjacent composited heights across the
    ///     shelf boundary stay continuous (no cliff).
    #[test]
    fn flatten_blends_at_edge() {
        let p = plan();
        let (atlas, localmap, seed) = fixture();
        let (sx, sz) = p.spawn_xz;
        let step = 1.0;
        let mut max_delta = 0.0_f32;
        // Walk radially from the centre out past the blend zone along +X.
        let mut d = 0.0_f32;
        let end = p.flat_radius_m + p.flat_blend_m + 20.0;
        while d < end {
            let h0 = vista_height_m(
                &p,
                sample_height_m(&atlas, &localmap, seed, sx + d, sz),
                sx + d,
                sz,
            );
            let h1 = vista_height_m(
                &p,
                sample_height_m(&atlas, &localmap, seed, sx + d + step, sz),
                sx + d + step,
                sz,
            );
            max_delta = max_delta.max((h1 - h0).abs());
            d += step;
        }
        // A genuine blend keeps per-metre change small; a hard cliff would be
        // many metres in one step.
        assert!(
            max_delta < 3.0,
            "shelf edge is not a smooth blend: max per-metre delta {max_delta}"
        );
    }

    /// The carved massif raise is localized: zero well outside the footprint.
    #[test]
    fn massif_raise_is_localized() {
        let p = plan();
        let far = p.massif.massif_raise(p.peak_xz.0 + 9000.0, p.peak_xz.1);
        assert_eq!(far, 0.0, "massif must not affect far-away ground");
        let summit = p.massif.massif_raise(p.peak_xz.0, p.peak_xz.1);
        assert!(summit > p.prominence_m * 0.8, "summit raise too small: {summit}");
    }

    /// Atmosphere fades distant geometry toward the sky and snowcaps high ground.
    #[test]
    fn atmosphere_and_bands_read() {
        let p = plan();
        let sky = [0.45, 0.62, 0.85];
        let low = band_color(p.shelf_height_m + 100.0, &p);
        let high = band_color(p.shelf_height_m + p.snowline_m + 500.0, &p);
        assert!(high[0] > low[0] && high[2] > low[2], "snow band must be paler than vegetation");
        let near = apply_atmosphere([0.3, 0.4, 0.2], 200.0, p.shelf_height_m + 50.0, &p, sky);
        let farc = apply_atmosphere([0.3, 0.4, 0.2], 8000.0, p.shelf_height_m + 50.0, &p, sky);
        // Distant colour is pulled toward the (bluer) sky.
        assert!(farc[2] > near[2], "distant geometry must fade toward sky");
    }
}
