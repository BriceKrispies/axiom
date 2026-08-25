//! The deterministic RNG and tileable value noise, ported from
//! `src/core/rng.js` and the `TileNoise` class in `src/ai/bake.js`.
//!
//! BIT-EXACTNESS IS THE WHOLE CONTRACT. This has to produce byte-identical
//! output to the JavaScript it replaces, or the pixel gate has to be
//! re-baselined and every future comparison against it is worthless. Two things
//! make that achievable here:
//!
//!   * The hot path contains no transcendentals. `n2`, `fbm` and `ridge` are
//!     multiply, add, floor, table lookup and a smoothstep — all IEEE-754
//!     operations with exactly-specified results, so f64 in Rust and a JS
//!     Number are the same value bit for bit.
//!   * The integer paths are written to match JavaScript's semantics rather
//!     than Rust's convenience: `Math.imul` is a wrapping 32-bit multiply,
//!     `>>>` is a logical shift on a u32, and `|0` / `&` coerce through int32.
//!     Every one of those is spelled out below rather than approximated.
//!
//! The one place JS and Rust genuinely differ is `%` on negatives, and the
//! source already guards it with `((a % p) + p) % p`, which is `rem_euclid`.

/// xoshiro128\*\*, seeded through SplitMix32 — `src/core/rng.js`.
pub struct Rng {
    s0: u32,
    s1: u32,
    s2: u32,
    s3: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        // SplitMix32 spreads one 32-bit seed across the four state words.
        let mut z = seed;
        let mut next = || {
            z = z.wrapping_add(0x9e37_79b9);
            let mut x = z;
            x = (x ^ (x >> 16)).wrapping_mul(0x21f0_aaad);
            x = (x ^ (x >> 15)).wrapping_mul(0x735a_2d97);
            x ^ (x >> 15)
        };
        let s0 = next();
        let s1 = next();
        let s2 = next();
        let s3 = next();
        Self { s0, s1, s2, s3 }
    }

    #[inline]
    pub fn u32(&mut self) -> u32 {
        #[inline]
        fn rot(x: u32, k: u32) -> u32 {
            (x << k) | (x >> (32 - k))
        }
        let result = rot(self.s1.wrapping_mul(5), 7).wrapping_mul(9);
        let t = self.s1 << 9;
        self.s2 ^= self.s0;
        self.s3 ^= self.s1;
        self.s1 ^= self.s2;
        self.s0 ^= self.s3;
        self.s2 ^= t;
        self.s3 = rot(self.s3, 11);
        result
    }

    /// Uniform [0,1). JS divides by 2^32 exactly; so does this.
    #[inline]
    pub fn float(&mut self) -> f64 {
        self.u32() as f64 / 4_294_967_296.0
    }

    /// Uniform integer [min,max], matching JS `min + (u32() % (max-min+1))`.
    #[inline]
    pub fn int(&mut self, min: u32, max: u32) -> u32 {
        min + (self.u32() % (max - min + 1))
    }
}

/// Tileable value noise on a 4096-entry table — `TileNoise` in `src/ai/bake.js`.
///
/// `tab` is a `Float32Array` in the source, so each sample is rounded to f32 on
/// store and widened back to f64 on read. That rounding is part of the output,
/// so it is reproduced exactly: `f32` storage, `f64` arithmetic.
pub struct TileNoise {
    tab: [f32; 4096],
    perm: [u16; 4096],
}

impl TileNoise {
    pub fn new(rng: &mut Rng) -> Self {
        let mut tab = [0.0f32; 4096];
        for slot in tab.iter_mut() {
            *slot = rng.float() as f32;
        }
        let mut perm = [0u16; 4096];
        for slot in perm.iter_mut() {
            *slot = rng.int(0, 4095) as u16;
        }
        Self { tab, perm }
    }

    #[inline]
    fn h(&self, ix: i32, iy: i32, period: i32) -> f64 {
        let p = period;
        let x = ix.rem_euclid(p);
        let y = iy.rem_euclid(p);
        // `this.tab[(this.perm[(x*73 + y*151) & 4095] + x*31 + y*17) & 4095]`.
        // `&` in JS coerces through int32, which is what i32 arithmetic here
        // reproduces; both operands are non-negative after the rem_euclid above.
        let slot = (x.wrapping_mul(73).wrapping_add(y.wrapping_mul(151))) & 4095;
        let idx = (self.perm[slot as usize] as i32)
            .wrapping_add(x.wrapping_mul(31))
            .wrapping_add(y.wrapping_mul(17))
            & 4095;
        self.tab[idx as usize] as f64
    }

    /// Value noise on a lattice of `period` cells over the unit tile.
    #[inline]
    pub fn n2(&self, u: f64, v: f64, period: f64) -> f64 {
        let x = u * period;
        let y = v * period;
        let ix = x.floor();
        let iy = y.floor();
        let fx = x - ix;
        let fy = y - iy;
        let sx = fx * fx * (3.0 - 2.0 * fx);
        let sy = fy * fy * (3.0 - 2.0 * fy);
        let p = period as i32;
        let ixi = ix as i32;
        let iyi = iy as i32;
        let a = self.h(ixi, iyi, p);
        let b = self.h(ixi + 1, iyi, p);
        let c = self.h(ixi, iyi + 1, p);
        let d = self.h(ixi + 1, iyi + 1, p);
        (a + (b - a) * sx) * (1.0 - sy) + (c + (d - c) * sx) * sy
    }

    #[inline]
    pub fn fbm(&self, u: f64, v: f64, period: f64, oct: u32, gain: f64) -> f64 {
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

    #[inline]
    pub fn ridge(&self, u: f64, v: f64, period: f64, oct: u32) -> f64 {
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
