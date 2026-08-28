//! Ported from Claude-of-Duty `src/ai/textures.js:1-951`.
//!
//! AI — procedural material set for the enemy characters.
//!
//! Tiling PBR sets, all generated on the CPU at boot from tileable value
//! noise: camouflage ripstop cloth (one bake per uniform pattern), cordura
//! nylon webbing, laminated plate-carrier shell, skin, glass-filled polymer,
//! parkerised steel and boot rubber. Each set is albedo (sRGB) + tangent
//! normal (Sobel of a height field) + a packed ORM (r = AO, g = roughness,
//! b = metalness), which is exactly the layout `MeshStandardMaterial` samples
//! when the same texture is bound to aoMap / roughnessMap / metalnessMap.
//!
//! TWO-SCALE CAMOUFLAGE — a single tile cannot carry both a 0.4 m macro blotch
//! and a 1.5 mm weave: that is a 300:1 frequency ratio, which needs a 2 k map
//! and seconds of CPU bake. So the system is split the way a shipping engine
//! splits it. The *base* tile is large ([`CLOTH_TILE`] = 0.78 m of cloth over
//! 512 px = 1.5 mm/texel) and carries the macro blotch field, the 3 cm pixel
//! layer, panel seams and folds — everything that has to survive at 25 m. A
//! second, small *detail* tile (5 cm over 512 px = 0.1 mm/texel) carries the
//! weave, the ripstop lattice and the nylon ribbing, and is blended in inside
//! the shader as a tangent-space normal plus a roughness delta. Both scales
//! are therefore present at every distance, and the macro pattern is not
//! averaged into flat tan by the mip chain.
//!
//! # Why this does not reuse [`crate::materials`]
//!
//! [`crate::materials::noise`] is a port of `src/materials/glsl/noise.js` —
//! sin-free Dave-Hoskins hashes evaluated analytically in a *shader*. The
//! [`TileNoise`] below is a completely different generator: a JS-side value
//! noise over a 4096-entry random table drawn from the [`crate::rng::Rng`]
//! stream at construction. Same words ("tileable value noise"), different
//! algorithm, different numbers — there is nothing to share. Likewise
//! [`crate::materials::bake`] bakes at texel *centres* (`(x + 0.5) / size`)
//! into `f32` textures with a `relief / world_size` Sobel; this file bakes at
//! texel *corners* (`x / size`) into 8-bit `Uint8Array`s with a
//! `normal_scale * 0.17` Sobel and V8's [`math_hypot3`]. Folding the two
//! together would change both. They are two independent procedural texture
//! systems in the same game and this port keeps them that way.
//!
//! # Storage width is part of the algorithm
//!
//! The source uses `Float32Array` for the noise table ([`TileNoise::tab`]) and
//! for both bakes' height/roughness scratch buffers, and `Uint8Array` for the
//! three output maps. Every one of those rounds on store and is read back
//! rounded, so this port reproduces the widths exactly: `Vec<f32>` scratch
//! widened to `f64` for the arithmetic, and [`to_u8`] for the 8-bit stores.
//! Porting the scratch as `f64` moves the Sobel normals by whole 8-bit steps.
//!
//! # What is not ported, and why
//!
//! `dataTexture()` (`textures.js:98-108`), `SoldierMaterials.get`,
//! `_attachShader`, `glass` and `dispose` (`textures.js:815-950`) are
//! Three.js: a `THREE.DataTexture` wrapper, `MeshStandardMaterial`
//! construction, an `onBeforeCompile` GLSL injection and a program cache key.
//! None of that has a CPU form to check against — GLSL held in a JS string has
//! no oracle at all. What survives the boundary is *data*, and that is ported:
//! the sampler settings `dataTexture` sets ([`TextureData`]), the silhouette
//! rim uniform ([`RIM`], [`rim_uniform`]), the detail-blend uniform
//! ([`DetailBlend`]) and the goggle-glass material constants ([`GLASS`]). The
//! two shader bodies themselves are transcribed verbatim into the doc comments
//! on [`rim_uniform`] and [`DetailBlend`] so the render/`gpu-backend`
//! workstream that eventually writes the WGSL has the source in front of it.
//!
//! `this.bakeMs` (`textures.js:537, 807`) is `performance.now()` — wall-clock,
//! deliberately not ported.
//!
//! # Determinism
//!
//! [`SoldierMaterials::new`] takes exactly one `rng.fork()` (`textures.js:536`)
//! and nothing else in this file draws. Preserve that fork.

use std::f64::consts::PI;

use crate::rng::Rng;

// ---------------------------------------------------------------------------
// Tileable value noise — `textures.js:30-79`.
// ---------------------------------------------------------------------------

/// Tileable value noise over a 4096-entry random table.
///
/// **`tab` is a `Float32Array` in the source** (`textures.js:32`): every
/// `rng.float()` is an `f64` rounded to `f32` on store, and the interpolation
/// reads the rounded value back. Note the top of the range actually reaches
/// `1.0` — `Rng::float`'s maximum draw is `(2^32 - 1) / 2^32`, which rounds up
/// to exactly `1.0f32` — so `n2` returns `[0, 1]` inclusive, not `[0, 1)`.
#[derive(Debug, Clone)]
pub struct TileNoise {
    /// `new Float32Array(4096)`, filled with `rng.float()`.
    pub tab: Vec<f32>,
    /// `new Uint16Array(4096)`, filled with `rng.int(0, 4095)`.
    pub perm: Vec<u16>,
}

impl TileNoise {
    /// `TileNoise`'s table length, both arrays (`textures.js:32-35`).
    pub const TABLE_LEN: usize = 4096;

    /// `fbm`'s defaulted `oct` argument (`textures.js:57`). No call site in
    /// this file uses it — every one passes `oct` explicitly — but it is part
    /// of the signature, so it is named rather than lost.
    pub const FBM_DEFAULT_OCT: i32 = 4;
    /// `fbm`'s defaulted `gain` argument (`textures.js:57`). This one *is*
    /// used: every `fbm` call except `camoTexel`'s four macro fields and its
    /// fine dot layer relies on it.
    pub const FBM_DEFAULT_GAIN: f64 = 0.5;
    /// `ridge`'s defaulted `oct` argument (`textures.js:69`). Unused by every
    /// call site in this file, same as `FBM_DEFAULT_OCT`.
    pub const RIDGE_DEFAULT_OCT: i32 = 3;

    /// `new TileNoise(rng)` (`textures.js:31-36`). Draw order is the contract:
    /// 4096 `float()`s, then 4096 `int(0, 4095)`s.
    pub fn new(rng: &mut Rng) -> Self {
        let mut tab = Vec::with_capacity(Self::TABLE_LEN);
        for _ in 0..Self::TABLE_LEN {
            // `Float32Array` store — the f64 draw is rounded here, and this is
            // the value every later interpolation sees.
            tab.push(rng.float() as f32);
        }
        let mut perm = Vec::with_capacity(Self::TABLE_LEN);
        for _ in 0..Self::TABLE_LEN {
            perm.push(rng.int(0, 4095) as u16);
        }
        TileNoise { tab, perm }
    }

    /// `_h(ix, iy, period)` (`textures.js:38-43`) — the wrapped lattice hash.
    ///
    /// `ix`/`iy` are `i32` here where the source passes doubles from
    /// `Math.floor`; they are always integral and always well inside `i32`
    /// (the widest lattice this file walks is `8.4 * 320`). `period | 0` is
    /// JS `ToInt32`, and `((ix % p) + p) % p` is the double-modulo that makes
    /// a *negative* lattice coordinate wrap forward instead of indexing
    /// backwards — `garmentRelief` passes `v - 2.2`, so negatives are real,
    /// not hypothetical. Rust's `%` on integers is the same truncated
    /// remainder JS's is, so the expression is transcribed literally rather
    /// than collapsed to `rem_euclid`.
    pub fn h(&self, ix: i32, iy: i32, period: f64) -> f64 {
        let p = period as i32;
        let x = ((ix % p) + p) % p;
        let y = ((iy % p) + p) % p;
        let perm = i32::from(self.perm[((x * 73 + y * 151) & 4095) as usize]);
        f64::from(self.tab[((perm + x * 31 + y * 17) & 4095) as usize])
    }

    /// Value noise on a lattice of `period` cells over the unit tile.
    /// `n2` (`textures.js:46-55`).
    pub fn n2(&self, u: f64, v: f64, period: f64) -> f64 {
        let x = u * period;
        let y = v * period;
        let ixf = x.floor();
        let iyf = y.floor();
        let fx = x - ixf;
        let fy = y - iyf;
        let sx = fx * fx * (3.0 - 2.0 * fx);
        let sy = fy * fy * (3.0 - 2.0 * fy);
        let (ix, iy) = (ixf as i32, iyf as i32);
        let a = self.h(ix, iy, period);
        let b = self.h(ix + 1, iy, period);
        let c = self.h(ix, iy + 1, period);
        let d = self.h(ix + 1, iy + 1, period);
        (a + (b - a) * sx) * (1.0 - sy) + (c + (d - c) * sx) * sy
    }

    /// `fbm(u, v, period, oct = 4, gain = 0.5)` (`textures.js:57-66`).
    /// Both octave frequency and octave *period* double together, which is
    /// what keeps the whole stack tileable rather than just its base octave.
    pub fn fbm(&self, u: f64, v: f64, period: f64, oct: i32, gain: f64) -> f64 {
        let mut a = 1.0;
        let mut s = 0.0;
        let mut norm = 0.0;
        let mut p = period;
        for _ in 0..oct {
            s += a * self.n2(u, v, p);
            norm += a;
            a *= gain;
            p *= 2.0;
        }
        s / norm
    }

    /// Ridged noise for fibre and scratch structure. `ridge(u, v, period,
    /// oct = 3)` (`textures.js:69-78`). The gain is hard-coded `0.55` in the
    /// source, unlike [`TileNoise::fbm`]'s parameterised one.
    pub fn ridge(&self, u: f64, v: f64, period: f64, oct: i32) -> f64 {
        let mut a = 1.0;
        let mut s = 0.0;
        let mut norm = 0.0;
        let mut p = period;
        for _ in 0..oct {
            s += a * (1.0 - (self.n2(u, v, p) * 2.0 - 1.0).abs());
            norm += a;
            a *= 0.55;
            p *= 2.0;
        }
        s / norm
    }
}

// ---------------------------------------------------------------------------
// helpers — `textures.js:85-96`, `280-281`, `392`.
// ---------------------------------------------------------------------------

/// Linear -> sRGB, scaled to the 0-255 byte range. `srgb` (`textures.js:85-88`).
///
/// Returns an `f64`, not a `u8`: the source's `srgb()` also returns a double
/// and the 8-bit truncation happens at the `Uint8Array` store (see [`to_u8`]).
/// Keeping them separate is what lets a test pin the pre-quantisation value.
pub fn srgb(v: f64) -> f64 {
    let c = if v <= 0.0 {
        0.0
    } else if v >= 1.0 {
        1.0
    } else {
        v
    };
    (if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }) * 255.0
}

/// `smooth(e0, e1, x)` (`textures.js:90-93`) — a smoothstep that is *not*
/// guarded against `e1 == e0`.
///
/// `Math.min(1, Math.max(0, t))` is transcribed as `f64::clamp`, not
/// `.max(0.0).min(1.0)`: Rust's `max`/`min` swallow a `NaN` operand where JS's
/// `Math.max`/`Math.min` propagate it, but `f64::clamp` propagates `NaN` the
/// same way JS does. No call site in this file passes `e0 == e1` (the
/// narrowest `ridgeLine` width is `0.009`), so the divergence is unreachable —
/// but the version that cannot diverge costs nothing.
pub fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `mix(a, b, t)` (`textures.js:95`).
pub fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// `mix3(a, b, t)` (`textures.js:96`).
pub fn mix3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        mix(a[0], b[0], t),
        mix(a[1], b[1], t),
        mix(a[2], b[2], t),
    ]
}

/// Distance from the centre of a repeating cell, in cell units: `0` on the
/// line, `0.5` half a cell away. `cellDist` (`textures.js:280`).
///
/// Rust's `%` on floats is JS's `%` — a truncated remainder that keeps the
/// dividend's sign — so the source's `(((x % 1) + 1) % 1)` normalisation
/// transcribes literally.
pub fn cell_dist(x: f64) -> f64 {
    (((x % 1.0) + 1.0) % 1.0 - 0.5).abs()
}

/// A THIN feature `w` cell-units wide, centred on the cell line.
/// `ridgeLine` (`textures.js:281`).
///
/// Note the **descending** smoothstep: `smooth(w, 0, d)`, edges reversed, `1`
/// at `d == 0` and `0` at `d >= w`. The source's own doc (`textures.js:272-279`)
/// says writing it the other way round — `smooth(0.5, 0.47, d)` — is what once
/// turned the ripstop cloth into corduroy, so the argument order here is
/// load-bearing.
pub fn ridge_line(d: f64, w: f64) -> f64 {
    smooth(w, 0.0, d)
}

/// Rec.709 relative luminance. `lum3` (`textures.js:392`).
pub fn lum3(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// JS `Uint8Array[i] = v`, i.e. ECMAScript `ToUint8`: truncate toward zero,
/// then take the result **modulo 256**. It is not a clamp — `Uint8Array` is
/// not `Uint8ClampedArray`, and `-1.5` stores as `255`, not `0`. Verified
/// against Node 24: `[254.999, -0.4, -1.5, 255.0, 256.7, NaN]` stores as
/// `[254, 0, 255, 255, 0, 0]`.
///
/// Every write site in this file is proven in-range by construction (`srgb`
/// clamps to `[0, 255]`; every `ao`/`rough`/`metal` expression is bounded in
/// `[0, 1]`; the normal components are unit), so the modulo never actually
/// wraps — but the wrap is the semantics, and a Rust `as u8` cast *saturates*
/// instead, which is a different function.
pub fn to_u8(v: f64) -> u8 {
    if !v.is_finite() {
        return 0;
    }
    // `as i64` saturates for out-of-i64 magnitudes where ToUint8 would keep
    // reducing; unreachable at every call site here, and there is no
    // finite-f64 modulo-256 that is both exact and cheap.
    (v.trunc() as i64).rem_euclid(256) as u8
}

/// V8's `Math.hypot`, transcribed (`textures.js:155`, `204`).
///
/// **This is not `sqrt(x*x + y*y + z*z)`.** V8 normalises by the largest
/// magnitude and Kahan-compensates the sum of squares, which rounds
/// differently. Measured under Node 24 over 200 000 random `(x, y, 1)`
/// triples of the shape the Sobel actually produces: the naive form disagreed
/// with `Math.hypot` on 50 738 of them (25 %), this form on 0. A quarter of
/// every normal map's texels would have been wrong.
pub fn math_hypot3(x: f64, y: f64, z: f64) -> f64 {
    let args = [x.abs(), y.abs(), z.abs()];
    // V8 returns Infinity as soon as it sees a non-finite magnitude, before
    // any summation. Unreachable here (`z` is always exactly 1.0 and the
    // Sobel deltas are finite), transcribed anyway.
    if args.iter().any(|n| n.is_infinite()) {
        return f64::INFINITY;
    }
    let mut max = 0.0f64;
    for n in args {
        if n > max {
            max = n;
        }
    }
    if max == 0.0 {
        max = 1.0;
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for n in args {
        let n = n / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

// ---------------------------------------------------------------------------
// Texel buffers — the CPU half of `dataTexture` (`textures.js:98-108`).
// ---------------------------------------------------------------------------

/// One baked RGBA tile: the `Uint8Array` `dataTexture()` wraps, plus the two
/// sampler settings that actually vary per call. The rest of what
/// `dataTexture` sets is fixed for every texture in this file and is recorded
/// as associated constants rather than dead per-instance fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureData {
    /// Tile edge length in texels; the buffer is `size * size * 4` bytes.
    pub size: u32,
    /// RGBA8, row-major. Row `y`, column `x` starts at `(y * size + x) * 4`.
    pub pixels: Vec<u8>,
    /// `t.colorSpace = srgbSpace ? THREE.SRGBColorSpace : THREE.NoColorSpace`.
    pub srgb: bool,
    /// `t.anisotropy = aniso`.
    pub anisotropy: u32,
}

impl TextureData {
    /// `t.wrapS = t.wrapT = THREE.RepeatWrapping` — every tile in this file
    /// repeats, which is what the whole periodic-noise design is for.
    pub const WRAP_REPEAT: bool = true;
    /// `t.generateMipmaps = true`.
    pub const GENERATE_MIPMAPS: bool = true;
    /// `t.minFilter = THREE.LinearMipmapLinearFilter`.
    pub const MIN_FILTER: &'static str = "LinearMipmapLinearFilter";
    /// `t.magFilter = THREE.LinearFilter`.
    pub const MAG_FILTER: &'static str = "LinearFilter";

    /// The RGBA quadruple at `(x, y)`.
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.size + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

/// The three maps [`bake`] produces (`textures.js:164-168`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakedSet {
    /// sRGB colour, alpha `255`.
    pub albedo: TextureData,
    /// `r` = AO, `g` = roughness, `b` = metalness, `a` = 255.
    pub orm: TextureData,
    /// Tangent-space normal `* 0.5 + 0.5`, `a` = 255.
    pub normal: TextureData,
}

/// The per-texel out-parameter every generator fills: `out.rgb` (linear
/// albedo), `out.h` (height, metres-ish), `out.rough`, `out.metal`, `out.ao`
/// (`textures.js:110-114`, `120`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Texel {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub h: f64,
    pub rough: f64,
    pub metal: f64,
    pub ao: f64,
}

impl Texel {
    /// The per-texel reset [`bake`] performs before every `fn` call
    /// (`textures.js:124-128`), which is also the initial value at
    /// `textures.js:120`.
    pub const BAKE_RESET: Texel = Texel {
        r: 0.5,
        g: 0.5,
        b: 0.5,
        h: 0.0,
        rough: 0.7,
        metal: 0.0,
        ao: 1.0,
    };

    /// `measureCamo`'s scratch initialiser (`textures.js:396`) — a *different*
    /// set of defaults from [`Texel::BAKE_RESET`], and not reset between
    /// samples. `camoTexel` overwrites every field, so it never shows; the
    /// difference is preserved rather than tidied away.
    pub const MEASURE_INIT: Texel = Texel {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        h: 0.0,
        rough: 0.0,
        metal: 0.0,
        ao: 1.0,
    };
}

/// `fn(u, v, out, x, y)` — the per-texel generator [`bake`] runs
/// (`textures.js:129`). No generator in this file reads `x`/`y`, but the
/// source passes them, so they stay in the signature.
pub type BakeFn<'a> = dyn FnMut(f64, f64, &mut Texel, u32, u32) + 'a;

/// `fn(u, v, out)` — the per-texel generator [`bake_detail`] runs
/// (`textures.js:185`). Only `out.h` and `out.rough` are read back.
pub type BakeDetailFn<'a> = dyn FnMut(f64, f64, &mut Texel) + 'a;

/// The Sobel slope constant, `k = normalScale * 0.17` (`textures.js:145`,
/// `193`). The source's comment: "the Sobel kernel already sums ~8 neighbour
/// deltas; keep the slope sane or every fabric turns to corduroy".
const SOBEL_SLOPE: f64 = 0.17;

/// Wrapped height read — `at(x, y)` (`textures.js:142`, `192`), the tile-
/// seamless neighbour fetch the Sobel needs at the edges. Reads the `f32`
/// scratch and widens; the rounding on store is part of the algorithm.
fn wrapped_h(height: &[f32], size: i32, x: i32, y: i32) -> f64 {
    let yi = ((y % size) + size) % size;
    let xi = ((x % size) + size) % size;
    f64::from(height[(yi * size + xi) as usize])
}

/// Run a per-texel generator over a tile and pack the three maps.
/// `bake(size, fn, aniso, normalScale = 1)` (`textures.js:115-169`).
///
/// UVs are texel **corners** (`x / size`), not centres — unlike
/// [`crate::materials::bake`], which uses `(x + 0.5) / size`. Both are their
/// own source's convention.
pub fn bake(size: u32, f: &mut BakeFn<'_>, aniso: u32, normal_scale: f64) -> BakedSet {
    let n = (size as usize) * (size as usize);
    let mut alb = vec![0u8; n * 4];
    let mut orm = vec![0u8; n * 4];
    let mut nrm = vec![0u8; n * 4];
    // `new Float32Array(size * size)` — see the module doc on storage width.
    let mut height = vec![0f32; n];

    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) as usize;
            let mut out = Texel::BAKE_RESET;
            f(
                f64::from(x) / f64::from(size),
                f64::from(y) / f64::from(size),
                &mut out,
                x,
                y,
            );
            alb[i * 4] = to_u8(srgb(out.r));
            alb[i * 4 + 1] = to_u8(srgb(out.g));
            alb[i * 4 + 2] = to_u8(srgb(out.b));
            alb[i * 4 + 3] = 255;
            orm[i * 4] = to_u8(out.ao * 255.0);
            orm[i * 4 + 1] = to_u8(out.rough * 255.0);
            orm[i * 4 + 2] = to_u8(out.metal * 255.0);
            orm[i * 4 + 3] = 255;
            height[i] = out.h as f32;
        }
    }

    // Sobel -> tangent normal, wrapping so the tile stays seamless.
    let si = size as i32;
    let at = |x: i32, y: i32| wrapped_h(&height, si, x, y);
    let k = normal_scale * SOBEL_SLOPE;
    for y in 0..size {
        for x in 0..size {
            let (xi, yi) = (x as i32, y as i32);
            // Transcribed with the source's exact term order and grouping —
            // float addition is not associative, so tidying this changes the
            // last bits of every normal.
            let dx = at(xi + 1, yi - 1) + 2.0 * at(xi + 1, yi) + at(xi + 1, yi + 1)
                - at(xi - 1, yi - 1)
                - 2.0 * at(xi - 1, yi)
                - at(xi - 1, yi + 1);
            let dy = at(xi - 1, yi + 1) + 2.0 * at(xi, yi + 1) + at(xi + 1, yi + 1)
                - at(xi - 1, yi - 1)
                - 2.0 * at(xi, yi - 1)
                - at(xi + 1, yi - 1);
            let mut nx = -dx * k;
            let mut ny = -dy * k;
            let mut nz = 1.0;
            let l = math_hypot3(nx, ny, nz);
            nx /= l;
            ny /= l;
            nz /= l;
            let i = (y * size + x) as usize;
            nrm[i * 4] = to_u8((nx * 0.5 + 0.5) * 255.0);
            nrm[i * 4 + 1] = to_u8((ny * 0.5 + 0.5) * 255.0);
            nrm[i * 4 + 2] = to_u8((nz * 0.5 + 0.5) * 255.0);
            nrm[i * 4 + 3] = 255;
        }
    }

    BakedSet {
        albedo: TextureData {
            size,
            pixels: alb,
            srgb: true,
            anisotropy: aniso,
        },
        orm: TextureData {
            size,
            pixels: orm,
            srgb: false,
            anisotropy: aniso,
        },
        normal: TextureData {
            size,
            pixels: nrm,
            srgb: false,
            anisotropy: aniso,
        },
    }
}

/// Bake a *detail* tile: one RGBA texture whose rgb is a tangent normal and
/// whose alpha is a roughness delta around `0.5`.
/// `bakeDetail(size, fn, aniso, normalScale = 1)` (`textures.js:177-212`).
///
/// This is the high-frequency half of the two-scale system — 5 cm of cloth
/// over 512 px, so a 1.5 mm thread is 15 texels wide and still there when the
/// base tile has run out of resolution.
pub fn bake_detail(
    size: u32,
    f: &mut BakeDetailFn<'_>,
    aniso: u32,
    normal_scale: f64,
) -> TextureData {
    let n = (size as usize) * (size as usize);
    // Both scratch buffers are `Float32Array` in the source.
    let mut height = vec![0f32; n];
    let mut rough = vec![0f32; n];

    for y in 0..size {
        for x in 0..size {
            // `out = { h: 0, rough: 0 }`, reset per texel (textures.js:180-184).
            let mut out = Texel {
                h: 0.0,
                rough: 0.0,
                ..Texel::BAKE_RESET
            };
            f(
                f64::from(x) / f64::from(size),
                f64::from(y) / f64::from(size),
                &mut out,
            );
            let i = (y * size + x) as usize;
            height[i] = out.h as f32;
            rough[i] = out.rough as f32;
        }
    }

    let mut buf = vec![0u8; n * 4];
    let si = size as i32;
    let at = |x: i32, y: i32| wrapped_h(&height, si, x, y);
    let k = normal_scale * SOBEL_SLOPE;
    for y in 0..size {
        for x in 0..size {
            let (xi, yi) = (x as i32, y as i32);
            let dx = at(xi + 1, yi - 1) + 2.0 * at(xi + 1, yi) + at(xi + 1, yi + 1)
                - at(xi - 1, yi - 1)
                - 2.0 * at(xi - 1, yi)
                - at(xi - 1, yi + 1);
            let dy = at(xi - 1, yi + 1) + 2.0 * at(xi, yi + 1) + at(xi + 1, yi + 1)
                - at(xi - 1, yi - 1)
                - 2.0 * at(xi, yi - 1)
                - at(xi + 1, yi - 1);
            let nx = -dx * k;
            let ny = -dy * k;
            let nz = 1.0;
            let l = math_hypot3(nx, ny, nz);
            let i = (y * size + x) as usize;
            // `bake` divides in place and then scales; `bakeDetail` writes
            // `nx / l * 0.5 + 0.5` inline. Same value, both transcribed as
            // written.
            buf[i * 4] = to_u8((nx / l * 0.5 + 0.5) * 255.0);
            buf[i * 4 + 1] = to_u8((ny / l * 0.5 + 0.5) * 255.0);
            buf[i * 4 + 2] = to_u8((nz / l * 0.5 + 0.5) * 255.0);
            // The one explicitly clamped store in the file — the roughness
            // delta is signed and unbounded.
            buf[i * 4 + 3] =
                to_u8((0.0f64).max((255.0f64).min((f64::from(rough[i]) * 0.5 + 0.5) * 255.0)));
        }
    }

    TextureData {
        size,
        pixels: buf,
        srgb: false,
        anisotropy: aniso,
    }
}

// ---------------------------------------------------------------------------
// Pattern definitions — `textures.js:218-270`.
// ---------------------------------------------------------------------------

/// Metres of cloth that map to one base tile (`textures.js:228`).
pub const CLOTH_TILE: f64 = 0.78;

/// One camouflage pattern's five tonal families and macro parameters.
///
/// Family luminances: pale 0.335 / base 0.275 / mid 0.205 / dark 0.125 /
/// olive 0.19 — a 2.7:1 macro value ratio inside the 0.18-0.32 window real
/// printed multicam occupies, with the dark blotches allowed below it. The
/// families are the *pattern*; [`ClothBudget`] is what the finished map is
/// remapped onto, so these five colours only set hue and ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamoConfig {
    /// Target mean linear luminance for this pattern (`CLOTH_BUDGET.mean`
    /// override — see [`budget_for`]).
    pub budget: f64,
    pub pale: [f64; 3],
    pub base: [f64; 3],
    pub mid: [f64; 3],
    pub dark: [f64; 3],
    pub olive: [f64; 3],
    /// `macro` in the source (`textures.js:244`) — the macro blotch lattice
    /// period. `macro` is a Rust keyword, hence the trailing underscore.
    pub macro_: f64,
    /// Domain-warp amplitude for the blotch field.
    pub warp: f64,
}

/// `CAMO.arid` (`textures.js:236-246`) — desert multicam is the pale one: it
/// gets the top of the window.
pub const CAMO_ARID: CamoConfig = CamoConfig {
    budget: 0.104,
    pale: [0.382, 0.318, 0.212],
    base: [0.320, 0.256, 0.163],
    mid: [0.241, 0.186, 0.116],
    dark: [0.146, 0.111, 0.072],
    olive: [0.176, 0.190, 0.116],
    macro_: 2.0,
    warp: 0.15,
};

/// `CAMO.woodland` (`textures.js:247-257`) — olive drab in the field sits well
/// under desert tan.
pub const CAMO_WOODLAND: CamoConfig = CamoConfig {
    budget: 0.092,
    pale: [0.354, 0.340, 0.230],
    base: [0.246, 0.259, 0.170],
    mid: [0.174, 0.190, 0.126],
    dark: [0.104, 0.110, 0.083],
    olive: [0.210, 0.196, 0.132],
    macro_: 3.0,
    warp: 0.17,
};

/// `CAMO.urban` (`textures.js:258-269`) — wolf grey / near-black urban kit:
/// the darkest of the three, and the one that reads as a plaster mannequin if
/// it is allowed anywhere near 0.2.
pub const CAMO_URBAN: CamoConfig = CamoConfig {
    budget: 0.083,
    pale: [0.330, 0.334, 0.342],
    base: [0.226, 0.230, 0.239],
    mid: [0.150, 0.154, 0.163],
    dark: [0.078, 0.079, 0.088],
    olive: [0.190, 0.188, 0.182],
    macro_: 2.0,
    warp: 0.14,
};

/// The `CAMO` object (`textures.js:230-270`), in declaration order. Order is
/// the source's; nothing indexes it numerically, but a table whose order
/// silently changes is exactly the trap the port recipe names.
pub const CAMO: [(&str, CamoConfig); 3] = [
    ("arid", CAMO_ARID),
    ("woodland", CAMO_WOODLAND),
    ("urban", CAMO_URBAN),
];

/// `CAMO[name] ?? CAMO.arid` (`textures.js:549`) — an unrecognised pattern
/// name silently bakes arid rather than failing.
pub fn camo_config(name: &str) -> CamoConfig {
    CAMO.iter()
        .find(|(n, _)| *n == name)
        .map_or(CAMO_ARID, |(_, c)| *c)
}

/// Garment-scale relief for the base cloth tile: felled panel seams with their
/// stitch beads, pocket-edge creases and the broad wrinkle field. The weave
/// itself is far too fine for this tile and lives in the detail map.
/// `garmentRelief` (`textures.js:288-319`).
pub fn garment_relief(nz: &TileNoise, u: f64, v: f64) -> f64 {
    // horizontal felled seams every 1/4 tile (~20 cm): a 2 mm sunk channel
    // with a 4 mm raised felled lip beside it
    let drift = (nz.fbm(u, v, 3.0, 2, 0.5) - 0.5) * 0.22;
    let sa = cell_dist(v * 4.0 + drift);
    let mut h =
        -ridge_line(sa, 0.013) * 0.62 + (ridge_line(sa, 0.030) - ridge_line(sa, 0.016)) * 0.34;
    // vertical seams, sparser (~31 cm). Note the swapped argument order:
    // `fbm(v + 4.1, u, ...)`, not `fbm(u, v + 4.1, ...)`.
    let drift2 = (nz.fbm(v + 4.1, u, 3.0, 2, 0.5) - 0.5) * 0.26;
    let sb = cell_dist(u * 2.5 + drift2);
    h += -ridge_line(sb, 0.009) * 0.46 + (ridge_line(sb, 0.022) - ridge_line(sb, 0.011)) * 0.22;
    // stitch beads, only on the seams themselves, 9 mm apart
    let on_seam = ridge_line(sa, 0.020).max(ridge_line(sb, 0.014));
    h += on_seam * (0.5 + 0.5 * ((u + v) * 520.0).sin()) * 0.26;
    // pocket-edge creases: a coarse rectangular lattice, only sometimes present
    let gate = smooth(0.55, 0.72, nz.fbm(u + 1.7, v + 2.3, 3.0, 2, 0.5));
    let pu = cell_dist(u * 3.5 + 0.31);
    let pv = cell_dist(v * 3.0 + 0.17);
    h -= gate * ridge_line(pu, 0.012).max(ridge_line(pv, 0.014)) * 0.55;
    // wrinkles: 8 cm folds plus 3 cm crumple — the low-frequency half of the
    // relief, and the part that actually catches the key light
    h += (nz.fbm(u, v, 10.0, 3, 0.5) - 0.5) * 0.95;
    h += (nz.fbm(u + 5.3, v + 1.9, 26.0, 2, 0.5) - 0.5) * 0.34;
    // 1-2 cm CREASE field. Ridged (not fbm) on purpose: a crease in cloth is a
    // sharp line with a soft valley either side, and it is the one relief
    // scale that still separates a sleeve from a rendered tube at 25 m.
    // `v - 2.2` is why `TileNoise::h` has to wrap negative lattice indices.
    let crease = nz.ridge(u + 3.1, v - 2.2, 52.0, 2);
    h += (crease - 0.55) * 0.46;
    h += (nz.ridge(v * 0.7 + 8.4, u * 0.7 + 1.1, 74.0, 2) - 0.55) * 0.22;
    h
}

/// ALBEDO BUDGET for the uniform cloth, in linear luminance
/// (`textures.js:321-365`).
///
/// The bake used to be trusted to land in budget by construction and it did
/// not: measured mean was 0.171 with pale blotches at 0.386 — simultaneously
/// under budget on average and *over* it on the pale family, which is a chalky
/// figure with no dark structure. So the bake is measured and remapped instead
/// of hoped at: the mean is forced onto `mean`, the macro spread is stretched
/// by `contrast`, and every texel is clamped into `[min, max]`.
///
/// WHY 0.104 AND NOT THE 0.20-0.22 REAL-WORLD FIGURE — measured off the
/// shipping frame, not guessed. The environment's *sunlit* surfaces behave
/// like 0.05-0.09 albedo, so a physically-honest 0.21 uniform renders far
/// brighter than sunlit plaster and the soldier reads as a white mannequin.
/// The whole kit therefore comes down to cloth 0.104 desert / 0.092 olive /
/// 0.083 wolf grey, pale blotches capped at 0.152, which puts the finished
/// uniform at 0.075-0.11 linear. The macro window is still 3.8:1
/// (0.040-0.152) so the pattern keeps its internal value structure, and
/// `contrast` is 1.5 so the macro blotches do not flatten as the window
/// narrows. See the source for the full measurement log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClothBudget {
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub contrast: f64,
    pub sat: f64,
}

/// `CLOTH_BUDGET` (`textures.js:365`).
pub const CLOTH_BUDGET: ClothBudget = ClothBudget {
    mean: 0.104,
    min: 0.040,
    max: 0.152,
    contrast: 1.5,
    sat: 1.35,
};

/// The same calibration applied to the rest of the kit (`textures.js:372-384`).
/// `0.104 / 0.205 = 0.51`. The GEAR vertex tints in `soldier.js` express the
/// *hierarchy* (cloth > pouches > webbing > boots) as fractions of their base
/// bake, so when the cloth came down to meet the world's albedo the nylon and
/// laminate bakes had to come with it or the pouches would end up paler than
/// the uniform they are strapped to.
pub const KIT_CAL: f64 = 0.51;

/// `budgetFor(cfg)` (`textures.js:386-390`). Only the MEAN moves per pattern:
/// the window, the contrast stretch and the saturation are shared.
///
/// `None` is the source's `cfg?.budget` yielding `undefined` — both a missing
/// argument and a config object without a `budget` key land here.
pub fn budget_for(cfg: Option<&CamoConfig>) -> ClothBudget {
    let mean = cfg.map_or(CLOTH_BUDGET.mean, |c| c.budget);
    if mean == CLOTH_BUDGET.mean {
        return CLOTH_BUDGET;
    }
    ClothBudget {
        mean,
        ..CLOTH_BUDGET
    }
}

/// `measureCamo`'s return shape (`textures.js:412`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamoMeasure {
    pub mean: f64,
    pub sd: f64,
    pub min: f64,
    pub max: f64,
}

/// `measureCamo`'s defaulted grid resolution (`textures.js:395`). This is the
/// value [`CamoSampler::new`] uses, so it is load-bearing: change it and every
/// remapped texel moves.
pub const MEASURE_N: u32 = 96;

/// Mean/sd of a camo pattern's linear luminance, sampled on a coarse grid.
/// `measureCamo(nz, cfg, n = 96)` (`textures.js:395-413`).
pub fn measure_camo(nz: &TileNoise, cfg: &CamoConfig, n: u32) -> CamoMeasure {
    // One scratch texel reused across the whole grid, exactly as the source
    // does — see `Texel::MEASURE_INIT` for why its defaults differ from the
    // bake's.
    let mut out = Texel::MEASURE_INIT;
    let mut s = 0.0;
    let mut s2 = 0.0;
    let mut mn = f64::INFINITY;
    let mut mx = f64::NEG_INFINITY;
    for y in 0..n {
        for x in 0..n {
            camo_texel(
                nz,
                cfg,
                f64::from(x) / f64::from(n),
                f64::from(y) / f64::from(n),
                &mut out,
            );
            let l = lum3(out.r, out.g, out.b);
            s += l;
            s2 += l * l;
            if l < mn {
                mn = l;
            }
            if l > mx {
                mx = l;
            }
        }
    }
    let nn = f64::from(n) * f64::from(n);
    let mean = s / nn;
    CamoMeasure {
        mean,
        sd: (0.0f64).max(s2 / nn - mean * mean).sqrt(),
        min: mn,
        max: mx,
    }
}

/// The one place the finished cloth texel comes from: raw pattern -> budget.
/// `makeCamoSampler(nz, cfg, B = budgetFor(cfg))` (`textures.js:425-433`).
///
/// The source returns a closure with a `srcMean` property hung off it; Rust
/// gets a struct with a method, which is the same thing said out loud.
#[derive(Debug, Clone, Copy)]
pub struct CamoSampler<'a> {
    nz: &'a TileNoise,
    cfg: &'a CamoConfig,
    budget: ClothBudget,
    /// `fn.srcMean` — the pattern's *pre-remap* mean luminance, reported by
    /// `SoldierMaterials.camoStats[name].was`.
    pub src_mean: f64,
}

impl<'a> CamoSampler<'a> {
    /// `makeCamoSampler(nz, cfg)` with the defaulted `B = budgetFor(cfg)`.
    pub fn new(nz: &'a TileNoise, cfg: &'a CamoConfig) -> Self {
        Self::with_budget(nz, cfg, budget_for(Some(cfg)))
    }

    /// `makeCamoSampler(nz, cfg, B)` with an explicit budget. No call site in
    /// this file passes one; the parameter exists in the source, so it exists
    /// here.
    pub fn with_budget(nz: &'a TileNoise, cfg: &'a CamoConfig, budget: ClothBudget) -> Self {
        let pre = measure_camo(nz, cfg, MEASURE_N);
        CamoSampler {
            nz,
            cfg,
            budget,
            src_mean: pre.mean,
        }
    }

    /// The returned closure's body (`textures.js:427-430`).
    pub fn sample(&self, u: f64, v: f64, out: &mut Texel) {
        camo_texel(self.nz, self.cfg, u, v, out);
        apply_budget(out, self.src_mean, &self.budget);
    }
}

/// Force one baked texel into the budget: recentre the mean, stretch the macro
/// contrast about it, clamp, and push the hue a little further from neutral —
/// a desaturated tan at these values is what makes cloth read as plaster.
/// `applyBudget` (`textures.js:435-450`).
pub fn apply_budget(out: &mut Texel, src_mean: f64, b: &ClothBudget) {
    let l = lum3(out.r, out.g, out.b);
    if l < 1e-6 {
        return;
    }
    let mut t = b.mean + (l - src_mean) * b.contrast;
    t = if t < b.min {
        b.min
    } else if t > b.max {
        b.max
    } else {
        t
    };
    let k = t / l;
    // saturate around the texel's own luminance, then rescale to the target
    // value
    let r = l + (out.r - l) * b.sat;
    let g = l + (out.g - l) * b.sat;
    let bl = l + (out.b - l) * b.sat;
    let l2 = lum3(r, g, bl).max(1e-6);
    let k2 = (l * k) / l2;
    out.r = r * k2;
    out.g = g * k2;
    out.b = bl * k2;
}

/// The raw (pre-budget) camouflage pattern. `camoTexel` (`textures.js:452-491`).
///
/// Two-scale blotch camouflage in the spirit of Multicam / MARPAT: the MACRO
/// field decides which of five tonal families a texel belongs to at 0.2-0.4 m;
/// the fine 3 cm dot layer only modulates the chosen family by +-18 %. That
/// ratio is the whole point — at 25 m the dot layer mips away and what is left
/// is still a blotch pattern with real value contrast, instead of the flat tan
/// a single high-frequency dot field averages to.
pub fn camo_texel(nz: &TileNoise, cfg: &CamoConfig, u: f64, v: f64, out: &mut Texel) {
    let m = cfg.macro_;
    // domain warp so the blotches get organic, elongated shapes instead of the
    // round blobs raw value noise thresholds into
    let wx = nz.fbm(u + 0.31, v + 0.17, m * 2.0, 2, 0.5) - 0.5;
    let wy = nz.fbm(u + 0.73, v + 0.59, m * 2.0, 2, 0.5) - 0.5;
    let mu = u + wx * cfg.warp;
    let mv = v + wy * cfg.warp;

    // ---- macro: four overlapping blotch fields, one per non-base family ----
    let a = nz.fbm(mu + 0.11, mv, m, 2, 0.40);
    let b = nz.fbm(mu, mv + 0.37, m, 2, 0.40);
    let c = nz.fbm(mu + 0.61, mv + 0.23, m + 1.0, 2, 0.44);
    let d = nz.fbm(mu + 0.29, mv + 0.83, m + 2.0, 2, 0.44);

    // narrow transition bands: printed camo has hard edges between families,
    // and a soft ramp is exactly what averages to flat tan at distance
    let mut col = cfg.base;
    col = mix3(col, cfg.pale, smooth(0.535, 0.585, a));
    col = mix3(col, cfg.olive, smooth(0.555, 0.605, b) * 0.9);
    col = mix3(col, cfg.mid, smooth(0.515, 0.565, c));
    col = mix3(col, cfg.dark, smooth(0.605, 0.655, d));

    // ---- fine 3 cm pixel/dot layer, low amplitude --------------------------
    let f1 = smooth(0.40, 0.60, nz.fbm(u + 3.7, v + 1.3, 24.0, 2, 0.35));
    let f2 = smooth(0.52, 0.70, nz.n2(u + 7.1, v + 2.9, 48.0));
    let fine = 0.88 + 0.26 * f1 - 0.12 * f2;

    let h = garment_relief(nz, u, v);
    out.h = h;
    // sun bleaching on the crowns of the folds, dye pooling in the creases
    let bleach = 1.0 + 0.05 * smooth(-0.2, 0.9, h);
    out.r = col[0] * fine * bleach;
    out.g = col[1] * fine * bleach;
    out.b = col[2] * fine * bleach * 0.99;
    // matte ripstop: 0.86-0.95, roughest where the nap is raised
    out.rough = 0.905 - 0.045 * smooth(-0.6, 0.8, h) + 0.035 * (nz.fbm(u, v, 9.0, 3, 0.5) - 0.5);
    out.metal = 0.0;
    out.ao = 0.82 + 0.18 * smooth(-0.7, 0.7, h);
}

// ---------------------------------------------------------------------------
// Silhouette preservation — `textures.js:497-522`.
// ---------------------------------------------------------------------------

/// VIEW-DEPENDENT EDGE DARKENING — the second half of the "read as a person
/// against a blown sky" problem, and the half albedo cannot solve.
///
/// A character standing against a 0.94-linear sky loses its outline for two
/// reasons: the sky is brighter than anything physical the figure can be, and
/// bloom bleeds the sky *over* the last few pixels of him. Both are fixed by
/// the same thing a real photograph gets for free — a body is a closed
/// surface, so at its outline you are looking along the surface, through the
/// full thickness of fabric nap, dust and self-shadowing. Almost nothing comes
/// back.
///
/// - `strength` 0.62 — measured: a 0.09-albedo uniform against 0.94 sky ends
///   at ~0.10 screen linear, i.e. > 80 % outline contrast.
/// - `edge` 0.42 — `|N.V| < 0.58`, roughly the outer 18 % of a limb's width,
///   so it reads as form shading rather than a drawn line.
/// - `power` 1.9 — soft enough that it never becomes a cartoon outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RimParams {
    pub strength: f64,
    pub edge: f64,
    pub power: f64,
}

/// `RIM` (`textures.js:522`).
pub const RIM: RimParams = RimParams {
    strength: 0.62,
    edge: 0.42,
    power: 1.9,
};

/// The `owCharRim` uniform `_attachShader` builds (`textures.js:868-873`):
/// `vec4(RIM.strength * rimScale, RIM.edge, RIM.power, 0)`.
///
/// The shader that consumes it has no CPU oracle — it is GLSL spliced into
/// Three's `MeshStandardMaterial` before `#include <opaque_fragment>`
/// (`textures.js:913-921`). Transcribed here so the render workstream that
/// writes the WGSL has it:
///
/// ```glsl
/// {
///   float owF = 1.0 - abs( dot( normalize( vViewPosition ), nonPerturbedNormal ) );
///   float owEdge = pow( smoothstep( owCharRim.y, 1.0, owF ), owCharRim.z );
///   outgoingLight *= 1.0 - owCharRim.x * owEdge;
/// }
/// ```
///
/// It uses the GEOMETRIC normal, not the detail-perturbed one: perturbing the
/// rim makes the band crawl.
pub fn rim_uniform(rim_scale: f64) -> [f64; 4] {
    [RIM.strength * rim_scale, RIM.edge, RIM.power, 0.0]
}

/// The `owDetailParams` uniform (`textures.js:876-879`): which detail tile to
/// blend, at what tiling scale, and how much of its normal and roughness to
/// take.
///
/// The GLSL this feeds, also with no CPU oracle — two `String.replace`
/// splices into Three's standard fragment shader (`textures.js:894-909`):
///
/// ```glsl
/// // after #include <roughnessmap_fragment>:
/// roughnessFactor = clamp( roughnessFactor +
///   ( texture2D( owDetailTex, vNormalMapUv * owDetailParams.x ).w - 0.5 ) * owDetailParams.z,
///   0.04, 1.0 );
///
/// // replacing #include <normal_fragment_maps>:
/// vec3 owMapN = texture2D( normalMap, vNormalMapUv ).xyz * 2.0 - 1.0;
/// owMapN.xy *= normalScale;
/// owMapN.xy += ( texture2D( owDetailTex, vNormalMapUv * owDetailParams.x ).xy * 2.0 - 1.0 )
///   * owDetailParams.y;
/// normal = normalize( tbn * normalize( owMapN ) );
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetailBlend {
    /// Which entry of [`SoldierMaterials::details`] to sample.
    pub set: &'static str,
    /// `owDetailParams.x` — UV multiplier for the detail tile.
    pub scale: f64,
    /// `owDetailParams.y` — how much of the detail tangent slope to add.
    pub normal: f64,
    /// `owDetailParams.z` — how much of the signed roughness delta to add.
    pub rough: f64,
}

impl DetailBlend {
    /// `d?.scale ?? 8`, `d?.normal ?? 0.7`, `d?.rough ?? 0.2`
    /// (`textures.js:877`).
    pub const DEFAULT_SCALE: f64 = 8.0;
    pub const DEFAULT_NORMAL: f64 = 0.7;
    pub const DEFAULT_ROUGH: f64 = 0.2;
}

/// `SoldierMaterials.glass` (`textures.js:926-943`) — the flat material for
/// goggle lenses and optic glass, as data.
///
/// A goggle lens is the one place a *bright* grazing highlight is correct, so
/// the edge term runs at half strength ([`GLASS.rim_scale`]): enough that the
/// lens rim does not bloom into the sky, not enough to kill the sheen that
/// makes it read glass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassParams {
    pub tint: [f64; 3],
    pub roughness: f64,
    pub metalness: f64,
    pub env_map_intensity: f64,
    pub rim_scale: f64,
}

/// `glass(tint = [0.06, 0.07, 0.08])`'s constants (`textures.js:926-940`).
pub const GLASS: GlassParams = GlassParams {
    tint: [0.06, 0.07, 0.08],
    roughness: 0.11,
    metalness: 0.0,
    env_map_intensity: 1.4,
    rim_scale: 0.5,
};

// ---------------------------------------------------------------------------
// Public: the material set — `textures.js:528-813`.
// ---------------------------------------------------------------------------

/// `SoldierMaterials`'s `opts` (`textures.js:533-535, 548`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoldierOpts {
    /// `opts.size ?? 512`.
    pub size: u32,
    /// `opts.anisotropy ?? 8`.
    pub anisotropy: u32,
    /// `opts.camo ?? ['arid', 'woodland']`. Order is the bake order and the
    /// `sets` insertion order.
    pub camo: Vec<String>,
}

impl Default for SoldierOpts {
    fn default() -> Self {
        SoldierOpts {
            size: 512,
            anisotropy: 8,
            camo: vec!["arid".to_string(), "woodland".to_string()],
        }
    }
}

/// `this.camoStats[name]` (`textures.js:571-577`) — what the finished map
/// actually is, measured over every baked texel rather than hoped at. A camo
/// bake that is never measured drifts every time the noise is touched, and the
/// figure goes chalky without anybody noticing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamoStats {
    pub mean: f64,
    pub sd: f64,
    pub min: f64,
    pub max: f64,
    /// `sample.srcMean` — the pattern's mean *before* the budget remap.
    pub was: f64,
}

/// The full character material set (`textures.js:528-813`), minus the Three.js
/// material cache — see the module doc.
#[derive(Debug, Clone)]
pub struct SoldierMaterials {
    /// `this.sets`, in insertion order: `camo_<name>` for each requested
    /// pattern, then `nylon`, `plate`, `skin`, `polymer`, `steel`, `rubber`.
    pub sets: Vec<(String, BakedSet)>,
    /// `this.details`: `cloth`, then `nylon`.
    pub details: Vec<(String, TextureData)>,
    /// `this.camoStats`, one entry per requested pattern, in the same order.
    pub camo_stats: Vec<(String, CamoStats)>,
}

impl SoldierMaterials {
    /// `dsize = Math.min(512, size)` (`textures.js:765`) — the detail tiles
    /// never exceed 512 px however large the base tile is.
    pub const DETAIL_MAX_SIZE: u32 = 512;

    /// `new SoldierMaterials(rng, opts)` (`textures.js:533-813`).
    ///
    /// Takes exactly one `rng.fork()` and draws nothing else — preserve that.
    #[allow(clippy::too_many_lines)] // one bake closure per material set; splitting them apart from the source's order would make this undiffable
    pub fn new(rng: &mut Rng, opts: &SoldierOpts) -> Self {
        let size = opts.size;
        let aniso = opts.anisotropy;
        let nz = TileNoise::new(&mut rng.fork());

        let mut sets: Vec<(String, BakedSet)> = Vec::new();
        let mut camo_stats: Vec<(String, CamoStats)> = Vec::new();

        // ---- camouflage cloth, one bake per pattern ------------------------
        // Measure first, then bake through the budget remap, then report what
        // the map actually is.
        for name in &opts.camo {
            let cfg = camo_config(name);
            let sample = CamoSampler::new(&nz, &cfg);
            let mut s = 0.0;
            let mut s2 = 0.0;
            let mut mn = f64::INFINITY;
            let mut mx = f64::NEG_INFINITY;
            let mut n = 0u32;
            let baked = {
                let mut f = |u: f64, v: f64, out: &mut Texel, _x: u32, _y: u32| {
                    sample.sample(u, v, out);
                    let l = lum3(out.r, out.g, out.b);
                    s += l;
                    s2 += l * l;
                    n += 1;
                    if l < mn {
                        mn = l;
                    }
                    if l > mx {
                        mx = l;
                    }
                };
                bake(size, &mut f, aniso, 0.9)
            };
            sets.push((format!("camo_{name}"), baked));
            let nf = f64::from(n);
            let mean = s / nf;
            camo_stats.push((
                name.clone(),
                CamoStats {
                    mean,
                    sd: (0.0f64).max(s2 / nf - mean * mean).sqrt(),
                    min: mn,
                    max: mx,
                    was: sample.src_mean,
                },
            ));
        }

        // ---- cordura nylon: webbing, pouches, boot uppers, gloves ----------
        // Base albedo sits at the TOP of the plausible range (0.30) so the
        // assembly can place each piece of kit below it with a vertex tint:
        // pouches 0.19, webbing 0.13, sling 0.12, gloves 0.07, boots 0.055.
        // One material, five values — that internal value hierarchy is what
        // breaks the "one extruded blob" read at 25 m.
        let mut nylon_fn = |u: f64, v: f64, out: &mut Texel, _x: u32, _y: u32| {
            // 1000D cordura at 0.26 m/tile: the basket weave is 1 mm, the
            // binding tape and bar-tacks are the things this tile actually has
            // to carry
            let tu = u * 26.0;
            let tv = v * 26.0;
            let cell = (tu * PI).sin() * (tv * PI).sin();
            let mut h = cell * 0.34;
            h += (nz.fbm(u, v, 120.0, 2, 0.5) - 0.5) * 0.30;
            // PALS ribbing: 6 mm ribs, but only across the patches of the tile
            // that are webbing rather than plain cordura
            let rib = cell_dist(v * 44.0);
            let rib_gate = smooth(0.52, 0.70, nz.fbm(u + 8.3, v, 5.0, 2, 0.5));
            h += rib_gate * (ridge_line(rib, 0.30) - 0.5) * 0.30;
            // binding tape + bar-tacks on the hem rows (every 1/3 tile ~ 87 mm)
            let st = cell_dist(v * 3.0);
            let tape = ridge_line(st, 0.045);
            h += tape * 0.14;
            h += ridge_line(st, 0.020) * (0.5 + 0.5 * (u * 300.0).sin()) * 0.30;
            out.h = h;
            let shade = 0.84 + 0.16 * nz.fbm(u, v, 7.0, 3, 0.5);
            let base = 0.300 * KIT_CAL * shade;
            out.r = base * 1.05;
            out.g = base * 1.0;
            out.b = base * 0.90;
            // thread is paler and shinier than the webbing
            let thr = ridge_line(st, 0.020) * (0.25 + 0.25 * (u * 300.0).sin());
            out.r = mix(out.r, 0.335 * KIT_CAL, thr);
            out.g = mix(out.g, 0.320 * KIT_CAL, thr);
            out.b = mix(out.b, 0.278 * KIT_CAL, thr);
            out.rough =
                0.79 - 0.13 * smooth(-0.4, 0.9, h) + 0.05 * (nz.fbm(u + 2.0, v, 11.0, 2, 0.5) - 0.5);
            out.metal = 0.0;
            out.ao = 0.80 + 0.20 * smooth(-0.5, 1.0, h);
        };
        sets.push((
            "nylon".to_string(),
            bake(size, &mut nylon_fn, aniso, 1.15),
        ));

        // ---- laminated plate-carrier shell / painted helmet shell ----------
        // A carrier is not webbing: it is a laminate over a foam-backed plate,
        // so it is smoother (0.55-0.70) than the cloth around it and darker
        // than the pouches bolted to it. Quilted stitch grid, scuffed high
        // points.
        let mut plate_fn = |u: f64, v: f64, out: &mut Texel, _x: u32, _y: u32| {
            // quilting: a diamond stitch grid pressed into the laminate, and
            // the panels between it bulging over the foam
            let qu = cell_dist(u * 5.0 + v * 2.5);
            let qv = cell_dist(u * -2.5 + v * 5.0);
            let mut h = -(ridge_line(qu, 0.045) + ridge_line(qv, 0.045)) * 0.42;
            h += (1.0 - ridge_line(qu, 0.30).max(ridge_line(qv, 0.30))) * 0.26;
            // laminate grain + abrasion
            let grain = nz.fbm(u, v, 90.0, 3, 0.5);
            h += (grain - 0.5) * 0.24;
            let scuff = smooth(0.62, 0.86, nz.ridge(u * 0.7, v * 2.4, 22.0, 3));
            h -= scuff * 0.18;
            out.h = h;
            // Macro value variation. A carrier is the one part of the kit that
            // is pure flat colour if you let it be, and a flat slab in the
            // middle of the chest is the single loudest "moulded toy" cue on
            // the model: sun fade on the panels that face up, dust settled in
            // the quilting, dried sweat salt along the cummerbund.
            let fade = nz.fbm(u + 3.3, v, 3.0, 3, 0.5);
            let soil = nz.fbm(u + 7.7, v + 2.1, 8.0, 3, 0.5);
            let shade = 0.74 + 0.40 * fade;
            let base = 0.212 * KIT_CAL * shade;
            out.r = base * 1.04;
            out.g = base * 1.0;
            out.b = base * 0.93;
            // ground-in dust and grease darken the low panels
            out.r = mix(out.r, out.r * 0.66, smooth(0.44, 0.68, soil));
            out.g = mix(out.g, out.g * 0.64, smooth(0.44, 0.68, soil));
            out.b = mix(out.b, out.b * 0.60, smooth(0.44, 0.68, soil));
            // scuffs abrade to a paler, rougher grey
            out.r = mix(out.r, 0.283 * KIT_CAL, scuff * 0.7);
            out.g = mix(out.g, 0.274 * KIT_CAL, scuff * 0.7);
            out.b = mix(out.b, 0.258 * KIT_CAL, scuff * 0.7);
            // 0.55-0.72: laminate, markedly smoother than the 0.87-0.92 cloth
            // around it, and rougher again where it has been abraded
            out.rough = 0.590
                + 0.060 * smooth(-0.5, 0.7, -h)
                + 0.09 * scuff
                + 0.05 * smooth(0.44, 0.68, soil)
                + 0.025 * (nz.fbm(u, v + 5.1, 13.0, 2, 0.5) - 0.5);
            out.metal = 0.0;
            out.ao = 0.82 + 0.18 * smooth(-0.7, 0.7, h);
        };
        sets.push((
            "plate".to_string(),
            bake(size, &mut plate_fn, aniso, 1.05),
        ));

        // ---- skin ---------------------------------------------------------
        let mut skin_fn = |u: f64, v: f64, out: &mut Texel, _x: u32, _y: u32| {
            let pores = nz.fbm(u, v, 150.0, 3, 0.5);
            // `macro` in the source; a Rust keyword.
            let macro_n = nz.fbm(u, v, 11.0, 3, 0.5);
            let fine = nz.fbm(u, v, 320.0, 2, 0.5);
            out.h = (pores - 0.5) * 0.5 + (fine - 0.5) * 0.25;
            // Fitzpatrick IV base; per-instance tint shifts it
            let base = [0.295, 0.199, 0.148];
            let flush = [0.330, 0.186, 0.142];
            let mut col = mix3(base, flush, smooth(0.4, 0.75, macro_n));
            // stubble / beard shadow band handled by vertex colour; here just
            // follicle speckle
            let st = nz.fbm(u * 1.3, v * 1.3, 110.0, 2, 0.5);
            col = mix3(col, [0.115, 0.086, 0.074], smooth(0.62, 0.72, st) * 0.5);
            out.r = col[0];
            out.g = col[1];
            out.b = col[2];
            out.rough = 0.50 + 0.16 * macro_n - 0.10 * pores;
            out.metal = 0.0;
            out.ao = 0.9 + 0.1 * pores;
        };
        sets.push(("skin".to_string(), bake(size, &mut skin_fn, aniso, 0.75)));

        // ---- glass-filled polymer: weapon furniture, knee pads, buckles ----
        let mut polymer_fn = |u: f64, v: f64, out: &mut Texel, _x: u32, _y: u32| {
            // moulded pebble stipple + parting-line sheen
            let stip = nz.fbm(u, v, 128.0, 3, 0.5);
            let peb = smooth(0.45, 0.62, nz.fbm(u, v, 64.0, 2, 0.5));
            out.h = (stip - 0.5) * 0.6 + peb * 0.35;
            let scr = smooth(0.86, 1.0, nz.ridge(u * 0.6, v * 3.0, 26.0, 2));
            let v0 = 0.052 * (0.9 + 0.2 * nz.fbm(u, v, 8.0, 2, 0.5));
            out.r = mix(v0 * 1.02, 0.20, scr * 0.5);
            out.g = mix(v0, 0.195, scr * 0.5);
            out.b = mix(v0 * 0.98, 0.19, scr * 0.5);
            out.rough = 0.55 - 0.18 * peb + 0.10 * stip - 0.25 * scr;
            out.metal = 0.0;
            out.ao = 0.88 + 0.12 * stip;
        };
        sets.push((
            "polymer".to_string(),
            bake(size, &mut polymer_fn, aniso, 1.0),
        ));

        // ---- parkerised / phosphated steel --------------------------------
        let mut steel_fn = |u: f64, v: f64, out: &mut Texel, _x: u32, _y: u32| {
            let grain = nz.fbm(u * 0.25, v * 3.0, 90.0, 3, 0.5);
            let scratch = smooth(0.80, 1.0, nz.ridge(u * 0.3, v * 6.0, 40.0, 3));
            let pits = smooth(0.72, 0.9, nz.fbm(u, v, 190.0, 2, 0.5));
            out.h = (grain - 0.5) * 0.35 + scratch * 0.5 - pits * 0.45;
            let base = 0.055 + 0.02 * grain;
            // bare steel where the finish has rubbed through
            let bare = scratch * 0.85;
            out.r = mix(base, 0.52, bare);
            out.g = mix(base, 0.53, bare);
            out.b = mix(base * 1.02, 0.55, bare);
            out.rough = mix(0.46 + 0.14 * grain + 0.2 * pits, 0.20, bare);
            out.metal = 1.0;
            out.ao = 0.9 + 0.1 * grain - 0.2 * pits;
        };
        sets.push(("steel".to_string(), bake(size, &mut steel_fn, aniso, 1.1)));

        // ---- vulcanised rubber: boot soles, sling pads --------------------
        let mut rubber_fn = |u: f64, v: f64, out: &mut Texel, _x: u32, _y: u32| {
            // lug pattern: deep sipes cut between raised blocks
            let lug =
                ridge_line(cell_dist(u * 9.0), 0.085).max(ridge_line(cell_dist(v * 5.5), 0.095));
            let grain = nz.fbm(u, v, 160.0, 3, 0.5);
            out.h = -lug * 1.1 + (grain - 0.5) * 0.4;
            let c = 0.036 + 0.016 * grain;
            out.r = c;
            out.g = c * 0.99;
            out.b = c * 0.97;
            out.rough = 0.82 - 0.1 * grain + 0.08 * lug;
            out.metal = 0.0;
            out.ao = 0.72 + 0.28 * (1.0 - lug);
        };
        sets.push((
            "rubber".to_string(),
            bake(size, &mut rubber_fn, aniso, 1.4),
        ));

        /* ---------------- detail tiles: the high-frequency half ------------ */
        // 5 cm of surface per tile. Blended into the base normal + roughness
        // inside the shader, so a 1.5 mm weave survives no matter how large the
        // base tile has to be to carry the macro camo blotches.
        let mut details: Vec<(String, TextureData)> = Vec::new();
        let dsize = Self::DETAIL_MAX_SIZE.min(size);

        // ripstop cloth: 2-over-2 twill at ~1.5 mm plus the 7 mm ripstop lattice
        let mut cloth_fn = |u: f64, v: f64, out: &mut Texel| {
            let threads = 33.0; // 50 mm / 1.5 mm
            let tu = u * threads;
            let tv = v * threads;
            let wu = (tu * PI * 2.0).sin();
            let wv = (tv * PI * 2.0).sin();
            // A boolean off the SIGN of a sine: at texels where `(tu + tv)` is
            // an exact integer this is `sin(k * PI)`, i.e. a value ~1e-14 whose
            // sign decides the whole weave phase. Both V8 and Rust round the
            // argument identically and their `sin` error is orders of magnitude
            // below that, so the branch agrees — but it is the one place in
            // this file where a libm difference could flip a texel, not just
            // nudge it.
            let over = ((tu + tv) * PI).sin() > 0.0;
            let mut h = (if over {
                wu * 0.62 + wv * 0.22
            } else {
                wv * 0.62 + wu * 0.22
            }) * 0.5;
            // ripstop reinforcement lattice: a doubled thread every 8 mm
            h += (ridge_line(cell_dist(u * 6.0), 0.055) + ridge_line(cell_dist(v * 6.0), 0.055))
                * 0.30;
            // fibre fuzz
            h += (nz.fbm(u, v, 160.0, 2, 0.5) - 0.5) * 0.26;
            out.h = h;
            // raised fuzz scatters more: nap crowns read rougher than the
            // valleys
            out.rough = 0.32 * h - 0.18 * (nz.fbm(u + 2.7, v, 90.0, 2, 0.5) - 0.5);
        };
        details.push((
            "cloth".to_string(),
            bake_detail(dsize, &mut cloth_fn, aniso, 1.05),
        ));

        // nylon webbing / cordura: chunky basket weave with a resin sheen
        let mut nylon_detail_fn = |u: f64, v: f64, out: &mut Texel| {
            let cells_u = 25.0; // 2 mm basket
            let cells_v = 25.0;
            // The source inlines `cellDist`'s body here rather than calling it
            // (textures.js:794-795); the expression is character-for-character
            // the same.
            let cu = cell_dist(u * cells_u);
            let cv = cell_dist(v * cells_v);
            let over = ((u * cells_u + v * cells_v) * PI).sin() > 0.0;
            let mut h = (if over {
                smooth(0.5, 0.1, cu)
            } else {
                smooth(0.5, 0.1, cv)
            }) * 0.7
                - 0.25;
            h += (nz.fbm(u, v, 140.0, 2, 0.5) - 0.5) * 0.22;
            out.h = h;
            // the resin on the crowns of the weave is markedly smoother
            out.rough = -0.42 * h + 0.10 * (nz.fbm(u + 4.1, v, 70.0, 2, 0.5) - 0.5);
        };
        details.push((
            "nylon".to_string(),
            bake_detail(dsize, &mut nylon_detail_fn, aniso, 1.15),
        ));

        SoldierMaterials {
            sets,
            details,
            camo_stats,
        }
    }

    /// `this.sets[setName]`. `None` where the source throws
    /// `[ai] unknown material set "…"` (`textures.js:835`).
    pub fn set(&self, name: &str) -> Option<&BakedSet> {
        self.sets.iter().find(|(n, _)| n == name).map(|(_, s)| s)
    }

    /// `this.details[name]`.
    pub fn detail(&self, name: &str) -> Option<&TextureData> {
        self.details.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    /// `this.camoStats[name]`.
    pub fn camo_stat(&self, name: &str) -> Option<&CamoStats> {
        self.camo_stats
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s)
    }
}
