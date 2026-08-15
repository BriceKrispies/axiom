//! **Solid fields** — a set of posed solids smooth-unioned into one continuous
//! body, and the skin binding that falls out of the same blend.
//!
//! [`implicit_surface_mesh`](crate::implicit_surface_mesh) turns a sampled
//! [`ScalarField`] into a surface, but nothing could *build* such a field: the
//! layer refuses `impl Fn` parameters (a callback is an opaque capability that
//! could read a clock, which would make an operator unreplayable), so a caller
//! wanting a blended body had to hand-roll every distance itself. This module is
//! the missing half — a **value** describing the solids, which samples itself
//! onto a lattice.
//!
//! ## One solid shape, three parameter choices
//!
//! Every solid here is a **box core swept by a ball whose radius may taper along
//! the core's own `Z`**. That single form is all three shapes a jointed body
//! needs:
//!
//! | want | core half-extents | radii |
//! |---|---|---|
//! | a ball | `(0, 0, 0)` | `base == tip` |
//! | a tapered capsule (a limb, a neck, a tail) | `(0, 0, length/2)` | `base != tip` |
//! | a rounded box (a paw, a jaw) | the box | `base == tip` |
//!
//! There is no shape enum and no dispatch: a sphere *is* a round cone with no
//! length, and a capsule *is* a rounded box with two zero extents. The
//! constructors ([`Solid::ball`], [`Solid::round_cone`], [`Solid::rounded_box`])
//! name the three cases for the caller and all build the same value, so the
//! sampling loop evaluates one expression per solid rather than branching over a
//! variant. (The rounded-box mesh generator states the same identity from the
//! other side — see `primitive_rounded_box.rs`: a rounded box is the Minkowski
//! sum of a box and a ball.)
//!
//! ## The union is *smooth*, and that is the point
//!
//! A hard union (`min`) of solids is a pile of shapes with creases where they
//! meet. This module unions them with the **log-sum-exp smooth minimum**
//!
//! ```text
//! d(p) = m − k·ln( Σ exp(−(dᵢ(p) − m) / k) ),    m = min dᵢ(p)
//! ```
//!
//! which fuses neighbouring solids with a fillet of scale `k` (the blend
//! radius). A tail whose base sits inside the rump comes out *grown from* the
//! rump; a neck bridging a skull and a chest comes out as one throat.
//!
//! **Why log-sum-exp and not the usual polynomial `smin`.** The polynomial
//! smooth-minimum is pairwise, so folding it over `n` solids makes the result
//! depend on the order they were listed in — the same body, listed differently,
//! is a different body. Log-sum-exp is symmetric in all `n` at once, so a
//! [`SolidField`] is a *set* of solids rather than a sequence. (Float summation
//! is not associative, so reordering still moves the last ulp or two; that
//! rounding residue is the whole of the difference, and the tests hold it
//! there.) The `m` shift is the standard numerically-stable form: without it,
//! `exp(−d/k)` overflows for any solid far outside the body.
//!
//! **How far a blend reaches.** Where two solids are equidistant the field is
//! lowered by exactly `k·ln 2`, so a blend of `k` closes a gap of up to
//! `2·k·ln 2` between two surfaces and fillets everything closer than that. It
//! is a *width*, not a switch: too small and the parts stay separate bodies, too
//! large and the animal turns into a loaf. Where `n` solids crowd together the
//! pull grows to `k·ln n`, which is the bound [`SolidField::bounds`] grows by.
//! Far from the body one solid dominates the sum, the others contribute
//! essentially nothing, and the field is its own plain distance — a blend does
//! not inflate what it is not near.
//!
//! ## The skin weights are the blend
//!
//! The same expression carries, for free, **how much each solid owns a point**:
//!
//! ```text
//! wᵢ(p) = exp(−(dᵢ(p) − m) / k) / Σ exp(−(dⱼ(p) − m) / k)
//! ```
//!
//! Those weights are non-negative and sum to one — a partition of unity — and
//! they are exactly the terms the smooth minimum blended. So [`SolidField::skin`]
//! can hand back a vertex's four strongest **bones** and their influence without
//! painting weights, choosing a falloff, or inventing a heuristic: *the blend
//! that fused the surface is the blend that deforms it*. Bind those weights to a
//! skeleton and the body moves as one skin, with its parts still articulating.
//!
//! Several solids may share a bone (a torso built from three cones is one
//! spine), so influence is summed **per bone** before the top four are taken.
//!
//! ## What the field is, and is not
//!
//! It is a *bounded* distance, not an exact one: a tapered solid's field is
//! divided by `1 + slope` to restore a Lipschitz constant of one, so a blend
//! radius means the same width at a taper as it does on a cylinder. The zero
//! level set is unaffected — dividing by a positive constant cannot move it.

use axiom_kernel::Meters;
use axiom_math::Vec3;
use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

use crate::implicit_surface::{ImplicitSurfaceOptions, ScalarField};
use crate::tessellation::DetailBudget;

mod solid;

pub use solid::Solid;

/// How many bones one vertex may be bound to — the width of the skin streams
/// [`axiom_mesh::Mesh`] carries and of the linear-blend the renderers apply.
pub const SKIN_INFLUENCES: usize = 4;

/// The identity of the bounds fold: an inside-out box every real point widens.
const EMPTY_BOUNDS: (Vec3, Vec3) = (
    Vec3::new(f32::MAX, f32::MAX, f32::MAX),
    Vec3::new(f32::MIN, f32::MIN, f32::MIN),
);

/// Grow a `(low, high)` pair outward by `reach` on every axis.
fn grow(bounds: (Vec3, Vec3), reach: f32) -> (Vec3, Vec3) {
    let margin = Vec3::new(reach, reach, reach);
    (bounds.0.subtract(margin), bounds.1.add(margin))
}

/// The component-wise smaller of two points.
fn component_min(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

/// The component-wise larger of two points.
fn component_max(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

/// A set of solids fused by a smooth union of scale `blend`.
#[derive(Debug, Clone, PartialEq)]
pub struct SolidField {
    solids: Vec<Solid>,
    blend: f32,
}

impl SolidField {
    /// Fuse `solids` with a blend radius of `blend`.
    ///
    /// The blend must be greater than zero: at zero the union is the hard `min`,
    /// which this operator deliberately does not offer — a caller wanting
    /// unblended solids wants separate meshes, not one body with creases in it.
    ///
    /// Fails with [`MeshErrorCode::InvalidParameter`] on an empty solid set or a
    /// non-positive blend.
    pub fn new(solids: Vec<Solid>, blend: Meters) -> MeshResult<SolidField> {
        ((!solids.is_empty()) & (blend.get() > 0.0))
            .then_some(())
            .ok_or_else(|| {
                invalid("a solid field needs at least one solid and a blend radius above zero")
            })
            .map(|()| SolidField {
                solids,
                blend: blend.get(),
            })
    }

    /// The solids being fused, in the order they were given.
    pub fn solids(&self) -> &[Solid] {
        &self.solids
    }

    /// The blend radius.
    pub const fn blend(&self) -> Meters {
        Meters::finite_or_zero(self.blend)
    }

    /// The highest bone index any solid names, plus one — the palette width a
    /// skin built from this field expects.
    pub fn bone_count(&self) -> usize {
        self.solids
            .iter()
            .map(|solid| solid.bone() as usize + 1)
            .fold(0, usize::max)
    }

    /// The fused signed distance at `point`: the smooth minimum over every
    /// solid.
    pub fn distance(&self, point: Vec3) -> Meters {
        Meters::finite_or_zero(self.signed_distance(point))
    }

    /// The same distance as a bare scalar. Sampling a lattice is millions of
    /// these, so the unit is re-attached once at the public edge.
    fn signed_distance(&self, point: Vec3) -> f32 {
        let (nearest, total) = self.accumulate(point);
        nearest - self.blend * total.ln()
    }

    /// How much each solid owns `point`, in solid order: non-negative, summing
    /// to one. This is the partition of unity the smooth minimum blended, and it
    /// is what [`SolidField::skin`] binds a vertex with.
    pub fn influences(&self, point: Vec3) -> Vec<f32> {
        let (nearest, total) = self.accumulate(point);
        let scale = 1.0 / total;
        self.solids
            .iter()
            .map(|solid| self.falloff(solid.signed_distance(point) - nearest) * scale)
            .collect()
    }

    /// The nearest solid's distance and the summed exponential falloff — the two
    /// numbers both the fused distance and the influences are made of, computed
    /// the same way so neither can disagree with the other.
    ///
    /// The total is never zero and never below one: the nearest solid is its own
    /// zero excess and so contributes `exp(0) = 1`, and every other term is
    /// positive. That is why neither caller floors it before dividing or taking
    /// its logarithm.
    fn accumulate(&self, point: Vec3) -> (f32, f32) {
        let nearest = self
            .solids
            .iter()
            .map(|solid| solid.signed_distance(point))
            .fold(f32::INFINITY, f32::min);
        let total = self
            .solids
            .iter()
            .map(|solid| self.falloff(solid.signed_distance(point) - nearest))
            .sum();
        (nearest, total)
    }

    /// One solid's exponential falloff at `excess` — how much further from
    /// `point` it is than the nearest solid.
    fn falloff(&self, excess: f32) -> f32 {
        (-excess / self.blend).exp()
    }

    /// The axis-aligned box the fused body occupies, as `(low, high)`.
    ///
    /// The union of the solids' own boxes, grown by the distance the smooth
    /// minimum can push the surface outward: it lowers the field by at most
    /// `blend · ln(n)` for `n` solids, so the surface can move out by that much
    /// and no more.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        grow(
            self.solids.iter().map(Solid::bounds).fold(
                EMPTY_BOUNDS,
                |(low, high), (solid_low, solid_high)| {
                    (component_min(low, solid_low), component_max(high, solid_high))
                },
            ),
            self.blend * (self.solids.len() as f32).ln(),
        )
    }

    /// Sample the fused field onto `lattice`, ready for
    /// [`implicit_surface_mesh`](crate::implicit_surface_mesh).
    pub fn sample(&self, lattice: &SolidLattice) -> MeshResult<ScalarField> {
        let [cols, rows, depth] = lattice.counts;
        let plane = u64::from(cols) * u64::from(rows);
        let values: Vec<f32> = (0..plane * u64::from(depth))
            .map(|node| {
                // The layout `ScalarField` documents: X fastest, then Y, then Z.
                let x = node % u64::from(cols);
                let y = (node / u64::from(cols)) % u64::from(rows);
                let z = node / plane;
                self.signed_distance(lattice.node(x as u32, y as u32, z as u32))
            })
            .collect();
        ScalarField::new(values, cols, rows, depth)
    }

    /// Bind `points` to bones: for each point, the [`SKIN_INFLUENCES`] bones
    /// with the most influence over it, and their weights renormalized to sum to
    /// one.
    ///
    /// Influence is summed **per bone** first, so a bone built from several
    /// solids competes with its whole body rather than one piece of it. A point
    /// influenced by fewer than four bones is padded with zero-weight entries,
    /// which a linear blend adds nothing for.
    pub fn skin(&self, points: &[Vec3]) -> (Vec<[u16; 4]>, Vec<[f32; 4]>) {
        points.iter().map(|point| self.bind(*point)).unzip()
    }

    /// One point's four strongest bones and their normalized weights.
    fn bind(&self, point: Vec3) -> ([u16; 4], [f32; 4]) {
        let per_bone = self.influences(point).into_iter().zip(self.solids.iter()).fold(
            vec![0.0_f32; self.bone_count()],
            |mut totals, (influence, solid)| {
                totals[solid.bone() as usize] += influence;
                totals
            },
        );
        let mut ranked: Vec<(u16, f32)> = per_bone
            .into_iter()
            .enumerate()
            .map(|(bone, weight)| (bone as u16, weight))
            .collect();
        // Descending by weight, then by bone index, so a tie resolves the same
        // way in every process — the whole skin has to be replayable.
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let taken: Vec<(u16, f32)> = ranked
            .into_iter()
            .take(SKIN_INFLUENCES)
            .chain(core::iter::repeat((0, 0.0)))
            .take(SKIN_INFLUENCES)
            .collect();
        // The four kept weights cannot sum to zero: they are the largest of a
        // partition of unity, so the first alone is at least `1 / bone_count`.
        let scale = 1.0 / taken.iter().map(|(_, weight)| *weight).sum::<f32>();
        (
            core::array::from_fn(|slot| taken[slot].0),
            core::array::from_fn(|slot| taken[slot].1 * scale),
        )
    }
}

/// The lattice a [`SolidField`] is sampled onto: where node `(0, 0, 0)` sits,
/// how far apart the nodes are, and how many there are on each axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidLattice {
    origin: Vec3,
    spacing: Vec3,
    counts: [u32; 3],
}

impl SolidLattice {
    /// The smallest uniform lattice at `spacing` that covers `field`, with one
    /// spare cell of margin on every side.
    ///
    /// The margin is not cosmetic: marching cubes only emits a surface where the
    /// field *crosses* the iso level, so a body touching the lattice boundary
    /// comes out with a hole where it was cut off. A ring of nodes outside the
    /// body closes it.
    ///
    /// Fails with [`MeshErrorCode::InvalidParameter`] when `spacing` is not
    /// greater than zero.
    pub fn covering(field: &SolidField, spacing: Meters) -> MeshResult<SolidLattice> {
        let step = spacing.get();
        (step > 0.0)
            .then_some(())
            .ok_or_else(|| invalid("a lattice spacing must be greater than zero"))
            .map(|()| {
                let (low, high) = field.bounds();
                let margin = Vec3::new(step, step, step);
                let origin = low.subtract(margin);
                let span = high.add(margin).subtract(origin);
                SolidLattice {
                    origin,
                    spacing: Vec3::new(step, step, step),
                    // One node per cell, plus the closing one. The margin is a
                    // whole cell on each side, so every axis spans at least two
                    // cells and the lattice always has the two nodes a
                    // `ScalarField` requires.
                    counts: [span.x, span.y, span.z]
                        .map(|extent| (extent / step).ceil() as u32 + 1),
                }
            })
    }

    /// The world position of lattice node `(x, y, z)`.
    pub fn node(&self, x: u32, y: u32, z: u32) -> Vec3 {
        self.origin.add(Vec3::new(
            x as f32 * self.spacing.x,
            y as f32 * self.spacing.y,
            z as f32 * self.spacing.z,
        ))
    }

    /// The node counts on each axis.
    pub const fn counts(&self) -> [u32; 3] {
        self.counts
    }

    /// How many nodes the lattice holds — the number of field evaluations a
    /// sampling costs, which is what a caller sizes its detail against.
    pub fn node_count(&self) -> u64 {
        u64::from(self.counts[0]) * u64::from(self.counts[1]) * u64::from(self.counts[2])
    }

    /// The extraction options that match this lattice, at `budget`.
    pub const fn options(&self, budget: DetailBudget) -> ImplicitSurfaceOptions {
        ImplicitSurfaceOptions {
            origin: self.origin,
            spacing: self.spacing,
            budget,
        }
    }
}

/// An invalid-parameter error with `message`.
fn invalid(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::InvalidParameter, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axiom_math::Quat;
    use axiom_mesh::{Mesh, MeshStreams};

    use crate::implicit_surface::{implicit_surface_mesh, IsoValue};

    /// A length, for a test that does not care about the validation.
    fn m(value: f32) -> Meters {
        Meters::finite_or_zero(value)
    }

    /// A ball on bone `bone`.
    fn ball(centre: Vec3, radius: f32, bone: u16) -> Solid {
        Solid::ball(centre, m(radius), bone).expect("a ball of a positive radius is valid")
    }

    /// The field two balls `apart` units apart make, at blend `blend`.
    fn pair(apart: f32, blend: f32) -> SolidField {
        SolidField::new(
            vec![
                ball(Vec3::new(-apart * 0.5, 0.0, 0.0), 0.5, 0),
                ball(Vec3::new(apart * 0.5, 0.0, 0.0), 0.5, 1),
            ],
            m(blend),
        )
        .expect("two balls and a positive blend are a valid field")
    }

    /// Close enough for a value derived through a rotation.
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    /// The same, for the unit-typed distances the public API hands back.
    fn close_m(a: Meters, b: f32) -> bool {
        close(a.get(), b)
    }

    #[test]
    fn the_three_constructors_are_three_parameter_choices_of_one_solid() {
        // A ball is its centre distance less its radius, inside and out.
        let ball = ball(Vec3::new(1.0, 0.0, 0.0), 0.25, 0);
        assert!(close_m(ball.distance(Vec3::new(2.0, 0.0, 0.0)), 0.75));
        assert!(close_m(ball.distance(Vec3::new(1.0, 0.0, 0.0)), -0.25));
        assert!(close_m(ball.distance(Vec3::new(1.25, 0.0, 0.0)), 0.0));

        // An untapered round cone is a capsule: the distance to its segment,
        // less its radius — the same at either end and along the side.
        let capsule = Solid::round_cone(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -2.0),
            m(0.5),
            m(0.5),
            0,
        )
        .expect("a capsule is a valid round cone");
        assert!(close_m(capsule.distance(Vec3::new(0.0, 0.0, 0.5)), 0.0));
        assert!(close_m(capsule.distance(Vec3::new(0.0, 0.0, -2.5)), 0.0));
        assert!(close_m(capsule.distance(Vec3::new(0.5, 0.0, -1.0)), 0.0));
        assert!(close_m(capsule.distance(Vec3::new(0.0, 1.5, -1.0)), 1.0));

        // A rounded box grows the box by its radius, so its face sits at
        // half-extent + radius.
        let block =
            Solid::rounded_box(Vec3::ZERO, Quat::IDENTITY, Vec3::new(1.0, 2.0, 3.0), m(0.5), 0)
                .expect("a rounded box is valid");
        assert!(close_m(block.distance(Vec3::new(1.5, 0.0, 0.0)), 0.0));
        assert!(close_m(block.distance(Vec3::new(0.0, 2.5, 0.0)), 0.0));
        assert!(close_m(block.distance(Vec3::new(0.0, 0.0, 4.5)), 1.0));
        // ...and it is a solid: the middle is inside by its own thickness.
        assert!(close_m(block.distance(Vec3::ZERO), -1.5));
    }

    #[test]
    fn a_tapered_cone_narrows_from_its_base_to_its_tip() {
        let cone = Solid::round_cone(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -4.0),
            m(1.0),
            m(0.25),
            0,
        )
        .expect("a tapered cone is valid");
        // The base end is fat and the tip end is thin: the same offset from the
        // axis is inside at the base and outside at the tip.
        assert!(cone.distance(Vec3::new(0.5, 0.0, -0.1)).get() < 0.0);
        assert!(cone.distance(Vec3::new(0.5, 0.0, -3.9)).get() > 0.0);
        // The field stays a bounded distance: no two points are further apart in
        // field value than they are in space (Lipschitz <= 1), which is what the
        // slope re-normalization buys and what makes a blend radius mean one
        // width everywhere.
        let along = || (0..40).map(|s| Vec3::new(0.0, 3.0, -0.1 * s as f32));
        along().zip(along().skip(1)).for_each(|(a, b)| {
            let moved = a.distance(b);
            assert!(
                (cone.distance(a).get() - cone.distance(b).get()).abs() <= moved + 1.0e-5,
                "the field changed faster than the distance walked"
            );
        });
    }

    #[test]
    fn a_solid_rejects_what_it_cannot_be() {
        assert_eq!(
            Solid::ball(Vec3::new(f32::NAN, 0.0, 0.0), m(1.0), 0)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            Solid::ball(Vec3::ZERO, m(-1.0), 0).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            Solid::rounded_box(Vec3::ZERO, Quat::IDENTITY, Vec3::new(-1.0, 0.0, 0.0), m(1.0), 0)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        // A rotation with no direction in it cannot orient anything.
        assert_eq!(
            Solid::rounded_box(Vec3::ZERO, Quat::new(0.0, 0.0, 0.0, 0.0), Vec3::ONE, m(1.0), 0)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        // A round cone with coincident ends is a ball, and says so.
        assert_eq!(
            Solid::round_cone(Vec3::ZERO, Vec3::ZERO, m(1.0), m(1.0), 0)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_cone_aimed_down_positive_z_is_the_half_turn_case() {
        // `aim_negative_z`'s one degenerate input: the shortest arc from -Z to
        // +Z has no axis, so any perpendicular one is correct.
        let flipped =
            Solid::round_cone(Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0), m(0.5), m(0.5), 0)
                .expect("a cone along +Z is valid");
        // It is the same capsule as one aimed the other way, just reversed: both
        // ends are on the surface and the flank is on it too.
        assert!(close_m(flipped.distance(Vec3::new(0.0, 0.0, -0.5)), 0.0));
        assert!(close_m(flipped.distance(Vec3::new(0.0, 0.0, 2.5)), 0.0));
        assert!(close_m(flipped.distance(Vec3::new(0.0, 0.5, 1.0)), 0.0));
        // ...and the taper still runs base-to-tip along the authored direction.
        let tapered =
            Solid::round_cone(Vec3::ZERO, Vec3::new(0.0, 0.0, 4.0), m(1.0), m(0.25), 0)
                .expect("a tapered cone along +Z is valid");
        assert!(tapered.distance(Vec3::new(0.5, 0.0, 0.1)).get() < 0.0);
        assert!(tapered.distance(Vec3::new(0.5, 0.0, 3.9)).get() > 0.0);
    }

    #[test]
    fn a_field_needs_solids_and_a_blend() {
        assert_eq!(
            SolidField::new(Vec::new(), m(0.1)).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            SolidField::new(vec![ball(Vec3::ZERO, 1.0, 0)], m(0.0))
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        let field = pair(1.4, 0.2);
        assert_eq!(field.solids().len(), 2);
        // Each solid remembers the bone it was tagged with — that tag is the
        // whole of how a field becomes a skeleton.
        let tags: Vec<u16> = field.solids().iter().map(Solid::bone).collect();
        assert_eq!(tags, vec![0, 1]);
        assert!(close(field.blend().get(), 0.2));
        // The palette width is the highest bone named, plus one — not the solid
        // count, and not necessarily dense.
        assert_eq!(field.bone_count(), 2);
        let sparse = SolidField::new(vec![ball(Vec3::ZERO, 1.0, 5)], m(0.1))
            .expect("one ball on bone 5 is a valid field");
        assert_eq!(sparse.bone_count(), 6);
    }

    #[test]
    fn the_smooth_union_fuses_a_gap_a_hard_one_would_leave_open() {
        // Two balls with clear air between them: the hard union leaves the
        // midpoint outside both...
        let solids = pair(1.4, 0.4);
        let hard = solids
            .solids()
            .iter()
            .map(|solid| solid.distance(Vec3::ZERO).get())
            .fold(f32::INFINITY, f32::min);
        assert!(hard > 0.0, "the two balls really are apart: {hard}");
        // ...and the smooth union pulls the surface across it, so the midpoint is
        // inside one body.
        let bridged = solids.distance(Vec3::ZERO).get();
        assert!(bridged < 0.0, "the blend did not bridge the gap: {bridged}");
        // A tighter blend cannot bridge as much — the blend radius is a width,
        // not a switch, and its reach between two solids is `blend · ln 2`.
        assert!(pair(1.4, 0.05).distance(Vec3::ZERO).get() > 0.0);
        // ...which is the reach, to the float: two surfaces 0.2 apart are met
        // exactly when `blend · ln 2` is 0.2.
        assert!(close(
            pair(1.4, 0.2 / core::f32::consts::LN_2).distance(Vec3::ZERO).get(),
            0.0
        ));
        // The fused field is never above the hard union: a smooth minimum only
        // ever pulls the surface outward.
        (0..25).for_each(|step| {
            let p = Vec3::new(-1.2 + 0.1 * step as f32, 0.1, 0.0);
            let hard = solids
                .solids()
                .iter()
                .map(|solid| solid.distance(p).get())
                .fold(f32::INFINITY, f32::min);
            assert!(solids.distance(p).get() <= hard + 1.0e-5);
        });
    }

    #[test]
    fn the_union_does_not_depend_on_the_order_the_solids_were_listed_in() {
        // The whole reason for log-sum-exp over a pairwise smooth minimum: a
        // body is a *set* of solids, so listing them differently cannot change
        // it by so much as a float.
        let solids = vec![
            ball(Vec3::new(-0.4, 0.0, 0.0), 0.5, 0),
            ball(Vec3::new(0.4, 0.0, 0.0), 0.3, 1),
            ball(Vec3::new(0.0, 0.6, 0.0), 0.4, 2),
        ];
        let forward = SolidField::new(solids.clone(), m(0.25)).expect("valid");
        let reversed =
            SolidField::new(solids.into_iter().rev().collect(), m(0.25)).expect("valid");
        (0..30).for_each(|step| {
            let p = Vec3::new(-1.0 + 0.07 * step as f32, 0.3, 0.1);
            // Not *bit*-identical: a sum of floats is not associative, so
            // reordering the terms moves the last ulp or two. That residue is
            // the only difference — a pairwise smooth minimum reordered is a
            // visibly different surface, not a rounding away from the same one.
            let (one_way, other_way) = (forward.distance(p).get(), reversed.distance(p).get());
            assert!(
                (one_way - other_way).abs() < 1.0e-5,
                "the same body listed backwards is a different body at {p:?}:                  {one_way} vs {other_way}"
            );
        });
    }

    #[test]
    fn influences_are_a_partition_of_unity_led_by_the_nearest_solid() {
        let field = pair(1.4, 0.2);
        (0..30).for_each(|step| {
            let p = Vec3::new(-1.5 + 0.1 * step as f32, 0.2, 0.0);
            let influences = field.influences(p);
            assert_eq!(influences.len(), 2);
            assert!(influences.iter().all(|w| (0.0..=1.0).contains(w)));
            let whole: f32 = influences.iter().sum();
            assert!(close(whole, 1.0), "influences at {p:?} sum to {whole}");
        });
        // Deep inside the left ball, the left ball owns the point outright.
        let left = field.influences(Vec3::new(-0.7, 0.0, 0.0));
        assert!(left[0] > 0.99, "{left:?}");
        // At the midpoint the two share it exactly.
        let middle = field.influences(Vec3::ZERO);
        assert!(close(middle[0], middle[1]), "{middle:?}");
    }

    #[test]
    fn the_bounds_hold_the_whole_body_and_the_lattice_closes_it() {
        let field = pair(1.4, 0.2);
        let (low, high) = field.bounds();
        // Every solid is inside...
        field.solids().iter().for_each(|solid| {
            let (solid_low, solid_high) = solid.bounds();
            assert!(low.x <= solid_low.x && low.y <= solid_low.y && low.z <= solid_low.z);
            assert!(high.x >= solid_high.x && high.y >= solid_high.y && high.z >= solid_high.z);
        });
        // ...and so is the blended surface: every corner of the box is outside
        // the body, which is what lets marching cubes close it.
        [low.x, high.x].iter().for_each(|x| {
            [low.y, high.y].iter().for_each(|y| {
                [low.z, high.z].iter().for_each(|z| {
                    assert!(field.distance(Vec3::new(*x, *y, *z)).get() > 0.0);
                })
            })
        });

        let lattice = SolidLattice::covering(&field, m(0.1)).expect("a positive spacing is valid");
        let [cols, rows, depth] = lattice.counts();
        // The margin is a whole cell on every side, so no axis can come out
        // below the two nodes a field needs.
        let counts = lattice.counts();
        assert!((cols >= 3) & (rows >= 3) & (depth >= 3), "{counts:?}");
        assert_eq!(
            lattice.node_count(),
            u64::from(cols) * u64::from(rows) * u64::from(depth)
        );
        // Node (0,0,0) is the origin, and the far corner is past the body too.
        let options = lattice.options(DetailBudget::default());
        assert_eq!(lattice.node(0, 0, 0), options.origin);
        assert!(close(options.spacing.x, 0.1));
        assert!(field.distance(lattice.node(0, 0, 0)).get() > 0.0);
        assert!(field.distance(lattice.node(cols - 1, rows - 1, depth - 1)).get() > 0.0);
        assert_eq!(
            SolidLattice::covering(&field, m(0.0)).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    /// How many connected pieces a mesh's triangles form, by walking shared
    /// vertices. Test-only: it is the observable that separates "one body" from
    /// "a pile of shapes".
    fn components(mesh: &Mesh) -> usize {
        fn root(parent: &mut [usize], mut node: usize) -> usize {
            while parent[node] != node {
                parent[node] = parent[parent[node]];
                node = parent[node];
            }
            node
        }
        let mut parent: Vec<usize> = (0..mesh.vertex_count()).collect();
        for triangle in mesh.indices().chunks_exact(3) {
            for pair in [[0, 1], [1, 2]] {
                let a = root(&mut parent, triangle[pair[0]] as usize);
                let b = root(&mut parent, triangle[pair[1]] as usize);
                parent[a] = b;
            }
        }
        let mut roots: Vec<usize> = mesh
            .indices()
            .iter()
            .map(|vertex| root(&mut parent, *vertex as usize))
            .collect();
        roots.sort_unstable();
        roots.dedup();
        roots.len()
    }

    /// Fuse, sample and extract at `spacing`.
    fn surface(field: &SolidField, spacing: f32) -> Mesh {
        let lattice = SolidLattice::covering(field, m(spacing)).expect("valid spacing");
        implicit_surface_mesh(
            &field.sample(&lattice).expect("the field samples"),
            IsoValue::new(0.0).expect("zero is a finite iso level"),
            lattice.options(DetailBudget::default()),
        )
        .expect("the sampled body extracts")
    }

    #[test]
    fn a_blended_field_extracts_as_one_connected_body() {
        // The claim the whole module exists for. Two separated balls at a blend
        // too tight to bridge the gap are two bodies...
        assert_eq!(components(&surface(&pair(1.4, 0.05), 0.05)), 2);
        // ...and at a blend that bridges it, one. Same solids, same spacing,
        // same extraction: the blend is the whole difference.
        assert_eq!(components(&surface(&pair(1.4, 0.4), 0.05)), 1);
    }

    #[test]
    fn skin_binds_every_vertex_to_the_bones_that_own_it() {
        let field = pair(1.4, 0.2);
        let points = vec![
            Vec3::new(-0.9, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(0.9, 0.0, 0.0),
        ];
        let (joints, weights) = field.skin(&points);
        assert_eq!(joints.len(), 3);
        assert_eq!(weights.len(), 3);
        // Every row is a normalized blend, which is exactly what `axiom_mesh`
        // validates a skin stream for.
        weights.iter().for_each(|row| {
            assert!(row.iter().all(|w| *w >= 0.0));
            assert!(close(row.iter().sum::<f32>(), 1.0), "{row:?}");
        });
        // The near end of each ball is owned by that ball's bone...
        assert_eq!(joints[0][0], 0);
        assert!(weights[0][0] > 0.95);
        assert_eq!(joints[2][0], 1);
        assert!(weights[2][0] > 0.95);
        // ...and the seam between them is genuinely shared, which is what makes
        // the fused surface deform as one skin instead of tearing.
        let seam = weights[1];
        assert!(close(seam[0], 0.5) & close(seam[1], 0.5), "{seam:?}");
        // Only two bones exist, so the remaining influences are zero-weight
        // padding a linear blend adds nothing for.
        assert!(close(weights[0][2], 0.0) & close(weights[0][3], 0.0));
    }

    #[test]
    fn a_skinned_extraction_is_a_valid_axiom_mesh() {
        // The end-to-end shape the caller actually wants: fuse, extract, bind,
        // and hand the result to `axiom_mesh` — which validates the skin rows it
        // was given rather than taking anyone's word for them.
        let field = SolidField::new(
            vec![
                ball(Vec3::new(-0.4, 0.0, 0.0), 0.5, 0),
                ball(Vec3::new(0.4, 0.0, 0.0), 0.5, 1),
                ball(Vec3::new(0.0, 0.5, 0.0), 0.35, 2),
            ],
            m(0.15),
        )
        .expect("valid field");
        let body = surface(&field, 0.06);
        let (joints, weights) = field.skin(body.positions());
        let skinned = Mesh::from_streams(MeshStreams {
            positions: body.positions().to_vec(),
            normals: body.normals().to_vec(),
            indices: body.indices().to_vec(),
            joints,
            weights,
            ..MeshStreams::default()
        })
        .expect("a fused body with weights out of its own blend is a valid skinned mesh");
        assert!(skinned.is_skinned());
        assert_eq!(components(&skinned), 1, "three fused balls are one body");
        // Every one of the three bones actually owns some of the surface — a
        // binding that quietly dropped a bone would still validate.
        let bound: Vec<u16> = (0..3)
            .filter(|bone| {
                skinned
                    .joints()
                    .iter()
                    .zip(skinned.weights())
                    .any(|(row, w)| row.iter().zip(w).any(|(j, w)| (j == bone) & (*w > 0.25)))
            })
            .collect();
        assert_eq!(bound, vec![0, 1, 2]);
    }

    #[test]
    fn a_tie_between_bones_resolves_the_same_way_every_time() {
        // Four identical balls on four bones, sampled at the point equidistant
        // from all of them: the weights are equal, so the order is decided by
        // the tie-break alone. It has to be the bone index, or the same body
        // binds differently in two processes.
        let field = SolidField::new(
            (0..4)
                .map(|bone| {
                    let angle = bone as f32 * core::f32::consts::FRAC_PI_2;
                    ball(
                        Vec3::new(angle.cos(), angle.sin(), 0.0),
                        0.6,
                        3 - bone as u16,
                    )
                })
                .collect(),
            m(0.4),
        )
        .expect("valid field");
        let (joints, weights) = field.skin(&[Vec3::ZERO]);
        assert_eq!(joints[0], [0, 1, 2, 3], "a tie did not resolve by bone index");
        let shared = weights[0];
        assert!(shared.iter().all(|w| close(*w, 0.25)), "{shared:?}");
    }
}
