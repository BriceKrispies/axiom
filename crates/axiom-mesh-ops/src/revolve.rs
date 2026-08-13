//! Revolving a 2D profile about an axis — the lathe.
//!
//! A revolution takes a silhouette and spins it: the profile is read as
//! `(radius, height)` in the half-plane containing the axis, and every point
//! traces a circular arc about it. It is the operator behind wheels, bottles,
//! goblets, columns, bowls, dishes, pipes and every other object whose shape is
//! entirely described by its outline.
//!
//! ## The seam, and why the last ring is duplicated
//!
//! A full `TAU` revolution has to close. There are two ways to do that: wrap the
//! index arithmetic so the last span points back at ring `0`, or emit one more
//! ring that is *positionally identical* to ring `0` and stitch to it normally.
//! This operator does the second, because it is the only one that lets `u` reach
//! `1.0`. Collapsing the seam would force the last quad's texture coordinate to
//! run `0.9375 -> 0` and shear the last column of every wrapped texture — the
//! exact defect the engine's "duplicate a seam vertex, never collapse a seam"
//! convention exists to prevent.
//!
//! The duplicate is not a crack: the seam ring's angular index is folded back to
//! zero, so its positions are computed by the *same* expression as ring `0` and
//! are bit-identical, not merely close. A partial revolution has no seam at all
//! and leaves its two ends open for [`CapPolicy`] to close.
//!
//! ## Orientation
//!
//! The angular basis is built deterministically from the axis alone
//! (see [`crate::sweep_frames`]), never from a world up-vector, so revolving
//! about a tilted axis is as well-defined as revolving about `+Y`. The profile
//! is normalised to counter-clockwise in `(radius, height)`, which makes an
//! outer wall face away from the axis and an inner wall face toward it — exactly
//! what a tube wants. A **negative** angle sweeps the other way and is fully
//! supported: the rings are generated in increasing angle regardless, so the
//! triangle winding never inverts, and the cap policy still refers to the
//! caller's own start (`angle = 0`) and end.

use axiom_kernel::Radians;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{combine, Mesh, MeshError, MeshErrorCode, MeshResult};

use crate::cap_policy::CapPolicy;
use crate::polygon_triangulation::triangulate_profile;
use crate::profile::Profile;
use crate::sweep::{cap_mesh, column_arc, column_normals, column_points, oriented_ccw, stitch_rings};
use crate::sweep_frames::seed_normal;
use crate::tessellation::Segments;

/// Below this squared length an axis is not a direction.
const AXIS_EPSILON: f32 = 1.0e-12;

/// Below this an angle is no rotation at all; also the tolerance on "is this a
/// whole turn".
const ANGLE_EPSILON: f32 = 1.0e-6;

/// Revolve `profile` about `axis` through `angle`, in `segments` angular steps.
///
/// The profile's `x` is a **radius** from the axis and its `y` is a **height**
/// along it, so `Profile::closed([(1,-1), (2,-1), (2,1), (1,1)])` revolved a
/// whole turn is a tube of inner radius 1 and outer radius 2.
///
/// # Errors
///
/// - [`MeshErrorCode::DegenerateAxis`] when `axis` is zero-length or not finite.
/// - [`MeshErrorCode::InvalidParameter`] when `angle` is zero, or larger than a
///   whole turn in either direction.
/// - [`MeshErrorCode::InvalidProfile`] from cap triangulation.
pub fn revolve(
    profile: &Profile,
    axis: Vec3,
    angle: Radians,
    segments: Segments,
    caps: CapPolicy,
) -> MeshResult<Mesh> {
    usable_axis(axis)
        .and_then(|unit| usable_angle(angle).map(|turn| (unit, turn)))
        .and_then(|(unit, turn)| build_revolution(profile, unit, turn, segments, caps))
}

/// The axis as a unit vector, or [`MeshErrorCode::DegenerateAxis`].
fn usable_axis(axis: Vec3) -> MeshResult<Vec3> {
    let squared = axis.length_squared();
    (squared.is_finite() & (squared > AXIS_EPSILON))
        .then(|| axis.normalize().unwrap_or(Vec3::UNIT_Y))
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::DegenerateAxis,
                "a revolution axis must be finite and longer than zero",
            )
        })
}

/// The signed turn, or [`MeshErrorCode::InvalidParameter`].
fn usable_angle(angle: Radians) -> MeshResult<f32> {
    let turn = angle.get();
    ((turn.abs() > ANGLE_EPSILON) & (turn.abs() <= core::f32::consts::TAU + ANGLE_EPSILON))
        .then_some(turn)
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a revolution angle must be non-zero and no more than a whole turn",
            )
        })
}

/// Place every angular ring, skin them, and cap a partial revolution.
fn build_revolution(
    profile: &Profile,
    unit_axis: Vec3,
    turn: f32,
    segments: Segments,
    caps: CapPolicy,
) -> MeshResult<Mesh> {
    // A deterministic perpendicular, plus the second in-plane axis. `radial x
    // axis` (rather than `axis x radial`) is the choice that makes a
    // counter-clockwise `(radius, height)` profile emit outward-facing quads.
    let radial = seed_normal(unit_axis, Vec3::ZERO);
    let lateral = radial.cross(unit_axis);
    let oriented = oriented_ccw(profile);
    let columns = column_points(&oriented);
    let along = column_arc(&columns);
    let outward = column_normals(&oriented);
    let steps = segments.get() as usize;
    let whole = (turn.abs() - core::f32::consts::TAU).abs() <= ANGLE_EPSILON;
    let angles = ring_angles(turn, steps, whole);
    let rings: Vec<Vec<Vec3>> = angles
        .iter()
        .map(|a| place(*a, &columns, radial, lateral, unit_axis))
        .collect();
    let normals: Vec<Vec<Vec3>> = angles
        .iter()
        .map(|a| place(*a, &outward, radial, lateral, unit_axis))
        .collect();
    let uvs: Vec<Vec<Vec2>> = (0..=steps)
        .map(|i| {
            let u = i as f32 / steps as f32;
            along.iter().map(|v| Vec2::new(u, *v)).collect()
        })
        .collect();
    Mesh::from_streams(stitch_rings(&rings, &normals, &uvs, false))
        .and_then(|side| {
            revolution_caps(&oriented, &rings, &angles, radial, lateral, whole, caps)
                .map(|ends| (side, ends))
        })
        .and_then(|(side, ends)| {
            combine(&core::iter::once(side).chain(ends).collect::<Vec<Mesh>>())
        })
}

/// The angle of every ring, always in **increasing** order so the emitted
/// winding never depends on the sign of `turn`. A whole turn folds the final
/// ring's index back to zero, which makes its positions bit-identical to ring
/// `0` rather than merely within a rounding error of it.
fn ring_angles(turn: f32, steps: usize, whole: bool) -> Vec<f32> {
    let from = turn.min(0.0);
    let span = turn.abs();
    (0..=steps)
        .map(|i| {
            let folded = i * usize::from(!(whole & (i == steps)));
            from + span * folded as f32 / steps as f32
        })
        .collect()
}

/// Map `(radius, height)` pairs into 3D at one angle about the axis.
///
/// The same linear map carries positions and normals: a profile normal
/// `(nr, nh)` becomes `radial_direction * nr + axis * nh`, which is the exact
/// surface normal of the revolved shell.
fn place(
    angle: f32,
    values: &[Vec2],
    radial: Vec3,
    lateral: Vec3,
    unit_axis: Vec3,
) -> Vec<Vec3> {
    let direction = radial
        .mul_scalar(angle.cos())
        .add(lateral.mul_scalar(angle.sin()));
    values
        .iter()
        .map(|v| direction.mul_scalar(v.x).add(unit_axis.mul_scalar(v.y)))
        .collect()
}

/// The zero, one or two flat ends of a partial revolution.
///
/// `wanted[0]` is the caller's *start* (`angle = 0`) and `wanted[1]` their
/// *end*; for a negative turn those live at the last and first ring
/// respectively, because the rings themselves always run in increasing angle.
fn revolution_caps(
    oriented: &Profile,
    rings: &[Vec<Vec3>],
    angles: &[f32],
    radial: Vec3,
    lateral: Vec3,
    whole: bool,
    caps: CapPolicy,
) -> MeshResult<Vec<Mesh>> {
    let eligible = !whole & oriented.is_closed();
    let wanted = [
        eligible & caps.caps_start(),
        eligible & caps.caps_end(),
    ];
    let last = rings.len() - 1;
    let reversed_order = usize::from(angles[0] < 0.0);
    let ends = [[0usize, last][reversed_order], [last, 0][reversed_order]];
    (wanted[0] | wanted[1])
        .then(|| triangulate_profile(oriented))
        .transpose()
        .map(Option::unwrap_or_default)
        .and_then(|triangles| {
            (0..2)
                .filter(|end| wanted[*end])
                .map(|end| {
                    let at = ends[end];
                    let leading = at == 0;
                    let angle = angles[at];
                    let sweep = radial
                        .mul_scalar(-angle.sin())
                        .add(lateral.mul_scalar(angle.cos()));
                    let facing = sweep.mul_scalar([1.0, -1.0][usize::from(leading)]);
                    cap_mesh(
                        &rings[at][..oriented.point_count()],
                        oriented.points(),
                        &triangles,
                        facing,
                        leading,
                    )
                })
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use core::f32::consts::{FRAC_PI_2, PI, TAU};

    fn radians(v: f32) -> Radians {
        Radians::new(v).unwrap()
    }

    fn segments(n: u32) -> Segments {
        Segments::new(n).unwrap()
    }

    /// A tube wall: inner radius 1, outer radius 2, height -1 .. 1.
    fn tube() -> Profile {
        Profile::closed(vec![
            Vec2::new(1.0, -1.0),
            Vec2::new(2.0, -1.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(1.0, 1.0),
        ])
        .unwrap()
    }

    /// A solid disc silhouette touching the axis: revolves into a closed
    /// cylinder (or, partially, into a convex wedge).
    fn solid() -> Profile {
        Profile::closed(vec![
            Vec2::new(0.0, -1.0),
            Vec2::new(2.0, -1.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(0.0, 1.0),
        ])
        .unwrap()
    }

    fn radius_about_y(p: Vec3) -> f32 {
        (p.x * p.x + p.z * p.z).sqrt()
    }

    fn faces_outward(mesh: &Mesh, interior: Vec3) -> bool {
        mesh.indices().chunks(3).all(|t| {
            let p = mesh.positions();
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let out = a.add(b).add(c).mul_scalar(1.0 / 3.0).subtract(interior);
            let geometric = b.subtract(a).cross(c.subtract(a));
            geometric.length() < 1.0e-7 || geometric.dot(out) > 0.0
        })
    }

    #[test]
    fn a_whole_turn_of_a_rectangle_is_a_tube() {
        let mesh = revolve(
            &tube(),
            Vec3::UNIT_Y,
            radians(TAU),
            segments(12),
            CapPolicy::Both,
        )
        .unwrap();
        // 13 rings x 5 columns; the cap policy is ignored, a whole turn has no
        // ends to cap.
        assert_eq!(mesh.vertex_count(), 13 * 5);
        assert_eq!(mesh.triangle_count(), 12 * 4 * 2);
        for p in mesh.positions() {
            let r = radius_about_y(*p);
            assert!(
                (r - 1.0).abs() < 1.0e-4 || (r - 2.0).abs() < 1.0e-4,
                "unexpected radius {r}"
            );
            assert!((p.y.abs() - 1.0).abs() < 1.0e-5);
        }
    }

    #[test]
    fn the_seam_ring_is_bit_identical_to_the_first() {
        let mesh = revolve(
            &tube(),
            Vec3::UNIT_Y,
            radians(TAU),
            segments(8),
            CapPolicy::None,
        )
        .unwrap();
        let columns = 5;
        for j in 0..columns {
            assert_eq!(mesh.positions()[j], mesh.positions()[8 * columns + j]);
            assert_eq!(mesh.normals()[j], mesh.normals()[8 * columns + j]);
        }
        // ... and u still reaches both ends of the range.
        assert_eq!(mesh.uvs()[0].x, 0.0);
        assert_eq!(mesh.uvs()[8 * columns].x, 1.0);
    }

    #[test]
    fn an_outer_wall_faces_away_from_the_axis_and_an_inner_wall_toward_it() {
        let mesh = revolve(
            &tube(),
            Vec3::UNIT_Y,
            radians(TAU),
            segments(16),
            CapPolicy::None,
        )
        .unwrap();
        for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
            let outward = Vec3::new(p.x, 0.0, p.z).normalize().unwrap();
            let radial_component = n.dot(outward);
            // Corner vertices average a wall normal with a face normal, so the
            // sign is the assertion, not the magnitude.
            let expected = [-1.0f32, 1.0][usize::from(radius_about_y(*p) > 1.5)];
            assert!(
                radial_component * expected > 0.0,
                "wall normal pointed the wrong way: {n:?} at {p:?}"
            );
        }
    }

    #[test]
    fn a_whole_turn_of_a_silhouette_touching_the_axis_is_a_closed_solid() {
        let mesh = revolve(
            &solid(),
            Vec3::UNIT_Y,
            radians(TAU),
            segments(20),
            CapPolicy::None,
        )
        .unwrap();
        assert!(faces_outward(&mesh, Vec3::ZERO));
        let widest = mesh
            .positions()
            .iter()
            .fold(0.0f32, |m, p| m.max(radius_about_y(*p)));
        assert!((widest - 2.0).abs() < 1.0e-4);
    }

    #[test]
    fn a_bottle_silhouette_revolves_into_a_bottle() {
        // Base, body, shoulder, neck, lip, back to the axis: a silhouette no
        // primitive generator could express.
        let bottle = Profile::closed(vec![
            Vec2::new(0.0, -2.0),
            Vec2::new(1.5, -2.0),
            Vec2::new(1.5, 0.0),
            Vec2::new(0.6, 0.6),
            Vec2::new(0.5, 2.0),
            Vec2::new(0.8, 2.2),
            Vec2::new(0.0, 2.2),
        ])
        .unwrap();
        let mesh = revolve(
            &bottle,
            Vec3::UNIT_Y,
            radians(TAU),
            segments(24),
            CapPolicy::None,
        )
        .unwrap();
        assert_eq!(mesh.vertex_count(), 25 * 8);
        assert_eq!(mesh.triangle_count(), 24 * 7 * 2);
        // The silhouette is concave at the shoulder, so a convexity test would
        // be wrong here; what must hold is that the widest wall looks outward.
        for (p, n) in mesh.positions().iter().zip(mesh.normals()) {
            let r = radius_about_y(*p);
            // Points on the axis have no radial direction to compare against.
            if r > 1.4 {
                let outward = Vec3::new(p.x, 0.0, p.z).normalize().unwrap();
                assert!(
                    n.dot(outward) > 0.5,
                    "body wall normal {n:?} did not face outward at radius {r}"
                );
            }
        }
        let (widest, tallest, lowest) = mesh.positions().iter().fold(
            (0.0f32, f32::NEG_INFINITY, f32::INFINITY),
            |(w, hi, lo), p| (w.max(radius_about_y(*p)), hi.max(p.y), lo.min(p.y)),
        );
        assert!((widest - 1.5).abs() < 1.0e-4);
        assert!((tallest - 2.2).abs() < 1.0e-5);
        assert!((lowest + 2.0).abs() < 1.0e-5);
        // The neck really is narrower than the body.
        let neck_widest = mesh
            .positions()
            .iter()
            .filter(|p| p.y > 1.9 && p.y < 2.1)
            .fold(0.0f32, |m, p| m.max(radius_about_y(*p)));
        assert!(neck_widest < 0.55, "neck was {neck_widest}");
    }

    #[test]
    fn a_quarter_turn_leaves_two_open_ends_that_the_cap_policy_closes() {
        let bare = revolve(
            &tube(),
            Vec3::UNIT_Y,
            radians(FRAC_PI_2),
            segments(8),
            CapPolicy::None,
        )
        .unwrap();
        assert_eq!(bare.vertex_count(), 9 * 5);
        assert_eq!(bare.triangle_count(), 8 * 4 * 2);

        let capped = revolve(
            &tube(),
            Vec3::UNIT_Y,
            radians(FRAC_PI_2),
            segments(8),
            CapPolicy::Both,
        )
        .unwrap();
        assert_eq!(capped.vertex_count(), bare.vertex_count() + 8);
        assert_eq!(capped.triangle_count(), bare.triangle_count() + 4);
        // The start cap lies in the z = 0 plane and looks along -Z; the end cap
        // lies in x = 0 and looks along -X.
        let side = bare.vertex_count();
        assert!(capped.normals()[side]
            .subtract(Vec3::new(0.0, 0.0, -1.0))
            .length()
            < 1.0e-5);
        assert!(capped.normals()[side + 4]
            .subtract(Vec3::new(-1.0, 0.0, 0.0))
            .length()
            < 1.0e-5);
    }

    #[test]
    fn each_single_ended_cap_policy_adds_exactly_one_cap() {
        let count = |caps| {
            revolve(&tube(), Vec3::UNIT_Y, radians(PI), segments(6), caps)
                .unwrap()
                .triangle_count()
        };
        let bare = count(CapPolicy::None);
        assert_eq!(count(CapPolicy::Start), bare + 2);
        assert_eq!(count(CapPolicy::End), bare + 2);
        assert_eq!(count(CapPolicy::Both), bare + 4);
    }

    #[test]
    fn a_quarter_turn_wedge_is_wound_outward_with_its_caps() {
        let mesh = revolve(
            &solid(),
            Vec3::UNIT_Y,
            radians(FRAC_PI_2),
            segments(8),
            CapPolicy::Both,
        )
        .unwrap();
        // Every position is in the +X/+Z quadrant.
        for p in mesh.positions() {
            assert!(p.x > -1.0e-5 && p.z > -1.0e-5);
        }
        assert!(faces_outward(&mesh, Vec3::new(0.6, 0.0, 0.6)));
    }

    #[test]
    fn a_negative_turn_sweeps_the_other_way_without_inverting_the_winding() {
        let mesh = revolve(
            &solid(),
            Vec3::UNIT_Y,
            radians(-FRAC_PI_2),
            segments(8),
            CapPolicy::Both,
        )
        .unwrap();
        for p in mesh.positions() {
            assert!(p.x > -1.0e-5 && p.z < 1.0e-5);
        }
        assert!(faces_outward(&mesh, Vec3::new(0.6, 0.0, -0.6)));
        // The caller's start is still the `angle = 0` end, whichever ring it
        // landed on: its cap looks along +Z, out of the z < 0 wedge.
        let side = 9 * 5;
        assert!(mesh.normals()[side].subtract(Vec3::UNIT_Z).length() < 1.0e-5);
        assert!(mesh.normals()[side + 4]
            .subtract(Vec3::new(-1.0, 0.0, 0.0))
            .length()
            < 1.0e-5);
    }

    #[test]
    fn a_tilted_axis_revolves_just_as_well() {
        let axis = Vec3::new(1.0, 1.0, 0.0);
        let unit = axis.normalize().unwrap();
        let mesh = revolve(&tube(), axis, radians(TAU), segments(12), CapPolicy::None).unwrap();
        for p in mesh.positions() {
            let along = p.dot(unit);
            let radial = p.subtract(unit.mul_scalar(along)).length();
            assert!(
                (radial - 1.0).abs() < 1.0e-4 || (radial - 2.0).abs() < 1.0e-4,
                "radius about the tilted axis was {radial}"
            );
            assert!((along.abs() - 1.0).abs() < 1.0e-4);
        }
    }

    #[test]
    fn an_open_profile_revolves_into_a_shell_and_ignores_the_cap_policy() {
        let bowl = Profile::open(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.1),
            Vec2::new(1.8, 0.8),
            Vec2::new(2.0, 1.6),
        ])
        .unwrap();
        let mesh = revolve(
            &bowl,
            Vec3::UNIT_Y,
            radians(TAU),
            segments(10),
            CapPolicy::Both,
        )
        .unwrap();
        // 11 rings x 4 columns (no seam duplicate on an open profile), no caps.
        assert_eq!(mesh.vertex_count(), 11 * 4);
        assert_eq!(mesh.triangle_count(), 10 * 3 * 2);
    }

    #[test]
    fn a_zero_length_axis_is_a_degenerate_axis() {
        assert_eq!(
            revolve(
                &tube(),
                Vec3::ZERO,
                radians(TAU),
                segments(8),
                CapPolicy::None
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::DegenerateAxis
        );
    }

    #[test]
    fn a_non_finite_axis_is_a_degenerate_axis() {
        assert_eq!(
            revolve(
                &tube(),
                Vec3::new(f32::NAN, 1.0, 0.0),
                radians(TAU),
                segments(8),
                CapPolicy::None
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::DegenerateAxis
        );
        assert_eq!(
            revolve(
                &tube(),
                Vec3::new(0.0, f32::INFINITY, 0.0),
                radians(TAU),
                segments(8),
                CapPolicy::None
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::DegenerateAxis
        );
    }

    #[test]
    fn a_zero_angle_is_an_invalid_parameter() {
        assert_eq!(
            revolve(
                &tube(),
                Vec3::UNIT_Y,
                radians(0.0),
                segments(8),
                CapPolicy::None
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn more_than_a_whole_turn_is_an_invalid_parameter() {
        assert_eq!(
            revolve(
                &tube(),
                Vec3::UNIT_Y,
                radians(TAU + 0.5),
                segments(8),
                CapPolicy::None
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            revolve(
                &tube(),
                Vec3::UNIT_Y,
                radians(-TAU - 0.5),
                segments(8),
                CapPolicy::None
            )
            .unwrap_err()
            .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_clockwise_profile_is_normalised_before_revolving() {
        let mesh = revolve(
            &solid().reversed(),
            Vec3::UNIT_Y,
            radians(TAU),
            segments(16),
            CapPolicy::None,
        )
        .unwrap();
        assert!(faces_outward(&mesh, Vec3::ZERO));
    }

    #[test]
    fn a_revolution_is_reproducible() {
        let build = || {
            revolve(
                &tube(),
                Vec3::new(0.3, 1.0, -0.2),
                radians(1.7),
                segments(9),
                CapPolicy::Both,
            )
            .unwrap()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn a_whole_turn_is_accepted_at_the_exact_boundary() {
        assert!(revolve(
            &tube(),
            Vec3::UNIT_Y,
            radians(TAU),
            segments(3),
            CapPolicy::None
        )
        .is_ok());
        assert!(revolve(
            &tube(),
            Vec3::UNIT_Y,
            radians(-TAU),
            segments(3),
            CapPolicy::None
        )
        .is_ok());
    }

    #[test]
    fn a_meter_sized_profile_keeps_its_dimensions() {
        // A guard that the operator never rescales what it is handed.
        let half = Meters::new(0.5).unwrap();
        let disc = Profile::closed(vec![
            Vec2::new(0.0, -half.get()),
            Vec2::new(3.0, -half.get()),
            Vec2::new(3.0, half.get()),
            Vec2::new(0.0, half.get()),
        ])
        .unwrap();
        let mesh = revolve(
            &disc,
            Vec3::UNIT_Y,
            radians(TAU),
            segments(12),
            CapPolicy::None,
        )
        .unwrap();
        let widest = mesh
            .positions()
            .iter()
            .fold(0.0f32, |m, p| m.max(radius_about_y(*p)));
        assert!((widest - 3.0).abs() < 1.0e-4);
        assert!(mesh.positions().iter().all(|p| p.y.abs() <= 0.5 + 1.0e-6));
    }
}
