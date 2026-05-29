//! Helpers used by the cross-module translation in `cube_demo.rs`.
//!
//! Modules cannot import other modules, so the **app** is the one
//! place where data flows across module boundaries. The full
//! pipeline (`SceneSnapshot + ResolvedResources → RenderInput`,
//! `RenderCommandList → GpuSubmission`) lives in `cube_demo::run_tick`
//! because most of the data types involved are not nameable outside
//! their owning modules. This file holds the small helpers whose
//! signatures only use nameable primitives.

use axiom_math::{Mat4, Transform, Vec3, Vec4};

/// The clear colour used for the demo background.
pub const DEMO_CLEAR_COLOR: [f32; 4] = [0.05, 0.06, 0.08, 1.0];

/// The demo's basic-lit cube base colour.
pub const DEMO_CUBE_BASE_COLOR: Vec4 = Vec4::new(0.8, 0.4, 0.2, 1.0);

/// The demo's directional-light world direction.
pub const DEMO_LIGHT_DIRECTION_WORLD: Vec3 = Vec3::new(0.3, -1.0, 0.4);

/// The demo's directional-light colour.
pub const DEMO_LIGHT_COLOR: Vec3 = Vec3::new(1.0, 1.0, 1.0);

/// The demo's directional-light intensity.
pub const DEMO_LIGHT_INTENSITY: f32 = 1.0;

/// Rotate the cube parent node deterministically: one full revolution
/// every 360 ticks, around +Y.
pub fn cube_rotation_for_tick(tick: u64) -> f32 {
    let degrees_per_tick = 1.0_f32;
    ((tick % 360) as f32) * degrees_per_tick * std::f32::consts::PI / 180.0
}

/// Compute the view matrix from a camera node's world transform.
///
/// `view = inverse(world)`; works because the demo's camera node has
/// identity scale, which is the only case where `Transform::inverse`
/// succeeds.
pub fn view_matrix_from_world(camera_world: Transform) -> Mat4 {
    camera_world
        .inverse()
        .expect("demo camera node always has identity scale, so inverse succeeds")
        .to_matrix()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_is_zero_at_tick_zero() {
        assert_eq!(cube_rotation_for_tick(0), 0.0);
    }

    #[test]
    fn rotation_is_deterministic() {
        assert_eq!(cube_rotation_for_tick(60), cube_rotation_for_tick(60));
    }

    #[test]
    fn rotation_grows_with_tick_in_first_cycle() {
        let a = cube_rotation_for_tick(10);
        let b = cube_rotation_for_tick(20);
        assert!(b > a);
    }

    #[test]
    fn rotation_wraps_at_360() {
        // tick 0 and tick 360 produce the same angle.
        assert_eq!(cube_rotation_for_tick(0), cube_rotation_for_tick(360));
    }

    #[test]
    fn view_matrix_is_inverse_world() {
        let world = Transform::from_translation(Vec3::new(0.0, 0.0, 5.0));
        let view = view_matrix_from_world(world);
        // Applying view to the camera position should send it to the origin.
        let camera_pos = world.translation;
        let v = view.transform_point(camera_pos);
        assert!(v.x.abs() < 1.0e-5);
        assert!(v.y.abs() < 1.0e-5);
        assert!(v.z.abs() < 1.0e-5);
    }

    #[test]
    fn constants_have_expected_shape() {
        assert_eq!(DEMO_CLEAR_COLOR[3], 1.0);
        assert_eq!(DEMO_CUBE_BASE_COLOR.w, 1.0);
        assert_eq!(DEMO_LIGHT_INTENSITY, 1.0);
    }
}
