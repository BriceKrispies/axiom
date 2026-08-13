//! The closed loop the two creatures run, and how far along it they are on a
//! given tick.
//!
//! ## The path
//!
//! One `Curve::catmull_rom` around the scene at [`LOOP_RADIUS`], sampled once
//! into an **arc-length table** and never re-sampled. A spline's parameter is
//! not proportional to its length, so a runner advanced by parameter would
//! speed up and slow down for no reason a viewer could see; `sample_uniform`
//! inverts that once, at startup, and every later lookup is two table reads and
//! a lerp.
//!
//! Catmull-Rom reaches only its *interior* control points, so a closed loop is
//! authored by walking one full revolution and then repeating a point at each
//! end as a shaping handle. The seam is therefore interpolated by the same rule
//! as every other span — the loop joins smoothly rather than meeting at a
//! corner.
//!
//! ## Where the loop is, and why there
//!
//! It rings the whole scene at a radius that clears everything standing in it:
//! the road spans `z = -66 .. 72` and never leaves `|x| < 30`; the primitive row
//! sits at `z = -74` inside `|x| < 38`; the LOD ladder at `z = -60`; the
//! building at `(-44, -16)` and the sculpture at `(44, 8)` are both under 47
//! from the origin; the trees stand within 15.5 of the road. A loop at 86 clears
//! the lot — its nearest approach to the road is the road's far end at
//! `(-2, 72)`, 14 units away — and still sits 10 inside the terrain's 96
//! half-extent, so the runners are on ground the heightfield actually covers on
//! every span. `tests/locomotion.rs` measures both clearances against the real
//! road curve rather than a bounding box.
//!
//! ## Determinism
//!
//! Travel is `tick × TRAVEL_PER_TICK`. Not elapsed time, not an accumulator, not
//! a wall clock: a pure function of the engine tick the frame closure is handed.
//! Tick `N` therefore produces exactly one pose, in this process and the next
//! one, and the whole animation is replayable by counting.

use axiom_math::{Curve, Transform, Vec3};
use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

use crate::creature_dog::dog_limbs;
use crate::creature_human::human_limbs;
use crate::creature_pose::{CreaturePose, DOG_GAIT, HUMAN_GAIT};
use crate::creature_rig::{CreatureRig, LimbChain};
use crate::terrain::ground_y;

/// How far from the scene origin the loop runs.
pub const LOOP_RADIUS: f32 = 86.0;

/// How many control points one revolution is authored from, and how many
/// arc-length samples the table holds. 16 controls give a circle no viewer can
/// tell from round; 768 samples put the table's steps ~0.67 units apart, well
/// under the length of anything that rides it.
const LOOP_CONTROLS: u32 = 16;
const LOOP_SAMPLES: u32 = 768;

/// World units the dog covers per engine tick. At 60 Hz that is ~37 units a
/// second, or roughly 15 seconds a lap.
pub const TRAVEL_PER_TICK: f32 = 0.62;

/// How far behind the dog the human runs, measured **along the loop** rather
/// than as a straight line — so the human genuinely tracks the same path
/// through every bend instead of cutting the corners.
pub const HUMAN_LAG: f32 = 34.1;

/// A point on the loop: where it is, which way it runs, and which way is right.
#[derive(Debug, Clone, Copy)]
pub struct PathPoint {
    /// The point on the terrain, at ground height.
    pub position: Vec3,
    /// The unit direction of travel.
    pub forward: Vec3,
    /// The unit horizontal right of the direction of travel.
    pub right: Vec3,
}

/// The closed loop, pre-inverted into an arc-length table.
#[derive(Debug, Clone)]
pub struct LoopPath {
    /// Horizontal positions, equally spaced by arc length.
    positions: Vec<Vec3>,
    /// The unit tangent at each position.
    tangents: Vec<Vec3>,
    /// The loop's measured length.
    total: f32,
}

impl LoopPath {
    /// The scene-perimeter loop.
    pub fn perimeter() -> MeshResult<LoopPath> {
        let controls: Vec<Vec3> = (-1..=(LOOP_CONTROLS as i32 + 1))
            .map(|index| {
                let angle =
                    index as f32 / LOOP_CONTROLS as f32 * core::f32::consts::TAU;
                Vec3::new(LOOP_RADIUS * angle.cos(), 0.0, LOOP_RADIUS * angle.sin())
            })
            .collect();
        let curve = Curve::catmull_rom(controls).map_err(|_| {
            MeshError::new(
                MeshErrorCode::InvalidPath,
                "the authored perimeter loop is a valid Catmull-Rom curve",
            )
        })?;
        let samples = curve.sample_uniform(LOOP_SAMPLES).map_err(|_| {
            MeshError::new(
                MeshErrorCode::InvalidPath,
                "the perimeter loop admits a uniform arc-length sampling",
            )
        })?;
        let total = samples
            .last()
            .map(|sample| sample.distance().get())
            .filter(|length| *length > 1.0)
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidPath,
                    "the perimeter loop has a measurable length",
                )
            })?;
        Ok(LoopPath {
            positions: samples.iter().map(|sample| sample.position()).collect(),
            tangents: samples.iter().map(|sample| sample.tangent()).collect(),
            total,
        })
    }

    /// The loop's measured length.
    pub fn total(&self) -> f32 {
        self.total
    }

    /// The point `arc` units along the loop, wrapping — the loop is closed, so
    /// there is no end to fall off.
    pub fn at(&self, arc: f32) -> PathPoint {
        let steps = (self.positions.len() - 1) as f32;
        let raw = arc.rem_euclid(self.total) / self.total * steps;
        let index = (raw.floor().max(0.0) as usize).min(self.positions.len() - 2);
        let blend = (raw - index as f32).clamp(0.0, 1.0);
        let flat = lerp(self.positions[index], self.positions[index + 1], blend);
        let forward = lerp(self.tangents[index], self.tangents[index + 1], blend)
            .normalize()
            .unwrap_or(Vec3::UNIT_Z);
        PathPoint {
            position: Vec3::new(flat.x, ground_y(flat.x, flat.z), flat.z),
            forward,
            // `look_rotation` sends local `+X` to `forward × up`, so this is
            // exactly the body's own right — the two cannot drift apart.
            right: forward.cross(Vec3::UNIT_Y).normalize().unwrap_or(Vec3::UNIT_X),
        }
    }
}

/// Component-wise linear blend.
fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a.add(b.subtract(a).mul_scalar(t))
}

/// How far along the loop the dog is at `tick`.
pub fn dog_travel(tick: u64) -> f32 {
    tick as f32 * TRAVEL_PER_TICK
}

/// How far along the loop the human is at `tick` — the dog's position, one
/// fixed arc-length lag back.
pub fn human_travel(tick: u64) -> f32 {
    dog_travel(tick) - HUMAN_LAG
}

/// One creature's animation: the bones, the limb chains, and the gait dials.
#[derive(Debug, Clone)]
struct Runner {
    rig: CreatureRig,
    limbs: [LimbChain; 4],
    gait: CreaturePose,
}

/// The dog and the human running the loop.
///
/// Engine-free on purpose: this produces **transforms**, and the app's install
/// step is what knows which scene node each one belongs to. That is what lets
/// the whole animation — path, gait, inverse kinematics, determinism — be
/// tested natively without a browser or a GPU.
#[derive(Debug, Clone)]
pub struct CrucibleAnimation {
    path: LoopPath,
    dog: Runner,
    human: Runner,
}

impl CrucibleAnimation {
    /// Bind an animation to the two rigs the scene spawned.
    pub fn new(dog: CreatureRig, human: CreatureRig) -> MeshResult<CrucibleAnimation> {
        Ok(CrucibleAnimation {
            path: LoopPath::perimeter()?,
            dog: Runner {
                rig: dog,
                limbs: dog_limbs(),
                gait: DOG_GAIT,
            },
            human: Runner {
                rig: human,
                limbs: human_limbs(),
                gait: HUMAN_GAIT,
            },
        })
    }

    /// The loop both creatures run.
    pub fn path(&self) -> &LoopPath {
        &self.path
    }

    /// How many bones the dog has — the first `dog_bone_count()` transforms
    /// [`Self::transforms`] returns are its.
    pub fn dog_bone_count(&self) -> usize {
        self.dog.rig.len()
    }

    /// How many bones the human has.
    pub fn human_bone_count(&self) -> usize {
        self.human.rig.len()
    }

    /// Every bone's world transform at `tick`: the dog's bones in rig order,
    /// then the human's.
    pub fn transforms(&self, tick: u64) -> Vec<Transform> {
        let mut all = self.dog.gait.pose(
            &self.dog.rig,
            &self.dog.limbs,
            &self.path,
            dog_travel(tick),
        );
        all.extend(self.human.gait.pose(
            &self.human.rig,
            &self.human.limbs,
            &self.path,
            human_travel(tick),
        ));
        all
    }
}
