//! Ported from Claude-of-Duty `src/weapons/viewmodel.js:1-1088` — the
//! additive layer stack that drives the held weapon's transform every frame
//! (`class Viewmodel`, `viewmodel.js:100-1083`).
//!
//! **Scope of this slice.** `viewmodel.js` is two things: the *rig* — the
//! additive pose stack (`update`), the solved ADS translation, the per-shot
//! recoil impulse, the hand-target solve — and a *mesh/scene* layer
//! (`addWeapon`, the reticle sprite geometry, `_updateParts`'s moving-part
//! meshes, `dispose`) that has no consumer in this port yet: there is no
//! renderer wired to the viewmodel rig, and `addWeapon`'s cosmetic vertex-mask
//! baking calls into `materials.js`'s `bakeMasks`, which is not ported (see
//! `docs/work-manifests/claude-of-duty-port/05-port-status.md`'s remaining-work
//! list, item 3). This port carries the **rig**:
//!
//! - [`Viewmodel::update`] — the whole additive stack: base pose (hip/ADS/
//!   sprint/low-ready blend), sway (six layered [`Noise1::fbm`] fields plus a
//!   two-sine breathing cycle), stride bob, the spring-lag layer, recoil +
//!   settle springs, jump/land springs, and the keyframed clip offset from
//!   [`crate::weapons::clips`].
//! - [`Viewmodel::ads_pose`] — the solved (not authored) ADS translation:
//!   the sight node lands exactly on the camera axis at `eye_relief` for any
//!   weapon.
//! - [`Viewmodel::add_recoil`] — the physically-parameterised per-shot kick.
//! - [`Viewmodel::solve_hands`] — the per-frame two-bone IK solve for both
//!   arms, including the body-fixed-shoulder-into-rig-space rebasing the
//!   source's comment calls out (`viewmodel.js:930-935`).
//!
//! It does **not** carry `addWeapon`'s mesh construction, the reticle
//! sprite's geometry/material (`_updateReticle`'s *visibility/size* maths is
//! also skipped — it only ever feeds a sprite this port does not build),
//! `_updateParts`'s moving-part mesh drive, or the world-space queries
//! (`muzzleWorld`/`ejectWorld`/`boreDir`) that read a mesh's
//! `matrixWorld`. `weapons::hands`'s module doc records the matching
//! decision for `hands.js`.
//!
//! ## The camera boundary
//!
//! The source reads `ctx.camera.matrixWorld` every frame to copy the world
//! camera's position/orientation onto the viewmodel anchor
//! (`viewmodel.js:636-646`), then decomposes the anchor's *orientation* alone
//! for the lag layer's angular velocity (`viewmodel.js:649-664`). No
//! camera/render subsystem has landed in this port yet, so — following the
//! `WorldProbe` (`audio::spatial`)/`ScreenProjector` (`ui::markers`)
//! precedent — [`Viewmodel::update`] takes the orientation through the narrow
//! [`ViewCamera`] trait rather than a concrete camera type. The rig's own
//! output (`rig_pos`/`rig_quat`) is the transform the source composes as a
//! child of that camera anchor (view-model space), not a world transform —
//! world-space queries are out of scope for the same reason `addWeapon` is
//! (see above).

use crate::rng::Rng;
use crate::weapons::clips::{make_sample_result, Clip, SampleResult};
use crate::weapons::defs::WeaponDef;
use crate::weapons::hands::{Arm, ArmOpts, HandPoseName};
use crate::weapons::mathx::{
    clamp, clamp01, damp, smootherstep, wrap_pi, Noise1, Spring, Spring3, NOISE1_DEFAULT_SIZE, TAU,
};
use crate::weapons::models::GripTarget;
use crate::weapons::rig_math::{Q, V3};

/// The narrow camera contract [`Viewmodel::update`] needs: this frame's
/// world-space camera orientation (Y-up quaternion), the one piece of
/// `ctx.camera`/`anchor` the lag layer's angular-velocity computation reads.
/// See the module doc's "camera boundary" section.
pub trait ViewCamera {
    fn orientation(&self) -> Q;
}

/// A fixed orientation, for tests and for any caller that already has the
/// quaternion (an app's per-frame camera state) without wanting to define a
/// whole type for it. Mirrors `ui::markers::FixedCamera`.
#[derive(Debug, Clone, Copy)]
pub struct FixedOrientation(pub Q);

impl ViewCamera for FixedOrientation {
    fn orientation(&self) -> Q {
        self.0
    }
}

/// The subset of `s` (`viewmodel.js`'s per-frame input object,
/// `viewmodel.js:624-625`'s doc comment) that [`Viewmodel::update`] actually
/// reads. The doc comment additionally names `crouch`, `empty` and
/// `cycleTime`, but none of the three appears anywhere in `update`'s body —
/// `crouch`/`empty` are dead documented vocabulary (the same pattern
/// `clips.rs` found for `magHand`/`trigger`), and the `cycleTime` the source
/// reads inside `_updateParts` (out of scope — see module doc) is
/// `w.def.cycleTime`, not `s.cycleTime`. This struct carries only the six
/// fields the ported `update` touches.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameInput {
    pub ads: bool,
    pub sprint: bool,
    pub low_ready: bool,
    pub speed: f64,
    pub airborne: bool,
    pub trigger: bool,
}

/// The subset of one weapon's built model + def that the ported rig methods
/// read — `viewmodel.js`'s `weapons.get(id)` entry (`addWeapon`,
/// `viewmodel.js:405-434`) minus every mesh/parts field this slice does not
/// carry (`group`, `meshes`, `parts`, `clips`' events target, `optic`, ...).
#[derive(Debug, Clone)]
pub struct WeaponRig {
    pub def: &'static WeaponDef,
    /// `nodes.sight`, weapon space. `viewmodel.js:415`.
    pub sight: V3,
    pub grip_r: GripTarget,
    pub grip_l: GripTarget,
    /// `w.lhandPose` (`viewmodel.js:433`). The source's per-weapon *fitted*
    /// override (`_fitSupportHand`, out of scope — see `weapons::hands`'s
    /// module doc) is not applied; this is always the authored pose.
    pub lhand_pose: HandPoseName,
}

/// `NOISE_RATES` — `noiseRates`, `viewmodel.js:259`.
const NOISE_RATES: [f64; 6] = [0.13, 0.19, 0.271, 0.083, 0.117, 0.163];

/// `handBasis(out, finger, back)`. `viewmodel.js:88-98`. Right-handed hand
/// basis from a finger direction and a back-of-hand direction — the weapon
/// grip nodes' `finger`/`back` triples feed straight into this.
fn hand_basis(finger: V3, back: V3) -> Q {
    let bz = finger.scale(-1.0).normalize(); // hand +Z
    let mut by = back.sub(bz.scale(back.dot(bz)));
    if by.length_sq() < 1e-8 {
        by = V3::new(0.0, 1.0, 0.0).sub(bz.scale(bz.y));
    }
    by = by.normalize(); // hand +Y
    let bx = by.cross(bz).normalize(); // hand +X
    Q::from_basis(bx, by, bz)
}

/// `class Viewmodel` (rig subset — see module doc). `viewmodel.js:100-1083`.
#[derive(Debug, Clone)]
pub struct Viewmodel {
    rng: Rng,

    pub arm_r: Arm,
    pub arm_l: Arm,
    /// Body-fixed shoulders, in camera/anchor space. `viewmodel.js:165-166`.
    shoulder_r: V3,
    shoulder_l: V3,

    active: Option<WeaponRig>,

    ads_t: f64,
    ads_target: f64,
    sprint_t: f64,
    low_ready_t: f64,
    bob_phase: f64,
    noise_t: f64,
    trigger_t: f64,
    trigger_target: f64,

    lag: Spring3,
    lag_rot: Spring3,
    rec_pos: Spring3,
    rec_rot: Spring3,
    jump_spring: Spring,
    land_spring: Spring,
    settle: Spring3,

    noise: [Noise1; 6],

    ang_vel_yaw: f64,
    ang_vel_pitch: f64,
    prev_yaw: f64,
    prev_pitch: f64,
    has_prev: bool,

    clip: Option<Clip>,
    clip_t: f64,
    clip_prev_t: f64,
    clip_result: SampleResult,

    /// `boltCycle` (`viewmodel.js:274`) — set by [`Viewmodel::add_recoil`],
    /// read by `_updateParts` (out of scope, see module doc). Kept as plain
    /// state so `add_recoil` matches the source's assignment exactly even
    /// though nothing in this slice consumes it yet.
    pub bolt_cycle: f64,

    /// The rig's own transform this frame — a child of the camera anchor, so
    /// this is view-model space, not world space. `this.rig.position`/
    /// `.quaternion`, written at the end of [`Viewmodel::update`]
    /// (`viewmodel.js:819-826`).
    rig_pos: V3,
    rig_quat: Q,
}

impl Viewmodel {
    /// `constructor(ctx, mats)` (rig subset). `viewmodel.js:101-305`.
    /// `rng` is forked from the caller's, exactly as `ctx.rng.fork()`
    /// (`viewmodel.js:104`) — every `Noise1` table draw below consumes from
    /// that forked stream, in the source's order, so the RNG contract in the
    /// port recipe (fork order + draw order are part of determinism) holds.
    pub fn new(rng: &mut Rng) -> Self {
        let mut rng = rng.fork();
        // `for (i=0;i<6;i++) this.noise.push(new Noise1(this.rng, 512));`
        // `viewmodel.js:257-258` — six draws from the forked stream, in order.
        let noise: [Noise1; 6] = std::array::from_fn(|_| Noise1::new(&mut rng, NOISE1_DEFAULT_SIZE));

        // `viewmodel.js:130-148`: shoulder placement + starting pose per arm,
        // carried verbatim including the long comment there about why the
        // shoulders stay behind the eye and the reach is bought by the bone
        // cheat rather than blading the shoulder forward.
        let arm_r = Arm::new(
            1.0,
            ArmOpts {
                scale: 1.0,
                shoulder_x: 0.205,
                shoulder_y: -0.2,
                shoulder_z: 0.06,
                pose: HandPoseName::Grip,
                ..ArmOpts::default()
            },
        );
        let arm_l = Arm::new(
            -1.0,
            ArmOpts {
                scale: 0.97,
                shoulder_x: 0.2,
                shoulder_y: -0.22,
                shoulder_z: 0.02,
                pose: HandPoseName::Clamp,
                ..ArmOpts::default()
            },
        );

        Viewmodel {
            rng,
            arm_r,
            arm_l,
            shoulder_r: V3::new(0.205, -0.2, 0.06),
            shoulder_l: V3::new(-0.2, -0.22, 0.02),
            active: None,
            ads_t: 0.0,
            ads_target: 0.0,
            sprint_t: 0.0,
            low_ready_t: 0.0,
            bob_phase: 0.0,
            noise_t: 0.0,
            trigger_t: 0.0,
            trigger_target: 0.0,
            // `viewmodel.js:249-255`.
            lag: Spring3::new(5.4, 0.46),
            lag_rot: Spring3::new(6.2, 0.42),
            rec_pos: Spring3::new(9.0, 0.42),
            rec_rot: Spring3::new(9.0, 0.42),
            jump_spring: Spring::new(5.5, 0.5, 0.0),
            land_spring: Spring::new(7.5, 0.55, 0.0),
            settle: Spring3::new(2.2, 0.7),
            noise,
            ang_vel_yaw: 0.0,
            ang_vel_pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            has_prev: false,
            clip: None,
            clip_t: 0.0,
            clip_prev_t: 0.0,
            clip_result: make_sample_result(),
            bolt_cycle: 0.0,
            rig_pos: V3::ZERO,
            rig_quat: Q::IDENTITY,
        }
    }

    /// `setActive(id)` (rig subset — no `w.group.visible` toggle to make).
    /// `viewmodel.js:517-534`.
    pub fn set_active(&mut self, weapon: WeaponRig) {
        self.rec_pos.reset();
        self.rec_rot.reset();
        self.settle.reset();
        self.bolt_cycle = 0.0;
        self.arm_r.set_pose(HandPoseName::Grip);
        self.arm_l.set_pose(weapon.lhand_pose);
        self.active = Some(weapon);
    }

    pub fn active(&self) -> Option<&WeaponRig> {
        self.active.as_ref()
    }

    /// The rig's transform this frame (view-model space — see module doc).
    pub fn rig_pose(&self) -> (V3, Q) {
        (self.rig_pos, self.rig_quat)
    }

    /// The lag layer's low-passed, clamped angular velocity — `this._angVel`
    /// (`viewmodel.js:261`), exposed so the clamp behaviour is directly
    /// assertable rather than only inferable from the composed pose.
    pub fn ang_vel(&self) -> (f64, f64) {
        (self.ang_vel_yaw, self.ang_vel_pitch)
    }

    pub fn ads_t(&self) -> f64 {
        self.ads_t
    }

    /// `play(name)`, minus the duration return (nothing in this slice needs
    /// it — callers already hold `Clip::duration`). `viewmodel.js:540-549`.
    pub fn play(&mut self, clip: Clip) {
        self.clip_t = 0.0;
        self.clip_prev_t = -1.0;
        self.clip = Some(clip);
    }

    /// `stopClip()`. `viewmodel.js:551-555`.
    pub fn stop_clip(&mut self) {
        self.clip = None;
        self.clip_result.active = false;
        self.clip_result.lhand.weight = 0.0;
    }

    /// The solved (not authored) ADS pose: the translation that puts
    /// `sight` exactly on the camera axis at `eye_relief`, and the cant
    /// orientation. `viewmodel.js:709-718`'s `if (ads > 1e-4)` body,
    /// factored out to a pure function so it is testable in isolation of the
    /// rest of the additive stack. `adsPos = (0,0,-eyeRelief) -
    /// (sight · adsQuat)`.
    pub fn ads_pose(sight: V3, eye_relief: f64, ads_cant: [f64; 3]) -> (V3, Q) {
        let ads_quat = Q::from_euler_xyz(ads_cant[0], ads_cant[1], ads_cant[2]);
        let sight_local = sight.apply_quat(ads_quat);
        let ads_pos = V3::new(0.0, 0.0, -eye_relief).sub(sight_local);
        (ads_pos, ads_quat)
    }

    /// Per-shot viewmodel kick. `addRecoil(pitch, yaw, first)`.
    /// `viewmodel.js:574-608` (minus `this.boltCycle = 1`'s parts-drive
    /// consumer, which this slice still assigns for fidelity — see the
    /// `bolt_cycle` field doc).
    pub fn add_recoil(&mut self, pitch: f64, yaw: f64, first: bool) {
        let Some(weapon) = self.active.clone() else { return };
        let r = weapon.def.recoil;
        let ads = self.ads_t;
        let scale = crate::weapons::mathx::lerp(1.0, 0.54, ads) * if first { 1.18 } else { 1.0 };
        let jitter = 0.86 + self.rng.float() * 0.3;
        self.rec_pos.set_f(r.freq);
        self.rec_pos.set_z(r.damping);
        self.rec_rot.set_f(r.freq * 0.92);
        self.rec_rot.set_z(r.damping);
        // A velocity impulse of v0 on a spring of angular frequency w peaks
        // at roughly v0/w, so the kick amplitudes below are real
        // metres/radians. `viewmodel.js:588-599`.
        let wp = TAU * self.rec_pos.f();
        let wr = TAU * self.rec_rot.f();
        self.rec_pos.kick(
            self.rng.signed() * r.kick_back * 0.2 * scale * wp,
            r.kick_up * scale * jitter * wp,
            r.kick_back * scale * jitter * wp,
        );
        self.rec_rot.kick(
            (pitch * 5.5 + r.pitch * 1.4) * scale * jitter * wr,
            (-yaw * 4.5 - self.rng.signed() * r.yaw * 0.8) * scale * wr,
            (self.rng.signed() * 0.4 + 0.6) * r.roll * scale * wr,
        );
        let ws = TAU * self.settle.f();
        self.settle.kick(
            self.rng.signed() * 0.0012 * scale * ws,
            0.0018 * scale * ws,
            self.rng.signed() * 0.003 * scale * ws,
        );
        self.bolt_cycle = 1.0;
    }

    /// `jump()`. `viewmodel.js:610-612`.
    pub fn jump(&mut self) {
        self.jump_spring.kick(-1.2);
    }

    /// `land(speed = 3)`. `viewmodel.js:614-616`.
    pub fn land(&mut self, speed: f64) {
        self.land_spring.kick(clamp(speed * 0.45, 0.4, 3.4));
    }

    /// `update(dt, s)` (rig subset — no reticle/parts/FOV writes).
    /// `viewmodel.js:627-852`.
    pub fn update(&mut self, dt: f64, s: &FrameInput, camera: &dyn ViewCamera) {
        let Some(weapon) = self.active.clone() else { return };
        let def = weapon.def;
        // `dt = dt>0 ? (dt<0.1?dt:0.1) : 0`. `viewmodel.js:633`.
        let dt = clamp(dt, 0.0, 0.1);

        /* -------- angular velocity for the lag layer -------------------- */
        let cam_quat = camera.orientation();
        let e = cam_quat.to_euler_yxz();
        let yaw = e.y;
        let pitch = e.x;
        if self.has_prev && dt > 1e-5 {
            let dy = wrap_pi(yaw - self.prev_yaw) / dt;
            let dp = wrap_pi(pitch - self.prev_pitch) / dt;
            // Low-pass, then clamp: a teleport must not throw the gun off
            // screen. `viewmodel.js:655-657`.
            self.ang_vel_yaw = damp(self.ang_vel_yaw, clamp(dy, -9.0, 9.0), 18.0, dt);
            self.ang_vel_pitch = damp(self.ang_vel_pitch, clamp(dp, -9.0, 9.0), 18.0, dt);
        } else {
            self.ang_vel_yaw = 0.0;
            self.ang_vel_pitch = 0.0;
        }
        self.prev_yaw = yaw;
        self.prev_pitch = pitch;
        self.has_prev = true;

        /* -------- blends -------------------------------------------------- */
        let ads_rate = 1.0 / def.ads_time.max(0.05);
        // `this.clip && this.clip.name !== 'draw' ? 0 : s.ads ? 1 : 0`.
        // `viewmodel.js:668`.
        let want_ads = match &self.clip {
            Some(c) if c.name != "draw" => 0.0,
            _ => {
                if s.ads {
                    1.0
                } else {
                    0.0
                }
            }
        };
        self.ads_target = want_ads;
        self.ads_t = clamp01(self.ads_t + (if want_ads > 0.0 { ads_rate } else { -ads_rate * 1.25 }) * dt);
        let ads = smootherstep(0.0, 1.0, self.ads_t);

        let sprint_target = if s.sprint && self.clip.is_none() { 1.0 } else { 0.0 };
        self.sprint_t = damp(self.sprint_t, sprint_target, 9.0, dt);
        self.low_ready_t = damp(self.low_ready_t, if s.low_ready { 1.0 } else { 0.0 }, 8.0, dt);

        self.trigger_target = if s.trigger { 1.0 } else { 0.0 };
        self.trigger_t = damp(self.trigger_t, self.trigger_target, 26.0, dt);

        /* -------- base pose ------------------------------------------------ */
        let mut base_pos = V3::from_array(def.hip_pos);
        let mut base_quat = Q::from_euler_xyz(def.hip_rot[0], def.hip_rot[1], def.hip_rot[2]);

        if self.sprint_t > 1e-3 {
            let p = V3::from_array(def.sprint_pos);
            let q = Q::from_euler_xyz(def.sprint_rot[0], def.sprint_rot[1], def.sprint_rot[2]);
            base_pos = base_pos.lerp(p, self.sprint_t);
            base_quat = base_quat.slerp(q, self.sprint_t);
        }
        if self.low_ready_t > 1e-3 {
            let p = V3::from_array(def.low_ready_pos);
            let q = Q::from_euler_xyz(def.low_ready_rot[0], def.low_ready_rot[1], def.low_ready_rot[2]);
            base_pos = base_pos.lerp(p, self.low_ready_t);
            base_quat = base_quat.slerp(q, self.low_ready_t);
        }

        /* -------- ADS pose: solved, not authored ---------------------------- */
        if ads > 1e-4 {
            let (ads_pos, ads_quat) = Self::ads_pose(weapon.sight, def.eye_relief, def.ads_cant);
            base_pos = base_pos.lerp(ads_pos, ads);
            base_quat = base_quat.slerp(ads_quat, ads);
        }

        /* -------- additive layers -------------------------------------------- */
        let sway_scale = def.sway_scale
            * crate::weapons::mathx::lerp(1.0, 0.22, ads)
            * crate::weapons::mathx::lerp(1.0, 1.5, self.sprint_t);
        self.noise_t += dt;
        let n = &self.noise;
        let nr = NOISE_RATES;
        let t = self.noise_t;
        let sway_x = n[0].fbm(t * nr[0], 3, 0.5) * 0.55 + n[3].fbm(t * nr[3] * 2.3, 2, 0.5) * 0.45;
        let sway_y = n[1].fbm(t * nr[1], 3, 0.5) * 0.55 + n[4].fbm(t * nr[4] * 2.1, 2, 0.5) * 0.45;
        let sway_z = n[2].fbm(t * nr[2], 2, 0.5) * 0.6 + n[5].fbm(t * nr[5] * 1.7, 2, 0.5) * 0.4;
        let breath = (t * 1.38).sin() * 0.5 + (t * 0.61 + 1.1).sin() * 0.25;

        let mut px = sway_x * 0.0075 * sway_scale;
        let mut py = (sway_y * 0.006 + breath * 0.0022) * sway_scale;
        let mut pz = sway_z * 0.004 * sway_scale;
        let mut rx = (sway_y * 0.021 + breath * 0.006) * sway_scale;
        let mut ry = sway_x * 0.028 * sway_scale;
        let mut rz = sway_z * 0.017 * sway_scale;

        /* -------- movement bob ------------------------------------------------ */
        let speed = s.speed;
        let bob_amt = def.bob_scale
            * clamp01(speed / 4.2)
            * crate::weapons::mathx::lerp(1.0, 0.28, ads)
            * if s.airborne { 0.25 } else { 1.0 };
        if speed > 0.05 {
            self.bob_phase += dt * (3.1 + speed * 0.72) * if s.sprint { 1.05 } else { 1.0 };
            if self.bob_phase > TAU * 64.0 {
                self.bob_phase -= TAU * 64.0;
            }
        }
        let bp = self.bob_phase;
        px += bp.sin() * 0.0165 * bob_amt;
        py += (bp.cos().abs() - 0.6) * 0.0125 * bob_amt;
        pz += (bp * 2.0).sin() * 0.0055 * bob_amt;
        rz += bp.sin() * 0.031 * bob_amt;
        rx += (bp * 2.0).cos() * 0.014 * bob_amt;
        ry += (bp + 0.6).sin() * 0.019 * bob_amt;

        /* -------- weapon lag ---------------------------------------------- */
        let lag_scale = crate::weapons::mathx::lerp(1.0, 0.42, ads);
        let (av_yaw, av_pitch) = (self.ang_vel_yaw, self.ang_vel_pitch);
        self.lag.step(
            dt,
            clamp(-av_yaw * 0.019, -0.05, 0.05) * lag_scale,
            clamp(av_pitch * 0.014, -0.04, 0.04) * lag_scale,
            clamp(-av_yaw.abs() * 0.006, -0.03, 0.03) * lag_scale,
        );
        self.lag_rot.step(
            dt,
            clamp(-av_pitch * 0.075, -0.24, 0.24) * lag_scale,
            clamp(av_yaw * 0.085, -0.3, 0.3) * lag_scale,
            clamp(-av_yaw * 0.055, -0.2, 0.2) * lag_scale,
        );
        px += self.lag.x();
        py += self.lag.y();
        pz += self.lag.z();
        rx += self.lag_rot.x();
        ry += self.lag_rot.y();
        rz += self.lag_rot.z();

        /* -------- recoil + settle ----------------------------------------- */
        self.rec_pos.step(dt, 0.0, 0.0, 0.0);
        self.rec_rot.step(dt, 0.0, 0.0, 0.0);
        self.settle.step(dt, 0.0, 0.0, 0.0);
        px += self.rec_pos.x();
        py += self.rec_pos.y();
        pz += self.rec_pos.z();
        rx += self.rec_rot.x() + self.settle.y();
        ry += self.rec_rot.y() + self.settle.x();
        rz += self.rec_rot.z() + self.settle.z();

        /* -------- jump / land --------------------------------------------- */
        self.jump_spring.step_to_target(dt);
        self.land_spring.step_to_target(dt);
        py -= self.land_spring.x * 0.014 + self.jump_spring.x * 0.006;
        rx -= self.land_spring.x * 0.05;

        /* -------- clip (reload / inspect / draw) --------------------------- */
        if let Some(clip) = self.clip.as_ref() {
            let duration = clip.duration;
            self.clip_t += dt;
            let tt = clamp(self.clip_t, 0.0, duration);
            clip.sample(tt, &mut self.clip_result);
            self.clip_prev_t = tt;
            px += self.clip_result.pos[0];
            py += self.clip_result.pos[1];
            pz += self.clip_result.pos[2];
            rx += self.clip_result.rot[0];
            ry += self.clip_result.rot[1];
            rz += self.clip_result.rot[2];
            if self.clip_t >= duration {
                self.clip = None;
                self.clip_result.active = false;
                self.clip_result.lhand.weight = 0.0;
            }
        }

        /* -------- compose --------------------------------------------------- */
        self.rig_pos = V3::new(base_pos.x + px, base_pos.y + py, base_pos.z + pz);
        let add_quat = Q::from_euler_xyz(rx, ry, rz);
        self.rig_quat = base_quat.multiply(add_quat);

        /* -------- hands (IK only — see module doc) -------------------------- */
        self.solve_hands(&weapon);
    }

    /// `_solveHands(w, res)` (arm-solve half only — no `armR.setTrigger`
    /// mesh consumer beyond the stored curl, no magazine-in-hand geometry).
    /// `viewmodel.js:929-960`.
    fn solve_hands(&mut self, weapon: &WeaponRig) {
        // Shoulders are body-fixed: express the camera-space anchor in rig
        // space. `viewmodel.js:930-935`.
        let q_inv = self.rig_quat.invert();
        self.arm_r.shoulder = self.shoulder_r.sub(self.rig_pos).apply_quat(q_inv);
        self.arm_l.shoulder = self.shoulder_l.sub(self.rig_pos).apply_quat(q_inv);

        // ---- shooting hand: welded to the grip ----
        let g_r = weapon.grip_r;
        let hand_pos = V3::new(g_r.pos[0] as f64, g_r.pos[1] as f64, g_r.pos[2] as f64);
        let finger_r = V3::new(g_r.finger[0] as f64, g_r.finger[1] as f64, g_r.finger[2] as f64);
        let back_r = V3::new(g_r.back[0] as f64, g_r.back[1] as f64, g_r.back[2] as f64);
        let hand_quat = hand_basis(finger_r, back_r);
        self.arm_r.solve(hand_pos, hand_quat);
        self.arm_r.set_trigger(self.trigger_t);

        // ---- support hand: grip, or wherever the clip puts it ----
        let (pos, finger, back, pose) = if self.clip_result.active && self.clip_result.lhand.weight > 0.5 {
            (
                V3::from_array(self.clip_result.lhand.pos),
                V3::from_array(self.clip_result.lhand.finger),
                V3::from_array(self.clip_result.lhand.back),
                HandPoseName::from(self.clip_result.lhand.pose),
            )
        } else {
            let g_l = weapon.grip_l;
            (
                V3::new(g_l.pos[0] as f64, g_l.pos[1] as f64, g_l.pos[2] as f64),
                V3::new(g_l.finger[0] as f64, g_l.finger[1] as f64, g_l.finger[2] as f64),
                V3::new(g_l.back[0] as f64, g_l.back[1] as f64, g_l.back[2] as f64),
                weapon.lhand_pose,
            )
        };
        let hand_quat_l = hand_basis(finger, back);
        if pose != self.arm_l.pose_name {
            self.arm_l.set_pose(pose);
        }
        self.arm_l.solve(pos, hand_quat_l);
    }
}
