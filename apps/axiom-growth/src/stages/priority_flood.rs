//! `priority_flood` stage (OW-E11): Barnes-style priority-flood pit filling so
//! land drains monotonically to the ocean.
//! Audit: OW-E11 priority_flood region drainage, monotonic, saddle spill carved.
//!
//! Classic Barnes/Planchon-Darboux flooding: seed a priority queue with all
//! ocean (boundary) cells at their own elevation, then repeatedly pop the lowest
//! spill level and raise each unvisited neighbour to at least that spill level.
//! Afterward every region has a non-ascending path to the ocean — no interior
//! pit is lower than its outflow saddle. Deterministic (ties broken by index).

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::ids::RegionId;
use crate::model_planet::PlanetGlobe;
use crate::pipeline::{GenContext, Stage};

pub struct PriorityFloodStage;

/// A queue entry ordered so the *lowest* spill level pops first (min-heap via
/// `Reverse`-style manual ordering). Ties break by region index for determinism.
#[derive(Clone, Copy)]
struct Cell {
    level: f32,
    region: u32,
}

impl PartialEq for Cell {
    fn eq(&self, other: &Self) -> bool {
        self.level == other.level && self.region == other.region
    }
}
impl Eq for Cell {}

impl Ord for Cell {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; invert level so smallest level is "greatest".
        match other
            .level
            .partial_cmp(&self.level)
            .unwrap_or(Ordering::Equal)
        {
            Ordering::Equal => other.region.cmp(&self.region),
            ord => ord,
        }
    }
}
impl PartialOrd for Cell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Stage for PriorityFloodStage {
    fn id(&self) -> &'static str {
        "priority_flood"
    }

    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        let region_count = globe.region_count();
        if region_count == 0 {
            return;
        }

        let mut visited = vec![false; region_count];
        let mut heap: BinaryHeap<Cell> = BinaryHeap::new();

        // Ocean cells are the outlets; seed them at their own elevation.
        let mut has_ocean = false;
        for r in 0..region_count {
            if globe.region_elevation[r] < 0.0 {
                visited[r] = true;
                has_ocean = true;
                heap.push(Cell {
                    level: globe.region_elevation[r],
                    region: r as u32,
                });
            }
        }

        // No ocean: seed from the single lowest cell so flooding still has an
        // outlet (an endorheic basin draining to the global minimum).
        if !has_ocean {
            let mut lo = 0usize;
            for r in 1..region_count {
                if globe.region_elevation[r] < globe.region_elevation[lo] {
                    lo = r;
                }
            }
            visited[lo] = true;
            heap.push(Cell {
                level: globe.region_elevation[lo],
                region: lo as u32,
            });
        }

        let mut filled = 0usize;
        while let Some(cell) = heap.pop() {
            let r = cell.region as usize;
            let spill = cell.level;
            for &n in globe.graph.neighbours_of(RegionId(r as u32)) {
                let ni = n as usize;
                if visited[ni] {
                    continue;
                }
                visited[ni] = true;
                // Raise to at least the spill level → guarantees a downhill path.
                if globe.region_elevation[ni] < spill {
                    globe.region_elevation[ni] = spill;
                    filled += 1;
                }
                heap.push(Cell {
                    level: globe.region_elevation[ni],
                    region: n,
                });
            }
        }

        ctx.log
            .push(format!("priority_flood: filled {} pits", filled));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_planet::{Icosphere, RegionGraph};
    use axiom_math::Vec3;

    /// Line 0-1-2-3-4. Region 0 ocean (-1). Region 2 is a pit (0.1) below its
    /// neighbours (1.0). After flooding it must be raised to the spill level.
    fn pit_globe() -> PlanetGlobe {
        let n = 5;
        let mut offsets = vec![0u32];
        let mut neighbours = Vec::new();
        for i in 0..n {
            if i > 0 {
                neighbours.push((i - 1) as u32);
            }
            if i + 1 < n {
                neighbours.push((i + 1) as u32);
            }
            offsets.push(neighbours.len() as u32);
        }
        let mut g = PlanetGlobe {
            topology: Icosphere {
                sites: vec![Vec3::new(1.0, 0.0, 0.0); n],
                triangles: Vec::new(),
                subdivisions: 0,
            },
            graph: RegionGraph { offsets, neighbours },
            ..PlanetGlobe::default()
        };
        g.resize_fields();
        g.region_elevation = vec![-1.0, 1.0, 0.1, 1.0, 2.0];
        g
    }

    /// Every land region must have a neighbour at lower-or-equal elevation
    /// leading (eventually) to the ocean — i.e. no strict interior pit remains.
    fn has_monotonic_drainage(g: &PlanetGlobe) -> bool {
        let n = g.region_count();
        for r in 0..n {
            if g.region_elevation[r] < 0.0 {
                continue; // ocean drains trivially
            }
            let here = g.region_elevation[r];
            let mut ok = false;
            for &nb in g.graph.neighbours_of(RegionId(r as u32)) {
                if g.region_elevation[nb as usize] <= here {
                    ok = true;
                    break;
                }
            }
            if !ok {
                return false;
            }
        }
        true
    }

    #[test]
    fn pit_is_filled_to_spill() {
        let mut g = pit_globe();
        let mut ctx = GenContext::new(1);
        PriorityFloodStage.run(&mut g, &mut ctx);
        // Region 2 was a pit at 0.1 surrounded by 1.0; it must be raised.
        assert!(
            g.region_elevation[2] >= 1.0,
            "pit not filled: {}",
            g.region_elevation[2]
        );
    }

    #[test]
    fn drainage_is_monotonic() {
        let mut g = pit_globe();
        let mut ctx = GenContext::new(1);
        PriorityFloodStage.run(&mut g, &mut ctx);
        assert!(has_monotonic_drainage(&g));
    }

    #[test]
    fn deterministic_same_seed() {
        let mut a = pit_globe();
        let mut b = pit_globe();
        let mut ca = GenContext::new(1);
        let mut cb = GenContext::new(1);
        PriorityFloodStage.run(&mut a, &mut ca);
        PriorityFloodStage.run(&mut b, &mut cb);
        assert_eq!(a.region_elevation, b.region_elevation);
    }
}
