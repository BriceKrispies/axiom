//! The seeded **scene grammar**. Reusable macros compose prefabs into room
//! shells, corridors, and combat rooms; the level assembles three connected
//! areas from the [`Style`] seed. Nothing is hand-placed per object: walls come
//! from a perimeter loop, and crates / pipes / enemies are scattered by the
//! deterministic `axiom-entropy` stream, so the same seed always yields the same
//! facility.

use axiom::prelude::Vec3;
use axiom_entropy::{EntropyApi, EntropyStream};
use axiom_space::SpaceApi;

use crate::style::Style;

/// Which wall side of a room carries a doorway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// -Z (into the level).
    North,
    /// +Z (back toward the start).
    South,
}

/// The two enemy variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyKind {
    /// Variant A — the boxy "grunt".
    Grunt,
    /// Variant B — the cylindrical "sentry".
    Sentry,
}

/// One placed prefab instance: which prefab, where, and its yaw about +Y.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    /// The prefab name (in [`crate::prefabs`]).
    pub prefab: &'static str,
    /// World position.
    pub position: Vec3,
    /// Yaw about +Y, radians.
    pub yaw: f32,
}

/// A placed enemy (rendered like a prefab, but tracked for gameplay).
#[derive(Debug, Clone, Copy)]
pub struct EnemySpawn {
    /// The variant.
    pub kind: EnemyKind,
    /// World position.
    pub position: Vec3,
}

/// The whole generated level: every static/prop placement, the enemy roster, and
/// the semantic positions the gameplay ruleset needs.
#[derive(Debug, Clone)]
pub struct LevelLayout {
    /// Every renderable static/prop instance (walls, floors, doors, gate, lights,
    /// crates, pipes, weapon, exit).
    pub placements: Vec<Placement>,
    /// The enemy roster (also rendered by the scene).
    pub enemies: Vec<EnemySpawn>,
    /// Where the player starts.
    pub player_spawn: Vec3,
    /// The weapon-pickup position.
    pub weapon_pos: Vec3,
    /// The locked-gate position (combat → final).
    pub gate_pos: Vec3,
    /// The exit / win-trigger position.
    pub exit_pos: Vec3,
}

/// A rectangular room footprint in panels.
struct Room {
    center: Vec3,
    cols: i32,
    rows: i32,
}

impl Room {
    fn half_w(&self, panel: f32) -> f32 {
        self.cols as f32 * panel * 0.5
    }
    fn half_d(&self, panel: f32) -> f32 {
        self.rows as f32 * panel * 0.5
    }
}

/// Floor tiles filling a room footprint.
fn floor_tiles(room: &Room, panel: f32, out: &mut Vec<Placement>) {
    for iz in 0..room.rows {
        for ix in 0..room.cols {
            let x = room.center.x - room.half_w(panel) + (ix as f32 + 0.5) * panel;
            let z = room.center.z - room.half_d(panel) + (iz as f32 + 0.5) * panel;
            out.push(Placement { prefab: "floor", position: Vec3::new(x, 0.0, z), yaw: 0.0 });
        }
    }
}

/// Perimeter walls, leaving the center panel open on each side in `open` (a
/// doorway). N/S walls run along X (yaw 0); E/W walls run along Z (yaw 90°).
fn perimeter_walls(room: &Room, panel: f32, height: f32, open: &[Side], out: &mut Vec<Placement>) {
    let y = height * 0.5;
    let north_z = room.center.z - room.half_d(panel);
    let south_z = room.center.z + room.half_d(panel);
    for ix in 0..room.cols {
        let x = room.center.x - room.half_w(panel) + (ix as f32 + 0.5) * panel;
        let mid = ix == room.cols / 2;
        if !(mid && open.contains(&Side::North)) {
            out.push(Placement { prefab: "wall", position: Vec3::new(x, y, north_z), yaw: 0.0 });
        }
        if !(mid && open.contains(&Side::South)) {
            out.push(Placement { prefab: "wall", position: Vec3::new(x, y, south_z), yaw: 0.0 });
        }
    }
    let west_x = room.center.x - room.half_w(panel);
    let east_x = room.center.x + room.half_w(panel);
    for iz in 0..room.rows {
        let z = room.center.z - room.half_d(panel) + (iz as f32 + 0.5) * panel;
        out.push(Placement { prefab: "wall", position: Vec3::new(west_x, y, z), yaw: std::f32::consts::FRAC_PI_2 });
        out.push(Placement { prefab: "wall", position: Vec3::new(east_x, y, z), yaw: std::f32::consts::FRAC_PI_2 });
    }
}

/// A couple of ceiling light bars over a room.
fn ceiling_lights(room: &Room, panel: f32, height: f32, out: &mut Vec<Placement>) {
    for iz in [1, room.rows - 1] {
        let z = room.center.z - room.half_d(panel) + iz as f32 * panel;
        out.push(Placement { prefab: "light", position: Vec3::new(room.center.x, height - 0.3, z), yaw: 0.0 });
    }
}

/// A room shell = floor + perimeter walls + ceiling lights.
fn room_shell(room: &Room, style: &Style, open: &[Side], out: &mut Vec<Placement>) {
    floor_tiles(room, style.panel_size, out);
    perimeter_walls(room, style.panel_size, style.room_height, open, out);
    ceiling_lights(room, style.panel_size, style.room_height, out);
}

/// A short straight corridor floor between two z levels at x=center, with side
/// walls; `barrier` (a "door" or "gate" prefab) sits at the near end.
fn corridor(x: f32, z_near: f32, z_far: f32, style: &Style, barrier: &'static str, out: &mut Vec<Placement>) -> Vec3 {
    let panel = style.panel_size;
    let steps = ((z_near - z_far).abs() / panel).ceil().max(1.0) as i32;
    for i in 0..steps {
        let z = z_near - (i as f32 + 0.5) * panel * (z_near - z_far).signum();
        out.push(Placement { prefab: "floor", position: Vec3::new(x, 0.0, z), yaw: 0.0 });
        out.push(Placement { prefab: "wall", position: Vec3::new(x - panel * 0.5, style.room_height * 0.5, z), yaw: std::f32::consts::FRAC_PI_2 });
        out.push(Placement { prefab: "wall", position: Vec3::new(x + panel * 0.5, style.room_height * 0.5, z), yaw: std::f32::consts::FRAC_PI_2 });
    }
    let barrier_pos = Vec3::new(x, 1.5, z_near);
    out.push(Placement { prefab: barrier, position: barrier_pos, yaw: 0.0 });
    barrier_pos
}

/// Scatter `count` copies of `prefab` inside a room's inner area, jittered by the
/// stream. Returns their positions (so enemies can be tracked).
fn scatter(room: &Room, panel: f32, count: u32, prefab: &'static str, y: f32, rng: &mut EntropyStream, out: &mut Vec<Placement>) -> Vec<Vec3> {
    let inner_w = room.half_w(panel) - panel * 0.6;
    let inner_d = room.half_d(panel) - panel * 0.6;
    (0..count)
        .map(|_| {
            let fx = rng.unit().get() * 2.0 - 1.0;
            let fz = rng.unit().get() * 2.0 - 1.0;
            let yaw = rng.unit().get() * std::f32::consts::TAU;
            let position = Vec3::new(room.center.x + fx * inner_w, y, room.center.z + fz * inner_d);
            out.push(Placement { prefab, position, yaw });
            position
        })
        .collect()
}

/// Wall dressing: wood base skirting + emissive ceiling trim along the N/S walls,
/// and a metal pillar at each corner — the repeated structural motif that gives a
/// room readable depth. (E/W walls are left plain to keep the count bounded.)
fn wall_dressing(room: &Room, style: &Style, out: &mut Vec<Placement>) {
    let panel = style.panel_size;
    let h = style.room_height;
    let north_z = room.center.z - room.half_d(panel);
    let south_z = room.center.z + room.half_d(panel);
    for ix in 0..room.cols {
        let x = room.center.x - room.half_w(panel) + (ix as f32 + 0.5) * panel;
        for z in [north_z, south_z] {
            out.push(Placement { prefab: "base_trim", position: Vec3::new(x, 0.35, z), yaw: 0.0 });
            out.push(Placement { prefab: "ceiling_trim", position: Vec3::new(x, h - 0.25, z), yaw: 0.0 });
        }
    }
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            let x = room.center.x + sx * room.half_w(panel);
            let z = room.center.z + sz * room.half_d(panel);
            out.push(Placement { prefab: "pillar", position: Vec3::new(x, h * 0.5, z), yaw: 0.0 });
        }
    }
}

/// A raised platform dais with a support bracket at each corner.
fn platform_dais(center: Vec3, out: &mut Vec<Placement>) {
    out.push(Placement { prefab: "platform", position: Vec3::new(center.x, 0.25, center.z), yaw: 0.0 });
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            out.push(Placement { prefab: "bracket", position: Vec3::new(center.x + sx * 1.35, 0.35, center.z + sz * 1.35), yaw: 0.0 });
        }
    }
}

/// A run of wall vents at eye height along the +X wall of a room.
fn vent_run(room: &Room, style: &Style, out: &mut Vec<Placement>) {
    let panel = style.panel_size;
    let x = room.center.x + room.half_w(panel) - 0.15;
    for iz in [-1.0_f32, 1.0] {
        let z = room.center.z + iz * panel;
        out.push(Placement { prefab: "vent", position: Vec3::new(x, 2.0, z), yaw: std::f32::consts::FRAC_PI_2 });
    }
}

/// Build the whole three-area facility from the style seed.
pub fn build_level(style: &Style) -> LevelLayout {
    let panel = style.panel_size;
    let mut placements = Vec::new();

    // Three rooms laid out along -Z: start → combat → final.
    let start = Room { center: Vec3::new(0.0, 0.0, 0.0), cols: 3, rows: 3 };
    let corridor_len = 2.0 * panel;
    let combat_z = -(start.half_d(panel) + corridor_len + panel * 2.0);
    let combat = Room { center: Vec3::new(0.0, 0.0, combat_z), cols: 4, rows: 4 };
    let final_z = combat_z - (combat.half_d(panel) + corridor_len + panel * 1.5);
    let final_room = Room { center: Vec3::new(0.0, 0.0, final_z), cols: 3, rows: 3 };

    room_shell(&start, style, &[Side::North], &mut placements);
    room_shell(&combat, style, &[Side::South, Side::North], &mut placements);
    room_shell(&final_room, style, &[Side::South], &mut placements);

    // Structural dressing: skirting + emissive ceiling trim + corner pillars per
    // room; a platform dais in the start room; a vent run in the combat room.
    wall_dressing(&start, style, &mut placements);
    wall_dressing(&combat, style, &mut placements);
    wall_dressing(&final_room, style, &mut placements);
    platform_dais(Vec3::new(start.center.x - 2.5, 0.0, start.center.z - 2.5), &mut placements);
    vent_run(&combat, style, &mut placements);

    // Corridor 1 (start → combat) has a normal DOOR at the start end.
    corridor(0.0, start.center.z - start.half_d(panel), combat.center.z + combat.half_d(panel), style, "door", &mut placements);
    // Corridor 2 (combat → final) is blocked by the locked GATE.
    let gate_pos = corridor(0.0, combat.center.z - combat.half_d(panel), final_room.center.z + final_room.half_d(panel), style, "gate", &mut placements);

    // Seeded props + enemies in the combat room (deterministic per seed).
    let mut crate_rng = EntropyApi::stream(style.level_seed, &SpaceApi::child(&SpaceApi::root(), 1), 1);
    scatter(&combat, panel, 5, "crate", 0.6, &mut crate_rng, &mut placements);
    let mut pipe_rng = EntropyApi::stream(style.level_seed, &SpaceApi::child(&SpaceApi::root(), 2), 1);
    scatter(&combat, panel, 3, "pipe", style.room_height * 0.5, &mut pipe_rng, &mut placements);

    let mut enemy_rng = EntropyApi::stream(style.level_seed, &SpaceApi::child(&SpaceApi::root(), 3), 1);
    let mut enemy_places = Vec::new();
    let enemies: Vec<EnemySpawn> = (0..4)
        .map(|_| {
            let fx = enemy_rng.unit().get() * 2.0 - 1.0;
            let fz = enemy_rng.unit().get() * 2.0 - 1.0;
            let kind = if enemy_rng.ratio_bool(axiom::prelude::Ratio::new(0.5).unwrap()) { EnemyKind::Grunt } else { EnemyKind::Sentry };
            let position = Vec3::new(
                combat.center.x + fx * (combat.half_w(panel) - panel * 0.7),
                0.9,
                combat.center.z + fz * (combat.half_d(panel) - panel * 0.7),
            );
            enemy_places.push((kind, position));
            EnemySpawn { kind, position }
        })
        .collect();
    // Enemies are also placements (rendered) — each a composite body + a head
    // (grunt) or a glowing eye (sentry) for a more readable, interesting silhouette.
    for (kind, position) in enemy_places {
        let (body, detail, detail_y) = match kind {
            EnemyKind::Grunt => ("enemy_grunt", "grunt_head", 1.75),
            EnemyKind::Sentry => ("enemy_sentry", "sentry_eye", 1.6),
        };
        placements.push(Placement { prefab: body, position, yaw: 0.0 });
        placements.push(Placement { prefab: detail, position: Vec3::new(position.x, detail_y, position.z), yaw: 0.0 });
    }

    // Weapon pickup in the start room — a composite gun (body + barrel + grip) so
    // it reads as a real weapon, not a block. `weapon_pos` stays the pickup point.
    let weapon_pos = Vec3::new(2.0, 1.0, -2.0);
    placements.push(Placement { prefab: "weapon_body", position: weapon_pos, yaw: 0.0 });
    placements.push(Placement { prefab: "weapon_barrel", position: Vec3::new(weapon_pos.x, weapon_pos.y + 0.02, weapon_pos.z - 0.35), yaw: 0.0 });
    placements.push(Placement { prefab: "weapon_grip", position: Vec3::new(weapon_pos.x, weapon_pos.y - 0.22, weapon_pos.z + 0.12), yaw: 0.0 });
    let exit_pos = Vec3::new(final_room.center.x, 1.75, final_room.center.z);
    placements.push(Placement { prefab: "exit", position: exit_pos, yaw: 0.0 });

    LevelLayout {
        placements,
        enemies,
        player_spawn: Vec3::new(0.0, 1.6, start.center.z + start.half_d(panel) - 2.0),
        weapon_pos,
        gate_pos,
        exit_pos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_is_deterministic_for_seed() {
        let style = Style::facility();
        let a = build_level(&style);
        let b = build_level(&style);
        assert_eq!(a.placements.len(), b.placements.len());
        assert_eq!(a.enemies.len(), b.enemies.len());
        // Same seed → identical enemy positions.
        for (ea, eb) in a.enemies.iter().zip(&b.enemies) {
            assert_eq!(ea.position.x, eb.position.x);
            assert_eq!(ea.position.z, eb.position.z);
        }
    }

    #[test]
    fn level_has_three_areas_worth_of_geometry_and_four_enemies() {
        let layout = build_level(&Style::facility());
        assert_eq!(layout.enemies.len(), 4);
        // Plenty of walls/floors from three rooms + two corridors.
        assert!(layout.placements.len() > 60, "got {}", layout.placements.len());
        // The gate sits between the combat and final rooms (more negative Z).
        assert!(layout.gate_pos.z < 0.0);
        assert!(layout.exit_pos.z < layout.gate_pos.z);
    }
}
