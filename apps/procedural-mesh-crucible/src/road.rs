//! The road and the tunnel: two sweeps along the scene's curves.
//!
//! Both are `axiom_mesh_ops::sweep`, and the difference between them is entirely
//! in the profile and the cap policy — which is exactly the claim a crucible
//! should be able to make about a sweep operator. The road is a wide flat slab
//! carried along the road spline and progressively banked with
//! `SweepOptions::twist`; the tunnel is a closed **arch shell** (an outer arc and
//! an inner arc joined into one ring) carried along the derived tunnel spline
//! with `CapPolicy::None`, so you can drive into it.
//!
//! ## Why `initial_reference` is `+X` and not the default `+Y`
//!
//! A sweep places a profile's local `+X` on the frame normal and its local `+Y`
//! on the binormal. The road spline is close to horizontal, so seeding the frame
//! with `+Y` would put the road's *width* on the vertical axis and stand the
//! slab on edge. Seeding with `+X` puts width across the road and thickness
//! upward, which is what the profile is authored for. This is a real constraint
//! of the operator, not a workaround: the seed says which way the cross-section
//! is "up", and only the caller knows that.

use axiom_kernel::Radians;
use axiom_math::{Curve, Vec2, Vec3};
use axiom_mesh::{Mesh, MeshResult};
use axiom_mesh_ops::{sweep, CapPolicy, Profile, Samples, SweepOptions};

use crate::quantities::{radians, ratio};
use crate::variant::DetailParams;

/// Half the road's width, in metres.
const ROAD_HALF_WIDTH: f32 = 5.0;
/// Half the road slab's thickness, in metres.
const ROAD_HALF_THICKNESS: f32 = 0.35;
/// Total bank applied across the whole road, proportional to arc length.
const ROAD_BANK_RADIANS: f32 = 0.42;

/// The tunnel arch's outer and inner radii, in metres.
const TUNNEL_OUTER: f32 = 7.4;
const TUNNEL_INNER: f32 = 6.2;

/// The banked road: a flat slab profile swept along the road spline, twisted
/// progressively so the surface banks into the run.
pub fn road_mesh(path: &Curve, params: DetailParams) -> MeshResult<Mesh> {
    Profile::closed(vec![
        Vec2::new(-ROAD_HALF_WIDTH, -ROAD_HALF_THICKNESS),
        Vec2::new(ROAD_HALF_WIDTH, -ROAD_HALF_THICKNESS),
        Vec2::new(ROAD_HALF_WIDTH, ROAD_HALF_THICKNESS),
        Vec2::new(-ROAD_HALF_WIDTH, ROAD_HALF_THICKNESS),
    ])
    .and_then(|profile| {
        Samples::new(params.road_samples).map(|samples| (profile, samples))
    })
    .and_then(|(profile, samples)| {
        sweep(
            &profile,
            path,
            samples,
            SweepOptions {
                caps: CapPolicy::Both,
                twist: radians(ROAD_BANK_RADIANS),
                start_scale: ratio(1.0),
                end_scale: ratio(1.0),
                closed_path: false,
                initial_reference: Vec3::UNIT_X,
            },
        )
    })
}

/// The tunnel: an arch-shell cross-section swept along the derived tunnel
/// spline, open at both ends.
pub fn tunnel_mesh(path: &Curve, params: DetailParams) -> MeshResult<Mesh> {
    arch_profile(params.ring_segments)
        .and_then(|profile| {
            Samples::new(params.tunnel_samples).map(|samples| (profile, samples))
        })
        .and_then(|(profile, samples)| {
            sweep(
                &profile,
                path,
                samples,
                SweepOptions {
                    caps: CapPolicy::None,
                    twist: Radians::finite_or_zero(0.0),
                    start_scale: ratio(1.0),
                    end_scale: ratio(1.0),
                    closed_path: false,
                    initial_reference: Vec3::UNIT_X,
                },
            )
        })
}

/// A closed arch ring: the outer arc swept `0..pi`, then the inner arc swept
/// back `pi..0`. The result is a single non-convex closed outline — a real test
/// of the operator, because a convex ring would not exercise the profile's
/// winding normalisation or its perimeter parameterisation the same way.
fn arch_profile(segments: u32) -> MeshResult<Profile> {
    let steps = segments.max(6);
    let outer = (0..=steps).map(|i| arc_point(TUNNEL_OUTER, i, steps));
    let inner = (0..=steps).rev().map(|i| arc_point(TUNNEL_INNER, i, steps));
    Profile::closed(outer.chain(inner).collect())
}

/// One point on a half-circle of `radius`, at step `i` of `steps`.
fn arc_point(radius: f32, i: u32, steps: u32) -> Vec2 {
    let angle = core::f32::consts::PI * (i as f32 / steps as f32);
    Vec2::new(radius * angle.cos(), radius * angle.sin())
}
