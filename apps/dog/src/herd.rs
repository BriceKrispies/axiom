//! The crowd as something you can put your hand into: which dog you are holding,
//! how far each one has been knocked off its track, and how it gets back.
//!
//! # The track stays the authority
//!
//! This module does **not** take the dogs off their rings. A dragged dog is
//! still walking its own ring at its own arc-length offset, with the same gait
//! phase it would have had if nobody had touched it — what changes is that it is
//! *drawn somewhere else*. A dog carries a horizontal **displacement** from the
//! place the walk put it, and that is the entirety of the state here.
//!
//! Everything the feature has to promise falls out of that one choice:
//!
//! * **it goes back.** The displacement decays toward zero, so the dog converges
//!   on its own anchor — not on a plausible place near it.
//! * **it goes back *in sync*.** Its travel, its slot in the chain and its point
//!   in the trot were never touched, so there is nothing to resynchronise. The
//!   moment the displacement reaches zero the dog is bit-for-bit the dog the
//!   undisturbed field would have drawn, in step with every other. A controller
//!   that steered a free-moving dog back onto the ring could only ever
//!   *approach* that, and would have to chase the phase as well as the place.
//! * **it cannot be lost.** There is no state to corrupt: a displacement is a
//!   number that is being multiplied toward zero every tick, and
//!   [`Herd::calm`] sets it there directly.
//!
//! The alternative — hand the dogs to a physics solver and let them find their
//! own way home — would be a much larger machine that is *worse at the actual
//! requirement*, because "the same sync" is a statement about phase, and phase is
//! precisely what a free body loses first.
//!
//! # Collision, and why the dogs are capsules
//!
//! A dog holds the others off with its **body**: the [`CrowdSpace`] capsule laid
//! along its own heading, near enough 24 units long and 3.8 across at the
//! authored scale. Overlapping pairs are pushed apart along the shortest line
//! between their two spines, half the overlap each, [`SETTLING_PASSES`] times a
//! frame. The dog in your hand is immovable, so you can plough it through the
//! field and shoulder the rest aside — and that shove *spreads*, dog to dog,
//! down the chain it is driven into.
//!
//! One rule governs the whole thing: **the crowd is solid under your hand, and
//! permeable to a dog going home.** A push carrying the user's authority lands in
//! full; a push between two dogs that are both merely settling may turn a dog and
//! slide it around an obstacle, but may never carry it further from its own
//! anchor. That asymmetry is what makes the return unconditional rather than
//! usual, and [`Herd::shoved`] carries the argument and the measurements.
//!
//! It is worth being explicit about why this is not a circle. A circle that fits
//! between two rings 7.75 apart has a radius of about 3.5 — a seventh of the
//! animal's length — so a dog dragged along a ring passes clean *through* its
//! neighbours and only touches when the two centres nearly coincide. That reads
//! as no collision at all. The fix is not a bigger radius, because a circle big
//! enough to be the dog is far too big to fit between the rings: it is the wrong
//! *shape*, and the right one is the one the animal already has.
//!
//! The size is derived, in `rings.rs`, from the gap the layout actually left —
//! which is what makes the crowd *at rest* provably out of contact. That is not
//! a detail: a field whose dogs were already overlapping would have the push
//! fighting the return forever and would never settle. `tests/herd.rs` asserts
//! it against the real posed positions, and `rings.rs` asserts the arithmetic.
//!
//! The pass is `n²` over the crowd — at most 162 dogs, so ~13k segment distances
//! per pass, next to the 2392 bone transforms the same frame writes. A grid would
//! be faster and would need to be justified by a measurement nobody has taken.
//!
//! # Where it sits
//!
//! Engine-free and browser-free, like `locomotion.rs`: it takes positions and a
//! [`Ray`] and produces offsets, so the whole of it is tested natively. The
//! pointer events that drive it are in `src/pointer_input.rs`, and the frame that
//! feeds and applies it is in `src/install.rs`.

use axiom_math::Vec3;

use crate::locomotion::PathPoint;
use crate::orbit::Ray;
use crate::rings::CrowdSpace;
use crate::terrain::ground_y;

/// The share of its displacement a dog keeps after one tick.
///
/// Per **tick**, not per frame: the walk is measured in ticks (see
/// `locomotion.rs`), and a return rate measured in frames would mean a dog that
/// drifts home at a different speed on a 120 Hz screen than on a 60 Hz one.
///
/// At 0.98 a displacement is half gone in 34 ticks — a bit over half a second —
/// and a dog hauled the width of the field is home inside five. The tail of an
/// exponential is the part that decides how this reads: a gentler 0.986 spends
/// ten seconds finishing the last unit of a long drag, which stops looking like
/// a dog trotting back to its place and starts looking like one stuck.
const KEPT_PER_TICK: f32 = 0.98;

/// Below this, in world units, the rest of the displacement is given back at
/// once.
///
/// An exponential decay never actually arrives, and "never actually arrives" is
/// the difference between a dog that is back in the field and a dog that is
/// permanently a hundredth of a unit out of it. This is the point at which the
/// dog is put exactly on its anchor, so the undisturbed state is one the field
/// really returns to and `tests/herd.rs` can assert on the nose.
const SETTLED: f32 = 0.01;

/// How generously a pointer has to land on a dog to catch it, as a multiple of
/// the dog's own half-width — measured from its whole **body**, not from its
/// centre, so anywhere along the animal catches it.
///
/// Comfortably over 1: a fingertip on a phone covers more of the screen than a
/// dachshund's flank does, and a grab that misses reads as the feature being
/// broken rather than as the user being imprecise.
const GRAB_REACH: f32 = 2.2;

/// How many times the separation is relaxed per frame.
///
/// One pass resolves each overlapping pair, but moving a dog out of one
/// neighbour can push it into the next, so a crowd being ploughed through comes
/// apart over several passes. Doing them within the frame instead of across
/// frames is the difference between the field *parting* around a dragged dog and
/// the field slowly oozing out of it. Three is where it stops looking like a
/// queue and starts looking like a crowd; the cost is three `n²` sweeps of
/// segment distances, which at this scale is not the frame's problem.
const SETTLING_PASSES: usize = 3;

/// The dog currently in the user's hand.
#[derive(Debug, Clone, Copy)]
struct Hold {
    /// Which dog, as an index into the crowd.
    dog: usize,
    /// The world point it is being held at. Stored as a *position*, not as a
    /// displacement, because its anchor keeps walking underneath it: a held dog
    /// stays where the pointer put it while its own place on the ring travels on
    /// without it, and the displacement is re-derived from the two every frame.
    pinned: Vec3,
    /// The height the drag slides along — the dog's own, caught at the grab, so
    /// it tracks the pointer across a plane instead of climbing the terrain.
    plane: f32,
    /// Where on the animal the pointer landed: dog centre minus the point the
    /// grabbing ray met the plane at. Carried through the drag so the dog does
    /// not jump to centre itself under the pointer the instant it is caught.
    grip: Vec3,
}

/// How far each dog in the crowd has been knocked off its track, and which one
/// is being held.
///
/// Deliberately not `Default`: an untouched herd has *unbounded* room, not zero
/// room, until a frame tells it what the layout allows. A derived default would
/// be a second constructor that quietly disagreed with [`Herd::undisturbed`]
/// about that, and the one it disagreed on would contain every dog to the
/// origin.
#[derive(Debug, Clone)]
pub struct Herd {
    /// Where the walk says each dog is and which way it faces, this frame.
    /// Refreshed by [`Herd::settle`] rather than stored: the field is walking,
    /// so an anchor is only true for the tick it was read on.
    anchors: Vec<PathPoint>,
    /// Each dog's horizontal displacement from its anchor. The `y` component is
    /// always zero — the vertical is the terrain's business, resolved in
    /// [`Herd::displacement`].
    offsets: Vec<Vec3>,
    /// The room the crowd was given at the last settle, so a grab does not have
    /// to be told what it already knows.
    space: CrowdSpace,
    /// The dog in the user's hand, if any.
    held: Option<Hold>,
    /// The tick the last settle ran at, so the return is measured in ticks.
    clock: Option<u64>,
    /// Which dogs are part of the shove coming out of the user's hand, this
    /// frame. Held for the duration of a settle and rebuilt at the start of the
    /// next one — a scratch buffer kept on the struct so a frame does not
    /// allocate one.
    ///
    /// The flag *spreads*: a dog shoved by the held dog joins the shove, and can
    /// then shove the next in turn. Without that the hand's force would stop
    /// dead at the first animal it touched, because everyone behind that one is
    /// only allowed to be moved toward home (see [`Herd::shoved`]) — a dragged
    /// dog would nudge one neighbour and slide through the rest of the chain.
    hand_driven: Vec<bool>,
}

impl Herd {
    /// A crowd nobody has touched: every dog exactly where its ring puts it.
    pub fn undisturbed() -> Herd {
        Herd {
            anchors: Vec::new(),
            offsets: Vec::new(),
            space: CrowdSpace {
                half_length: 0.0,
                half_width: 0.0,
                bounds: f32::INFINITY,
            },
            held: None,
            clock: None,
            hand_driven: Vec::new(),
        }
    }

    /// Bring the disturbance up to date for `tick`: adopt this frame's anchors,
    /// give back what has decayed, push overlapping dogs apart, and re-derive
    /// the held dog's displacement from where its ring has walked to.
    ///
    /// The order is deliberate. The return runs first, so a dog that is done is
    /// exactly on its anchor before anything else looks at it. The hold is
    /// applied **before** separation rather than after, so the crowd is pushed
    /// out of the way of where the pointer is *now* instead of trailing a frame
    /// behind it during a fast drag. Containment is last, because it is the one
    /// rule nothing may override: a dog off the terrain has nothing to stand on.
    pub fn settle(&mut self, anchors: &[PathPoint], tick: u64, space: CrowdSpace) {
        let elapsed = self.clock.map_or(0, |last| tick.saturating_sub(last));
        self.clock = Some(tick);
        self.space = space;
        self.anchors.clear();
        self.anchors.extend_from_slice(anchors);
        self.offsets.resize(anchors.len(), Vec3::ZERO);
        self.held = self.held.filter(|hold| hold.dog < anchors.len());
        self.give_back(elapsed);
        self.pin();
        // The shove starts at the hand and spreads from there, over the passes.
        self.hand_driven.clear();
        self.hand_driven
            .resize(anchors.len(), false);
        self.holding()
            .into_iter()
            .for_each(|dog| self.hand_driven[dog] = true);
        (0..SETTLING_PASSES).for_each(|_| self.separate());
        self.contain();
    }

    /// Put every dog back where its ring says, let go of whatever is held, and
    /// forget the crowd entirely.
    ///
    /// This is what the study stage does on the way in (there is one dog on it,
    /// suspended at the origin, and it is not part of a crowd), so the field is
    /// pristine when it comes back.
    ///
    /// The anchors go too, not just the displacements. A stage with no crowd on
    /// it must have nothing to *catch*: leaving the last field's positions
    /// behind would let a press on the study grab a dog that is not on screen,
    /// and the field would come back with an animal pinned to a point the user
    /// never saw. The next field frame refills them.
    pub fn calm(&mut self) {
        self.anchors.clear();
        self.offsets.clear();
        self.hand_driven.clear();
        self.held = None;
    }

    /// The translation dog `index` is drawn at, on top of the pose its ring
    /// gives it: its horizontal displacement, plus the rise or fall of the
    /// terrain between where it should be standing and where it is.
    ///
    /// Without the vertical term a dog dragged across the basin would walk
    /// through the ground on one side of it and above the ground on the other:
    /// its whole pose, paw plants included, was resolved against the height at
    /// its anchor.
    pub fn displacement(&self, index: usize) -> Vec3 {
        self.offsets
            .get(index)
            .zip(self.anchors.get(index))
            .map(|(offset, anchor)| {
                let stood = anchor.position;
                Vec3::new(
                    offset.x,
                    ground_y(stood.x + offset.x, stood.z + offset.z) - stood.y,
                    offset.z,
                )
            })
            .unwrap_or(Vec3::ZERO)
    }

    /// Whether any dog is anywhere other than where its ring puts it. False for
    /// an untouched field, and true again the moment one is grabbed.
    pub fn disturbed(&self) -> bool {
        self.held.is_some() | self.offsets.iter().any(|offset| offset.length() > 0.0)
    }

    /// Which dog is in the user's hand, if any.
    pub fn holding(&self) -> Option<usize> {
        self.held.map(|hold| hold.dog)
    }

    /// Take hold of whichever dog `ray` passes nearest, if it passes near enough
    /// to one. Reports whether it caught anything, which is what the browser
    /// half needs to decide whether this gesture is a drag or the page's own.
    ///
    /// Nearest **to the camera**, not nearest to the ray: pointing into a
    /// crowded field must pick the dog in front, the one the user can actually
    /// see, rather than whichever of the dogs behind it happens to line up best.
    pub fn grab(&mut self, ray: Ray) -> bool {
        let reach = self.space.half_width * GRAB_REACH;
        // The ray is met as a very long segment, so one routine answers both
        // "how close does the pointer pass to this dog" and "how close do these
        // two dogs pass to each other" — a dog is the same body in both cases.
        let far = ray.origin.add(ray.direction.mul_scalar(RAY_LENGTH));
        self.held = (0..self.offsets.len())
            .map(|dog| {
                let (spine, tail) = self.body(dog);
                let (on_ray, on_dog) = closest_between(ray.origin, far, spine, tail);
                (dog, on_ray.distance(ray.origin), on_ray.distance(on_dog))
            })
            .filter(|(_, _, off)| *off <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(dog, _, _)| {
                let position = self.at(dog);
                Hold {
                    dog,
                    pinned: position,
                    plane: position.y,
                    grip: ray
                        .on_plane(position.y)
                        .map(|hit| position.subtract(hit))
                        .unwrap_or(Vec3::ZERO),
                }
            });
        self.held.is_some()
    }

    /// Drag the held dog to wherever `ray` now crosses the plane it was caught
    /// on. Does nothing if nothing is held, or if the pointer has been swung off
    /// the plane entirely (past the horizon).
    pub fn drag(&mut self, ray: Ray) {
        self.held = self.held.map(|hold| Hold {
            pinned: ray
                .on_plane(hold.plane)
                .map(|hit| hit.add(hold.grip))
                .unwrap_or(hold.pinned),
            ..hold
        });
    }

    /// Let go. The dog keeps the displacement it was let go at, and starts
    /// giving it back on the next settle.
    pub fn release(&mut self) {
        self.held = None;
    }

    /// Where dog `index` actually stands, on the flat: its anchor slid by its
    /// horizontal displacement, keeping the anchor's own height.
    ///
    /// Deliberately **not** [`Herd::displacement`] plus the anchor. This is the
    /// position the `n²` separation pass and every grab test read, and the
    /// terrain lookup that the drawn displacement needs is a sum of sines — an
    /// honest few hundred of those a frame at draw time, and a quarter of a
    /// million of them if the pair loop asked for one per dog per neighbour. The
    /// separation is horizontal anyway, so the height it would buy is unused.
    fn at(&self, index: usize) -> Vec3 {
        self.anchors
            .get(index)
            .zip(self.offsets.get(index))
            .map(|(anchor, offset)| {
                Vec3::new(
                    anchor.position.x + offset.x,
                    anchor.position.y,
                    anchor.position.z + offset.z,
                )
            })
            .unwrap_or(Vec3::ZERO)
    }

    /// Dog `index`'s body as the spine of its capsule: the two ends of a segment
    /// through where it stands, laid along the way it is facing and flattened
    /// onto the horizontal.
    ///
    /// Flattened because the collision is a plan-view problem — dogs on a slope
    /// are still side by side — and because keeping it in two dimensions is what
    /// keeps the pair loop cheap.
    fn body(&self, index: usize) -> (Vec3, Vec3) {
        let middle = self.at(index);
        let heading = self
            .anchors
            .get(index)
            .map(|anchor| Vec3::new(anchor.forward.x, 0.0, anchor.forward.z))
            .and_then(|flat| flat.normalize().ok())
            .unwrap_or(Vec3::UNIT_Z)
            .mul_scalar(self.space.half_length);
        (middle.subtract(heading), middle.add(heading))
    }

    /// Decay every free dog's displacement over `elapsed` ticks, and put the
    /// ones that have essentially arrived exactly home.
    fn give_back(&mut self, elapsed: u64) {
        let kept = KEPT_PER_TICK.powi(elapsed.min(i32::MAX as u64) as i32);
        let held = self.holding();
        self.offsets
            .iter_mut()
            .enumerate()
            .filter(|(dog, _)| Some(*dog) != held)
            .for_each(|(_, offset)| {
                let pulled = offset.mul_scalar(kept);
                *offset = [pulled, Vec3::ZERO][usize::from(pulled.length() < SETTLED)];
            });
    }

    /// One relaxation pass: push every overlapping pair apart along the line
    /// between them. A held dog is immovable and its partner takes the whole
    /// push, which is what lets a dragged dog shoulder its way through the field.
    fn separate(&mut self) {
        let span = self.space.half_width * 2.0;
        let held = self.holding();
        let count = self.offsets.len();
        (0..count).for_each(|a| {
            (a + 1..count).for_each(|b| {
                // Body against body, not centre against centre: the shortest
                // line between the two spines is where a dachshund's flank
                // actually meets its neighbour's.
                let (a0, a1) = self.body(a);
                let (b0, b1) = self.body(b);
                let (from, to) = closest_between(a0, a1, b0, b1);
                let gap = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
                let distance = gap.length();
                let overlap = span - distance;
                (overlap > 0.0).then(|| {
                    // Two dogs exactly on top of each other have no line between
                    // them to be pushed along; any axis will do, and the next
                    // pass has a real one to work with.
                    let push = gap
                        .normalize()
                        .unwrap_or(Vec3::UNIT_X)
                        .mul_scalar(overlap);
                    let (share_a, share_b) = Herd::shares(Some(a) == held, Some(b) == held);
                    // Whoever is pushing carries the hand's authority if they
                    // are part of its shove, and passes it to whoever they move.
                    let (from_a, from_b) = (self.hand_driven[a], self.hand_driven[b]);
                    self.offsets[a] =
                        Herd::shoved(self.offsets[a], push.mul_scalar(-share_a), from_b);
                    self.offsets[b] =
                        Herd::shoved(self.offsets[b], push.mul_scalar(share_b), from_a);
                    self.hand_driven[a] = from_a | (from_b & (share_a > 0.0));
                    self.hand_driven[b] = from_b | (from_a & (share_b > 0.0));
                });
            });
        });
    }

    /// How much of a push each of an overlapping pair takes: half each, unless
    /// one of them is the dog in the user's hand, which gives no ground at all.
    fn shares(a_held: bool, b_held: bool) -> (f32, f32) {
        [
            (0.5, 0.5),
            (0.0, 1.0),
            (1.0, 0.0),
            // Both held is unreachable — there is one hold — but a rule with a
            // hole in it is a rule waiting to be broken by a later change.
            (0.0, 0.0),
        ][usize::from(a_held) + 2 * usize::from(b_held)]
    }

    /// Take a push, and report where the dog ends up.
    ///
    /// **The crowd is solid under your hand, and permeable to a dog going
    /// home.** A push from the held dog lands in full — that is the user
    /// ploughing through the field, and the dogs it shoulders aside really are
    /// shoved out of place. A push between two *free* dogs may turn a dog and
    /// slide it around an obstacle, but it may never move it further from its
    /// own anchor than it already was: it is clamped back onto the circle of
    /// constant distance from home.
    ///
    /// That single clamp is what makes the return unconditional, and it is worth
    /// spelling out why nothing weaker will do. The field is a lattice of rings
    /// whose gaps are far narrower than a dachshund, so a dog released in the
    /// middle has to cross several chains to get home, and every one of those
    /// contacts is between two near-parallel bodies with the push straight along
    /// the line between them. Two such dogs needing to swap radially cannot slip
    /// past each other in any amount of time: the geometry is degenerate, there
    /// is no sideways for the solver to find, and the pair simply stands there.
    /// Measured, not deduced — an even split left a dog stranded 31 units out,
    /// and letting the further-from-home dog win left two locked at 6.
    ///
    /// With the clamp there is no equilibrium left to find. Every free dog's
    /// distance from its anchor is non-increasing under separation and shrinks
    /// by [`KEPT_PER_TICK`] under the return, so it reaches zero in bounded time
    /// whatever the crowd does — and the anchors are provably clear of one
    /// another, so the field's only resting state is the one the rings define.
    ///
    /// The price is honest and small: two free dogs whose only way apart is
    /// *away from home* will briefly overlap instead, while one walks through
    /// the other. A dog crossing the field is passing through a crowd, and for a
    /// moment it looks like it. Under the hand, where the collision is the point,
    /// nothing is given up at all.
    fn shoved(from: Vec3, push: Vec3, by_hand: bool) -> Vec3 {
        let moved = from.add(push);
        let ceiling = [from.length(), moved.length()][usize::from(by_hand)];
        moved
            .normalize()
            .map(|direction| direction.mul_scalar(moved.length().min(ceiling)))
            .unwrap_or(moved)
    }

    /// Re-derive the held dog's displacement from the point it is pinned at and
    /// the anchor that has walked on underneath it.
    fn pin(&mut self) {
        self.held
            .and_then(|hold| {
                self.anchors
                    .get(hold.dog)
                    .map(|anchor| (hold.dog, hold.pinned.subtract(anchor.position)))
            })
            .into_iter()
            .for_each(|(dog, offset)| {
                self.offsets[dog] = Vec3::new(offset.x, 0.0, offset.z);
            });
    }

    /// Keep every dog on the ground there is to stand on. A dog dragged at the
    /// rim, or shoved over it by the one in your hand, stops at the edge of the
    /// terrain instead of walking off the plate into the sky.
    fn contain(&mut self) {
        let bounds = self.space.bounds;
        let anchors = &self.anchors;
        self.offsets
            .iter_mut()
            .enumerate()
            .for_each(|(dog, offset)| {
                anchors.get(dog).map(|anchor| {
                    let stood = anchor.position;
                    let placed = Vec3::new(stood.x + offset.x, 0.0, stood.z + offset.z);
                    let reach = placed.length();
                    let held = placed.mul_scalar(bounds / reach.max(1.0e-3));
                    let inside = [held, placed][usize::from(reach <= bounds)];
                    *offset = Vec3::new(inside.x - stood.x, 0.0, inside.z - stood.z);
                });
            });
    }
}

/// How long a pointer ray is treated as being when it is met as a segment.
/// Twice the far plane, so it always outruns the scene and the clamp at its far
/// end can never be the answer.
const RAY_LENGTH: f32 = 1400.0;

/// The closest pair of points on two segments, `(on the first, on the second)`.
///
/// The one piece of real geometry in this module, and the one both the collision
/// and the grab are built from — a dog is a capsule to the pointer for exactly
/// the same reason it is a capsule to its neighbour. Two capsules overlap when
/// this distance is under the sum of their radii, which is the whole of the
/// collision test.
///
/// The parameters along each segment are solved and then clamped to `0..=1`, and
/// the second is re-solved after the first is clamped — that re-solve is what
/// makes the routine exact at the ends, where two dogs meeting nose to tail
/// actually touch. Parallel spines (two dogs abreast on the same ring, the
/// common case) leave the system singular, and the fallback picks an endpoint,
/// which for parallel segments is a correct answer rather than an approximation.
fn closest_between(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> (Vec3, Vec3) {
    const NEARLY_A_POINT: f32 = 1.0e-6;
    let (first, second) = (a1.subtract(a0), b1.subtract(b0));
    let between = a0.subtract(b0);
    let (along_first, along_second) = (first.dot(first), second.dot(second));
    let offset_second = second.dot(between);
    let degenerate = (along_first <= NEARLY_A_POINT, along_second <= NEARLY_A_POINT);
    let offset_first = first.dot(between);
    let skew = first.dot(second);
    let denominator = along_first * along_second - skew * skew;
    // The unclamped solution for the first segment, or its start when the two
    // are parallel and every point is as good as any other.
    let opened = ((skew * offset_second - offset_first * along_second)
        / denominator.max(NEARLY_A_POINT))
    .clamp(0.0, 1.0);
    let first_param = [opened, 0.0][usize::from(denominator <= NEARLY_A_POINT)];
    let second_param = (skew * first_param + offset_second) / along_second.max(NEARLY_A_POINT);
    // Clamping the second forces the first to be solved again against the end it
    // was clamped to.
    let clamped_second = second_param.clamp(0.0, 1.0);
    let resolved_first = ((clamped_second * skew - offset_first) / along_first.max(NEARLY_A_POINT))
        .clamp(0.0, 1.0);
    let (u, v) = match degenerate {
        (true, true) => (0.0, 0.0),
        (true, false) => (0.0, (offset_second / along_second).clamp(0.0, 1.0)),
        (false, true) => ((-offset_first / along_first).clamp(0.0, 1.0), 0.0),
        (false, false) => (resolved_first, clamped_second),
    };
    (
        a0.add(first.mul_scalar(u)),
        b0.add(second.mul_scalar(v)),
    )
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A field of dogs in a row along `X`, four units apart, each **facing
    /// across the row** — so they stand abreast like two ranks of a ring rather
    /// than nose to tail, and the four units between them is measured flank to
    /// flank against [`space`]'s 3.6-unit width.
    ///
    /// Standing on the real terrain, because a real anchor always does: the walk
    /// puts every dog at `ground_y` of its own position, and the drawn
    /// displacement is the *difference* between two ground heights. An anchor
    /// floating at `y = 0` over sloped ground would make an undisturbed dog look
    /// displaced.
    fn row(count: usize) -> Vec<PathPoint> {
        (0..count)
            .map(|dog| dog as f32 * 4.0)
            .map(|x| PathPoint {
                position: Vec3::new(x, ground_y(x, 0.0), 0.0),
                forward: Vec3::UNIT_Z,
                right: Vec3::UNIT_X,
            })
            .collect()
    }

    /// A body that leaves air at rest, exactly as `crowd_space` does for the
    /// real field: 3.6 units across against a 4.0-unit row. A width that made
    /// the row touch *exactly* would have the pair loop trading floating-point
    /// dust with the return every frame, and the crowd would never read as
    /// settled — which is the same reason `CROWD_SHARE` is under a half.
    fn space() -> CrowdSpace {
        CrowdSpace {
            half_length: 6.0,
            half_width: 1.8,
            bounds: 1000.0,
        }
    }

    /// A ray pointing straight down at a world point from well above it.
    fn straight_down(x: f32, z: f32) -> Ray {
        Ray {
            origin: Vec3::new(x, 100.0, z),
            direction: Vec3::new(0.0, -1.0, 0.0),
        }
    }

    #[test]
    fn the_real_field_at_rest_is_never_in_contact_at_any_setting_of_the_dials() {
        // The property the whole collision rests on, measured on the **posed**
        // bodies rather than on the ideal circles `rings.rs` does its arithmetic
        // with: the walk runs through a Catmull-Rom spline over sloped terrain,
        // so a dog does not stand exactly where the layout's trigonometry says.
        // If this ever fails, the crowd hums instead of settling.
        for size in [6.0, 10.0, 16.0] {
            for inner in [18.0, 26.0, 60.0] {
                for pitch in [3.0, 7.75, 20.0] {
                    for gap in [0.5, 1.5, 20.0] {
                        let config = crate::config::SceneConfig::defaults()
                            .with(crate::config::Dial::DogSize, size)
                            .with(crate::config::Dial::InnerRadius, inner)
                            .with(crate::config::Dial::RingSpacing, pitch)
                            .with(crate::config::Dial::DogGap, gap);
                        let animation = crate::locomotion::Animation::new(
                            crate::creature_dog::dog_parts(config.variant()).expect("the dog rigs"),
                            &config,
                        )
                        .expect("the authored rings are valid paths");
                        let space = crate::rings::crowd_space(&config);
                        let mut herd = Herd::undisturbed();
                        // Two ticks: the chain is at a different point of its
                        // own travel on each, over different ground.
                        for tick in [0u64, 617] {
                            herd.settle(&animation.anchors(tick), tick, space);
                            let count = herd.offsets.len();
                            (0..count).for_each(|a| {
                                (a + 1..count).for_each(|b| {
                                    let (a0, a1) = herd.body(a);
                                    let (b0, b1) = herd.body(b);
                                    let (from, to) = closest_between(a0, a1, b0, b1);
                                    let apart = Vec3::new(from.x, 0.0, from.z)
                                        .distance(Vec3::new(to.x, 0.0, to.z));
                                    assert!(
                                        apart > space.half_width * 2.0,
                                        "size {size} inner {inner} pitch {pitch} gap {gap} \
                                         tick {tick}: dogs {a} and {b} stand {apart} apart \
                                         inside a {} body",
                                        space.half_width * 2.0
                                    );
                                });
                            });
                            // ...and nothing moved, because nothing was touching.
                            assert!(!herd.disturbed());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn two_bodies_meet_flank_to_flank_and_nose_to_tail_at_their_real_distances() {
        // The capsule is the whole point of the collision, so the geometry it
        // rests on is measured directly rather than only through the crowd.
        let along = |x: f32, z: f32| (Vec3::new(x, 0.0, z - 6.0), Vec3::new(x, 0.0, z + 6.0));
        // Two bodies side by side: the gap is the distance between the lines.
        let (a0, a1) = along(0.0, 0.0);
        let (b0, b1) = along(5.0, 0.0);
        let (from, to) = closest_between(a0, a1, b0, b1);
        assert!((from.distance(to) - 5.0).abs() < 1.0e-4, "{from:?} {to:?}");
        // Nose to tail on one line: the gap is between the near ends, not
        // between the centres — this is exactly what a circle got wrong.
        let (c0, c1) = along(0.0, 20.0);
        let (from, to) = closest_between(a0, a1, c0, c1);
        assert!((from.distance(to) - 8.0).abs() < 1.0e-4, "{from:?} {to:?}");
        // Overlapping along the same line: touching, distance zero.
        let (d0, d1) = along(0.0, 4.0);
        let (from, to) = closest_between(a0, a1, d0, d1);
        assert!(from.distance(to) < 1.0e-4, "{from:?} {to:?}");
        // Crossed at right angles, and a degenerate point against a body.
        let crossed = closest_between(a0, a1, Vec3::new(-9.0, 0.0, 0.0), Vec3::new(9.0, 0.0, 0.0));
        assert!(crossed.0.distance(crossed.1) < 1.0e-4);
        let point = Vec3::new(3.0, 0.0, 0.0);
        let (near, _) = closest_between(a0, a1, point, point);
        assert!((near.distance(point) - 3.0).abs() < 1.0e-4);
        let (_, near) = closest_between(point, point, a0, a1);
        assert!((near.distance(point) - 3.0).abs() < 1.0e-4);
        let (one, two) = closest_between(point, point, point, point);
        assert_eq!(one, two);
    }

    #[test]
    fn an_untouched_crowd_stands_exactly_where_its_rings_put_it() {
        let mut herd = Herd::undisturbed();
        let anchors = row(6);
        (0..600u64).for_each(|tick| herd.settle(&anchors, tick, space()));
        assert!(!herd.disturbed());
        assert_eq!(herd.holding(), None);
        // Not "close to zero" — zero. An undisturbed field draws the transforms
        // the walk alone produces, which is what keeps `Animation::transforms`
        // byte-identical between a disturbed session and a fresh one.
        (0..anchors.len()).for_each(|dog| {
            assert_eq!(herd.displacement(dog), Vec3::ZERO, "dog {dog} drifted");
        });
    }

    #[test]
    fn a_dragged_dog_follows_the_pointer_and_walks_back_into_its_own_place() {
        let mut herd = Herd::undisturbed();
        let anchors = row(4);
        herd.settle(&anchors, 0, space());

        // Catch the dog at index 2 and haul it 30 units off the row.
        assert!(herd.grab(straight_down(8.0, 0.0)));
        assert_eq!(herd.holding(), Some(2));
        herd.drag(straight_down(8.0, 30.0));
        herd.settle(&anchors, 1, space());
        assert!((herd.at(2).z - 30.0).abs() < 1.0e-3, "{:?}", herd.at(2));
        // Held, it stays there however long the field walks on.
        (2..200u64).for_each(|tick| herd.settle(&anchors, tick, space()));
        assert!((herd.at(2).z - 30.0).abs() < 1.0e-3, "a held dog drifted home");

        // Let go, and it comes back — all the way back, to the exact anchor.
        herd.release();
        (200..1200u64).for_each(|tick| herd.settle(&anchors, tick, space()));
        assert!(!herd.disturbed(), "the dog never finished coming home");
        assert_eq!(herd.displacement(2), Vec3::ZERO);
        assert_eq!(herd.at(2), anchors[2].position);
    }

    #[test]
    fn the_return_is_gradual_rather_than_a_snap_back() {
        // "Gently" is a claim about the shape of the return, so it is measured:
        // a released dog is still visibly out of place a few ticks later, and
        // most of the way home well before it arrives.
        let mut herd = Herd::undisturbed();
        let anchors = row(3);
        herd.settle(&anchors, 0, space());
        assert!(herd.grab(straight_down(0.0, 0.0)));
        herd.drag(straight_down(0.0, 40.0));
        herd.settle(&anchors, 1, space());
        herd.release();

        herd.settle(&anchors, 6, space());
        let soon = herd.displacement(0).length();
        assert!(soon > 30.0, "the dog snapped home in five ticks: {soon}");
        herd.settle(&anchors, 300, space());
        let later = herd.displacement(0).length();
        assert!(later < soon * 0.1, "still {later} out after 300 ticks");
        assert!(later > 0.0, "arrived early enough to prove nothing");
    }

    #[test]
    fn dogs_hold_each_other_off_and_a_dragged_one_shoves_the_rest_aside() {
        let mut herd = Herd::undisturbed();
        let anchors = row(4);
        herd.settle(&anchors, 0, space());
        // Grab the end dog and drive it straight onto its neighbour's place.
        assert!(herd.grab(straight_down(0.0, 0.0)));
        herd.drag(straight_down(4.0, 0.0));
        // The pass is one round of relaxation a frame, so a chain of dogs comes
        // apart over a handful of frames rather than in one — which is what it
        // looks like on screen, and what this asserts.
        (1..40u64).for_each(|tick| herd.settle(&anchors, tick, space()));

        // The held dog is exactly where it was put — the crowd does not push it.
        assert!((herd.at(0).x - 4.0).abs() < 1.0e-3, "{:?}", herd.at(0));
        // ...and its neighbour has been shoved clear of it, by the whole of the
        // overlap rather than half: the dog in your hand does not give ground.
        let shoved = herd.at(1);
        assert!(
            shoved.distance(herd.at(0)) > space().half_width * 1.9,
            "the dogs are inside each other: {shoved:?}"
        );
        assert!(shoved.x > anchors[1].position.x + 1.0, "the neighbour did not move");
        // The shove carried down the line: dog 1 pushed dog 2 as well.
        assert!(herd.at(2).x > anchors[2].position.x, "the push stopped at the first dog");

        // Let go, and the whole line settles back onto its own anchors.
        herd.release();
        (40..2000u64).for_each(|tick| herd.settle(&anchors, tick, space()));
        assert!(!herd.disturbed(), "the shoved crowd never settled");
        (0..anchors.len()).for_each(|dog| assert_eq!(herd.at(dog), anchors[dog].position));
    }

    #[test]
    fn a_grab_takes_the_nearest_dog_to_the_camera_and_a_miss_takes_none() {
        let mut herd = Herd::undisturbed();
        // Two dogs on the same sight line, one behind the other.
        let far = |z: f32| PathPoint {
            position: Vec3::new(0.0, 0.0, z),
            // Facing across the sight line, so the ray meets each dog's flank
            // rather than running the length of its body.
            forward: Vec3::UNIT_X,
            right: Vec3::UNIT_Z,
        };
        let anchors = vec![far(40.0), far(10.0)];
        herd.settle(&anchors, 0, space());
        let along = Ray {
            origin: Vec3::new(0.0, 0.0, 0.0),
            direction: Vec3::new(0.0, 0.0, 1.0),
        };
        assert!(herd.grab(along));
        assert_eq!(herd.holding(), Some(1), "the grab reached past the near dog");

        // A ray through empty ground catches nothing, and leaves the previous
        // hold released rather than silently keeping it.
        assert!(!herd.grab(straight_down(500.0, 500.0)));
        assert_eq!(herd.holding(), None);
        // A ray pointing away from the field catches nothing either.
        assert!(!herd.grab(Ray {
            origin: Vec3::new(0.0, 0.0, 0.0),
            direction: Vec3::new(0.0, 0.0, -1.0),
        }));
    }

    #[test]
    fn a_dog_cannot_be_dragged_off_the_ground_or_left_stranded_by_a_bad_pointer() {
        let bounded = CrowdSpace {
            half_length: 6.0,
            half_width: 1.8,
            bounds: 50.0,
        };
        let mut herd = Herd::undisturbed();
        let anchors = row(2);
        herd.settle(&anchors, 0, bounded);
        assert!(herd.grab(straight_down(0.0, 0.0)));
        herd.drag(straight_down(0.0, 9_000.0));
        herd.settle(&anchors, 1, bounded);
        let placed = herd.at(0);
        assert!(
            Vec3::new(placed.x, 0.0, placed.z).length() <= 50.0 + 1.0e-3,
            "the dog was dragged off the terrain: {placed:?}"
        );

        // A ray that never meets the drag plane leaves the dog where it is
        // rather than teleporting it to a garbage position.
        let held = herd.at(0);
        herd.drag(Ray {
            origin: Vec3::new(0.0, 0.0, 0.0),
            direction: Vec3::new(1.0, 0.0, 0.0),
        });
        herd.settle(&anchors, 2, bounded);
        assert!(herd.at(0).distance(held) < 1.0e-3);
    }

    #[test]
    fn the_study_calms_the_field_and_a_shrinking_crowd_drops_the_dog_it_held() {
        let mut herd = Herd::undisturbed();
        let anchors = row(4);
        herd.settle(&anchors, 0, space());
        assert!(herd.grab(straight_down(12.0, 0.0)));
        herd.drag(straight_down(12.0, 20.0));
        herd.settle(&anchors, 1, space());
        assert!(herd.disturbed());

        herd.calm();
        assert!(!herd.disturbed());
        assert_eq!(herd.holding(), None);
        // Nothing on an empty stage can be caught, however well aimed.
        assert!(!herd.grab(straight_down(12.0, 0.0)));

        // A ring dial that shrinks the crowd out from under the held dog drops
        // it, rather than indexing a dog that is no longer in the field.
        herd.settle(&anchors, 2, space());
        assert!(herd.grab(straight_down(12.0, 0.0)));
        assert_eq!(herd.holding(), Some(3));
        herd.settle(&row(2), 3, space());
        assert_eq!(herd.holding(), None);
        assert!(!herd.disturbed());
    }
}
