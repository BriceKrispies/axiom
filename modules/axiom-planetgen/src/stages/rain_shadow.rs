//! `rain_shadow`: orographic lift windward, depletion leeward.
//!
//! For each land region we look at its most-upwind and most-downwind neighbour.
//! Where this region rises above its upwind neighbour the air rains out (windward
//! gain); the downwind neighbour, falling away, loses moisture (leeward loss).
//! Branchless: each land region emits 0-2 `(target, delta)` contributions that
//! are scattered into a delta field, then applied.

use axiom_geosphere::RegionId;

use crate::globe::PlanetGlobe;

/// Moisture gained per unit of windward elevation rise.
const WINDWARD_GAIN: f32 = 0.6;
/// Moisture lost per unit of leeward elevation drop.
const LEEWARD_LOSS: f32 = 0.6;

/// The most-upwind and most-downwind neighbour of region `r` (each `None` when it
/// has no neighbour scoring above zero). One branchless fold selects both.
fn upwind_downwind(globe: &PlanetGlobe, r: usize) -> (Option<usize>, Option<usize>) {
    let site = globe.topology.sites[r];
    let wind = globe.region_wind[r];
    let (up, _, down, _) = globe.graph.neighbours_of(RegionId(r as u32)).iter().fold(
        (None::<usize>, 0.0f32, None::<usize>, 0.0f32),
        |(up, up_s, down, down_s), &n| {
            let dir = globe.topology.sites[n as usize].subtract(site);
            let dir = dir.normalize().unwrap_or(dir);
            let uw = dir.dot(wind.mul_scalar(-1.0));
            let dw = dir.dot(wind);
            let (up2, up_s2) = [(up, up_s), (Some(n as usize), uw)][usize::from(uw > up_s)];
            let (down2, down_s2) =
                [(down, down_s), (Some(n as usize), dw)][usize::from(dw > down_s)];
            (up2, up_s2, down2, down_s2)
        },
    );
    (up, down)
}

/// The 0-2 moisture contributions a land region `r` makes: windward gain to
/// itself, leeward loss to its downwind neighbour.
fn region_contributions(globe: &PlanetGlobe, r: usize) -> impl Iterator<Item = (usize, f32)> {
    let (up, down) = upwind_downwind(globe, r);
    let h = globe.region_elevation[r];
    let windward = up.and_then(|u| {
        let rise = h - globe.region_elevation[u];
        (rise > 0.0).then_some((r, WINDWARD_GAIN * rise))
    });
    let leeward = down.and_then(|d| {
        let drop = h - globe.region_elevation[d];
        (drop > 0.0).then_some((d, -LEEWARD_LOSS * drop))
    });
    windward.into_iter().chain(leeward)
}

pub(crate) fn rain_shadow(globe: &mut PlanetGlobe) {
    let n = globe.region_count();
    let contributions: Vec<(usize, f32)> = (0..n)
        .filter(|&r| globe.region_elevation[r] >= 0.0)
        .flat_map(|r| region_contributions(globe, r))
        .collect();

    let mut delta = vec![0.0f32; n];
    contributions.iter().for_each(|&(t, a)| delta[t] += a);

    let region_moisture: Vec<f32> = (0..n)
        .map(|r| {
            let land = globe.region_elevation[r] >= 0.0;
            let updated = (globe.region_moisture[r] + delta[r]).clamp(0.0, 1.0);
            [globe.region_moisture[r], updated][usize::from(land)]
        })
        .collect();
    globe.region_moisture = region_moisture;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_geosphere::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line of 4 regions along +x, wind blowing +x. Region 1 is a tall ridge, so 1
    /// is windward-rising (wet) and region 2 behind it is in shadow (dry).
    fn ridge_globe() -> PlanetGlobe {
        let sites: Vec<Vec3> = (0..4)
            .map(|i| Vec3::new(i as f32 + 1.0, 0.0, 0.0))
            .collect();
        let offsets = vec![0u32, 1, 3, 5, 6];
        let neighbours = vec![1, 0, 2, 1, 3, 2];
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites,
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph {
                offsets,
                neighbours,
            },
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_elevation = vec![0.1, 2.0, 0.1, 0.1];
        g.region_wind = vec![Vec3::new(1.0, 0.0, 0.0); 4];
        g.region_moisture = vec![0.5, 0.5, 0.5, 0.5];
        g
    }

    #[test]
    fn windward_wetter_leeward_drier() {
        let mut g = ridge_globe();
        let before = g.region_moisture.clone();
        rain_shadow(&mut g);
        assert!(g.region_moisture.iter().all(|&m| (0.0..=1.0).contains(&m)));
        assert!(g.region_moisture[1] >= before[1]);
        assert!(g.region_moisture[2] < before[2]);
    }

    #[test]
    fn ocean_region_is_untouched() {
        // Region 0 is ocean; its moisture must be left exactly as-is.
        let mut g = ridge_globe();
        g.region_elevation[0] = -1.0;
        g.region_moisture[0] = 0.42;
        rain_shadow(&mut g);
        assert_eq!(g.region_moisture[0], 0.42);
    }

    #[test]
    fn empty_globe_is_a_noop() {
        let mut g = PlanetGlobe::default();
        rain_shadow(&mut g);
        assert!(g.region_moisture.is_empty());
    }

    #[test]
    fn deterministic_same_input() {
        let mut a = ridge_globe();
        let mut b = ridge_globe();
        rain_shadow(&mut a);
        rain_shadow(&mut b);
        assert_eq!(a.region_moisture, b.region_moisture);
    }
}
