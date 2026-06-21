//! Coherent noise (value/Perlin/simplex + FBM, domain warp) for worldgen.
//! Audit: "Procedural-generation math" gap — needed for elevation detail,
//! moisture, chunk detail_noise. Deterministic from a seed.
//!
//! INTERFACE CONTRACT (preserve signatures): agents implement the bodies.
//!
//! Implementation: 3D gradient (Perlin-style) noise hashed deterministically
//! from a 64-bit seed using a splitmix64-derived integer mixer. No std rand,
//! no wall clock — same seed + same point always yields the same value.
use axiom_math::Vec3;

/// Splitmix64-style avalanche mixer. Deterministic, no external state.
#[inline]
fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Hash an integer lattice cell + seed into a 64-bit value.
#[inline]
fn hash_cell(seed: u64, xi: i32, yi: i32, zi: i32) -> u64 {
    // Fold the (possibly negative) lattice coordinates into the 64-bit space
    // and run them through the avalanche mixer one component at a time so that
    // adjacent cells decorrelate fully.
    let mut h = seed ^ 0xA0761D_6478BD_642Fu64;
    h = mix64(h ^ (xi as i64 as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
    h = mix64(h ^ (yi as i64 as u64).wrapping_mul(0xCA01_F4D2_2A3B_C9D1));
    h = mix64(h ^ (zi as i64 as u64).wrapping_mul(0x9E37_79B1_85EB_CA87));
    mix64(h)
}

/// A pseudo-random gradient vector for a lattice cell, drawn from a fixed set
/// of 12 edge-midpoint directions (classic Perlin gradient table). Returns a
/// non-normalized but unit-ish gradient; the dot product is scaled to [-1,1].
#[inline]
fn cell_gradient(h: u64) -> Vec3 {
    // 12 gradient directions toward the edges of a cube.
    const GRADS: [(f32, f32, f32); 12] = [
        (1.0, 1.0, 0.0),
        (-1.0, 1.0, 0.0),
        (1.0, -1.0, 0.0),
        (-1.0, -1.0, 0.0),
        (1.0, 0.0, 1.0),
        (-1.0, 0.0, 1.0),
        (1.0, 0.0, -1.0),
        (-1.0, 0.0, -1.0),
        (0.0, 1.0, 1.0),
        (0.0, -1.0, 1.0),
        (0.0, 1.0, -1.0),
        (0.0, -1.0, -1.0),
    ];
    let idx = (h % 12) as usize;
    let g = GRADS[idx];
    Vec3::new(g.0, g.1, g.2)
}

/// Quintic smoothstep fade, Perlin's 6t^5 - 15t^4 + 10t^3.
#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Core 3D gradient noise. Output is in roughly [-1, 1].
fn gradient_noise(seed: u64, p: Vec3) -> f32 {
    let xf = p.x.floor();
    let yf = p.y.floor();
    let zf = p.z.floor();

    let x0 = xf as i32;
    let y0 = yf as i32;
    let z0 = zf as i32;
    let x1 = x0.wrapping_add(1);
    let y1 = y0.wrapping_add(1);
    let z1 = z0.wrapping_add(1);

    // Fractional position within the cell.
    let fx = p.x - xf;
    let fy = p.y - yf;
    let fz = p.z - zf;

    let u = fade(fx);
    let v = fade(fy);
    let w = fade(fz);

    // Dot of each corner's gradient with the offset from that corner.
    #[inline]
    fn corner(seed: u64, ci: i32, cj: i32, ck: i32, dx: f32, dy: f32, dz: f32) -> f32 {
        let g = cell_gradient(hash_cell(seed, ci, cj, ck));
        g.dot(Vec3::new(dx, dy, dz))
    }

    let n000 = corner(seed, x0, y0, z0, fx, fy, fz);
    let n100 = corner(seed, x1, y0, z0, fx - 1.0, fy, fz);
    let n010 = corner(seed, x0, y1, z0, fx, fy - 1.0, fz);
    let n110 = corner(seed, x1, y1, z0, fx - 1.0, fy - 1.0, fz);
    let n001 = corner(seed, x0, y0, z1, fx, fy, fz - 1.0);
    let n101 = corner(seed, x1, y0, z1, fx - 1.0, fy, fz - 1.0);
    let n011 = corner(seed, x0, y1, z1, fx, fy - 1.0, fz - 1.0);
    let n111 = corner(seed, x1, y1, z1, fx - 1.0, fy - 1.0, fz - 1.0);

    let nx00 = lerp(n000, n100, u);
    let nx10 = lerp(n010, n110, u);
    let nx01 = lerp(n001, n101, u);
    let nx11 = lerp(n011, n111, u);

    let nxy0 = lerp(nx00, nx10, v);
    let nxy1 = lerp(nx01, nx11, v);

    let n = lerp(nxy0, nxy1, w);

    // The raw gradient-noise range for these gradients is about
    // [-1/sqrt(2)*sqrt(2), ...]; empirically within ~[-1,1] after scaling.
    // A scale of ~1.0 keeps values comfortably inside [-1, 1].
    (n * 1.0).clamp(-1.0, 1.0)
}

/// Single-octave value/gradient noise in [-1,1].
pub fn value_noise(seed: u64, p: Vec3) -> f32 {
    gradient_noise(seed, p)
}

/// Fractal-Brownian-motion field over unit-sphere (or world-space) positions.
#[derive(Debug, Clone)]
pub struct Fbm {
    pub seed: u64,
    pub octaves: u32,
    pub frequency: f32,
    pub lacunarity: f32,
    pub gain: f32,
}

impl Fbm {
    pub fn new(seed: u64, octaves: u32, frequency: f32) -> Self {
        Self {
            seed,
            octaves,
            frequency,
            lacunarity: 2.0,
            gain: 0.5,
        }
    }

    /// Sample in roughly [-1, 1]. Sums `octaves` octaves of gradient noise,
    /// scaling frequency by `lacunarity` and amplitude by `gain` each octave,
    /// then normalizing by the total amplitude so the output stays bounded.
    pub fn sample(&self, p: Vec3) -> f32 {
        let octaves = self.octaves.max(1);
        let mut freq = self.frequency;
        let mut amp = 1.0_f32;
        let mut sum = 0.0_f32;
        let mut total_amp = 0.0_f32;

        for o in 0..octaves {
            // Decorrelate octaves by deriving a per-octave seed.
            let oseed = mix64(self.seed ^ (o as u64).wrapping_mul(0x68E3_1DA4));
            let sp = p.mul_scalar(freq);
            sum += gradient_noise(oseed, sp) * amp;
            total_amp += amp;
            freq *= self.lacunarity;
            amp *= self.gain;
        }

        if total_amp > 0.0 {
            (sum / total_amp).clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }

    /// Domain-warped sample. Audit: OW-E17 terrain_warp. Offsets the input
    /// point by a vector-valued noise field (three decorrelated FBM channels)
    /// scaled by `warp`, then samples the base field at the warped location.
    pub fn sample_warped(&self, p: Vec3, warp: f32) -> f32 {
        if warp == 0.0 {
            return self.sample(p);
        }
        // Three decorrelated offset fields built from distinct seeds.
        let qx = Fbm {
            seed: mix64(self.seed ^ 0x1111_1111),
            ..self.clone()
        };
        let qy = Fbm {
            seed: mix64(self.seed ^ 0x2222_2222),
            ..self.clone()
        };
        let qz = Fbm {
            seed: mix64(self.seed ^ 0x3333_3333),
            ..self.clone()
        };

        let offset = Vec3::new(qx.sample(p), qy.sample(p), qz.sample(p)).mul_scalar(warp);
        let warped = p.add(offset);
        self.sample(warped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn value_noise_is_deterministic() {
        let p = Vec3::new(1.5, -2.25, 3.75);
        let a = value_noise(42, p);
        let b = value_noise(42, p);
        assert_eq!(a, b, "same seed + same point must match exactly");
    }

    #[test]
    fn value_noise_differs_by_seed() {
        let p = Vec3::new(0.3, 0.7, -0.4);
        let a = value_noise(1, p);
        let b = value_noise(2, p);
        assert!(
            a != b,
            "different seeds should generally differ: {a} vs {b}"
        );
    }

    #[test]
    fn value_noise_bounded() {
        for i in 0..2000 {
            let f = i as f32;
            let p = Vec3::new(f * 0.131, -f * 0.077, f * 0.213);
            let n = value_noise(7, p);
            assert!(n >= -1.0 && n <= 1.0, "value_noise out of range: {n}");
        }
    }

    #[test]
    fn value_noise_varies_across_space() {
        let seed = 99;
        let a = value_noise(seed, Vec3::new(0.5, 0.5, 0.5));
        let b = value_noise(seed, Vec3::new(10.5, 7.25, -3.5));
        let c = value_noise(seed, Vec3::new(-8.0, 2.0, 5.0));
        assert!(
            !(approx(a, b, 1.0e-6) && approx(b, c, 1.0e-6)),
            "noise should vary across space"
        );
    }

    #[test]
    fn fbm_is_deterministic() {
        let f = Fbm::new(123, 5, 1.0);
        let p = Vec3::new(2.0, -1.0, 0.5);
        assert_eq!(f.sample(p), f.sample(p));
    }

    #[test]
    fn fbm_bounded() {
        let f = Fbm::new(5150, 6, 0.9);
        for i in 0..3000 {
            let t = i as f32;
            let p = Vec3::new(t * 0.017, t * 0.029 - 5.0, -t * 0.011);
            let n = f.sample(p);
            assert!(n >= -1.0 && n <= 1.0, "fbm out of range: {n}");
        }
    }

    #[test]
    fn fbm_respects_octave_count() {
        // More octaves should generally produce a different (more detailed)
        // value at the same point.
        let p = Vec3::new(3.3, -2.2, 1.1);
        let one = Fbm::new(77, 1, 1.5).sample(p);
        let many = Fbm::new(77, 6, 1.5).sample(p);
        assert!(one != many, "octave count should affect output");
    }

    #[test]
    fn fbm_varies_across_space() {
        let f = Fbm::new(2024, 4, 1.0);
        // Use non-integer coordinates: gradient noise is exactly 0 at integer
        // lattice points, so integer samples would all read 0 and falsely match.
        let a = f.sample(Vec3::new(0.37, 0.91, -0.22));
        let b = f.sample(Vec3::new(5.13, 5.67, 5.41));
        assert!(a != b, "fbm should vary across space");
    }

    #[test]
    fn sample_warped_is_deterministic() {
        let f = Fbm::new(808, 4, 1.0);
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(f.sample_warped(p, 0.5), f.sample_warped(p, 0.5));
    }

    #[test]
    fn sample_warped_zero_warp_equals_sample() {
        let f = Fbm::new(303, 4, 1.0);
        let p = Vec3::new(-1.5, 0.25, 4.0);
        assert_eq!(f.sample_warped(p, 0.0), f.sample(p));
    }

    #[test]
    fn sample_warped_changes_output() {
        let f = Fbm::new(606, 5, 1.0);
        let p = Vec3::new(2.5, -3.5, 0.75);
        let plain = f.sample(p);
        let warped = f.sample_warped(p, 1.5);
        assert!(warped >= -1.0 && warped <= 1.0);
        assert!(
            plain != warped,
            "non-zero warp should generally change output"
        );
    }
}
