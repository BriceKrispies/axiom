//! A static **heightfield** collider surface: a regular grid of surface heights
//! over the collider's local XZ plane, centred on the body origin.
//!
//! `heights[iz * nx + ix]` is the surface `y` at local
//! `(x, z) = (ix·spacing_x − half_x, iz·spacing_z − half_z)`, so the grid is
//! centred: `x ∈ [−half_x, +half_x]`, `z ∈ [−half_z, +half_z]`. A heightfield is
//! **single-valued** (one height per column), which is exactly what a shallow
//! curved track surface is, and lets a sphere collide it by the deterministic
//! vertical-projection contact (exact for gentle slopes; see `contact_pair`).
//!
//! This type is private (`pub(crate)`): the facade takes/returns dimensioned
//! `Meters`, so the naked-float boundary stays clean.

use axiom_math::Vec3;

/// A regular `nx × nz` grid of local surface heights.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Heightfield {
    nx: u32,
    nz: u32,
    spacing_x: f32,
    spacing_z: f32,
    half_x: f32,
    half_z: f32,
    heights: Vec<f32>,
}

impl Heightfield {
    /// Build a heightfield from its grid dimensions, spacings, and row-major
    /// heights (`heights[iz*nx + ix]`). Callers validate `nx, nz ≥ 2`, positive
    /// spacings, and `heights.len() == nx·nz` at the facade.
    pub(crate) fn new(nx: u32, nz: u32, spacing_x: f32, spacing_z: f32, heights: Vec<f32>) -> Self {
        let half_x = (nx.saturating_sub(1) as f32) * spacing_x * 0.5;
        let half_z = (nz.saturating_sub(1) as f32) * spacing_z * 0.5;
        Heightfield { nx, nz, spacing_x, spacing_z, half_x, half_z, heights }
    }

    /// The grid height at integer indices (clamped into range).
    fn at(&self, ix: u32, iz: u32) -> f32 {
        let cx = ix.min(self.nx - 1);
        let cz = iz.min(self.nz - 1);
        self.heights[(cz * self.nx + cx) as usize]
    }

    /// The bilinear-sampled surface height at local `(x, z)`, clamped to the grid
    /// footprint (a flat skirt continues the edge height outside the grid).
    pub(crate) fn sample(&self, x: f32, z: f32) -> f32 {
        let fx = ((x + self.half_x) / self.spacing_x).clamp(0.0, (self.nx - 1) as f32);
        let fz = ((z + self.half_z) / self.spacing_z).clamp(0.0, (self.nz - 1) as f32);
        let ix0 = fx as u32;
        let iz0 = fz as u32;
        let tx = fx - ix0 as f32;
        let tz = fz - iz0 as f32;
        let h00 = self.at(ix0, iz0);
        let h10 = self.at(ix0 + 1, iz0);
        let h01 = self.at(ix0, iz0 + 1);
        let h11 = self.at(ix0 + 1, iz0 + 1);
        let low = h00 + (h10 - h00) * tx;
        let high = h01 + (h11 - h01) * tx;
        low + (high - low) * tz
    }

    /// The upward unit surface normal at local `(x, z)` — the central-difference
    /// gradient of the sampled height, `(−dh/dx, 1, −dh/dz)` normalized.
    pub(crate) fn normal_at(&self, x: f32, z: f32) -> Vec3 {
        let gx = (self.sample(x + self.spacing_x, z) - self.sample(x - self.spacing_x, z)) / (2.0 * self.spacing_x);
        let gz = (self.sample(x, z + self.spacing_z) - self.sample(x, z - self.spacing_z)) / (2.0 * self.spacing_z);
        let v = Vec3::new(-gx, 1.0, -gz);
        let len = v.length_squared().sqrt().max(f32::MIN_POSITIVE);
        v.mul_scalar(1.0 / len)
    }

    /// Whether local `(x, z)` falls within the grid's XZ footprint.
    pub(crate) fn within(&self, x: f32, z: f32) -> bool {
        (x >= -self.half_x) & (x <= self.half_x) & (z >= -self.half_z) & (z <= self.half_z)
    }

    /// The grid's `(min, max)` height.
    pub(crate) fn bounds(&self) -> (f32, f32) {
        let min = self.heights.iter().copied().fold(f32::INFINITY, f32::min);
        let max = self.heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (min, max)
    }

    /// The collider's local axis-aligned half-extents: the XZ half-span, and the
    /// larger of `|min|` / `|max|` in `y` so the body-centred AABB encloses the grid.
    pub(crate) fn half_extents(&self) -> Vec3 {
        let (min, max) = self.bounds();
        Vec3::new(self.half_x, min.abs().max(max.abs()), self.half_z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3×3 grid, 1 m spacing, flat at height 2.
    fn flat() -> Heightfield {
        Heightfield::new(3, 3, 1.0, 1.0, vec![2.0; 9])
    }

    /// A 3×3 grid tilted so height rises +1 per +1 x (a 45° ramp about z).
    fn ramp() -> Heightfield {
        Heightfield::new(3, 3, 1.0, 1.0, vec![-1.0, 0.0, 1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0])
    }

    #[test]
    fn flat_samples_are_constant_and_normal_is_up() {
        let h = flat();
        assert_eq!(h.sample(0.0, 0.0), 2.0);
        assert_eq!(h.sample(0.4, -0.3), 2.0);
        // Clamped outside the grid: still the edge (flat) height.
        assert_eq!(h.sample(100.0, -100.0), 2.0);
        let n = h.normal_at(0.0, 0.0);
        assert!((n.x).abs() < 1.0e-6 && (n.z).abs() < 1.0e-6 && (n.y - 1.0).abs() < 1.0e-6);
        assert!(h.within(0.5, -0.5) && !h.within(2.0, 0.0));
    }

    #[test]
    fn ramp_interpolates_and_tilts_its_normal() {
        let h = ramp();
        // Bilinear along x at the centre column.
        assert!((h.sample(0.0, 0.0) - 0.0).abs() < 1.0e-6);
        assert!((h.sample(0.5, 0.0) - 0.5).abs() < 1.0e-6);
        assert!((h.sample(-1.0, 0.0) + 1.0).abs() < 1.0e-6);
        // The normal tilts away from +x (gradient +1) and stays unit.
        let n = h.normal_at(0.0, 0.0);
        assert!(n.x < 0.0, "normal leans -x on a +x-rising ramp");
        assert!((n.length_squared() - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn bounds_and_half_extents_cover_the_grid() {
        let h = ramp();
        assert_eq!(h.bounds(), (-1.0, 1.0));
        let he = h.half_extents();
        assert_eq!(he.x, 1.0); // (3-1)*1/2
        assert_eq!(he.z, 1.0);
        assert_eq!(he.y, 1.0); // max(|-1|, |1|)
    }
}
