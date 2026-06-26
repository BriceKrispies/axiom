//! The DOOM level as a runtime **document** instead of compiled constants.
//!
//! [`LevelDoc`] is the parsed level: the wall grid plus every gameplay/visual
//! tunable that used to be a `const`. The canonical default document is the
//! committed `level.axiom` file, embedded here at compile time via
//! [`DEFAULT_LEVEL`]; the same file is served and watched by the
//! `axiom-dev-reload` dev server, so editing it hot-reloads the running browser
//! demo over SSE (see `web.rs`).
//!
//! Parsing lives entirely in the app — this is the app's translation of editable
//! text into the engine's authoring vocabulary, which is exactly where
//! cross-boundary glue belongs.

/// The built-in level: the committed `level.axiom`, embedded at compile time. It
/// is both the wasm build's default level and the file the dev server watches.
pub const DEFAULT_LEVEL: &str = include_str!("../level.axiom");

/// The parsed level: wall grid (row-major), the player start, and enemy spawns
/// in row-major order (so enemy `i` here is `Player` index `i` in the scene).
#[derive(Debug, Clone)]
pub struct MapData {
    pub width: usize,
    pub height: usize,
    pub walls: Vec<Vec<bool>>,
    pub start: (f32, f32),
    pub enemy_spawns: Vec<(f32, f32)>,
}

/// A linear-RGB colour triple as authored in the document.
type Rgb = [f32; 3];

/// Every gameplay/visual tunable the game reads. Each field defaults to the
/// value that used to be a `const` in `lib.rs`; the document overrides any it
/// names.
#[derive(Debug, Clone, Copy)]
pub struct Tunables {
    pub wall_height: f32,
    pub eye: f32,
    pub move_speed: f32,
    pub turn_speed: f32,
    pub enemy_speed: f32,
    pub pitch_limit: f32,
    pub enemy_y: f32,
    pub enemy_scale: f32,
    pub fire_range: f32,
    pub fire_half_angle: f32,
    pub fire_cooldown: u32,
    pub contact_radius: f32,
    pub contact_damage: i32,
    pub hurt_cooldown: u32,
    pub max_health: i32,
    pub start_ammo: u32,
    pub ammo_per_kill: u32,
    pub kill_score: u32,
}

impl Default for Tunables {
    fn default() -> Self {
        Tunables {
            wall_height: 2.0,
            eye: 1.0,
            move_speed: 0.06,
            turn_speed: 0.045,
            enemy_speed: 0.025,
            pitch_limit: 1.5,
            enemy_y: 0.5,
            enemy_scale: 0.7,
            fire_range: 14.0,
            fire_half_angle: 0.18,
            fire_cooldown: 10,
            contact_radius: 0.7,
            contact_damage: 4,
            hurt_cooldown: 12,
            max_health: 100,
            start_ammo: 50,
            ammo_per_kill: 5,
            kill_score: 100,
        }
    }
}

/// The level's authored colours (linear RGB).
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub wall_a: Rgb,
    pub wall_b: Rgb,
    pub floor: Rgb,
    pub ceiling: Rgb,
    pub enemy: Rgb,
    pub clear: Rgb,
}

impl Default for Colors {
    fn default() -> Self {
        Colors {
            wall_a: [0.40, 0.16, 0.16],
            wall_b: [0.20, 0.22, 0.30],
            floor: [0.10, 0.10, 0.12],
            ceiling: [0.05, 0.06, 0.09],
            enemy: [0.85, 0.20, 0.18],
            clear: [0.02, 0.02, 0.03],
        }
    }
}

/// A complete level: the grid plus tunables and colours.
#[derive(Debug, Clone)]
pub struct LevelDoc {
    pub map: MapData,
    pub tun: Tunables,
    pub colors: Colors,
}

impl Default for LevelDoc {
    /// The built-in level — `DEFAULT_LEVEL` parsed.
    fn default() -> Self {
        LevelDoc::parse(DEFAULT_LEVEL)
    }
}

impl LevelDoc {
    /// Parse a level document. Tunable and colour lines (`key = value`) precede a
    /// `[map]` marker; every line after the marker is a verbatim grid row.
    /// Anything unrecognised or unparseable falls back to the default, so a
    /// partial or slightly-malformed document still yields a playable level.
    pub fn parse(text: &str) -> LevelDoc {
        let mut tun = Tunables::default();
        let mut colors = Colors::default();
        let mut map_rows: Vec<String> = Vec::new();
        let mut in_map = false;

        for raw in text.lines() {
            let line = raw.trim_end_matches('\r');
            if in_map {
                // Inside the map, every non-empty line is a grid row verbatim
                // (a `#` here is a wall, never a comment).
                if !line.is_empty() {
                    map_rows.push(line.to_string());
                }
                continue;
            }
            let trimmed = line.trim();
            if trimmed == "[map]" {
                in_map = true;
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                apply_setting(&mut tun, &mut colors, key.trim(), value.trim());
            }
        }

        // Fall back to the built-in grid only when a document carried no map at
        // all (e.g. a tunables-only edit); never recurse for the real default.
        let map = if map_rows.is_empty() {
            default_map_rows()
        } else {
            map_rows
        };
        LevelDoc {
            map: parse_map(&map),
            tun,
            colors,
        }
    }

    /// An upper bound on the renderables this grid can ever produce, used to
    /// pre-size the live backend's per-instance buffer so an in-place reload can
    /// add walls/enemies without exceeding it: each cell contributes at most one
    /// renderable (a wall, or an enemy on a floor cell), plus the floor and
    /// ceiling slabs.
    pub fn grid_capacity(&self) -> usize {
        self.map.width * self.map.height + 2
    }
}

/// Apply one `key = value` setting to the tunables/colours, ignoring unknown
/// keys and unparseable values (which keep their defaults).
fn apply_setting(tun: &mut Tunables, colors: &mut Colors, key: &str, value: &str) {
    match key {
        "wall_height" => set_f32(&mut tun.wall_height, value),
        "eye" => set_f32(&mut tun.eye, value),
        "move_speed" => set_f32(&mut tun.move_speed, value),
        "turn_speed" => set_f32(&mut tun.turn_speed, value),
        "enemy_speed" => set_f32(&mut tun.enemy_speed, value),
        "pitch_limit" => set_f32(&mut tun.pitch_limit, value),
        "enemy_y" => set_f32(&mut tun.enemy_y, value),
        "enemy_scale" => set_f32(&mut tun.enemy_scale, value),
        "fire_range" => set_f32(&mut tun.fire_range, value),
        "fire_half_angle" => set_f32(&mut tun.fire_half_angle, value),
        "contact_radius" => set_f32(&mut tun.contact_radius, value),
        "fire_cooldown" => set_u32(&mut tun.fire_cooldown, value),
        "hurt_cooldown" => set_u32(&mut tun.hurt_cooldown, value),
        "start_ammo" => set_u32(&mut tun.start_ammo, value),
        "ammo_per_kill" => set_u32(&mut tun.ammo_per_kill, value),
        "kill_score" => set_u32(&mut tun.kill_score, value),
        "contact_damage" => set_i32(&mut tun.contact_damage, value),
        "max_health" => set_i32(&mut tun.max_health, value),
        "color_wall_a" => set_rgb(&mut colors.wall_a, value),
        "color_wall_b" => set_rgb(&mut colors.wall_b, value),
        "color_floor" => set_rgb(&mut colors.floor, value),
        "color_ceiling" => set_rgb(&mut colors.ceiling, value),
        "color_enemy" => set_rgb(&mut colors.enemy, value),
        "color_clear" => set_rgb(&mut colors.clear, value),
        _ => {}
    }
}

fn set_f32(slot: &mut f32, value: &str) {
    if let Ok(v) = value.parse::<f32>() {
        *slot = v;
    }
}

fn set_u32(slot: &mut u32, value: &str) {
    if let Ok(v) = value.parse::<u32>() {
        *slot = v;
    }
}

fn set_i32(slot: &mut i32, value: &str) {
    if let Ok(v) = value.parse::<i32>() {
        *slot = v;
    }
}

/// Parse a whitespace-separated RGB triple; keep the default unless all three
/// channels parse.
fn set_rgb(slot: &mut Rgb, value: &str) {
    let parts: Vec<f32> = value
        .split_whitespace()
        .filter_map(|c| c.parse().ok())
        .collect();
    if let [r, g, b] = parts[..] {
        *slot = [r, g, b];
    }
}

/// The built-in grid rows (used only when a document omits the `[map]` section).
fn default_map_rows() -> Vec<String> {
    let mut rows = Vec::new();
    let mut in_map = false;
    for raw in DEFAULT_LEVEL.lines() {
        let line = raw.trim_end_matches('\r');
        if in_map {
            if !line.is_empty() {
                rows.push(line.to_string());
            }
        } else if line.trim() == "[map]" {
            in_map = true;
        }
    }
    rows
}

/// Build the wall grid from the level's map rows. `#` is a wall, `S` the player
/// start, `E` an enemy spawn (collected in row-major order). The grid may be
/// ragged; out-of-bounds lookups are treated as walls by the caller.
fn parse_map(rows: &[String]) -> MapData {
    let height = rows.len();
    let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    let mut walls = Vec::with_capacity(height);
    let mut start = (1.0, 1.0);
    let mut enemy_spawns = Vec::new();
    for (row, line) in rows.iter().enumerate() {
        let mut wall_row = Vec::with_capacity(line.chars().count());
        for (col, c) in line.chars().enumerate() {
            wall_row.push(c == '#');
            if c == 'S' {
                start = (col as f32, row as f32);
            }
            if c == 'E' {
                enemy_spawns.push((col as f32, row as f32));
            }
        }
        walls.push(wall_row);
    }
    MapData {
        width,
        height,
        walls,
        start,
        enemy_spawns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_level_parses_into_the_expected_grid() {
        let doc = LevelDoc::default();
        assert_eq!(doc.map.height, 10);
        assert_eq!(doc.map.width, 18);
        assert_eq!(doc.map.enemy_spawns.len(), 4);
        // The start is an open floor cell.
        let (sx, sz) = doc.map.start;
        assert!(!doc.map.walls[sz as usize][sx as usize]);
        // Capacity bounds the whole grid plus the two slabs.
        assert_eq!(doc.grid_capacity(), 18 * 10 + 2);
    }

    #[test]
    fn defaults_match_the_former_constants() {
        let t = Tunables::default();
        assert_eq!(t.wall_height, 2.0);
        assert_eq!(t.max_health, 100);
        assert_eq!(t.start_ammo, 50);
        assert_eq!(Colors::default().enemy, [0.85, 0.20, 0.18]);
    }

    #[test]
    fn tunables_and_colours_override_from_the_document() {
        let doc = LevelDoc::parse(
            "wall_height = 5.5\nmax_health = 250\ncolor_enemy = 0.1 0.2 0.3\n[map]\n###\n#S#\n###\n",
        );
        assert_eq!(doc.tun.wall_height, 5.5);
        assert_eq!(doc.tun.max_health, 250);
        assert_eq!(doc.colors.enemy, [0.1, 0.2, 0.3]);
        // Untouched keys keep their defaults.
        assert_eq!(doc.tun.move_speed, Tunables::default().move_speed);
        assert_eq!(doc.map.width, 3);
        assert_eq!(doc.map.height, 3);
    }

    #[test]
    fn unknown_keys_and_bad_values_fall_back_to_defaults() {
        let doc = LevelDoc::parse(
            "nonsense = 1\nwall_height = not_a_number\ncolor_floor = 0.1 0.2\n[map]\n#S#\n",
        );
        assert_eq!(doc.tun.wall_height, Tunables::default().wall_height);
        // A two-channel colour is rejected (needs three).
        assert_eq!(doc.colors.floor, Colors::default().floor);
    }

    #[test]
    fn a_tunables_only_document_keeps_the_builtin_grid() {
        // No [map] section → fall back to the built-in grid rather than empty.
        let doc = LevelDoc::parse("wall_height = 3.0\n");
        assert_eq!(doc.tun.wall_height, 3.0);
        assert_eq!(doc.map.height, 10);
        assert_eq!(doc.map.width, 18);
    }

    #[test]
    fn comments_before_the_map_are_ignored_but_walls_in_the_map_are_not() {
        let doc =
            LevelDoc::parse("# this is a comment\nwall_height = 1.0\n[map]\n####\n#SE#\n####\n");
        assert_eq!(doc.tun.wall_height, 1.0);
        // The leading '#' rows are walls, not comments.
        assert!(doc.map.walls[0].iter().all(|&w| w));
        assert_eq!(doc.map.enemy_spawns.len(), 1);
    }
}
