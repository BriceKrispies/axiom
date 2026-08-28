//! Ported from Claude-of-Duty `src/fx/atlas.js:1-947` — the whole file.
//!
//! Every FX texture in the game is baked here, once, at load time — there
//! are no image files anywhere in this project. Two atlases:
//!
//! - **particles** — RGBA. RGB is a detail/tint field the shader multiplies
//!   by the per-particle colour; A is coverage. [`P`].
//! - **decals** — albedo RGBA + normal RGB (Sobel-derived from a per-tile
//!   height field) + ORM (`r`=ao, `g`=roughness, `b`=metalness). [`D`].
//!
//! Baking is a pure per-texel evaluation of a "painter" function over
//! `(x, y) in [-1, 1]^2` plus a shared [`crate::fx::noise::Noise`] instance —
//! there is no THREE dependency in the bake itself, only in how the source
//! wraps the finished bytes into a `THREE.DataTexture`
//! (`makeTexture`, `atlas.js:771-782`). That wrapping is the GPU-upload seam:
//! [`bake_particle_atlas`]/[`bake_decal_atlas`]/[`bake_brass_textures`]
//! return the raw byte buffers a future `windowing`-side uploader would hand
//! to a GPU texture, same as [`crate::materials::bake`] returns pixel buffers
//! rather than a live render target.
//!
//! ## Why this does not reuse `crate::materials::bake::Texture`
//!
//! That type exists (landed in `c2f3fbb5`) and was considered, but it is
//! deliberately **`f32`, never quantized** (see its own module doc: "this
//! port never actually quantizes to `u8` at all"). This atlas's source is
//! genuinely `u8`-quantized (`new Uint8Array(size*size*4)`,
//! `encodeSrgb(v) * 255` — `atlas.js:791, 808-811`) and the golden capture in
//! `tests/fx/atlas_port.rs` compares byte-for-byte against that quantized
//! output, so reusing the unquantized `f32` `Texture` would either lose the
//! byte fidelity the golden needs or force a re-quantization step the reused
//! module does not have. The *approach* — a flat row-major buffer, a
//! `linear -> sRGB` encode at write time, a Sobel-from-height normal pass —
//! is the same one `materials::bake` established; see [`encode_srgb`] (this
//! module re-exports [`crate::fx::noise::encode_srgb`], the fx-local noise
//! toolkit's own copy of the identical formula) and [`bake_decal_atlas`]'s
//! height-to-normal pass, the Sobel-derived analogue of
//! `materials::bake::sobel`.

use crate::fx::noise::{clamp01, encode_srgb, smoothstep, Noise};
use crate::rng::Rng;

/// Particle atlas tile indices (4x4 layout). `atlas.js:20-37` (`P`).
pub mod p {
    pub const SMOKE_A: usize = 0;
    pub const SMOKE_B: usize = 1;
    pub const WISP: usize = 2;
    pub const DUST: usize = 3;
    pub const SPARK: usize = 4;
    pub const STREAK: usize = 5;
    pub const FLASH_LOBE: usize = 6;
    pub const FLASH_CORE: usize = 7;
    pub const CHIP: usize = 8;
    pub const SPLINTER: usize = 9;
    pub const DROPLET: usize = 10;
    pub const MIST: usize = 11;
    pub const SPLASH: usize = 12;
    pub const RING: usize = 13;
    pub const FIRE: usize = 14;
    pub const MOTE: usize = 15;
}

/// Decal atlas tile indices (4x4 layout). `atlas.js:42-59` (`D`).
pub mod d {
    pub const HOLE_CONCRETE: usize = 0;
    pub const HOLE_CONCRETE_B: usize = 1;
    pub const HOLE_METAL: usize = 2;
    pub const HOLE_WOOD: usize = 3;
    pub const HOLE_PLASTER: usize = 4;
    pub const GLASS_CRACK: usize = 5;
    pub const BLOOD_A: usize = 6;
    pub const BLOOD_B: usize = 7;
    pub const SCORCH: usize = 8;
    pub const IMPACT_DIRT: usize = 9;
    pub const IMPACT_SAND: usize = 10;
    pub const SCRAPE: usize = 11;
    pub const RIPPLE: usize = 12;
    pub const HOLE_GLASS: usize = 13;
    pub const SMUDGE: usize = 14;
    pub const TEAR: usize = 15;
}

pub const ATLAS_COLS: u32 = 4;

// ============================================================================
// particle tiles — `atlas.js:71-312` (`PARTICLE_PAINTERS`)
// ============================================================================

/// One evaluation of a particle tile painter: `out = [r, g, b, a]`, linear
/// RGB, for a point `(x, y)` in `-1..1` at radius `r = hypot(x, y)`.
fn paint_particle_tile(tile: usize, n: &Noise, x: f64, y: f64, r: f64) -> [f64; 4] {
    match tile {
        p::SMOKE_A => {
            let w = n.warped(x * 2.05 + 3.7, y * 2.05 - 1.4, 0.8, 5);
            let lump = n.worley(x * 2.4 + 7.1, y * 2.4 + 2.3);
            let tear = n.fbm(x * 7.4 - 6.2, y * 7.4 + 3.9, 3);
            let mut a = smoothstep(0.0, 0.58, 1.0 - r + (w - 0.5) * 0.8 + (lump - 0.45) * 0.26);
            a *= 1.0 - smoothstep(0.35, 0.95, r) * (1.0 - tear) * 0.85;
            a *= smoothstep(1.02, 0.66, r);
            let det = n.fbm(x * 4.6 - 2.1, y * 4.6 + 8.4, 4);
            let l = 0.42 + 0.66 * det * det;
            [l, l * 0.995, l * 0.99, a]
        }
        p::SMOKE_B => {
            let cw = 1.0 - n.worley(x * 3.05 - 4.3, y * 3.05 + 6.7);
            let f = n.fbm(x * 1.9 + 12.2, y * 1.9 - 3.3, 4);
            let tear = n.fbm(x * 8.8 + 1.7, y * 8.8 - 4.4, 3);
            let mut a = smoothstep(0.0, 0.52, 1.0 - r * 1.02 + (cw - 0.42) * 0.68 + (f - 0.5) * 0.46);
            a *= 1.0 - smoothstep(0.3, 0.92, r) * (1.0 - tear) * 0.9;
            a *= smoothstep(1.0, 0.6, r);
            let det = n.fbm(x * 6.1 + 4.4, y * 6.1 - 5.2, 4);
            let l = 0.38 + 0.7 * (0.35 * det + 0.65 * cw);
            [l, l * 0.99, l * 0.975, a]
        }
        p::WISP => {
            let fil = n.ridged(x * 2.6 + 1.3, y * 1.05 - 6.0, 4);
            let body = smoothstep(0.0, 0.75, 1.0 - (x * 1.05).hypot(y * 0.78));
            let mut a = clamp01(smoothstep(0.28, 0.85, fil) * 1.15 + 0.16) * body * body;
            a *= smoothstep(1.05, 0.55, r);
            let l = 0.5 + 0.55 * fil;
            [l, l * 0.99, l * 0.985, a * 0.9]
        }
        p::DUST => {
            let w = n.warped(x * 3.3 + 21.1, y * 3.3 + 5.9, 0.55, 5);
            let grit = n.fbm(x * 15.5 + 3.3, y * 15.5 - 7.7, 2);
            let mut a = smoothstep(0.0, 0.5, 1.0 - r * 1.04 + (w - 0.5) * 0.86);
            a *= 1.0 - smoothstep(0.25, 0.9, r) * (1.0 - grit) * 0.8;
            a *= smoothstep(1.0, 0.62, r);
            let l = 0.48 + 0.42 * w + 0.16 * smoothstep(0.6, 0.92, grit);
            [l.min(1.0), (l * 0.985).min(1.0), (l * 0.96).min(1.0), a]
        }
        p::SPARK => {
            let core = (-r * r * 26.0).exp();
            let glow = (-r * r * 4.5).exp() * 0.4;
            let a = clamp01(core + glow) * smoothstep(1.0, 0.85, r);
            [1.0, 1.0, 1.0, a]
        }
        p::STREAK => {
            let half_w = 0.26 * (0.55 + 0.45 * smoothstep(-1.0, 0.6, y));
            let q = x.abs() / half_w;
            let core = (-q * q * 3.0).exp();
            let along = smoothstep(-1.02, -0.55, y) * smoothstep(1.02, 0.72, y);
            let taper = 0.1 + 0.9 * smoothstep(-1.0, 0.85, y);
            let jag = 0.85 + 0.3 * n.fbm(x * 6.0 + 2.2, y * 3.1 - 8.8, 3);
            let mut a = core * along * taper * jag;
            a += (-(x * x * 90.0 + (y - 0.72) * (y - 0.72) * 30.0)).exp() * 0.9;
            [1.0, 1.0, 1.0, clamp01(a)]
        }
        p::FLASH_LOBE => {
            let u = clamp01((x + 1.0) * 0.5);
            let swell = (u + 0.02).powf(0.5) * (1.0 - u).powf(0.7);
            let lean = 0.26 * u * u - 0.07 * u + 0.2 * (n.fbm(u * 2.4 + 4.1, 8.3, 3) - 0.5) * u;
            let mut w = 0.04 + 1.5 * swell;
            let shear = n.fbm(u * 4.6 - 3.3, y * 2.2 + 1.9, 4);
            let flank = smoothstep(0.0, 0.5, (y - lean) / w.max(1e-3));
            w *= 1.0 - flank * (1.0 - shear) * 0.5;
            let q = (y - lean) / w.max(1e-3);
            let mut a = (-q * q * 2.0).exp();
            let frag = n.fbm(u * 6.8 + 12.7, y * 4.4 - 6.4, 4);
            a *= 1.0 - smoothstep(0.46, 1.0, u) * (1.0 - frag) * 1.5;
            a *= 0.72 + 0.46 * n.fbm(u * 7.9 - 8.1, y * 3.6 + 15.2, 4);
            a *= smoothstep(-1.0, -0.86, x) * smoothstep(1.0, 0.86, u);
            a += (-((x + 0.8) * (x + 0.8) * 13.0 + y * y * 30.0)).exp() * 0.85;
            let edge = clamp01(q.abs() * 0.72);
            let t = clamp01(smoothstep(0.02, 0.72, u) * 0.85 + edge * 0.4);
            let soot = smoothstep(0.62, 1.0, u) * 0.34;
            let den = 0.82 + 0.34 * shear;
            [
                clamp01((1.0 - soot) * den),
                clamp01((1.0 - 0.52 * t) * (1.0 - soot * 1.3) * den),
                clamp01((1.0 - 0.84 * t) * (1.0 - soot * 1.6) * den),
                clamp01(a),
            ]
        }
        p::FLASH_CORE => {
            let ang = y.atan2(x);
            let lump = n.fbm(ang.cos() * 1.7 + 30.1, ang.sin() * 1.7 + 9.4, 3);
            let churn = n.fbm(x * 3.1 - 5.5, y * 3.1 + 2.2, 4);
            let rr = r * (0.84 + 0.32 * lump) * (0.94 + 0.14 * churn);
            let mut a = (-rr * rr * 15.0).exp() * (0.74 + 0.48 * churn) + (-rr * rr * 44.0).exp() * 0.95;
            a *= smoothstep(1.02, 0.72, r);
            let t = smoothstep(0.0, 0.7, rr);
            [1.0, 1.0 - 0.2 * t, 1.0 - 0.5 * t, clamp01(a)]
        }
        p::CHIP => {
            let ang = y.atan2(x);
            let shape = 0.52 + 0.34 * n.fbm(ang.cos() * 1.8 + 5.5, ang.sin() * 1.8 + 2.9, 2)
                - 0.1 * (ang * 2.5).sin().abs();
            let a = smoothstep(shape, shape - 0.09, r);
            let facet = n.fbm(x * 3.4 - 4.2, y * 3.4 + 1.1, 3);
            let l = 0.34 + 0.5 * facet + 0.22 * clamp01(0.5 - y);
            [l, l * 0.985, l * 0.955, a]
        }
        p::SPLINTER => {
            let bend = 0.16 * (y * 2.1 + 1.3).sin();
            let w = 0.155 * (1.0 - 0.72 * y.abs()) * (0.7 + 0.6 * n.fbm(y * 4.4 + 2.2, 7.7, 2));
            let dd = (x - bend).abs() / w.max(0.02);
            let a = smoothstep(1.0, 0.72, dd) * smoothstep(1.0, 0.9, y.abs());
            let grain = n.fbm(x * 9.0 + 1.1, y * 2.2 - 3.3, 3);
            let l = 0.3 + 0.5 * grain;
            [l, l * 0.82, l * 0.6, a]
        }
        p::DROPLET => {
            let yy = y * 0.92;
            let head = x.hypot((yy - 0.16) * 1.05) / 0.5;
            let tail_w = 0.42 * clamp01(0.9 - yy * 0.75);
            let tail = (x / tail_w.max(0.03)).hypot((yy + 0.5) * 0.85);
            let a = clamp01(smoothstep(1.0, 0.82, head) + smoothstep(1.0, 0.55, tail) * 0.9);
            let spec = (-((x + 0.16) * (x + 0.16) + (yy - 0.3) * (yy - 0.3)) * 26.0).exp();
            let l = 0.55 + 0.45 * spec + 0.16 * clamp01(0.4 - yy);
            [l.min(1.0), (l * 0.9).min(1.0), (l * 0.86).min(1.0), a]
        }
        p::MIST => {
            let f = n.fbm(x * 5.2 + 31.4, y * 5.2 - 12.6, 5);
            let mut a = smoothstep(0.04, 0.9, (1.0 - r) * 0.95 + (f - 0.5) * 0.95) * 0.78;
            a *= smoothstep(1.02, 0.5, r);
            let l = 0.5 + 0.5 * f;
            [l, l * 0.97, l * 0.96, a]
        }
        p::SPLASH => {
            let width = 0.66 - 0.3 * smoothstep(-1.0, 1.0, y);
            let col = 1.0 - x.abs() / width.max(0.05);
            let body = n.warped(x * 2.4 + 2.4, y * 1.5 - 9.1, 0.7, 4);
            let shred = n.fbm(x * 5.5 - 1.2, y * 3.0 + 4.4, 4);
            let mut a = smoothstep(0.0, 0.5, col + (body - 0.5) * 0.55);
            a *= 1.0 - smoothstep(-0.2, 1.0, y) * (1.0 - shred) * 0.95;
            a *= smoothstep(1.04, 0.84, y.abs());
            let drop = (-((x - 0.12) * (x - 0.12) * 55.0 + (y - 0.74) * (y - 0.74) * 45.0)).exp();
            a = clamp01(a + drop * 0.5);
            let l = 0.58 + 0.42 * body + 0.12 * shred;
            [(l * 0.94).min(1.0), (l * 0.98).min(1.0), l.min(1.0), a]
        }
        p::RING => {
            let ang = y.atan2(x);
            let wob = 1.0 + 0.055 * (n.fbm(ang.cos() * 3.1 + 6.6, ang.sin() * 3.1 - 2.2, 3) - 0.5);
            let t = (r * wob - 0.74) / 0.16;
            let mut a = (-t * t * 3.2).exp();
            a += smoothstep(0.78, 0.2, r) * 0.1;
            [1.0, 1.0, 1.0, clamp01(a) * smoothstep(1.0, 0.94, r)]
        }
        p::FIRE => {
            let w = n.warped(x * 2.5 + 9.2, y * 2.5 + 17.5, 0.95, 5);
            let mut a = smoothstep(0.08, 0.6, 1.0 - r * 0.98 + (w - 0.5) * 0.8);
            a *= smoothstep(1.02, 0.6, r);
            let heat = clamp01(1.15 - r * 1.35 + (w - 0.5) * 0.65);
            [1.0, 0.28 + 0.7 * heat, 0.06 + 0.82 * heat.powf(2.6), a]
        }
        p::MOTE => {
            let j = 1.0 + 0.3 * (n.fbm(x * 5.0 + 44.2, y * 5.0 - 17.3, 2) - 0.5);
            let a = (-(r * j) * (r * j) * 11.0).exp() * 0.95;
            [1.0, 0.99, 0.96, a * smoothstep(1.0, 0.8, r)]
        }
        _ => unreachable!("particle tile index out of range: {tile}"),
    }
}

// ============================================================================
// decal tiles — `atlas.js:314-762`
// ============================================================================

const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// Accumulator for `mixReset`/`mixAdd`/`mixInto` (`atlas.js:344-366`) — a
/// stack value here instead of the source's module-level `Float64Array`
/// scratch (see [`crate::fx::util`]'s module doc for why: nothing shares
/// this across calls, so there is no allocation to avoid by hoisting it).
#[derive(Default)]
struct Mix {
    r: f64,
    g: f64,
    b: f64,
    cov: f64,
}

impl Mix {
    fn add(&mut self, cov: f64, r: f64, g: f64, b: f64) {
        if cov <= 0.0 {
            return;
        }
        self.r += r * cov;
        self.g += g * cov;
        self.b += b * cov;
        self.cov += cov;
    }

    /// `mixInto(out, edge)`, `atlas.js:359-366`. Returns `(r, g, b, a)`.
    fn into_rgba(&self, edge: f64) -> (f64, f64, f64, f64) {
        let inv = if self.cov > 1e-6 { 1.0 / self.cov } else { 0.0 };
        (
            self.r * inv,
            self.g * inv,
            self.b * inv,
            clamp01(self.cov) * edge,
        )
    }
}

/// Contact-occlusion annulus. `atlas.js:372-379`.
fn contact_ao(r: f64, rb: f64, peak: f64) -> f64 {
    if r <= rb {
        return peak;
    }
    let t = clamp01((r - rb) / (rb * 0.9));
    peak * (1.0 - t).powf(2.2)
}

/// 3-5 asymmetric radial spall cracks running out of the bore. `atlas.js:
/// 389-407`.
fn radial_spall(n: &Noise, ang: f64, r: f64, seed: f64, count: i32) -> f64 {
    let mut c = 0.0_f64;
    for k in 0..count {
        let kf = k as f64;
        let h = n.fbm(kf * 4.7 + seed, kf * 2.3 - seed * 0.7, 2);
        let h2 = n.fbm(kf * 2.9 - seed, kf * 5.1 + seed * 1.3, 2);
        let len = 0.24 + 0.52 * h2;
        if r > len {
            continue;
        }
        let a0 = ((kf + (h - 0.5) * 1.5) / count as f64) * TWO_PI + seed;
        let mut d = ang - a0;
        d -= TWO_PI * (d / TWO_PI).round();
        let wob = (n.fbm(r * 7.5 + kf * 3.3, seed + kf * 1.7, 3) - 0.5) * 0.5;
        let w = 0.03 * (1.0 - r / len) + 0.006;
        let q = (d + wob * r).abs() * r / w;
        c = c.max(smoothstep(1.0, 0.25, q) * (0.4 + 0.85 * h2) * smoothstep(len, len * 0.55, r));
    }
    clamp01(c)
}

/// Relief strength per decal tile — `atlas.js:769` (`DECAL_RELIEF`).
const DECAL_RELIEF: [f64; 16] = [
    2.6, 3.0, 2.2, 2.4, 2.3, 1.4, 0.8, 0.7, 0.35, 2.4, 1.9, 1.6, 1.1, 1.5, 0.2, 1.7,
];

/// One evaluation of a decal tile painter: `out = [r, g, b, a, height,
/// roughness, metalness]`. `height` 0.5 == flush with the wall, 0 == deep,
/// 1 == proud. `atlas.js:413-761` (`DECAL_PAINTERS`).
fn paint_decal_tile(tile: usize, n: &Noise, x: f64, y: f64, r: f64) -> [f64; 7] {
    match tile {
        d::HOLE_CONCRETE => {
            let ang = y.atan2(x);
            let jit = n.fbm(ang.cos() * 2.6 + 3.1, ang.sin() * 2.6 - 7.4, 3) - 0.5;
            let rb = 0.185 * (1.0 + jit * 0.36);
            let bore = smoothstep(rb + 0.028, rb - 0.012, r);
            let crumb = n.fbm(x * 5.2 + 2.2, y * 5.2 - 1.1, 4).powf(1.4);
            let rim_t = (r - rb * 1.3) / (0.11 + 0.05 * (jit + 0.5));
            let rim = clamp01((-rim_t * rim_t * 1.4).exp() * (0.3 + 1.25 * crumb)) * (1.0 - bore);
            let ao = contact_ao(r, rb, 0.40 + 0.12 * crumb) * (1.0 - bore);
            let crack = radial_spall(n, ang, r, 3.7, 4) * smoothstep(rb * 0.8, rb * 1.25, r);
            let grit = clamp01(1.0 - n.worley(x * 12.5 + 8.8, y * 12.5 - 3.3) * 2.1).powf(2.0)
                * smoothstep(0.85, 0.2, r);
            let mut mix = Mix::default();
            mix.add(bore, 0.021, 0.0195, 0.018);
            mix.add(rim * 0.92, 0.235, 0.222, 0.198);
            mix.add(grit * 0.4, 0.19, 0.18, 0.163);
            mix.add(crack * 0.8, 0.045, 0.042, 0.038);
            mix.add(ao, 0.0, 0.0, 0.0);
            let (rr, gg, bb, a) = mix.into_rgba(smoothstep(1.0, 0.74, r));
            let height = 0.5 - bore * 0.5 + rim * 0.1 - crack * 0.22 - ao * 0.1;
            [rr, gg, bb, a, height, clamp01(0.99 - rim * 0.05), 0.0]
        }
        d::HOLE_CONCRETE_B => {
            let ang = y.atan2(x);
            let lobes = 0.26 + 0.13 * n.fbm(ang.cos() * 1.7 + 20.5, ang.sin() * 1.7 + 6.6, 3);
            let erode = n.fbm(x * 6.6 + 14.2, y * 6.6 - 8.1, 4);
            let crater = smoothstep(lobes + 0.1, lobes - 0.08, r + (erode - 0.5) * 0.14);
            let rb = 0.105 + 0.028 * n.fbm(ang.cos() * 3.0 + 1.5, ang.sin() * 3.0 - 2.5, 2);
            let bore = smoothstep(rb + 0.03, rb - 0.014, r);
            let chips = (1.0 - n.worley(x * 13.5 + 8.8, y * 13.5 - 3.3)).powf(2.6)
                * crater
                * (0.4 + 1.1 * n.fbm(x * 5.5 - 1.1, y * 5.5 + 6.2, 3));
            let crack = radial_spall(n, ang, r, 8.2, 5) * smoothstep(rb * 0.9, rb * 1.4, r);
            let floor_ao = crater * 0.34;
            let ao = contact_ao(r, lobes * 0.92, 0.34 + 0.14 * chips) * (1.0 - bore) * (1.0 - crater * 0.6);
            let mut mix = Mix::default();
            mix.add(bore, 0.020, 0.0185, 0.017);
            mix.add(crater * 0.64, 0.205 + 0.05 * chips, 0.195 + 0.045 * chips, 0.175 + 0.04 * chips);
            mix.add(crack * 0.78, 0.042, 0.039, 0.035);
            mix.add(clamp01(floor_ao + ao), 0.0, 0.0, 0.0);
            let (rr, gg, bb, a) = mix.into_rgba(smoothstep(1.0, 0.76, r));
            let height = 0.5 - bore * 0.5 - crater * 0.24 + chips * 0.12 - crack * 0.2;
            [rr, gg, bb, a, height, 0.99, 0.0]
        }
        d::HOLE_METAL => {
            let ang = y.atan2(x);
            let warp = n.fbm(ang.cos() * 2.2 + 4.4, ang.sin() * 2.2 + 9.1, 3);
            let petal = 0.45 * (ang * 2.5 + warp * 5.5).sin().abs() + 0.55 * warp;
            let rh = 0.14 + 0.035 * petal;
            let hole = smoothstep(rh + 0.03, rh - 0.015, r);
            let lip_t = (r - rh * (1.35 + 0.5 * petal)) / (0.07 + 0.09 * petal);
            let grain = n.fbm(x * 9.5 + 1.3, y * 9.5 - 5.6, 3);
            let lip = (-lip_t * lip_t * 2.4).exp() * (0.45 + 1.05 * grain);
            let scuff = smoothstep(0.9, 0.2, r) * n.fbm(x * 7.5 + 6.1, y * 7.5 - 2.2, 3);
            let scratch = clamp01(1.0 - n.worley_edge(x * 6.0 + 2.0, y * 6.0 - 8.0) * 9.0).powf(2.0)
                * smoothstep(0.85, 0.1, r);
            let ao = contact_ao(r, rh, 0.34) * (1.0 - hole);
            let a = clamp01(hole + lip * 0.7 + scuff * 0.5 + scratch * 0.3 + ao) * smoothstep(1.0, 0.75, r);
            let bare = clamp01(lip * 0.85 + scratch * 0.45);
            let l = if hole > 0.5 {
                0.018
            } else {
                clamp01((0.09 + 0.24 * bare + 0.07 * scuff) * (1.0 - ao))
            };
            [
                l,
                l * 0.985,
                l * 0.96,
                a,
                0.5 - hole * 0.5 + lip * 0.26,
                clamp01(0.62 - bare * 0.42),
                clamp01(bare * 0.85),
            ]
        }
        d::HOLE_WOOD => {
            let ang = y.atan2(x);
            let fib = n.fbm(x * 1.6 + 5.5, y * 11.0 - 2.2, 4);
            let rb = 0.16 * (1.0 + (fib - 0.5) * 0.34);
            let bore = smoothstep(rb + 0.03, rb - 0.014, r);
            let splinter = clamp01(1.0 - (ang * 6.5 + fib * 5.0).sin().abs() * 1.05).powf(3.2)
                * smoothstep(0.52, rb * 0.9, r)
                * (0.45 + 1.0 * n.fbm(x * 7.7 - 2.2, y * 7.7 + 5.5, 3));
            let lip_t = (r - rb * 1.25) / 0.1;
            let lip = clamp01((-lip_t * lip_t * 1.5).exp() * (0.4 + 1.1 * fib)) * (1.0 - bore);
            let ao = contact_ao(r, rb, 0.44 + 0.1 * fib) * (1.0 - bore);
            let crack = radial_spall(n, ang, r, 5.9, 3) * smoothstep(rb * 0.8, rb * 1.3, r);
            let mut mix = Mix::default();
            mix.add(bore, 0.015, 0.0125, 0.010);
            mix.add(clamp01(lip * 0.85 + splinter * 0.6), 0.125, 0.088, 0.052);
            mix.add(crack * 0.7, 0.03, 0.022, 0.014);
            mix.add(ao, 0.0, 0.0, 0.0);
            let (rr, gg, bb, a) = mix.into_rgba(smoothstep(1.0, 0.72, r));
            let height = 0.5 - bore * 0.5 + splinter * 0.22 - crack * 0.18 - ao * 0.08;
            [rr, gg, bb, a, height, clamp01(0.9 - splinter * 0.1), 0.0]
        }
        d::HOLE_PLASTER => {
            let ang = y.atan2(x);
            let jit = n.fbm(ang.cos() * 2.2 + 14.4, ang.sin() * 2.2 + 3.3, 3) - 0.5;
            let rb = 0.195 * (1.0 + jit * 0.4);
            let bore = smoothstep(rb + 0.032, rb - 0.014, r);
            let crumb = (1.0 - n.worley(x * 11.5 - 3.3, y * 11.5 + 7.7)).powf(2.4)
                * (0.35 + 1.2 * n.fbm(x * 4.4 + 9.9, y * 4.4 - 2.2, 3));
            let rim_t = (r - rb * 1.26) / (0.12 + 0.05 * (jit + 0.5));
            let rim = clamp01((-rim_t * rim_t * 1.35).exp() * (0.32 + 1.2 * crumb)) * (1.0 - bore);
            let powder = smoothstep(0.9, 0.18, r) * n.fbm(x * 3.1 + 8.2, y * 3.1 - 4.4, 4) * 0.3;
            let ao = contact_ao(r, rb, 0.40 + 0.12 * crumb) * (1.0 - bore);
            let crack = radial_spall(n, ang, r, 1.9, 4) * smoothstep(rb * 0.8, rb * 1.25, r);
            let mut mix = Mix::default();
            mix.add(bore, 0.018, 0.0165, 0.015);
            mix.add(rim * 0.94, 0.305, 0.278, 0.234);
            mix.add(powder, 0.26, 0.238, 0.20);
            mix.add(crack * 0.8, 0.04, 0.036, 0.03);
            mix.add(ao, 0.0, 0.0, 0.0);
            let (rr, gg, bb, a) = mix.into_rgba(smoothstep(1.0, 0.75, r));
            let height = 0.5 - bore * 0.5 + rim * 0.12 - crack * 0.2 - ao * 0.1;
            [rr, gg, bb, a, height, 1.0, 0.0]
        }
        d::GLASS_CRACK => {
            let ang = y.atan2(x);
            let radial = clamp01(
                1.0 - (ang * 5.5 + n.fbm(r * 3.0 + 2.0, ang + 4.0, 3) * 6.0).sin().abs() * 1.6,
            )
            .powf(6.0);
            let conc = clamp01(1.0 - (r * 17.0 + n.fbm(x * 2.0, y * 2.0, 2) * 4.0).sin().abs() * 1.9).powf(5.0);
            let web = clamp01(radial * smoothstep(0.98, 0.05, r) + conc * smoothstep(0.95, 0.12, r) * 0.7);
            let rh = 0.055;
            let hole = smoothstep(rh + 0.02, rh - 0.01, r);
            let shatter =
                clamp01(1.0 - n.worley_edge(x * 4.4 + 1.1, y * 4.4 - 6.6) * 8.0).powf(3.0) * smoothstep(0.7, 0.1, r);
            let a = clamp01(web * 0.95 + hole + shatter * 0.8);
            let l = if hole > 0.5 { 0.02 } else { clamp01(0.5 + 0.45 * web) };
            [
                l * 0.94,
                l * 0.98,
                l,
                a,
                0.5 - hole * 0.4 + web * 0.2,
                clamp01(0.42 - web * 0.3),
                0.0,
            ]
        }
        d::BLOOD_A => {
            let ang = y.atan2(x);
            let edge = 0.44 + 0.3 * n.fbm(ang.cos() * 2.1 + 6.5, ang.sin() * 2.1 - 9.9, 3);
            let wob = n.fbm(x * 5.5 - 3.3, y * 5.5 + 1.7, 4);
            let mut a = smoothstep(edge, edge - 0.16, r + (wob - 0.5) * 0.16);
            let s = n.worley(x * 4.2 + 3.7, y * 4.2 - 5.5);
            a = clamp01(a + clamp01(1.0 - s * 3.4).powf(5.0) * smoothstep(1.0, 0.42, r) * 0.95);
            let thick = a * (0.25 + 1.35 * n.warped(x * 4.4 - 2.2, y * 4.4 + 4.4, 0.7, 4).powf(1.5));
            let rim = smoothstep(edge - 0.16, edge, r);
            [
                clamp01(0.075 + 0.075 * thick - rim * 0.035),
                clamp01(0.009 + 0.017 * thick),
                clamp01(0.008 + 0.014 * thick),
                a,
                0.5 + thick * 0.12,
                clamp01(0.5 - thick * 0.26 + rim * 0.24),
                0.0,
            ]
        }
        d::BLOOD_B => {
            let ang = y.atan2(x);
            let edge = 0.4 + 0.22 * n.fbm(ang.cos() * 1.8 + 16.5, ang.sin() * 1.8 + 2.2, 3);
            let mut a = smoothstep(edge, edge - 0.12, x.hypot((y + 0.12) * 1.18));
            let lane = n.fbm(x * 9.5 + 1.7, 3.3, 2);
            let in_lane = clamp01(1.0 - (x * 11.5 + lane * 4.0).sin().abs() * 1.5).powf(5.0);
            let under = smoothstep(edge, edge * 0.25, x.abs());
            let y0 = -edge * 0.8;
            let run_len = (0.3 + 0.55 * lane) * under;
            let t = if run_len > 0.02 { (y0 - y) / run_len } else { -1.0 };
            let run = if t > 0.0 && t < 1.0 {
                in_lane * under * (1.0 - t * 0.65) * smoothstep(0.0, 0.06, t) * smoothstep(1.0, 0.86, t)
            } else {
                0.0
            };
            a = clamp01(a + run * 0.9);
            let bead = (-((x - (lane - 0.5) * 0.12) * 26.0).powi(2) - ((y - (y0 - run_len)) * 22.0).powi(2)).exp();
            a = clamp01(a + bead * in_lane * under * 0.85);
            let thick = a * (0.5 + 0.8 * n.fbm(x * 4.4 + 7.7, y * 4.4 - 1.1, 3));
            [
                clamp01(0.07 + 0.08 * thick),
                clamp01(0.008 + 0.016 * thick),
                clamp01(0.007 + 0.013 * thick),
                a,
                0.5 + thick * 0.1,
                clamp01(0.46 - thick * 0.24),
                0.0,
            ]
        }
        d::SCORCH => {
            let ang = y.atan2(x);
            let streak = 0.55 + 0.55 * n.fbm(ang.cos() * 3.3 + 4.4, ang.sin() * 3.3 - 8.8, 4);
            let body = smoothstep(0.98 * streak, 0.05, r);
            let soot = n.fbm(x * 3.4 + 2.4, y * 3.4 + 12.2, 5);
            let a = clamp01(body * (0.45 + 0.85 * soot));
            let l = clamp01(0.035 + 0.07 * (1.0 - body) + 0.03 * soot);
            [l, l * 0.96, l * 0.92, a * 0.92, 0.5 - body * 0.04, 1.0, 0.0]
        }
        d::IMPACT_DIRT => {
            let ang = y.atan2(x);
            let rh = 0.26 + 0.09 * (n.fbm(ang.cos() * 1.9 + 9.1, ang.sin() * 1.9 - 3.7, 3) - 0.5);
            let crater = smoothstep(rh + 0.09, rh - 0.05, r);
            let collar = (-(((r - rh * 1.5) / 0.24).powi(2)) * 1.7).exp();
            let clods = (1.0 - n.worley(x * 3.9 + 5.2, y * 3.9 + 1.4)).powf(2.4);
            let a = clamp01(crater + collar * 0.85 * (0.4 + clods) + smoothstep(1.0, 0.3, r) * 0.3);
            let l = clamp01(0.045 + 0.06 * (1.0 - crater) + 0.07 * collar * clods);
            [
                l,
                l * 0.82,
                l * 0.62,
                a,
                0.5 - crater * 0.4 + collar * clods * 0.2,
                0.96,
                0.0,
            ]
        }
        d::IMPACT_SAND => {
            let ang = y.atan2(x);
            let rh = 0.3 + 0.07 * (n.fbm(ang.cos() * 2.4 + 1.1, ang.sin() * 2.4 + 8.3, 3) - 0.5);
            let crater = smoothstep(rh + 0.13, rh - 0.06, r);
            let collar = (-(((r - rh * 1.42) / 0.26).powi(2)) * 1.5).exp();
            let grain = n.fbm(x * 11.0 + 3.3, y * 11.0 - 6.6, 3);
            let rays = clamp01(
                1.0 - (ang * 4.5 + n.fbm(ang.cos() * 2.0, ang.sin() * 2.0, 3) * 6.0).sin().abs() * 1.15,
            )
            .powf(2.5)
                * collar;
            let a = clamp01(crater * 0.8 + collar * 0.45 + rays * 0.45 + smoothstep(1.0, 0.3, r) * 0.3);
            let l = clamp01(0.3 + 0.1 * collar + 0.1 * rays - 0.16 * crater + 0.08 * grain);
            [
                l,
                l * 0.9,
                l * 0.72,
                a,
                0.5 - crater * 0.3 + collar * 0.12,
                1.0,
                0.0,
            ]
        }
        d::SCRAPE => {
            let wander = (n.fbm(x * 2.6 + 2.2, 1.7, 3) - 0.5) * 0.22;
            let taper = smoothstep(-0.95, -0.55, x) * smoothstep(1.0, 0.35, x);
            let w = 0.19 * taper * (0.55 + 0.8 * n.fbm(x * 4.4 - 6.6, 3.3, 3));
            let dd = (y - wander).abs() / w.max(0.02);
            let gouge = smoothstep(1.15, 0.15, dd) * taper;
            let striae = clamp01(1.0 - (y * 34.0 + n.fbm(x * 3.2, 5.5, 2) * 4.0).sin().abs() * 1.25).powf(4.0)
                * gouge
                * (0.3 + 1.2 * n.fbm(x * 9.0 + 1.1, y * 2.0, 3));
            let a = clamp01(gouge * 0.85 + striae * 0.4);
            let l = clamp01(0.18 + 0.34 * striae + 0.16 * gouge);
            [
                l,
                l * 0.98,
                l * 0.95,
                a,
                0.5 - gouge * 0.22 + striae * 0.1,
                clamp01(0.55 - striae * 0.35),
                clamp01(gouge * 0.85),
            ]
        }
        d::RIPPLE => {
            let wob = 1.0 + 0.06 * (n.fbm(x * 3.0 + 6.0, y * 3.0 - 2.0, 3) - 0.5);
            let rings = (r * wob * 22.0).sin() * (-r * r * 2.6).exp();
            let a = clamp01((-r * r * 2.2).exp() * 0.6) * smoothstep(1.0, 0.85, r);
            [0.42, 0.46, 0.5, a, 0.5 + rings * 0.28, 0.06, 0.0]
        }
        d::HOLE_GLASS => {
            let ang = y.atan2(x);
            let rad = clamp01(
                1.0 - (ang * 4.5 + n.fbm(r * 4.0 + 1.0, ang * 2.0, 3) * 5.0).sin().abs() * 1.5,
            )
            .powf(7.0);
            let web = rad * smoothstep(0.72, 0.04, r);
            let hole = smoothstep(0.08, 0.05, r);
            let frost = (1.0 - n.worley(x * 7.0 + 2.2, y * 7.0 - 3.3)).powf(3.0) * smoothstep(0.34, 0.04, r);
            let a = clamp01(web * 0.9 + hole + frost * 0.85);
            let l = if hole > 0.5 { 0.02 } else { clamp01(0.55 + 0.4 * (web + frost)) };
            [
                l * 0.95,
                l * 0.99,
                l,
                a,
                0.5 - hole * 0.4 + (web + frost) * 0.18,
                clamp01(0.36 - frost * 0.24),
                0.0,
            ]
        }
        d::SMUDGE => {
            let w = n.warped(x * 2.2 + 18.8, y * 2.2 - 5.5, 0.8, 5);
            let a = clamp01(smoothstep(0.05, 0.75, (1.0 - r) + (w - 0.5) * 0.9)) * 0.7;
            let l = clamp01(0.22 + 0.22 * w);
            [l, l * 0.96, l * 0.9, a, 0.5, 1.0, 0.0]
        }
        d::TEAR => {
            let ang = y.atan2(x);
            let rip = clamp01(
                1.0 - (ang * 2.5 + n.fbm(ang.cos() * 2.0 + 3.0, ang.sin() * 2.0, 2) * 4.0).sin().abs() * 1.2,
            )
            .powf(3.0);
            let rh = 0.1 + 0.14 * rip;
            let hole = smoothstep(rh + 0.05, rh - 0.02, r);
            let fray =
                clamp01(1.0 - n.worley_edge(x * 8.0 + 4.4, y * 8.0 - 1.1) * 9.0).powf(2.0) * smoothstep(0.5, 0.1, r);
            let a = clamp01(hole + fray * 0.9);
            let l = if hole > 0.5 { 0.02 } else { clamp01(0.18 + 0.3 * fray) };
            [
                l,
                l * 0.94,
                l * 0.88,
                a,
                0.5 - hole * 0.45 + fray * 0.12,
                0.98,
                0.0,
            ]
        }
        _ => unreachable!("decal tile index out of range: {tile}"),
    }
}

// ============================================================================
// bakers — `atlas.js:764-947`
// ============================================================================

/// A baked particle atlas: `size * size` RGBA8 bytes, row-major, plus the
/// tile-grid column count. `atlas.js:784-816` (`buildParticleAtlas`), minus
/// the `THREE.DataTexture` wrap — see the module doc.
pub struct ParticleAtlas {
    pub data: Vec<u8>,
    pub cols: u32,
    pub size: u32,
}

/// A baked decal atlas: albedo (RGB + height-in-alpha's *source*, though the
/// baked albedo alpha channel here is coverage per the painter contract —
/// see the module doc on [`paint_decal_tile`]), a Sobel-derived normal map,
/// and packed ORM. `atlas.js:822-898` (`buildDecalAtlas`).
pub struct DecalAtlas {
    pub albedo: Vec<u8>,
    pub normal: Vec<u8>,
    pub orm: Vec<u8>,
    pub cols: u32,
    pub size: u32,
}

/// Baked brass casing maps (normal + ORM), `atlas.js:904-947`
/// (`buildBrassTextures`).
pub struct BrassTextures {
    pub normal: Vec<u8>,
    pub orm: Vec<u8>,
}

/// Fragment-center sample coordinate in `-1..1` for texel `(px, py)` within a
/// `tile`-px tile, matching `((px + 0.5) / tile) * 2 - 1` at every one of
/// `atlas.js`'s bake loops.
fn tile_coord(px: u32, tile: u32) -> f64 {
    ((f64::from(px) + 0.5) / f64::from(tile)) * 2.0 - 1.0
}

/// `buildParticleAtlas(rng, size)`, `atlas.js:788-816`.
pub fn bake_particle_atlas(rng: &mut Rng, size: u32) -> ParticleAtlas {
    let n = Noise::new(rng);
    let tile = size / ATLAS_COLS;
    let mut data = vec![0u8; (size * size * 4) as usize];
    for t in 0..16usize {
        let ox = (t as u32 % ATLAS_COLS) * tile;
        let oy = (t as u32 / ATLAS_COLS) * tile;
        for py in 0..tile {
            let y = tile_coord(py, tile);
            for px in 0..tile {
                let x = tile_coord(px, tile);
                let r = x.hypot(y);
                let out = if r < 1.45 {
                    paint_particle_tile(t, &n, x, y, r)
                } else {
                    [0.0, 0.0, 0.0, 0.0]
                };
                // A 1px transparent gutter stops bilinear/mip bleed between tiles.
                let gutter = if px < 1 || py < 1 || px > tile - 2 || py > tile - 2 {
                    0.0
                } else {
                    1.0
                };
                let i = (((oy + py) * size + ox + px) * 4) as usize;
                data[i] = (encode_srgb(out[0]) * 255.0) as u8;
                data[i + 1] = (encode_srgb(out[1]) * 255.0) as u8;
                data[i + 2] = (encode_srgb(out[2]) * 255.0) as u8;
                data[i + 3] = (clamp01(out[3]) * 255.0 * gutter) as u8;
            }
        }
    }
    ParticleAtlas { data, cols: ATLAS_COLS, size }
}

/// `buildDecalAtlas(rng, size)`, `atlas.js:822-898`.
pub fn bake_decal_atlas(rng: &mut Rng, size: u32) -> DecalAtlas {
    let n = Noise::new(rng);
    let tile = size / ATLAS_COLS;
    let mut albedo = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut orm = vec![0u8; (size * size * 4) as usize];
    let mut height = vec![0.0f64; (size * size) as usize];

    for t in 0..16usize {
        let ox = (t as u32 % ATLAS_COLS) * tile;
        let oy = (t as u32 / ATLAS_COLS) * tile;
        for py in 0..tile {
            let y = tile_coord(py, tile);
            for px in 0..tile {
                let x = tile_coord(px, tile);
                let r = x.hypot(y);
                let out = if r < 1.45 {
                    paint_decal_tile(t, &n, x, y, r)
                } else {
                    [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 0.0]
                };
                let gutter = if px < 1 || py < 1 || px > tile - 2 || py > tile - 2 {
                    0.0
                } else {
                    1.0
                };
                let idx = ((oy + py) * size + ox + px) as usize;
                let i = idx * 4;
                albedo[i] = (encode_srgb(out[0]) * 255.0) as u8;
                albedo[i + 1] = (encode_srgb(out[1]) * 255.0) as u8;
                albedo[i + 2] = (encode_srgb(out[2]) * 255.0) as u8;
                albedo[i + 3] = (clamp01(out[3]) * 255.0 * gutter) as u8;
                let ao = clamp01(0.35 + 0.65 * smoothstep(0.05, 0.55, out[4]));
                orm[i] = (ao * 255.0) as u8;
                orm[i + 1] = (clamp01(out[5]) * 255.0) as u8;
                orm[i + 2] = (clamp01(out[6]) * 255.0) as u8;
                orm[i + 3] = 255;
                height[idx] = out[4];
            }
        }
    }

    // Height -> tangent-space normal, sampled inside the tile only.
    for t in 0..16u32 {
        let ox = (t % ATLAS_COLS) * tile;
        let oy = (t / ATLAS_COLS) * tile;
        let relief = DECAL_RELIEF[t as usize] * (f64::from(tile) / 256.0);
        for py in 0..tile {
            for px in 0..tile {
                let cx = px.clamp(1, tile.saturating_sub(2));
                let cy = py.clamp(1, tile.saturating_sub(2));
                let at = |dx: i32, dy: i32| -> f64 {
                    let xx = (oy + (cy as i32 + dy) as u32) * size + ox + (cx as i32 + dx) as u32;
                    height[xx as usize]
                };
                let gx = (at(1, 0) - at(-1, 0)) * relief * 8.0;
                let gy = (at(0, 1) - at(0, -1)) * relief * 8.0;
                let (nx, ny, nz) = normalize3(-gx, -gy, 1.0);
                let i = (((oy + py) * size + ox + px) * 4) as usize;
                normal[i] = ((nx * 0.5 + 0.5) * 255.0) as u8;
                normal[i + 1] = ((ny * 0.5 + 0.5) * 255.0) as u8;
                normal[i + 2] = ((nz * 0.5 + 0.5) * 255.0) as u8;
                normal[i + 3] = 255;
            }
        }
    }

    DecalAtlas {
        albedo,
        normal,
        orm,
        cols: ATLAS_COLS,
        size,
    }
}

fn normalize3(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let l = (x * x + y * y + z * z).sqrt();
    let l = if l == 0.0 { 1.0 } else { l };
    (x / l, y / l, z / l)
}

/// `buildBrassTextures(rng, size)`, `atlas.js:904-947`.
pub fn bake_brass_textures(rng: &mut Rng, size: u32) -> BrassTextures {
    let n = Noise::new(rng);
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut orm = vec![0u8; (size * size * 4) as usize];
    let mut h = vec![0.0f64; (size * size) as usize];

    for y in 0..size {
        for x in 0..size {
            let u = f64::from(x) / f64::from(size);
            let v = f64::from(y) / f64::from(size);
            let draw = n.fbm(u * 42.0, v * 3.5, 3);
            let rings = (v * 190.0).sin() * 0.5 + 0.5;
            let scuff = clamp01(1.0 - n.worley_edge(u * 9.0, v * 9.0) * 7.0).powf(2.0);
            h[(y * size + x) as usize] = 0.5 + (draw - 0.5) * 0.35 + (rings - 0.5) * 0.08 - scuff * 0.25;
            let i = ((y * size + x) * 4) as usize;
            orm[i] = (clamp01(1.0 - scuff * 0.55) * 255.0) as u8;
            orm[i + 1] = (clamp01(0.19 + 0.3 * scuff + 0.12 * draw) * 255.0) as u8;
            orm[i + 2] = 255;
            orm[i + 3] = 255;
        }
    }
    for y in 0..size {
        for x in 0..size {
            let at = |dx: i64, dy: i64| -> f64 {
                let yy = (((y as i64 + dy) % size as i64) + size as i64) % size as i64;
                let xx = (((x as i64 + dx) % size as i64) + size as i64) % size as i64;
                h[(yy as u32 * size + xx as u32) as usize]
            };
            let gx = (at(1, 0) - at(-1, 0)) * 5.0;
            let gy = (at(0, 1) - at(0, -1)) * 5.0;
            let (nx, ny, nz) = normalize3(-gx, -gy, 1.0);
            let i = ((y * size + x) * 4) as usize;
            normal[i] = ((nx * 0.5 + 0.5) * 255.0) as u8;
            normal[i + 1] = ((ny * 0.5 + 0.5) * 255.0) as u8;
            normal[i + 2] = ((nz * 0.5 + 0.5) * 255.0) as u8;
            normal[i + 3] = 255;
        }
    }
    BrassTextures { normal, orm }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_atlas_has_the_right_byte_size() {
        let mut rng = Rng::new(1);
        let atlas = bake_particle_atlas(&mut rng, 64);
        assert_eq!(atlas.data.len(), 64 * 64 * 4);
        assert_eq!(atlas.cols, 4);
    }

    #[test]
    fn particle_atlas_gutter_is_transparent() {
        let mut rng = Rng::new(1);
        let atlas = bake_particle_atlas(&mut rng, 64);
        // top-left texel of the whole atlas is inside every tile's 1px gutter.
        assert_eq!(atlas.data[3], 0);
    }

    #[test]
    fn particle_atlas_is_deterministic() {
        let mut rng1 = Rng::new(777);
        let mut rng2 = Rng::new(777);
        let a1 = bake_particle_atlas(&mut rng1, 32);
        let a2 = bake_particle_atlas(&mut rng2, 32);
        assert_eq!(a1.data, a2.data);
    }

    #[test]
    fn decal_atlas_has_the_right_byte_sizes() {
        let mut rng = Rng::new(2);
        let atlas = bake_decal_atlas(&mut rng, 64);
        assert_eq!(atlas.albedo.len(), 64 * 64 * 4);
        assert_eq!(atlas.normal.len(), 64 * 64 * 4);
        assert_eq!(atlas.orm.len(), 64 * 64 * 4);
    }

    #[test]
    fn decal_normal_alpha_is_always_opaque() {
        let mut rng = Rng::new(3);
        let atlas = bake_decal_atlas(&mut rng, 32);
        for chunk in atlas.normal.chunks_exact(4) {
            assert_eq!(chunk[3], 255);
        }
    }

    #[test]
    fn brass_textures_have_the_right_byte_sizes() {
        let mut rng = Rng::new(4);
        let brass = bake_brass_textures(&mut rng, 32);
        assert_eq!(brass.normal.len(), 32 * 32 * 4);
        assert_eq!(brass.orm.len(), 32 * 32 * 4);
    }

    #[test]
    fn contact_ao_peaks_inside_the_bore() {
        assert_eq!(contact_ao(0.1, 0.2, 0.4), 0.4);
        assert!(contact_ao(0.5, 0.2, 0.4) < 0.4);
    }
}
