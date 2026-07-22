//! Bounded instance-pool construction for the retained scene: the juice,
//! receiver-ring, pre-snap-chalk, and debug pools are each a fixed set of cube
//! entities spawned once at the hidden pose, tagged with the material variant
//! the per-tick sync assigns them by. Split out of [`super`] so the scene
//! install stays a readable list of what the scene contains.

use axiom::prelude::{Entity, Handle, Material, Mesh, RunningApp, Spawn};

use crate::debug::DebugMaterial;
use crate::presentation::chalk::{ChalkMaterial, CHALK_LINE_POOL, CHALK_PRIMARY_POOL};
use crate::presentation::particles::EffectMaterial;
use crate::presentation::receiver_ring::{
    RingKind, ELIGIBLE_RING_POOL, RECEIVER_RING_POOL, TARGET_RING_POOL,
};

use super::{color3, hidden, JUICE_POOL};

/// Hard bound on debug marker instances.
const DEBUG_POOL: usize = 512;

/// Spawn `count` hidden cubes of `handle`, each tagged with `tag`.
fn fill<T: Copy>(
    app: &mut RunningApp,
    cube: Handle<Mesh>,
    plan: &[(T, usize, Handle<Material>)],
    capacity: usize,
) -> Vec<(Entity, T)> {
    let mut pool = Vec::with_capacity(capacity);
    for &(tag, count, handle) in plan {
        for _ in 0..count {
            pool.push((app.spawn(Spawn::new(hidden(), cube, handle)), tag));
        }
    }
    pool
}

/// Pre-snap route chalk: cyan dots for the routes (distinct from the white yard
/// lines), volt dots for the primary read — the field twin of the huddle
/// chalkboard.
pub(super) fn chalk(app: &mut RunningApp, cube: Handle<Mesh>) -> Vec<(Entity, ChalkMaterial)> {
    let line = app.add_material(Material::lit(color3([0.20, 0.82, 0.98])));
    let primary = app.add_material(Material::lit(color3([0.78, 0.99, 0.20])));
    let plan = [
        (ChalkMaterial::Line, CHALK_LINE_POOL, line),
        (ChalkMaterial::Primary, CHALK_PRIMARY_POOL, primary),
    ];
    fill(app, cube, &plan, CHALK_LINE_POOL + CHALK_PRIMARY_POOL)
}

/// Event-driven juice: dust, impact rings, speed streaks, catch flash, ball
/// trail — bounded per kind.
pub(super) fn juice(app: &mut RunningApp, cube: Handle<Mesh>) -> Vec<(Entity, EffectMaterial)> {
    let dust = app.add_material(Material::lit(color3([0.62, 0.54, 0.38])));
    let ring = app.add_material(Material::lit(color3([0.95, 0.94, 0.86])));
    let streak = app.add_material(Material::lit(color3([0.98, 0.98, 0.99])));
    let flash = app.add_material(Material::lit(color3([1.0, 0.92, 0.45])));
    let trail = app.add_material(Material::lit(color3([0.85, 0.62, 0.30])));
    let plan = [
        (EffectMaterial::Dust, 96, dust),
        (EffectMaterial::Ring, 24, ring),
        (EffectMaterial::Streak, 24, streak),
        (EffectMaterial::Flash, 8, flash),
        (EffectMaterial::Trail, 16, trail),
    ];
    fill(app, cube, &plan, JUICE_POOL)
}

/// Receiver rings: RED on the current read (where the pass would go), white on
/// the other receivers the quarterback could legally reach.
pub(super) fn receiver_rings(
    app: &mut RunningApp,
    cube: Handle<Mesh>,
) -> Vec<(Entity, RingKind)> {
    let target = app.add_material(Material::lit(color3([0.96, 0.16, 0.14])));
    let eligible = app.add_material(Material::lit(color3([0.97, 0.98, 0.97])));
    let plan = [
        (RingKind::Target, TARGET_RING_POOL, target),
        (RingKind::Eligible, ELIGIBLE_RING_POOL, eligible),
    ];
    fill(app, cube, &plan, RECEIVER_RING_POOL)
}

/// Diagnostic markers (F1 overlay): routes, steering targets, collision circles,
/// catch volumes, trajectory, camera aim, foot locks, and the biomechanics view.
pub(super) fn debug(app: &mut RunningApp, cube: Handle<Mesh>) -> Vec<(Entity, DebugMaterial)> {
    let route = app.add_material(Material::lit(color3([0.15, 0.85, 0.95])));
    let target = app.add_material(Material::lit(color3([0.95, 0.25, 0.85])));
    let collision = app.add_material(Material::lit(color3([0.25, 0.95, 0.35])));
    let catch = app.add_material(Material::lit(color3([0.98, 0.62, 0.15])));
    let trajectory = app.add_material(Material::lit(color3([0.98, 0.92, 0.20])));
    let camera = app.add_material(Material::lit(color3([0.95, 0.15, 0.15])));
    let foot_lock = app.add_material(Material::lit(color3([1.0, 0.35, 0.15])));
    let foot_now = app.add_material(Material::lit(color3([0.20, 0.60, 1.0])));
    let foot_land = app.add_material(Material::lit(color3([0.55, 1.0, 0.30])));
    let move_vec = app.add_material(Material::lit(color3([1.0, 1.0, 1.0])));
    // Biomechanical debug view: the three roots, the weight point, the stance foot.
    let gameplay_root = app.add_material(Material::lit(color3([0.10, 0.10, 0.10])));
    let visual_root = app.add_material(Material::lit(color3([0.95, 0.95, 0.30])));
    let pelvis = app.add_material(Material::lit(color3([1.0, 0.30, 0.75])));
    let weight = app.add_material(Material::lit(color3([0.45, 0.20, 0.95])));
    let stance_foot = app.add_material(Material::lit(color3([1.0, 0.55, 0.05])));
    let plan = [
        (DebugMaterial::Route, 100, route),
        (DebugMaterial::Target, 24, target),
        (DebugMaterial::Collision, 128, collision),
        (DebugMaterial::CatchVolume, 16, catch),
        (DebugMaterial::Trajectory, 40, trajectory),
        (DebugMaterial::CameraAim, 12, camera),
        (DebugMaterial::FootLock, 14, foot_lock),
        (DebugMaterial::FootNow, 28, foot_now),
        (DebugMaterial::FootLanding, 14, foot_land),
        (DebugMaterial::MoveVector, 56, move_vec),
        (DebugMaterial::GameplayRoot, 14, gameplay_root),
        (DebugMaterial::VisualRoot, 14, visual_root),
        (DebugMaterial::Pelvis, 14, pelvis),
        (DebugMaterial::WeightPoint, 14, weight),
        (DebugMaterial::StanceFoot, 14, stance_foot),
    ];
    fill(app, cube, &plan, DEBUG_POOL)
}
