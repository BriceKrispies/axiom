//! The directional shadow caster's camera.
//!
//! Deriving *where the shadow map looks from* is a distinct concern from
//! orchestrating a frame, and it is the one piece of the pipeline whose
//! correctness is purely geometric: given the sun's travel direction, produce
//! the light's view-projection, or `None` when the direction is degenerate.
//! Keeping it here rather than inline in the facade gives that geometry its own
//! home and its own test, and keeps the facade file about composition.
//!
//! ## A known limitation, stated where it lives
//!
//! [`SHADOW_EXTENT`] is a fixed half-extent around the **world origin** — the
//! shadow camera always looks at `Vec3::ZERO`. Any scene whose action happens
//! far from the origin therefore leaves the shadow box entirely and renders
//! unshadowed, no matter how the key light is aimed. A 9 km course starting at
//! the origin is shadowed for its first ~20 m and unshadowed for the rest.
//!
//! Fixing that means letting the caller supply the volume the shadow should
//! cover (a focus point, or a fitted view frustum), which is a change to the
//! frame contract rather than to this function — recorded here so the next
//! reader finds the limitation attached to the code that has it, instead of
//! rediscovering it from a render.

use axiom_math::{Mat4, Vec3};

use crate::render_pipeline_api::GL_TO_WGPU_DEPTH;

/// Orthographic half-extent (world units) the shadow map covers around origin.
const SHADOW_EXTENT: f32 = 20.0;
/// Distance up-sun the shadow camera sits.
const SHADOW_DISTANCE: f32 = 40.0;
const NEAR: f32 = 0.1;
const FAR: f32 = 100.0;

/// Build the directional shadow caster's light view-projection from the sun's
/// world travel `direction`: an orthographic box of half-size [`SHADOW_EXTENT`]
/// looking from up-sun back at the origin, depth-corrected to wgpu's `[0,1]`
/// clip depth (the same `GL_TO_WGPU_DEPTH` fix the camera uses). `None` for a
/// degenerate (zero) direction — the caller substitutes identity, disabling
/// shadows. Branchless: the up vector is a table pick and the fallible matrix
/// steps are `Option` combinators.
pub(crate) fn shadow_light_view_proj(direction: Vec3) -> Option<Mat4> {
    let len =
        (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
    let n = Vec3::new(direction.x / len, direction.y / len, direction.z / len);
    let eye = Vec3::new(
        -n.x * SHADOW_DISTANCE,
        -n.y * SHADOW_DISTANCE,
        -n.z * SHADOW_DISTANCE,
    );
    // A near-vertical sun would make the default up parallel to the view; pick a
    // sideways up in that case (table index, no branch).
    let up = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)][(n.y.abs() > 0.99) as usize];
    let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
    Mat4::look_at(eye, Vec3::ZERO, up).ok().and_then(|view| {
        Mat4::orthographic(
            -SHADOW_EXTENT,
            SHADOW_EXTENT,
            -SHADOW_EXTENT,
            SHADOW_EXTENT,
            NEAR,
            FAR,
        )
        .ok()
        .map(|proj| depth_fix.multiply(proj).multiply(view))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_light_view_proj_covers_tilted_vertical_and_degenerate_suns() {
        let tilted = shadow_light_view_proj(Vec3::new(0.3, -1.0, 0.4)).unwrap();
        assert_ne!(tilted, Mat4::IDENTITY);
        // A near-vertical sun (|n.y| > 0.99) takes the sideways-up table arm and
        // still yields a valid matrix (look-at forward is not parallel to up).
        let vertical = shadow_light_view_proj(Vec3::new(0.0, -1.0, 0.0)).unwrap();
        assert_ne!(vertical, Mat4::IDENTITY);
        // A zero direction is degenerate (look-at eye == target) → None, so the
        // caller falls back to identity and shadows become a no-op.
        assert!(shadow_light_view_proj(Vec3::ZERO).is_none());
    }
}
