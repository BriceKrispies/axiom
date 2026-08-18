//! Ported from Claude-of-Duty `src/fx/noise.js:1-158` — the whole file.
//!
//! CPU noise toolkit used to bake the FX texture atlases at load time
//! ([`crate::fx::atlas`]). Everything is seeded from an [`crate::rng::Rng`]
//! fork so a capture is byte-identical run to run; nothing here runs per
//! frame.
//!
//! **Not the same noise as [`crate::materials::noise`].** That module ports
//! `materials/glsl/noise.js` — a hash-lattice (Quilez-style) noise family
//! written as GLSL, evaluated in `f64` as a *reference for a future WGSL
//! emitter*. This file ports a completely different algorithm: classic Ken
//! Perlin gradient noise over a shuffled 256-entry permutation table, plus a
//! Worley/cellular lattice with its own jittered feature-point table — the
//! source's `noise.js` shares no code with `materials/glsl/noise.js` and
//! produces a different numeric sequence for the same input, so this is a
//! second, independent noise implementation, not a re-export.
//!
//! ## Determinism
//!
//! [`Noise::new`] consumes the `rng` it is given via a Fisher-Yates shuffle
//! of the permutation table (256 draws of [`crate::rng::Rng::int`]) followed
//! by 256 pairs of [`crate::rng::Rng::float`] for the Worley jitter table —
//! in that exact order, matching `noise.js:19-33`. Every draw after
//! construction is call-order-sensitive the same way.

use crate::rng::Rng;

/// Quintic fade curve, `noise.js:8`.
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// 16 evenly spread unit gradients (`noise.js:10-14`, `GRAD`). Computed once;
/// deterministic and seed-independent, exactly like the source's module-level
/// `Float32Array`.
fn grad_table() -> [(f64, f64); 16] {
    let mut g = [(0.0, 0.0); 16];
    let mut i = 0;
    while i < 16 {
        let a = (i as f64 / 16.0) * std::f64::consts::PI * 2.0;
        g[i] = (a.cos(), a.sin());
        i += 1;
    }
    g
}

pub fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

/// `smoothstep(a, b, x)`, `noise.js:154-157`.
///
/// **Source quirk, preserved exactly:** the divisor is `b - a || 1e-6` — JS
/// `||` only falls back to `1e-6` when `b - a` is exactly `0` (falsy), not
/// when it is merely small or negative. A naive `(b - a).max(1e-6)` would be
/// a different function: it would also clamp every *reversed* edge pair
/// (`b < a`, used throughout `atlas.js` to invert a falloff direction, e.g.
/// `smoothstep(1.02, 0.66, r)`) up to a positive divisor, flipping their
/// sign. Only the exact-zero case gets the fallback here.
pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let span = b - a;
    let divisor = if span == 0.0 { 1e-6 } else { span };
    let t = clamp01((x - a) / divisor);
    t * t * (3.0 - 2.0 * t)
}

/// sRGB encode for atlases sampled as sRGB textures. `noise.js:161-164`.
pub fn encode_srgb(v: f64) -> f64 {
    let v = clamp01(v);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The CPU noise toolkit, `class Noise` (`noise.js:16-142`).
pub struct Noise {
    /// 512 = the 256-entry permutation table, doubled so `p[i & 255]` never
    /// needs a second wrap (`noise.js:20-25`).
    p: [u8; 512],
    /// Jittered Worley feature points, `noise.js:27-31`.
    cell: [(f64, f64); 256],
    grad: [(f64, f64); 16],
}

impl Noise {
    /// `constructor(rng)`, `noise.js:17-33`. Consumes `rng` — see the module
    /// doc for the exact draw order.
    pub fn new(rng: &mut Rng) -> Self {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = i as u8;
        }
        // Fisher-Yates: `for (i = 255; i > 0; i--) { j = rng.int(0, i); swap(i, j); }`
        for i in (1..256usize).rev() {
            let j = rng.int(0, i as i32) as usize;
            t.swap(i, j);
        }
        let mut p = [0u8; 512];
        for (i, v) in p.iter_mut().enumerate() {
            *v = t[i & 255];
        }

        let mut cell = [(0.0, 0.0); 256];
        for c in cell.iter_mut() {
            *c = (rng.float(), rng.float());
        }

        Noise {
            p,
            cell,
            grad: grad_table(),
        }
    }

    fn hash(&self, ix: i64, iy: i64) -> u8 {
        let a = self.p[(ix.rem_euclid(256)) as usize];
        self.p[((a as i64 + iy.rem_euclid(256)) & 255) as usize]
    }

    /// Perlin gradient noise, roughly `-1..1`. `noise.js:43-59`.
    pub fn perlin(&self, x: f64, y: f64) -> f64 {
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        let u = fade(fx);
        let v = fade(fy);
        let g = self.grad;
        let ixi = ix as i64;
        let iyi = iy as i64;
        let h00 = (self.hash(ixi, iyi) & 15) as usize;
        let h10 = (self.hash(ixi + 1, iyi) & 15) as usize;
        let h01 = (self.hash(ixi, iyi + 1) & 15) as usize;
        let h11 = (self.hash(ixi + 1, iyi + 1) & 15) as usize;
        let d00 = g[h00].0 * fx + g[h00].1 * fy;
        let d10 = g[h10].0 * (fx - 1.0) + g[h10].1 * fy;
        let d01 = g[h01].0 * fx + g[h01].1 * (fy - 1.0);
        let d11 = g[h11].0 * (fx - 1.0) + g[h11].1 * (fy - 1.0);
        let a = d00 + u * (d10 - d00);
        let b = d01 + u * (d11 - d01);
        (a + v * (b - a)) * 1.42
    }

    /// fBm in `0..1`, fixed `lac = 2.03, gain = 0.5` — every call site in the
    /// source passes only `(x, y, oct)`, so those defaults are never
    /// overridden and are baked in here rather than threaded as parameters.
    /// `noise.js:65-77`.
    pub fn fbm(&self, x: f64, y: f64, oct: i32) -> f64 {
        let (lac, gain) = (2.03, 0.5);
        let mut amp = 0.5;
        let mut f = 1.0;
        let mut sum = 0.0;
        let mut norm = 0.0;
        for _ in 0..oct {
            sum += self.perlin(x * f, y * f) * amp;
            norm += amp;
            amp *= gain;
            f *= lac;
        }
        sum / norm / 2.0 + 0.5
    }

    /// Ridged multifractal in `0..1` — veins, cracks, filaments. Fixed
    /// `lac = 2.11, gain = 0.5`, same reasoning as [`Noise::fbm`].
    /// `noise.js:80-91`.
    pub fn ridged(&self, x: f64, y: f64, oct: i32) -> f64 {
        let (lac, gain) = (2.11, 0.5);
        let mut amp = 0.5;
        let mut f = 1.0;
        let mut sum = 0.0;
        let mut norm = 0.0;
        for _ in 0..oct {
            let n = 1.0 - self.perlin(x * f, y * f).abs();
            sum += n * n * amp;
            norm += amp;
            amp *= gain;
            f *= lac;
        }
        sum / norm
    }

    /// Domain-warped fBm — the cheapest way to stop noise looking like noise.
    /// `noise.js:94-98`.
    pub fn warped(&self, x: f64, y: f64, warp: f64, oct: i32) -> f64 {
        let wx = self.perlin(x * 0.7 + 13.1, y * 0.7 - 4.2) * warp;
        let wy = self.perlin(x * 0.7 - 8.6, y * 0.7 + 21.5) * warp;
        self.fbm(x + wx, y + wy, oct)
    }

    /// F1 Worley distance, `0..~1`. `noise.js:101-117`.
    pub fn worley(&self, x: f64, y: f64) -> f64 {
        let ix = x.floor() as i64;
        let iy = y.floor() as i64;
        let mut best = 8.0;
        for oy in -1..=1i64 {
            for ox in -1..=1i64 {
                let h = self.hash(ix + ox, iy + oy) as usize;
                let (jx, jy) = self.cell[h];
                let cx = (ix + ox) as f64 + jx;
                let cy = (iy + oy) as f64 + jy;
                let dx = cx - x;
                let dy = cy - y;
                let d = dx * dx + dy * dy;
                if d < best {
                    best = d;
                }
            }
        }
        best.sqrt().min(1.0)
    }

    /// F2-F1 Worley — cell walls, i.e. crack networks. `noise.js:120-138`.
    pub fn worley_edge(&self, x: f64, y: f64) -> f64 {
        let ix = x.floor() as i64;
        let iy = y.floor() as i64;
        let mut b1 = 8.0;
        let mut b2 = 8.0;
        for oy in -1..=1i64 {
            for ox in -1..=1i64 {
                let h = self.hash(ix + ox, iy + oy) as usize;
                let (jx, jy) = self.cell[h];
                let cx = (ix + ox) as f64 + jx;
                let cy = (iy + oy) as f64 + jy;
                let dx = cx - x;
                let dy = cy - y;
                let d = (dx * dx + dy * dy).sqrt();
                if d < b1 {
                    b2 = b1;
                    b1 = d;
                } else if d < b2 {
                    b2 = d;
                }
            }
        }
        (b2 - b1).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothstep_endpoints() {
        assert_eq!(smoothstep(0.0, 1.0, 0.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 1.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    }

    #[test]
    fn encode_srgb_clamps() {
        assert_eq!(encode_srgb(-1.0), 0.0);
        // Not exactly 1.0: `1.055 * 1.0.powf(1/2.4) - 0.055` loses the last
        // bit to floating-point rounding (`1.055`/`0.055` are not exactly
        // representable in `f64`) — the same rounding a JS `Number`
        // computing the identical formula would produce.
        assert!((encode_srgb(2.0) - 1.0).abs() < 1e-15);
    }

    #[test]
    fn noise_is_deterministic_for_a_fixed_seed() {
        let mut rng = Rng::new(1234);
        let n1 = Noise::new(&mut rng);
        let mut rng2 = Rng::new(1234);
        let n2 = Noise::new(&mut rng2);
        assert_eq!(n1.perlin(1.7, 3.1), n2.perlin(1.7, 3.1));
        assert_eq!(n1.worley(2.2, -1.4), n2.worley(2.2, -1.4));
    }

    #[test]
    fn perlin_stays_in_a_reasonable_range() {
        let mut rng = Rng::new(7);
        let n = Noise::new(&mut rng);
        for i in 0..50 {
            let x = i as f64 * 0.37;
            let v = n.perlin(x, -x * 0.6);
            assert!((-1.5..=1.5).contains(&v));
        }
    }

    #[test]
    fn fbm_and_ridged_and_warped_stay_bounded() {
        let mut rng = Rng::new(99);
        let n = Noise::new(&mut rng);
        for i in 0..20 {
            let x = i as f64 * 0.53;
            let y = -i as f64 * 0.21;
            let f = n.fbm(x, y, 4);
            assert!((-0.5..=1.5).contains(&f));
            let r = n.ridged(x, y, 3);
            assert!((-0.1..=1.1).contains(&r));
            let w = n.warped(x, y, 0.5, 4);
            assert!((-0.5..=1.5).contains(&w));
        }
    }

    #[test]
    fn worley_and_worley_edge_are_nonnegative() {
        let mut rng = Rng::new(42);
        let n = Noise::new(&mut rng);
        for i in 0..20 {
            let x = i as f64 * 0.31;
            let y = i as f64 * 0.17;
            assert!(n.worley(x, y) >= 0.0);
            assert!(n.worley_edge(x, y) >= 0.0);
        }
    }
}
