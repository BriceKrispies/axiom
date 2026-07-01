//! Single-octave 3D gradient (Perlin-style) noise, keyed by the kernel digest.
//!
//! The integer lattice is hashed with [`axiom_kernel::StableHash`]: a cell's
//! `(seed, xi, yi, zi)` coordinates are folded into one FNV-1a digest, which
//! selects a per-cell gradient [`axiom_math::Vec3`] from a fixed 12-direction
//! table. The noise at a point is the quintic-faded trilinear interpolation of the
//! eight surrounding corners' gradient·offset dot products. Deterministic and
//! platform-stable — the same seed + point always yields the same value, because
//! the only source of "randomness" is the kernel's canonical-bytes digest.

use axiom_kernel::StableHash;
use axiom_math::Vec3;

use crate::noise_value::NoiseValue;

/// The 12 classic Perlin gradient directions (toward the edge midpoints of a
/// cube). A cell's digest selects one of these; the dot with the corner offset is
/// the corner's contribution.
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

/// Hash an integer lattice cell + seed into a stable 64-bit digest. The
/// (possibly negative) coordinates are sign-extended into the 64-bit word space
/// and folded — with the seed — through the kernel's FNV-1a [`StableHash`], so
/// adjacent cells decorrelate fully and the result is identical on every platform.
fn hash_cell(seed: u64, xi: i32, yi: i32, zi: i32) -> u64 {
    StableHash::of_words(&[seed, xi as i64 as u64, yi as i64 as u64, zi as i64 as u64]).raw()
}

/// The gradient vector for a lattice cell, chosen from [`GRADS`] by its digest.
fn cell_gradient(h: u64) -> Vec3 {
    let g = GRADS[(h % 12) as usize];
    Vec3::new(g.0, g.1, g.2)
}

/// Quintic smoothstep fade, Perlin's `6t^5 - 15t^4 + 10t^3`.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Linear interpolation from `a` to `b` by `t`.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// One corner's contribution: the cell gradient dotted with the offset from that
/// corner to the sample point.
fn corner(seed: u64, ci: i32, cj: i32, ck: i32, dx: f32, dy: f32, dz: f32) -> f32 {
    cell_gradient(hash_cell(seed, ci, cj, ck)).dot(Vec3::new(dx, dy, dz))
}

/// Core 3D gradient noise as a raw `f32`, clamped to `[-1, 1]`. Straight-line
/// (floor / fade / lerp / table lookup) — the internal signal the typed
/// [`value_noise`] and [`crate::Fbm`] wrap.
pub(crate) fn raw_gradient_noise(seed: u64, p: Vec3) -> f32 {
    let xf = p.x.floor();
    let yf = p.y.floor();
    let zf = p.z.floor();

    let x0 = xf as i32;
    let y0 = yf as i32;
    let z0 = zf as i32;
    let x1 = x0.wrapping_add(1);
    let y1 = y0.wrapping_add(1);
    let z1 = z0.wrapping_add(1);

    let fx = p.x - xf;
    let fy = p.y - yf;
    let fz = p.z - zf;

    let u = fade(fx);
    let v = fade(fy);
    let w = fade(fz);

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

    lerp(nxy0, nxy1, w).clamp(-1.0, 1.0)
}

/// Single-octave value/gradient noise as a bounded [`NoiseValue`] in `[-1, 1]`.
/// Deterministic in `(seed, p)`.
pub fn value_noise(seed: u64, p: Vec3) -> NoiseValue {
    NoiseValue::from_signal(raw_gradient_noise(seed, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn is_deterministic() {
        let p = Vec3::new(1.5, -2.25, 3.75);
        assert_eq!(value_noise(42, p), value_noise(42, p));
    }

    #[test]
    fn differs_by_seed() {
        let p = Vec3::new(0.3, 0.7, -0.4);
        assert_ne!(value_noise(1, p), value_noise(2, p));
    }

    #[test]
    fn stays_bounded() {
        for i in 0..2000 {
            let f = i as f32;
            let p = Vec3::new(f * 0.131, -f * 0.077, f * 0.213);
            let n = value_noise(7, p).get();
            assert!((-1.0..=1.0).contains(&n), "value_noise out of range: {n}");
        }
    }

    #[test]
    fn varies_across_space() {
        let seed = 99;
        let a = value_noise(seed, Vec3::new(0.5, 0.5, 0.5)).get();
        let b = value_noise(seed, Vec3::new(10.5, 7.25, -3.5)).get();
        let c = value_noise(seed, Vec3::new(-8.0, 2.0, 5.0)).get();
        // Evaluate both comparisons unconditionally (`&`, not `&&`) so neither
        // operand is short-circuited away — keeps the test region-complete.
        let ab = approx(a, b, 1.0e-6);
        let bc = approx(b, c, 1.0e-6);
        assert!(!(ab & bc), "noise should vary across space");
    }

    #[test]
    fn adjacent_cells_decorrelate() {
        // Two lattice cells one step apart must (almost always) pick different
        // gradients — the StableHash-keyed lattice replaces the old splitmix mixer
        // and must still fully decorrelate neighbours. Scan a row of adjacent cells
        // and confirm not every gradient is identical.
        let seed = 0x5EED_D00D_1234_ABCD;
        let grads: Vec<Vec3> = (0..16)
            .map(|x| cell_gradient(hash_cell(seed, x, 0, 0)))
            .collect();
        let all_same = grads.iter().all(|g| *g == grads[0]);
        assert!(!all_same, "adjacent lattice cells failed to decorrelate");
    }

    /// Integer lattice points read exactly `0` (every corner offset is a basis
    /// gradient dotted with a zero/unit offset that cancels under interpolation) —
    /// exercises the floor / fade / lerp path at the lattice nodes.
    #[test]
    fn integer_lattice_points_are_zero() {
        let seed = 123;
        for i in -3..3 {
            let n = raw_gradient_noise(seed, Vec3::new(i as f32, (i + 1) as f32, (i - 1) as f32));
            assert!(n.abs() < 1.0e-6, "expected ~0 at integer lattice, got {n}");
        }
    }
}
