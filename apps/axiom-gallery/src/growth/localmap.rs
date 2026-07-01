//! Game-world local tangent frame anchored on the planet.
use crate::growth::ids::ChunkCoord;
use crate::growth::model_planet::PlanetSurfaceAtlas;
use crate::growth::model_world::{GameWorldLocalMap, CELL_SIZE_M, CHUNK_SIZE_CELLS};
use axiom_math::Vec3;

impl GameWorldLocalMap {
    /// Anchor at a hospitable play start: a land region (elevation >= 0) as close
    /// as possible to mid-latitude (|lat| ~ 30 deg), avoiding the poles where the
    /// tangent basis degenerates. Falls back to any land region, then region 0.
    pub fn anchored(atlas: &PlanetSurfaceAtlas) -> Self {
        // Prefer land near mid-latitude. We score every region and keep the best;
        // this is deterministic and independent of region ordering.
        const TARGET_LAT_RAD: f32 = std::f32::consts::FRAC_PI_6; // ~30 degrees.
        let mut best_idx: Option<usize> = None;
        let mut best_score = f32::INFINITY;
        for (i, site) in atlas.sites.iter().enumerate() {
            let is_land = atlas.region_elevation.get(i).copied().unwrap_or(-1.0) >= 0.0;
            if !is_land {
                continue;
            }
            let lat = axiom_math::latitude(*site).get();
            // Distance from the target band (either hemisphere) plus a small
            // penalty for being very near the poles.
            let band = (lat.abs() - TARGET_LAT_RAD).abs();
            let pole_penalty = if lat.abs() > 1.30 { 10.0 } else { 0.0 };
            let score = band + pole_penalty;
            if score < best_score {
                best_score = score;
                best_idx = Some(i);
            }
        }

        // Fallbacks: first land region, else region 0.
        let anchor_idx = best_idx
            .or_else(|| atlas.region_elevation.iter().position(|&e| e >= 0.0))
            .unwrap_or(0);

        let dir = atlas
            .sites
            .get(anchor_idx)
            .copied()
            .unwrap_or(Vec3::new(0.0, 1.0, 0.0))
            .normalize()
            .unwrap_or(Vec3::new(0.0, 1.0, 0.0));
        Self::anchored_at(atlas, dir)
    }

    /// Anchor the local tangent frame at a caller-chosen unit direction on the
    /// planet, instead of auto-picking a hospitable land region as
    /// [`Self::anchored`] does. This is the path the overworld map-pick flow uses:
    /// the player clicks a spot, that pixel maps to a lat/long → unit direction,
    /// and the descended first-person world is anchored exactly there. The input
    /// is normalised (falling back to the north pole if it is degenerate) so the
    /// tangent basis is always well-formed.
    pub fn anchored_at(atlas: &PlanetSurfaceAtlas, unit_dir: Vec3) -> Self {
        let dir = unit_dir.normalize().unwrap_or(Vec3::new(0.0, 1.0, 0.0));
        let (east, north) = axiom_math::tangent_basis(dir);
        Self {
            anchor_dir: [dir.x, dir.y, dir.z],
            tangent_east: [east.x, east.y, east.z],
            tangent_north: [north.x, north.y, north.z],
            planet_radius_m: atlas.planet_radius_m.get(),
        }
    }

    /// Map a chunk-local world-metre offset to a unit direction on the sphere.
    ///
    /// We use the exponential map of the sphere at the anchor: travel along the
    /// geodesic in the tangent direction `(x_m, z_m)` by the arc angle
    /// `dist_m / radius`. This is the same continuous, well-defined function for
    /// every chunk, so two world positions that coincide (e.g. a shared chunk
    /// edge) always map to the *same* unit direction — which is what lets
    /// neighbouring chunks share macro context exactly.
    pub fn world_metres_to_unit_dir(&self, x_m: f32, z_m: f32) -> [f32; 3] {
        let dir = Vec3::new(self.anchor_dir[0], self.anchor_dir[1], self.anchor_dir[2]);
        let east = Vec3::new(
            self.tangent_east[0],
            self.tangent_east[1],
            self.tangent_east[2],
        );
        let north = Vec3::new(
            self.tangent_north[0],
            self.tangent_north[1],
            self.tangent_north[2],
        );
        let r = self.planet_radius_m.max(1.0);

        // Tangent-plane displacement vector and its magnitude in metres.
        let tangent = east.mul_scalar(x_m).add(north.mul_scalar(z_m));
        let dist_m = tangent.length();
        if dist_m <= f32::EPSILON {
            return [dir.x, dir.y, dir.z];
        }
        // Unit tangent direction of travel.
        let t_hat = tangent.normalize().unwrap_or(east);
        // Exponential map: p = cos(theta) * dir + sin(theta) * t_hat.
        let theta = dist_m / r;
        let (s, c) = theta.sin_cos();
        let p = dir.mul_scalar(c).add(t_hat.mul_scalar(s));
        let p = p.normalize().unwrap_or(dir);
        [p.x, p.y, p.z]
    }

    /// Chunk coord to the world-metre origin of its corner.
    pub fn chunk_origin_m(coord: ChunkCoord) -> (f32, f32) {
        let s = CHUNK_SIZE_CELLS as f32 * CELL_SIZE_M;
        (coord.x as f32 * s, coord.z as f32 * s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::model_planet::PlanetSurfaceAtlas;

    fn atlas() -> PlanetSurfaceAtlas {
        PlanetSurfaceAtlas {
            planet_radius_m: axiom_kernel::Meters::finite_or_zero(6_000_000.0),
            ..PlanetSurfaceAtlas::default()
        }
    }

    #[test]
    fn anchored_at_uses_the_caller_direction_and_keeps_radius() {
        // A caller-chosen direction (off-axis, non-unit) is normalised into the
        // anchor, and the planet radius is carried through.
        let a = atlas();
        let lm = GameWorldLocalMap::anchored_at(&a, Vec3::new(0.0, 0.0, 2.0));
        assert!((lm.anchor_dir[2] - 1.0).abs() < 1.0e-5, "anchor points +Z");
        assert!(lm.anchor_dir[0].abs() < 1.0e-5 && lm.anchor_dir[1].abs() < 1.0e-5);
        assert_eq!(lm.planet_radius_m, a.planet_radius_m.get());
        // The origin maps to the anchor direction exactly.
        let at_origin = lm.world_metres_to_unit_dir(0.0, 0.0);
        assert_eq!(at_origin, lm.anchor_dir);
    }

    #[test]
    fn anchored_at_tangent_basis_is_orthonormal_to_the_anchor() {
        let lm = GameWorldLocalMap::anchored_at(&atlas(), Vec3::new(0.3, 0.4, 0.5));
        let dir = Vec3::new(lm.anchor_dir[0], lm.anchor_dir[1], lm.anchor_dir[2]);
        let east = Vec3::new(lm.tangent_east[0], lm.tangent_east[1], lm.tangent_east[2]);
        let north = Vec3::new(
            lm.tangent_north[0],
            lm.tangent_north[1],
            lm.tangent_north[2],
        );
        assert!(dir.dot(east).abs() < 1.0e-4, "east _|_ anchor");
        assert!(dir.dot(north).abs() < 1.0e-4, "north _|_ anchor");
        assert!(east.dot(north).abs() < 1.0e-4, "east _|_ north");
    }

    #[test]
    fn anchored_at_degenerate_input_falls_back_to_a_pole() {
        // A zero vector cannot be normalised; the anchor falls back to +Y.
        let lm = GameWorldLocalMap::anchored_at(&atlas(), Vec3::ZERO);
        assert_eq!(lm.anchor_dir, [0.0, 1.0, 0.0]);
    }
}
