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

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::*;

pub mod level;

use level::LevelDoc;

/// The spatial-query surface the game asks the engine each tick — the engine
/// answers "what is where" so the game never re-derives geometry, returning the
/// first-class [`Entity`] of whatever it hits. Implemented by the engine's
/// [`RunningApp`]; the game holds only this narrow view of it, which also lets a
/// test drive the same logic against a real headless engine.
pub trait RetroFpsSpace {
    /// The [`Entity`] of the nearest bounded node a ray enters within `max`.
    fn raycast(&self, origin: Vec3, direction: Vec3, max: Meters) -> Option<Entity>;
    /// The [`Entity`] of every bounded node overlapping the query box.
    fn overlap_box(&self, center: Vec3, half_extents: Vec3) -> Vec<Entity>;
}

impl RetroFpsSpace for RunningApp {
    fn raycast(&self, origin: Vec3, direction: Vec3, max: Meters) -> Option<Entity> {
        RunningApp::raycast(self, origin, direction, max)
    }
    fn overlap_box(&self, center: Vec3, half_extents: Vec3) -> Vec<Entity> {
        RunningApp::overlap_box(self, center, half_extents)
    }
}

/// The engine handles the game holds to re-spawn enemies at runtime: the cube
/// mesh and the enemy material, captured from [`level_setup`] (the only place
/// `Assets::add` mints them). Replaces the old hand-kept `CUBE_MESH_ID`/
/// `ENEMY_MATERIAL_ID` constants with real, type-safe [`Handle`]s.
#[derive(Debug, Clone, Copy)]
pub struct RetroFpsAssets {
    cube: Handle<Mesh>,
    enemy: Handle<Material>,
}

/// Half-width of the player's collision box (world units): the player is a small
/// box swept against wall geometry; enemies are walked through (contact damage).
const PLAYER_HALF: f32 = 0.2;

/// Half-extent of an enemy's bounds in the unit-cube frame (the engine scales it
/// by the enemy's world scale). Used both when authoring enemies and re-spawning.
const ENEMY_BOUNDS_HALF: f32 = 0.5;

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

/// An enemy's commanded engine pose plus its spawn, liveness, and the [`Entity`]
/// of its engine node. The `x,y,z` fields are exactly what the engine node holds
/// (the app only ever moves it by the delta it stores back here), so no read-back
/// is needed. `entity` is the engine handle used to despawn it and to recognize
/// it in raycast/overlap hits; it is `None` only between construction and binding.
#[derive(Debug, Clone, Copy)]
struct Enemy {
    x: f32,
    y: f32,
    z: f32,
    spawn: (f32, f32),
    alive: bool,
    entity: Option<Entity>,
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

/// The per-tick commands the engine applies: the first-person camera control,
/// one world-delta move per *living* enemy, the player indices to despawn this
/// tick (enemies killed this frame), and the HUD snapshot.
#[derive(Debug, Clone)]
pub struct StepCommands {
    /// The camera control (controller index 0).
    pub control: FirstPersonInput,
    /// One move per living enemy (player index `i`); dead enemies issue none.
    pub enemies: Vec<PlayerInput>,
    /// Entities to despawn this tick — enemies killed this frame. The app removes
    /// their engine nodes (the engine owns object lifetime), so a killed enemy is
    /// gone for real rather than parked off-screen.
    pub despawns: Vec<Entity>,
    /// Enemies to spawn this tick — `(player index, world transform)`. Used on
    /// respawn to re-create the enemies killed (and despawned) during the life
    /// that just ended; the app turns each into an engine `spawn` and writes the
    /// returned [`Entity`] back via [`RetroFpsGame::set_enemy_entity`].
    pub spawns: Vec<(u32, Transform)>,
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
                entity: None,
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

    /// Bind each enemy to its engine [`Entity`]. The enemies are authored in
    /// `level_setup` marked `Player(i)`, so after `build_retro_fps_app` (or a fork's
    /// `restore_sim`) the game recovers each handle by index. Call once whenever a
    /// fresh or restored engine scene's enemy nodes need re-associating.
    pub fn bind_entities(&mut self, app: &RunningApp) {
        self.enemies.iter_mut().enumerate().for_each(|(i, e)| {
            e.entity = app.player_entity(i as u32);
        });
    }

    /// Record the engine [`Entity`] of enemy `index` — called after a respawn
    /// re-spawns a killed enemy and gets back its fresh handle.
    pub fn set_enemy_entity(&mut self, index: usize, entity: Entity) {
        self.enemies
            .get_mut(index)
            .into_iter()
            .for_each(|e| e.entity = Some(entity));
    }

    /// The index of the *alive* enemy whose engine node is `entity`, if any — the
    /// classifier for raycast / overlap hits. A hit that resolves to `None` is
    /// geometry (a wall); a dead enemy's stale handle never matches (it is
    /// despawned and filtered by liveness), so it cannot alias a new occupant.
    fn enemy_index_of(&self, entity: Entity) -> Option<usize> {
        self.enemies
            .iter()
            .position(|e| e.alive && e.entity == Some(entity))
    }

    /// Does `entity` belong to a live enemy? (`false` ⇒ it's geometry — a wall.)
    fn is_enemy(&self, entity: Entity) -> bool {
        self.enemy_index_of(entity).is_some()
    }

    /// The [`Entity`] handles of every *bound, live* enemy — the classification set
    /// the static enemy-movement helpers ([`Self::chase`], [`Self::enemy_wall_blocked`])
    /// carry, since they run inside `self.enemies.iter_mut()` and so cannot borrow
    /// `&self` to call [`Self::is_enemy`]. Snapshotted once per move tick.
    fn enemy_entities(&self) -> Vec<Entity> {
        self.enemies
            .iter()
            .filter(|e| e.alive)
            .filter_map(|e| e.entity)
            .collect()
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

    /// The player's current pose: world position `(x, z)`, look `yaw` and `pitch`
    /// (radians). The eye height is the level's `tun.eye`. Read by the debug
    /// overlay and the agent bridge so a session can see exactly where the player
    /// stands and which way they look — the readout that makes a view-dependent
    /// visual artifact reproducible at an exact pose.
    pub fn pose(&self) -> (f32, f32, f32, f32) {
        (self.px, self.pz, self.yaw, self.pitch)
    }

    /// The number of enemy slots (live or dead). Their stable [`Player`] indices
    /// are `0..enemy_count()` — the subject ids a perceiving agent tracks them by
    /// (a respawn reuses the same index, so the id is stable across a kill).
    pub fn enemy_count(&self) -> usize {
        self.enemies.len()
    }

    /// The world Y a perception/aim ray is cast at — the enemy centre height, so a
    /// horizontal probe meets both enemy boxes and full-height walls (the same
    /// height [`Self::fire_shot`] aims at). The perception adapter casts its sight
    /// rays from here.
    pub fn sight_height(&self) -> f32 {
        self.level.tun.enemy_y
    }

    /// Teleport the player to an absolute pose, returning the one corrective
    /// [`FirstPersonInput`] that moves the engine's camera there in a single tick.
    /// The controller applies the yaw first, then moves by `Ry(-yaw)·move_local`,
    /// so we hand it `move_local = Ry(yaw)·world_delta` (the same local-frame
    /// convention as [`Self::move_player`]). Walls are ignored — a debug teleport
    /// for scripting "stand here, look there" reproductions. Pitch is clamped to
    /// the level limit so the app mirror stays in lockstep with the engine's
    /// clamped controller.
    pub fn teleport(&mut self, px: f32, pz: f32, yaw: f32, pitch: f32) -> FirstPersonInput {
        let pitch = pitch.clamp(-self.level.tun.pitch_limit, self.level.tun.pitch_limit);
        let (dx, dz) = (px - self.px, pz - self.pz);
        let (s, c) = (yaw.sin(), yaw.cos());
        let move_local = Vec3::new(dx * c - dz * s, 0.0, dx * s + dz * c);
        let control = FirstPersonInput::new(
            0,
            move_local,
            Angle::radians(yaw - self.yaw),
            Angle::radians(pitch - self.pitch),
        );
        self.px = px;
        self.pz = pz;
        self.yaw = yaw;
        self.pitch = pitch;
        control
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
                    // Entity handles are not serialized (they would break replay
                    // determinism); a restored game re-binds them to the restored
                    // engine nodes via `bind_entities` after `restore_sim`.
                    entity: None,
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

    /// Advance one deterministic tick under `intent`, asking the engine `space`
    /// for every spatial answer (wall collision, hitscan, contact). When the
    /// player has died, this tick snaps the camera back to the start and respawns
    /// enemies.
    pub fn step(&mut self, intent: Intent, space: &dyn RetroFpsSpace) -> StepCommands {
        if self.health <= 0 {
            self.respawn(space)
        } else {
            self.live_step(intent, space)
        }
    }

    /// The normal (alive) tick: look, move, shoot, contact damage, enemy chase.
    fn live_step(&mut self, intent: Intent, space: &dyn RetroFpsSpace) -> StepCommands {
        self.fire_cd = self.fire_cd.saturating_sub(1);
        self.hurt_cd = self.hurt_cd.saturating_sub(1);
        let tun = self.level.tun;

        // 1. Look, then move relative to the new yaw (walls answered by the engine).
        let yaw_delta = (intent.turn_left as i32 - intent.turn_right as i32) as f32
            * tun.turn_speed
            + intent.look_yaw;
        let pitch_delta = intent.look_pitch;
        self.yaw += yaw_delta;
        self.pitch = (self.pitch + pitch_delta).clamp(-tun.pitch_limit, tun.pitch_limit);
        let forward = (intent.forward as i32 - intent.backward as i32) as f32 * tun.move_speed;
        let strafe =
            (intent.strafe_right as i32 - intent.strafe_left as i32) as f32 * tun.move_speed;
        let move_local = self.move_player(forward, strafe, space);
        let control = FirstPersonInput::new(
            0,
            move_local,
            Angle::radians(yaw_delta),
            Angle::radians(pitch_delta),
        );

        // 2. Shoot: one engine raycast — the nearest hit is the target or a wall.
        //    A kill names the enemy's Entity to despawn this tick (gone for real,
        //    not parked).
        let despawns: Vec<Entity> = self.fire(intent.fire, space).into_iter().collect();

        // 3. Contact damage before enemies move, so it reads the engine's
        //    start-of-frame enemy positions (this frame's moves land on `tick`).
        if self.hurt_cd == 0 && self.any_enemy_in_contact(space) {
            self.health -= tun.contact_damage;
            self.hurt_cd = tun.hurt_cooldown;
        }

        // 4. Living enemies chase; dead ones issue no move (they are despawned).
        let enemies = self.update_enemies(space, false);

        StepCommands {
            control,
            enemies,
            despawns,
            spawns: Vec::new(),
            hud: self.hud(),
        }
    }

    /// Apply a `(forward, strafe)` move at the current yaw with axis-separated
    /// wall sliding, returning the `move_local` (camera frame) that reproduces the
    /// applied motion in the engine. Wall hits are engine overlap queries.
    fn move_player(&mut self, forward: f32, strafe: f32, space: &dyn RetroFpsSpace) -> Vec3 {
        let (sin, cos) = (self.yaw.sin(), self.yaw.cos());
        let dx = strafe * cos - forward * sin;
        let dz = -strafe * sin - forward * cos;
        let nx = if self.wall_blocked(space, self.px + dx, self.pz) {
            self.px
        } else {
            self.px + dx
        };
        let nz = if self.wall_blocked(space, self.px, self.pz + dz) {
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

    /// Is a player-sized box at `(x, z)` (eye height) overlapping wall geometry?
    /// Live-enemy hits are ignored — the player walks through them — so only an
    /// Entity that is *not* a live enemy (i.e. geometry) blocks.
    fn wall_blocked(&self, space: &dyn RetroFpsSpace, x: f32, z: f32) -> bool {
        let center = Vec3::new(x, self.level.tun.eye, z);
        let half = Vec3::new(PLAYER_HALF, PLAYER_HALF, PLAYER_HALF);
        space
            .overlap_box(center, half)
            .into_iter()
            .any(|hit| !self.is_enemy(hit))
    }

    /// Resolve a fire input: spend ammo on cooldown, then cast one ray along the
    /// facing. The engine returns the nearest bounded node — an enemy dies, a wall
    /// (or nothing) blocks — so line-of-sight and target selection are one query.
    /// Returns the [`Entity`] of an enemy killed this shot (to despawn it).
    fn fire(&mut self, firing: bool, space: &dyn RetroFpsSpace) -> Option<Entity> {
        if firing && self.fire_cd == 0 && self.ammo != 0 {
            self.fire_shot(space)
        } else {
            None
        }
    }

    fn fire_shot(&mut self, space: &dyn RetroFpsSpace) -> Option<Entity> {
        let tun = self.level.tun;
        self.fire_cd = tun.fire_cooldown;
        self.ammo -= 1;
        // Cast at enemy height so the horizontal ray meets enemy boxes and walls.
        let origin = Vec3::new(self.px, tun.enemy_y, self.pz);
        let dir = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let range = Meters::new(tun.fire_range).expect("authored fire range is finite");
        // The nearest hit is the target or a wall; classify it against the live
        // enemies. A wall (or nothing) yields no kill; an enemy dies and its
        // Entity is returned so the caller despawns that exact node.
        match space
            .raycast(origin, dir, range)
            .and_then(|hit| self.enemy_index_of(hit).map(|index| (hit, index)))
        {
            Some((entity, index)) => {
                self.enemies[index].alive = false;
                self.score += tun.kill_score;
                self.ammo += tun.ammo_per_kill;
                Some(entity)
            }
            None => None,
        }
    }

    /// Move every *living* enemy one tick and return their world-delta
    /// [`PlayerInput`]s. A living enemy chases the player (axis-separated wall
    /// stops); on `respawning` it snaps back to its spawn cell instead. Dead
    /// enemies are skipped entirely — they were despawned when killed, so there is
    /// no node to move. Each enemy's stored pose is updated to the new target, so
    /// the delta equals `target - current` and the engine node tracks it exactly.
    fn update_enemies(&mut self, space: &dyn RetroFpsSpace, respawning: bool) -> Vec<PlayerInput> {
        let (px, pz) = (self.px, self.pz);
        let tun = self.level.tun;
        // Snapshot the live-enemy handles before the mutable walk so `chase` can
        // tell wall geometry from enemy boxes without re-borrowing `self`.
        let enemy_entities = self.enemy_entities();
        self.enemies
            .iter_mut()
            .enumerate()
            .filter(|(_, e)| e.alive)
            .map(|(i, e)| {
                let (tx, ty, tz) = if respawning {
                    // Respawn: snap back to the spawn cell.
                    (e.spawn.0, tun.enemy_y, e.spawn.1)
                } else {
                    // Chase the player, stopped by walls (engine queries).
                    Self::chase(space, &tun, e, px, pz, &enemy_entities)
                };
                let input = PlayerInput::new(i as u32, Vec3::new(tx - e.x, ty - e.y, tz - e.z));
                e.x = tx;
                e.y = ty;
                e.z = tz;
                input
            })
            .collect()
    }

    /// One chase step for a living enemy toward `(px, pz)`, with per-axis wall
    /// stops from the engine. Returns its new `(x, y, z)`.
    fn chase(
        space: &dyn RetroFpsSpace,
        tun: &level::Tunables,
        e: &Enemy,
        px: f32,
        pz: f32,
        enemy_entities: &[Entity],
    ) -> (f32, f32, f32) {
        let (dx, dz) = (px - e.x, pz - e.z);
        let len = (dx * dx + dz * dz).sqrt();
        // Too close to move: zero the step (the normalized delta is masked to 0),
        // with the denominator floored away from zero so it never yields NaN/inf.
        let moving = (len >= 1.0e-4) as i32 as f32;
        let safe_len = len.max(1.0e-4);
        let (nx, nz) = (
            dx / safe_len * tun.enemy_speed * moving,
            dz / safe_len * tun.enemy_speed * moving,
        );
        let half = 0.5 * tun.enemy_scale;
        let tx = if Self::enemy_wall_blocked(space, tun, half, e.x + nx, e.z, enemy_entities) {
            e.x
        } else {
            e.x + nx
        };
        let tz = if Self::enemy_wall_blocked(space, tun, half, e.x, e.z + nz, enemy_entities) {
            e.z
        } else {
            e.z + nz
        };
        (tx, tun.enemy_y, tz)
    }

    /// Is an enemy-sized box at `(x, z)` overlapping wall geometry? Live-enemy hits
    /// (the chasing enemy itself and others, identified by `enemy_entities`) are
    /// ignored, so enemies collide only with walls.
    fn enemy_wall_blocked(
        space: &dyn RetroFpsSpace,
        tun: &level::Tunables,
        half: f32,
        x: f32,
        z: f32,
        enemy_entities: &[Entity],
    ) -> bool {
        let center = Vec3::new(x, tun.enemy_y, z);
        let h = Vec3::new(half, half, half);
        space
            .overlap_box(center, h)
            .into_iter()
            .any(|hit| !enemy_entities.contains(&hit))
    }

    /// Is any living enemy within melee contact of the player? An engine overlap
    /// of a box around the player, sized so a hit triggers near `contact_radius`.
    fn any_enemy_in_contact(&self, space: &dyn RetroFpsSpace) -> bool {
        let tun = self.level.tun;
        let enemy_half = 0.5 * tun.enemy_scale;
        let half = (tun.contact_radius - enemy_half).max(0.0);
        let center = Vec3::new(self.px, tun.enemy_y, self.pz);
        let h = Vec3::new(half, half, half);
        space
            .overlap_box(center, h)
            .into_iter()
            .any(|hit| self.is_enemy(hit))
    }

    /// Death → reset: snap the camera back to the start (one corrective control),
    /// restore health/score/ammo, **re-spawn** the enemies killed during the life
    /// that just ended (the engine creates fresh nodes), and return every
    /// surviving enemy to its spawn. A full arena reset — the spawn primitive lets
    /// death revive the dead instead of leaving them gone.
    fn respawn(&mut self, space: &dyn RetroFpsSpace) -> StepCommands {
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
        // Revive every enemy killed (and despawned) during the last life: mark it
        // alive at its spawn and request a fresh engine node for it. Survivors keep
        // their node and are returned to spawn by the move below.
        let scale = tun.enemy_scale;
        let spawns: Vec<(u32, Transform)> = self
            .enemies
            .iter_mut()
            .enumerate()
            .filter(|(_, e)| !e.alive)
            .map(|(i, e)| {
                e.alive = true;
                e.x = e.spawn.0;
                e.y = tun.enemy_y;
                e.z = e.spawn.1;
                (
                    i as u32,
                    block(e.spawn.0, tun.enemy_y, e.spawn.1, scale, scale, scale),
                )
            })
            .collect();
        let enemies = self.update_enemies(space, true);
        StepCommands {
            control,
            enemies,
            despawns: Vec::new(),
            spawns,
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
pub fn build_retro_fps_app(doc: &LevelDoc) -> (RunningApp, RetroFpsAssets) {
    let sink: Rc<RefCell<Option<RetroFpsAssets>>> = Rc::new(RefCell::new(None));
    let app = App::new()
        .window(
            Window::new(960, 600)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(color_of(doc.colors.clear)),
        )
        .add_plugins(DefaultPlugins)
        .setup(level_setup(doc.clone(), sink.clone()))
        .build();
    let assets = (*sink.borrow()).expect("level_setup captured the enemy assets");
    (app, assets)
}

/// Re-author a running retro FPS app onto a new level document: update the background
/// colour and rebuild the scene (walls, floor/ceiling, enemies, camera, light)
/// in place, keeping the engine ticking. This is the hot-reload entry the browser
/// arm calls when a new `level.axiom` arrives over SSE. Returns the freshly-minted
/// [`RetroFpsAssets`] (the new scene's cube mesh + enemy material), since re-authoring
/// registers new handles the caller must keep for runtime re-spawns.
pub fn reload_retro_fps(running: &mut RunningApp, doc: &LevelDoc) -> RetroFpsAssets {
    let sink: Rc<RefCell<Option<RetroFpsAssets>>> = Rc::new(RefCell::new(None));
    running.set_clear_color(color_of(doc.colors.clear).to_array());
    running.reauthor(level_setup(doc.clone(), sink.clone()));
    // Copy the captured assets out (RetroFpsAssets: Copy), dropping the borrow first.
    let captured = *sink.borrow();
    captured.expect("level_setup captured the enemy assets")
}

/// Apply a step's lifecycle commands to the engine before its tick: despawn the
/// enemies killed this frame (by [`Entity`]), then spawn the ones a respawn
/// revived, writing each new node's [`Entity`] back into the matching [`Enemy`]
/// via [`RetroFpsGame::set_enemy_entity`]. Centralizes the enemy prototype (a cube
/// mesh, the enemy material, bounds, and a contact shadow), built from the
/// app-held `RetroFpsAssets` handles, so the browser, the agent bridge, and the tests
/// all create enemies identically.
pub fn apply_lifecycle(
    game: &mut RetroFpsGame,
    running: &mut RunningApp,
    assets: &RetroFpsAssets,
    commands: &StepCommands,
) {
    commands.despawns.iter().for_each(|&entity| {
        running.despawn(entity);
    });
    commands.spawns.iter().for_each(|&(index, transform)| {
        let entity = running.spawn(
            Spawn::new(transform, assets.cube, assets.enemy)
                .with_player(index)
                .with_bounds(Vec3::new(
                    ENEMY_BOUNDS_HALF,
                    ENEMY_BOUNDS_HALF,
                    ENEMY_BOUNDS_HALF,
                ))
                .casts_contact_shadow(),
        );
        game.set_enemy_entity(index as usize, entity);
    });
}

/// The scene-authoring closure for a level document: the cube-walled level, its
/// floor/ceiling slabs, the enemy cubes, the first-person camera at the start,
/// and the directional light. Shared by [`build_retro_fps_app`] (initial build) and
/// [`reload_retro_fps`] (live re-author), so both produce an identical scene for a
/// given document.
fn level_setup(
    doc: LevelDoc,
    assets_sink: Rc<RefCell<Option<RetroFpsAssets>>>,
) -> impl FnOnce(&mut SceneCommands, &mut Assets<Mesh>, &mut Assets<Material>) {
    move |world, meshes, materials| {
        let cube = meshes.add(Mesh::cube());
        let wall_a = materials.add(Material::lit(color_of(doc.colors.wall_a)));
        let wall_b = materials.add(Material::lit(color_of(doc.colors.wall_b)));
        let floor = materials.add(Material::lit(color_of(doc.colors.floor)));
        let ceiling = materials.add(Material::lit(color_of(doc.colors.ceiling)));
        let enemy = materials.add(Material::lit(color_of(doc.colors.enemy)));
        // Hand the runtime-respawn handles (cube mesh + enemy material) back to the
        // builder, which holds them in RetroFpsAssets for `apply_lifecycle` to spawn
        // fresh enemies against the same registered assets.
        *assets_sink.borrow_mut() = Some(RetroFpsAssets { cube, enemy });
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
                        // A wall is queryable geometry: its unit-cube bounds, sized
                        // by the wall's scale, is exactly the cell box the player's
                        // movement and shots collide with (engine spatial queries).
                        Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
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

        // Enemies: a red cube Player per spawn, in row-major (index) order. Each is
        // a discrete dynamic object, so it is marked a contact-shadow caster — the
        // Canvas 2D backend grounds it with a real, depth-tested planar shadow on
        // the floor (the walls, being level geometry, cast none).
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
                    ContactShadowCaster,
                    // The enemy is queryable too: its bounds (sized by the enemy
                    // scale) is the hitbox the player's hitscan and contact checks
                    // resolve against, classified `Player(i)` by its marker.
                    Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
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

// The retro FPS agent: drives the real game through the reusable `axiom-agent` module
// (observe → decide → emit player-equivalent intents) and owns the app-side
// translation between the module's neutral contracts and the retro FPS `Intent`. The
// native HTTP bridge bin sits on top of it. Native + `agent`-feature only, so the
// wasm build and default gates skip it.
#[cfg(all(feature = "retro-fps-agent", not(target_arch = "wasm32")))]
pub mod agent;

// Live, game-agnostic perception: the app-side sense adapter that casts the
// reusable `axiom-perception` ray-fan against this game's engine world, classifies
// hits by their entity-native `Tag`, tracks moving enemies, and feeds the neutral
// facts through the same `axiom-agent` loop — so the agent genuinely sees and
// reasons about what it sees. Native + `agent`-feature only.
#[cfg(all(feature = "retro-fps-agent", not(target_arch = "wasm32")))]
pub mod perception;

#[cfg(test)]
mod tests {
    use super::*;
    use level::Tunables;

    fn idle() -> Intent {
        Intent::default()
    }

    /// A fresh game plus its engine app and runtime assets on the built-in level —
    /// the same trio the browser and agent drive. The game asks `app` for every
    /// spatial answer, and its enemies are bound to their engine [`Entity`]s so
    /// raycast/overlap hits classify correctly from the first tick.
    fn game_and_app() -> (RetroFpsGame, RunningApp, RetroFpsAssets) {
        let doc = LevelDoc::default();
        let mut game = RetroFpsGame::from_level(&doc);
        let (app, assets) = build_retro_fps_app(&doc);
        game.bind_entities(&app);
        (game, app, assets)
    }

    /// One real frame: step the game against the engine, apply its lifecycle
    /// commands (despawn killed / spawn revived enemies, rebinding new handles),
    /// then tick so the engine's world tracks the game — exactly the web/agent loop.
    fn drive(
        game: &mut RetroFpsGame,
        app: &mut RunningApp,
        assets: &RetroFpsAssets,
        tick: u64,
        intent: Intent,
    ) -> StepCommands {
        let cmd = game.step(intent, &*app);
        apply_lifecycle(game, app, assets, &cmd);
        app.tick_with_controls(tick, &cmd.enemies, &[cmd.control]);
        cmd
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
        // Strafing left (-X) from the start runs into the west wall; the engine's
        // overlap query stops the player's box before it crosses into the wall.
        let (mut g, mut app, assets) = game_and_app();
        let start_x = g.level.map.start.0;
        for t in 0..30 {
            drive(
                &mut g,
                &mut app,
                &assets,
                t,
                Intent {
                    strafe_left: true,
                    ..idle()
                },
            );
        }
        assert!(g.px < start_x, "the player slid west");
        assert!(g.px >= 0.49, "the west wall blocks leftward strafing");
        // The player never ends inside a wall: a tiny probe box at its position
        // finds no geometry (the engine kept it out). A hit that isn't a live
        // enemy is geometry.
        let eye = Tunables::default().eye;
        let probe = app.overlap_box(Vec3::new(g.px, eye, g.pz), Vec3::new(0.05, 0.05, 0.05));
        assert!(
            !probe.iter().any(|&h| !g.is_enemy(h)),
            "the player never ends inside a wall"
        );
    }

    #[test]
    fn moving_forward_advances_along_negative_z() {
        let (mut g, mut app, assets) = game_and_app();
        let z0 = g.pz;
        let cmd = drive(
            &mut g,
            &mut app,
            &assets,
            0,
            Intent {
                forward: true,
                ..idle()
            },
        );
        assert!(g.pz < z0, "forward moves up the room (-Z)");
        // The control reproduces the move: local forward is -Z.
        assert!(cmd.control.move_local.z < 0.0);
    }

    #[test]
    fn mouse_look_feeds_yaw_and_pitch_into_the_control_and_clamps_pitch() {
        let (mut g, mut app, assets) = game_and_app();
        let cmd = drive(
            &mut g,
            &mut app,
            &assets,
            0,
            Intent {
                look_yaw: 0.3,
                look_pitch: -0.2,
                ..idle()
            },
        );
        // Mouse yaw/pitch flow straight into the controller input...
        assert_eq!(cmd.control.yaw.as_radians(), 0.3);
        assert_eq!(cmd.control.pitch.as_radians(), -0.2);
        assert!((g.yaw - 0.3).abs() < 1.0e-6);
        // ...and the tracked pitch clamps to the engine's limit on a big look.
        drive(
            &mut g,
            &mut app,
            &assets,
            1,
            Intent {
                look_pitch: 5.0,
                ..idle()
            },
        );
        assert_eq!(g.pitch, Tunables::default().pitch_limit);
    }

    #[test]
    fn turning_changes_yaw_and_is_reported_in_the_control() {
        let (mut g, mut app, assets) = game_and_app();
        let cmd = drive(
            &mut g,
            &mut app,
            &assets,
            0,
            Intent {
                turn_left: true,
                ..idle()
            },
        );
        assert!(g.yaw > 0.0);
        assert_eq!(cmd.control.yaw.as_radians(), Tunables::default().turn_speed);
    }

    #[test]
    fn firing_down_the_lane_kills_the_aligned_enemy() {
        // Stand just south of the (4,3) enemy, aim -Z straight at it, and fire: the
        // engine raycast hits the enemy box and the shot kills it.
        let (mut g, app, _assets) = game_and_app();
        g.px = 4.0;
        g.pz = 6.0;
        g.yaw = 0.0; // facing -Z, straight at the (4,3) enemy
        let before = g.hud().enemies_alive;
        let cmd = g.step(
            Intent {
                fire: true,
                ..idle()
            },
            &app,
        );
        let tun = Tunables::default();
        assert_eq!(cmd.hud.enemies_alive, before - 1, "the lined-up enemy dies");
        assert_eq!(cmd.hud.score, tun.kill_score);
        assert_eq!(cmd.hud.ammo, tun.start_ammo - 1 + tun.ammo_per_kill);
        // The kill names exactly one enemy to despawn this tick.
        assert_eq!(cmd.despawns.len(), 1);
    }

    #[test]
    fn a_killed_enemy_is_despawned_not_parked() {
        let (mut g, mut app, _assets) = game_and_app();
        g.px = 4.0;
        g.pz = 6.0;
        let cmd = g.step(
            Intent {
                fire: true,
                ..idle()
            },
            &app,
        );
        // The kill names an enemy Entity to despawn; applying it removes the engine
        // node (the enemy is gone for real, not parked below the floor).
        assert_eq!(cmd.despawns.len(), 1);
        let killed = cmd.despawns[0];
        assert!(
            app.despawn(killed),
            "the engine had the enemy node to remove"
        );
        assert!(!app.despawn(killed), "a second despawn is a clean no-op");
        // Its game-side record is simply marked dead.
        let dead = g
            .enemies
            .iter()
            .find(|e| e.entity == Some(killed))
            .expect("the killed enemy is tracked by its Entity");
        assert!(!dead.alive);
    }

    #[test]
    fn a_wall_blocks_a_shot() {
        // Aiming through the dividing wall at a right-room enemy must miss.
        let (mut g, app, _assets) = game_and_app();
        g.px = 4.0;
        g.pz = 5.0;
        g.yaw = -std::f32::consts::FRAC_PI_2; // face +X, toward the divider wall
        let before = g.hud().enemies_alive;
        g.step(
            Intent {
                fire: true,
                ..idle()
            },
            &app,
        );
        assert_eq!(g.hud().enemies_alive, before, "the wall stops the shot");
    }

    #[test]
    fn standing_in_an_enemy_drains_then_resets_health() {
        // Stand on a living enemy: contact damage ticks down, and at zero the next
        // step respawns at full health back at the start.
        let (mut g, mut app, assets) = game_and_app();
        let tun = Tunables::default();
        let spawn = g.enemies[0].spawn;
        g.px = spawn.0;
        g.pz = spawn.1;
        let mut saw_damage = false;
        // Enough ticks to drain full health through the hurt cooldown and die.
        let max_ticks = (tun.max_health / tun.contact_damage + 2) as u64 * tun.hurt_cooldown as u64;
        for t in 0..max_ticks {
            let h = g.health;
            drive(&mut g, &mut app, &assets, t, idle());
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
    fn respawn_revives_enemies_killed_in_the_previous_life() {
        let (mut g, mut app, assets) = game_and_app();
        let total = g.hud().enemies_alive;
        // Kill the lined-up (4,3) enemy, then apply + tick so its node is despawned.
        g.px = 4.0;
        g.pz = 6.0;
        g.yaw = 0.0;
        let cmd = g.step(
            Intent {
                fire: true,
                ..idle()
            },
            &app,
        );
        assert_eq!(cmd.despawns.len(), 1, "the kill despawns one enemy");
        apply_lifecycle(&mut g, &mut app, &assets, &cmd);
        app.tick_with_controls(0, &cmd.enemies, &[cmd.control]);
        assert_eq!(g.hud().enemies_alive, total - 1);

        // Force death: the next step takes the respawn path, which re-spawns the
        // killed enemy (the survivors-only compromise is gone).
        g.health = 0;
        let cmd = g.step(idle(), &app);
        assert_eq!(cmd.spawns.len(), 1, "respawn revives the killed enemy");
        apply_lifecycle(&mut g, &mut app, &assets, &cmd);
        app.tick_with_controls(1, &cmd.enemies, &[cmd.control]);

        // The full enemy set is alive again and health is restored, and the revived
        // enemy is re-bound to its fresh engine node.
        assert_eq!(g.hud().enemies_alive, total);
        assert_eq!(g.health, Tunables::default().max_health);
        assert!(
            g.enemies.iter().all(|e| e.entity.is_some()),
            "every revived enemy is bound to a fresh Entity"
        );
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
            let (mut g, mut app, assets) = game_and_app();
            let mut last = g.hud();
            let mut t = 0;
            for _ in 0..3 {
                for &i in &script {
                    last = drive(&mut g, &mut app, &assets, t, i).hud;
                    t += 1;
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
        let (app, _assets) = build_retro_fps_app(&doc);
        assert_eq!(
            app.renderable_count(),
            new_walls + 2 + doc.map.enemy_spawns.len()
        );
    }

    #[test]
    fn reload_retro_fps_reauthors_the_running_scene() {
        // Start on the built-in level, then hot-reload onto a tiny level: the
        // renderable count changes and the engine keeps ticking.
        let (mut running, _assets) = build_retro_fps_app(&LevelDoc::default());
        let before = running.renderable_count();
        let _ = running.tick(0);
        let tiny = LevelDoc::parse("[map]\n#####\n#S.E#\n#####\n");
        let _assets = reload_retro_fps(&mut running, &tiny);
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
    fn the_level_geometry_stays_still_across_ticks() {
        // Walls/floor/ceiling are static architecture: with no input, the rendered
        // instance transforms must be byte-identical from one tick to a much later
        // one. Guards against re-introducing a per-frame geometry animation (e.g. a
        // wall bob) — the scene system would otherwise move them every frame.
        let (mut running, _assets) = build_retro_fps_app(&LevelDoc::default());
        let at_0 = running.tick(0).instance_floats();
        let at_22 = running.tick(22).instance_floats();
        assert!(!at_0.is_empty(), "the level renders geometry");
        assert_eq!(at_0, at_22, "static geometry must not move between frames");
    }

    #[test]
    fn the_scene_has_walls_floor_ceiling_and_one_renderable_per_enemy() {
        let doc = LevelDoc::default();
        let (app, _assets) = build_retro_fps_app(&doc);
        let wall_count: usize = doc.map.walls.iter().flatten().filter(|&&w| w).count();
        // walls + floor + ceiling + one cube per enemy.
        let expected = wall_count + 2 + doc.map.enemy_spawns.len();
        assert_eq!(app.renderable_count(), expected);
    }

    #[test]
    fn the_first_frame_draws_every_renderable_and_runs_deterministically() {
        let (mut a, _assets_a) = build_retro_fps_app(&LevelDoc::default());
        let (mut b, _assets_b) = build_retro_fps_app(&LevelDoc::default());
        let fa = a.tick(0);
        assert_eq!(fa.draws().len(), a.renderable_count());
        assert_eq!(fa, b.tick(0), "tick 0 replays byte-identically");
    }

    #[test]
    fn default_game_matches_new() {
        assert_eq!(RetroFpsGame::default().hud(), RetroFpsGame::new().hud());
    }
}
