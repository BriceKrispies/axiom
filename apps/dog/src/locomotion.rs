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

use crate::config::SceneConfig;
use crate::creature_dog::dog_limbs;
use crate::creature_pose::Gait;
use crate::creature_rig::{CreatureRig, LimbChain};
use crate::herd::Herd;
use crate::rings::{ring_dogs, rings, Ring, RingDog, Winding};
use crate::terrain::ground_y;

/// How many control points one revolution is authored from, and how many
/// arc-length samples the table holds. 16 controls give a circle no viewer can
/// tell from round; 768 samples put the table's steps well under a fifth of a
/// unit on either ring, far under the length of anything that rides it.
const LOOP_CONTROLS: u32 = 16;
const LOOP_SAMPLES: u32 = 768;

/// World units a dog covers per engine tick, at the walk-speed dial's default.
///
/// This is set by the STEP RATE it implies, not by the ground speed it looks
/// like. A leg completes one cycle every `stride / speed` ticks, so at 60 Hz the
/// step frequency is `60 · speed / stride`. With the dachshund's 5.2 stride, 0.21
/// gives ~2.4 steps a second per leg, which is a real dog's trot; an earlier 0.62
/// implied 7.2 Hz, and a leg cycling seven times a second reads as vibration
/// rather than walking.
///
/// Ground speed follows from that at ~12.6 units a second, close to half a body
/// length — an unhurried walk. Moving the dial does NOT change the stride, so the
/// limb-reach budget is untouched: it rescales time, not geometry, which is
/// exactly why it is the cheapest dial on the panel and the one that reads
/// instantly.
pub const DEFAULT_TRAVEL_PER_TICK: f32 = 0.21;

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
    /// The walk around one [`Ring`], at its radius and in the field's winding.
    pub fn ring(ring: Ring, winding: Winding) -> MeshResult<LoopPath> {
        LoopPath::circle(ring.radius, winding)
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

/// How far the lead dog of a ring has walked at `tick`, at `speed` units a tick.
pub fn dog_travel(tick: u64, speed: f32) -> f32 {
    tick as f32 * speed
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
pub struct Animation {
    /// One walk per ring the configuration lays out, innermost first.
    paths: Vec<LoopPath>,
    /// The rings those walks were built from — so a dog's ring index resolves to
    /// the same circle the path was sampled off.
    rings: Vec<Ring>,
    /// Every dog, in the order [`Self::transforms`] emits them.
    dogs: Vec<RingDog>,
    /// How many dogs each ring's chain holds — the divisor that spaces the chain
    /// evenly and closes it exactly at the seam. Counted off the crowd rather
    /// than re-derived, so a ring truncated by the instance pool still spaces the
    /// dogs it kept.
    chain: Vec<usize>,
    /// The single rig every dog instances.
    rig: CreatureRig,
    /// Its four solvable legs.
    limbs: [LimbChain; 4],
    /// The trot they all walk, with every dial resolved.
    gait: Gait,
    /// World units a dog covers per tick.
    speed: f32,
}

impl Animation {
    /// Bind an animation to the rig the scene registered, at `config`.
    ///
    /// This is where the layout dials are paid for: the arc-length tables are
    /// inverted once, here, and every later lookup is two table reads and a lerp.
    /// Moving a *layout* dial rebuilds this value; moving a gait dial does not
    /// (see [`Animation::follows`]).
    pub fn new(dog: CreatureRig, config: &SceneConfig) -> MeshResult<Animation> {
        let laid = rings(config);
        let dogs = ring_dogs(config);
        Ok(Animation {
            paths: laid
                .iter()
                .map(|ring| LoopPath::ring(*ring, config.winding()))
                .collect::<MeshResult<Vec<LoopPath>>>()?,
            chain: laid
                .iter()
                .map(|ring| dogs.iter().filter(|dog| dog.ring == ring.index).count().max(1))
                .collect(),
            rings: laid,
            dogs,
            rig: dog,
            limbs: dog_limbs(),
            gait: config.gait(),
            speed: config.travel_per_tick(),
        })
    }

    /// Re-resolve the dials that do **not** move a ring: the gait and the walking
    /// speed. Everything here is read per pose, so it costs nothing to change —
    /// which is the difference between a dial that reads instantly and one that
    /// re-inverts eight arc-length tables.
    pub fn retune(&mut self, config: &SceneConfig) {
        self.gait = config.gait();
        self.speed = config.travel_per_tick();
    }

    /// Whether this animation's rings are the ones `config` asks for. False means
    /// the paths have to be rebuilt; true means [`Animation::retune`] is enough.
    pub fn follows(&self, config: &SceneConfig) -> bool {
        (self.rings == rings(config)) & (self.dogs == ring_dogs(config))
    }

    /// Every dog, in the order their bones come back from
    /// [`Self::transforms`].
    pub fn dogs(&self) -> &[RingDog] {
        &self.dogs
    }

    /// The walk around one ring, indexed as [`Animation::rings`] is.
    pub fn path(&self, ring: usize) -> &LoopPath {
        &self.paths[ring.min(self.paths.len() - 1)]
    }

    /// The one rig every dog instances — handed back so a layout rebuild reuses
    /// the bones the scene registered instead of re-deriving them.
    pub fn rig(&self) -> &CreatureRig {
        &self.rig
    }

    /// The rings this animation walks.
    pub fn rings(&self) -> &[Ring] {
        &self.rings
    }

    /// The trot every dog walks, with every gait dial resolved.
    pub fn gait(&self) -> Gait {
        self.gait
    }

    /// World units a dog covers per tick.
    pub fn speed(&self) -> f32 {
        self.speed
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
                let chain = self.chain.get(dog.ring).copied().unwrap_or(1);
                dog_travel(tick, self.speed)
                    + dog.slot as f32 * self.path(dog.ring).total() / chain as f32
            })
            .unwrap_or_else(|| dog_travel(tick, self.speed))
    }

    /// Where the walk puts each dog at `tick`, and which way it is facing — one
    /// [`PathPoint`] per dog, in [`Self::dogs`] order.
    ///
    /// This is the **anchor** the disturbance in `src/herd.rs` is measured
    /// against: the place a dog would be standing if nobody had touched it. It
    /// is read straight off the same arc-length tables the pose is, so a dog's
    /// anchor and its pose can never disagree about which ring it is on.
    ///
    /// The heading comes with it because a dog collides as a body laid *along*
    /// its ring, not as a circle around its middle — the capsule needs to know
    /// which way the animal is pointing, and this is the one place that already
    /// knows.
    pub fn anchors(&self, tick: u64) -> Vec<PathPoint> {
        self.dogs
            .iter()
            .enumerate()
            .map(|(index, dog)| self.path(dog.ring).at(self.travel(index, tick)))
            .collect()
    }

    /// Every bone of every dog at `tick`: dog by dog in [`Self::dogs`] order,
    /// each dog's bones in rig order.
    ///
    /// The walk as the rings define it, with nothing on top — a pure function of
    /// the tick and the configuration.
    pub fn transforms(&self, tick: u64) -> Vec<Transform> {
        self.displaced(tick, &Herd::undisturbed())
    }

    /// The same walk, drawn where the crowd currently *stands*: every dog's
    /// bones slid by whatever displacement it is carrying.
    ///
    /// A whole dog moves as one rigid piece, because its pose was resolved for
    /// its anchor — planted paws, hip heights, terrain and all. Sliding the
    /// finished bones keeps that pose internally consistent and keeps the
    /// disturbance out of the gait entirely, which is the property that lets a
    /// released dog come back *in step* rather than merely come back.
    ///
    /// An undisturbed crowd takes the fast path and returns exactly what
    /// [`Self::transforms`] does — the same floats, not merely equal ones.
    pub fn displaced(&self, tick: u64, herd: &Herd) -> Vec<Transform> {
        let moved = herd.disturbed();
        self.dogs
            .iter()
            .enumerate()
            .flat_map(|(index, dog)| {
                let bones = self.gait.pose(
                    &self.rig,
                    &self.limbs,
                    self.path(dog.ring),
                    self.travel(index, tick),
                );
                let slide = [Vec3::ZERO, herd.displacement(index)][usize::from(moved)];
                bones.into_iter().map(move |bone| Transform {
                    translation: bone.translation.add(slide),
                    ..bone
                })
            })
            .collect()
    }
}
