//! The concentric closed rings the dogs walk, and how far round its own ring
//! each dog is on a given tick.
//!
//! ## The path
//!
//! One `Curve::catmull_rom` per ring, sampled once into an **arc-length table**
//! and never re-sampled. A spline's parameter is not proportional to its length,
//! so a walker advanced by parameter would speed up and slow down for no reason
//! a viewer could see; `sample_uniform` inverts that once, at startup, and every
//! later lookup is two table reads and a lerp.
//!
//! Catmull-Rom reaches only its *interior* control points, so a closed ring is
//! authored by walking one full revolution and then repeating a point at each
//! end as a shaping handle. The seam is therefore interpolated by the same rule
//! as every other span — the ring joins smoothly rather than meeting at a
//! corner.
//!
//! ## Which way each ring is walked
//!
//! The control points are laid out at `sign · index / N` of a turn, where the
//! sign is the ring's [`Winding`]. Reversing the sign reverses the authored
//! parameter direction, which reverses the tangent, which reverses the facing —
//! there is no separate "turn the dog around" step anywhere, and so no second
//! place for the two to disagree. See `rings.rs` for which sign is which way.
//!
//! ## Spacing, and why it is measured rather than authored
//!
//! A dog's start offset is `slot · total / count`, where `total` is the ring's
//! **measured** length. Deriving it from the ideal circumference instead would
//! leave the last gap short by whatever the terrain's relief and the spline's
//! sampling added, and that error would sit at the seam where it is most
//! visible. Measured, the chain closes exactly.
//!
//! ## Determinism
//!
//! Travel is `tick × TRAVEL_PER_TICK` plus the dog's own fixed offset. Not
//! elapsed time, not an accumulator, not a wall clock: a pure function of the
//! engine tick the frame closure is handed. Tick `N` therefore produces exactly
//! one pose per dog, in this process and the next one, and the whole animation
//! is replayable by counting.

use axiom_math::{Curve, Transform, Vec3};
use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

use crate::creature_dog::dog_limbs;
use crate::creature_pose::{CreaturePose, DOG_GAIT};
use crate::creature_rig::{CreatureRig, LimbChain};
use crate::rings::{ring_dogs, Ring, RingDog, Winding, RINGS};
use crate::terrain::ground_y;

/// How many control points one revolution is authored from, and how many
/// arc-length samples the table holds. 16 controls give a circle no viewer can
/// tell from round; 768 samples put the table's steps well under a fifth of a
/// unit on either ring, far under the length of anything that rides it.
const LOOP_CONTROLS: u32 = 16;
const LOOP_SAMPLES: u32 = 768;

/// World units a dog covers per engine tick. At 60 Hz that is ~37 units a
/// second, or a shade under two body lengths — a trot, not a sprint.
pub const TRAVEL_PER_TICK: f32 = 0.62;

/// A point on a ring: where it is, which way the walk runs, and which way is
/// right.
#[derive(Debug, Clone, Copy)]
pub struct PathPoint {
    /// The point on the terrain, at ground height.
    pub position: Vec3,
    /// The unit direction of travel.
    pub forward: Vec3,
    /// The unit horizontal right of the direction of travel.
    pub right: Vec3,
}

/// One closed ring, pre-inverted into an arc-length table.
#[derive(Debug, Clone)]
pub struct LoopPath {
    /// Horizontal positions, equally spaced by arc length.
    positions: Vec<Vec3>,
    /// The unit tangent at each position.
    tangents: Vec<Vec3>,
    /// The ring's measured length.
    total: f32,
}

impl LoopPath {
    /// The walk around one [`Ring`], at its radius and in its winding.
    pub fn ring(ring: Ring) -> MeshResult<LoopPath> {
        LoopPath::circle(ring.radius, ring.winding())
    }

    /// A closed circular walk of `radius`, authored in the given winding.
    pub fn circle(radius: f32, winding: Winding) -> MeshResult<LoopPath> {
        let controls: Vec<Vec3> = (-1..=(LOOP_CONTROLS as i32 + 1))
            .map(|index| {
                let angle = winding.sign() * index as f32 / LOOP_CONTROLS as f32
                    * core::f32::consts::TAU;
                Vec3::new(radius * angle.cos(), 0.0, radius * angle.sin())
            })
            .collect();
        let curve = Curve::catmull_rom(controls).map_err(|_| {
            MeshError::new(
                MeshErrorCode::InvalidPath,
                "the authored ring is a valid Catmull-Rom curve",
            )
        })?;
        let samples = curve.sample_uniform(LOOP_SAMPLES).map_err(|_| {
            MeshError::new(
                MeshErrorCode::InvalidPath,
                "the ring admits a uniform arc-length sampling",
            )
        })?;
        let total = samples
            .last()
            .map(|sample| sample.distance().get())
            .filter(|length| *length > 1.0)
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidPath,
                    "the ring has a measurable length",
                )
            })?;
        Ok(LoopPath {
            positions: samples.iter().map(|sample| sample.position()).collect(),
            tangents: samples.iter().map(|sample| sample.tangent()).collect(),
            total,
        })
    }

    /// The ring's measured length.
    pub fn total(&self) -> f32 {
        self.total
    }

    /// The point `arc` units along the ring, wrapping — the ring is closed, so
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

/// How far the lead dog of a ring has walked at `tick`.
pub fn dog_travel(tick: u64) -> f32 {
    tick as f32 * TRAVEL_PER_TICK
}

/// Every ring of dogs, walking.
///
/// Engine-free on purpose: this produces **transforms**, and the app's install
/// step is what knows which scene node each one belongs to. That is what lets
/// the whole animation — paths, gait, inverse kinematics, determinism — be
/// tested natively without a browser or a GPU.
///
/// One rig, one set of limb chains, one gait — for every dog in the crowd. The
/// animation is exactly as parameterised as the geometry is: a dog differs from
/// its neighbour by *where on which ring it is*, and by nothing else.
#[derive(Debug, Clone)]
pub struct CrucibleAnimation {
    /// One walk per entry in [`RINGS`], innermost first.
    paths: Vec<LoopPath>,
    /// Every dog, in the order [`Self::transforms`] emits them.
    dogs: Vec<RingDog>,
    /// The single rig every dog instances.
    rig: CreatureRig,
    /// Its four solvable legs.
    limbs: [LimbChain; 4],
    /// The trot they all walk.
    gait: CreaturePose,
}

impl CrucibleAnimation {
    /// Bind an animation to the rig the scene registered.
    pub fn new(dog: CreatureRig) -> MeshResult<CrucibleAnimation> {
        Ok(CrucibleAnimation {
            paths: RINGS
                .iter()
                .map(|ring| LoopPath::ring(*ring))
                .collect::<MeshResult<Vec<LoopPath>>>()?,
            dogs: ring_dogs(),
            rig: dog,
            limbs: dog_limbs(),
            gait: DOG_GAIT,
        })
    }

    /// Every dog, in the order their bones come back from
    /// [`Self::transforms`].
    pub fn dogs(&self) -> &[RingDog] {
        &self.dogs
    }

    /// The walk around one ring, indexed as [`RINGS`] is.
    pub fn path(&self, ring: usize) -> &LoopPath {
        &self.paths[ring.min(self.paths.len() - 1)]
    }

    /// How many bones one dog has — every dog has the same ones, in the same
    /// order, because there is only one rig.
    pub fn bone_count(&self) -> usize {
        self.rig.len()
    }

    /// How many dogs are walking.
    pub fn dog_count(&self) -> usize {
        self.dogs.len()
    }

    /// How far round its own ring dog `index` is at `tick`: the shared travel
    /// plus its own fixed place in the chain.
    ///
    /// The offset is a whole number of *slots* of the ring's measured length, so
    /// the chain is evenly spaced and closes exactly at the seam. It also gives
    /// every dog a different point in the gait cycle for free — the stride is
    /// 5.2 units and the slots are ~25.5 apart, so the legs run as a wave around
    /// the ring instead of stamping in lockstep.
    pub fn travel(&self, index: usize, tick: u64) -> f32 {
        self.dogs
            .get(index)
            .map(|dog| {
                let ring = RINGS[dog.ring];
                dog_travel(tick) + dog.slot as f32 * self.path(dog.ring).total() / ring.count() as f32
            })
            .unwrap_or_else(|| dog_travel(tick))
    }

    /// Every bone of every dog at `tick`: dog by dog in [`Self::dogs`] order,
    /// each dog's bones in rig order.
    pub fn transforms(&self, tick: u64) -> Vec<Transform> {
        self.dogs
            .iter()
            .enumerate()
            .flat_map(|(index, dog)| {
                self.gait.pose(
                    &self.rig,
                    &self.limbs,
                    self.path(dog.ring),
                    self.travel(index, tick),
                )
            })
            .collect()
    }
}
