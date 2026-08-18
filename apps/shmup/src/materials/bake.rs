//! Ported from Claude-of-Duty `src/materials/generator.js:1-393` — the
//! `TextureForge`'s bake pipeline, minus the WebGL/Three.js plumbing it uses
//! to drive it (render targets, shader materials, framebuffer binds). See the
//! module doc below for why this is a **CPU bake**, and
//! [`crate::materials::noise`] for the tileable noise library every
//! `owSurface` implementation here is built from.
//!
//! ## The bake contract (`generator.js:14-21`), reproduced exactly
//!
//! Each surface is one function `owSurface(uv) -> (albedo, height, roughness,
//! metal, ao)` — see [`SurfaceSample`] and the [`SurfaceFn`] alias. Baking one
//! surface produces up to three textures ([`bake`] → [`BakedSet`]):
//!
//! ```text
//! albedo.rgb = base colour (sRGB)   albedo.a = height (or an alpha-test mask)
//! orm.r = AO/cavity   orm.g = roughness   orm.b = metalness
//! normal.rgb = tangent-space, OpenGL +Y — a Sobel filter over the height field
//! ```
//!
//! The source drives this with four full-screen GPU draws per surface (one
//! into a scratch half-float height target, three into the real 8-bit output
//! targets) so nothing is read back to the CPU. That plumbing — `THREE.
//! WebGLRenderTarget`, an orthographic full-screen triangle, `ShaderMaterial`
//! uniforms — has no CPU analogue and is not ported: this module reproduces
//! the *bake*, not the renderer driving it. `axiom-proc-texture` (the engine's
//! own procedural texture layer) already bakes on the CPU, and a CPU bake is
//! testable without a browser, which is the point of doing it this way now.
//! A future GPU path (WGSL emission of `owSurface`, real render targets) would
//! need: a full-screen-triangle draw per output channel, a half-float scratch
//! target for height (see the doc on [`bake_height`] for why 8 bits isn't
//! enough), and the same Sobel kernel as a fragment shader — this module's
//! [`sobel`] is line-for-line portable to that shader.
//!
//! ## Height precision
//!
//! The source bakes height into a **half-float** scratch render target
//! specifically because an 8-bit height field stair-steps the Sobel
//! (`generator.js:180-181`, `_heightRT`). This port keeps that guarantee by
//! using `f32` for the height buffer ([`bake_height`]) — never rounding it to
//! an 8-bit texel — even though the final *albedo alpha channel* (which also
//! carries height, per the contract above) is nominally 8-bit-range `[0,1]`
//! `f32` here too, since this port never actually quantizes to `u8` at all
//! (there is no display path yet to quantize for).
//!
//! ## Two shared maps, ported here: detail and macro
//!
//! [`detail_surface`] (`DETAIL_SRC`, `generator.js:91-120`) and
//! [`macro_surface`] (`MACRO_SRC`, `generator.js:126-138`) are the two
//! generator bodies the source actually defines inline in this file (every
//! per-material `owSurface` — concrete, brick, … — lives in sibling
//! `glsl/surfaces-*.js` files, out of scope for this port). [`build_detail`]
//! and [`build_macro`] reproduce `TextureForge.buildDetail`/`buildMacro`
//! (`generator.js:344-381`) with their exact default sizes/seeds/world
//! scales.

use super::noise::{
    gl_clamp, gl_smoothstep, ow_fbm01, ow_scratches, ow_warp, ow_worley, Vec2, Vec3,
};

// ---------------------------------------------------------------------------
// The `owSurface` contract.
// ---------------------------------------------------------------------------

/// One `owSurface(uv)` evaluation — the GLSL out-parameters `(alb, h, rough,
/// metal, ao)` (`generator.js:15-21`) as a return value. `albedo` is
/// **linear-space**: the sRGB encode described in the module doc happens once,
/// in [`bake`], not inside a surface function (matching the source, where the
/// hardware encodes on write to an `SRGBColorSpace` render target — the
/// surface shader itself only ever writes linear numbers).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSample {
    pub albedo: Vec3,
    pub height: f64,
    pub roughness: f64,
    pub metal: f64,
    pub ao: f64,
}

impl SurfaceSample {
    /// The `FOOTER`'s defaults (`generator.js:44-45`):
    /// `vec3 alb = vec3(0.5); float h = 0.5, rough = 0.5, metal = 0.0, ao =
    /// 1.0;`. No surface in this port actually relies on these — every
    /// `owSurface` body always assigns every out-parameter, exactly like the
    /// source's two inline generators — but the constant is named here rather
    /// than left implicit, matching the FOOTER's own explicit initialisation.
    pub const FOOTER_DEFAULT: SurfaceSample = SurfaceSample {
        albedo: Vec3 {
            x: 0.5,
            y: 0.5,
            z: 0.5,
        },
        height: 0.5,
        roughness: 0.5,
        metal: 0.0,
        ao: 1.0,
    };
}

/// `owSurface(vec2 uv) -> (vec3 alb, float h, float rough, float metal, float
/// ao)`. A plain `Fn` rather than a trait: the only two implementations that
/// exist in this port ([`detail_surface`], [`macro_surface`]) close over their
/// `uSeed` uniform, exactly as `generator.js`'s per-material GLSL bodies close
/// over the `uSeed`/`uTintA`/`uTintB`/`uParam` uniforms declared in `HEADER`
/// (`generator.js:32-40`) — a future per-material generator does the same.
pub type SurfaceFn<'a> = dyn Fn(Vec2) -> SurfaceSample + 'a;

// ---------------------------------------------------------------------------
// Texel buffer.
// ---------------------------------------------------------------------------

/// A square RGBA texel buffer — the CPU stand-in for a `THREE.
/// WebGLRenderTarget`'s backing store. Every channel stays `f32` in `[0,1]`;
/// nothing in this port ever quantizes to 8-bit, since there is no display
/// path yet that would need it to.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub size: u32,
    /// Row-major, `size * size` texels. Row `y`, column `x` is `texels[y *
    /// size + x]`; `v = (y + 0.5) / size` increases with `y`, matching this
    /// module's [`texel_uv`] convention.
    pub texels: Vec<[f32; 4]>,
}

impl Texture {
    fn new(size: u32) -> Self {
        Texture {
            size,
            texels: vec![[0.0; 4]; (size as usize) * (size as usize)],
        }
    }

    fn set(&mut self, x: u32, y: u32, texel: [f32; 4]) {
        let idx = (y * self.size + x) as usize;
        self.texels[idx] = texel;
    }

    pub fn get(&self, x: u32, y: u32) -> [f32; 4] {
        self.texels[(y * self.size + x) as usize]
    }

    /// Wrapped read, one channel — `RepeatWrapping` (`generator.js:190-191`):
    /// an out-of-range texel index wraps around the tile rather than clamping
    /// or reading garbage. Used only by [`sobel`], which needs the height
    /// buffer's neighbours at the tile's edge.
    fn wrapped_r(&self, x: i64, y: i64) -> f64 {
        let size = i64::from(self.size);
        let xi = x.rem_euclid(size) as u32;
        let yi = y.rem_euclid(size) as u32;
        f64::from(self.get(xi, yi)[0])
    }
}

/// The fragment-center UV of texel `(x, y)` in a `size x size` tile — what a
/// full-screen-triangle draw evaluates at, and the source's `vUv` varying.
fn texel_uv(x: u32, y: u32, size: u32) -> Vec2 {
    Vec2::new(
        (f64::from(x) + 0.5) / f64::from(size),
        (f64::from(y) + 0.5) / f64::from(size),
    )
}

// ---------------------------------------------------------------------------
// sRGB encode — the hardware's write-side counterpart to
// `noise::ow_srgb`'s decode.
// ---------------------------------------------------------------------------

/// Linear -> sRGB encode, applied to `albedo` when `linear_albedo` is
/// `false` (`generator.js:276`, `albedoRT = this._target(size, { srgb:
/// def.linearAlbedo !== true })`). WebGL performs this in hardware on write to
/// an `SRGBColorSpace` 8-bit render target; there is no GLSL source for it to
/// transcribe (the shader itself only ever writes linear values), so this is
/// the standard IEC 61966-2-1 encode — the algebraic inverse of
/// [`super::noise::ow_srgb`]'s decode. This module's tests pin it directly
/// against that existing, already-golden-pinned decode (a round trip) rather
/// than against a fresh JS capture.
fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn linear_to_srgb3(c: Vec3) -> Vec3 {
    Vec3::new(
        linear_to_srgb(c.x),
        linear_to_srgb(c.y),
        linear_to_srgb(c.z),
    )
}

// ---------------------------------------------------------------------------
// The bake itself — `TextureForge.build` (`generator.js:260-321`).
// ---------------------------------------------------------------------------

/// One `TextureForge.build(def)` call's worth of parameters
/// (`generator.js:240-259`). Fields not needed by a CPU bake (`key`, the
/// program cache) are dropped; `tint_a`/`tint_b`/`param` are omitted too —
/// nothing in this port's two surfaces ([`detail_surface`],
/// [`macro_surface`]) reads them (`DETAIL_SRC`/`MACRO_SRC` never reference
/// `uTintA`/`uTintB`/`uParam`), so [`SurfaceFn`] closures take only `uv`
/// rather than carrying unused uniform plumbing.
pub struct BakeDef<'a> {
    pub surface: &'a SurfaceFn<'a>,
    pub size: u32,
    /// Metres the tile spans — drives the Sobel's normal slope.
    pub world_size: f32,
    /// Peak-to-trough height relief, in metres.
    pub relief: f32,
    /// `def.linearAlbedo === true`: skip the sRGB encode (the map is data,
    /// not colour).
    pub linear_albedo: bool,
    /// `def.orm !== false`.
    pub want_orm: bool,
    /// `def.normal !== false`.
    pub want_normal: bool,
}

/// A built texture set — `TextureForge.build`'s return value
/// (`generator.js:313-320`), minus the `THREE.Texture` wrapper (this port
/// returns the pixel buffers themselves).
pub struct BakedSet {
    /// `rgb` = albedo (sRGB-encoded unless `linear_albedo`), `a` = height.
    pub albedo: Texture,
    /// `r` = AO, `g` = roughness, `b` = metalness, `a` = 1. `None` when
    /// `want_orm` was `false`.
    pub orm: Option<Texture>,
    /// Tangent-space normal `* 0.5 + 0.5`, `a` = 1. `None` when `want_normal`
    /// was `false`.
    pub normal: Option<Texture>,
    pub size: u32,
    pub world_size: f32,
    pub relief: f32,
}

/// `bake_height`: the scratch height pass (`generator.js:280-287`), skipped
/// entirely when nothing needs the Sobel — "the height pass exists only to
/// feed the Sobel, so it is skipped with it" (`generator.js:280`). See the
/// module doc for why this stays `f32` rather than dropping to an 8-bit texel.
fn bake_height(surface: &SurfaceFn, size: u32) -> Texture {
    let mut height = Texture::new(size);
    for y in 0..size {
        for x in 0..size {
            let h = surface(texel_uv(x, y, size)).height as f32;
            height.set(x, y, [h, h, h, 1.0]);
        }
    }
    height
}

/// `SOBEL` (`generator.js:53-78`): a 3x3 Sobel kernel over the height field,
/// converted from a per-texel slope to a per-tile slope
/// (`strength = relief / worldSize`, `generator.js:305`) so the resulting
/// normal map is physically consistent with the metres-per-tile mapping.
/// `RepeatWrapping` neighbours ([`Texture::wrapped_r`]) at the tile edge,
/// matching the render target's wrap mode (`generator.js:190-191`).
fn sobel(height: &Texture, size: u32, relief: f32, world_size: f32) -> Texture {
    let strength = f64::from(relief) / f64::from(world_size);
    let mut normal = Texture::new(size);
    for y in 0..size {
        for x in 0..size {
            let (xi, yi) = (i64::from(x), i64::from(y));
            let tl = height.wrapped_r(xi - 1, yi + 1);
            let t = height.wrapped_r(xi, yi + 1);
            let tr = height.wrapped_r(xi + 1, yi + 1);
            let l = height.wrapped_r(xi - 1, yi);
            let r = height.wrapped_r(xi + 1, yi);
            let bl = height.wrapped_r(xi - 1, yi - 1);
            let b = height.wrapped_r(xi, yi - 1);
            let br = height.wrapped_r(xi + 1, yi - 1);

            let dx = ((tr + 2.0 * r + br) - (tl + 2.0 * l + bl)) * 0.125;
            let dy = ((tl + 2.0 * t + tr) - (bl + 2.0 * b + br)) * 0.125;

            // dx/dy are per-texel; `/ uTexel` = `* size` converts to a slope
            // over the whole tile.
            let sx = dx * f64::from(size);
            let sy = dy * f64::from(size);

            let (nx, ny, nz) = normalize3(-sx * strength, -sy * strength, 1.0);
            normal.set(
                x,
                y,
                [
                    (nx * 0.5 + 0.5) as f32,
                    (ny * 0.5 + 0.5) as f32,
                    (nz * 0.5 + 0.5) as f32,
                    1.0,
                ],
            );
        }
    }
    normal
}

/// `normalize(vec3)`. Not guarded against a zero-length input: `z` is always
/// `1.0` here (`SOBEL`'s `vec3(-sx*strength, -sy*strength, 1.0)`), so the
/// length is always `>= 1.0` and never zero — the same reasoning
/// [`super::noise::Vec2::normalize`]'s doc gives for its call site.
fn normalize3(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let len = (x * x + y * y + z * z).sqrt();
    (x / len, y / len, z / len)
}

/// `TextureForge.build` (`generator.js:260-321`), minus the render-target
/// bookkeeping (`prevTarget`/`autoClear` save-restore, the shared program
/// cache) that has no meaning outside a live `WebGLRenderer`.
pub fn bake(def: &BakeDef) -> BakedSet {
    let size = def.size;

    // "The height pass exists only to feed the Sobel, so it is skipped with
    // it" (generator.js:280).
    let height = def.want_normal.then(|| bake_height(def.surface, size));

    let mut albedo = Texture::new(size);
    for y in 0..size {
        for x in 0..size {
            let sample = (def.surface)(texel_uv(x, y, size));
            let rgb = if def.linear_albedo {
                sample.albedo
            } else {
                linear_to_srgb3(sample.albedo)
            };
            albedo.set(
                x,
                y,
                [rgb.x as f32, rgb.y as f32, rgb.z as f32, sample.height as f32],
            );
        }
    }

    let orm = def.want_orm.then(|| {
        let mut t = Texture::new(size);
        for y in 0..size {
            for x in 0..size {
                let sample = (def.surface)(texel_uv(x, y, size));
                t.set(
                    x,
                    y,
                    [sample.ao as f32, sample.roughness as f32, sample.metal as f32, 1.0],
                );
            }
        }
        t
    });

    let normal = height
        .as_ref()
        .map(|h| sobel(h, size, def.relief, def.world_size));

    BakedSet {
        albedo,
        orm,
        normal,
        size,
        world_size: def.world_size,
        relief: def.relief,
    }
}

// ---------------------------------------------------------------------------
// DETAIL_SRC — `generator.js:80-120`.
// ---------------------------------------------------------------------------

/// `owFbm01`/`owWorley`/`owScratches` at NYQUIST-respecting frequencies (see
/// the source's `DETAIL_SRC` doc comment, `generator.js:80-90`), then combined
/// into pores/grit/scratches. `DETAIL_SRC`, `generator.js:91-120`.
pub fn detail_surface(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(8.0);
    let p = uv.mul(p_const).add_scalar(seed);

    // ~10 mm swell, ~3.5 mm tooth.
    let a = ow_fbm01(p.scale(3.0), p_const.scale(3.0), 4, 0.55);
    let b = ow_fbm01(p.scale(9.0), p_const.scale(9.0), 4, 0.52);
    // 3.9 mm pits and 1.6 mm grains.
    let pores = ow_worley(p.scale(8.0), p_const.scale(8.0), 1.0);
    let grit = ow_worley(p.scale(20.0), p_const.scale(20.0), 1.0);
    let scr = ow_scratches(p.scale(2.5), p_const.scale(2.5), 16.0, 1.0, 0.66)
        + ow_scratches(
            p.scale(4.0).add_scalar(5.0),
            p_const.scale(4.0),
            11.0,
            -2.0,
            0.70,
        ) * 0.8;

    // Proud grains: a solid, rounded bump rather than a threshold speck.
    let grit_a = gl_smoothstep(0.34, 0.08, pores.f1) * gl_step(0.38, pores.id_x);
    let grit_b = gl_smoothstep(0.30, 0.06, grit.f1) * gl_step(0.34, grit.id_x);
    let pit = gl_smoothstep(0.26, 0.0, pores.f1) * gl_step(0.72, pores.id_y);

    let mut h = 0.5 + (a - 0.5) * 0.34 + (b - 0.5) * 0.26;
    h -= pit * 0.38;
    h += grit_a * 0.26 * (0.5 + grit.id_x) + grit_b * 0.20;
    h -= gl_clamp(scr, 0.0, 1.0) * 0.18;

    // Albedo tracks the grain so a proud grain reads light and its trough
    // reads dark; the material shader scales this by the per-surface detail
    // albedo amount.
    let albedo_v =
        0.5 + (a - 0.5) * 0.22 + (b - 0.5) * 0.15 + grit_a * 0.16 + grit_b * 0.10 - pit * 0.14;

    SurfaceSample {
        albedo: Vec3::splat(albedo_v),
        height: gl_clamp(h, 0.0, 1.0),
        roughness: 0.5 + (b - 0.5) * 0.5,
        metal: 0.0,
        ao: 1.0 - pit * 0.45 - grit_b * 0.10,
    }
}

/// GLSL `step(edge, x)`: `1.0` when `x >= edge`, else `0.0`. Not one of
/// `noise.js`'s functions (it's a bare GLSL builtin `generator.js` calls
/// directly), so it lives here rather than in [`super::noise`], which mirrors
/// `noise.js` function-for-function.
fn gl_step(edge: f64, x: f64) -> f64 {
    if x < edge {
        0.0
    } else {
        1.0
    }
}

/// `TextureForge.buildDetail(size = 1024, seed = 1)` (`generator.js:345-363`):
/// the shared micro-detail normal + matching micro albedo/roughness. `orm:
/// false` — "the ORM output was never bound anywhere" — and `linearAlbedo:
/// true` — "the detail map is DATA, not colour" — are the source's own
/// documented reasons, reproduced here as the fixed `BakeDef`.
pub fn build_detail(size: u32, seed: f64) -> BakedSet {
    let surface = move |uv: Vec2| detail_surface(uv, seed);
    bake(&BakeDef {
        surface: &surface,
        size,
        // 1.6 mm grain standing ~0.4 mm proud: a real tooth, not a bump-map
        // hint.
        world_size: 0.25,
        relief: 0.0034,
        linear_albedo: true,
        want_orm: false,
        want_normal: true,
    })
}

// ---------------------------------------------------------------------------
// MACRO_SRC — `generator.js:122-138`.
// ---------------------------------------------------------------------------

/// Four bands of low-frequency variation packed into RGBA, used by every
/// material to break up tiling. `MACRO_SRC`, `generator.js:126-138`.
pub fn macro_surface(uv: Vec2, seed: f64) -> SurfaceSample {
    let p_const = Vec2::splat(6.0);
    let p = uv.mul(p_const).add_scalar(seed * 3.0);

    let a = ow_fbm01(p.scale(0.5), p_const.scale(0.5), 4, 0.62);
    // `p * 1.0` in the source is a no-op scale, kept only to mirror the line;
    // `p` alone is the same value.
    let warped = ow_warp(p, p_const, 1.1, 3);
    let b = ow_fbm01(warped, p_const, 4, 0.58);
    let c = ow_fbm01(p.scale(2.5), p_const.scale(2.5), 4, 0.55);
    let d = ow_fbm01(p.scale(7.0), p_const.scale(7.0), 4, 0.5);

    SurfaceSample {
        albedo: Vec3::new(a, b, c),
        height: d,
        roughness: 0.5,
        metal: 0.0,
        ao: 1.0,
    }
}

/// `TextureForge.buildMacro(size = 256, seed = 2)` (`generator.js:365-381`):
/// the shared 4-band low-frequency variation map. `orm: false, normal: false`
/// — "the macro ORM and macro normal were baked and then never sampled."
pub fn build_macro(size: u32, seed: f64) -> BakedSet {
    let surface = move |uv: Vec2| macro_surface(uv, seed);
    bake(&BakeDef {
        surface: &surface,
        size,
        world_size: 32.0,
        relief: 0.5,
        linear_albedo: true,
        want_orm: false,
        want_normal: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Sobel, pinned against a known height field — no JS capture needed,
    // since a Sobel kernel over an analytic field has a closed-form answer.
    // -----------------------------------------------------------------

    /// A pure ramp `h(x, y) = x / size` has a constant, exactly known
    /// gradient: `dh/dx` is `1/size` per texel everywhere except the wrapped
    /// seam (where `RepeatWrapping` makes the ramp discontinuous — see the
    /// second assertion), and `dh/dy = 0` everywhere. This pins [`sobel`]
    /// against hand-derived numbers rather than a fresh capture.
    #[test]
    fn sobel_matches_a_hand_derived_gradient_on_a_ramp() {
        let size = 8u32;
        let mut height = Texture::new(size);
        for y in 0..size {
            for x in 0..size {
                let h = f64::from(x) / f64::from(size);
                height.set(x, y, [h as f32, 0.0, 0.0, 1.0]);
            }
        }
        // relief/world_size = 1.0, so `strength` is 1 and the slope reads
        // straight off the Sobel without a scale factor muddying the check.
        let normal = sobel(&height, size, 1.0, 1.0);

        // Interior texels (x = 2..=5): every neighbour used by the Sobel
        // stencil (x-1..=x+1) is a plain, unwrapped ramp sample, so
        // dx = ((h(x+1) - h(x-1)) * (1+2+1)) * 0.125 = (h(x+1)-h(x-1))*0.5
        // = (2/size) * 0.5 = 1/size, and the tile-slope sx = dx * size = 1.
        // normal = normalize(-1, 0, 1) = (-1, 0, 1) / sqrt(2).
        let expected_x = -1.0 / std::f64::consts::SQRT_2 * 0.5 + 0.5;
        let expected_z = 1.0 / std::f64::consts::SQRT_2 * 0.5 + 0.5;
        for x in 2..=5u32 {
            let n = normal.get(x, 4);
            assert!(
                (f64::from(n[0]) - expected_x).abs() < 1e-6,
                "x={x}: nx={}, want {expected_x}",
                n[0]
            );
            assert!(
                (f64::from(n[1]) - 0.5).abs() < 1e-6,
                "x={x}: ny={}, want 0.5 (flat in y)",
                n[1]
            );
            assert!(
                (f64::from(n[2]) - expected_z).abs() < 1e-6,
                "x={x}: nz={}, want {expected_z}",
                n[2]
            );
        }

        // At the wrapped seam (x = 0), the left neighbour wraps to x = 7
        // (h = 7/8), so the ramp reads as a steep *downward* step rather
        // than the smooth +1/size slope the interior sees — a genuine
        // consequence of `RepeatWrapping` on a non-tileable field, not a
        // bug. dx = ((h(1) - h(7)) * 0.5) = ((1/8 - 7/8) * 0.5) = -0.375,
        // sx = dx * 8 = -3.
        let seam = normal.get(0, 4);
        let strength = 1.0;
        let (enx, _eny, enz) = normalize3(-(-3.0) * strength, 0.0, 1.0);
        assert!((f64::from(seam[0]) - (enx * 0.5 + 0.5)).abs() < 1e-6);
        assert!((f64::from(seam[2]) - (enz * 0.5 + 0.5)).abs() < 1e-6);
    }

    /// A flat field has zero gradient everywhere: the Sobel must report a
    /// perfectly straight-up normal regardless of `relief`/`world_size`.
    #[test]
    fn sobel_of_a_flat_field_is_straight_up() {
        let size = 4u32;
        let mut height = Texture::new(size);
        for y in 0..size {
            for x in 0..size {
                height.set(x, y, [0.37, 0.0, 0.0, 1.0]);
            }
        }
        let normal = sobel(&height, size, 0.09, 2.5);
        for y in 0..size {
            for x in 0..size {
                let n = normal.get(x, y);
                assert_eq!(n, [0.5, 0.5, 1.0, 1.0]);
            }
        }
    }

    // -----------------------------------------------------------------
    // sRGB encode.
    // -----------------------------------------------------------------

    #[test]
    fn linear_to_srgb_matches_the_standard_iec_constants() {
        assert_eq!(linear_to_srgb(0.0), 0.0);
        // `1.0.powf(1.0 / 2.4)` is not bit-exact 1.0 through libm, so this
        // needs the same transcendental tolerance as everything else built
        // from `powf`.
        assert!((linear_to_srgb(1.0) - 1.0).abs() < 1e-12);
        // A well-known reference point: linear 0.5 encodes to ~0.7354 sRGB.
        assert!((linear_to_srgb(0.5) - 0.735_356_983).abs() < 1e-8);
        // Below the linear toe, the encode is the plain `* 12.92` segment.
        assert!((linear_to_srgb(0.001) - 0.001 * 12.92).abs() < 1e-12);
    }

    #[test]
    fn linear_to_srgb_round_trips_through_ow_srgb_decode() {
        use super::super::noise::ow_srgb;
        for c in [0.0, 0.02, 0.13, 0.5, 0.7, 0.9999] {
            let encoded = linear_to_srgb(c);
            let decoded = ow_srgb(Vec3::splat(encoded)).x;
            assert!(
                (decoded - c).abs() < 1e-9,
                "round trip failed for {c}: encoded {encoded}, decoded back to {decoded}"
            );
        }
    }

    // -----------------------------------------------------------------
    // detail_surface / macro_surface, pinned against a Node capture of
    // generator.js's DETAIL_SRC/MACRO_SRC transcribed to plain JS doubles
    // (same discipline as tests/materials_noise_port.rs).
    // -----------------------------------------------------------------

    fn assert_close(actual: f64, expected: f64, at: &str) {
        assert_close_tol(actual, expected, 1e-9, at);
    }

    fn assert_close_tol(actual: f64, expected: f64, tol: f64, at: &str) {
        assert!(
            (actual - expected).abs() < tol,
            "{at}: expected {expected:.17}, got {actual:.17}"
        );
    }

    fn pts() -> [Vec2; 5] {
        [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.13, 0.77),
            Vec2::new(0.42, 0.09),
            Vec2::new(0.91, 0.36),
            Vec2::new(1.0, 1.0),
        ]
    }

    #[test]
    fn detail_surface_matches_the_javascript_samples() {
        // (alb, h, rough, ao) — metal is always 0.0 for DETAIL_SRC.
        let expected = [
            (0.5, 0.5, 0.5, 1.0),
            (0.5054909043081465, 0.5076513889961605, 0.485193396321412, 1.0),
            (0.5366838562430196, 0.5773348092256236, 0.5090969423000731, 1.0),
            (0.5585757088168839, 0.5459395087145156, 0.4321876941325852, 1.0),
            (0.5, 0.5, 0.5, 1.0),
        ];
        for (uv, (alb, h, rough, ao)) in pts().into_iter().zip(expected) {
            let s = detail_surface(uv, 1.0);
            assert_close(s.albedo.x, alb, "albedo.x");
            assert_close(s.albedo.y, alb, "albedo.y");
            assert_close(s.albedo.z, alb, "albedo.z");
            assert_close(s.height, h, "height");
            assert_close(s.roughness, rough, "roughness");
            assert_eq!(s.metal, 0.0);
            assert_close(s.ao, ao, "ao");
        }
    }

    #[test]
    fn macro_surface_matches_the_javascript_samples() {
        let expected = [
            ((0.5, 0.5658089361511092, 0.5), 0.5),
            ((0.5042119725863053, 0.5511857934766511, 0.4891005124566141), 0.45603667416882926),
            ((0.47860131696686503, 0.41558531969695195, 0.37541844323580187), 0.5206372714576722),
            ((0.5147087533869802, 0.5519017766634716, 0.38710922603323084), 0.6456236216453026),
            ((0.5, 0.56580893615111, 0.5), 0.5),
        ];
        for (uv, ((ar, ag, ab), h)) in pts().into_iter().zip(expected) {
            let s = macro_surface(uv, 2.0);
            assert_close(s.albedo.x, ar, "albedo.x");
            assert_close(s.albedo.y, ag, "albedo.y");
            assert_close(s.albedo.z, ab, "albedo.z");
            assert_close(s.height, h, "height");
            assert_eq!(s.roughness, 0.5);
            assert_eq!(s.metal, 0.0);
            assert_eq!(s.ao, 1.0);
        }
    }

    // -----------------------------------------------------------------
    // Full pipeline, end-to-end: a 6x6 `build_detail` tile (albedo.rgba +
    // the Sobel-derived normal), pinned against the same JS capture script
    // run through the full pipeline (uv-per-texel + Sobel), not just the
    // bare `owSurface` samples above. This is what proves [`bake`]'s texel
    // addressing/wrapping match the source, not just the maths inside one
    // `owSurface` call.
    // -----------------------------------------------------------------

    #[test]
    fn build_detail_tile_matches_the_javascript_capture() {
        #[rustfmt::skip]
        let expected_albedo: [[f64; 4]; 36] = [
            [0.49999999999999983,0.49999999999999983,0.49999999999999983,0.4999999999999998],
            [0.5877413736315008,0.5877413736315008,0.5877413736315008,0.6754827472630016],
            [0.6599999999999999,0.6599999999999999,0.6599999999999999,0.8160081061130586],
            [0.5,0.5,0.5,0.5],
            [0.4999999999999997,0.4999999999999997,0.4999999999999997,0.4999999999999995],
            [0.5630190155465602,0.5630190155465602,0.5630190155465602,0.6260380310931197],
            [0.5798365535309268,0.5798365535309268,0.5798365535309268,0.615956853957971],
            [0.5,0.5,0.5,0.5],
            [0.6389495845629549,0.6389495845629549,0.6389495845629549,0.6210544502486226],
            [0.5,0.5,0.5,0.5],
            [0.5179239960454961,0.5179239960454961,0.5179239960454961,0.5358479920909922],
            [0.5448830605301622,0.5448830605301622,0.5448830605301622,0.5781123410948737],
            [0.5000000000000007,0.5000000000000007,0.5000000000000007,0.5000000000000011],
            [0.4999999999999997,0.4999999999999997,0.4999999999999997,0.49999999999999944],
            [0.49999999999999806,0.49999999999999806,0.49999999999999806,0.4999999999999967],
            [0.5087332013940892,0.5087332013940892,0.5087332013940892,0.5103156158608968],
            [0.5,0.5,0.5,0.5000000000000001],
            [0.5341509147068138,0.5341509147068138,0.5341509147068138,0.5403345248016805],
            [0.5756300841825093,0.5756300841825093,0.5756300841825093,0.6130813535788208],
            [0.5614782779168965,0.5614782779168965,0.5614782779168965,0.6229565558337932],
            [0.5000000000000002,0.5000000000000002,0.5000000000000002,0.5000000000000004],
            [0.5019308157349076,0.5019308157349076,0.5019308157349076,0.5017879472101591],
            [0.5,0.5,0.5,0.5],
            [0.500000000000002,0.500000000000002,0.500000000000002,0.5000000000000036],
            [0.4999999999999998,0.4999999999999998,0.4999999999999998,0.49999999999999956],
            [0.524306919976384,0.524306919976384,0.524306919976384,0.5310479158716214],
            [0.5175298152094135,0.5175298152094135,0.5175298152094135,0.5350596304188272],
            [0.5,0.5,0.5,0.5],
            [0.5483770209607913,0.5483770209607913,0.5483770209607913,0.5589304278169799],
            [0.49999999999999944,0.49999999999999944,0.49999999999999944,0.49999999999999906],
            [0.5000000000000006,0.5000000000000006,0.5000000000000006,0.500000000000001],
            [0.4999999999999981,0.4999999999999981,0.4999999999999981,0.49999999999999695],
            [0.5764357311046249,0.5764357311046249,0.5764357311046249,0.6135846837611086],
            [0.5000000000000004,0.5000000000000004,0.5000000000000004,0.5000000000000008],
            [0.4969993715667693,0.4969993715667693,0.4969993715667693,0.4918554371098039],
            [0.5025721817858038,0.5025721817858038,0.5025721817858038,0.5040335192062493],
        ];
        #[rustfmt::skip]
        let expected_normal: [[f64; 3]; 36] = [
            [0.4999146081997657,0.49843944572173915,0.9999975573726186],
            [0.4961715529555198,0.49937054318729096,0.9999849465507422],
            [0.5017899125321942,0.4999238088709169,0.9999967903977374],
            [0.5042784975383877,0.4997375518057081,0.9999816252421276],
            [0.49829548042885974,0.4991734798795958,0.9999964114646447],
            [0.4995499099387535,0.4984286624274448,0.9999973283100319],
            [0.500750280395963,0.5013320436664166,0.9999976627335354],
            [0.4983364108821689,0.5034014678218917,0.9999856622823335],
            [0.5008423233291188,0.5040654948852276,0.9999827619455968],
            [0.502480705422718,0.5015063966816029,0.9999915767986931],
            [0.49840737227686166,0.5003844761770919,0.9999973157077993],
            [0.49918289194891013,0.5008741732608016,0.9999985681534928],
            [0.5001827066310852,0.49980062463786035,0.9999999268677466],
            [0.5005507164018342,0.499377886737605,0.9999993096860569],
            [0.5005127399851971,0.5005985576833939,0.9999993788260214],
            [0.5004345522410548,0.500781964142768,0.9999991996957888],
            [0.4993045541853321,0.5007549023375221,0.9999989464764698],
            [0.49901473376200783,0.5009942317948524,0.9999980407497397],
            [0.49879320835648056,0.5000473615673806,0.9999985414086836],
            [0.5009746233612248,0.49950450832760185,0.9999988045958774],
            [0.5013416491319224,0.4995366589066779,0.999997985288579],
            [0.499878258970959,0.4996258701006047,0.9999998452059162],
            [0.49986514066254145,0.4996572254466623,0.9999998643185461],
            [0.4991471166371236,0.5001108668070935,0.9999992602979733],
            [0.49907681106386453,0.5017599233911242,0.999996050376246],
            [0.49963982600765294,0.5012515856396945,0.9999983038052047],
            [0.5009346465028502,0.4994776343888761,0.9999988535687687],
            [0.5003773367133304,0.4994804928734576,0.9999995877291803],
            [0.4999885475829404,0.5000716221235464,0.9999999947391136],
            [0.499982838201183,0.5005771098933283,0.9999996666495325],
            [0.49963063097893173,0.49862059402711706,0.9999979608015299],
            [0.49705109186744356,0.49709402711992734,0.999982858968631],
            [0.5010532767095717,0.49639780938703826,0.9999859146325634],
            [0.5027314908257369,0.4988677277554448,0.9999912568409903],
            [0.4993160647877464,0.49995829644431855,0.9999995304932184],
            [0.5002174701977905,0.49901495926947487,0.9999989824004368],
        ];

        let set = build_detail(6, 1.0);
        assert_eq!(set.size, 6);
        assert_eq!(set.world_size, 0.25);
        assert_eq!(set.relief, 0.0034);
        assert!(set.orm.is_none(), "buildDetail sets orm: false");
        let normal = set.normal.as_ref().expect("buildDetail wants a normal map");

        for y in 0..6u32 {
            for x in 0..6u32 {
                let i = (y * 6 + x) as usize;
                let a = set.albedo.get(x, y);
                let want_a = expected_albedo[i];
                for ch in 0..4 {
                    // Same compounded-libm-drift reasoning as the normal
                    // channel below: each texel is its own full `owSurface`
                    // evaluation (two `owFbm01`s, two `owWorley`s, two
                    // `owScratches`), so tiny per-call drift accumulates past
                    // the single-sample tolerance the point tests above use.
                    assert_close_tol(f64::from(a[ch]), want_a[ch], 1e-6, "albedo channel");
                }
                let n = normal.get(x, y);
                let want_n = expected_normal[i];
                for ch in 0..3 {
                    // The normal channel's `normalize()` (sqrt) sits on top of
                    // 9 upstream `owSurface` evaluations (the Sobel stencil's
                    // 9 height samples), each already carrying the 1e-9-class
                    // libm drift the point samples above tolerate — so the
                    // compounded tolerance here is wider, still far tighter
                    // than a texel's worth of visual difference.
                    assert_close_tol(f64::from(n[ch]), want_n[ch], 1e-6, "normal channel");
                }
            }
        }
    }

    #[test]
    fn build_macro_has_no_orm_or_normal() {
        let set = build_macro(4, 2.0);
        assert_eq!(set.size, 4);
        assert_eq!(set.world_size, 32.0);
        assert_eq!(set.relief, 0.5);
        assert!(set.orm.is_none(), "buildMacro sets orm: false");
        assert!(set.normal.is_none(), "buildMacro sets normal: false");
    }

    // -----------------------------------------------------------------
    // Bake orchestration: linear_albedo skips the sRGB encode, and want_orm
    // controls whether the ORM texture exists at all.
    // -----------------------------------------------------------------

    #[test]
    fn bake_applies_srgb_encode_only_when_not_linear_albedo() {
        let flat = |_uv: Vec2| SurfaceSample {
            albedo: Vec3::splat(0.5),
            height: 0.25,
            roughness: 0.1,
            metal: 0.0,
            ao: 1.0,
        };
        let linear = bake(&BakeDef {
            surface: &flat,
            size: 2,
            world_size: 1.0,
            relief: 0.01,
            linear_albedo: true,
            want_orm: true,
            want_normal: false,
        });
        let encoded = bake(&BakeDef {
            surface: &flat,
            size: 2,
            world_size: 1.0,
            relief: 0.01,
            linear_albedo: false,
            want_orm: false,
            want_normal: false,
        });

        let lin_texel = linear.albedo.get(0, 0);
        assert_eq!(lin_texel[0], 0.5, "linear_albedo must not encode");
        assert_eq!(lin_texel[3], 0.25, "alpha always carries height");

        let enc_texel = encoded.albedo.get(0, 0);
        assert!(
            (f64::from(enc_texel[0]) - linear_to_srgb(0.5)).abs() < 1e-6,
            "non-linear-albedo bake must sRGB-encode"
        );
        assert!(encoded.orm.is_none(), "want_orm: false must skip the ORM texture");
        let orm = linear.orm.expect("want_orm: true must produce an ORM texture");
        assert_eq!(orm.get(0, 0), [1.0, 0.1, 0.0, 1.0]);
    }
}
