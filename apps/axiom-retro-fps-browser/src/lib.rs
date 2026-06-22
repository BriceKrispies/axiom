//! # Axiom retro FPS (browser) — a first-person shooter on just the engine
//!
//! A blocky retro FPS-style level built entirely from the engine's cube primitive:
//! walls, floor and ceiling are scaled, coloured cube instances; the player is
//! the engine's first-person [`Controller`] camera; enemies are cube
//! [`Player`] nodes the app chases toward the player each tick. The whole game —
//! grid collision, enemy AI, hitscan shooting, contact damage, health/score — is
//! deterministic app logic in this `lib.rs` (native-tested); the `wasm32`-only
//! [`web`] arm captures the keyboard, drives the live windowing loop, and paints
//! the HUD into the DOM.
//!
//! ## The level is a live document, not a constant
//!
//! The level (grid + every gameplay/visual tunable) is a [`LevelDoc`] parsed from
//! the editable `level.axiom` document (see [`level`]). The browser arm subscribes
//! to that document over SSE (served by the `axiom-dev-reload` dev server) and, on
//! every save, re-authors the running engine scene in place via
//! [`reload_retro_fps`] — so editing a wall hot-reloads the demo with no recompile and
//! no page reload.
//!
//! ## Why this needs no per-frame engine "set transform"
//!
//! The app is the gameplay authority and stays byte-for-byte in sync with the
//! engine by only ever issuing the *same* per-tick deltas the engine applies:
//! the camera is moved by [`FirstPersonInput`] (yaw, then move-relative-to-
//! facing) and each enemy by a [`PlayerInput`] world delta. The app mirrors that
//! exact math locally (see [`RetroFpsGame::step`]), so its tracked poses equal the
//! engine's nodes without ever reading them back.

use axiom::prelude::*;

pub mod level;

use level::{LevelDoc, MapData};

/// The presentation canvas element id (must match the gallery/host page).
pub const CANVAS_ID: &str = "axiom-retro-fps-canvas";

/// A linear colour channel from an authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// A linear-RGB [`Color`] from an authored triple.
fn color_of(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(ch(rgb[0]), ch(rgb[1]), ch(rgb[2]))
}

/// Round a world coordinate to its grid cell index (cell centres are integers).
fn cell_of(coord: f32) -> i32 {
    (coord + 0.5).floor() as i32
}

/// Is the world cell containing `(x, z)` a wall, treating anything out of bounds
/// as a wall? The four-way bounds test is folded into the lookup itself: a
/// past-the-end index misses the `.get(..)`, and a *negative* index cast to
/// `usize` wraps to a huge value that also misses — so both out-of-bounds cases
/// resolve through the same `.get(..).unwrap_or(true)`, with no `||` and no
/// panicking index.
fn map_wall_at(map: &MapData, x: f32, z: f32) -> bool {
    let col = cell_of(x) as usize;
    let row = cell_of(z) as usize;
    map.walls
        .get(row)
        .and_then(|line| line.get(col))
        .copied()
        .unwrap_or(true)
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

/// A little-endian, bounds-checked cursor over [`RetroFpsGame::write_state`] bytes.
struct StateReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl StateReader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let end = self.at + N;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        slice.try_into().ok()
    }
    fn f32(&mut self) -> Option<f32> {
        self.take::<4>().map(f32::from_le_bytes)
    }
    fn i32(&mut self) -> Option<i32> {
        self.take::<4>().map(i32::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }
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

/// The deterministic retro FPS game state and per-tick simulation. Holds the level
/// document (grid + tunables), the player pose (position + yaw), health/score/
/// ammo, and the enemies. It is engine-agnostic apart from the input value types
/// it emits.
#[derive(Debug, Clone)]
pub struct RetroFpsGame {
    level: LevelDoc,
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

impl RetroFpsGame {
    /// Start a fresh game from the built-in level document.
    pub fn new() -> Self {
        Self::from_level(&LevelDoc::default())
    }

    /// Start a fresh game from a level document — the player at its start, full
    /// health/ammo, and one live enemy per spawn (in row-major order, so enemy
    /// `i` is [`Player`] index `i` in the scene).
    pub fn from_level(doc: &LevelDoc) -> Self {
        let enemies = doc
            .map
            .enemy_spawns
            .iter()
            .map(|&(x, z)| Enemy {
                x,
                y: doc.tun.enemy_y,
                z,
                spawn: (x, z),
                alive: true,
            })
            .collect();
        RetroFpsGame {
            px: doc.map.start.0,
            pz: doc.map.start.1,
            yaw: 0.0,
            pitch: 0.0,
            health: doc.tun.max_health,
            score: 0,
            ammo: doc.tun.start_ammo,
            fire_cd: 0,
            hurt_cd: 0,
            enemies,
            level: doc.clone(),
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

    /// Serialize the mutable game state (player pose, vitals, cooldowns, and every
    /// enemy) to little-endian bytes. The immutable `level` document is not
    /// included — a fork restores into the same level. Pairs with
    /// [`RetroFpsGame::read_state`] to fork-and-resume from a recorded frame.
    pub fn write_state(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.px.to_le_bytes());
        bytes.extend_from_slice(&self.pz.to_le_bytes());
        bytes.extend_from_slice(&self.yaw.to_le_bytes());
        bytes.extend_from_slice(&self.pitch.to_le_bytes());
        bytes.extend_from_slice(&self.health.to_le_bytes());
        bytes.extend_from_slice(&self.score.to_le_bytes());
        bytes.extend_from_slice(&self.ammo.to_le_bytes());
        bytes.extend_from_slice(&self.fire_cd.to_le_bytes());
        bytes.extend_from_slice(&self.hurt_cd.to_le_bytes());
        bytes.extend_from_slice(&(self.enemies.len() as u32).to_le_bytes());
        for e in &self.enemies {
            bytes.extend_from_slice(&e.x.to_le_bytes());
            bytes.extend_from_slice(&e.y.to_le_bytes());
            bytes.extend_from_slice(&e.z.to_le_bytes());
            bytes.extend_from_slice(&e.spawn.0.to_le_bytes());
            bytes.extend_from_slice(&e.spawn.1.to_le_bytes());
            bytes.push(u8::from(e.alive));
        }
        bytes
    }

    /// Restore mutable game state from bytes produced by [`RetroFpsGame::write_state`],
    /// keeping the current `level`. Returns `false` (leaving `self` unchanged) if
    /// the buffer is truncated/malformed, so a bad fork never corrupts the game.
    pub fn read_state(&mut self, bytes: &[u8]) -> bool {
        let mut r = StateReader { bytes, at: 0 };
        let parsed = (|| {
            let px = r.f32()?;
            let pz = r.f32()?;
            let yaw = r.f32()?;
            let pitch = r.f32()?;
            let health = r.i32()?;
            let score = r.u32()?;
            let ammo = r.u32()?;
            let fire_cd = r.u32()?;
            let hurt_cd = r.u32()?;
            let count = r.u32()?;
            let mut enemies = Vec::new();
            for _ in 0..count {
                let x = r.f32()?;
                let y = r.f32()?;
                let z = r.f32()?;
                let sx = r.f32()?;
                let sz = r.f32()?;
                let alive = r.u8()? != 0;
                enemies.push(Enemy {
                    x,
                    y,
                    z,
                    spawn: (sx, sz),
                    alive,
                });
            }
            Some((
                px, pz, yaw, pitch, health, score, ammo, fire_cd, hurt_cd, enemies,
            ))
        })();
        match parsed {
            Some((px, pz, yaw, pitch, health, score, ammo, fire_cd, hurt_cd, enemies)) => {
                self.px = px;
                self.pz = pz;
                self.yaw = yaw;
                self.pitch = pitch;
                self.health = health;
                self.score = score;
                self.ammo = ammo;
                self.fire_cd = fire_cd;
                self.hurt_cd = hurt_cd;
                self.enemies = enemies;
                true
            }
            None => false,
        }
    }

    /// Is the world cell containing `(x, z)` a wall (or out of bounds)?
    fn is_wall(&self, x: f32, z: f32) -> bool {
        map_wall_at(&self.level.map, x, z)
    }

    /// Is the straight segment from the player to `(tx, tz)` free of walls? A
    /// coarse march in ~quarter-cell steps — enough to stop shots through walls.
    fn line_clear(&self, tx: f32, tz: f32) -> bool {
        let (dx, dz) = (tx - self.px, tz - self.pz);
        let dist = (dx * dx + dz * dz).sqrt();
        let steps = (dist / 0.25).ceil() as i32;
        !(1..steps).any(|i| {
            let t = i as f32 / steps as f32;
            self.is_wall(self.px + dx * t, self.pz + dz * t)
        })
    }

    /// Advance one deterministic tick under `intent`, returning the engine
    /// commands (camera control + enemy moves) and the HUD. When the player has
    /// died, this tick snaps the camera back to the start and respawns enemies.
    pub fn step(&mut self, intent: Intent) -> StepCommands {
        // Dead → respawn; else the live tick. Compute both as kind-gated paths
        // and pick one without an `if`: a dead player runs `respawn` and parks
        // the live tick, a living one does the reverse.
        let dead = self.health <= 0;
        dead.then(|| self.respawn())
            .map_or_else(|| self.live_step(intent), |cmds| cmds)
    }

    /// The normal (alive) tick: look, move, shoot, enemy chase, contact damage.
    fn live_step(&mut self, intent: Intent) -> StepCommands {
        self.fire_cd = self.fire_cd.saturating_sub(1);
        self.hurt_cd = self.hurt_cd.saturating_sub(1);
        let tun = self.level.tun;

        // 1. Look: keypad/key turn plus mouse yaw; mouse pitch (clamped). Then
        //    move relative to the new yaw.
        let yaw_delta = (intent.turn_left as i32 - intent.turn_right as i32) as f32
            * tun.turn_speed
            + intent.look_yaw;
        let pitch_delta = intent.look_pitch;
        self.yaw += yaw_delta;
        self.pitch = (self.pitch + pitch_delta).clamp(-tun.pitch_limit, tun.pitch_limit);
        let forward = (intent.forward as i32 - intent.backward as i32) as f32 * tun.move_speed;
        let strafe =
            (intent.strafe_right as i32 - intent.strafe_left as i32) as f32 * tun.move_speed;
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
        ((self.hurt_cd == 0) & self.any_enemy_in_contact()).then(|| {
            self.health -= tun.contact_damage;
            self.hurt_cd = tun.hurt_cooldown;
        });

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
        let nx = self
            .is_wall(self.px + dx, self.pz)
            .then_some(self.px)
            .unwrap_or(self.px + dx);
        let nz = self
            .is_wall(self.px, self.pz + dz)
            .then_some(self.pz)
            .unwrap_or(self.pz + dz);
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
        // Gate the whole shot on the fire guard (all-pure checks → `&`), so no
        // early-return branch is needed.
        (firing & (self.fire_cd == 0) & (self.ammo != 0)).then(|| self.fire_shot());
    }

    /// The actual shot, run only once the fire guard passes: spend ammo and kill
    /// the nearest eligible enemy. The "nearest eligible" search is a `fold` over
    /// the enemies, each eligibility test reduced to a boolean mask.
    fn fire_shot(&mut self) {
        let tun = self.level.tun;
        self.fire_cd = tun.fire_cooldown;
        self.ammo -= 1;
        let (fx, fz) = (-self.yaw.sin(), -self.yaw.cos());
        let cone = tun.fire_half_angle.cos();
        let range = tun.fire_range;
        let best = self
            .enemies
            .iter()
            .enumerate()
            .fold(None::<(usize, f32)>, |best, (i, e)| {
                let (dx, dz) = (e.x - self.px, e.z - self.pz);
                let dist = (dx * dx + dz * dz).sqrt();
                let aim = (fx * dx + fz * dz) / dist;
                // Eligible iff alive, in range, inside the cone, and unobstructed.
                // `line_clear` is only reached when the cheap masks already hold
                // (it is pure regardless), so a flat `&` chain is safe.
                let eligible = e.alive
                    & (1.0e-4..=range).contains(&dist)
                    & (aim >= cone)
                    & self.line_clear(e.x, e.z);
                let closer = best.is_none_or(|(_, d)| dist < d);
                (eligible & closer).then_some((i, dist)).or(best)
            });
        best.iter().for_each(|&(i, _)| {
            self.enemies[i].alive = false;
            self.score += tun.kill_score;
            self.ammo += tun.ammo_per_kill;
        });
    }

    /// Move every enemy one tick and return the world-delta [`PlayerInput`]s. A
    /// living enemy chases the player (axis-separated wall stops); a dead enemy
    /// parks below the floor; on `respawning` every enemy returns to its spawn
    /// alive. Each enemy's stored pose is updated to the new target, so the delta
    /// always equals `target - current` — the engine node tracks it exactly.
    fn update_enemies(&mut self, respawning: bool) -> Vec<PlayerInput> {
        let (px, pz) = (self.px, self.pz);
        let tun = self.level.tun;
        let map = &self.level.map;
        self.enemies
            .iter_mut()
            .enumerate()
            .map(|(i, e)| {
                // Respawn forces the enemy alive; the post-respawn liveness then
                // selects the target without an `if`. All three candidate targets
                // are pure to compute, so evaluating them unconditionally is safe.
                e.alive |= respawning;
                let respawn_t = (e.spawn.0, tun.enemy_y, e.spawn.1);
                let chase_t = Self::chase(map, &tun, e, px, pz);
                let park_t = (e.spawn.0, tun.park_y, e.spawn.1);
                // alive ? (respawning ? respawn_t : chase_t) : park_t
                let live_t = respawning.then_some(respawn_t).unwrap_or(chase_t);
                let (tx, ty, tz) = e.alive.then_some(live_t).unwrap_or(park_t);
                let input = PlayerInput::new(i as u32, Vec3::new(tx - e.x, ty - e.y, tz - e.z));
                e.x = tx;
                e.y = ty;
                e.z = tz;
                input
            })
            .collect()
    }

    /// One chase step for a living enemy toward `(px, pz)`, with per-axis wall
    /// stops. Returns its new `(x, y, z)`.
    fn chase(map: &MapData, tun: &level::Tunables, e: &Enemy, px: f32, pz: f32) -> (f32, f32, f32) {
        let (dx, dz) = (px - e.x, pz - e.z);
        let len = (dx * dx + dz * dz).sqrt();
        // Too close to move: zero out the step (mask the normalized delta to 0).
        // This avoids the `len < eps` early-return and keeps the same result —
        // when too close, `tx,tz` collapse to the enemy's own position. The
        // denominator is floored away from zero so the normalize never yields
        // NaN/inf (which the mask would not fully clear); the mask then forces a
        // genuine zero step in exactly the `len < eps` case.
        let moving = (len >= 1.0e-4) as i32 as f32;
        let safe_len = len.max(1.0e-4);
        let (nx, nz) = (
            dx / safe_len * tun.enemy_speed * moving,
            dz / safe_len * tun.enemy_speed * moving,
        );
        let tx = Self::map_wall(map, e.x + nx, e.z)
            .then_some(e.x)
            .unwrap_or(e.x + nx);
        let tz = Self::map_wall(map, e.x, e.z + nz)
            .then_some(e.z)
            .unwrap_or(e.z + nz);
        (tx, tun.enemy_y, tz)
    }

    /// Wall lookup against a map (the static helper used by enemy chasing).
    fn map_wall(map: &MapData, x: f32, z: f32) -> bool {
        map_wall_at(map, x, z)
    }

    /// Is any living enemy within melee contact of the player?
    fn any_enemy_in_contact(&self) -> bool {
        let radius = self.level.tun.contact_radius;
        self.enemies.iter().any(|e| {
            let (dx, dz) = (e.x - self.px, e.z - self.pz);
            // `&` over two pure tests (no guarded index): alive AND within range.
            e.alive & ((dx * dx + dz * dz).sqrt() < radius)
        })
    }

    /// Death → reset: snap the camera back to the start (one corrective control),
    /// respawn every enemy at its spawn, and restore health/score/ammo.
    fn respawn(&mut self) -> StepCommands {
        let tun = self.level.tun;
        // Correct yaw and pitch back to zero (look level, facing -Z). After the
        // yaw correction the frame is world-aligned, so the move back to start is
        // the world delta directly.
        let (yaw_delta, pitch_delta) = (-self.yaw, -self.pitch);
        let (dx, dz) = (
            self.level.map.start.0 - self.px,
            self.level.map.start.1 - self.pz,
        );
        let control = FirstPersonInput::new(
            0,
            Vec3::new(dx, 0.0, dz),
            Angle::radians(yaw_delta),
            Angle::radians(pitch_delta),
        );
        self.px = self.level.map.start.0;
        self.pz = self.level.map.start.1;
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.health = tun.max_health;
        self.score = 0;
        self.ammo = tun.start_ammo;
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

impl Default for RetroFpsGame {
    fn default() -> Self {
        RetroFpsGame::new()
    }
}

/// Build the engine app for a retro FPS level document: the cube-walled rooms, floor
/// and ceiling, a first-person [`Controller`] camera at the start, a directional
/// light, and one enemy cube ([`Player`]) per spawn (in the same order
/// [`RetroFpsGame`] enumerates them, so indices line up).
pub fn build_retro_fps_app(doc: &LevelDoc) -> RunningApp {
    App::new()
        .window(
            Window::new(960, 600)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(color_of(doc.colors.clear)),
        )
        .add_plugins(DefaultPlugins)
        .setup(level_setup(doc.clone()))
        .build()
}

/// Re-author a running retro FPS app onto a new level document: update the background
/// colour and rebuild the scene (walls, floor/ceiling, enemies, camera, light)
/// in place, keeping the engine ticking. This is the hot-reload entry the browser
/// arm calls when a new `level.axiom` arrives over SSE.
pub fn reload_retro_fps(running: &mut RunningApp, doc: &LevelDoc) {
    running.set_clear_color(color_of(doc.colors.clear).to_array());
    running.reauthor(level_setup(doc.clone()));
}

/// The scene-authoring closure for a level document: the cube-walled level, its
/// floor/ceiling slabs, the enemy cubes, the first-person camera at the start,
/// and the directional light. Shared by [`build_retro_fps_app`] (initial build) and
/// [`reload_retro_fps`] (live re-author), so both produce an identical scene for a
/// given document.
fn level_setup(
    doc: LevelDoc,
) -> impl FnOnce(&mut SceneCommands, &mut Assets<Mesh>, &mut Assets<Material>) {
    move |world, meshes, materials| {
        let cube = meshes.add(Mesh::cube());
        let wall_a = materials.add(Material::lit(color_of(doc.colors.wall_a)));
        let wall_b = materials.add(Material::lit(color_of(doc.colors.wall_b)));
        let floor = materials.add(Material::lit(color_of(doc.colors.floor)));
        let ceiling = materials.add(Material::lit(color_of(doc.colors.ceiling)));
        let enemy = materials.add(Material::lit(color_of(doc.colors.enemy)));
        let wh = doc.tun.wall_height;

        // Walls: one scaled cube per wall cell, two-tone by parity. The
        // wall-cell filter replaces the `continue`; the parity selects the
        // material via an index into a 2-tone table (no `if/else`).
        doc.map.walls.iter().enumerate().for_each(|(row, line)| {
            line.iter()
                .enumerate()
                .filter(|&(_, &is_wall)| is_wall)
                .for_each(|(col, _)| {
                    let mat = [wall_a, wall_b][(row + col) % 2];
                    world.spawn((
                        block(col as f32, wh * 0.5, row as f32, 1.0, wh, 1.0),
                        Renderable {
                            mesh: cube,
                            material: mat,
                        },
                    ));
                });
        });

        // Floor + ceiling: two big flat slabs spanning the whole grid.
        let (cx, cz) = (
            (doc.map.width as f32 - 1.0) * 0.5,
            (doc.map.height as f32 - 1.0) * 0.5,
        );
        let (gw, gh) = (doc.map.width as f32, doc.map.height as f32);
        world.spawn((
            block(cx, -0.05, cz, gw, 0.1, gh),
            Renderable {
                mesh: cube,
                material: floor,
            },
        ));
        world.spawn((
            block(cx, wh + 0.05, cz, gw, 0.1, gh),
            Renderable {
                mesh: cube,
                material: ceiling,
            },
        ));

        // Enemies: a red cube Player per spawn, in row-major (index) order.
        let scale = doc.tun.enemy_scale;
        doc.map
            .enemy_spawns
            .iter()
            .enumerate()
            .for_each(|(i, &(x, z))| {
                world.spawn((
                    block(x, doc.tun.enemy_y, z, scale, scale, scale),
                    Renderable {
                        mesh: cube,
                        material: enemy,
                    },
                    Player::new(i as u32),
                ));
            });

        // The first-person camera at the start, facing -Z (yaw 0).
        world.spawn((
            Transform::from_translation(Vec3::new(doc.map.start.0, doc.tun.eye, doc.map.start.1)),
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
    }
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
    use level::Tunables;

    fn idle() -> Intent {
        Intent::default()
    }

    #[test]
    fn the_builtin_level_has_a_start_and_enemies() {
        let doc = LevelDoc::default();
        // The default grid is rectangular with a start and four enemy spawns.
        assert!(doc.map.walls.iter().all(|r| r.len() == doc.map.width));
        assert_eq!(doc.map.enemy_spawns.len(), 4);
        // The start cell is open floor.
        let (sx, sz) = doc.map.start;
        assert!(!doc.map.walls[sz as usize][sx as usize]);
    }

    #[test]
    fn walking_into_a_wall_does_not_pass_through_it() {
        // The start is the bottom-left corner; -Z (forward) leads up the room, but
        // strafing left (-X) immediately hits the west wall.
        let mut g = RetroFpsGame::new();
        for _ in 0..30 {
            g.step(Intent {
                strafe_left: true,
                ..idle()
            });
        }
        // The player slides to the floor-cell boundary (~x=0.5) but the west wall
        // (column 0) stops it there — it never crosses into the wall cell.
        assert!(g.px >= 0.49, "the west wall blocks leftward strafing");
        assert!(
            !g.is_wall(g.px, g.pz),
            "the player never ends inside a wall"
        );
    }

    #[test]
    fn moving_forward_advances_along_negative_z() {
        let mut g = RetroFpsGame::new();
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
        let mut g = RetroFpsGame::new();
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
        assert_eq!(g.pitch, Tunables::default().pitch_limit);
    }

    #[test]
    fn turning_changes_yaw_and_is_reported_in_the_control() {
        let mut g = RetroFpsGame::new();
        let cmd = g.step(Intent {
            turn_left: true,
            ..idle()
        });
        assert!(g.yaw > 0.0);
        assert_eq!(cmd.control.yaw.as_radians(), Tunables::default().turn_speed);
    }

    #[test]
    fn firing_down_the_lane_kills_the_aligned_enemy() {
        // Place the player just south of the left-room enemy at (4,3) and aim at
        // it (north = -Z, the default facing), then fire.
        let mut g = RetroFpsGame::new();
        g.px = 4.0;
        g.pz = 6.0;
        g.yaw = 0.0; // facing -Z, straight at the (4,3) enemy
        let before = g.hud().enemies_alive;
        let cmd = g.step(Intent {
            fire: true,
            ..idle()
        });
        let tun = Tunables::default();
        assert_eq!(cmd.hud.enemies_alive, before - 1, "the lined-up enemy dies");
        assert_eq!(cmd.hud.score, tun.kill_score);
        assert_eq!(cmd.hud.ammo, tun.start_ammo - 1 + tun.ammo_per_kill);
    }

    #[test]
    fn a_dead_enemy_is_parked_below_the_floor() {
        let mut g = RetroFpsGame::new();
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
        let mut g = RetroFpsGame::new();
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
        let mut g = RetroFpsGame::new();
        let tun = Tunables::default();
        let spawn = g.enemies[0].spawn;
        g.px = spawn.0;
        g.pz = spawn.1;
        let mut saw_damage = false;
        // Enough ticks to drain full health through the hurt cooldown and die.
        let max_ticks =
            (tun.max_health / tun.contact_damage + 2) as usize * tun.hurt_cooldown as usize;
        for _ in 0..max_ticks {
            let h = g.health;
            g.step(idle());
            if g.health < h {
                saw_damage = true;
            }
        }
        assert!(saw_damage, "contact with an enemy costs health");
        // After enough contact the player died and respawned at the start.
        assert_eq!(g.health, tun.max_health);
        assert_eq!((g.px, g.pz), g.level.map.start);
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
            let mut g = RetroFpsGame::new();
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
    fn a_longer_wall_run_in_the_document_adds_wall_renderables() {
        // Fill the second row entirely with walls; the scene gains the extra wall
        // instances those new '#' cells produce.
        let base = LevelDoc::default();
        let base_walls: usize = base.map.walls.iter().flatten().filter(|&&w| w).count();
        let doc = LevelDoc::parse(
            "[map]\n\
             ##################\n\
             ##################\n\
             #.......#........#\n\
             #...E...#....E...#\n\
             #.......#........#\n\
             #................#\n\
             #.......#........#\n\
             #...E...#........#\n\
             #S......#........#\n\
             ##################\n",
        );
        let new_walls: usize = doc.map.walls.iter().flatten().filter(|&&w| w).count();
        assert!(new_walls > base_walls, "filling a row adds walls");
        // The realized scene reflects the new wall count (walls + floor + ceiling
        // + one cube per enemy).
        let app = build_retro_fps_app(&doc);
        assert_eq!(
            app.renderable_count(),
            new_walls + 2 + doc.map.enemy_spawns.len()
        );
    }

    #[test]
    fn reload_retro_fps_reauthors_the_running_scene() {
        // Start on the built-in level, then hot-reload onto a tiny level: the
        // renderable count changes and the engine keeps ticking.
        let mut running = build_retro_fps_app(&LevelDoc::default());
        let before = running.renderable_count();
        let _ = running.tick(0);
        let tiny = LevelDoc::parse("[map]\n#####\n#S.E#\n#####\n");
        reload_retro_fps(&mut running, &tiny);
        let tiny_walls: usize = tiny.map.walls.iter().flatten().filter(|&&w| w).count();
        assert_eq!(
            running.renderable_count(),
            tiny_walls + 2 + tiny.map.enemy_spawns.len()
        );
        assert_ne!(before, running.renderable_count());
        // The next frame renders the reloaded scene at the continuing tick.
        let frame = running.tick(1);
        assert_eq!(frame.tick(), 1);
    }

    #[test]
    fn the_scene_has_walls_floor_ceiling_and_one_renderable_per_enemy() {
        let doc = LevelDoc::default();
        let app = build_retro_fps_app(&doc);
        let wall_count: usize = doc.map.walls.iter().flatten().filter(|&&w| w).count();
        // walls + floor + ceiling + one cube per enemy.
        let expected = wall_count + 2 + doc.map.enemy_spawns.len();
        assert_eq!(app.renderable_count(), expected);
    }

    #[test]
    fn the_first_frame_draws_every_renderable_and_runs_deterministically() {
        let mut a = build_retro_fps_app(&LevelDoc::default());
        let mut b = build_retro_fps_app(&LevelDoc::default());
        let fa = a.tick(0);
        assert_eq!(fa.draws().len(), a.renderable_count());
        assert_eq!(fa, b.tick(0), "tick 0 replays byte-identically");
    }

    #[test]
    fn default_game_matches_new() {
        assert_eq!(RetroFpsGame::default().hud(), RetroFpsGame::new().hud());
    }
}
