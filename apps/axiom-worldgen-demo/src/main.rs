//! Headless proof that the chunked-worldgen substrate streams, culls, and scales.
//!
//! Walks a camera across an endless procedural forest, composing `axiom-world`
//! (residency ring + frustum cull + distance LOD) with `axiom-forest` (per-chunk
//! trees over `axiom-scatter`), and reports the resident / visible / drawn-tree
//! counts each step. Then it replays the walk and asserts a byte-identical result,
//! proving determinism. This is app-tier orchestration — the app owns the chunk
//! cache and the terrain height; the modules own the reusable policy + generation.

use std::collections::BTreeMap;

use axiom_forest::{ForestApi, ForestConfig};
use axiom_kernel::{Meters, Ratio};
use axiom_math::{Aabb, Mat4, Vec3};
use axiom_scatter::{CellCoord, ScatterRule};
use axiom_streaming::ChunkCoord;
use axiom_world::{WorldApi, WorldConfig, WorldFramePlan};

/// World size of one chunk's side, in metres.
const CHUNK_M: f32 = 32.0;
/// Residency load radius (chunks) and hysteresis margin.
const LOAD_RADIUS: i32 = 3;
const MARGIN: i32 = 1;
/// How many steps the camera walks.
const STEPS: usize = 12;

/// Deterministic terrain height (metres) at world `(x, z)` — a couple of sines.
/// The production path swaps `axiom-noise`'s Fbm in here; streaming is unaffected.
fn ground(x: Meters, z: Meters) -> Meters {
    Meters::finite_or_zero(2.0 * (x.get() * 0.03).sin() + 1.5 * (z.get() * 0.025).cos())
}

/// A chunk's world AABB: its x/z square, y spanning the terrain band + tallest tree.
fn chunk_aabb(c: ChunkCoord) -> Aabb {
    let x = c.x as f32 * CHUNK_M;
    let z = c.z as f32 * CHUNK_M;
    Aabb::new(
        Vec3::new(x, -6.0, z),
        Vec3::new(x + CHUNK_M, 8.0, z + CHUNK_M),
    )
    .unwrap()
}

fn world() -> WorldApi {
    WorldApi::new(WorldConfig {
        chunk_size: Meters::finite_or_zero(CHUNK_M),
        load_radius: LOAD_RADIUS,
        margin: MARGIN,
        lod_bands: [50.0, 100.0, 170.0]
            .iter()
            .map(|b| Meters::finite_or_zero(*b))
            .collect(),
    })
}

fn forest_config() -> ForestConfig {
    ForestConfig {
        cell_size: Meters::finite_or_zero(CHUNK_M),
        scatter: ScatterRule {
            sites_per_side: 8,
            jitter: Ratio::new(0.7).unwrap(),
            fill: Ratio::new(0.75).unwrap(),
        },
        min_size: Meters::finite_or_zero(2.0),
        max_size: Meters::finite_or_zero(5.0),
    }
}

/// The camera at walk step `s`: eye advancing along −Z, looking forward + down.
fn camera_at(s: usize) -> (Vec3, Mat4) {
    let eye = Vec3::new(4.0, 6.0, 20.0 - s as f32 * CHUNK_M * 0.5);
    let target = eye.add(Vec3::new(0.6, -1.0, -20.0));
    let proj = Mat4::perspective(std::f32::consts::FRAC_PI_3, 16.0 / 9.0, 0.3, 600.0).unwrap();
    let view = Mat4::look_at(eye, target, Vec3::UNIT_Y).unwrap();
    (eye, proj.multiply(view))
}

/// One per-step record: resident chunks, visible chunks, trees drawn, trees
/// resident, and the visible-chunk LOD histogram (levels 0..=3).
type Step = (usize, usize, usize, usize, [usize; 4]);

/// Walk the camera across the streamed world, generating trees on load and
/// tearing them down on unload, and record each step.
fn walk(seed: u64) -> Vec<Step> {
    let mut w = world();
    let cfg = forest_config();
    let mut trees: BTreeMap<ChunkCoord, usize> = BTreeMap::new();
    let mut report = Vec::new();
    for s in 0..STEPS {
        let (eye, vp) = camera_at(s);
        let plan: WorldFramePlan = w.frame_plan(eye, vp, chunk_aabb);
        // Generate the newly-loaded chunks' trees; drop the unloaded ones.
        for c in &plan.load {
            let n = ForestApi::chunk_trees(seed, CellCoord::new(c.x, c.z), &cfg, ground).len();
            trees.insert(*c, n);
        }
        for c in &plan.unload {
            trees.remove(c);
        }
        let drawn: usize = plan
            .visible
            .iter()
            .map(|v| trees.get(&v.coord).copied().unwrap_or(0))
            .sum();
        let resident_trees: usize = trees.values().sum();
        let mut lod = [0usize; 4];
        for v in &plan.visible {
            lod[(v.lod as usize).min(3)] += 1;
        }
        report.push((trees.len(), plan.visible.len(), drawn, resident_trees, lod));
    }
    report
}

fn main() {
    let seed = 1337;
    let report = walk(seed);

    println!("Axiom worldgen streaming proof — {CHUNK_M} m chunks, load radius {LOAD_RADIUS}, walking -Z\n");
    println!(
        "{:>4}  {:>8}  {:>7}  {:>20}  {:>15}",
        "step", "resident", "visible", "trees drawn/resident", "lod 0/1/2/3"
    );
    for (s, (res, vis, drawn, tot, lod)) in report.iter().enumerate() {
        println!(
            "{s:>4}  {res:>8}  {vis:>7}  {:>20}  {:>15}",
            format!("{drawn}/{tot}"),
            format!("{}/{}/{}/{}", lod[0], lod[1], lod[2], lod[3]),
        );
    }

    // Streaming: the resident set stays inside the keep ring no matter how far we
    // walk — a bounded working set, not the whole (unbounded) world.
    let keep = 2 * (LOAD_RADIUS + MARGIN) + 1;
    let max_resident = report.iter().map(|r| r.0).max().unwrap();
    assert!(
        max_resident as i32 <= keep * keep,
        "resident set must stay within the keep ring"
    );

    // Culling: some steps draw strictly fewer trees than are resident (the ones
    // outside the frustum are never touched).
    assert!(
        report.iter().any(|r| r.2 < r.3),
        "frustum culling must hide some resident trees"
    );

    // Determinism: replaying the identical walk yields the identical report.
    assert_eq!(
        walk(seed),
        report,
        "the streamed world must replay identically"
    );

    println!(
        "\nOK: streamed (resident stays <= {} of an endless world), culled (drawn < resident), deterministic (identical replay).",
        keep * keep,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_culls_and_replays_deterministically() {
        let report = walk(1337);
        // Streaming: bounded working set regardless of walk length.
        let keep = 2 * (LOAD_RADIUS + MARGIN) + 1;
        assert!(report.iter().map(|r| r.0).max().unwrap() as i32 <= keep * keep);
        // Culling: some step draws fewer trees than are resident.
        assert!(report.iter().any(|r| r.2 < r.3));
        // Culling actually removes a lot: at least one step draws under half the
        // resident trees (the frustum hides the majority of the ring).
        assert!(report.iter().any(|r| r.2 * 2 < r.3));
        // Determinism: identical replay.
        assert_eq!(walk(1337), report);
    }
}
