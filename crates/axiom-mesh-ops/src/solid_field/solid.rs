//! The [`Solid`] value: one posed shape in a [`SolidField`](super::SolidField).
//!
//! Split out of `solid_field.rs` to keep both files inside the engine's
//! file-size budget. It is the *shape* half of the module — one struct, its
//! three constructors, and the one distance expression they all evaluate; the
//! blending, sampling and skinning that consume it stay in the parent.

use axiom_kernel::Meters;
use axiom_math::{Quat, Vec3};
use axiom_mesh::MeshResult;

use super::{component_max, component_min, grow, invalid, EMPTY_BOUNDS};

/// Numeric floor: the shortest core a round cone may have, the taper span a
/// zero-length core divides by, and the arc below which [`aim_negative_z`] takes
/// its half-turn case.
const EPSILON: f32 = 1.0e-6;

/// One posed solid: a box core swept by a ball whose radius tapers along the
/// core's local `Z`, tagged with the bone that moves it.
///
/// Local `+Z` carries the `base` radius and local `−Z` the `tip` radius, which
/// is the convention a bone authored "from its pivot, down its own `−Z`" already
/// uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Solid {
    /// The core's centre, in the field's space.
    centre: Vec3,
    /// The core's orientation. Unit, so its conjugate is its inverse.
    rotation: Quat,
    /// The core box's half-extents. Any component may be zero — that is what
    /// makes a segment or a point core.
    half_extents: Vec3,
    /// The sweeping ball's radius at local `+Z`.
    base: f32,
    /// The sweeping ball's radius at local `−Z`.
    tip: f32,
    /// The bone this solid belongs to.
    bone: u16,
}

impl Solid {
    /// A ball of `radius` at `centre`.
    pub fn ball(centre: Vec3, radius: Meters, bone: u16) -> MeshResult<Solid> {
        Solid::new(centre, Quat::IDENTITY, Vec3::ZERO, radius, radius, bone)
    }

    /// A tapered capsule: the segment `from` → `to`, swept by a ball of `base`
    /// radius at `from` shrinking to `tip` at `to`.
    ///
    /// Fails with [`MeshErrorCode::InvalidParameter`](axiom_mesh::MeshErrorCode::InvalidParameter) when the two ends
    /// coincide — that solid is a [`Solid::ball`], and asking for it here would
    /// mean inventing an orientation for a segment that has no direction.
    pub fn round_cone(
        from: Vec3,
        to: Vec3,
        base: Meters,
        tip: Meters,
        bone: u16,
    ) -> MeshResult<Solid> {
        let along = to.subtract(from);
        let length = along.length();
        (length > EPSILON)
            .then_some(())
            .ok_or_else(|| {
                invalid("a round cone needs two distinct ends; a zero-length one is a ball")
            })
            .and_then(|()| {
                Solid::new(
                    from.add(along.mul_scalar(0.5)),
                    aim_negative_z(along.mul_scalar(1.0 / length)),
                    Vec3::new(0.0, 0.0, length * 0.5),
                    base,
                    tip,
                    bone,
                )
            })
    }

    /// A box of `half_extents` at `centre`, turned by `rotation`, with its edges
    /// and corners rounded off at `radius`.
    ///
    /// Unlike [`crate::rounded_box`], the rounding **grows** the box rather than
    /// eating into it: this is the swept solid, and its bounds are
    /// `half_extents + radius`.
    pub fn rounded_box(
        centre: Vec3,
        rotation: Quat,
        half_extents: Vec3,
        radius: Meters,
        bone: u16,
    ) -> MeshResult<Solid> {
        Solid::new(centre, rotation, half_extents, radius, radius, bone)
    }

    /// The validated solid: a finite centre, finite non-negative half-extents
    /// and radii, and a normalizable rotation.
    ///
    /// Private because it can express a solid the public constructors cannot: a
    /// core with no length along `Z` carrying two *different* radii, which has
    /// no span to taper over. Every constructor above passes one radius twice
    /// for such a core, so the case cannot arise; [`Solid::radius_at`] resolves
    /// it to `tip` rather than leaving it undefined.
    fn new(
        centre: Vec3,
        rotation: Quat,
        half_extents: Vec3,
        base: Meters,
        tip: Meters,
        bone: u16,
    ) -> MeshResult<Solid> {
        let extents = [half_extents.x, half_extents.y, half_extents.z];
        let placed = [centre.x, centre.y, centre.z].iter().all(|c| c.is_finite());
        let shaped = extents.iter().all(|e| e.is_finite() & (*e >= 0.0));
        let solid = (base.get() >= 0.0) & (tip.get() >= 0.0);
        (placed & shaped & solid)
            .then_some(())
            .ok_or_else(|| {
                invalid(
                    "a solid needs a finite centre and finite non-negative half-extents and radii",
                )
            })
            .and_then(|()| {
                rotation
                    .normalize()
                    .map_err(|_| invalid("a solid's rotation must be normalizable"))
            })
            .map(|rotation| Solid {
                centre,
                rotation,
                half_extents,
                base: base.get(),
                tip: tip.get(),
                bone,
            })
    }

    /// The bone that moves this solid.
    pub const fn bone(&self) -> u16 {
        self.bone
    }

    /// The signed distance from `point` to this solid: negative inside, zero on
    /// the surface, and Lipschitz-bounded by one outside.
    pub fn distance(&self, point: Vec3) -> Meters {
        Meters::finite_or_zero(self.signed_distance(point))
    }

    /// The same distance as a bare scalar, for the arithmetic inside the module.
    /// A field evaluation is a few million of these, and the unit is re-attached
    /// once at the public edge rather than wrapped and unwrapped per solid.
    pub(super) fn signed_distance(&self, point: Vec3) -> f32 {
        let local = self.rotation.conjugate().rotate(point.subtract(self.centre));
        // The exact distance to the box core: the length of the part of the
        // offset that is outside the box, plus (for an interior point, where
        // that length is zero) the negative distance to the nearest face.
        let q = Vec3::new(
            local.x.abs() - self.half_extents.x,
            local.y.abs() - self.half_extents.y,
            local.z.abs() - self.half_extents.z,
        );
        let outside = Vec3::new(q.x.max(0.0), q.y.max(0.0), q.z.max(0.0)).length();
        let inside = q.x.max(q.y).max(q.z).min(0.0);
        // ...less the sweeping ball's radius where this point stands along the
        // core, re-normalized by the taper's slope so the field stays a bounded
        // distance rather than a steeper-than-unit one.
        (outside + inside - self.radius_at(local.z)) / (1.0 + self.slope())
    }

    /// The sweeping ball's radius at local height `z`, held at the end radii
    /// beyond the core's own span (which is what makes the ends hemispherical
    /// caps rather than cones running to a point).
    fn radius_at(&self, z: f32) -> f32 {
        let span = self.half_extents.z * 2.0;
        let along = ((z + self.half_extents.z) / span.max(EPSILON)).clamp(0.0, 1.0);
        self.tip + (self.base - self.tip) * along
    }

    /// How fast the radius changes along the core, in radius per unit of length.
    /// Zero for every untapered solid, including the zero-length ones.
    fn slope(&self) -> f32 {
        (self.base - self.tip).abs() / (self.half_extents.z * 2.0).max(EPSILON)
    }

    /// The axis-aligned box this solid occupies, as `(low, high)`.
    ///
    /// Taken from the eight rotated corners of the core, grown by the larger of
    /// the two radii — conservative for a tapered solid by design: a bound that
    /// is slightly loose costs a few empty lattice nodes, and one that is tight
    /// by a rounding error clips the body.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let signs = [-1.0_f32, 1.0];
        grow(
            (0..8_usize)
                .map(|corner| {
                    self.centre.add(self.rotation.rotate(Vec3::new(
                        signs[corner & 1] * self.half_extents.x,
                        signs[(corner >> 1) & 1] * self.half_extents.y,
                        signs[(corner >> 2) & 1] * self.half_extents.z,
                    )))
                })
                .fold(EMPTY_BOUNDS, |(low, high), corner| {
                    (component_min(low, corner), component_max(high, corner))
                }),
            self.base.max(self.tip),
        )
    }
}

/// The rotation taking local `−Z` to the unit direction `along` — the shortest
/// arc between them, built directly rather than through
/// [`Quat::look_rotation`].
///
/// A solid of revolution has no roll to get wrong, so there is no reference-up
/// to supply and no parallel-to-up case to fail on: `look_rotation` would force
/// this constructor to carry an error arm that nothing can reach, which the
/// Coverage Law reads (correctly) as a design signal rather than an exception.
///
/// The shortest arc is `(axis = −Z × along, w = 1 + (−Z · along))`, unnormalized;
/// [`Solid::new`] normalizes it. Its one degenerate input is `along == +Z`, the
/// half-turn, where the axis vanishes and *any* perpendicular axis is correct —
/// so that case selects a half-turn about `X`, which sends `−Z` to `+Z`.
fn aim_negative_z(along: Vec3) -> Quat {
    let arc = Quat::new(along.y, -along.x, 0.0, 1.0 - along.z);
    [arc, HALF_TURN_ABOUT_X][usize::from(arc.length_squared() < EPSILON)]
}

/// The rotation the [`aim_negative_z`] half-turn case resolves to.
const HALF_TURN_ABOUT_X: Quat = Quat::new(1.0, 0.0, 0.0, 0.0);
