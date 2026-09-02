//! **The firing machine, wired.** The composition step that turns
//! [`crate::weapons::system`] — a complete port of `weapons/index.js:1-843`
//! that nothing constructed — into a gun the running scene pulls a trigger on.
//!
//! This is not a port of a source file. `weapons/index.js` reads its world
//! through five `ctx` seams, and `system.rs` names all five as traits because
//! none of the concrete types existed when it landed. This file binds the four
//! that [`crate::scene::game::Game`] can honestly satisfy, and takes the fifth
//! as an explicit parameter because `Game` does not hold it:
//!
//! | seam (`system.rs`)  | bound to |
//! |---------------------|----------|
//! | [`WeaponInput`]     | [`crate::input::Input`] — `system.rs` already implements it |
//! | [`FireCamera`]      | [`PoseCamera`], built from the frame's resolved [`CameraPose`] |
//! | [`WeaponPlayer`]    | [`PlayerLink`] over [`CameraRig`] + [`Movement`] |
//! | `ctx.events`        | [`crate::scene::game::Game::events`], the bus the whole scene shares |
//! | [`WeaponPhysics`]   | **nothing** — an explicit `Option` parameter, `None` today. See below. |
//!
//! ## The physics seam is open, and why
//!
//! [`WeaponPhysics`] is `spawn_debris` + `remove_rigid_body` on top of
//! [`RaycastWorld`][crate::weapons::ballistics::RaycastWorld], and all four
//! methods exist on [`crate::physics::system::PhysicsCore`]: `raycast`,
//! `fire_bullet` (over the ported multi-layer solver in
//! [`crate::physics::penetration`]), `spawn_debris` and `remove_rigid_body`.
//! The claim at `physics/probe.rs:27-30` that `fire_bullet` "needs the
//! penetration solver (`src/physics/penetration.js`, not ported)" is **stale**:
//! `penetration.rs` is 400-odd lines of ported solver and
//! `PhysicsCore::fire_bullet` already drives it.
//!
//! What actually blocks the binding is a signature mismatch:
//! `RaycastWorld::raycast` takes `&self`, and every real physics facade in this
//! port needs `&mut self` (`PhysicsCore::raycast` counts rays). An adapter would
//! have to launder that through a `RefCell`, which is the kind of shortcut this
//! repository does not take. The structural fix is to widen
//! `RaycastWorld::raycast` to `&mut self` — every caller in
//! `ballistics.rs` already holds `&mut dyn RaycastWorld`, so the change is
//! free there, and the one test implementation
//! (`apps/shmup/tests/weapons_port.rs:293`) moves with it.
//!
//! Until then, `physics: None` is not a fudge — it is precisely the source's
//! own `if (phys)` absent-facade path: rounds still fly and expire, dropped
//! magazines still pool and retire on the shorter two-second timer, and no
//! behaviour is invented.
//!
//! ## What the rest of the frame reads
//!
//! [`WeaponsRig::frame`] returns a [`WeaponsFrame`] — the trigger, the shots
//! that left the barrel, the brass that left the port, the ADS curve and the
//! ammunition. The **events** go where the source puts them, on the shared
//! [`EventBus`]: `weapon:fire`, `weapon:shell` and `weapon:reload` out of
//! [`WeaponCore`], `bullet:tracer` out of
//! [`crate::weapons::ballistics::ProjectileSim`]. `crate::fx` should subscribe
//! to those three rather than read this struct — with the caveat
//! `system.rs`'s module doc records: the shared payload structs
//! ([`crate::audio::system::WeaponFire`] / `WeaponShell`) carry neither the
//! shot direction and seed nor the shell velocity, case dimensions and spin.
//! Those are computed, in the source's order and off the source's RNG draws,
//! and surface here as [`WeaponsFrame::fire`] / [`WeaponsFrame::shell`]. A
//! consumer that needs them per-shot rather than per-frame needs that
//! vocabulary converged, which is the integration pass's job.
//!
//! ## The viewmodel moved here
//!
//! [`WeaponCore`] **owns** a [`crate::weapons::viewmodel::Viewmodel`] and drives
//! it from real input in `late_update` — with recoil, bolt hold, clip playback
//! and the reload choreography, none of which a standalone rig receives. Any
//! second `Viewmodel` in the scene is now a rig that cannot recoil, rendered in
//! place of one that can; [`WeaponsRig::rig_pose`] is the pose the renderer
//! should hang the gun off.

use crate::engine::Time;
use crate::events::EventBus;
use crate::player::camera::CameraRig;
use crate::player::movement::{Movement, MovementState};
use crate::rng::Rng;
use crate::scene::game::CameraPose;
use crate::ui::WeaponPull;
use crate::weapons::rig_math::{M4, Q, V3};
use crate::weapons::system::{
    Ammo, FireCamera, FirePayload, ShellPayload, Stance, WeaponCore, WeaponInput, WeaponPhysics,
    WeaponPlayer,
};
use crate::weapons::viewmodel::ViewCamera;

/* ==================================================================== */
/* ctx.camera                                                           */
/* ==================================================================== */

/// `ctx.camera`, built from the [`CameraPose`] the frame resolved.
///
/// Two orientations, deliberately, because [`FireCamera`] asks for two and its
/// doc explains that conflating them "compiles and is wrong in the last bits of
/// every shot direction":
///
/// * [`FireCamera::aim_orientation`] is `cam.quaternion` — the camera's own
///   rotation, composed from the pose's Euler angles in **`YXZ`** order, which
///   is the order the source overrides Three's default to (`engine.js:30`) and
///   the order [`crate::scene::app::write_camera`] composes the rendered camera
///   in. Building it any other way aims the gun somewhere the player is not
///   looking.
/// * [`ViewCamera::orientation`] is the viewmodel *anchor*'s quaternion, which
///   the source derives as
///   `anchor.quaternion.setFromRotationMatrix(cam.matrixWorld)`
///   (`viewmodel.js:641`) — out through the world matrix and back through the
///   trace method. That round trip is reproduced here rather than short-cut,
///   because it does not return the camera's own quaternion bit-for-bit and the
///   difference compounds through the rig's spring chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoseCamera {
    position: V3,
    /// `cam.quaternion`.
    aim: Q,
    /// `anchor.quaternion`, after the `matrixWorld` round trip.
    anchor: Q,
}

impl PoseCamera {
    pub fn new(pose: CameraPose) -> Self {
        let position = V3::new(pose.eye[0], pose.eye[1], pose.eye[2]);
        let aim = Q::from_euler_yxz(pose.rotation.pitch, pose.rotation.yaw, pose.rotation.roll);
        // `cam.matrixWorld` is `compose(position, quaternion, 1)`; the anchor
        // reads its rotation back off the three basis columns.
        let world = M4::compose(position, aim, V3::new(1.0, 1.0, 1.0));
        let e = world.e;
        let anchor = Q::from_basis(
            V3::new(e[0], e[1], e[2]),
            V3::new(e[4], e[5], e[6]),
            V3::new(e[8], e[9], e[10]),
        );
        PoseCamera {
            position,
            aim,
            anchor,
        }
    }
}

impl ViewCamera for PoseCamera {
    fn orientation(&self) -> Q {
        self.anchor
    }
}

impl FireCamera for PoseCamera {
    fn position(&self) -> V3 {
        self.position
    }

    fn aim_orientation(&self) -> Q {
        self.aim
    }

    fn as_view_camera(&self) -> &dyn ViewCamera {
        self
    }
}

/* ==================================================================== */
/* ctx.peek('player')                                                   */
/* ==================================================================== */

/// `ctx.peek('player')`, bound to the two pieces of [`crate::scene::game::Game`]
/// that hold what `weapons/index.js` reads off `PlayerSystem`.
///
/// It borrows rather than copies because one of the ten members is an *output*:
/// `addRecoil` is the camera climb, and it lands on the real
/// [`CameraRig`] the frame is about to draw through. The other output,
/// `setAdsProgress`, is captured in [`PlayerLink::ads_progress`] rather than
/// applied — see its doc.
pub struct PlayerLink<'a> {
    rig: &'a mut CameraRig,
    movement: &'a Movement,
    ads_requested: bool,
    ads_progress: f64,
}

impl<'a> PlayerLink<'a> {
    pub fn new(rig: &'a mut CameraRig, movement: &'a Movement, ads_requested: bool) -> Self {
        PlayerLink {
            rig,
            movement,
            ads_requested,
            ads_progress: 0.0,
        }
    }

    /// The last value `weapons` pushed through `player.setAdsProgress(t)`.
    ///
    /// The source feeds it into `_updateAds`'s `_adsExternal` arm so the
    /// camera FOV and move speed follow the *weapon's* ADS curve rather than
    /// the raw button. `Game::update_ads` does not port that arm (its own
    /// comment says so), so this value is recorded and not applied. Wiring it
    /// is a change to `player/` and to that method, not to this file.
    pub fn ads_progress(&self) -> f64 {
        self.ads_progress
    }
}

impl WeaponPlayer for PlayerLink<'_> {
    fn add_recoil(&mut self, pitch: f64, yaw: f64, roll: f64, punch: f64) {
        self.rig.add_recoil(pitch, yaw, roll, punch);
    }

    fn velocity(&self) -> Option<V3> {
        let v = self.movement.velocity;
        Some(V3::new(v[0], v[1], v[2]))
    }

    fn ads_requested(&self) -> bool {
        self.ads_requested
    }

    fn sprinting(&self) -> bool {
        self.movement.sprinting
    }

    fn horizontal_speed(&self) -> f64 {
        self.movement.horizontal_speed
    }

    fn stance(&self) -> Option<Stance> {
        Some(match self.movement.stance {
            crate::player::tuning::Stance::Stand => Stance::Stand,
            crate::player::tuning::Stance::Crouch => Stance::Crouch,
            crate::player::tuning::Stance::Prone => Stance::Prone,
        })
    }

    fn airborne(&self) -> bool {
        !self.movement.grounded
    }

    /// `player?.state === 'mantle'`. The port's state machine spells the two
    /// ledge moves apart (`Mantle` and `Vault`); the source's string compare
    /// only ever matches the first, and `mantling` below is what catches both.
    fn state_is_mantle(&self) -> bool {
        self.movement.state == MovementState::Mantle
    }

    fn mantling(&self) -> bool {
        self.movement.mantle_motion.active
    }

    fn set_ads_progress(&mut self, t: f64) {
        self.ads_progress = t;
    }
}

/* ==================================================================== */
/* The frame's outputs                                                  */
/* ==================================================================== */

/// What one frame of the firing machine produced.
///
/// Everything here is a *reading*, not a channel: the machine's real outputs
/// are the four events on the shared [`EventBus`]. These are the facts the
/// frame itself needs — the viewmodel's trigger, the HUD's ammunition, the
/// crosshair's bloom — plus the two rich payloads the shared event vocabulary
/// cannot yet carry (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponsFrame {
    /// `this._state.trigger` — the fire input is down *and* the weapon could
    /// answer it. This is the bit the viewmodel rig reads, and it is what
    /// `scene::app::drive_viewmodel` used to hardcode to `false`.
    pub trigger: bool,
    /// Rounds that left the barrel this frame. `>= 1` on any frame a shot
    /// happened; a burst at a low frame rate can be more than one.
    pub shots: u32,
    /// The full `weapon:fire` payload of the **last** shot this frame — origin,
    /// direction and seed. `None` on a frame nothing fired.
    pub fire: Option<FirePayload>,
    /// Cases that left the ejection port this frame (deferred ~50 ms behind the
    /// shot that produced them, as the source defers them).
    pub shells: u32,
    /// The full `weapon:shell` payload of the last case — position, velocity,
    /// case length and radius, spin. `None` on a frame nothing ejected.
    pub shell: Option<ShellPayload>,
    /// `wp.adsProgress` — the rig's own 0..1 ADS curve, which is the value
    /// `player.setAdsProgress` was handed.
    pub ads_progress: f64,
    /// `wp.spreadDegrees` — the live cone half-angle.
    pub spread_degrees: f64,
    /// `wp.firing` — a shot within the last 0.12 s, which is what drives the
    /// muzzle-flash decay rather than the instantaneous `shots`.
    pub firing: bool,
    /// `wp.ammo`.
    pub ammo: Ammo,
    pub reloading: bool,
    /// Rounds currently in flight, for a tracer renderer that wants to know
    /// whether there is anything to draw.
    pub bullets_live: u32,
}

impl Default for WeaponsFrame {
    fn default() -> Self {
        WeaponsFrame {
            trigger: false,
            shots: 0,
            fire: None,
            shells: 0,
            shell: None,
            ads_progress: 0.0,
            spread_degrees: 0.0,
            firing: false,
            ammo: Ammo::default(),
            reloading: false,
            bullets_live: 0,
        }
    }
}

/* ==================================================================== */
/* The rig                                                              */
/* ==================================================================== */

/// The constructed firing machine, and the frame ordering it needs.
///
/// The source's `WeaponSystem` implements three engine phases —
/// `fixedUpdate`, `update` and `lateUpdate` — and
/// [`crate::weapons::system::WeaponSystem`] deliberately declares **no** phases
/// (`phases()` returns `&[]`) because all three need the camera/player/physics
/// seams a `Ctx` cannot supply. So the host drives them, in order:
///
/// 1. [`WeaponsRig::fixed_step`] — once per fixed substep, integrating rounds
///    in flight at a fixed rate. Calling it once per *frame* instead would
///    integrate ballistics at a variable rate and lose determinism.
/// 2. [`WeaponsRig::frame`] — `update` (input, the fire-mode machine, the
///    shot), then `lateUpdate` (the rig pose, the clip beats, the deferred
///    brass), then the renderer's anchor walk.
///
/// [`WeaponsRig::on_land`] and [`WeaponsRig::on_jump`] stand in for the two
/// subscriptions `WeaponSystem::wire_events` makes: nothing in this scene
/// emits `player:land` / `player:jump` on the bus yet (the movement machine
/// surfaces them as flags, which `Game::drain_movement_events` drains), so the
/// beats are handed over directly at the site that already drains them.
pub struct WeaponsRig {
    core: WeaponCore,
}

impl WeaponsRig {
    /// `new WeaponSystem()`. Takes the root stream so the two forks the source
    /// makes off it — the facade's own, then the viewmodel's — land in the
    /// source's order; see [`WeaponCore::new`].
    pub fn new(rng: &mut Rng) -> Self {
        WeaponsRig {
            core: WeaponCore::new(rng),
        }
    }

    /// `init(ctx)` — build all three weapons, their clips and their recoil
    /// patterns, and hold the bus every event is emitted on.
    pub fn init(&mut self, events: EventBus, time: Time) {
        self.core.init(events, time);
    }

    /// `fixedUpdate(h)`. Once per fixed substep, before or after the movement
    /// step — the projectile integrator shares no state with it.
    pub fn fixed_step(&mut self, h: f64, physics: Option<&mut (dyn WeaponPhysics + '_)>) {
        self.core.fixed_update(h, physics);
    }

    /// `update(dt)` + `lateUpdate(dt)` + the renderer's scene-graph walk, in
    /// the order the source's engine runs them.
    ///
    /// `pose` must be the frame's **resolved** camera — after the camera rig
    /// has run, because `tryFire` builds the aim basis from it.
    pub fn frame(
        &mut self,
        dt: f64,
        time: Time,
        input: &dyn WeaponInput,
        pose: CameraPose,
        mut player: Option<&mut (dyn WeaponPlayer + '_)>,
        mut physics: Option<&mut (dyn WeaponPhysics + '_)>,
    ) -> WeaponsFrame {
        let camera = PoseCamera::new(pose);

        self.core.update(
            dt,
            time,
            input,
            &camera,
            player.as_deref_mut(),
            physics.as_deref_mut(),
        );

        // `pending_shots` is accumulated by `tryFire` during `update` and
        // drained by `lateUpdate`, and the brass queue is armed during
        // `update` and released by `lateUpdate`. Both are read across that
        // boundary rather than reconstructed, so a frame that fired twice
        // reports two.
        let shots = self.core.pending_shots();
        let armed_before = self.armed_shells();

        self.core
            .late_update(dt, time, &camera, player.as_deref_mut(), physics);

        let shells = armed_before.saturating_sub(self.armed_shells());

        // The renderer's walk over `ctx.viewScene`, which is the only thing
        // that composes the viewmodel anchor's world matrix. It runs last, so
        // the one-frame anchor lag the source has is the lag this has.
        self.core.sync_anchor(&camera);

        WeaponsFrame {
            trigger: self.core.frame_state().trigger,
            shots,
            fire: (shots > 0).then(|| self.core.fire_payload()),
            shells: shells as u32,
            shell: (shells > 0).then(|| self.core.shell_payload()),
            ads_progress: self.core.ads_progress(),
            spread_degrees: self.core.spread_degrees(),
            firing: self.core.firing(),
            ammo: self.core.ammo(),
            reloading: self.core.reloading(),
            bullets_live: self.core.stats.live,
        }
    }

    fn armed_shells(&self) -> usize {
        self.core.shell_queue().iter().filter(|s| s.t >= 0.0).count()
    }

    /// The `player:land` subscription (`index.js:174-177`) — the landing jolt
    /// the rig absorbs. `speed` is the source's `Math.abs(e.velocity)`.
    pub fn on_land(&mut self, speed: f64) {
        self.core.viewmodel.land(speed.abs());
    }

    /// The `player:jump` subscription (`index.js:178-179`).
    pub fn on_jump(&mut self) {
        self.core.viewmodel.jump();
    }

    /// The rig's transform in **view-model space** — a child of the camera
    /// anchor, exactly as `viewmodel.js` composes it. The renderer's world
    /// transform is the camera's own composed with this.
    pub fn rig_pose(&self) -> (V3, Q) {
        self.core.viewmodel.rig_pose()
    }

    /// `weapons.getHudState()` as the shape [`crate::ui::FramePull`] wants, so
    /// the HUD's ammunition counter, fire-mode label, reload bar and reticle
    /// bloom read the real weapon instead of the empty defaults.
    pub fn hud_pull(&mut self) -> WeaponPull {
        let h = self.core.hud_state();
        WeaponPull {
            name: Some(h.name.clone()),
            mode: Some(h.mode.to_string()),
            ammo: Some(i64::from(h.ammo)),
            reserve: Some(i64::from(h.reserve)),
            mag_size: Some(i64::from(h.mag_size)),
            reloading: Some(h.reloading),
            reload_progress: Some(h.reload_progress),
            ads: Some(h.ads),
            spread: Some(h.spread),
            // `lethal`/`tactical` are the grenade counts, which come from a
            // different subsystem the source's HUD polls; `weapons` has never
            // supplied them.
            lethal_count: None,
            tactical_count: None,
        }
    }

    /// The machine itself, for a caller that needs the full facade —
    /// `set_weapon`, `cycle_fire_mode`, `debug_pose`, the projectile pool.
    pub fn core(&self) -> &WeaponCore {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut WeaponCore {
        &mut self.core
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::engine::CAPTURE_SEED;
    use crate::input::Input;
    use crate::player::camera::Euler;

    fn pose(pitch: f64, yaw: f64, roll: f64) -> CameraPose {
        CameraPose {
            eye: [1.0, 1.7, -2.0],
            rotation: Euler { pitch, yaw, roll },
            fov_degrees: 80.0,
        }
    }

    fn rig() -> WeaponsRig {
        let mut root = Rng::new(CAPTURE_SEED);
        let mut rig = WeaponsRig::new(&mut root);
        rig.init(EventBus::new(), Time::default());
        rig
    }

    /// One host, driving the rig the way `Game::frame` is asked to.
    ///
    /// `Input` latches every press at `begin_frame` (`mouse_down` only fills
    /// `pending_down`), so a harness that skips it never presses anything —
    /// the first draft of these tests did exactly that and fired zero rounds
    /// while looking like it held the trigger down.
    struct Host {
        rig: WeaponsRig,
        cam: CameraRig,
        movement: Movement,
        input: Input,
        config: Config,
        time: Time,
    }

    impl Host {
        fn new() -> Self {
            Host {
                rig: rig(),
                cam: CameraRig::new(80.0),
                movement: Movement::new(),
                input: Input::new(),
                config: Config::default(),
                time: Time {
                    dt: 1.0 / 60.0,
                    fixed: 1.0 / 60.0,
                    scale: 1.0,
                    ..Time::default()
                },
            }
        }

        fn step(&mut self, pose: CameraPose) -> WeaponsFrame {
            self.time.frame += 1;
            self.time.elapsed += self.time.dt;
            self.time.raw += self.time.dt;
            self.input.begin_frame(&self.config, None);
            self.rig.fixed_step(1.0 / 60.0, None);
            let mut link = PlayerLink::new(&mut self.cam, &self.movement, false);
            self.rig
                .frame(1.0 / 60.0, self.time, &self.input, pose, Some(&mut link), None)
        }
    }

    /// `Q::from_euler_yxz` is transcribed from Three's closed form; this pins
    /// it against the composition that form encodes, `qy * qx * qz`. If the
    /// transcription had a sign wrong, every shot would leave the barrel in
    /// the wrong direction the moment the player looked off dead-centre.
    #[test]
    fn the_yxz_euler_quaternion_is_yaw_times_pitch_times_roll() {
        // Hamilton product, spelled out rather than imported: `rig_math::Q`
        // has no multiply, which is part of why the closed form exists.
        let mul = |a: Q, b: Q| {
            Q::new(
                a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
                a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
                a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
                a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
            )
        };
        let axis = |x: f64, y: f64, z: f64, angle: f64| {
            let (s, c) = ((angle * 0.5).sin(), (angle * 0.5).cos());
            Q::new(x * s, y * s, z * s, c)
        };
        for &(pitch, yaw, roll) in &[
            (0.0, 0.0, 0.0),
            (0.3, -0.9, 0.05),
            (-0.7, 2.1, -0.2),
            (1.2, 0.4, 0.9),
        ] {
            let composed = mul(
                mul(axis(0.0, 1.0, 0.0, yaw), axis(1.0, 0.0, 0.0, pitch)),
                axis(0.0, 0.0, 1.0, roll),
            );
            let closed = Q::from_euler_yxz(pitch, yaw, roll);
            for (a, b) in [
                (composed.x, closed.x),
                (composed.y, closed.y),
                (composed.z, closed.z),
                (composed.w, closed.w),
            ] {
                assert!((a - b).abs() < 1e-15, "{a} vs {b} at {pitch},{yaw},{roll}");
            }
        }
        // And it round-trips through the decomposition already in `rig_math`.
        let e = Q::from_euler_yxz(0.3, -0.9, 0.05).to_euler_yxz();
        assert!((e.x - 0.3).abs() < 1e-12);
        assert!((e.y + 0.9).abs() < 1e-12);
        assert!((e.z - 0.05).abs() < 1e-12);
    }

    /// A pure yaw must not tilt the horizon — the same invariant
    /// `scene::app::combined_yaw_and_pitch_introduce_no_roll` pins for the
    /// rendered camera, checked here for the *aim* basis, because the two
    /// disagreeing means the gun shoots somewhere the player is not looking.
    #[test]
    fn the_aim_basis_matches_the_rendered_camera_basis() {
        for &(pitch, yaw) in &[(0.0, 0.7), (0.4, -1.3), (-0.6, 2.4)] {
            let cam = PoseCamera::new(pose(pitch, yaw, 0.0));
            let right = cam.aim_orientation().rotate(V3::new(1.0, 0.0, 0.0));
            assert!(right.y.abs() < 1e-12, "yaw={yaw} pitch={pitch} banked");
            let fwd = cam.aim_orientation().rotate(V3::new(0.0, 0.0, -1.0));
            assert!((fwd.x - (-yaw.sin() * pitch.cos())).abs() < 1e-12);
            assert!((fwd.y - pitch.sin()).abs() < 1e-12);
        }
    }

    /// The anchor orientation goes out through `matrixWorld` and back — the
    /// same rotation, and deliberately not the same expression, which is the
    /// distinction `FireCamera::aim_orientation` exists to preserve.
    #[test]
    fn the_anchor_orientation_is_the_matrix_round_trip_of_the_aim() {
        let cam = PoseCamera::new(pose(0.37, -1.11, 0.09));
        let a = cam.aim_orientation();
        let b = cam.orientation();
        for (x, y) in [(a.x, b.x), (a.y, b.y), (a.z, b.z), (a.w, b.w)] {
            assert!((x - y).abs() < 1e-12, "{x} vs {y}");
        }
        assert_eq!(b, PoseCamera::new(pose(0.37, -1.11, 0.09)).orientation());
    }

    #[test]
    fn a_fresh_rig_holds_a_loaded_rifle_and_has_not_fired() {
        let mut rig = rig();
        assert_eq!(rig.core().current().id, "rifle");
        let ammo = rig.core().ammo();
        assert!(ammo.mag > 0 && ammo.chambered && !ammo.empty);
        assert_eq!(
            rig.hud_pull().ammo,
            Some(i64::from(ammo.mag.min(ammo.mag_size)))
        );
        assert_eq!(rig.core().stats.fired, 0);
    }

    /// The whole point of the slice: holding the trigger fires rounds, the
    /// magazine drains, the camera climbs, and brass comes out.
    ///
    /// It does **not** assert the trigger bit goes true, and that is not an
    /// omission — in `auto` it provably cannot. `update` runs `run_trigger`
    /// *first* and only then evaluates `state.trigger = input.fire() &&
    /// can_fire()` (`system.rs:1666-1667`, `index.js:683-684`). In `auto`,
    /// `run_trigger` calls `try_fire` on every held frame (`index.js:701`), and
    /// `try_fire`'s guard is character-for-character `can_fire`'s
    /// (`system.rs:1277` vs `:1262-1266`), so by the time the bit is computed
    /// `can_fire()` is false down every path: the shot that just fired set
    /// `fire_timer = 60 / rpm` (`system.rs:1361`), a dry chamber set it to 0.25
    /// (`:1283`), and otherwise `try_fire` was blocked by the very condition
    /// `can_fire` is about to re-check. `input.fire()` false closes the last
    /// path. The bit is therefore identically false for a held auto trigger,
    /// in the port and in the source alike.
    ///
    /// That is the source's behaviour, so it is this port's behaviour. The bit
    /// is genuinely live in `semi` and `burst`, where `run_trigger` fires on the
    /// press *edge* and leaves the held frames for `can_fire` to answer true —
    /// which is what
    /// [`the_trigger_bit_is_live_in_semi_where_auto_can_never_show_it`] pins,
    /// and which is what keeps this suite honest about `drive_viewmodel` no
    /// longer hardcoding the bit to `false`.
    #[test]
    fn holding_the_trigger_drains_the_magazine_and_kicks_the_camera() {
        let mut host = Host::new();
        host.input.mouse_down(0);
        let start = host.rig.core().ammo().mag;

        let (shots, shells, triggered) = (0..120).fold((0u32, 0u32, false), |acc, _| {
            let out = host.step(pose(0.0, 0.0, 0.0));
            (acc.0 + out.shots, acc.1 + out.shells, acc.2 | out.trigger)
        });

        assert!(shots > 5, "only {shots} rounds left the barrel");
        assert!(shells > 0, "no brass was ejected");
        assert!(
            !triggered,
            "the trigger bit went true in auto — `run_trigger` must run before \
             `state.trigger` is computed, and `try_fire`'s guard must stay \
             identical to `can_fire`'s"
        );
        assert!(
            host.rig.core().ammo().mag < start,
            "the magazine never drained"
        );
        assert!(
            host.cam.recoil_pitch.value != 0.0 || host.cam.recoil_yaw.value != 0.0,
            "the camera never climbed"
        );
        assert!(host.rig.core().stats.fired >= shots);
    }

    /// The trigger bit reaches the viewmodel — it is not hardcoded `false`, and
    /// it is not dead just because
    /// [`holding_the_trigger_drains_the_magazine_and_kicks_the_camera`] cannot
    /// see it.
    ///
    /// `semi` is where it is observable. `run_trigger` fires only on the press
    /// EDGE there (`index.js:719`), so the held frames after the shot leave
    /// `try_fire` uncalled; `fire_timer` decays past zero (`system.rs:1599`),
    /// the round `try_fire` chambered on its way out (`:1296-1298`) keeps
    /// `chambered` true, and `can_fire()` answers true with the trigger still
    /// down — exactly the state `viewmodel.rs:918` turns into a pulled trigger
    /// finger.
    #[test]
    fn the_trigger_bit_is_live_in_semi_where_auto_can_never_show_it() {
        let mut host = Host::new();
        // The rifle's modes are ['auto', 'burst', 'semi'] (`defs.js:26`) and
        // `set_weapon` starts it on `modes[0]`, so cycle round to `semi`.
        let mode = (0..3).fold("auto", |m, _| match m {
            "semi" => m,
            _ => host.rig.core_mut().cycle_fire_mode(),
        });
        assert_eq!(mode, "semi", "the rifle should offer a semi-automatic mode");

        host.input.mouse_down(0);
        // 120 frames, the same budget the auto test uses — comfortably past the
        // draw animation, during which `switching()` holds `can_fire` false.
        let triggered = (0..120).fold(false, |acc, _| acc | host.step(pose(0.0, 0.0, 0.0)).trigger);

        assert!(
            triggered,
            "the trigger bit never went true in semi — it is not reaching the rig"
        );
    }

    /// A shot carries the two fields the shared event vocabulary drops. Were
    /// these zero, `fx` would have a muzzle flash with no direction.
    #[test]
    fn a_shot_reports_a_real_origin_direction_and_seed() {
        let mut host = Host::new();
        // Warm up first. `sync_anchor` is the renderer's scene walk and runs at
        // the END of a frame, so on frame one the viewmodel anchor is still the
        // identity and the muzzle sits at the world origin — the one-frame
        // anchor lag `system.rs`'s module doc spells out, faithfully preserved.
        // Asserting on frame one would be asserting on that artefact.
        (0..4).for_each(|_| {
            host.step(pose(0.0, 0.5, 0.0));
        });
        host.input.mouse_down(0);
        let fired = (0..8)
            .filter_map(|_| host.step(pose(0.0, 0.5, 0.0)).fire)
            .next()
            .expect("eight frames on a full magazine fire at least once");
        assert_eq!(fired.weapon, Some("rifle"));
        assert!(
            (fired.dir.length() - 1.0).abs() < 1e-9,
            "the bore direction is not unit: {}",
            fired.dir.length()
        );
        // The muzzle is out in the world near the eye, not at the origin.
        assert!(fired.origin.length() > 0.5, "{:?}", fired.origin);
    }

    /// Same inputs, same frames, same rounds — the port is seed-driven and the
    /// wiring must not introduce entropy of its own.
    #[test]
    fn the_firing_machine_is_deterministic_for_the_same_inputs() {
        let run = || {
            let mut host = Host::new();
            host.input.mouse_down(0);
            let trace: Vec<_> = (0..90)
                .map(|i| {
                    let out = host.step(pose(f64::from(i) * 0.001, 0.3, 0.0));
                    (out.shots, out.shells, out.fire.map(|f| f.seed))
                })
                .collect();
            (trace, host.rig.core().ammo(), host.rig.rig_pose())
        };
        let a = run();
        let b = run();
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
        assert_eq!(a.2, b.2);
    }

    /// The reload path end to end: fire the magazine dry, and the auto-reload
    /// on a dry trigger pull draws from reserve and emits its phases on the
    /// shared bus — which is how `audio` will hear it.
    #[test]
    fn firing_dry_auto_reloads_from_reserve_and_emits_its_phases() {
        use std::any::Any;
        use std::cell::RefCell;
        use std::rc::Rc;

        let events = EventBus::new();
        let phases: Rc<RefCell<Vec<String>>> = Rc::default();
        let seen = Rc::clone(&phases);
        let _sub = events.on("weapon:reload", move |p: &dyn Any| {
            if let Some(phase) = p
                .downcast_ref::<crate::audio::system::WeaponReload>()
                .and_then(|r| r.phase)
            {
                seen.borrow_mut().push(format!("{phase:?}"));
            }
            Ok(())
        });

        let mut host = Host::new();
        let mut root = Rng::new(CAPTURE_SEED);
        host.rig = WeaponsRig::new(&mut root);
        host.rig.init(events, Time::default());
        let reserve0 = host.rig.core().ammo().reserve;

        for i in 0..900 {
            // Re-press the trigger periodically: the dry-click auto-reload is
            // an EDGE (`input.fire_pressed()`), so a trigger held down from
            // frame one never produces one.
            if i % 8 == 0 {
                host.input.mouse_up(0);
            }
            if i % 8 == 2 {
                host.input.mouse_down(0);
            }
            host.step(pose(0.0, 0.0, 0.0));
        }

        assert!(
            host.rig.core().ammo().reserve < reserve0,
            "the reserve was never drawn on"
        );
        let seen = phases.borrow();
        assert!(seen.contains(&"Start".to_string()), "phases: {seen:?}");
        assert!(seen.contains(&"End".to_string()), "phases: {seen:?}");
    }

    /// The landing beat reaches the rig — one of the two subscriptions
    /// `wire_events` makes, handed over by hand because nothing on this
    /// scene's bus emits `player:land` yet.
    #[test]
    fn the_landing_beat_moves_the_rig() {
        let mut host = Host::new();
        (0..240).for_each(|_| {
            host.step(pose(0.0, 0.0, 0.0));
        });
        let before = host.rig.rig_pose();
        host.rig.on_land(6.0);
        (0..3).for_each(|_| {
            host.step(pose(0.0, 0.0, 0.0));
        });
        assert_ne!(before, host.rig.rig_pose(), "the landing jolt did nothing");
    }

    /// The HUD pull carries the real weapon, which is what replaces the
    /// `weapon: None` in `Game::hud_frame`'s `FramePull`.
    #[test]
    fn the_hud_pull_tracks_the_magazine_as_it_drains() {
        let mut host = Host::new();
        let full = host
            .rig
            .hud_pull()
            .ammo
            .expect("a wired weapon reports ammo");
        host.input.mouse_down(0);
        (0..60).for_each(|_| {
            host.step(pose(0.0, 0.0, 0.0));
        });
        let pull = host.rig.hud_pull();
        assert!(pull.ammo.expect("still reporting") < full);
        assert_eq!(pull.mode.as_deref(), Some("auto"));
        assert!(pull.name.is_some_and(|n| !n.is_empty()));
        assert!(
            pull.spread.is_some_and(|s| s > 0.0),
            "firing blooms the reticle"
        );
    }
}
