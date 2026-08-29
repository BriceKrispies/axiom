//! **Cascaded shadow maps — the CPU reference.**
//!
//! A transcription of `C:/dev/Claude-of-Duty/src/render/csm.js` (`4x2048 CSM`,
//! the configuration the original boots with): the split scheme, the
//! per-cascade bounding-sphere ortho fit, the whole-texel snap, the atlas
//! layout, and the fragment stage's cascade selection, cross-fade and
//! PCSS/Vogel filter. Everything here is pure arithmetic over plain numbers, so
//! the maths that decides *where a shadow lands* is measured by the coverage
//! gate and comparable against a real adapter, rather than being observable only
//! as a wrong pixel.
//!
//! ## What the source does, in order
//!
//! 1. **Splits.** A practical (log↔uniform) blend at `lambda = 0.86` over
//!    `[camera.near, min(camera.far, MAX_DISTANCE)]`, with the two ends written
//!    exactly rather than through the blend. `lambda` weights the *logarithmic*
//!    term here — 0.86 is heavily logarithmic, which is what puts three of the
//!    four cascades inside the first ~30 m of a 140 m range.
//! 2. **Fit.** Each sub-frustum's **bounding sphere**, in view space, on the
//!    `-z` axis, from the closed form in [`sub_frustum_sphere`]. A sphere rather
//!    than the eight corners because a sphere is rotation-invariant: the ortho
//!    extent does not change as the camera turns. The radius is then quantised
//!    to 1/16 of a world unit (`ceil(r * 16) / 16`) so float drift in the fov
//!    terms cannot wobble it frame to frame.
//! 3. **Snap.** The world origin is projected into the cascade's light clip and
//!    the projection is translated so that point lands on a whole texel. That
//!    nails the sampled texel grid to *world* space rather than to the camera,
//!    which is what removes shadow swimming. The two stabilisers are
//!    complementary and both are required: the sphere fixes the grid's *size*,
//!    the snap fixes its *phase*.
//! 4. **Uniforms.** Per cascade: far split, near split, world texel size, and
//!    the light depth range. Cascades past `count` are filled with the source's
//!    own sentinels (`1e9 / 1e9 / 0.01 / 1.0`) so the shader's `vd < split[i]`
//!    scan never selects one.
//! 5. **Fragment.** Select the first cascade whose far split exceeds the view
//!    depth, normal-offset + slope-scaled bias, optional PCSS blocker search,
//!    Vogel-disk PCF, a cross-fade over the last 12% of the cascade, and a
//!    global fade-out over the last 12% of the whole range.
//!
//! ## Deliberate divergences from the source, and why
//!
//! - **Clip convention.** three targets GL clip (`z ∈ [-1, 1]`, `v` up); wgpu
//!   uses `z ∈ [0, 1]` and a `v`-down framebuffer. [`CascadeSet`] therefore
//!   carries the *wgpu* matrix — the GL ortho premultiplied by the same
//!   `GL_TO_WGPU_DEPTH` fix the engine's camera and single-cascade shadow
//!   already apply — and [`project`] reads `ndc.z` directly instead of the
//!   source's `* 0.5 + 0.5`, and flips `v`. This is renderer convention, not
//!   algorithm: the depth stored in the map is still *linear* in light space
//!   (the projection is orthographic), so the source's `(recv - blocker) * range`
//!   metres conversion and its `bias / range` are unchanged.
//! - **Rounding.** JS `Math.round` rounds a half *up* (toward `+∞`);
//!   `f32::round` rounds a half *away from zero*. The snap transcribes the
//!   source's rule as `floor(x + 0.5)`, and makes that one decision in `f64` —
//!   the width JS does all of `csm.js` in — because it is the only place in the
//!   fit where a last-bit difference changes an integer rather than a smooth
//!   quantity. The splits and the sphere solve are computed in `f64` and
//!   narrowed once, for the same reason.
//! - **Light-loop identity test.** The source's `owSunShadow` opens with
//!   `dot(lightDirView, owSunDirView) < 0.999 → 1.0`, which exists to pick the
//!   sun out of three's directional-light *loop*. Axiom has exactly one
//!   shadow-casting directional light, so there is no loop to pick out of and
//!   the test has no referent here. Not ported; recorded so its absence is a
//!   decision rather than an omission.
//! - **World reconstruction.** The source recomputes `wPos`/`wN` from view space
//!   inside the shader. Axiom's fragment stage already carries `world_pos` and a
//!   world normal, so [`sun_shadow`] takes them directly. Same values, one fewer
//!   matrix.
//!
//! ## Branchlessness
//!
//! The Rust here is branchless: the sphere's two-case solve, the up-vector pick,
//! the shader's early-outs and the cross-fade gate are all table selects over
//! values that are always safe to evaluate. Where the source *early-returns*
//! before a divide (`blocker /= count` after `count < 0.5` returns), the
//! reference divides by a clamped denominator and discards the result through
//! the same select — value-equivalent, and a table index is immune to the `NaN`
//! an unclamped divide would produce. The WGSL this module specifies keeps the
//! source's control flow verbatim: shader text is data, and a filter loop stays
//! a filter loop.

use axiom_math::{Mat4, Vec3, Vec4};

/// The most cascades the atlas and the uniform lanes hold. The source clamps its
/// `cascades` option to `[1, 4]` and packs every per-cascade lane into a `vec4`,
/// so four is the shape of the data, not a budget.
pub(crate) const MAX_CASCADES: usize = 4;

/// The atlas edge, in texels. The source clamps `mapSize` to at most this:
/// "4 x 4096 x R32F is a quarter of a gigabyte for shadows nobody can see; 2048
/// with PCSS reads sharper than 4096 without it."
pub(crate) const MAP_SIZE: u32 = 2048;

/// The practical-split blend weight. Weights the **logarithmic** term.
pub(crate) const LAMBDA: f64 = 0.86;

/// How far up-sun of its fitted sphere each cascade's eye sits, beyond the
/// sphere's own radius. Everything within this of the volume still casts.
pub(crate) const BACK_DISTANCE: f32 = 140.0;

/// The furthest the cascade range ever reaches, whatever the camera's far plane.
pub(crate) const MAX_DISTANCE: f32 = 140.0;

/// The near plane of every cascade's ortho. Exactly zero in the source: an
/// orthographic projection has no reciprocal-of-near term, so zero is a legal
/// plane and it spends none of the depth range on empty space in front of the
/// eye.
const NEAR: f32 = 0.0;

/// The split/near-split sentinel written into cascades past `count`. Larger than
/// any view depth, so the shader's `vd < split[i]` scan can never select one.
const UNUSED_SPLIT: f32 = 1.0e9;
/// The world-texel sentinel for an unused cascade.
const UNUSED_TEXEL: f32 = 0.01;
/// The depth-range sentinel for an unused cascade. Never zero — it is a divisor.
const UNUSED_RANGE: f32 = 1.0;

/// Radius quantum: the fitted radius is rounded up to a multiple of `1/16` of a
/// world unit so float drift in the fov terms cannot wobble the ortho extent.
const RADIUS_QUANTUM: f64 = 16.0;

/// How near-vertical the sun has to be before the up vector switches to `+Z`.
const VERTICAL_SUN: f32 = 0.98;

/// Caster-cull margin, in shadow texels. The shader samples outside a receiver's
/// own projected point by up to the sum of the whole-texel snap (1), the normal
/// offset (1.65 at grazing incidence), the PCSS blocker search (10) and the PCF
/// disc (`max_filter_texels`, 9 at ultra); 32 is comfortably past that sum and
/// still 1.5% of a cascade's extent. The source measured 2 texels as *not*
/// output-preserving (0.04% of pixels, up to 26/255).
const CULL_MARGIN_TEXELS: f32 = 32.0;

/// The wgpu depth remap: GL clip `z ∈ [-1, 1]` → wgpu clip `z ∈ [0, 1]`. The
/// same matrix `axiom-render-pipeline` applies to the camera and to the existing
/// single-cascade shadow, column-major.
const GL_TO_WGPU_DEPTH: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0,
];

/// Bytes one atlas occupies: `count` layers of `map_size²` R32Float texels. The
/// source's "quarter of a gigabyte" remark is this function at `4 x 4096`; at the
/// shipped `4 x 2048` it is 67 MB.
pub(crate) fn atlas_byte_size(map_size: u32, count: usize) -> u64 {
    u64::from(map_size) * u64::from(map_size) * 4 * count.min(MAX_CASCADES).max(1) as u64
}

/// The camera the cascades are fitted to. `world` is the camera's world matrix
/// (three's `matrixWorld`): the sphere centres are `(0, 0, cz)` pushed through
/// it, so the volume follows the view without the fit needing a view matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CascadeCamera {
    pub(crate) world: Mat4,
    pub(crate) fovy_radians: f32,
    pub(crate) aspect: f32,
    pub(crate) near: f32,
    pub(crate) far: f32,
}

/// `owCsmParams`, lane for lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CascadeParams {
    /// `x` — shadow strength. `<= 0` disables the term entirely.
    pub(crate) strength: f32,
    /// `y` — `tan(sun angular radius)`; the metres-of-penumbra-per-metre-of-gap
    /// factor PCSS multiplies the blocker gap by.
    pub(crate) softness: f32,
    /// `z` — the PCF disc's maximum radius, in texels.
    pub(crate) max_filter_texels: f32,
    /// `w` — temporal rotation added to the per-pixel Vogel phase.
    pub(crate) rotation: f32,
}

impl Default for CascadeParams {
    /// The source's constructor value: `vec4(1, 0.022, 9, 0)`.
    fn default() -> Self {
        CascadeParams {
            strength: 1.0,
            softness: 0.022,
            max_filter_texels: 9.0,
            rotation: 0.0,
        }
    }
}

/// Tap counts and whether PCSS runs, from the source's quality tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CascadeQuality {
    pub(crate) blocker_taps: u32,
    pub(crate) pcf_taps: u32,
    pub(crate) pcss: bool,
}

/// `csmShaderChunk`'s tier table: `quality >= 3 ? 16 : quality >= 2 ? 12 : 8`
/// blocker taps, `20 / 14 / 8` PCF taps, PCSS at `quality >= 2`.
pub(crate) fn quality_tier(level: u32) -> CascadeQuality {
    let index = level.min(3) as usize;
    CascadeQuality {
        blocker_taps: [8, 8, 12, 16][index],
        pcf_taps: [8, 8, 14, 20][index],
        pcss: [false, false, true, true][index],
    }
}

/// One cascade's fit. Kept flat rather than behind accessors because every field
/// is read by either the uniform pack or the caster cull.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    view_proj: Mat4,
    split_near: f32,
    split_far: f32,
    texel: f32,
    range: f32,
    centre: Vec3,
    radius: f32,
}

/// The source's `for (let i = N; i < 4; i++)` sentinel row.
const UNUSED_FIT: Fit = Fit {
    view_proj: Mat4::IDENTITY,
    split_near: UNUSED_SPLIT,
    split_far: UNUSED_SPLIT,
    texel: UNUSED_TEXEL,
    range: UNUSED_RANGE,
    centre: Vec3::ZERO,
    radius: 0.0,
};

/// A whole frame's cascade fit: `count` live cascades plus the sentinel rows,
/// packed exactly as the shader's `vec4` lanes expect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CascadeSet {
    count: usize,
    map_size: u32,
    fits: [Fit; MAX_CASCADES],
}

impl CascadeSet {
    /// How many cascades are live. Always in `[1, MAX_CASCADES]`.
    pub(crate) fn count(&self) -> usize {
        self.count
    }

    /// The atlas edge this set was fitted against, in texels.
    pub(crate) fn map_size(&self) -> u32 {
        self.map_size
    }

    /// `owCsmMatrix` — the light view-projection of each cascade, in wgpu clip.
    pub(crate) fn matrices(&self) -> [[f32; 16]; MAX_CASCADES] {
        core::array::from_fn(|i| self.fits[i].view_proj.as_cols_array())
    }

    /// `owCsmSplit` — each cascade's FAR view depth.
    pub(crate) fn split(&self) -> [f32; MAX_CASCADES] {
        core::array::from_fn(|i| self.fits[i].split_far)
    }

    /// `owCsmSplitNear` — each cascade's NEAR view depth (the cross-fade's `a`).
    pub(crate) fn split_near(&self) -> [f32; MAX_CASCADES] {
        core::array::from_fn(|i| self.fits[i].split_near)
    }

    /// `owCsmTexel` — one shadow texel in world units, `2r / map_size`.
    pub(crate) fn texel(&self) -> [f32; MAX_CASCADES] {
        core::array::from_fn(|i| self.fits[i].texel)
    }

    /// `owCsmRange` — the light depth range `far - near` each cascade's stored
    /// depth is normalised over. The metres-per-unit-depth factor PCSS needs.
    pub(crate) fn range(&self) -> [f32; MAX_CASCADES] {
        core::array::from_fn(|i| self.fits[i].range)
    }

    /// The world-space centre of cascade `i`'s fitted sphere.
    pub(crate) fn centre(&self, index: usize) -> Vec3 {
        self.fits[index.min(MAX_CASCADES - 1)].centre
    }

    /// The world-space radius of cascade `i`'s fitted sphere.
    pub(crate) fn radius(&self, index: usize) -> f32 {
        self.fits[index.min(MAX_CASCADES - 1)].radius
    }

    /// The caster-cull slack for cascade `i`, in world units:
    /// [`CULL_MARGIN_TEXELS`] shadow texels. A caster outside
    /// `radius + margin` laterally, or outside the light-axis slab
    /// `[-radius - margin, radius + BACK_DISTANCE + margin]`, cannot darken any
    /// texel this cascade is ever *sampled* at.
    pub(crate) fn cull_margin(&self, index: usize) -> f32 {
        CULL_MARGIN_TEXELS * self.fits[index.min(MAX_CASCADES - 1)].texel
    }
}

/// The cascade boundaries: `count + 1` view depths over `[near, far]`.
///
/// Transcribed from `update()`:
/// ```text
/// s[0] = n
/// s[i] = lambda * (n * (f/n)^(i/N))  +  (1 - lambda) * (n + (f - n) * (i/N))
/// s[N] = f
/// ```
/// The two ends are written directly, not through the blend — at `p = 0` and
/// `p = 1` the blend is algebraically `n` and `f`, but not bit-for-bit, and the
/// shader compares against these values.
pub(crate) fn splits(count: usize, near: f32, far: f32) -> [f32; MAX_CASCADES + 1] {
    let count = count.min(MAX_CASCADES).max(1);
    let n = f64::from(near);
    let f = f64::from(far);
    core::array::from_fn(|i| {
        let p = i as f64 / count as f64;
        let log_split = n * (f / n).powf(p);
        let uni_split = n + (f - n) * p;
        let blended = (LAMBDA * log_split + (1.0 - LAMBDA) * uni_split) as f32;
        // i == 0 -> exactly `near`; i >= count -> exactly `far`.
        let end = [far, near][usize::from(i == 0)];
        [end, blended][usize::from((i > 0) & (i < count))]
    })
}

/// The bounding sphere of the view-space sub-frustum `[cn, cf]`, as
/// `(centre_z, radius)` on the `-z` axis. `k2 = tan(fovy/2)² + tan(fovx/2)²`.
///
/// Two cases, exactly as the source writes them:
///
/// - `k2² * (cf + cn) >= cf - cn` — the sphere through both caps would sit past
///   the far cap, so the far cap's own circumcircle is the whole answer:
///   `cz = -cf`, `r = cf * sqrt(k2)`.
/// - otherwise the equidistant point, `cz = -0.5 * (cf + cn) * (1 + k2)`, with
///   the closed-form radius.
///
/// Both arms are always finite, so this is a table select rather than a branch.
/// Solved in `f64` and narrowed once — the source is JS, which is `f64`
/// throughout, and the quantisation below makes the result exact in `f32`.
pub(crate) fn sub_frustum_sphere(cn: f32, cf: f32, k2: f32) -> (f32, f32) {
    let cn = f64::from(cn);
    let cf = f64::from(cf);
    let k2 = f64::from(k2);
    let far_cap = k2 * k2 * (cf + cn) >= cf - cn;
    let cz = [-0.5 * (cf + cn) * (1.0 + k2), -cf][usize::from(far_cap)];
    let radius = [
        0.5 * ((cf - cn) * (cf - cn)
            + 2.0 * (cf * cf + cn * cn) * k2
            + (cf + cn) * (cf + cn) * k2 * k2)
            .sqrt(),
        cf * k2.sqrt(),
    ][usize::from(far_cap)];
    // `ceil(r * 16) / 16`: stabilise the radius against float drift. The result
    // is a multiple of 1/16 and therefore exact in f32.
    (
        cz as f32,
        ((radius * RADIUS_QUANTUM).ceil() / RADIUS_QUANTUM) as f32,
    )
}

/// The whole-texel snap's translation, as `(dx, dy)` in NDC.
///
/// The world origin is projected through `view_proj` and the projection is
/// nudged so that point lands on a whole texel of the `map_size` grid. Because
/// the same world point is snapped every frame, the grid is nailed to world
/// space rather than to the camera, which is what stops the shadow swimming as
/// the camera moves.
///
/// `Math.round(x)` in JS is `floor(x + 0.5)` — it rounds a half toward `+∞`,
/// where `f32::round` rounds a half away from zero. Transcribed as the source
/// writes it, and evaluated in `f64`: this is the only step in the fit where a
/// last-bit difference selects a different integer rather than perturbing a
/// smooth quantity.
pub(crate) fn texel_snap(view_proj: Mat4, map_size: u32) -> (f32, f32) {
    let origin = view_proj.transform_vec4(Vec4::new(0.0, 0.0, 0.0, 1.0));
    let half = f64::from(map_size) * 0.5;
    let sx = f64::from(origin.x) * half;
    let sy = f64::from(origin.y) * half;
    (
        (((sx + 0.5).floor() - sx) / half) as f32,
        (((sy + 0.5).floor() - sy) / half) as f32,
    )
}

/// Nail an **already-composed** light view-projection to the shadow map's
/// whole-texel grid — [`texel_snap`]'s other caller, for the single-cascade path
/// that `axiom-render-pipeline` fits.
///
/// # Why the single-cascade path needs this too
///
/// [`fit_one`] applies the snap because this module's header sets out why it is
/// mandatory: *"the sphere fixes the grid's **size**, the snap fixes its
/// **phase**"*, and **both are required** to stop a shadow swimming.
/// `axiom_render_pipeline::shadow_view` does the first half — it fits the ortho
/// box to the view frustum's bounding sphere, which is rotation-invariant, so
/// the box stops changing size as the camera turns — and then does not do the
/// second. Its volume is centred on the camera and moves continuously with it,
/// so the texel grid slides under the world every frame.
///
/// What that looks like is not a shimmer at the shadow's edge. At the atlas
/// sizes a device tier actually grants (1024 or 2048 over a ~116 m box, so 6-11
/// cm texels filtered by a 5x5 PCF at a 1.25-texel spread) the penumbra is
/// 35-70 cm wide and quantised into 25 discrete taps, so the edge is a visible
/// staircase — and an unsnapped grid makes that whole staircase crawl across the
/// ground as the player walks. Reported from the game as "a second projection of
/// the world" on the road, visible when zoomed and moving.
///
/// # Why it can be applied after composition
///
/// [`fit_one`] snaps the *projection*, before the view multiply and the depth
/// fix. Applying it to the composed `depth_fix * proj * view` is the same
/// matrix. Writing `T` for the translation that adds `(dx, dy)` to the
/// projection's last column, `D * (P + T) * V = D*P*V + D*T*V`; `T` is zero
/// except `T[0][3] = dx`, `T[1][3] = dy`, and an affine view matrix has last row
/// `(0, 0, 0, 1)`, so `T*V` is zero except the same two entries, and `D` (which
/// touches only `z`) leaves them alone. So the product differs from the
/// unsnapped one in exactly elements 12 and 13 — which is what this adds. The
/// probe itself is unaffected by the depth fix for the same reason: it reads
/// only `x` and `y` of the projected origin.
pub(crate) fn snap_view_proj(view_proj: [f32; 16], map_size: u32) -> [f32; 16] {
    let (dx, dy) = texel_snap(Mat4::from_cols_array(view_proj), map_size.max(1));
    let mut snapped = view_proj;
    snapped[12] += dx;
    snapped[13] += dy;
    snapped
}

/// Fit `count` cascades to `camera` for a sun at `sun_dir` (pointing **from the
/// scene toward the sun**, the source's convention — the negation of the
/// travel direction `axiom-render-pipeline`'s single-cascade fit takes).
///
/// `None` for a degenerate sun (zero direction, so the look-at basis collapses)
/// or a degenerate camera (a zero-extent volume, which no orthographic
/// projection exists for). The caller substitutes the unshadowed path, exactly
/// as the single-cascade fit's `None` already does.
pub(crate) fn fit(
    count: usize,
    camera: CascadeCamera,
    sun_dir: Vec3,
    map_size: u32,
) -> Option<CascadeSet> {
    let count = count.min(MAX_CASCADES).max(1);
    let map_size = map_size.max(1);
    let far = camera.far.min(MAX_DISTANCE);
    let bounds = splits(count, camera.near, far);
    let tan_v = (camera.fovy_radians * 0.5).tan();
    let tan_h = tan_v * camera.aspect;
    let k2 = tan_v * tan_v + tan_h * tan_h;
    sun_dir.normalize().ok().and_then(|sun| {
        // `(fit, resolved)` rather than `Option<Fit>`: a cascade past `count` is
        // the sentinel row and is trivially resolved, and carrying the
        // discriminant beside the value keeps the array total — no per-element
        // default to unwrap back out of an `Option` afterwards.
        let built: [(Fit, bool); MAX_CASCADES] = core::array::from_fn(|i| {
            [
                (UNUSED_FIT, true),
                fit_one(bounds[i], bounds[i + 1], k2, camera.world, sun, map_size),
            ][usize::from(i < count)]
        });
        built
            .iter()
            .take(count)
            .all(|(_, resolved)| *resolved)
            .then(|| CascadeSet {
                count,
                map_size,
                fits: core::array::from_fn(|i| built[i].0),
            })
    })
}

/// One cascade's ortho fit, snap included. `false` when the look-at basis or the
/// ortho box is degenerate — a zero sun, or a zero-extent volume.
fn fit_one(
    cn: f32,
    cf: f32,
    k2: f32,
    camera_world: Mat4,
    sun: Vec3,
    map_size: u32,
) -> (Fit, bool) {
    let (cz, radius) = sub_frustum_sphere(cn, cf, k2);
    let centre = camera_world.transform_point(Vec3::new(0.0, 0.0, cz));
    // A near-vertical sun makes the default up parallel to the view direction;
    // the source swaps to +Z past |y| > 0.98. Table pick, no branch.
    let up = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)]
        [usize::from(sun.y.abs() > VERTICAL_SUN)];
    let eye = centre.add(sun.mul_scalar(radius + BACK_DISTANCE));
    let cam_far = 2.0 * radius + BACK_DISTANCE;
    let depth_fix = Mat4::from_cols_array(GL_TO_WGPU_DEPTH);
    Mat4::look_at(eye, centre, up)
        .ok()
        .and_then(|view| {
            Mat4::orthographic(-radius, radius, -radius, radius, NEAR, cam_far)
                .ok()
                .map(|proj| (view, proj))
        })
        .map_or((UNUSED_FIT, false), |(view, proj)| {
            // Snap in the source's GL clip space, before the depth fix — the fix
            // touches only `z`, so the order is immaterial to x/y, and matching
            // the source's space keeps the transcription readable.
            let (dx, dy) = texel_snap(proj.multiply(view), map_size);
            let mut snapped = proj.as_cols_array();
            snapped[12] += dx;
            snapped[13] += dy;
            (
                Fit {
                    view_proj: depth_fix
                        .multiply(Mat4::from_cols_array(snapped))
                        .multiply(view),
                    split_near: cn,
                    split_far: cf,
                    texel: (2.0 * radius) / map_size as f32,
                    range: cam_far - NEAR,
                    centre,
                    radius,
                },
                true,
            )
        })
}

// The fragment stage the atlas is *read* by: cascade selection, the
// normal-offset/slope bias, the PCSS blocker search, the Vogel-disc PCF, the
// cross-fade and the far fade-out. Split from the fit above because they are
// two different questions — where the map looks from, and how a receiver reads
// it — and because one file may not exceed the engine size budget.
mod shading;

#[cfg(test)]
mod tests {
    use super::shading::project;
    use super::*;

    /// A light view-projection of the shape `axiom_render_pipeline::shadow_view`
    /// builds: an ortho box of `radius` fitted around a centre that follows the
    /// camera, looking down-sun, depth-fixed to wgpu clip.
    fn single_cascade_vp(centre: Vec3, radius: f32) -> [f32; 16] {
        let sun = Vec3::new(0.35, 0.82, 0.45).normalize().expect("a real sun");
        let eye = centre.add(sun.mul_scalar(radius + BACK_DISTANCE));
        let view = Mat4::look_at(eye, centre, Vec3::new(0.0, 1.0, 0.0)).expect("a real basis");
        let proj = Mat4::orthographic(-radius, radius, -radius, radius, NEAR, 2.0 * radius + BACK_DISTANCE)
            .expect("a real box");
        Mat4::from_cols_array(GL_TO_WGPU_DEPTH)
            .multiply(proj)
            .multiply(view)
            .as_cols_array()
    }

    /// The snap's definition: after it, the world origin sits **on** a whole
    /// texel, so re-probing finds nothing left to move.
    #[test]
    fn snapping_leaves_the_origin_on_a_whole_texel() {
        let vp = single_cascade_vp(Vec3::new(3.7, 0.0, -11.3), 58.0);
        let snapped = snap_view_proj(vp, 2048);
        let (dx, dy) = texel_snap(Mat4::from_cols_array(snapped), 2048);
        // Half a texel in NDC is `1 / map_size`; a residual this far under it is
        // float noise, not a grid the probe would move again.
        assert!(dx.abs() < 1.0e-6, "x residual {dx}");
        assert!(dy.abs() < 1.0e-6, "y residual {dy}");
    }

    /// It is a translation in clip x/y and nothing else — the argument in
    /// `snap_view_proj`'s doc, checked rather than asserted in prose.
    #[test]
    fn snapping_touches_only_the_two_translation_lanes() {
        let vp = single_cascade_vp(Vec3::new(-2.0, 1.0, 4.0), 40.0);
        let snapped = snap_view_proj(vp, 1024);
        (0..16)
            .filter(|i| *i != 12 && *i != 13)
            .for_each(|i| assert_eq!(vp[i], snapped[i], "lane {i} moved"));
        assert!(
            (vp[12] != snapped[12]) | (vp[13] != snapped[13]),
            "the probe found nothing to snap on a volume that is not already aligned"
        );
    }

    /// **The defect this exists to remove.** Two camera positions a few
    /// centimetres apart — one walking pace at 60 Hz — must land on the *same*
    /// texel grid, or the whole quantised penumbra crawls across the ground.
    ///
    /// Measured on the unsnapped matrices for contrast: the grid moves by a
    /// fraction of a texel every frame, which is exactly the swim.
    #[test]
    fn a_walking_camera_keeps_one_texel_grid() {
        let radius = 58.0;
        let a = single_cascade_vp(Vec3::new(0.0, 0.0, 0.0), radius);
        let b = single_cascade_vp(Vec3::new(0.05, 0.0, 0.03), radius);
        // The grid's phase is where the origin lands, in texels.
        let phase = |m: [f32; 16]| {
            let o = Mat4::from_cols_array(m).transform_vec4(Vec4::new(0.0, 0.0, 0.0, 1.0));
            let half = 2048.0_f32 * 0.5;
            (o.x * half, o.y * half)
        };
        let (ux, uy) = phase(a);
        let (vx, vy) = phase(b);
        assert!(
            ((ux - vx).abs() > 1.0e-4) | ((uy - vy).abs() > 1.0e-4),
            "the unsnapped grids already agreed, so this test proves nothing"
        );
        let (sx, sy) = phase(snap_view_proj(a, 2048));
        let (tx, ty) = phase(snap_view_proj(b, 2048));
        // Both land on a whole texel, so both round to the same integer phase.
        assert!((sx - sx.round()).abs() < 1.0e-3, "a is off-grid: {sx}");
        assert!((tx - tx.round()).abs() < 1.0e-3, "b is off-grid: {tx}");
        assert!((sy - sy.round()).abs() < 1.0e-3, "a is off-grid: {sy}");
        assert!((ty - ty.round()).abs() < 1.0e-3, "b is off-grid: {ty}");
    }

    /// A zero map size is clamped rather than dividing by zero — the same
    /// posture `fit` takes for its own `map_size`.
    #[test]
    fn a_zero_map_size_is_clamped_and_finite() {
        let vp = single_cascade_vp(Vec3::new(1.0, 0.0, 2.0), 12.0);
        let snapped = snap_view_proj(vp, 0);
        assert!(snapped.iter().all(|v| v.is_finite()));
    }

    /// The source's shipped configuration: `4 x 2048`, 140 m, lambda 0.86.
    fn street_camera() -> CascadeCamera {
        CascadeCamera {
            world: Mat4::translation(Vec3::new(0.0, 3.0, 10.0)),
            fovy_radians: 60_f32.to_radians(),
            aspect: 16.0 / 9.0,
            near: 0.5,
            far: 300.0,
        }
    }

    /// Pointing FROM the scene TOWARD the sun, the source's convention.
    fn sun() -> Vec3 {
        Vec3::new(0.35, 0.85, 0.4).normalize().unwrap()
    }

    fn street_set() -> CascadeSet {
        fit(4, street_camera(), sun(), MAP_SIZE).unwrap()
    }

    #[test]
    fn the_split_scheme_is_the_practical_blend_at_lambda_0_86() {
        let s = splits(4, 0.5, 140.0);
        // Ends are exact, not the blend's algebraic equivalent.
        assert_eq!(s[0], 0.5, "s[0] is written as `near`");
        assert_eq!(s[4], 140.0, "s[N] is written as `far`");
        // Recompute the interior boundaries independently from the formula text.
        [1_usize, 2, 3].into_iter().for_each(|i| {
            let p = f64::from(i as u32) / 4.0;
            let expected = (0.86 * (0.5 * (140.0_f64 / 0.5).powf(p))
                + 0.14 * (0.5 + 139.5 * p)) as f32;
            assert_eq!(s[i], expected, "split {i} is the lambda-0.86 blend");
        });
        // The whole point of lambda 0.86: it is heavily logarithmic, so three of
        // the four cascades sit inside the first ~45 m of a 140 m range. A
        // uniform split would have put them at 35 / 70 / 105.
        let (s1, s2, s3) = (s[1], s[2], s[3]);
        assert!(s1 < 7.0, "s[1] = {s1} must stay near the eye");
        assert!(s2 < 18.0, "s[2] = {s2} must stay near the eye");
        assert!(s3 < 45.0, "s[3] = {s3} must stay near the eye");
        assert!((s[1] < s[2]) & (s[2] < s[3]), "splits ascend");
        // A count of one is the whole range in one slice, and the clamp holds
        // outside [1, 4].
        assert_eq!(splits(1, 0.5, 140.0)[1], 140.0);
        assert_eq!(splits(0, 0.5, 140.0), splits(1, 0.5, 140.0));
        assert_eq!(splits(9, 0.5, 140.0), splits(4, 0.5, 140.0));
    }

    #[test]
    fn the_sub_frustum_sphere_takes_both_arms_of_the_source_solve() {
        let k2 = {
            let tv = (60_f32.to_radians() * 0.5).tan();
            let th = tv * (16.0 / 9.0);
            tv * tv + th * th
        };
        // A NARROW fov over a long slice takes the general arm: `k2` is small,
        // so `k2^2 * (cf + cn)` stays under `cf - cn` and the equidistant point
        // really does lie inside the slice.
        let narrow = {
            let tv = (15_f32.to_radians() * 0.5).tan();
            tv * tv * 2.0
        };
        let (cz_general, r_general) = sub_frustum_sphere(30.0, 140.0, narrow);
        let expected_general = -0.5 * 170.0 * (1.0 + f64::from(narrow));
        assert!(
            (f64::from(cz_general) - expected_general).abs() < 1.0e-3,
            "centre {cz_general} is not -0.5*(cf+cn)*(1+k2) = {expected_general}"
        );
        assert!(
            (cz_general > -140.0) & (cz_general < -30.0),
            "the equidistant centre {cz_general} must lie inside the slice"
        );
        // The general arm's sphere really does contain the slice's far cap
        // corner — that is the whole claim the closed form makes.
        let corner = ((140.0_f32 * narrow.sqrt()).powi(2) + (140.0 + cz_general).powi(2)).sqrt();
        assert!(
            corner <= r_general + 1.0e-3,
            "far corner {corner} is inside r {r_general}"
        );
        // A WIDE fov takes the far-cap arm, and at a street camera's `k2` (1.39)
        // it takes it for every cascade: the far cap's circumcircle is the whole
        // answer, exactly.
        let (cz_cap, r_cap) = sub_frustum_sphere(2.0, 2.05, k2);
        assert_eq!(cz_cap, -2.05, "the far-cap arm centres on the far cap");
        let expected_cap = (2.05_f64 * f64::from(k2).sqrt() * 16.0).ceil() / 16.0;
        assert_eq!(r_cap, expected_cap as f32, "the far-cap radius is cf*sqrt(k2)");
        assert_eq!(
            sub_frustum_sphere(30.0, 140.0, k2).0,
            -140.0,
            "a street fov takes the far-cap arm even on a long slice"
        );
        // Both radii are exact multiples of 1/16 — the drift stabiliser.
        [r_general, r_cap].into_iter().for_each(|r| {
            assert_eq!(r * 16.0, (r * 16.0).round(), "radius {r} is a 1/16 multiple");
        });
    }

    #[test]
    fn the_snap_lands_the_world_origin_on_a_whole_texel() {
        let set = street_set();
        // After the fit, re-project the world origin through each cascade: its
        // texel coordinate must be a whole number. That is the property the snap
        // exists for, and it holds per cascade, not just on average.
        (0..set.count()).for_each(|i| {
            let m = Mat4::from_cols_array(set.matrices()[i]);
            let o = m.transform_vec4(Vec4::new(0.0, 0.0, 0.0, 1.0));
            let half = f64::from(set.map_size()) * 0.5;
            [f64::from(o.x) * half, f64::from(o.y) * half]
                .into_iter()
                .for_each(|t| {
                    let residual = (t - (t + 0.5).floor()).abs();
                    assert!(
                        residual < 1.0e-3,
                        "cascade {i} origin texel {t} is {residual} off the grid"
                    );
                });
        });
        // JS `Math.round` rounds a half toward +infinity; `f32::round` rounds it
        // away from zero. A projection whose origin sits exactly on a half-texel
        // is where the two disagree, and the transcription must follow JS.
        // A pure translation of -0.5 texels puts the origin at exactly -0.5.
        let half_texel = 1.0 / f64::from(MAP_SIZE);
        let m = Mat4::from_cols_array([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            -(half_texel as f32), 0.0, 0.0, 1.0,
        ]);
        let (dx, _) = texel_snap(m, MAP_SIZE);
        // sx = -0.5 -> floor(-0.5 + 0.5) = 0 -> dx = +0.5/half. `f32::round`
        // would have chosen -1 and produced a dx of -0.5/half.
        assert!(dx > 0.0, "a half-texel must round toward +infinity, got {dx}");
        assert_eq!(dx, half_texel as f32);
    }

    #[test]
    fn the_fit_produces_the_source_uniform_lanes_and_the_sentinels() {
        let set = street_set();
        assert_eq!(set.count(), 4);
        assert_eq!(set.map_size(), MAP_SIZE);
        let s = splits(4, 0.5, 140.0);
        let split = set.split();
        let split_near = set.split_near();
        let texel = set.texel();
        let range = set.range();
        (0..4).for_each(|i| {
            assert_eq!(split[i], s[i + 1], "split lane {i}");
            assert_eq!(split_near[i], s[i], "split-near lane {i}");
            assert_eq!(
                texel[i],
                2.0 * set.radius(i) / MAP_SIZE as f32,
                "texel lane {i} is 2r/mapSize"
            );
            assert_eq!(
                range[i],
                2.0 * set.radius(i) + BACK_DISTANCE,
                "range lane {i} is the ortho depth span"
            );
            assert_eq!(set.cull_margin(i), 32.0 * texel[i], "cull margin {i}");
        });
        // Cascade 0 is metres across, cascade 3 is a couple of hundred: that
        // spread is the whole reason four cascades beat one.
        let (near_texel, far_texel) = (texel[0], texel[3]);
        assert!(near_texel < 0.009, "near texel {near_texel} is millimetres");
        assert!(far_texel > near_texel * 15.0, "far texel {far_texel} is coarse");
        // The one-cascade set fills lanes 1..4 with the source's sentinels.
        let one = fit(1, street_camera(), sun(), MAP_SIZE).unwrap();
        assert_eq!(one.count(), 1);
        assert_eq!(one.split(), [140.0, 1.0e9, 1.0e9, 1.0e9]);
        assert_eq!(one.split_near(), [0.5, 1.0e9, 1.0e9, 1.0e9]);
        assert_eq!(one.texel()[1], 0.01);
        assert_eq!(one.range()[1], 1.0);
        assert_eq!(one.matrices()[1], Mat4::IDENTITY.as_cols_array());
        assert_eq!(one.centre(1), Vec3::ZERO);
        assert_eq!(one.radius(1), 0.0);
        // Index clamping on the accessors (a shader lane is 0..4 by construction).
        assert_eq!(one.centre(99), one.centre(3));
        assert_eq!(one.radius(99), one.radius(3));
        assert_eq!(one.cull_margin(99), one.cull_margin(3));
        // Debug/PartialEq are part of the surface.
        assert!(format!("{set:?}").contains("CascadeSet"));
        assert_eq!(set, street_set());
        assert_ne!(set, one);
    }

    #[test]
    fn every_cascade_covers_the_view_slice_it_owns() {
        let set = street_set();
        let s = splits(4, 0.5, 140.0);
        // A point at the far end of each cascade's own depth slice, on the view
        // axis at ground level, must be inside that cascade's map.
        (0..4).for_each(|i| {
            let depth = s[i + 1] * 0.99;
            let p = Vec3::new(0.0, 0.0, 10.0 - depth);
            let (u, v, d) = project(&set, i, p, Vec3::new(0.0, 1.0, 0.0), 0.8);
            assert!(
                (0.0..=1.0).contains(&u) & (0.0..=1.0).contains(&v) & (0.0..=1.0).contains(&d),
                "cascade {i} does not cover its own far edge: uvd {u} {v} {d}"
            );
        });
        // A 20 m mast beside the road at 40 m still casts — that is what the
        // 140 m back-distance buys.
        let (u, v, d) = project(
            &set,
            3,
            Vec3::new(9.0, 20.0, -30.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.8,
        );
        assert!(
            (0.0..=1.0).contains(&u) & (0.0..=1.0).contains(&v) & (0.0..=1.0).contains(&d),
            "a tall caster is clipped out of cascade 3: uvd {u} {v} {d}"
        );
    }

    #[test]
    fn a_vertical_sun_and_a_degenerate_one_take_their_own_arms() {
        // |y| > 0.98 swaps the up vector to +Z; the fit must still resolve.
        let vertical = fit(4, street_camera(), Vec3::new(0.0, 1.0, 0.0), MAP_SIZE);
        assert!(vertical.is_some(), "a straight-overhead sun must still fit");
        assert_ne!(vertical.unwrap(), street_set());
        // A zero sun cannot be normalised: no fit, and the caller falls back to
        // the unshadowed path.
        assert!(fit(4, street_camera(), Vec3::ZERO, MAP_SIZE).is_none());
        // A zero-extent camera makes every radius zero, and no orthographic
        // projection exists for a zero-width box.
        let flat = CascadeCamera {
            world: Mat4::IDENTITY,
            fovy_radians: 60_f32.to_radians(),
            aspect: 1.0,
            near: 0.0,
            far: 0.0,
        };
        assert!(fit(4, flat, sun(), MAP_SIZE).is_none());
        // Count and map size are clamped rather than trusted.
        assert_eq!(fit(0, street_camera(), sun(), MAP_SIZE).unwrap().count(), 1);
        assert_eq!(fit(9, street_camera(), sun(), MAP_SIZE).unwrap().count(), 4);
        assert_eq!(fit(2, street_camera(), sun(), 0).unwrap().map_size(), 1);
    }

    #[test]
    fn the_atlas_is_four_layers_of_2048_r32f() {
        assert_eq!(atlas_byte_size(MAP_SIZE, MAX_CASCADES), 67_108_864);
        assert_eq!(atlas_byte_size(MAP_SIZE, 1), 16_777_216);
        // The source's own remark, checked: 4 x 4096 really is a quarter of a
        // gigabyte, which is why the map size is clamped to 2048.
        assert_eq!(atlas_byte_size(4096, 4), 268_435_456);
        // Clamped like every other count.
        assert_eq!(atlas_byte_size(MAP_SIZE, 0), atlas_byte_size(MAP_SIZE, 1));
        assert_eq!(atlas_byte_size(MAP_SIZE, 99), atlas_byte_size(MAP_SIZE, 4));
    }

    #[test]
    fn the_quality_tiers_match_the_shader_chunk() {
        assert_eq!(
            quality_tier(3),
            CascadeQuality {
                blocker_taps: 16,
                pcf_taps: 20,
                pcss: true
            }
        );
        assert_eq!(quality_tier(9), quality_tier(3), "the tier saturates");
        assert_eq!(
            quality_tier(2),
            CascadeQuality {
                blocker_taps: 12,
                pcf_taps: 14,
                pcss: true
            }
        );
        assert_eq!(
            quality_tier(0),
            CascadeQuality {
                blocker_taps: 8,
                pcf_taps: 8,
                pcss: false
            }
        );
        assert_eq!(quality_tier(1), quality_tier(0));
        assert!(format!("{:?}", quality_tier(3)).contains("CascadeQuality"));
        assert!(format!("{:?}", CascadeParams::default()).contains("CascadeParams"));
        assert_eq!(CascadeParams::default().max_filter_texels, 9.0);
        assert_eq!(CascadeParams::default().softness, 0.022);
        assert_eq!(CascadeParams::default().strength, 1.0);
        assert_eq!(CascadeParams::default().rotation, 0.0);
        assert_ne!(
            CascadeParams::default(),
            CascadeParams {
                strength: 0.5,
                ..CascadeParams::default()
            }
        );
    }








    /// **The shadow path takes the snap from this module and nothing else yet.**
    ///
    /// This replaces `nothing_in_the_shadow_path_compiles_this_yet`, which
    /// asserted the whole module was unreachable from the shipped shadow path
    /// and fired — correctly — the moment [`snap_view_proj`] was wired into
    /// `scene_renderer`. Its demand was that any wiring come with an argument
    /// about what the one-cascade configuration now renders, so here it is.
    ///
    /// **It is deliberately NOT byte-identical.** The snap translates the light
    /// matrix by up to half a texel, so every shadowed pixel may move by that
    /// much — which is the entire point: an unsnapped grid slides under the
    /// world continuously as the camera moves, and half a texel *once* is the
    /// price of it never sliding again. The map is still one cascade, still fit
    /// by `axiom_render_pipeline::shadow_view`, still filtered by
    /// `scene_wgsl.rs`'s 5x5 comparison PCF.
    ///
    /// So the guard is narrowed rather than dropped: the shadow path may name
    /// the snap, and naming anything that *selects* a cascade means the pass has
    /// genuinely become cascaded and this test has to be replaced again.
    #[test]
    fn the_shadow_path_takes_the_snap_and_no_cascade_selection() {
        [
            ("scene_wgsl.rs", include_str!("scene_wgsl.rs")),
            ("scene_renderer.rs", include_str!("scene_renderer.rs")),
            ("shadow_cull.rs", include_str!("shadow_cull.rs")),
        ]
        .iter()
        .for_each(|(name, source)| {
            ["CascadeSet", "cascade_shadow", "select_cascade", "cascade::fit"]
                .iter()
                .for_each(|symbol| {
                    assert!(
                        !source.contains(symbol),
                        "{name} references `{symbol}`: the shadow pass is now \
                         cascaded, so this test must be replaced by one proving \
                         what the multi-cascade configuration renders"
                    );
                });
            // Every `cascade::` the shadow path names must be the snap.
            source.match_indices("cascade::").for_each(|(at, _)| {
                assert!(
                    source[at..].starts_with("cascade::snap_view_proj"),
                    "{name} names a cascade item other than the snap at byte {at}"
                );
            });
        });
    }

}

/// **Real-adapter proof.** The CPU reference above says where a shadow lands;
/// this renders the cascades on a real GPU with the reference's own matrices and
/// checks that it does.
///
/// Two claims, in order:
///
/// 1. **The map holds the caster where the reference projects it.** A receiver
///    the reference marks shadowed must project into a texel of the rendered
///    atlas that holds a *nearer* depth than the receiver's, and a receiver it
///    marks lit must project into an empty one. That is the projection, the
///    split scheme and the snap all being right at once, against real
///    rasterisation.
/// 2. **The WGSL means what the reference means.** The fragment stage —
///    selection, cross-fade, fade-out, PCSS, Vogel PCF — runs on the adapter over
///    the same atlas and is compared to [`sun_shadow`] probe for probe. The
///    tolerance is *measured*, then asserted, so the justification cannot rot.
///
/// Compiled only with a real GPU available, and it **asserts** an adapter rather
/// than skipping: a parity test that silently passes when nothing ran is worse
/// than no parity test.
#[cfg(all(test, feature = "offscreen"))]
mod adapter_proof;
