//! Ported from Claude-of-Duty `src/sky/noise.js:1-91` — `NOISE_GLSL`, the
//! procedural noise the dome, environment bake and volumetric fog draw from
//! (kept as one chunk in the source specifically so those three consumers
//! never disagree with each other).
//!
//! No `dome.js`/fog pass is in this port slice, so nothing in
//! [`super::luts`] calls these functions yet — they are ported now as a
//! reference implementation so a later slice pulling in the sky's clouds/fog
//! GLSL has a CPU oracle to check against, the same role
//! `crate::materials::noise` plays for the surface library.
//!
//! Ported at `f64` throughout, for the same reason `crate::materials::noise`
//! gives in its own module doc: GLSL `float` is 32-bit, so there is no
//! narrower-precision oracle to match, and staying at `f64` keeps the
//! transcendental tolerance in `tests/sky_port.rs` from also fighting `f32`
//! rounding.

use super::atmosphere::Vec3;

/// A minimal `f64` 2-vector — this module's own vocabulary, not shared with
/// [`super::atmosphere::Vec3`] (which is 3D) or `crate::materials::noise`'s
/// `Vec2` (a different file's reference port). See
/// `crate::materials::noise`'s module doc for why every GLSL-porting module
/// in this crate owns its minimal vector type rather than sharing one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub fn new(x: f64, y: f64) -> Self {
        Vec2 { x, y }
    }

    pub fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }

    pub fn scale(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }

    pub fn add_scalar(self, s: f64) -> Vec2 {
        Vec2::new(self.x + s, self.y + s)
    }

    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// Added for `super::clouds`' `skCumulusLight` (`clouds.js:180`).
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Added for `super::clouds`' `skCumulusLight` (`clouds.js:180`).
    pub fn normalize(self) -> Vec2 {
        self.scale(1.0 / self.length())
    }
}

fn gl_fract(x: f64) -> f64 {
    x - x.floor()
}

fn gl_mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// GLSL `f*f*(3-2f)` smoothstep weight, used inline by `skVal2`/`skVal3`.
fn smooth(v: f64) -> f64 {
    v * v * (3.0 - 2.0 * v)
}

/// `skHash12`, `noise.js:13-17`.
pub fn hash12(p: Vec2) -> f64 {
    let p3v = Vec3::new(p.x, p.y, p.x).scale(0.1031).fract();
    let yzx = Vec3::new(p3v.y, p3v.z, p3v.x).add(Vec3::splat(33.33));
    let p3v = p3v.add(Vec3::splat(p3v.dot(yzx)));
    gl_fract((p3v.x + p3v.y) * p3v.z)
}

/// `skHash13`, `noise.js:19-23`.
pub fn hash13(p: Vec3) -> f64 {
    let p = p.scale(0.1031).fract();
    let yzx = Vec3::new(p.y, p.z, p.x).add(Vec3::splat(33.33));
    let p = p.add(Vec3::splat(p.dot(yzx)));
    gl_fract((p.x + p.y) * p.z)
}

/// `skHash33`, `noise.js:25-29`.
pub fn hash33(p: Vec3) -> Vec3 {
    let p = p.mul(Vec3::new(0.1031, 0.113_69, 0.137_87)).fract();
    let yxz = Vec3::new(p.y, p.x, p.z).add(Vec3::splat(19.19));
    let p = p.add(Vec3::splat(p.dot(yxz)));
    Vec3::new(
        gl_fract((p.x + p.y) * p.z),
        gl_fract((p.x + p.z) * p.y),
        gl_fract((p.y + p.z) * p.x),
    )
}

/// Interleaved gradient noise (Jimenez) — the right dither for a raymarch.
/// `skIGN`, `noise.js:32-34`.
pub fn ign(p: Vec2) -> f64 {
    gl_fract(52.982_918_9 * gl_fract(p.dot(Vec2::new(0.067_110_56, 0.005_837_15))))
}

/// `skVal2`, `noise.js:36-41`.
pub fn val2(p: Vec2) -> f64 {
    let i = Vec2::new(p.x.floor(), p.y.floor());
    let f = Vec2::new(gl_fract(p.x), gl_fract(p.y));
    let f = Vec2::new(smooth(f.x), smooth(f.y));
    let a = hash12(i);
    let b = hash12(i.add(Vec2::new(1.0, 0.0)));
    let c = hash12(i.add(Vec2::new(0.0, 1.0)));
    let d = hash12(i.add(Vec2::new(1.0, 1.0)));
    gl_mix(gl_mix(a, b, f.x), gl_mix(c, d, f.x), f.y)
}

/// `skVal3`, `noise.js:43-51`.
pub fn val3(p: Vec3) -> f64 {
    let i = Vec3::new(p.x.floor(), p.y.floor(), p.z.floor());
    let fr = p.fract();
    let f = Vec3::new(smooth(fr.x), smooth(fr.y), smooth(fr.z));
    let h = |dx: f64, dy: f64, dz: f64| hash13(i.add(Vec3::new(dx, dy, dz)));
    let x00 = gl_mix(h(0.0, 0.0, 0.0), h(1.0, 0.0, 0.0), f.x);
    let x10 = gl_mix(h(0.0, 1.0, 0.0), h(1.0, 1.0, 0.0), f.x);
    let y0 = gl_mix(x00, x10, f.y);
    let x01 = gl_mix(h(0.0, 0.0, 1.0), h(1.0, 0.0, 1.0), f.x);
    let x11 = gl_mix(h(0.0, 1.0, 1.0), h(1.0, 1.0, 1.0), f.x);
    let y1 = gl_mix(x01, x11, f.y);
    gl_mix(y0, y1, f.z)
}

/// `const mat2 SK_ROT = mat2( 0.8, 0.6, -0.6, 0.8 );`, `noise.js:53`. GLSL's
/// `mat2` constructor is column-major (column0 = `(0.8, 0.6)`, column1 =
/// `(-0.6, 0.8)`), so `SK_ROT * p == (0.8*p.x - 0.6*p.y, 0.6*p.x + 0.8*p.y)`
/// — the same column-major convention `crate::materials::noise::ow_rot`
/// documents at its own call site.
///
/// `pub` (not `fn`, as when this module had no outside consumer): `SK_ROT` is
/// a *shared* GLSL constant declared once in `NOISE_GLSL` and used directly
/// — not just through `skFbm2`/`skRidge2` — by `clouds.js`'s
/// `skSmoothRidge2` (`clouds.js:80`), so `super::clouds::smooth_ridge2`
/// reuses this exact rotation rather than redefining the same matrix.
pub fn sk_rot(p: Vec2) -> Vec2 {
    Vec2::new(0.8 * p.x - 0.6 * p.y, 0.6 * p.x + 0.8 * p.y)
}

/// `skFbm2`, `noise.js:55-64`.
pub fn fbm2(p: Vec2, oct: i32) -> f64 {
    let mut p = p;
    let mut a = 0.5;
    let mut s = 0.0;
    let mut n = 0.0;
    for _ in 0..oct {
        s += a * val2(p);
        n += a;
        p = sk_rot(p).scale(2.04).add_scalar(7.13);
        a *= 0.5;
    }
    s / n.max(1e-4)
}

/// Ridged variant — fibrous cirrus streaks and wind-torn fog wisps.
/// `skRidge2`, `noise.js:67-76`.
pub fn ridge2(p: Vec2, oct: i32) -> f64 {
    let mut p = p;
    let mut a = 0.5;
    let mut s = 0.0;
    let mut n = 0.0;
    for _ in 0..oct {
        s += a * (1.0 - (val2(p) * 2.0 - 1.0).abs());
        n += a;
        p = sk_rot(p).scale(2.11).add_scalar(3.71);
        a *= 0.52;
    }
    s / n.max(1e-4)
}

/// `skFbm3`, `noise.js:78-87`.
pub fn fbm3(p: Vec3, oct: i32) -> f64 {
    let mut p = p;
    let mut a = 0.5;
    let mut s = 0.0;
    let mut n = 0.0;
    for _ in 0..oct {
        s += a * val3(p);
        n += a;
        p = p.scale(2.07).add(Vec3::new(11.3, 5.1, 7.7));
        a *= 0.5;
    }
    s / n.max(1e-4)
}
