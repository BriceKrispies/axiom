//! End-to-end proof of the static **triangle-soup** collider: a level's
//! collision geometry attaches as a triangle buffer, gets a BVH built over it,
//! and answers ray queries through the public facade — including from a rotated
//! body, which is the case that proves the ray is taken into the soup's local
//! space rather than the soup being assumed axis-aligned at the origin.

use axiom_kernel::{Meters, Ratio};
use axiom_math::{Quat, Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};

fn r(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn m(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

fn world() -> PhysicsApi {
    PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 64, 64, 1, true, r(0.0), r(0.05)).unwrap()
}

/// A flat `size × size` floor at `y = 0`, as two triangles.
fn floor(size: f32) -> Vec<f32> {
    vec![
        -size, 0.0, -size, size, 0.0, -size, size, 0.0, size, //
        -size, 0.0, -size, size, 0.0, size, -size, 0.0, size,
    ]
}

fn attach(px: &mut PhysicsApi, at: Transform, soup: &[f32]) -> PhysicsBodyHandle {
    let mat = PhysicsApi::material(r(0.5), r(0.0), r(1.0)).unwrap();
    let body = px.create_static_body(at).unwrap();
    px.attach_triangle_soup_collider(body, soup, mat, false)
        .expect("a well-formed soup attaches");
    body
}

#[test]
fn a_ray_finds_the_soup_and_reports_where_it_met_it() {
    let mut px = world();
    attach(&mut px, Transform::from_translation(Vec3::ZERO), &floor(5.0));

    let hit = px
        .raycast(Vec3::new(1.0, 4.0, 1.0), Vec3::new(0.0, -1.0, 0.0), m(20.0))
        .expect("straight down onto the floor");

    assert!(
        (hit.distance().get() - 4.0).abs() < 1e-3,
        "expected to travel 4m, got {}",
        hit.distance().get()
    );
    assert!(
        (hit.point().y - 0.0).abs() < 1e-3,
        "should meet the floor at y=0, got {}",
        hit.point().y
    );
    // The normal faces the caster, which is up for a ray coming down.
    assert!(hit.normal().y > 0.5, "normal {:?}", hit.normal());
}

#[test]
fn a_ray_that_misses_the_soup_reports_nothing() {
    let mut px = world();
    attach(&mut px, Transform::from_translation(Vec3::ZERO), &floor(1.0));

    // Well outside the 1m floor.
    assert!(px
        .raycast(Vec3::new(20.0, 4.0, 20.0), Vec3::new(0.0, -1.0, 0.0), m(20.0))
        .is_none());
    // Pointing away from it.
    assert!(px
        .raycast(Vec3::new(0.0, 4.0, 0.0), Vec3::new(0.0, 1.0, 0.0), m(20.0))
        .is_none());
    // Stopping short of it.
    assert!(px
        .raycast(Vec3::new(0.0, 4.0, 0.0), Vec3::new(0.0, -1.0, 0.0), m(1.0))
        .is_none());
}

/// The case a soup assumed to sit at the origin would get wrong.
#[test]
fn the_soup_moves_and_rotates_with_its_body() {
    let mut px = world();
    // The floor is raised 3m and stood on its edge by a quarter turn about X,
    // which turns the horizontal floor into a vertical wall in the XY plane.
    let at = Transform::new(
        Vec3::new(0.0, 3.0, 0.0),
        Quat::from_euler_xyz(core::f32::consts::FRAC_PI_2, 0.0, 0.0),
        Vec3::ONE,
    );
    attach(&mut px, at, &floor(5.0));

    // A ray along +Z at the body's height now meets that wall.
    let hit = px
        .raycast(Vec3::new(0.0, 3.0, -4.0), Vec3::new(0.0, 0.0, 1.0), m(20.0))
        .expect("the rotated soup is a wall in the way");
    assert!(
        (hit.distance().get() - 4.0).abs() < 1e-3,
        "expected 4m to the wall, got {}",
        hit.distance().get()
    );

    // ...and straight down through where the floor used to be hits nothing,
    // because it is no longer horizontal.
    assert!(
        px.raycast(Vec3::new(2.0, 9.0, 2.0), Vec3::new(0.0, -1.0, 0.0), m(20.0))
            .is_none(),
        "a wall should not be hit by a vertical ray beside it"
    );
}

#[test]
fn the_nearest_of_two_soups_wins() {
    let mut px = world();
    attach(&mut px, Transform::from_translation(Vec3::ZERO), &floor(5.0));
    attach(
        &mut px,
        Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),
        &floor(5.0),
    );

    let hit = px
        .raycast(Vec3::new(0.5, 6.0, 0.5), Vec3::new(0.0, -1.0, 0.0), m(20.0))
        .expect("hits something");
    assert!(
        (hit.distance().get() - 4.0).abs() < 1e-3,
        "should stop at the upper floor (4m), got {}",
        hit.distance().get()
    );
}

#[test]
fn raycast_all_reports_every_soup_along_the_ray_nearest_first() {
    let mut px = world();
    attach(&mut px, Transform::from_translation(Vec3::ZERO), &floor(5.0));
    attach(
        &mut px,
        Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),
        &floor(5.0),
    );

    let hits = px.raycast_all(Vec3::new(0.5, 6.0, 0.5), Vec3::new(0.0, -1.0, 0.0), m(20.0));
    assert_eq!(hits.len(), 2, "both floors are in the way");
    assert!(hits[0].distance().get() < hits[1].distance().get(), "nearest first");
}

#[test]
fn a_buffer_with_no_complete_triangle_is_refused() {
    let mut px = world();
    let mat = PhysicsApi::material(r(0.5), r(0.0), r(1.0)).unwrap();
    let body = px.create_static_body(Transform::IDENTITY).unwrap();

    assert!(px
        .attach_triangle_soup_collider(body, &[], mat, false)
        .is_err());
    // Eight floats is not a triangle.
    assert!(px
        .attach_triangle_soup_collider(body, &[0.0; 8], mat, false)
        .is_err());
}

/// A soup flat on one axis is the normal case, not a degenerate one — a floor
/// is flat on Y and a wall is flat on X or Z. An earlier version of the shape
/// validation demanded positive extent on all three axes and rejected every
/// floor in the world; this is the test that would have caught it.
#[test]
fn a_soup_flat_on_one_axis_is_a_floor_not_an_error() {
    let mut px = world();
    let mat = PhysicsApi::material(r(0.5), r(0.0), r(1.0)).unwrap();
    let body = px.create_static_body(Transform::IDENTITY).unwrap();

    // Every vertex at y = 0: zero reach on Y, which is what a floor is.
    let flat = [-1.0, 0.0, -1.0, 1.0, 0.0, -1.0, 1.0, 0.0, 1.0];
    assert!(px
        .attach_triangle_soup_collider(body, &flat, mat, false)
        .is_ok());
}

/// A non-finite vertex is a broken buffer, and is refused.
#[test]
fn a_soup_with_a_non_finite_vertex_is_refused() {
    let mut px = world();
    let mat = PhysicsApi::material(r(0.5), r(0.0), r(1.0)).unwrap();
    let body = px.create_static_body(Transform::IDENTITY).unwrap();

    let broken = [0.0, 0.0, 0.0, 1.0, f32::INFINITY, 0.0, 0.0, 0.0, 1.0];
    assert!(px
        .attach_triangle_soup_collider(body, &broken, mat, false)
        .is_err());
}

#[test]
fn attaching_to_a_body_that_does_not_exist_is_refused() {
    let mut px = world();
    let mat = PhysicsApi::material(r(0.5), r(0.0), r(1.0)).unwrap();
    let ghost = PhysicsBodyHandle::from_raw(9999);
    assert!(px
        .attach_triangle_soup_collider(ghost, &floor(1.0), mat, false)
        .is_err());
}

/// The other two query tables carry a soup entry so they stay exhaustive, and
/// both report nothing: a static soup answers rays today and is not yet swept or
/// overlapped against. These pin that as a stated behaviour rather than an
/// accident, so the day it changes, a test says so.
#[test]
fn a_soup_is_not_yet_overlapped_or_swept_against() {
    let mut px = world();
    attach(&mut px, Transform::from_translation(Vec3::ZERO), &floor(5.0));

    // A capsule sitting right on the floor overlaps nothing.
    assert!(px
        .overlap_capsule(Vec3::new(0.0, 0.0, 0.0), Quat::IDENTITY, m(1.0), m(1.0))
        .is_empty());
    assert!(px.overlap_sphere(Vec3::ZERO, m(2.0)).is_empty());

    // ...and a capsule driven down through it passes straight through.
    assert!(px
        .capsule_cast(
            Vec3::new(0.0, 4.0, 0.0),
            Quat::IDENTITY,
            m(0.5),
            m(0.5),
            Vec3::new(0.0, -8.0, 0.0),
        )
        .is_none());
}
