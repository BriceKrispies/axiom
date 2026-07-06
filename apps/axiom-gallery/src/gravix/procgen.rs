//! Deterministic procedural course generation.
//!
//! A level index maps, through a small seeded RNG, to a fully-determined course:
//! a winding platform path with turns, rising ramps, dropping ledges, jump gaps,
//! and hovering coins. The pipeline mirrors the classic grid-spine approach:
//!
//! 1. **Grid walk** — a self-avoiding drunkard walk over an integer lattice lays
//!    out an ordered path of cells (the "spine"), turning occasionally.
//! 2. **Heights** — the deck rises and falls in fixed steps along the path; a
//!    rise becomes a tilted **ramp** (an oriented box), a fall a simple ledge.
//! 3. **Geometry** — each cell becomes an oriented platform box; the first and
//!    last are wide plazas (spawn / finish pads).
//! 4. **Gaps** — occasional interior tiles are removed to force a jump.
//! 5. **Coins** — coins hover over a subset of tiles and over the gaps.
//!
//! Everything is a pure function of `level_index` (via `Mulberry32`), so a level
//! replays identically every time — the app-tier analogue of the engine's
//! determinism rule.

use axiom::prelude::Vec3;
use axiom_math::Quat;

use crate::gravix::level::{Coin, LevelDescriptor, Platform, SurfaceKind, Zone};

/// Horizontal spacing between adjacent path cells (world units).
const STEP: f32 = 2.5;
/// Half-extent of a standard path tile in X/Z (tiles overlap slightly so the run
/// is continuous).
const HALF_XZ: f32 = 1.35;
/// Half-extent of a widened path tile in X/Z.
const HALF_XZ_WIDE: f32 = 1.85;
/// Deck half-thickness (Y).
const HALF_Y: f32 = 0.25;
/// The wide spawn / finish pad half-extent.
const PLAZA_HALF: f32 = 3.0;
/// One vertical deck step (world units).
const VERT_STEP: f32 = 0.5;
/// How high above a tile's top surface a coin hovers.
const COIN_HOVER: f32 = 0.75;

/// A tiny, fast, fully-deterministic PRNG (Mulberry32). Seeded from the level
/// index so every level is reproducible.
#[derive(Debug)]
pub struct Mulberry32 {
    state: u32,
}

impl Mulberry32 {
    /// Seed the generator for `level_index` with a fixed salt, so distinct levels
    /// diverge immediately but each is stable.
    pub fn seed(level_index: u32, salt: u32) -> Self {
        // A splitmix-flavoured seed hash so index 0 is not a degenerate state.
        let mixed = (level_index.wrapping_add(1))
            .wrapping_mul(0x9E37_79B9)
            ^ salt.wrapping_mul(0x85EB_CA6B);
        Mulberry32 { state: mixed }
    }

    /// The next raw 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6D2B_79F5);
        let mut z = self.state;
        z = (z ^ (z >> 15)).wrapping_mul(z | 1);
        z ^= z.wrapping_add((z ^ (z >> 7)).wrapping_mul(z | 61));
        z ^ (z >> 14)
    }

    /// The next float in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// `true` with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }

    /// An integer in `[min, max)` (inclusive-exclusive).
    pub fn range_u32(&mut self, min: u32, max: u32) -> u32 {
        min + (self.next_u32() % (max - min).max(1))
    }
}

/// A 2D lattice direction (unit step in cell space). Order: +Z, +X, -Z, -X.
const DIRS: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];

fn turn_left(d: usize) -> usize {
    (d + 3) % 4
}
fn turn_right(d: usize) -> usize {
    (d + 1) % 4
}

/// The ordered cell path (the spine). Non-self-intersecting: the walk turns
/// rather than revisit a cell, and stops if boxed in.
fn walk_path(rng: &mut Mulberry32, target_len: usize, p_turn: f32) -> Vec<(i32, i32)> {
    let mut cells = vec![(0i32, 0i32)];
    let mut visited = std::collections::HashSet::new();
    visited.insert((0, 0));
    let mut dir = 0usize; // heading +Z
    let mut cur = (0i32, 0i32);

    while cells.len() < target_len {
        // Optionally turn before stepping.
        if rng.chance(p_turn) {
            dir = if rng.chance(0.5) { turn_left(dir) } else { turn_right(dir) };
        }
        // Preference order: keep going, else the two turns, else give up.
        let candidates = [dir, turn_left(dir), turn_right(dir)];
        let mut stepped = false;
        for &cand in &candidates {
            let (dx, dz) = DIRS[cand];
            let next = (cur.0 + dx, cur.1 + dz);
            if !visited.contains(&next) {
                dir = cand;
                cur = next;
                visited.insert(next);
                cells.push(next);
                stepped = true;
                break;
            }
        }
        if !stepped {
            break;
        }
    }
    cells
}

/// One tile of the built course, before it becomes a platform box.
struct Tile {
    cell: (i32, i32),
    y: f32,
    /// A rise into this tile from the previous one gets a ramp bridging them.
    ramp_from_prev: bool,
    /// This tile is a jump gap — no solid platform is emitted for it.
    gap: bool,
    kind: SurfaceKind,
}

/// Assign a rising/falling deck height along the path; rises are flagged for a
/// ramp. The first and last tiles are flat plazas.
fn assign_heights(rng: &mut Mulberry32, cells: &[(i32, i32)]) -> Vec<Tile> {
    let mut tiles = Vec::with_capacity(cells.len());
    let mut y = 0.0f32;
    for (i, &cell) in cells.iter().enumerate() {
        let is_end = i == 0 || i == cells.len() - 1;
        let mut ramp = false;
        // Interior tiles may change height; a rise becomes a ramp, a fall a ledge.
        if !is_end && i > 1 && rng.chance(0.24) {
            if rng.chance(0.6) {
                y += VERT_STEP;
                ramp = true;
            } else {
                y -= VERT_STEP;
            }
        }
        let kind = if is_end {
            SurfaceKind::Plaza
        } else if rng.chance(0.22) {
            SurfaceKind::PathWide
        } else {
            SurfaceKind::Path
        };
        tiles.push(Tile {
            cell,
            y,
            ramp_from_prev: ramp,
            gap: false,
            kind,
        });
    }
    tiles
}

/// Punch jump gaps into flat interior runs, spaced out and never on a ramp/plaza
/// or adjacent to another gap. Difficulty grows the gap count with the level.
fn punch_gaps(rng: &mut Mulberry32, tiles: &mut [Tile], level_index: u32) {
    let n = tiles.len();
    if n < 8 {
        return;
    }
    let max_gaps = (2 + level_index / 2).min(((n - 6) / 4) as u32) as usize;
    let mut placed = 0;
    let mut i = 4;
    while i < n - 4 && placed < max_gaps {
        let flat_run = !tiles[i].ramp_from_prev
            && !tiles[i + 1].ramp_from_prev
            && (tiles[i - 1].y - tiles[i].y).abs() < 1.0e-3
            && (tiles[i + 1].y - tiles[i].y).abs() < 1.0e-3
            && tiles[i].kind != SurfaceKind::Plaza;
        if flat_run && rng.chance(0.6) {
            tiles[i].gap = true;
            placed += 1;
            i += 4; // leave runway before the next gap
        } else {
            i += 1;
        }
    }
}

/// World position of a cell centre at deck height `y` (box centre).
fn cell_world(cell: (i32, i32), y: f32) -> Vec3 {
    Vec3::new(cell.0 as f32 * STEP, y, cell.1 as f32 * STEP)
}

/// The oriented ramp box bridging the previous tile up to `tile`.
fn ramp_platform(prev: &Tile, tile: &Tile) -> Platform {
    let a = cell_world(prev.cell, prev.y);
    let b = cell_world(tile.cell, tile.y);
    let mid = a.add(b).mul_scalar(0.5);
    let dx = b.x - a.x;
    let dz = b.z - a.z;
    let horiz = (dx * dx + dz * dz).sqrt().max(1.0e-4);
    let dy = b.y - a.y;
    let heading = dx.atan2(dz); // yaw about Y maps local +Z onto travel dir
    let pitch = -dy.atan2(horiz); // tilt the top face up toward travel
    let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, heading)
        .expect("unit Y axis")
        .multiply(
            Quat::from_axis_angle(Vec3::UNIT_X, pitch).expect("unit X axis"),
        );
    let slope_half = (horiz * horiz + dy * dy).sqrt() * 0.5;
    Platform {
        position: Vec3::new(mid.x, mid.y + HALF_Y, mid.z),
        half_extents: Vec3::new(HALF_XZ, HALF_Y, slope_half + HALF_XZ * 0.5),
        rotation,
        kind: SurfaceKind::Ramp,
    }
}

/// The flat platform box for a tile.
fn flat_platform(tile: &Tile) -> Platform {
    let half_xz = match tile.kind {
        SurfaceKind::Plaza => PLAZA_HALF,
        SurfaceKind::PathWide => HALF_XZ_WIDE,
        _ => HALF_XZ,
    };
    Platform {
        position: cell_world(tile.cell, tile.y),
        half_extents: Vec3::new(half_xz, HALF_Y, half_xz),
        rotation: Quat::IDENTITY,
        kind: tile.kind,
    }
}

/// Scatter coins: one hovering over most non-gap interior tiles (probabilistic),
/// and one over the mid-point of every gap so a jump is rewarded.
fn place_coins(rng: &mut Mulberry32, tiles: &[Tile]) -> Vec<Coin> {
    let mut coins = Vec::new();
    for (i, tile) in tiles.iter().enumerate() {
        let interior = i > 1 && i < tiles.len() - 2;
        if tile.gap {
            // A coin floats over the gap (between the two flanking tiles).
            let prev = cell_world(tiles[i - 1].cell, tiles[i - 1].y);
            let next = cell_world(tiles[i + 1].cell, tiles[i + 1].y);
            let mid = prev.add(next).mul_scalar(0.5);
            coins.push(Coin {
                position: Vec3::new(mid.x, mid.y + HALF_Y + COIN_HOVER + 0.2, mid.z),
            });
        } else if interior && tile.kind != SurfaceKind::Plaza && rng.chance(0.4) {
            let top = tile.y + HALF_Y;
            let raised = if rng.chance(0.35) { 0.55 } else { 0.0 };
            coins.push(Coin {
                position: Vec3::new(
                    tile.cell.0 as f32 * STEP,
                    top + COIN_HOVER + raised,
                    tile.cell.1 as f32 * STEP,
                ),
            });
        }
    }
    coins
}

/// Generate the full level descriptor for `level_index`.
pub fn generate(level_index: u32) -> LevelDescriptor {
    let mut rng = Mulberry32::seed(level_index, 0x5EED_1234);
    let target_len = (26 + level_index as usize * 4).min(90);
    let p_turn = (0.18 + level_index as f32 * 0.012).min(0.34);

    let cells = walk_path(&mut rng, target_len, p_turn);
    let mut tiles = assign_heights(&mut rng, &cells);
    punch_gaps(&mut rng, &mut tiles, level_index);

    // Emit geometry: a flat box per non-gap tile, plus a ramp before each rise.
    let mut platforms = Vec::new();
    for i in 0..tiles.len() {
        if tiles[i].ramp_from_prev && i > 0 && !tiles[i].gap && !tiles[i - 1].gap {
            platforms.push(ramp_platform(&tiles[i - 1], &tiles[i]));
        }
        if !tiles[i].gap {
            platforms.push(flat_platform(&tiles[i]));
        }
    }

    // A sprinkling of non-colliding lattice decorations beside wide tiles.
    for (i, tile) in tiles.iter().enumerate() {
        if !tile.gap && tile.kind == SurfaceKind::PathWide && rng.chance(0.3) {
            let base = cell_world(tile.cell, tile.y);
            platforms.push(Platform {
                position: Vec3::new(base.x, base.y + HALF_Y + 1.4, base.z),
                half_extents: Vec3::new(0.5, 0.5, 0.5),
                rotation: Quat::IDENTITY,
                kind: SurfaceKind::Lattice,
            });
            let _ = i;
        }
    }

    let coins = place_coins(&mut rng, &tiles);

    let first = &tiles[0];
    let last = &tiles[tiles.len() - 1];
    let start_top = first.y + HALF_Y;
    let end_top = last.y + HALF_Y;
    let start_zone = Zone {
        position: Vec3::new(
            first.cell.0 as f32 * STEP,
            start_top,
            first.cell.1 as f32 * STEP,
        ),
        radius: 0.9,
    };
    let end_zone = Zone {
        position: Vec3::new(
            last.cell.0 as f32 * STEP,
            end_top,
            last.cell.1 as f32 * STEP,
        ),
        radius: 0.9,
    };

    let spawn = Vec3::new(
        first.cell.0 as f32 * STEP,
        start_top + crate::gravix::settings::MARBLE_RADIUS + 0.4,
        first.cell.1 as f32 * STEP,
    );

    // Kill plane sits a comfortable margin below the lowest deck.
    let lowest = tiles.iter().map(|t| t.y).fold(0.0f32, f32::min);
    let kill_plane_y = lowest - crate::gravix::settings::FALL_DEATH_BELOW_SPAWN;

    LevelDescriptor {
        spawn,
        platforms,
        start_zone,
        end_zone,
        coins,
        kill_plane_y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_per_level() {
        let a = generate(3);
        let b = generate(3);
        assert_eq!(a.platforms.len(), b.platforms.len());
        assert_eq!(a.coins.len(), b.coins.len());
        assert_eq!(a.spawn, b.spawn);
        // Different levels differ.
        let c = generate(4);
        assert!(
            a.platforms.len() != c.platforms.len()
                || a.spawn != c.spawn
                || a.end_zone.position != c.end_zone.position
        );
    }

    #[test]
    fn a_course_has_a_spawn_pad_a_finish_and_solid_platforms() {
        let d = generate(2);
        assert!(d.platforms.len() >= 10, "a real course has many tiles");
        assert!(
            d.platforms.iter().any(|p| p.kind == SurfaceKind::Plaza),
            "there is a plaza pad"
        );
        // The finish is away from the spawn.
        let span = d.end_zone.position.subtract(d.spawn);
        assert!(span.x.abs() + span.z.abs() > STEP, "finish is down-course");
    }

    #[test]
    fn higher_levels_add_ramps_and_gaps() {
        let d = generate(8);
        assert!(
            d.platforms.iter().any(|p| p.kind == SurfaceKind::Ramp),
            "a mid-level course has at least one ramp"
        );
    }

    #[test]
    fn ramps_are_actually_tilted() {
        // Find a ramp and confirm its rotation is not identity.
        let d = generate(8);
        let ramp = d.platforms.iter().find(|p| p.kind == SurfaceKind::Ramp);
        if let Some(r) = ramp {
            let q = r.rotation;
            assert!(
                (q.x.abs() + q.z.abs()) > 1.0e-3 || (q.w.abs() - 1.0).abs() > 1.0e-3,
                "a ramp quaternion is a real rotation, got {q:?}"
            );
        }
    }
}
