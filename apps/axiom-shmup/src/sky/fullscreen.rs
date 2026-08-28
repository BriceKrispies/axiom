//! Ported from Claude-of-Duty `src/sky/fullscreen.js:1-101`.
//!
//! ## The "it is all plumbing" claim, checked
//!
//! [`super`]'s module doc records `fullscreen.js` as unported "in full", on
//! the grounds that it is GPU object lifetimes with no portable computation.
//! That is *nearly* true and it is worth saying exactly where it is not,
//! because "this file is plumbing" is the shape of justification that has
//! already hidden real arithmetic three times in this subsystem.
//!
//! Genuinely unportable, and deliberately absent here: the module-level
//! `BufferGeometry`/`Scene`/`Camera`/`Mesh` singletons, [`blit`]'s
//! `renderer.setRenderTarget` + `renderer.render` pair, the whole `SkyPass`
//! class (a `ShaderMaterial` plus a two-line `render`), and every
//! `WebGLRenderTarget` option `hdrTarget`/`floatTarget` set (`HalfFloatType`/
//! `FloatType`, `RGBAFormat`, filters, wrap modes, `depthBuffer: false`).
//! None of those has a value a CPU test can read.
//!
//! Three things in the file *are* computation, and they are here:
//!
//! 1. [`FULLSCREEN_TRIANGLE`] and [`BOUNDING_SPHERE_RADIUS`] — the vertex
//!    data (`fullscreen.js:16-21`). It is data, not plumbing, and it is
//!    the data the sky dome is drawn with.
//! 2. [`sky_vert_uv`] — `SKY_VERT`'s entire body (`fullscreen.js:36-42`).
//!    The shader is three statements and one of them is arithmetic.
//! 3. [`hdr_target_size`] — `hdrTarget`'s `Math.max(1, w | 0)` size clamp
//!    (`fullscreen.js:81`). `| 0` is ECMAScript `ToInt32`: it truncates
//!    toward zero, wraps modulo 2^32, and maps every non-finite input to 0.
//!    A `as u32`/`as i32` cast in Rust saturates instead of wrapping, so
//!    this is exactly the class of trap the port recipe warns about — see
//!    [`to_int32`].

/// The shared full-screen triangle's `position` attribute, `itemSize` 3 —
/// `fullscreen.js:17-20`. One oversized triangle, not two triangles: it has
/// no interior edge for the rasteriser to double-shade.
pub const FULLSCREEN_TRIANGLE: [f32; 9] = [-1.0, -1.0, 0.0, 3.0, -1.0, 0.0, -1.0, 3.0, 0.0];

/// `_geometry.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e8)` —
/// `fullscreen.js:21`. Deliberately absurd so nothing ever frustum-culls it.
pub const BOUNDING_SPHERE_RADIUS: f64 = 1e8;

/// `_geometry.boundingSphere`'s centre — the origin.
pub const BOUNDING_SPHERE_CENTER: [f64; 3] = [0.0, 0.0, 0.0];

/// `SKY_VERT`'s body, `fullscreen.js:36-42`:
///
/// ```glsl
/// vUv = position.xy * 0.5 + 0.5;
/// gl_Position = vec4( position.xy, 0.0, 1.0 );
/// ```
///
/// Returns `vUv`. `gl_Position` is `(x, y, 0, 1)` — the position passed
/// straight through, which is why there is nothing to return for it.
pub fn sky_vert_uv(x: f64, y: f64) -> (f64, f64) {
    (x * 0.5 + 0.5, y * 0.5 + 0.5)
}

/// ECMAScript `ToInt32(v)` — what JavaScript's `v | 0` does.
///
/// Not `v as i32`. Rust's float-to-int cast **saturates** (`1e21 as i32` is
/// `i32::MAX`); `ToInt32` truncates toward zero and then takes the value
/// modulo 2^32 into `[-2^31, 2^31)`, so `1e21 | 0` is `-559939584` and
/// `4294967297 | 0` is `1`. Non-finite inputs (`NaN`, `±Infinity`) map to 0.
///
/// This lives here rather than in [`crate::jsmath`] only because that module
/// belongs to another slice of this port and may not be edited from here; if
/// a second call site ever appears it should move there, which is exactly the
/// consolidation `jsmath`'s own doc argues for.
pub fn to_int32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    // `2^32`; `rem_euclid` lands in `[0, 2^32)`, then fold the top half down.
    let m = v.trunc().rem_euclid(4_294_967_296.0);
    if m >= 2_147_483_648.0 {
        (m - 4_294_967_296.0) as i32
    } else {
        m as i32
    }
}

/// `hdrTarget(w, h)`'s dimension clamp, `fullscreen.js:81`:
/// `Math.max(1, w | 0)`. `floatTarget` shares it — that function is
/// `hdrTarget` with one option overridden (`fullscreen.js:99-101`), so it
/// clamps identically and needs no separate port.
pub fn hdr_target_size(w: f64, h: f64) -> (u32, u32) {
    // `Math.max(1, ...)` on an i32, so the result is always >= 1.
    let cw = to_int32(w).max(1);
    let ch = to_int32(h).max(1);
    (cw as u32, ch as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_triangle_covers_the_whole_clip_square() {
        // Every corner of the [-1, 1] clip square is inside the triangle.
        assert_eq!(FULLSCREEN_TRIANGLE.len(), 9);
        assert_eq!(&FULLSCREEN_TRIANGLE[0..3], &[-1.0, -1.0, 0.0]);
        assert_eq!(&FULLSCREEN_TRIANGLE[3..6], &[3.0, -1.0, 0.0]);
        assert_eq!(&FULLSCREEN_TRIANGLE[6..9], &[-1.0, 3.0, 0.0]);
    }

    #[test]
    fn vert_uv_maps_clip_space_onto_the_unit_square() {
        assert_eq!(sky_vert_uv(-1.0, -1.0), (0.0, 0.0));
        assert_eq!(sky_vert_uv(1.0, 1.0), (1.0, 1.0));
        assert_eq!(sky_vert_uv(0.0, 0.0), (0.5, 0.5));
    }

    #[test]
    fn to_int32_wraps_where_an_as_cast_would_saturate() {
        assert_eq!(to_int32(4_294_967_297.0), 1);
        assert_eq!(to_int32(2_147_483_648.0), -2_147_483_648);
        assert_eq!(to_int32(-4.9), -4);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(f64::INFINITY), 0);
        // The saturating `as` cast would give `i32::MAX` for both of these.
        assert_eq!(to_int32(1e21), -559_939_584);
        assert_eq!(to_int32(-1e21), 559_939_584);
    }

    #[test]
    fn target_size_never_goes_below_one() {
        assert_eq!(hdr_target_size(512.0, 256.0), (512, 256));
        assert_eq!(hdr_target_size(0.0, 0.0), (1, 1));
        assert_eq!(hdr_target_size(-4.0, -9.0), (1, 1));
        assert_eq!(hdr_target_size(1920.75, 1080.9), (1920, 1080));
    }
}
