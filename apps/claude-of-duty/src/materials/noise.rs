//! Ported from Claude-of-Duty `src/materials/glsl/noise.js:1-218` — the whole
//! embedded GLSL body (`NOISE_GLSL`).
//!
//! The tileable procedural noise library every one of the 19 surface
//! generators is built on. It is ported here as CPU-evaluable `f64` maths —
//! not as a WGSL string — because the goal right now is a *reference
//! implementation* the eventual WGSL emitter can be pinned against, the same
//! way [`crate::rng::Rng`] is the reference the JS `rng.js` port is pinned
//! against. Emitting this as shader source is a separate, later workstream.
//!
//! **The one property everything below exists to preserve: periodicity.**
//! Every function takes a `per` (period, in lattice cells) and wraps its hash
//! lattice with GLSL `mod()` before hashing, so a texture generated over
//! `uv in [0,1)` with `p = uv * per` tiles seamlessly — `f(p) == f(p + per)`
//! for every function below that takes a period. Octaves double both
//! frequency *and* period together, which is what keeps a whole fbm stack
//! seamless rather than just its base octave. `tests/materials_noise_port.rs`
//! pins this directly: `f(p) == f(p + per)` for a grid of `p`, for every
//! periodic function.
//!
//! Hashes are sin-free (Dave Hoskins style): the source's comment notes
//! `sin()`-based hashes band badly on Apple GPUs at high lattice coordinates,
//! so every hash here is built from `fract`/multiply churn instead.
//!
//! ## GLSL → Rust mapping
//!
//! - `float`/`int` → `f64`/`i32`. The source is shader code, so unlike the
//!   rest of this port (which matches a JS `number`), there is no `f64`
//!   precedent to match — GLSL `float` is 32-bit. This reference
//!   implementation is deliberately kept in `f64` throughout (higher
//!   precision than the eventual GPU evaluation will have) so the golden
//!   values pinned in `tests/materials_noise_port.rs` are not fighting `f32`
//!   rounding on top of the `sin`/`cos`/`pow` tolerance every transcendental
//!   already needs. The WGSL emission workstream is the point at which `f32`
//!   truncation becomes an explicit, separately-tested concern.
//! - `vec2`/`vec3`/`vec4` → [`Vec2`]/[`Vec3`]/[`Vec4`], minimal `Copy` structs
//!   with exactly the operations these functions use (add, scale, component
//!   multiply, `dot`, `floor`, `fract`, periodic `mod`). There is no shared
//!   vector type anywhere in this crate yet (see the note in
//!   `weapons/ballistics.rs`'s `Vec3`), and a generic swizzle system is not
//!   worth building for 24 hand-transcribed functions — each GLSL swizzle
//!   (`p.xyx`, `p3.yzx`, …) is instead expanded inline at its one call site,
//!   which keeps every function diffable against the source line-for-line.
//! - `mod(x, y)` → [`gl_mod`], **not** Rust's `%`. GLSL's `mod` is
//!   `x - y * floor(x / y)`: always non-negative for `y > 0`, even when `x`
//!   is negative. Rust's `%` keeps the sign of `x`. Using `%` here would
//!   silently break periodicity for any negative lattice coordinate.
//! - `fract(x)` → [`gl_fract`] = `x - floor(x)`, for the same negative-input
//!   reason.
//! - `mix`/`step` compositions (e.g. [`ow_srgb`]) are ported as an `if`/`else`
//!   rather than the literal arithmetic form: `mix(a, b, step(edge, x))`
//!   selects `a` exactly when `step` is exactly `0.0` and `b` exactly when it
//!   is exactly `1.0` (a boolean-valued `mix`, not a blend), so the two forms
//!   are bit-identical and the `if`/`else` reads directly as the sRGB decode
//!   piecewise definition it is. Apps are outside the Branchless Law, so nothing
//!   forces the arithmetic form here.
//!
//! ## The `for (int i = 0; i < 10; i++){ if (i >= oct) break; ... }` idiom
//!
//! Every fbm-family function ([`ow_fbm`], [`ow_ridged`], [`ow_billow`]) loops
//! to a GLSL-mandated compile-time bound of 10 and breaks early at the
//! caller's `oct`, because stock GLSL cannot loop a variable number of times.
//! Rust has no such restriction, but the loop is ported with the same shape
//! (bounded at 10, breaking at `oct`) rather than simplified to `for _ in
//! 0..oct`, because `oct > 10` is reachable from the surface library's
//! `MatParams` data and the source's behaviour for that case — silently
//! capping at 10 octaves — is a real, load-bearing contract for callers, not
//! an artifact of the port. `tests/materials_noise_port.rs` pins the cap.

// ---------------------------------------------------------------------------
// Scalar GLSL primitives.
// ---------------------------------------------------------------------------

/// GLSL `fract(x)` = `x - floor(x)`. Always in `[0, 1)`, unlike Rust's
/// `f64::fract` which keeps the sign of `x` (`(-0.3).fract() == -0.3`).
pub fn gl_fract(x: f64) -> f64 {
    x - x.floor()
}

/// GLSL `mod(x, y)` = `x - y * floor(x / y)`. Always non-negative for `y > 0`,
/// unlike Rust's `%` which keeps the sign of `x`.
pub fn gl_mod(x: f64, y: f64) -> f64 {
    x - y * (x / y).floor()
}

/// GLSL `mix(a, b, t)` = `a * (1 - t) + b * t`, written as the source writes
/// it (`a + (b - a) * t`), which is the same value with one fewer rounding.
pub fn gl_mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// GLSL `clamp(x, a, b)`.
pub fn gl_clamp(x: f64, a: f64, b: f64) -> f64 {
    x.max(a).min(b)
}

/// GLSL `smoothstep(edge0, edge1, x)`. Not guarded against `edge0 == edge1`
/// (unlike [`ow_remap`]'s `max(b - a, 1e-5)`) — the source does not guard it
/// either at any of this file's three call sites, and every one of them
/// passes a non-degenerate span.
pub fn gl_smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = gl_clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---------------------------------------------------------------------------
// Minimal vector types — see the module doc for why these exist instead of a
// generic swizzle-capable vector library.
// ---------------------------------------------------------------------------

/// GLSL `vec2`, for CPU evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }

    /// `vec2(s)` — both components the same value.
    pub fn splat(s: f64) -> Self {
        Vec2::new(s, s)
    }

    pub fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }

    pub fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }

    /// Component-wise multiply (`vec2 * vec2` in GLSL).
    pub fn mul(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x * o.x, self.y * o.y)
    }

    /// `v * s` — uniform scale.
    pub fn scale(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }

    /// `v + s` — a scalar broadcast-added to both components.
    pub fn add_scalar(self, s: f64) -> Vec2 {
        Vec2::new(self.x + s, self.y + s)
    }

    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    pub fn floor(self) -> Vec2 {
        Vec2::new(self.x.floor(), self.y.floor())
    }

    pub fn fract(self) -> Vec2 {
        Vec2::new(gl_fract(self.x), gl_fract(self.y))
    }

    /// Component-wise `mod(self, per)`.
    pub fn modulo(self, per: Vec2) -> Vec2 {
        Vec2::new(gl_mod(self.x, per.x), gl_mod(self.y, per.y))
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// `normalize(v)`. Not guarded against a zero-length input — see
    /// [`ow_voronoi_edge`]'s call site for why that never happens there.
    pub fn normalize(self) -> Vec2 {
        self.scale(1.0 / self.length())
    }
}

/// GLSL `vec3`, for CPU evaluation. Only the operations `owHash12`/`owHash32`
/// and [`ow_srgb`] use.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }

    pub fn splat(s: f64) -> Self {
        Vec3::new(s, s, s)
    }

    pub fn mul(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    pub fn scale(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn add_scalar(self, s: f64) -> Vec3 {
        Vec3::new(self.x + s, self.y + s, self.z + s)
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn fract(self) -> Vec3 {
        Vec3::new(gl_fract(self.x), gl_fract(self.y), gl_fract(self.z))
    }
}

/// GLSL `vec4`, for CPU evaluation. Only [`ow_hash42`] returns one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Vec4 {
    pub fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Vec4 { x, y, z, w }
    }

    pub fn mul(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x * o.x, self.y * o.y, self.z * o.z, self.w * o.w)
    }

    pub fn dot(self, o: Vec4) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    pub fn add_scalar(self, s: f64) -> Vec4 {
        Vec4::new(self.x + s, self.y + s, self.z + s, self.w + s)
    }

    pub fn fract(self) -> Vec4 {
        Vec4::new(
            gl_fract(self.x),
            gl_fract(self.y),
            gl_fract(self.z),
            gl_fract(self.w),
        )
    }
}

/// The result of [`ow_worley`]: `.x`/`.y` were the source's `vec4.x`/`.y`
/// (F1/F2 distance), `.z`/`.w` its `.z`/`.w` (the F1 cell's two hash
/// channels). Named fields instead of a bare `Vec4` — GLSL leans on the
/// caller remembering which swizzle means what; Rust does not have to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WorleyResult {
    /// Distance to the nearest feature point.
    pub f1: f64,
    /// Distance to the second-nearest feature point.
    pub f2: f64,
    /// First hash channel of the F1 cell (`id.x` in the source).
    pub id_x: f64,
    /// Second hash channel of the F1 cell (`id.y` in the source).
    pub id_y: f64,
}

// ---------------------------------------------------------------------------
// Hashes — `noise.js:15-40`.
// ---------------------------------------------------------------------------

/// `owHash11`, `noise.js:15-20`.
pub fn ow_hash11(p: f64) -> f64 {
    let mut p = gl_fract(p * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    gl_fract(p)
}

/// `owHash12`, `noise.js:21-25`.
pub fn ow_hash12(p: Vec2) -> f64 {
    // vec3(p.xyx) * 0.1031
    let mut p3 = Vec3::new(p.x, p.y, p.x).scale(0.1031).fract();
    // p3 += dot(p3, p3.yzx + 33.33)
    let yzx = Vec3::new(p3.y, p3.z, p3.x).add_scalar(33.33);
    p3 = p3.add_scalar(p3.dot(yzx));
    gl_fract((p3.x + p3.y) * p3.z)
}

/// `owHash22`, `noise.js:26-30`.
pub fn ow_hash22(p: Vec2) -> Vec2 {
    let mut p3 = Vec3::new(p.x, p.y, p.x)
        .mul(Vec3::new(0.1031, 0.1030, 0.0973))
        .fract();
    let yzx = Vec3::new(p3.y, p3.z, p3.x).add_scalar(33.33);
    p3 = p3.add_scalar(p3.dot(yzx));
    // (p3.xx + p3.yz) * p3.zy
    let sum = Vec2::new(p3.x + p3.y, p3.x + p3.z);
    let zy = Vec2::new(p3.z, p3.y);
    sum.mul(zy).fract()
}

/// `owHash32`, `noise.js:31-35`.
pub fn ow_hash32(p: Vec2) -> Vec3 {
    let mut p3 = Vec3::new(p.x, p.y, p.x)
        .mul(Vec3::new(0.1031, 0.1030, 0.0973))
        .fract();
    // p3.yxz + 33.33 (note: yxz here, not yzx — matches the source exactly)
    let yxz = Vec3::new(p3.y, p3.x, p3.z).add_scalar(33.33);
    p3 = p3.add_scalar(p3.dot(yxz));
    // (p3.xxy + p3.yzz) * p3.zyx
    let sum = Vec3::new(p3.x + p3.y, p3.x + p3.z, p3.y + p3.z);
    let zyx = Vec3::new(p3.z, p3.y, p3.x);
    sum.mul(zyx).fract()
}

/// `owHash42`, `noise.js:36-40`.
pub fn ow_hash42(p: Vec2) -> Vec4 {
    let mut p4 = Vec4::new(p.x, p.y, p.x, p.y)
        .mul(Vec4::new(0.1031, 0.1030, 0.0973, 0.1099))
        .fract();
    let wzxy = Vec4::new(p4.w, p4.z, p4.x, p4.y).add_scalar(33.33);
    p4 = p4.add_scalar(p4.dot(wzxy));
    // (p4.xxyz + p4.yzzw) * p4.zywx
    let sum = Vec4::new(p4.x + p4.y, p4.x + p4.z, p4.y + p4.z, p4.z + p4.w);
    let zywx = Vec4::new(p4.z, p4.y, p4.w, p4.x);
    sum.mul(zywx).fract()
}

// ---------------------------------------------------------------------------
// Gradient (Perlin) noise — `noise.js:42-58`.
// ---------------------------------------------------------------------------

/// `owGrad2`, `noise.js:43-46`.
pub fn ow_grad2(i: Vec2, per: Vec2) -> Vec2 {
    let a = ow_hash12(i.modulo(per).add_scalar(0.317)) * 6.283_185_307_18;
    Vec2::new(a.cos(), a.sin())
}

/// Periodic gradient (Perlin) noise. Returns approximately `[-1, 1]`.
/// `owNoise`, `noise.js:49-57`.
pub fn ow_noise(p: Vec2, per: Vec2) -> f64 {
    let i = p.floor();
    let f = p.fract();
    let fade = |v: f64| v * v * v * (v * (v * 6.0 - 15.0) + 10.0);
    let u = Vec2::new(fade(f.x), fade(f.y));
    let a = ow_grad2(i.add(Vec2::new(0.0, 0.0)), per).dot(f.sub(Vec2::new(0.0, 0.0)));
    let b = ow_grad2(i.add(Vec2::new(1.0, 0.0)), per).dot(f.sub(Vec2::new(1.0, 0.0)));
    let c = ow_grad2(i.add(Vec2::new(0.0, 1.0)), per).dot(f.sub(Vec2::new(0.0, 1.0)));
    let d = ow_grad2(i.add(Vec2::new(1.0, 1.0)), per).dot(f.sub(Vec2::new(1.0, 1.0)));
    gl_mix(gl_mix(a, b, u.x), gl_mix(c, d, u.x), u.y) * 1.4142
}

/// `owNoise01`, `noise.js:58`.
pub fn ow_noise01(p: Vec2, per: Vec2) -> f64 {
    ow_noise(p, per) * 0.5 + 0.5
}

/// Periodic value noise — blockier than [`ow_noise`], good for cell-ish tint
/// variation. `owValue`, `noise.js:61-69`.
pub fn ow_value(p: Vec2, per: Vec2) -> f64 {
    let i = p.floor();
    let f = p.fract();
    let smooth = |v: f64| v * v * (3.0 - 2.0 * v);
    let u = Vec2::new(smooth(f.x), smooth(f.y));
    let a = ow_hash12(i.add(Vec2::new(0.0, 0.0)).modulo(per).add_scalar(1.7));
    let b = ow_hash12(i.add(Vec2::new(1.0, 0.0)).modulo(per).add_scalar(1.7));
    let c = ow_hash12(i.add(Vec2::new(0.0, 1.0)).modulo(per).add_scalar(1.7));
    let d = ow_hash12(i.add(Vec2::new(1.0, 1.0)).modulo(per).add_scalar(1.7));
    gl_mix(gl_mix(a, b, u.x), gl_mix(c, d, u.x), u.y)
}

// ---------------------------------------------------------------------------
// fbm family — `noise.js:71-107`. See the module doc for the `oct`-capped-
// at-10 loop shape.
// ---------------------------------------------------------------------------

/// `owFbm`, `noise.js:72-81`.
pub fn ow_fbm(p: Vec2, per: Vec2, oct: i32, gain: f64) -> f64 {
    let mut p = p;
    let mut per = per;
    let mut s = 0.0;
    let mut a = 0.5;
    let mut n = 0.0;
    for i in 0..10 {
        if i >= oct {
            break;
        }
        s += a * ow_noise(p, per);
        n += a;
        p = p.scale(2.0);
        per = per.scale(2.0);
        a *= gain;
    }
    s / n.max(1e-4)
}

/// `owFbm01`, `noise.js:82`.
pub fn ow_fbm01(p: Vec2, per: Vec2, oct: i32, gain: f64) -> f64 {
    ow_fbm(p, per, oct, gain) * 0.5 + 0.5
}

/// Ridged fbm — sharp creases, good for cracks / rock. Returns `[0, 1]`.
/// `owRidged`, `noise.js:85-95`.
pub fn ow_ridged(p: Vec2, per: Vec2, oct: i32, gain: f64) -> f64 {
    let mut p = p;
    let mut per = per;
    let mut s = 0.0;
    let mut a = 0.5;
    let mut n = 0.0;
    for i in 0..10 {
        if i >= oct {
            break;
        }
        let v = 1.0 - ow_noise(p, per).abs();
        s += a * v * v;
        n += a;
        p = p.scale(2.0);
        per = per.scale(2.0);
        a *= gain;
    }
    s / n.max(1e-4)
}

/// Billowy fbm — puffy clumps, good for rust blooms and clay. `owBillow`,
/// `noise.js:98-107`.
pub fn ow_billow(p: Vec2, per: Vec2, oct: i32, gain: f64) -> f64 {
    let mut p = p;
    let mut per = per;
    let mut s = 0.0;
    let mut a = 0.5;
    let mut n = 0.0;
    for i in 0..10 {
        if i >= oct {
            break;
        }
        s += a * ow_noise(p, per).abs();
        n += a;
        p = p.scale(2.0);
        per = per.scale(2.0);
        a *= gain;
    }
    s / n.max(1e-4)
}

// ---------------------------------------------------------------------------
// Domain warp — `noise.js:109-114`.
// ---------------------------------------------------------------------------

/// `owWarp`, `noise.js:110-114`.
pub fn ow_warp(p: Vec2, per: Vec2, amp: f64, oct: i32) -> Vec2 {
    let q = Vec2::new(
        ow_fbm(p.add(Vec2::new(1.7, 9.2)), per, oct, 0.5),
        ow_fbm(p.add(Vec2::new(8.3, 2.8)), per, oct, 0.5),
    );
    p.add(q.scale(amp))
}

// ---------------------------------------------------------------------------
// Worley / Voronoi — `noise.js:116-170`.
// ---------------------------------------------------------------------------

/// Periodic Worley/Voronoi: F1/F2 distance and the F1 cell's hash id.
/// `owWorley`, `noise.js:122-138`.
pub fn ow_worley(p: Vec2, per: Vec2, jitter: f64) -> WorleyResult {
    let ip = p.floor();
    let fp = p.fract();
    let mut f1 = 8.0;
    let mut f2 = 8.0;
    let mut id = Vec2::default();
    for y in -1..=1 {
        for x in -1..=1 {
            let g = Vec2::new(f64::from(x), f64::from(y));
            let cell = ip.add(g).modulo(per);
            let o = ow_hash22(cell.add_scalar(0.771))
                .scale(jitter)
                .add_scalar((1.0 - jitter) * 0.5);
            let r = g.add(o).sub(fp);
            let d = r.dot(r);
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = ow_hash22(cell.add_scalar(3.117));
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    WorleyResult {
        f1: f1.sqrt(),
        f2: f2.sqrt(),
        id_x: id.x,
        id_y: id.y,
    }
}

/// Distance to the *edge* of the Voronoi cell (Quilez two-pass) — a much
/// better-looking crack network than `F2 - F1`. Returns `[0, ~0.7]`.
/// `owVoronoiEdge`, `noise.js:144-170`.
pub fn ow_voronoi_edge(p: Vec2, per: Vec2, jitter: f64) -> f64 {
    let ip = p.floor();
    let fp = p.fract();

    let feature_point = |g: Vec2| -> Vec2 {
        ow_hash22(ip.add(g).modulo(per).add_scalar(0.771))
            .scale(jitter)
            .add_scalar((1.0 - jitter) * 0.5)
    };

    let mut mr = Vec2::default();
    let mut mg = Vec2::default();
    let mut md = 8.0;
    for y in -1..=1 {
        for x in -1..=1 {
            let g = Vec2::new(f64::from(x), f64::from(y));
            let o = feature_point(g);
            let r = g.add(o).sub(fp);
            let d = r.dot(r);
            if d < md {
                md = d;
                mr = r;
                mg = g;
            }
        }
    }

    let mut md = 8.0_f64;
    for y in -2..=2 {
        for x in -2..=2 {
            let g = mg.add(Vec2::new(f64::from(x), f64::from(y)));
            let o = feature_point(g);
            let r = g.add(o).sub(fp);
            let diff = r.sub(mr);
            if diff.dot(diff) > 1e-5 {
                md = md.min(mr.add(r).scale(0.5).dot(diff.normalize()));
            }
        }
    }
    md
}

/// Crack network: warped Voronoi edges, thinned and broken up so lines
/// terminate instead of forming a perfect mesh. Returns `[0, 1]`, `1` = deep
/// crack. `owCracks`, `noise.js:176-184`.
pub fn ow_cracks(p: Vec2, per: Vec2, jitter: f64, width: f64, break_up: f64) -> f64 {
    let wp = ow_warp(p, per, 0.20, 3);
    let e = ow_voronoi_edge(wp, per, jitter);
    let mut c = 1.0 - gl_smoothstep(0.0, width, e);
    // Break the network so it reads as damage, not as a net.
    let mask = ow_fbm01(
        p.scale(1.7).add(Vec2::new(11.3, 11.3)),
        per.scale(1.7),
        4,
        0.55,
    );
    c *= gl_smoothstep(break_up, break_up + 0.28, mask);
    gl_clamp(c, 0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Utilities — `noise.js:186-209`.
// ---------------------------------------------------------------------------

/// `owSat`, `noise.js:187`.
pub fn ow_sat(x: f64) -> f64 {
    gl_clamp(x, 0.0, 1.0)
}

/// `owSat3`, `noise.js:188`.
pub fn ow_sat3(x: Vec3) -> Vec3 {
    Vec3::new(ow_sat(x.x), ow_sat(x.y), ow_sat(x.z))
}

/// `owRemap`, `noise.js:189-191`. The one guarded division in this file —
/// `max(b - a, 1e-5)` — ported exactly as the guard, unlike [`gl_smoothstep`]
/// which the source leaves unguarded.
pub fn ow_remap(x: f64, a: f64, b: f64, c: f64, d: f64) -> f64 {
    c + (d - c) * gl_clamp((x - a) / (b - a).max(1e-5), 0.0, 1.0)
}

/// `owRot`, `noise.js:192-195`.
///
/// **Source quirk, preserved exactly:** GLSL's `mat2(x0, y0, x1, y1)`
/// constructor is column-major — column 0 is `(x0, y0)`, column 1 is
/// `(x1, y1)` — and `m * v = v.x * column0 + v.y * column1`. The source
/// writes `mat2(c, -s, s, c) * p`, i.e. column0 = `(c, -s)`, column1 =
/// `(s, c)`, giving `(c*p.x + s*p.y, c*p.y - s*p.x)` — a **clockwise**
/// rotation for positive `a`, not the counter-clockwise
/// `(c*p.x - s*p.y, s*p.x + c*p.y)` the same-looking row-major matrix would
/// give. Whether that was intentional in the source or an author's
/// column/row mix-up, it is the behaviour every caller of `owRot` (currently
/// none in this file; future shear/scratch generators) will see, so it is
/// ported as-is rather than "corrected" to the standard convention.
/// `tests/materials_noise_port.rs::ow_rot_matches_the_column_major_glsl_matrix`
/// pins the direction against a captured 90-degree turn.
pub fn ow_rot(p: Vec2, a: f64) -> Vec2 {
    let s = a.sin();
    let c = a.cos();
    Vec2::new(c * p.x + s * p.y, c * p.y - s * p.x)
}

/// sRGB hex-ish helper: authoring colours in gamma space, output linear.
/// `owSRGB`, `noise.js:197-199`. See the module doc for why this is an
/// `if`/`else` rather than the source's `mix`/`step` — they are the same
/// value.
pub fn ow_srgb(c: Vec3) -> Vec3 {
    let decode = |ci: f64| -> f64 {
        if ci > 0.04045 {
            ((ci + 0.055) / 1.055).powf(2.4)
        } else {
            ci / 12.92
        }
    };
    Vec3::new(decode(c.x), decode(c.y), decode(c.z))
}

/// Anisotropic shear that preserves tileability: `k` and `stretch` must be
/// integers so the lattice still wraps on `per`. `owShear`, `noise.js:204-206`.
pub fn ow_shear(p: Vec2, k: f64, stretch: f64) -> Vec2 {
    Vec2::new(p.x + p.y * k, p.y * stretch)
}

/// `owShearPer`, `noise.js:207-209`.
pub fn ow_shear_per(per: Vec2, stretch: f64) -> Vec2 {
    Vec2::new(per.x, per.y * stretch)
}

/// Scratch lines: long thin streaks running along a sheared axis. `[0, 1]`.
/// `owScratches`, `noise.js:212-217`.
pub fn ow_scratches(p: Vec2, per: Vec2, stretch: f64, k: f64, thin: f64) -> f64 {
    let q = ow_shear(p, k, stretch);
    let qper = ow_shear_per(per, stretch);
    let n = ow_fbm01(q, qper, 4, 0.5);
    gl_smoothstep(thin, thin + 0.06, n) * (1.0 - gl_smoothstep(thin + 0.06, thin + 0.2, n))
}
