//! The directional shadow caster's camera.
//!
//! Deriving *where the shadow map looks from* is a distinct concern from
//! orchestrating a frame, and it is the one piece of the pipeline whose
//! correctness is purely geometric: given the sun's travel direction and the
//! frame's camera, produce the light's view-projection, or `None` when the
//! direction is degenerate. Keeping it here rather than inline in the facade
//! gives that geometry its own home and its own test, and keeps the facade file
//! about composition.
//!
//! ## The shadow volume is fitted to the view frustum
//!
//! The volume is the **bounding sphere of the camera's own frustum**, sliced from
//! its near plane to [`SHADOW_RANGE`] ([`shadow_volume`]), and the ortho box is
//! fitted to that sphere ([`shadow_light_view_proj`]). A sphere rather than the
//! eight corners because a sphere is rotation-invariant: the box's size does not
//! change as the camera turns, so a cast shadow does not shimmer while you steer.
//!
//! That is the second of two defects this file has now fixed, and they are the
//! same defect twice:
//!
//! 1. The volume used to be pinned to `Vec3::ZERO`, so any scene whose action
//!    happened far from the origin left the shadow frustum entirely and rendered
//!    unshadowed (the main pass treats fragments outside the shadow frustum as
//!    lit) — a 9 km course was shadowed for its first ~20 m and flat for the
//!    remaining 8.98 km. That was fixed by centring the box on the *camera*.
//! 2. The volume was then still a **fixed 40 m cube** — a constant the engine
//!    assumed about every app's world scale and every camera's field of view. A
//!    portrait phone camera (`aspect 0.56`) and a 21:9 desktop camera at the same
//!    vertical fov see frusta that differ by **1.6x** in bounding radius, and
//!    both were handed the same box: the wide one starved (its shadows cut off
//!    mid-screen), the tall one wasting half its map on width it cannot see. And
//!    at any speed the 40 m cube is a puddle: a chase camera showing 800 m of
//!    road got cast shadows for the ~35 m around the car and a flat, unshadowed
//!    slab for everything ahead of it — which reads as "this scene has one
//!    shadow, under the hero" rather than as a sunlit world.
//!
//! Both are the same mistake — a shadow volume is a property of the **view**,
//! never of a constant — so the fix is the same shape: derive it from the camera
//! the pipeline already resolves. No new frame-contract field, no per-app focus
//! knob to forget, and every app is fixed at once.
//!
//! ## Two real trades: coverage against texel density, and ASPECT
//!
//! [`SHADOW_RANGE`] is the only number left, and it is a genuine trade, so the
//! arithmetic is written down rather than tuned by feel. The shadow atlas edge is
//! the device tier's (`HostDeviceProfile::shadow_map_size`: 1024 baseline, 2048
//! extended) and the main pass filters it with a 5x5 PCF at a 1.25-texel spread,
//! so the world-space penumbra is `6.25 * 2r / atlas`.
//!
//! The radius, though, is as much a function of the **aspect** as of the range,
//! and that second dependency is what this table used to hide. Every figure it
//! published was computed at `aspect 0.563` — a portrait phone racer — and read
//! as though it were the engine's penumbra. A landscape frame at the same
//! vertical fov sees a frustum whose bounding sphere is **1.78x** larger, so
//! every published number was 1.78x optimistic for every landscape app, and
//! nothing noticed, because the number was a comment rather than an assertion.
//!
//! It is now an assertion. The table below carries both aspects, it carries only
//! the range actually shipped, and every cell in it is recomputed from
//! [`shadow_volume`] by `the_published_penumbra_table_is_what_the_fit_actually_produces`
//! — a row a test cannot reach is a row this file will not publish.
//!
//! 80° vertical fov, near 1.2, `SHADOW_RANGE` = 60 m:
//!
//! | aspect | radius | texel @1024 | penumbra @1024 | @2048 |
//! |--------|--------|-------------|----------------|-------|
//! | 0.563 (portrait phone) | 57.8 m | 11.3 cm | 71 cm | 35 cm |
//! | 1.778 (16:9 landscape) | 102.7 m | 20.1 cm | 125 cm | 63 cm |
//!
//! 60 m buys ~3.3x the shadowed road that defect 2's 20 m box did, for a
//! *portrait* penumbra still well under the width of the objects casting into it
//! (a car is 4.5 m, a palm crown 6 m), which is the line: a soft edge is a
//! sunlit-day shadow, a smeared one is a stain. Moving [`SHADOW_RANGE`] moves
//! every column of both rows — that is the whole decision.
//!
//! **At 16:9 that line is crossed, and no choice of range uncrosses it.** A
//! 1.25 m penumbra on the baseline atlas is wider than an awning is deep, so a
//! landscape app's contact seams smear below the noise floor and the frame reads
//! as unshadowed — while shortening the range far enough to sharpen them would
//! cut the shadowed distance back to a puddle, which is defect 2 again. One
//! cascade cannot serve the metre scale and the hundred-metre scale at once.
//! That is exactly what cascaded shadow maps exist to fix, and the fit for them
//! is already ported and tested in `axiom-gpu-backend`'s `cascade` module —
//! bound by nothing, because the frame contract carries exactly one
//! `light_view_proj`. See `docs/work-manifests/shmup-port/notes/csm.md`. Until
//! that lands, a landscape app runs on the coarse row of this table, and the
//! table now says so out loud instead of quoting a phone's numbers at it.

use axiom_math::{Mat4, Transform, Vec3};

use crate::render_pipeline_api::GL_TO_WGPU_DEPTH;

/// How far down the view axis the single shadow cascade reaches (world units).
/// See the module docs' coverage/penumbra table for what moving it costs.
const SHADOW_RANGE: f32 = 60.0;
/// Slack (world units) between the shadow camera and the near face of its volume.
/// The eye sits `radius + this` up-sun of the volume's centre, so a caster up to
/// this far *above* the fitted sphere — a palm crown standing up out of a volume
/// fitted to the road it shades — is still inside the map instead of clipped out
/// of it and dropping its shadow.
const SHADOW_DEPTH_MARGIN: f32 = 20.0;
/// The shadow camera's near plane. The far plane is derived from the volume.
const NEAR: f32 = 0.1;

/// The world-space sphere a frame's cast shadows are rendered over: the bounding
/// sphere of the camera frustum's `[near, SHADOW_RANGE]` slice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShadowVolume {
    centre: Vec3,
    radius: f32,
}

/// The volume a frame with **no camera** falls back to. There is no view to fit
/// to, so the sphere sits at the origin at half the range — the same "nothing to
/// follow, stay put" default the focus point used to take, now carrying a radius
/// with it.
pub(crate) const ORIGIN_VOLUME: ShadowVolume = ShadowVolume {
    centre: Vec3::ZERO,
    radius: SHADOW_RANGE * 0.5,
};

/// Fit the shadow volume to a frame's camera: the bounding sphere of the
/// perspective frustum's `[near, SHADOW_RANGE]` slice, in world space.
///
/// The sphere's centre sits on the view axis (`-Z` local, the convention
/// `Mat4::look_at` builds and the perspective projection assumes) at the depth
/// where the near cap and the far cap are equidistant — the closed form
/// `((f + n) / 2) * (1 + tan_h^2 + tan_w^2)` — pulled back into the slice for the
/// wide-fov case where that point already lies beyond the far cap and the far
/// cap's own circumcircle is the whole answer. The radius is then the larger of
/// the two cap distances. Branchless: `min`/`max` on the two candidates, no
/// control flow, and `min`/`max` rather than `clamp` so a pathological
/// `near > SHADOW_RANGE` camera degrades instead of panicking.
pub(crate) fn shadow_volume(
    camera_world: Transform,
    fovy_radians: f32,
    aspect: f32,
    near: f32,
) -> ShadowVolume {
    let tan_h = (fovy_radians * 0.5).tan();
    let tan_w = tan_h * aspect;
    let spread = tan_h * tan_h + tan_w * tan_w;
    let far = SHADOW_RANGE;
    let centre_z = (0.5 * (far + near) * (1.0 + spread)).min(far).max(near);
    let to_near = centre_z - near;
    let to_far = centre_z - far;
    let radius = (near * near * spread + to_near * to_near)
        .max(far * far * spread + to_far * to_far)
        .sqrt();
    ShadowVolume {
        centre: camera_world.transform_point(Vec3::new(0.0, 0.0, -centre_z)),
        radius,
    }
}

/// Build the directional shadow caster's light view-projection from the sun's
/// world travel `direction` and the `volume` the map covers (see
/// [`shadow_volume`]): an orthographic box circumscribing the volume's sphere,
/// looking from up-sun back at its centre, depth-corrected to wgpu's `[0,1]` clip
/// depth (the same `GL_TO_WGPU_DEPTH` fix the camera uses). `None` for a
/// degenerate (zero) direction — the caller substitutes identity, disabling
/// shadows.
///
/// The depth range is derived from the sphere, not fixed: the eye sits
/// `radius + SHADOW_DEPTH_MARGIN` up-sun, so the sphere spans
/// `[margin, 2*radius + margin]` in light depth and the far plane is exactly the
/// far side of it. A fixed depth range would clip the volume's own back half the
/// moment the volume grew, which is how "the shadow box got bigger and the
/// shadows disappeared" happens.
///
/// Branchless: the up vector is a table pick and the fallible matrix steps are
/// `Option` combinators.
pub(crate) fn shadow_light_view_proj(direction: Vec3, volume: ShadowVolume) -> Option<Mat4> {
    let len =
        (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
    let n = Vec3::new(direction.x / len, direction.y / len, direction.z / len);
    let radius = volume.radius;
    let distance = radius + SHADOW_DEPTH_MARGIN;
    // A zero direction makes `n` non-finite, which carries into `eye` and makes
    // the look-at forward un-normalizable — the degenerate arm below, preserved
    // for any volume.
    let eye = volume.centre.subtract(n.mul_scalar(distance));
    // A near-vertical sun would make the default up parallel to the view; pick a
    // sideways up in that case (table index, no branch).
    let up = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)][(n.y.abs() > 0.99) as usize];
    let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
    Mat4::look_at(eye, volume.centre, up).ok().and_then(|view| {
        Mat4::orthographic(
            -radius,
            radius,
            -radius,
            radius,
            NEAR,
            distance + radius,
        )
        .ok()
        .map(|proj| depth_fix.multiply(proj).multiply(view))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera 3 m up looking down -Z, the shape every chase camera has.
    fn chase_camera() -> Transform {
        Transform::from_translation(Vec3::new(0.0, 3.0, 10.0))
    }

    /// 80° vertical fov on a portrait phone frame — burnt-rubber's shape.
    fn portrait() -> ShadowVolume {
        shadow_volume(chase_camera(), 80_f32.to_radians(), 0.563, 1.2)
    }

    #[test]
    fn the_volume_is_fitted_ahead_of_the_camera_along_its_forward_axis() {
        let v = portrait();
        // On the view axis (the camera has no lateral offset and no rotation),
        // at the camera's height, and *ahead* of it down -Z.
        assert!(v.centre.x.abs() < 1.0e-3, "on the view axis");
        assert!((v.centre.y - 3.0).abs() < 1.0e-3, "at the camera's height");
        assert!(v.centre.z < 10.0, "the volume sits ahead of the eye");
        // The whole slice is inside the sphere: the far cap's centre is the
        // deepest point on the axis the volume must reach.
        let far_cap_z = 10.0 - SHADOW_RANGE;
        assert!(
            (v.centre.z - far_cap_z).abs() <= v.radius + 1.0e-3,
            "the far cap {far_cap_z} is inside a sphere at {} r{}",
            v.centre.z,
            v.radius
        );
        // Debug + PartialEq are part of this type's surface.
        assert!(format!("{v:?}").contains("ShadowVolume"));
        assert_eq!(v, portrait());
        assert_ne!(v, ORIGIN_VOLUME);
    }

    /// The defect the frustum fit exists to remove: the old box was one constant
    /// for every camera. A wide frame genuinely needs a bigger volume than a tall
    /// one at the same vertical fov, and now gets one.
    #[test]
    fn a_wide_camera_gets_a_bigger_volume_than_a_tall_one() {
        let fovy = 80_f32.to_radians();
        let tall = shadow_volume(chase_camera(), fovy, 0.563, 1.2).radius;
        let wide = shadow_volume(chase_camera(), fovy, 2.33, 1.2).radius;
        assert!(
            wide > tall * 1.3,
            "a 21:9 frustum ({wide}) must outgrow a portrait one ({tall})"
        );
        // ...and both stay inside the range they were sliced to, plus the slack a
        // bounding sphere of a cone necessarily carries (a very wide frustum's
        // far cap is itself wider than the slice is long).
        assert!(tall < SHADOW_RANGE, "tall radius {tall} is bounded");
        assert!(wide < SHADOW_RANGE * 2.5, "wide radius {wide} is bounded");
    }

    /// A camera whose near plane is beyond the cascade range is nonsense, but it
    /// must degrade rather than panic (which `clamp` would) or produce a
    /// zero-sized box (which `Mat4::orthographic` rejects).
    #[test]
    fn a_near_plane_past_the_range_still_yields_a_usable_volume() {
        let v = shadow_volume(chase_camera(), 80_f32.to_radians(), 1.0, SHADOW_RANGE + 40.0);
        assert!(v.radius > 0.0, "radius {} is usable", v.radius);
        assert!(v.radius.is_finite());
        assert!(shadow_light_view_proj(Vec3::new(0.3, -1.0, 0.4), v).is_some());
    }

    #[test]
    fn shadow_light_view_proj_covers_tilted_vertical_and_degenerate_suns() {
        let v = portrait();
        let tilted = shadow_light_view_proj(Vec3::new(0.3, -1.0, 0.4), v).unwrap();
        assert_ne!(tilted, Mat4::IDENTITY);
        // A near-vertical sun (|n.y| > 0.99) takes the sideways-up table arm and
        // still yields a valid matrix (look-at forward is not parallel to up).
        let vertical = shadow_light_view_proj(Vec3::new(0.0, -1.0, 0.0), v).unwrap();
        assert_ne!(vertical, Mat4::IDENTITY);
        // A zero direction is degenerate (look-at eye == target) → None, so the
        // caller falls back to identity and shadows become a no-op — and that
        // holds for the camera-less fallback volume too, not just a fitted one.
        assert!(shadow_light_view_proj(Vec3::ZERO, v).is_none());
        assert!(shadow_light_view_proj(Vec3::ZERO, ORIGIN_VOLUME).is_none());
    }

    /// **The published penumbra table, asserted instead of asserted-in-prose.**
    ///
    /// The module docs' table used to carry one radius column, computed at
    /// `aspect 0.563`, read as though it were the engine's penumbra. It was not:
    /// the fitted radius scales with the frustum's *width* as much as with its
    /// depth, so at 16:9 every cell is 1.78x larger, and every landscape app has
    /// been running on numbers tuned for a portrait phone. Nothing caught it
    /// because the number was a comment. This is the assertion — every cell of
    /// the table recomputed from [`shadow_volume`] itself, at both aspects and
    /// both device-tier atlas edges.
    #[test]
    fn the_published_penumbra_table_is_what_the_fit_actually_produces() {
        let fovy = 80_f32.to_radians();
        let radius = |aspect: f32| shadow_volume(chase_camera(), fovy, aspect, 1.2).radius;
        let portrait_r = radius(0.563);
        let wide_r = radius(16.0 / 9.0);
        assert!(
            (portrait_r - 57.79).abs() < 0.02,
            "the table publishes a 57.8 m portrait radius; the fit produces {portrait_r}"
        );
        assert!(
            (wide_r - 102.69).abs() < 0.02,
            "the table publishes a 102.7 m 16:9 radius; the fit produces {wide_r}"
        );
        // The finding itself: the same camera, the same range, 1.78x the sphere.
        let widening = wide_r / portrait_r;
        assert!(
            (widening - 1.777).abs() < 0.005,
            "a 16:9 frustum's bounding sphere is {widening}x a portrait one's, \
             not the 1.78x the table publishes"
        );
        // The texel and penumbra cells. `6.25 * 2r / atlas` is the main pass's
        // 5x5 PCF at a 1.25-texel spread (`scene_wgsl.rs`'s `shadow_factor`).
        let texel = |r: f32, atlas: f32| 2.0 * r / atlas;
        [(portrait_r, 0.113_f32), (wide_r, 0.201)]
            .into_iter()
            .for_each(|(r, published)| {
                let t = texel(r, 1024.0);
                assert!(
                    (t - published).abs() < 0.001,
                    "the 1024 texel at r {r} is {t} m; the table says {published} m"
                );
            });
        [
            (portrait_r, 1024.0_f32, 0.705_f32),
            (portrait_r, 2048.0, 0.353),
            (wide_r, 1024.0, 1.254),
            (wide_r, 2048.0, 0.627),
        ]
        .into_iter()
        .for_each(|(r, atlas, published)| {
            let p = 6.25 * texel(r, atlas);
            assert!(
                (p - published).abs() < 0.005,
                "the penumbra at r {r} / atlas {atlas} is {p} m; the table says {published} m"
            );
        });
        // And the line the docs draw, kept mechanical: a portrait frame's
        // penumbra stays under the width of what casts into it; a landscape
        // frame's is wider than a 1 m awning is deep, which is exactly why a
        // landscape app on one cascade reads as unshadowed.
        assert!(
            6.25 * texel(portrait_r, 1024.0) < 1.0,
            "the portrait row must stay sub-metre"
        );
        assert!(
            6.25 * texel(wide_r, 1024.0) > 1.0,
            "the landscape degradation is real; this stays asserted until \
             cascades remove it"
        );
    }

    /// **The property `axiom-gpu-backend`'s shadow depth bias is derived from.**
    ///
    /// The main pass's bias used to be a constant in *normalized device depth*
    /// (`0.0015`), so the world offset it bought was `0.0015 * (far - near)` of
    /// whatever box this file fitted — a quantity that has nothing to do with
    /// what governs shadow acne, and that does not move at all when the device
    /// tier doubles the atlas. `scene_wgsl.rs` now states the bias in shadow
    /// *texels* and converts it into NDC using the fitted volume's own extent
    /// and depth range, and it recovers both out of this matrix rather than
    /// through a new uniform, because it can:
    ///
    /// - [`shadow_light_view_proj`] returns `depth_fix * orthographic * look_at`
    ///   and `look_at` is rigid, so each row's directional part is that row's
    ///   ortho scale times a unit rotation row;
    /// - row 0's magnitude is therefore `1 / radius` (the ortho half-extent);
    /// - row 2's magnitude is `1 / (far - near)` — `depth_fix`'s `0.5`
    ///   cancelling the orthographic's `2 / (far - near)`.
    ///
    /// That is a contract between this file and the shader, so it is asserted
    /// here, where the matrix is built, and named in the WGSL comment that
    /// relies on it.
    #[test]
    fn the_light_view_proj_rows_recover_the_fitted_extent_and_depth_range() {
        let v = portrait();
        let m = shadow_light_view_proj(Vec3::new(0.3, -1.0, 0.4), v)
            .unwrap()
            .as_cols_array();
        // Column-major: row `r`'s directional part is m[r], m[4 + r], m[8 + r].
        let row = |r: usize| (m[r] * m[r] + m[4 + r] * m[4 + r] + m[8 + r] * m[8 + r]).sqrt();
        let recovered_radius = 1.0 / row(0);
        let recovered_range = 1.0 / row(2);
        assert!(
            (recovered_radius - v.radius).abs() < 1.0e-3,
            "row 0 recovers a half-extent of {recovered_radius}; the fit's is {}",
            v.radius
        );
        let ortho_span = v.radius + SHADOW_DEPTH_MARGIN + v.radius - NEAR;
        assert!(
            (recovered_range - ortho_span).abs() < 1.0e-2,
            "row 2 recovers a depth range of {recovered_range}; the ortho spans {ortho_span}"
        );
        // The defect, measured. The retired `0.0015` NDC constant meant this
        // many texels — and the SAME constant meant twice as many on the
        // extended tier's 2048 atlas, because an NDC constant cannot see the
        // atlas at all. The derived bias is 1.75 texels on both, which is why
        // 1.75 is neither a retune of the baseline tier nor a regression of it.
        let old_world_bias = 0.0015 * recovered_range;
        let in_texels = |atlas: f32| old_world_bias / (2.0 * v.radius / atlas);
        assert!(
            (in_texels(1024.0) - 1.80).abs() < 0.01,
            "the shipped constant was {} texels at 1024",
            in_texels(1024.0)
        );
        assert!(
            (in_texels(2048.0) - 3.60).abs() < 0.02,
            "the shipped constant was {} texels at 2048 — the same NDC number, \
             twice the peter-panning in the unit that matters",
            in_texels(2048.0)
        );
    }

    #[test]
    fn the_shadow_volume_travels_with_the_camera_instead_of_pinning_to_the_origin() {
        // The regression the first fix exists to hold: a moment ~1.9 km down a
        // course must be *inside* the shadow box. Light-clip coordinates of the
        // volume's own centre: centred in x/y and within the [0,1] depth range.
        let sun = Vec3::new(0.3, -1.0, 0.4);
        let far_camera = Transform::from_translation(Vec3::new(0.0, 3.0, -1900.0));
        let v = shadow_volume(far_camera, 80_f32.to_radians(), 0.563, 1.2);
        let far = shadow_light_view_proj(sun, v).unwrap();
        let centre = far.transform_point(v.centre);
        assert!(centre.x.abs() < 1.0e-3, "centre is centred in light x");
        assert!(centre.y.abs() < 1.0e-3, "centre is centred in light y");
        assert!(
            (0.0..=1.0).contains(&centre.z),
            "centre depth is inside wgpu's [0,1] clip"
        );

        // The origin fallback misses that same point by orders of magnitude in
        // light space — it is nowhere near the map.
        let at_origin = shadow_light_view_proj(sun, ORIGIN_VOLUME).unwrap();
        let missed = at_origin.transform_point(v.centre);
        let lateral = missed.x.abs().max(missed.y.abs());
        assert!(lateral > 1.0, "the origin volume misses the far camera");
        assert_ne!(far, at_origin);
    }

    /// The regression THIS fix exists to hold, and the one the reference frame
    /// showed: the road well ahead of the car must be inside the shadow map, not
    /// outside it and therefore rendered flat-lit. 20 m of light-space extent
    /// could not reach it; a fitted volume can.
    #[test]
    fn the_road_far_ahead_of_the_car_is_inside_the_shadow_map() {
        let sun = Vec3::new(0.3, -1.0, 0.4);
        let v = portrait();
        let lvp = shadow_light_view_proj(sun, v).unwrap();
        // Road points 20 m, 50 m and 80 m down-track from the eye, at road level.
        [20.0_f32, 50.0, 80.0].into_iter().for_each(|ahead| {
            let p = Vec3::new(0.0, 0.0, 10.0 - ahead);
            let c = lvp.transform_point(p);
            assert!(
                c.x.abs() <= 1.0 && c.y.abs() <= 1.0 && (0.0..=1.0).contains(&c.z),
                "road {ahead} m ahead falls outside the shadow frustum at {c:?}"
            );
        });
        // And a 20 m palm standing beside the road at 50 m casts too: the depth
        // margin is what keeps a tall caster off the near plane.
        let crown = Vec3::new(9.0, 20.0, 10.0 - 50.0);
        let c = lvp.transform_point(crown);
        assert!(
            c.x.abs() <= 1.0 && c.y.abs() <= 1.0 && (0.0..=1.0).contains(&c.z),
            "a palm crown at {crown:?} is clipped out of the shadow map at {c:?}"
        );
    }
}
