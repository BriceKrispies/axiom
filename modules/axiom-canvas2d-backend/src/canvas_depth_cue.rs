//! Pure, deterministic Canvas depth-cue math.
//!
//! Every function here is a small arithmetic transform of colour/depth/geometry
//! — no allocation, no browser types, no scene/game knowledge. The conversion
//! stage (`frame_packet_raster`) calls the **per-triangle** cues (fake lighting,
//! height tint, distance falloff) to bake one flat colour per triangle; the
//! rasterizer post-passes call the **per-pixel** helpers (fog, vertical grade).
//! Splitting the cues this way keeps the hot pixel loop to cheap arithmetic and
//! does all geometry-derived work once per triangle.
//!
//! ## Fake lighting normal
//! A screen-space normal degenerates to "facing the viewer" because screen x/y
//! are in pixels (~hundreds) while NDC depth is ~`[-1,1]`, so the cross product
//! is dominated by the screen-area term. Instead the **model-space** face normal
//! (cross of two model edges, from positions already read for projection) is
//! rotated into world space by the draw's `world` matrix upper-3×3 — a real
//! face normal at ~one cross + one mat3 multiply per triangle.

use crate::canvas_depth_cue_profile::CanvasDepthCueProfile;

/// Smallest squared length treated as non-degenerate when normalizing.
const NORMAL_EPS: f32 = 1e-12;
/// Upper clamp for triangle brightness (keeps a runaway light direction safe).
const MAX_BRIGHTNESS: f32 = 4.0;

/// Which per-triangle cues actually modified a triangle's colour (for counters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TriangleCuesApplied {
    pub(crate) lit: bool,
    pub(crate) height_tinted: bool,
    pub(crate) falloff: bool,
}

/// The world-space face normal of a triangle: the model-space edge cross product
/// rotated by the `world` matrix's upper-3×3 (column-major). Deterministic and
/// NaN-safe (a degenerate triangle yields a zero normal → ambient-only).
pub(crate) fn face_normal_world(model: &[[f32; 3]; 3], world: &[f32; 16]) -> [f32; 3] {
    let e1 = sub3(model[1], model[0]);
    let e2 = sub3(model[2], model[0]);
    let n = cross3(e1, e2);
    let nx = world[0] * n[0] + world[4] * n[1] + world[8] * n[2];
    let ny = world[1] * n[0] + world[5] * n[1] + world[9] * n[2];
    let nz = world[2] * n[0] + world[6] * n[1] + world[10] * n[2];
    normalize3([nx, ny, nz])
}

/// The world-space Y (elevation) of a model point under the `world` matrix
/// (column-major row-1 dot point) — used for the height/elevation tint.
pub(crate) fn world_y(point: [f32; 3], world: &[f32; 16]) -> f32 {
    world[1] * point[0] + world[5] * point[1] + world[9] * point[2] + world[13]
}

/// Triangle brightness from a world-space normal, the (normalized) to-light
/// direction, and the light's `intensity`: `ambient + max(dot, 0) * diffuse *
/// intensity`, optionally quantized into `lighting_band_count` bands, clamped to
/// a safe range. The `ambient`/`diffuse` knobs are the profile's exposure
/// controls; the direction and intensity come from the frame's real scene light.
/// `1.0` (no-op) when lighting is disabled.
pub(crate) fn lighting_brightness(
    normal: [f32; 3],
    light_dir: [f32; 3],
    intensity: f32,
    profile: &CanvasDepthCueProfile,
) -> f32 {
    let raw = dot3(normal, light_dir).max(0.0);
    let lit = [raw, band(raw, profile.lighting.band_count)][usize::from(profile.lighting.banded)];
    let brightness = profile.lighting.ambient + lit * profile.lighting.diffuse * intensity;
    [1.0, brightness.clamp(0.0, MAX_BRIGHTNESS)][usize::from(profile.lighting.enabled)]
}

/// Quantize `x ∈ [0,1]` into `count` bands (floor); `count` is treated as ≥ 1.
fn band(x: f32, count: u32) -> f32 {
    let n = count.max(1) as f32;
    (x * n).floor() / n
}

/// Height factor in `[0,1]` from a world-space Y and the draw's Y extent. A flat
/// draw (`max == min`) maps to `0.0` deterministically (no tint gradient).
pub(crate) fn height_factor(y: f32, y_min: f32, y_max: f32) -> f32 {
    let span = y_max - y_min;
    [0.0, ((y - y_min) / span).clamp(0.0, 1.0)][usize::from(span > NORMAL_EPS)]
}

/// Compose all **per-triangle** cues onto a base linear RGBA colour in the
/// documented order — lighting, then height tint, then distance falloff — and
/// report which ran. Alpha is preserved throughout.
pub(crate) fn shade_triangle(
    base: [f32; 4],
    brightness: f32,
    light_color: [f32; 3],
    hfactor: f32,
    depth: f32,
    profile: &CanvasDepthCueProfile,
) -> ([f32; 4], TriangleCuesApplied) {
    // 3. lighting: multiply RGB by brightness and the scene light's colour (alpha
    // untouched). The colour tint applies only when lighting is on, so a disabled
    // light is a true no-op (neutral white).
    let lit = profile.lighting.enabled;
    let tint = [[1.0, 1.0, 1.0], light_color][usize::from(lit)];
    let after_light = [
        base[0] * brightness * tint[0],
        base[1] * brightness * tint[1],
        base[2] * brightness * tint[2],
        base[3],
    ];

    // 4. height/elevation tint: mix toward the elevation colour.
    let tinted = profile.enable_height_tint;
    let tint = mix4(profile.low_height_color, profile.high_height_color, hfactor);
    let s = [0.0, profile.height_tint_strength][usize::from(tinted)];
    let after_tint = [
        mix(after_light[0], tint[0], s),
        mix(after_light[1], tint[1], s),
        mix(after_light[2], tint[2], s),
        after_light[3],
    ];

    // 5. distance detail/colour falloff: desaturate toward luminance by depth.
    let falloff_on = profile.enable_distance_detail_falloff;
    let t = falloff_t(depth, profile);
    let lum = luminance(after_tint);
    let f = [0.0, t][usize::from(falloff_on)];
    let after_falloff = [
        mix(after_tint[0], lum, f),
        mix(after_tint[1], lum, f),
        mix(after_tint[2], lum, f),
        after_tint[3],
    ];

    (
        after_falloff,
        TriangleCuesApplied {
            lit,
            height_tinted: tinted,
            falloff: falloff_on,
        },
    )
}

/// Distance-falloff fraction (`0` near, up to a modest cap far) from depth and
/// the profile's falloff range. Safe for an inverted/degenerate range.
fn falloff_t(depth: f32, profile: &CanvasDepthCueProfile) -> f32 {
    let span = (profile.detail_falloff_far - profile.detail_falloff_near).abs();
    let inv = [0.0, 1.0 / span][usize::from(span > NORMAL_EPS)];
    // Desaturate up to ~22% at the far end — "slightly less saturated", subtle.
    ((depth - profile.detail_falloff_near) * inv).clamp(0.0, 1.0) * 0.22
}

/// Per-pixel depth-fog mix fraction (already including `fog_strength`): `0` near,
/// `fog_strength` far. Deterministic for any (even inverted) fog range.
pub(crate) fn fog_mix(depth: f32, profile: &CanvasDepthCueProfile) -> f32 {
    let span = (profile.fog.far - profile.fog.near).abs();
    let inv = [0.0, 1.0 / span][usize::from(span > NORMAL_EPS)];
    ((depth - profile.fog.near) * inv).clamp(0.0, 1.0) * profile.fog.strength.clamp(0.0, 1.0)
}

/// Per-pixel vertical-grade mix fraction from screen `y` (0 top → strength
/// bottom): a subtle lower-screen darkening anchor.
pub(crate) fn vertical_grade_mix(y: u32, height: u32, profile: &CanvasDepthCueProfile) -> f32 {
    let h = (height.max(1)) as f32;
    (y as f32 / h).clamp(0.0, 1.0) * profile.vertical_grade_strength.clamp(0.0, 1.0)
}

/// Linear interpolation `a·(1-t) + b·t`.
pub(crate) fn mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// Linear `0.0..=1.0` channel → clamped, rounded RGBA8 byte.
pub(crate) fn to_byte(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        mix(a[0], b[0], t),
        mix(a[1], b[1], t),
        mix(a[2], b[2], t),
        mix(a[3], b[3], t),
    ]
}

/// Rec. 601 luma of an RGBA colour (alpha ignored).
fn luminance(c: [f32; 4]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Normalize a vector; a (near-)zero vector returns zero (NaN-safe).
pub(crate) fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len2 = dot3(v, v);
    let inv = [0.0, len2.sqrt().recip()][usize::from(len2 > NORMAL_EPS)];
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> CanvasDepthCueProfile {
        CanvasDepthCueProfile::low_poly_framebuffer()
    }

    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    #[test]
    fn normalize_is_nan_safe_for_zero() {
        assert_eq!(normalize3([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        assert_eq!(normalize3([0.0, 5.0, 0.0]), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn face_normal_of_xy_triangle_points_up_z() {
        // CCW triangle in the XY plane → normal +Z.
        let m = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        assert_eq!(face_normal_world(&m, &IDENTITY), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn triangle_facing_light_is_brighter_than_facing_away() {
        let p = profile();
        let light = normalize3(p.lighting.direction);
        let toward = lighting_brightness(light, light, 1.0, &p); // normal == light dir
        let away = lighting_brightness([-light[0], -light[1], -light[2]], light, 1.0, &p);
        assert!(toward > away);
        // Ambient floor: even the away-facing triangle is not black.
        assert!(away >= p.lighting.ambient - 1e-6);
    }

    #[test]
    fn higher_intensity_brightens_the_lit_term() {
        let p = profile();
        let n = [0.0, 1.0, 0.0];
        let l = [0.0, 1.0, 0.0]; // normal faces the light
        let dim = lighting_brightness(n, l, 0.5, &p);
        let bright = lighting_brightness(n, l, 2.0, &p);
        assert!(bright > dim);
        // normal·light == 1, so dim == ambient + 1·diffuse·0.5 (intensity scales
        // only the diffuse term, not the ambient floor).
        assert!((dim - (p.lighting.ambient + p.lighting.diffuse * 0.5)).abs() < 1e-6);
    }

    #[test]
    fn disabled_lighting_is_unity() {
        let mut p = profile();
        p.lighting.enabled = false;
        assert_eq!(
            lighting_brightness([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], 1.0, &p),
            1.0
        );
    }

    #[test]
    fn banded_lighting_quantizes_exactly() {
        let mut p = profile();
        p.lighting.banded = true;
        p.lighting.band_count = 4;
        p.lighting.ambient = 0.0;
        p.lighting.diffuse = 1.0;
        // dot == 0.7 → band floor(0.7*4)/4 = 2/4 = 0.5 → brightness 0.5.
        let n = [0.0, 1.0, 0.0];
        let l = [0.0, 0.7, 0.0];
        assert_eq!(lighting_brightness(n, l, 1.0, &p), 0.5);
    }

    #[test]
    fn brightness_clamps_to_safe_range() {
        let mut p = profile();
        p.lighting.ambient = 10.0;
        p.lighting.diffuse = 10.0;
        let b = lighting_brightness([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], 1.0, &p);
        assert_eq!(b, MAX_BRIGHTNESS);
    }

    #[test]
    fn height_factor_low_high_and_flat() {
        assert_eq!(height_factor(0.0, 0.0, 10.0), 0.0);
        assert_eq!(height_factor(10.0, 0.0, 10.0), 1.0);
        assert_eq!(height_factor(5.0, 0.0, 10.0), 0.5);
        // Clamp out of range.
        assert_eq!(height_factor(-3.0, 0.0, 10.0), 0.0);
        assert_eq!(height_factor(13.0, 0.0, 10.0), 1.0);
        // Flat draw → 0 (no gradient, deterministic).
        assert_eq!(height_factor(5.0, 5.0, 5.0), 0.0);
    }

    #[test]
    fn shade_lighting_scales_rgb_preserves_alpha() {
        let mut p = profile();
        p.enable_height_tint = false;
        p.enable_distance_detail_falloff = false;
        // brightness 0.5 halves rgb (white light = no tint), alpha unchanged.
        let (c, applied) = shade_triangle([0.8, 0.4, 0.2, 1.0], 0.5, [1.0, 1.0, 1.0], 0.0, 0.0, &p);
        assert!((c[0] - 0.4).abs() < 1e-6);
        assert!((c[1] - 0.2).abs() < 1e-6);
        assert!((c[2] - 0.1).abs() < 1e-6);
        assert_eq!(c[3], 1.0);
        assert!(applied.lit);
        assert!(!applied.height_tinted);
        assert!(!applied.falloff);
    }

    #[test]
    fn shade_tints_by_the_light_colour_when_lit_and_ignores_it_when_disabled() {
        let mut p = profile();
        p.enable_height_tint = false;
        p.enable_distance_detail_falloff = false;
        // Lit: a red light zeroes the green/blue channels of a white surface.
        let (lit, _) = shade_triangle([1.0, 1.0, 1.0, 1.0], 1.0, [1.0, 0.0, 0.0], 0.0, 0.0, &p);
        assert!((lit[0] - 1.0).abs() < 1e-6);
        assert!(lit[1].abs() < 1e-6);
        assert!(lit[2].abs() < 1e-6);
        // Disabled lighting: the colour tint is neutral (white), so base survives.
        p.lighting.enabled = false;
        let (off, _) = shade_triangle([1.0, 1.0, 1.0, 1.0], 1.0, [1.0, 0.0, 0.0], 0.0, 0.0, &p);
        assert_eq!(off, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn shade_height_tint_strength_zero_is_unchanged() {
        let mut p = profile();
        p.lighting.enabled = false;
        p.enable_distance_detail_falloff = false;
        p.height_tint_strength = 0.0;
        let base = [0.3, 0.5, 0.7, 1.0];
        let (c, _) = shade_triangle(base, 1.0, [1.0, 1.0, 1.0], 1.0, 0.0, &p);
        assert!((c[0] - base[0]).abs() < 1e-6);
        assert!((c[1] - base[1]).abs() < 1e-6);
        assert!((c[2] - base[2]).abs() < 1e-6);
    }

    #[test]
    fn shade_height_tint_pulls_toward_elevation_colour() {
        let mut p = profile();
        p.lighting.enabled = false;
        p.enable_distance_detail_falloff = false;
        p.height_tint_strength = 1.0; // full mix for the test
        p.high_height_color = [1.0, 1.0, 1.0, 1.0];
        p.low_height_color = [0.0, 0.0, 0.0, 1.0];
        // hfactor 1 → tint == high colour → full mix yields the high colour.
        let (hi, _) = shade_triangle([0.5, 0.5, 0.5, 1.0], 1.0, [1.0, 1.0, 1.0], 1.0, 0.0, &p);
        assert!((hi[0] - 1.0).abs() < 1e-6);
        // hfactor 0 → tint == low colour.
        let (lo, _) = shade_triangle([0.5, 0.5, 0.5, 1.0], 1.0, [1.0, 1.0, 1.0], 0.0, 0.0, &p);
        assert!((lo[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn distance_falloff_desaturates_far_more_than_near() {
        let mut p = profile();
        p.lighting.enabled = false;
        p.enable_height_tint = false;
        p.detail_falloff_near = 0.0;
        p.detail_falloff_far = 1.0;
        let base = [1.0, 0.0, 0.0, 1.0]; // saturated red
        let (near, _) = shade_triangle(base, 1.0, [1.0, 1.0, 1.0], 0.0, 0.0, &p); // depth 0 → t 0
        let (far, _) = shade_triangle(base, 1.0, [1.0, 1.0, 1.0], 0.0, 1.0, &p); // depth 1 → t max
                                                                                 // Far red channel is pulled toward luminance (lower); near is unchanged.
        assert!((near[0] - 1.0).abs() < 1e-6);
        assert!(far[0] < near[0]);
    }

    #[test]
    fn fog_mix_near_far_clamp_and_invalid_range() {
        let mut p = profile();
        p.fog.near = 0.0;
        p.fog.far = 1.0;
        p.fog.strength = 1.0;
        assert_eq!(fog_mix(0.0, &p), 0.0); // near: no fog
        assert_eq!(fog_mix(1.0, &p), 1.0); // far: full fog
        assert_eq!(fog_mix(-5.0, &p), 0.0); // clamps below
        assert_eq!(fog_mix(9.0, &p), 1.0); // clamps above
                                           // Degenerate range → 0 (safe), never NaN.
        p.fog.far = 0.0;
        assert_eq!(fog_mix(0.5, &p), 0.0);
    }

    #[test]
    fn vertical_grade_bottom_stronger_than_top() {
        let p = profile();
        let top = vertical_grade_mix(0, 100, &p);
        let bottom = vertical_grade_mix(99, 100, &p);
        assert!(bottom > top);
        assert_eq!(top, 0.0);
        // Zero height never divides by zero.
        assert_eq!(vertical_grade_mix(0, 0, &p), 0.0);
    }

    #[test]
    fn composition_exact_for_a_known_input_and_profile() {
        // Lighting only (height/falloff off), brightness 0.5 on a known colour.
        let mut p = profile();
        p.enable_height_tint = false;
        p.enable_distance_detail_falloff = false;
        let (c, _) = shade_triangle([0.6, 0.4, 0.2, 1.0], 0.5, [1.0, 1.0, 1.0], 0.0, 0.5, &p);
        assert_eq!(c, [0.3, 0.2, 0.1, 1.0]);
    }
}
