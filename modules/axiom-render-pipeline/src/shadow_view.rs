//! The directional shadow caster's camera.
//!
//! Deriving *where the shadow map looks from* is a distinct concern from
//! orchestrating a frame, and it is the one piece of the pipeline whose
//! correctness is purely geometric: given the sun's travel direction and the
//! world point the shadow volume should cover, produce the light's
//! view-projection, or `None` when the direction is degenerate. Keeping it here
//! rather than inline in the facade gives that geometry its own home and its own
//! test, and keeps the facade file about composition.
//!
//! ## The shadow volume follows the view
//!
//! [`SHADOW_EXTENT`] is a fixed half-extent, but it is **not** anchored at the
//! world origin. The volume is centred on a *focus point*, and
//! [`shadow_focus`] derives that point from the frame's own camera: the camera's
//! position pushed [`SHADOW_FOCUS_AHEAD`] along its forward axis, so the box
//! straddles what the viewer is actually looking at.
//!
//! That is the fix for a defect this file used to only document: with the box
//! pinned to `Vec3::ZERO`, any scene whose action happened far from the origin
//! left the shadow frustum entirely and rendered unshadowed (the main pass
//! treats fragments outside the shadow frustum as lit), so a 9 km course was
//! shadowed for its first ~20 m and flat for the remaining 8.98 km. The shadow
//! volume is a property of the *view*, not of the world origin, so it is derived
//! from the camera the pipeline already resolves — no new frame-contract field,
//! and every app is fixed at once rather than each app remembering to aim a
//! focus knob.

use axiom_math::{Mat4, Transform, Vec3};

use crate::render_pipeline_api::GL_TO_WGPU_DEPTH;

/// Orthographic half-extent (world units) the shadow map covers around its focus.
const SHADOW_EXTENT: f32 = 20.0;
/// How far along the camera's forward axis the focus sits, as a fraction of the
/// box. Half an extent puts the near face one half-extent *behind* the camera
/// and the far face one and a half ahead — cast shadows are something you look
/// at, so the volume is spent forward rather than split evenly around the eye.
const SHADOW_FOCUS_AHEAD: f32 = SHADOW_EXTENT * 0.5;
/// Distance up-sun the shadow camera sits.
const SHADOW_DISTANCE: f32 = 40.0;
const NEAR: f32 = 0.1;
const FAR: f32 = 100.0;

/// The world point the shadow volume centres on, for a frame whose camera node
/// has the world transform `camera_world`: the camera's position pushed
/// [`SHADOW_FOCUS_AHEAD`] down its own forward axis (`-Z`, the convention
/// `Mat4::look_at` builds and the perspective projection assumes).
pub(crate) fn shadow_focus(camera_world: Transform) -> Vec3 {
    camera_world.transform_point(Vec3::new(0.0, 0.0, -SHADOW_FOCUS_AHEAD))
}

/// Build the directional shadow caster's light view-projection from the sun's
/// world travel `direction` and the world `focus` the volume covers (see
/// [`shadow_focus`]): an orthographic box of half-size [`SHADOW_EXTENT`] looking
/// from up-sun back at `focus`, depth-corrected to wgpu's `[0,1]` clip depth
/// (the same `GL_TO_WGPU_DEPTH` fix the camera uses). `None` for a degenerate
/// (zero) direction — the caller substitutes identity, disabling shadows.
/// Branchless: the up vector is a table pick and the fallible matrix steps are
/// `Option` combinators.
pub(crate) fn shadow_light_view_proj(direction: Vec3, focus: Vec3) -> Option<Mat4> {
    let len =
        (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
    let n = Vec3::new(direction.x / len, direction.y / len, direction.z / len);
    // A zero direction makes `n` non-finite, which carries into `eye` and makes
    // the look-at forward un-normalizable — the degenerate arm below, preserved
    // for any focus.
    let eye = focus.subtract(n.mul_scalar(SHADOW_DISTANCE));
    // A near-vertical sun would make the default up parallel to the view; pick a
    // sideways up in that case (table index, no branch).
    let up = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)][(n.y.abs() > 0.99) as usize];
    let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
    Mat4::look_at(eye, focus, up).ok().and_then(|view| {
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
        let tilted = shadow_light_view_proj(Vec3::new(0.3, -1.0, 0.4), Vec3::ZERO).unwrap();
        assert_ne!(tilted, Mat4::IDENTITY);
        // A near-vertical sun (|n.y| > 0.99) takes the sideways-up table arm and
        // still yields a valid matrix (look-at forward is not parallel to up).
        let vertical = shadow_light_view_proj(Vec3::new(0.0, -1.0, 0.0), Vec3::ZERO).unwrap();
        assert_ne!(vertical, Mat4::IDENTITY);
        // A zero direction is degenerate (look-at eye == target) → None, so the
        // caller falls back to identity and shadows become a no-op — and that
        // holds at a focus far from the origin too, not just at `ZERO`.
        assert!(shadow_light_view_proj(Vec3::ZERO, Vec3::ZERO).is_none());
        assert!(shadow_light_view_proj(Vec3::ZERO, Vec3::new(0.0, 0.0, -1900.0)).is_none());
    }

    #[test]
    fn shadow_focus_sits_ahead_of_the_camera_along_its_forward_axis() {
        // An untilted camera 3 m up and 10 m back along +Z looks down -Z, so the
        // focus is SHADOW_FOCUS_AHEAD nearer the origin at the same height.
        let camera = Transform::from_translation(Vec3::new(0.0, 3.0, 10.0));
        let focus = shadow_focus(camera);
        assert_eq!(focus, Vec3::new(0.0, 3.0, 10.0 - SHADOW_FOCUS_AHEAD));
    }

    #[test]
    fn the_shadow_volume_travels_with_the_focus_instead_of_pinning_to_the_origin() {
        // The regression this file exists to hold: a moment ~1.9 km down a course
        // must be *inside* the shadow box. Light-clip coordinates of the focus
        // point itself: centred in x/y and within the [0,1] depth range.
        let sun = Vec3::new(0.3, -1.0, 0.4);
        let far_focus = Vec3::new(0.0, 0.0, -1900.0);
        let far = shadow_light_view_proj(sun, far_focus).unwrap();
        let centre = far.transform_point(far_focus);
        assert!(centre.x.abs() < 1.0e-3, "focus is centred in light x");
        assert!(centre.y.abs() < 1.0e-3, "focus is centred in light y");
        let depth_in_range = (0.0..=1.0).contains(&centre.z);
        assert!(depth_in_range, "focus depth is inside wgpu's [0,1] clip");

        // The old origin-anchored box misses that same point by orders of
        // magnitude in light space — it is nowhere near the map.
        let at_origin = shadow_light_view_proj(sun, Vec3::ZERO).unwrap();
        let missed = at_origin.transform_point(far_focus);
        let lateral = missed.x.abs().max(missed.y.abs());
        assert!(lateral > 1.0, "origin-anchored box misses the far focus");
        assert_ne!(far, at_origin);
    }
}
