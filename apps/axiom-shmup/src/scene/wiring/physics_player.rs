//! **Wiring: `physics` + `player`.** The two subsystem facades, constructed and
//! stepped.
//!
//! [`crate::physics::system::PhysicsCore`] (a port of `src/physics/index.js`)
//! and [`crate::player::system::PlayerCore`] (a port of
//! `src/player/index.js:1-752`) are complete and, until this file, nothing
//! built either of them. This is the seam that does — construction, frame
//! order, event subscription, disposal — and nothing else. It decides no
//! behaviour: every line here is either `main.js`'s `engine.add(...)` or
//! `engine.js`'s phase ordering, spelled out for two systems instead of eleven.
//!
//! ## Why the cores and not the registry
//!
//! [`crate::registry::Registry`] is the source's real composition root, and it
//! cannot admit these two:
//!
//! * `PlayerSystem::deps()` is `["physics", "world", "render"]`, and neither
//!   `world` nor `render` is a ported [`crate::registry::Subsystem`]. The
//!   moment `player` is registered, `Registry::resolve` fails with
//!   *"player" depends on unregistered subsystem "world"*.
//! * `Subsystem::init`/`fixed_update`/`update` all take `&Ctx<'_>`, and
//!   [`crate::engine::Ctx`] holds a private `&Registry`, so no code outside
//!   `crate::engine` can build one.
//! * `PlayerSystem::new` wants an `Rc<RefCell<Input>>`, because the
//!   `Subsystem` phase signatures carry no input. [`crate::scene::game::Game`]
//!   is handed `&mut Input` per frame instead.
//!
//! So the composition root here is [`crate::scene::game::Game`], and this file
//! drives the two **cores** directly, in the order the registry would have
//! produced. The `Ctx`-only doors were the only thing standing in the way, and
//! both have been widened at the source rather than worked around: the
//! subscription bodies only ever read `ctx.events`, so they are now
//! [`crate::physics::system::subscribe`] and
//! [`crate::player::system::subscribe`], taking an [`EventBus`].
//!
//! ## Frame order
//!
//! `engine.js:239-318`, restricted to these two. `physics` has no deps and
//! `player` depends on it, so physics always runs first within a phase.
//!
//! | `Game::frame` | this file |
//! |---|---|
//! | inside the `while accumulator >= FIXED_DT` loop | [`PhysicsPlayer::fixed_update`] |
//! | after `time.alpha` is set, with the rendered update | [`PhysicsPlayer::update`] |
//! | after every `update` | [`PhysicsPlayer::late_update`] |
//!
//! `late_update` is physics-only (`player` declares no `LateUpdate` phase).
//!
//! ## One BVH, not two
//!
//! `PhysicsCore::new` builds its own [`StaticWorld`](crate::physics::bvh::StaticWorld)
//! from an unbuilt soup; the level has already built and shared one.
//! [`crate::physics::system::PhysicsCore::with_static_world`] exists so this
//! seam adopts the level's — which is also the source's `addStatic` case, and
//! is what keeps the ballistics solver, the rigid bodies and the character
//! sweeps all resolving against the *same* geometry rather than two copies of
//! it that could drift.
//!
//! ## What this does not wire, and why
//!
//! * **`PlayerCore`'s low-health pass.** `index.js:180-184` creates it only
//!   when `ctx.peek('render')` answers. No `render` subsystem is ported, so
//!   the pass is not installed — the same arm the source takes.
//! * **`actor:death` -> ragdoll.** `PhysicsCore` documents it as unwired
//!   (it needs `specFromSkeleton`, unported). Nothing changes here.
//! * **The hitbox's `owner`.** `index.js:169` passes `this`; there is no actor
//!   type at this tier, so [`crate::player::system::PlayerCore::IS_PLAYER`]
//!   remains the port's stand-in and the mirrored collider carries no owner id.
//!   An `ai` slice that needs to know *whose* hitbox it hit will have to mint
//!   one.

use std::cell::RefCell;
use std::rc::Rc;

use crate::config::Config;
use crate::engine::Time;
use crate::events::{EventBus, SubscriptionId};
use crate::physics::probe::PhysicsWorld;
use crate::physics::surfaces::layer;
use crate::physics::system as physics_system;
use crate::physics::system::{
    ColliderOpts, ColliderShape, InterpolatedPose, PhysicsCore, PhysicsStats,
};
use crate::player::system as player_system;
use crate::player::system::{PlayerCore, PlayerLook, PlayerPhysics, Spawn, SpawnSource};
use crate::rng::Rng;
use crate::scene::game::CameraPose;
use crate::scene::level::Level;
use crate::world::palette::Surface;

/// `ctx.peek('world').spawn(i)` — the level's spawn table behind the one method
/// [`PlayerCore`] calls (`crate::player::system::SpawnSource`, seam 4).
///
/// `crate::scene::level::Level` cannot implement the trait itself without
/// `level.rs` naming `player::system`, which would point the level at the
/// player. The adapter belongs on the composition side, which is here.
pub struct LevelSpawns {
    table: Vec<Spawn>,
}

impl LevelSpawns {
    /// The level's spawn points, already in world space
    /// (`crate::scene::level::build_level` runs them through `A.toWorld`).
    pub fn from_level(level: &Level) -> Self {
        LevelSpawns {
            table: level
                .spawns
                .iter()
                .map(|s| Spawn {
                    position: s.position,
                    yaw: s.yaw,
                })
                .collect(),
        }
    }
}

impl SpawnSource for LevelSpawns {
    /// `world.spawn(i)` (`world/index.js:409-412`), with the source's
    /// wrap-around index arithmetic — the same one
    /// [`crate::scene::level::Level::spawn`] performs, minus its panic on an
    /// empty table (the trait says `Option`, so an empty table is `None`).
    fn spawn(&self, index: i64) -> Option<Spawn> {
        let n = self.table.len() as i64;
        (n > 0).then(|| self.table[(((index % n) + n) % n) as usize])
    }
}

/// The two subsystems, built and steppable.
///
/// Hold one of these in [`crate::scene::game::Game`] and call the three
/// methods from the three places named in the module doc's table.
pub struct PhysicsPlayer {
    physics: Rc<RefCell<PhysicsCore>>,
    player: Rc<RefCell<PlayerCore>>,
    /// The BVH probe the player's character controller and ledge probes run
    /// against — the same `Rc<StaticWorld>` the physics core holds.
    probe: Rc<PhysicsWorld>,
    events: EventBus,
    offs: Vec<(&'static str, SubscriptionId)>,
    /// `index.js:169`'s `addCollider` handle, mirrored from
    /// `PlayerCore::hitbox` every frame — see [`PhysicsPlayer::mirror_hitbox`].
    hitbox: u32,
    /// The last `update`'s rigid-body render poses (`index.js:915-932`). A
    /// renderer that spawns nodes for debris reads these; nothing does yet.
    poses: Vec<InterpolatedPose>,
}

impl PhysicsPlayer {
    /// Build both subsystems over the level.
    ///
    /// `root` is the engine's root random stream, and this **draws two forks
    /// from it**, physics then player — `main.js`'s `add` order, which is the
    /// order `Registry::resolve` would init them in and therefore the order the
    /// source forks in. Where the call sits relative to the level's forks and
    /// the HUD's fork decides every later stream: the source's order is
    /// `world` (the level) -> `physics` -> `player` -> ... -> `ui` (the HUD),
    /// so this belongs **after** `build_level` and **before** `Hud::new`.
    ///
    /// `collision_batches` is `stats.objects`: how many batches the level
    /// registered into its `StaticWorld`. `Level` does not expose the count and
    /// `StaticWorld` has no object accessor (see
    /// [`crate::physics::system::StaticRegistry`]), so it is a parameter rather
    /// than a guess. Nothing but the debug stat reads it.
    ///
    /// `time` should be the clock `Game` starts with (`fixed = FIXED_DT`,
    /// `scale = 1.0`); `PlayerCore` caches it and `fixed_update`/`update`
    /// refresh it every frame.
    pub fn new(
        level: &Level,
        collision_batches: usize,
        config: Config,
        time: Time,
        events: &EventBus,
        root: &mut Rng,
    ) -> Self {
        // The level's BVH, adopted rather than rebuilt.
        let probe = Rc::new(PhysicsWorld::new(Rc::clone(&level.world)));

        // ---- physics -------------------------------------------------------
        // `init(ctx)`: `this.rng = ctx.rng.fork()`, `this.ctx = ctx`.
        let mut core = PhysicsCore::with_static_world(Rc::clone(&level.world), collision_batches);
        core.set_rng(root.fork());
        core.set_events(events.clone());
        // `index.js:169`'s player capsule, on LAYER.PLAYER so the player's own
        // muzzle rays and movement sweeps (MASK.BULLET / MASK.CHARACTER, which
        // both omit PLAYER) cannot see it.
        let hitbox = core.add_collider(ColliderOpts {
            shape: ColliderShape::Capsule,
            layer: layer::PLAYER,
            surface: Surface::Flesh,
            part: Some("torso".to_string()),
            radius: 0.3,
            ..ColliderOpts::default()
        });
        let physics = Rc::new(RefCell::new(core));

        // ---- player --------------------------------------------------------
        let player = Rc::new(RefCell::new(PlayerCore::new(config)));
        player.borrow_mut().init(
            Rc::clone(&probe) as Rc<dyn PlayerPhysics>,
            Some(Rc::new(LevelSpawns::from_level(level)) as Rc<dyn SpawnSource>),
            root.fork(),
            events.clone(),
            config,
            time,
        );
        // `ctx.peek('render')` answers nothing in this port, so the low-health
        // pass is not installed — the source's own `if` takes the same arm.

        // ---- subscriptions --------------------------------------------------
        // Physics first, so an `explosion` reaches the impulse solver before it
        // reaches the player's damage handler. That is registry order.
        let mut offs = physics_system::subscribe(&physics, events);
        offs.extend(player_system::subscribe(&player, events));

        let mut wired = PhysicsPlayer {
            physics,
            player,
            probe,
            events: events.clone(),
            offs,
            hitbox,
            poses: Vec::new(),
        };
        wired.mirror_hitbox();
        wired
    }

    /// One fixed step, physics then player. Call from inside `Game::frame`'s
    /// `while accumulator >= FIXED_DT` loop.
    ///
    /// The step handed to physics is `time.fixed`, which is what `engine.js`
    /// passes every `fixedUpdate` — not the frame delta.
    pub fn fixed_update(&mut self, time: &Time, config: &Config, input: &dyn PlayerLook) {
        self.physics.borrow_mut().fixed_update(time.fixed);
        self.player
            .borrow_mut()
            .fixed_update(time, config, input);
    }

    /// The rendered update, physics then player. Call once per frame, **after**
    /// `time.alpha` has been set from the leftover accumulator — physics
    /// interpolates its rigid bodies with it.
    ///
    /// `PlayerCore::update` is given `time.dt`, the `f64`, deliberately: the
    /// `Seconds` the `Subsystem` phase carries is an `f32`, and a narrowed `dt`
    /// changes every spring in the camera rig.
    pub fn update(&mut self, time: &Time, config: &Config, input: &dyn PlayerLook) {
        self.poses = self.physics.borrow_mut().update(time.alpha);
        self.player
            .borrow_mut()
            .update(time.dt, time, config, input);
        self.mirror_hitbox();
    }

    /// `lateUpdate(dt, ctx)` — physics only; `player` declares no `LateUpdate`
    /// phase. `camera` is the eye position the debug view culls its wireframe
    /// around; [`PhysicsPlayer::pose`] is the value to pass.
    pub fn late_update(&mut self, dt: f64, camera: Option<[f64; 3]>) {
        self.physics.borrow_mut().late_update(dt, camera);
    }

    /// Copy `PlayerCore::hitbox` onto the registered collider — `_syncHitbox`'s
    /// `setSegment` half (`index.js:294-301`).
    ///
    /// The player owns the capsule as a value (seam 5: nothing held a collider
    /// registry when `player/system.rs` was written) and physics owns the
    /// registry. Neither can reach the other without one of them importing the
    /// other's facade, so the copy lands here, at the tier that holds both.
    /// Without it the player is a ghost: `fire_bullet` and every AI trace pass
    /// straight through.
    fn mirror_hitbox(&mut self) {
        let hit = self.player.borrow().hitbox;
        let Some(hit) = hit else {
            return;
        };
        let mut physics = self.physics.borrow_mut();
        let Some(collider) = physics.collider_mut(self.hitbox) else {
            return;
        };
        collider.set_segment(
            hit.ax,
            hit.ay,
            hit.az,
            hit.bx,
            hit.by,
            hit.bz,
            Some(hit.radius),
        );
        collider.layer = hit.layer;
        collider.surface = hit.surface;
        collider.enabled = hit.enabled;
    }

    /// The camera pose the player resolved this frame — the same three values
    /// [`crate::scene::game::Game::pose`] reads off its own rig, so a `Game`
    /// that has handed its player over to this seam can return this unchanged.
    pub fn pose(&self) -> CameraPose {
        let player = self.player.borrow();
        let rig = player.camera_rig();
        CameraPose {
            eye: rig.eye_position,
            rotation: rig.rotation,
            fov_degrees: rig.fov,
        }
    }

    /// `setControlEnabled(on)` (`index.js:625-637`) — what the pause menu
    /// drives. It does more than flip a flag: it flushes the latched input,
    /// zeroes the velocity, cancels a mantle and drops the ADS blend.
    pub fn set_control_enabled(&mut self, on: bool) {
        self.player.borrow_mut().set_control_enabled(on);
    }

    /// The player facade — `ctx.get('player')`.
    pub fn player(&self) -> Rc<RefCell<PlayerCore>> {
        Rc::clone(&self.player)
    }

    /// The physics facade — `ctx.get('physics')`. Queries, ballistics, rigid
    /// bodies, ragdolls and colliders all live behind it.
    pub fn physics(&self) -> Rc<RefCell<PhysicsCore>> {
        Rc::clone(&self.physics)
    }

    /// The shared BVH probe, for a caller that wants a raycast without
    /// borrowing the whole core.
    pub fn probe(&self) -> Rc<PhysicsWorld> {
        Rc::clone(&self.probe)
    }

    /// The registered player capsule's collider id, so a caller can recognise
    /// its own hitbox in a [`crate::physics::system::Hit`].
    pub fn hitbox_collider(&self) -> u32 {
        self.hitbox
    }

    /// Last frame's interpolated rigid-body poses (`index.js:915-932`).
    pub fn interpolated_poses(&self) -> &[InterpolatedPose] {
        &self.poses
    }

    /// `_syncStats()`'s output — triangles, nodes, bodies, ragdolls, raycasts.
    pub fn physics_stats(&self) -> PhysicsStats {
        self.physics.borrow().stats
    }

    /// `dispose()` for both, in reverse dependency order (player, then
    /// physics), with every subscription actually cancelled.
    pub fn dispose(&mut self) {
        for (name, id) in self.offs.drain(..) {
            self.events.off(name, id);
        }
        self.player.borrow_mut().dispose();
        self.physics.borrow_mut().dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FIXED_DT, MAX_SUBSTEPS};
    use crate::engine::CAPTURE_SEED;
    use crate::input::Input;
    use crate::physics::system::{add_fallback_ground, StaticRegistry};
    use crate::physics::bvh::StaticWorld;
    use crate::player::system::ExplosionEvent;
    use crate::scene::level::build_level;

    /// A stand-in for `Game::frame` — the same clock arithmetic and the same
    /// three call sites, so the ordering this file documents is the ordering
    /// the tests actually exercise.
    struct Driver {
        systems: PhysicsPlayer,
        config: Config,
        time: Time,
        accumulator: f64,
        input: Input,
        events: EventBus,
    }

    impl Driver {
        fn new() -> Self {
            let config = Config::default();
            let mut root = Rng::new(CAPTURE_SEED);
            let level = build_level(&mut root);
            let events = EventBus::new();
            let mut time = Time::default();
            time.fixed = FIXED_DT;
            time.scale = 1.0;
            let systems = PhysicsPlayer::new(&level, 0, config, time, &events, &mut root);
            Driver {
                systems,
                config,
                time,
                accumulator: 0.0,
                input: Input::new(),
                events,
            }
        }

        fn frame(&mut self, dt: f64) {
            self.time.frame += 1;
            self.time.raw += dt;
            self.time.dt = dt * self.time.scale;
            self.time.elapsed += self.time.dt;
            self.time.fixed = FIXED_DT;
            self.input.begin_frame(&self.config, None);

            self.accumulator += self.time.dt;
            let mut steps = 0u32;
            while self.accumulator >= FIXED_DT && steps < MAX_SUBSTEPS {
                self.systems
                    .fixed_update(&self.time, &self.config, &self.input);
                self.accumulator -= FIXED_DT;
                steps += 1;
            }
            self.time.alpha = self.accumulator / FIXED_DT;

            self.systems.update(&self.time, &self.config, &self.input);
            let eye = self.systems.pose().eye;
            self.systems.late_update(self.time.dt, Some(eye));
        }

        fn run(&mut self, frames: usize) {
            for _ in 0..frames {
                self.frame(1.0 / 60.0);
            }
        }
    }

    #[test]
    fn a_registry_that_registered_geometry_gets_no_fallback_ground_plane() {
        // The regression: `PhysicsCore::new` used to lay a 600 m concrete plane
        // at y = 0 under EVERY world, populated or not. `_ensureStatics`
        // returns at its first line once `_explicitStatics > 0`
        // (`index.js:330`), and the plane is the last-resort arm
        // (`index.js:364`) — never a default.
        let mut registry = StaticRegistry::new();
        registry.add_triangles(
            &[0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
            1,
            Surface::Concrete,
            layer::STATIC,
            "test:floor",
        );
        let core = PhysicsCore::new(registry);
        assert_eq!(core.fallback_id(), -1, "a populated world got a ground plane");
        assert_eq!(core.triangle_count(), 1, "the plane's two triangles snuck in");
    }

    #[test]
    fn an_empty_registry_still_gets_the_last_resort_ground() {
        let core = PhysicsCore::new(StaticRegistry::new());
        assert!(core.fallback_id() >= 0, "an empty world has nothing to stand on");
        assert_eq!(core.triangle_count(), 2);
    }

    #[test]
    fn adopting_a_built_world_shares_it_rather_than_rebuilding() {
        let mut soup = StaticWorld::new();
        add_fallback_ground(&mut soup);
        soup.build();
        let shared = Rc::new(soup);
        let core = PhysicsCore::with_static_world(Rc::clone(&shared), 1);
        assert_eq!(core.fallback_id(), -1, "the builder's world is not re-grounded");
        assert!(
            Rc::ptr_eq(&core.static_world(), &shared),
            "the core built a second BVH instead of adopting the level's"
        );
        assert_eq!(core.stats.objects, 1);
    }

    #[test]
    fn the_wired_player_stands_on_the_level_and_does_not_fall_through() {
        let mut d = Driver::new();
        let y0 = d.systems.player().borrow().feet_position()[1];
        d.run(240);
        let player = d.systems.player();
        let player = player.borrow();
        assert!(player.grounded(), "the player is not on the ground");
        assert!(
            (player.feet_position()[1] - y0).abs() < 0.2,
            "drifted from {y0} to {}",
            player.feet_position()[1]
        );
        // The eye rides above the feet, and the pose reports it.
        let pose = d.systems.pose();
        assert!(pose.eye[1] > player.feet_position()[1] + 1.0);
    }

    #[test]
    fn holding_forward_walks_the_wired_player() {
        let mut d = Driver::new();
        d.run(10);
        let start = d.systems.player().borrow().feet_position();
        d.input.key_down("KeyW");
        d.run(120);
        let end = d.systems.player().borrow().feet_position();
        let travelled = (end[0] - start[0]).hypot(end[2] - start[2]);
        assert!(travelled > 3.0, "walked only {travelled} m in two seconds");
    }

    #[test]
    fn the_player_capsule_is_registered_with_physics_and_follows_the_player() {
        // Seam 5, closed at the composition tier: without this the player is a
        // ghost that no bullet and no AI trace can touch.
        let mut d = Driver::new();
        d.run(10);
        let id = d.systems.hitbox_collider();
        let before = {
            let physics = d.systems.physics();
            let physics = physics.borrow();
            let c = physics.collider(id).expect("the hitbox is registered");
            assert_eq!(c.layer, layer::PLAYER);
            assert!(c.enabled);
            [c.ax, c.ay, c.az]
        };
        d.input.key_down("KeyW");
        d.run(120);
        let after = {
            let physics = d.systems.physics();
            let physics = physics.borrow();
            let c = physics.collider(id).expect("the hitbox is registered");
            [c.ax, c.ay, c.az]
        };
        assert_ne!(before, after, "the hitbox stayed behind when the player moved");
    }

    #[test]
    fn an_explosion_on_the_bus_reaches_both_subsystems_and_stops_at_dispose() {
        let mut d = Driver::new();
        d.run(10);
        assert_eq!(d.events.handler_count("explosion"), 2, "physics and player");
        let health_before = d.systems.player().borrow().health_fraction();
        let eye = d.systems.pose().eye;
        d.events.emit(
            "explosion",
            &ExplosionEvent {
                position: eye,
                radius: Some(6.0),
                damage: Some(80.0),
            },
        );
        assert!(
            d.systems.player().borrow().health_fraction() < health_before,
            "the player took no damage from a blast at its own eye"
        );

        // `dispose` must CANCEL the subscriptions, not merely forget their ids.
        d.systems.dispose();
        assert_eq!(d.events.handler_count("explosion"), 0);
        assert_eq!(d.events.handler_count("damage:dealt"), 0);
        assert_eq!(d.events.handler_count("bullet:impact"), 0);
    }
}
