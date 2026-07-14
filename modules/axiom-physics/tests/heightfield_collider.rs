//! End-to-end proof of the static **heightfield** collider: a dynamic sphere
//! dropped above a flat heightfield settles on its surface (it neither floats nor
//! tunnels through), a sphere dropped above a tilted heightfield slides downhill,
//! and the facade validates the grid dimensions.

use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

fn step(n: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(n), Tick::new(n), 16_666_667, n)
}

fn r(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn m(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

fn world() -> PhysicsApi {
    PhysicsApi::with_config(
        Vec3::new(0.0, -9.8, 0.0),
        8,
        64,
        64,
        1,
        true,
        r(0.0),
        r(0.05),
    )
    .unwrap()
}

fn ball_y(px: &PhysicsApi, ball: PhysicsBodyHandle) -> f32 {
    px.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == ball)
        .unwrap()
        .transform()
        .translation
        .y
}

fn ball_xz(px: &PhysicsApi, ball: PhysicsBodyHandle) -> (f32, f32) {
    let t = px
        .snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == ball)
        .unwrap()
        .transform()
        .translation;
    (t.x, t.z)
}

#[test]
fn a_sphere_settles_on_a_flat_heightfield() {
    let mut px = world();
    let mat = PhysicsApi::material(r(0.9), r(0.0), r(1.0)).unwrap();
    let ground = px
        .create_static_body(Transform::from_translation(Vec3::ZERO))
        .unwrap();
    let heights = vec![m(0.0); 25]; // flat 5×5 grid at local y = 0
    px.attach_heightfield_collider(ground, 5, 5, m(1.0), m(1.0), &heights, mat, false)
        .unwrap();

    let ball = px
        .create_dynamic_body(
            Transform::from_translation(Vec3::new(0.0, 3.0, 0.0)),
            r(1.0),
        )
        .unwrap();
    px.attach_sphere_collider(ball, m(0.5), mat, false).unwrap();

    (0..240).for_each(|n| px.step(step(n)).unwrap());
    let y = ball_y(&px, ball);
    // Rests near its radius (0.5) above the surface — above the field, not through it.
    assert!(
        y > 0.25 && y < 1.0,
        "ball rests on the flat heightfield, y = {y}"
    );
}

#[test]
fn a_sphere_slides_downhill_on_a_tilted_heightfield() {
    let mut px = world();
    let mat = PhysicsApi::material(r(0.2), r(0.0), r(1.0)).unwrap();
    let ground = px
        .create_static_body(Transform::from_translation(Vec3::ZERO))
        .unwrap();
    // A 5×5 grid tilted so height falls toward +x (a gentle ramp): h = -0.25·x.
    let heights: Vec<Meters> = (0..25)
        .map(|k| {
            let ix = (k % 5) as f32 - 2.0; // local x index centred
            m(-0.25 * ix)
        })
        .collect();
    px.attach_heightfield_collider(ground, 5, 5, m(1.0), m(1.0), &heights, mat, false)
        .unwrap();

    let ball = px
        .create_dynamic_body(
            Transform::from_translation(Vec3::new(0.0, 1.5, 0.0)),
            r(1.0),
        )
        .unwrap();
    px.attach_sphere_collider(ball, m(0.4), mat, false).unwrap();

    let (x0, _) = ball_xz(&px, ball);
    (0..180).for_each(|n| px.step(step(n)).unwrap());
    let (x1, _) = ball_xz(&px, ball);
    // Gravity + the tilted contact normal push the ball down the +x slope.
    assert!(x1 > x0 + 0.3, "ball slides downhill (+x): {x0} -> {x1}");
    assert!(
        ball_y(&px, ball) > -1.0,
        "ball stays on the ramp, not through it"
    );
}

#[test]
fn heightfield_attach_validates_grid_dimensions() {
    let mut px = world();
    let mat = PhysicsApi::material(r(0.5), r(0.0), r(1.0)).unwrap();
    let ground = px
        .create_static_body(Transform::from_translation(Vec3::ZERO))
        .unwrap();
    // nx < 2 is rejected.
    assert!(px
        .attach_heightfield_collider(ground, 1, 3, m(1.0), m(1.0), &vec![m(0.0); 3], mat, false)
        .is_err());
    // A heights length that is not nx·nz is rejected.
    assert!(px
        .attach_heightfield_collider(ground, 3, 3, m(1.0), m(1.0), &vec![m(0.0); 4], mat, false)
        .is_err());
    // A well-formed grid attaches.
    assert!(px
        .attach_heightfield_collider(ground, 3, 3, m(1.0), m(1.0), &vec![m(0.0); 9], mat, false)
        .is_ok());
}

#[test]
fn heightfield_attach_rejects_a_missing_body_and_over_capacity() {
    let mat = PhysicsApi::material(r(0.5), r(0.0), r(1.0)).unwrap();
    // A collider on a body that does not exist is rejected.
    let mut px = world();
    let bogus = PhysicsBodyHandle::from_raw(9999);
    assert!(px
        .attach_heightfield_collider(bogus, 3, 3, m(1.0), m(1.0), &vec![m(0.0); 9], mat, false)
        .is_err());
    // At collider capacity (max_colliders = 1), a second collider is rejected.
    let mut full =
        PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 4, 1, 1, true, r(0.0), r(0.0))
            .unwrap();
    let ground = full
        .create_static_body(Transform::from_translation(Vec3::ZERO))
        .unwrap();
    assert!(full
        .attach_sphere_collider(ground, m(0.5), mat, false)
        .is_ok());
    assert!(
        full.attach_heightfield_collider(
            ground,
            3,
            3,
            m(1.0),
            m(1.0),
            &vec![m(0.0); 9],
            mat,
            false
        )
        .is_err(),
        "a second collider exceeds max_colliders = 1"
    );
}
