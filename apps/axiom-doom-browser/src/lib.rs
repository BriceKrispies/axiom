//! # Axiom DOOM (browser) — a first-person shooter on just the engine
//!
//! A blocky DOOM-style level built entirely from the engine's cube primitive:
//! walls, floor and ceiling are scaled, coloured cube instances; the player is
//! the engine's first-person [`Controller`] camera; enemies are cube
//! [`Player`] nodes the app chases toward the player each tick. The whole game —
//! grid collision, enemy AI, hitscan shooting, contact damage, health/score — is
//! deterministic app logic in this `lib.rs` (native-tested); the `wasm32`-only
//! [`web`] arm captures the keyboard, drives the live windowing loop, and paints
//! the HUD into the DOM.
//!
//! ## Why this needs no per-frame engine "set transform"
//!
//! The app is the gameplay authority and stays byte-for-byte in sync with the
//! engine by only ever issuing the *same* per-tick deltas the engine applies:
//! the camera is moved by [`FirstPersonInput`] (yaw, then move-relative-to-
//! facing) and each enemy by a [`PlayerInput`] world delta. The app mirrors that
//! exact math locally (see [`DoomGame::step`]), so its tracked poses equal the
//! engine's nodes without ever reading them back.

use axiom::prelude::*;

/// The presentation canvas element id (must match the gallery/host page).
pub const CANVAS_ID: &str = "axiom-doom-canvas";

// --- Tunables (all per-tick; the sim runs at the engine's fixed step) ---

const WALL_HEIGHT: f32 = 2.0;
const EYE: f32 = 1.0;
const MOVE_SPEED: f32 = 0.06;
const TURN_SPEED: f32 = 0.045;
const ENEMY_SPEED: f32 = 0.025;
/// Pitch clamp, mirroring the engine's controller limit so the app's tracked
/// pitch matches the camera (lets respawn snap the view level).
const PITCH_LIMIT: f32 = 1.5;
const ENEMY_Y: f32 = 0.5;
const ENEMY_SCALE: f32 = 0.7;
/// Where a dead enemy is parked: far below the floor, out of view.
const PARK_Y: f32 = -1000.0;
const FIRE_RANGE: f32 = 14.0;
/// Half-angle of the aiming cone, in radians.
const FIRE_HALF_ANGLE: f32 = 0.18;
const FIRE_COOLDOWN: u32 = 10;
const CONTACT_RADIUS: f32 = 0.7;
/// Health lost per "bite", and the cooldown (ticks) between bites while a player
/// stays in contact — so melee drains at a fair, survivable rate.
const CONTACT_DAMAGE: i32 = 4;
const HURT_COOLDOWN: u32 = 12;
const MAX_HEALTH: i32 = 100;
const START_AMMO: u32 = 50;
const AMMO_PER_KILL: u32 = 5;
const KILL_SCORE: u32 = 100;

/// The level. `#` wall, `.` floor, `S` player start, `E` enemy spawn. Two rooms
/// (split by the `#` column) joined by the open doorway row.
const MAP: &[&str] = &[
    "##################",
    "#.......#.......E#",
    "#.......#........#",
    "#...E...#....E...#",
    "#.......#........#",
    "#................#",
    "#.......#........#",
    "#...E...#........#",
    "#S......#........#",
    "##################",
];

/// A linear colour channel from an authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// Round a world coordinate to its grid cell index (cell centres are integers).
fn cell_of(coord: f32) -> i32 {
    (coord + 0.5).floor() as i32
}

/// The parsed level: wall grid (row-major), the player start, and enemy spawns
/// in row-major order (so enemy `i` here is [`Player`] index `i` in the scene).
#[derive(Debug, Clone)]
struct MapData {
    width: usize,
    height: usize,
    walls: Vec<Vec<bool>>,
    start: (f32, f32),
    enemy_spawns: Vec<(f32, f32)>,
}

fn parse_map() -> MapData {
    let height = MAP.len();
    let width = MAP[0].len();
    let mut walls = vec![vec![false; width]; height];
    let mut start = (1.0, 1.0);
    let mut enemy_spawns = Vec::new();
    for (row, line) in MAP.iter().enumerate() {
        for (col, c) in line.chars().enumerate() {
            match c {
                '#' => walls[row][col] = true,
                'S' => start = (col as f32, row as f32),
                'E' => enemy_spawns.push((col as f32, row as f32)),
                _ => {}
            }
        }
    }
    MapData {
        width,
        height,
        walls,
        start,
        enemy_spawns,
    }
}

/// An enemy's commanded engine pose plus its spawn and liveness. The `x,y,z`
/// fields are exactly what the engine node holds (the app only ever moves it by
/// the delta it stores back here), so no read-back is needed.
#[derive(Debug, Clone, Copy)]
struct Enemy {
    x: f32,
    y: f32,
    z: f32,
    spawn: (f32, f32),
    alive: bool,
}

/// One tick of held controls, decoded from the keyboard / on-screen pad.
#[derive(Debug, Default, Clone, Copy)]
pub struct Intent {
    /// Move forward (up).
    pub forward: bool,
    /// Move backward (down).
    pub backward: bool,
    /// Turn left.
    pub turn_left: bool,
    /// Turn right.
    pub turn_right: bool,
    /// Strafe left (optional; off on the tank pad).
    pub strafe_left: bool,
    /// Strafe right.
    pub strafe_right: bool,
    /// Fire this tick.
    pub fire: bool,
    /// Mouse-look yaw delta this tick (radians; positive turns left). Added to
    /// any keyboard/keypad turn.
    pub look_yaw: f32,
    /// Mouse-look pitch delta this tick (radians; positive looks up).
    pub look_pitch: f32,
}

/// The player-facing state the HUD shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hud {
    /// Remaining health (0..=100).
    pub health: i32,
    /// Kill score.
    pub score: u32,
    /// Remaining ammo.
    pub ammo: u32,
    /// Living enemies left.
    pub enemies_alive: u32,
}

/// The per-tick commands the engine applies: the first-person camera control and
/// one world-delta move per enemy, plus the HUD snapshot for this tick.
#[derive(Debug, Clone)]
pub struct StepCommands {
    /// The camera control (controller index 0).
    pub control: FirstPersonInput,
    /// One move per enemy, in spawn order (player index `i`).
    pub enemies: Vec<PlayerInput>,
    /// The HUD after this tick.
    pub hud: Hud,
}

/// The deterministic DOOM game state and per-tick simulation. Holds the level,
/// the player pose (position + yaw), health/score/ammo, and the enemies. It is
/// engine-agnostic apart from the input value types it emits.
#[derive(Debug, Clone)]
pub struct DoomGame {
    map: MapData,
    px: f32,
    pz: f32,
    yaw: f32,
    pitch: f32,
    health: i32,
    score: u32,
    ammo: u32,
    fire_cd: u32,
    hurt_cd: u32,
    enemies: Vec<Enemy>,
}

impl DoomGame {
    /// Start a fresh game from the built-in level.
    pub fn new() -> Self {
        let map = parse_map();
        let enemies = map
            .enemy_spawns
            .iter()
            .map(|&(x, z)| Enemy {
                x,
                y: ENEMY_Y,
                z,
                spawn: (x, z),
                alive: true,
            })
            .collect();
        DoomGame {
            px: map.start.0,
            pz: map.start.1,
            yaw: 0.0,
            pitch: 0.0,
            health: MAX_HEALTH,
            score: 0,
            ammo: START_AMMO,
            fire_cd: 0,
            hurt_cd: 0,
            enemies,
            map,
        }
    }

    /// The HUD snapshot for the current state.
    pub fn hud(&self) -> Hud {
        Hud {
            health: self.health,
            score: self.score,
            ammo: self.ammo,
            enemies_alive: self.enemies.iter().filter(|e| e.alive).count() as u32,
        }
    }

    /// Is the world cell containing `(x, z)` a wall (or out of bounds)?
    fn is_wall(&self, x: f32, z: f32) -> bool {
        let col = cell_of(x);
        let row = cell_of(z);
        if col < 0 || row < 0 || col as usize >= self.map.width || row as usize >= self.map.height {
            return true;
        }
        self.map.walls[row as usize][col as usize]
    }

    /// Is the straight segment from the player to `(tx, tz)` free of walls? A
    /// coarse march in ~quarter-cell steps — enough to stop shots through walls.
    fn line_clear(&self, tx: f32, tz: f32) -> bool {
        let (dx, dz) = (tx - self.px, tz - self.pz);
        let dist = (dx * dx + dz * dz).sqrt();
        let steps = (dist / 0.25).ceil() as i32;
        for i in 1..steps {
            let t = i as f32 / steps as f32;
            if self.is_wall(self.px + dx * t, self.pz + dz * t) {
                return false;
            }
        }
        true
    }

    /// Advance one deterministic tick under `intent`, returning the engine
    /// commands (camera control + enemy moves) and the HUD. When the player has
    /// died, this tick snaps the camera back to the start and respawns enemies.
    pub fn step(&mut self, intent: Intent) -> StepCommands {
        if self.health <= 0 {
            return self.respawn();
        }
        self.fire_cd = self.fire_cd.saturating_sub(1);
        self.hurt_cd = self.hurt_cd.saturating_sub(1);

        // 1. Look: keypad/key turn plus mouse yaw; mouse pitch (clamped). Then
        //    move relative to the new yaw.
        let yaw_delta =
            (intent.turn_left as i32 - intent.turn_right as i32) as f32 * TURN_SPEED + intent.look_yaw;
        let pitch_delta = intent.look_pitch;
        self.yaw += yaw_delta;
        self.pitch = (self.pitch + pitch_delta).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        let forward = (intent.forward as i32 - intent.backward as i32) as f32 * MOVE_SPEED;
        let strafe = (intent.strafe_right as i32 - intent.strafe_left as i32) as f32 * MOVE_SPEED;
        let move_local = self.move_player(forward, strafe);
        let control = FirstPersonInput::new(
            0,
            move_local,
            Angle::radians(yaw_delta),
            Angle::radians(pitch_delta),
        );

        // 2. Shoot before enemies move, so a killed enemy parks this same tick.
        self.fire(intent.fire);

        // 3. Enemies chase (or park if dead); collect their world-delta moves.
        let enemies = self.update_enemies(false);

        // 4. Contact damage: a periodic "bite" while a living enemy is in melee
        //    range, gated by a cooldown so standing in a crowd is survivable.
        if self.hurt_cd == 0 && self.any_enemy_in_contact() {
            self.health -= CONTACT_DAMAGE;
            self.hurt_cd = HURT_COOLDOWN;
        }

        StepCommands {
            control,
            enemies,
            hud: self.hud(),
        }
    }

    /// Apply a `(forward, strafe)` move at the current yaw with axis-separated
    /// wall sliding, returning the `move_local` (in the camera's own frame) that
    /// reproduces the *applied* (unblocked) motion in the engine.
    fn move_player(&mut self, forward: f32, strafe: f32) -> Vec3 {
        let (sin, cos) = (self.yaw.sin(), self.yaw.cos());
        let dx = strafe * cos - forward * sin;
        let dz = -strafe * sin - forward * cos;
        let nx = if self.is_wall(self.px + dx, self.pz) {
            self.px
        } else {
            self.px + dx
        };
        let nz = if self.is_wall(self.px, self.pz + dz) {
            self.pz
        } else {
            self.pz + dz
        };
        let (adx, adz) = (nx - self.px, nz - self.pz);
        self.px = nx;
        self.pz = nz;
        // Express the applied world delta in the camera's local frame, so the
        // engine (which rotates move_local by the yaw-only facing) reproduces
        // exactly (adx, adz).
        Vec3::new(adx * cos - adz * sin, 0.0, adx * sin + adz * cos)
    }

    /// Resolve a fire input: spend ammo on cooldown and kill the nearest living
    /// enemy inside the aiming cone, in range, and in line of sight.
    fn fire(&mut self, firing: bool) {
        if !firing || self.fire_cd > 0 || self.ammo == 0 {
            return;
        }
        self.fire_cd = FIRE_COOLDOWN;
        self.ammo -= 1;
        let (fx, fz) = (-self.yaw.sin(), -self.yaw.cos());
        let cone = FIRE_HALF_ANGLE.cos();
        let mut best: Option<(usize, f32)> = None;
        for (i, e) in self.enemies.iter().enumerate() {
            if !e.alive {
                continue;
            }
            let (dx, dz) = (e.x - self.px, e.z - self.pz);
            let dist = (dx * dx + dz * dz).sqrt();
            if !(1.0e-4..=FIRE_RANGE).contains(&dist) {
                continue;
            }
            let aim = (fx * dx + fz * dz) / dist;
            if aim < cone || !self.line_clear(e.x, e.z) {
                continue;
            }
            if best.is_none_or(|(_, d)| dist < d) {
                best = Some((i, dist));
            }
        }
        if let Some((i, _)) = best {
            self.enemies[i].alive = false;
            self.score += KILL_SCORE;
            self.ammo += AMMO_PER_KILL;
        }
    }

    /// Move every enemy one tick and return the world-delta [`PlayerInput`]s. A
    /// living enemy chases the player (axis-separated wall stops); a dead enemy
    /// parks below the floor; on `respawning` every enemy returns to its spawn
    /// alive. Each enemy's stored pose is updated to the new target, so the delta
    /// always equals `target - current` — the engine node tracks it exactly.
    fn update_enemies(&mut self, respawning: bool) -> Vec<PlayerInput> {
        let (px, pz) = (self.px, self.pz);
        let mut inputs = Vec::with_capacity(self.enemies.len());
        for (i, e) in self.enemies.iter_mut().enumerate() {
            let (tx, ty, tz) = if respawning {
                e.alive = true;
                (e.spawn.0, ENEMY_Y, e.spawn.1)
            } else if e.alive {
                Self::chase(&self.map, e, px, pz)
            } else {
                (e.spawn.0, PARK_Y, e.spawn.1)
            };
            inputs.push(PlayerInput::new(
                i as u32,
                Vec3::new(tx - e.x, ty - e.y, tz - e.z),
            ));
            e.x = tx;
            e.y = ty;
            e.z = tz;
        }
        inputs
    }

    /// One chase step for a living enemy toward `(px, pz)`, with per-axis wall
    /// stops. Returns its new `(x, y, z)`.
    fn chase(map: &MapData, e: &Enemy, px: f32, pz: f32) -> (f32, f32, f32) {
        let (dx, dz) = (px - e.x, pz - e.z);
        let len = (dx * dx + dz * dz).sqrt();
        if len < 1.0e-4 {
            return (e.x, ENEMY_Y, e.z);
        }
        let (nx, nz) = (dx / len * ENEMY_SPEED, dz / len * ENEMY_SPEED);
        let tx = if Self::map_wall(map, e.x + nx, e.z) {
            e.x
        } else {
            e.x + nx
        };
        let tz = if Self::map_wall(map, e.x, e.z + nz) {
            e.z
        } else {
            e.z + nz
        };
        (tx, ENEMY_Y, tz)
    }

    /// Wall lookup against a map (the static helper used by enemy chasing).
    fn map_wall(map: &MapData, x: f32, z: f32) -> bool {
        let col = cell_of(x);
        let row = cell_of(z);
        if col < 0 || row < 0 || col as usize >= map.width || row as usize >= map.height {
            return true;
        }
        map.walls[row as usize][col as usize]
    }

    /// Is any living enemy within melee contact of the player?
    fn any_enemy_in_contact(&self) -> bool {
        self.enemies.iter().any(|e| {
            e.alive && {
                let (dx, dz) = (e.x - self.px, e.z - self.pz);
                (dx * dx + dz * dz).sqrt() < CONTACT_RADIUS
            }
        })
    }

    /// Death → reset: snap the camera back to the start (one corrective control),
    /// respawn every enemy at its spawn, and restore health/score/ammo.
    fn respawn(&mut self) -> StepCommands {
        // Correct yaw and pitch back to zero (look level, facing -Z). After the
        // yaw correction the frame is world-aligned, so the move back to start is
        // the world delta directly.
        let (yaw_delta, pitch_delta) = (-self.yaw, -self.pitch);
        let (dx, dz) = (self.map.start.0 - self.px, self.map.start.1 - self.pz);
        let control = FirstPersonInput::new(
            0,
            Vec3::new(dx, 0.0, dz),
            Angle::radians(yaw_delta),
            Angle::radians(pitch_delta),
        );
        self.px = self.map.start.0;
        self.pz = self.map.start.1;
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.health = MAX_HEALTH;
        self.score = 0;
        self.ammo = START_AMMO;
        self.fire_cd = 0;
        self.hurt_cd = 0;
        let enemies = self.update_enemies(true);
        StepCommands {
            control,
            enemies,
            hud: self.hud(),
        }
    }
}

impl Default for DoomGame {
    fn default() -> Self {
        DoomGame::new()
    }
}

/// Build the engine app for the DOOM level: the cube-walled rooms, floor and
/// ceiling, a first-person [`Controller`] camera at the start, a directional
/// light, and one enemy cube ([`Player`]) per spawn (in the same order
/// [`DoomGame`] enumerates them, so indices line up).
pub fn build_doom_app() -> RunningApp {
    let map = parse_map();
    App::new()
        .window(
            Window::new(960, 600)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(ch(0.02), ch(0.02), ch(0.03))),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let wall_a = materials.add(Material::lit(Color::linear_rgb(ch(0.40), ch(0.16), ch(0.16))));
            let wall_b = materials.add(Material::lit(Color::linear_rgb(ch(0.20), ch(0.22), ch(0.30))));
            let floor = materials.add(Material::lit(Color::linear_rgb(ch(0.10), ch(0.10), ch(0.12))));
            let ceiling = materials.add(Material::lit(Color::linear_rgb(ch(0.05), ch(0.06), ch(0.09))));
            let enemy = materials.add(Material::lit(Color::linear_rgb(ch(0.85), ch(0.20), ch(0.18))));

            // Walls: one scaled cube per wall cell, two-tone by parity.
            for (row, line) in map.walls.iter().enumerate() {
                for (col, &is_wall) in line.iter().enumerate() {
                    if !is_wall {
                        continue;
                    }
                    let mat = if (row + col) % 2 == 0 { wall_a } else { wall_b };
                    world.spawn((
                        block(col as f32, WALL_HEIGHT * 0.5, row as f32, 1.0, WALL_HEIGHT, 1.0),
                        Renderable {
                            mesh: cube,
                            material: mat,
                        },
                    ));
                }
            }

            // Floor + ceiling: two big flat slabs spanning the whole grid.
            let (cx, cz) = ((map.width as f32 - 1.0) * 0.5, (map.height as f32 - 1.0) * 0.5);
            world.spawn((
                block(cx, -0.05, cz, map.width as f32, 0.1, map.height as f32),
                Renderable {
                    mesh: cube,
                    material: floor,
                },
            ));
            world.spawn((
                block(
                    cx,
                    WALL_HEIGHT + 0.05,
                    cz,
                    map.width as f32,
                    0.1,
                    map.height as f32,
                ),
                Renderable {
                    mesh: cube,
                    material: ceiling,
                },
            ));

            // Enemies: a red cube Player per spawn, in row-major (index) order.
            for (i, &(x, z)) in map.enemy_spawns.iter().enumerate() {
                world.spawn((
                    block(x, ENEMY_Y, z, ENEMY_SCALE, ENEMY_SCALE, ENEMY_SCALE),
                    Renderable {
                        mesh: cube,
                        material: enemy,
                    },
                    Player::new(i as u32),
                ));
            }

            // The first-person camera at the start, facing -Z (yaw 0).
            world.spawn((
                Transform::from_translation(Vec3::new(map.start.0, EYE, map.start.1)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(70.0),
                    near: Meters::new(0.05).expect("near plane is finite"),
                    far: Meters::new(200.0).expect("far plane is finite"),
                }),
                Controller::new(0),
            ));

            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.2),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
        })
        .build()
}

/// A translated, axis-scaled cube transform (identity rotation).
fn block(x: f32, y: f32, z: f32, sx: f32, sy: f32, sz: f32) -> Transform {
    let mut t = Transform::from_translation(Vec3::new(x, y, z));
    t.scale = Vec3::new(sx, sy, sz);
    t
}

#[cfg(target_arch = "wasm32")]
mod web;

// The native agent bridge (drive the game from JSON, read back state/images).
// Native + `agent`-feature only, so the wasm build and default gates skip it.
#[cfg(all(feature = "agent", not(target_arch = "wasm32")))]
pub mod agent;

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> Intent {
        Intent::default()
    }

    #[test]
    fn map_is_rectangular_with_a_start_and_enemies() {
        let m = parse_map();
        assert!(MAP.iter().all(|r| r.len() == m.width));
        assert_eq!(m.height, MAP.len());
        assert_eq!(m.enemy_spawns.len(), 4);
        // The start cell is open floor.
        assert!(!m.is_wall_start());
    }

    impl MapData {
        fn is_wall_start(&self) -> bool {
            self.walls[self.start.1 as usize][self.start.0 as usize]
        }
    }

    #[test]
    fn walking_into_a_wall_does_not_pass_through_it() {
        // The start is the bottom-left corner; -Z (forward) leads up the room, but
        // strafing left (-X) immediately hits the west wall.
        let mut g = DoomGame::new();
        for _ in 0..30 {
            g.step(Intent {
                strafe_left: true,
                ..idle()
            });
        }
        // The player slides to the floor-cell boundary (~x=0.5) but the west wall
        // (column 0) stops it there — it never crosses into the wall cell.
        assert!(g.px >= 0.49, "the west wall blocks leftward strafing");
        assert!(!g.is_wall(g.px, g.pz), "the player never ends inside a wall");
    }

    #[test]
    fn moving_forward_advances_along_negative_z() {
        let mut g = DoomGame::new();
        let z0 = g.pz;
        let cmd = g.step(Intent {
            forward: true,
            ..idle()
        });
        assert!(g.pz < z0, "forward moves up the room (-Z)");
        // The control reproduces the move: local forward is -Z.
        assert!(cmd.control.move_local.z < 0.0);
    }

    #[test]
    fn mouse_look_feeds_yaw_and_pitch_into_the_control_and_clamps_pitch() {
        let mut g = DoomGame::new();
        let cmd = g.step(Intent {
            look_yaw: 0.3,
            look_pitch: -0.2,
            ..idle()
        });
        // Mouse yaw/pitch flow straight into the controller input...
        assert_eq!(cmd.control.yaw.as_radians(), 0.3);
        assert_eq!(cmd.control.pitch.as_radians(), -0.2);
        assert!((g.yaw - 0.3).abs() < 1.0e-6);
        // ...and the tracked pitch clamps to the engine's limit on a big look.
        g.step(Intent {
            look_pitch: 5.0,
            ..idle()
        });
        assert_eq!(g.pitch, PITCH_LIMIT);
    }

    #[test]
    fn turning_changes_yaw_and_is_reported_in_the_control() {
        let mut g = DoomGame::new();
        let cmd = g.step(Intent {
            turn_left: true,
            ..idle()
        });
        assert!(g.yaw > 0.0);
        assert_eq!(cmd.control.yaw.as_radians(), TURN_SPEED);
    }

    #[test]
    fn firing_down_the_lane_kills_the_aligned_enemy() {
        // Place the player just south of the left-room enemy at (4,3) and aim at
        // it (north = -Z, the default facing), then fire.
        let mut g = DoomGame::new();
        g.px = 4.0;
        g.pz = 6.0;
        g.yaw = 0.0; // facing -Z, straight at the (4,3) enemy
        let before = g.hud().enemies_alive;
        let cmd = g.step(Intent {
            fire: true,
            ..idle()
        });
        assert_eq!(cmd.hud.enemies_alive, before - 1, "the lined-up enemy dies");
        assert_eq!(cmd.hud.score, KILL_SCORE);
        assert_eq!(cmd.hud.ammo, START_AMMO - 1 + AMMO_PER_KILL);
    }

    #[test]
    fn a_dead_enemy_is_parked_below_the_floor() {
        let mut g = DoomGame::new();
        g.px = 4.0;
        g.pz = 6.0;
        g.step(Intent {
            fire: true,
            ..idle()
        });
        // The kill tick parked exactly one enemy under the floor.
        assert!(g.enemies.iter().any(|e| !e.alive && e.y < -100.0));
    }

    #[test]
    fn a_wall_blocks_a_shot() {
        // Aiming through the dividing wall at a right-room enemy must miss.
        let mut g = DoomGame::new();
        g.px = 4.0;
        g.pz = 5.0;
        g.yaw = -std::f32::consts::FRAC_PI_2; // face +X, toward the divider wall
        let before = g.hud().enemies_alive;
        g.step(Intent {
            fire: true,
            ..idle()
        });
        assert_eq!(g.hud().enemies_alive, before, "the wall stops the shot");
    }

    #[test]
    fn standing_in_an_enemy_drains_then_resets_health() {
        // Drop the player onto a living enemy: contact damage ticks down, and at
        // zero the next step respawns at full health back at the start.
        let mut g = DoomGame::new();
        let spawn = g.enemies[0].spawn;
        g.px = spawn.0;
        g.pz = spawn.1;
        let mut saw_damage = false;
        // Enough ticks to drain full health through the hurt cooldown and die.
        let max_ticks = (MAX_HEALTH / CONTACT_DAMAGE + 2) as usize * HURT_COOLDOWN as usize;
        for _ in 0..max_ticks {
            let h = g.health;
            g.step(idle());
            if g.health < h {
                saw_damage = true;
            }
        }
        assert!(saw_damage, "contact with an enemy costs health");
        // After enough contact the player died and respawned at the start.
        assert_eq!(g.health, MAX_HEALTH);
        assert_eq!((g.px, g.pz), g.map.start);
        assert_eq!(g.score, 0);
    }

    #[test]
    fn the_simulation_is_deterministic_for_a_fixed_script() {
        let script = [
            Intent {
                forward: true,
                ..idle()
            },
            Intent {
                turn_left: true,
                forward: true,
                ..idle()
            },
            Intent {
                fire: true,
                ..idle()
            },
            Intent {
                turn_right: true,
                ..idle()
            },
        ];
        let run = || {
            let mut g = DoomGame::new();
            let mut last = g.hud();
            for _ in 0..3 {
                for &i in &script {
                    last = g.step(i).hud;
                }
            }
            (last, g.px, g.pz, g.yaw)
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn the_scene_has_walls_floor_ceiling_and_one_renderable_per_enemy() {
        let app = build_doom_app();
        let map = parse_map();
        let wall_count: usize = map.walls.iter().flatten().filter(|&&w| w).count();
        // walls + floor + ceiling + one cube per enemy.
        let expected = wall_count + 2 + map.enemy_spawns.len();
        assert_eq!(app.renderable_count(), expected);
    }

    #[test]
    fn the_first_frame_draws_every_renderable_and_runs_deterministically() {
        let mut a = build_doom_app();
        let mut b = build_doom_app();
        let fa = a.tick(0);
        assert_eq!(fa.draws().len(), a.renderable_count());
        assert_eq!(fa, b.tick(0), "tick 0 replays byte-identically");
    }

    #[test]
    fn default_game_matches_new() {
        assert_eq!(DoomGame::default().hud(), DoomGame::new().hud());
    }
}
