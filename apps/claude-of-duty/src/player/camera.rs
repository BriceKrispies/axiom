//! Camera feel.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/player/camera.js:1-357` — the whole
//! file, minus `applyTo(camera)` (see the module doc comment below).
//!
//! Everything a modern shooter does to make a floating pair of eyes read as a
//! body, layered so no single effect ever dominates:
//!
//! | channel | what it does |
//! |---|---|
//! | eye height       | stance-smoothed, so crouching is a movement not a cut |
//! | view bob         | 1:2 Lissajous (figure-eight) locked to footstep cadence |
//! | step micro-shift | a per-footfall vertical spring on top of the bob |
//! | landing impact   | dip + pitch + roll from the actual impact speed |
//! | strafe / turn roll | a degree of bank into the direction of travel |
//! | slide            | deep dip, forward push and a shoulder roll |
//! | mantle           | curve-driven offsets handed over by `MantleMotion` |
//! | breathing sway   | two detuned sines, amplified by ADS, wounds, suppression |
//! | recoil           | spring-damper impulse channel owned by the camera |
//! | kick             | a second, independent channel the weapon system pushes |
//! | trauma shake     | noise-driven, decays, used by explosions and heavy hits |
//! | FOV              | critically-damped springs: ADS crisp, sprint breathing |
//!
//! Position offsets are built in the *yaw* basis (not the full view basis) so
//! looking up does not turn vertical bob into forward/backward lurch.
//!
//! **Not ported: `applyTo(camera)`** (`camera.js:346-356`). It writes the
//! composed transform onto a live `THREE.PerspectiveCamera` — a render-layer
//! object this crate has no equivalent for yet (no viewer camera is wired up).
//! Everything up to and including the composed values (`eye_position`,
//! `rotation`, `fov`) is ported and available on [`CameraRig`]; whatever binds
//! a render camera reads those fields directly rather than calling a ported
//! `applyTo`. `forward` therefore also never updates past its constructed
//! default here (the source only ever recomputes it inside `applyTo`, from
//! the camera's quaternion).

use crate::config::Config;
use crate::engine::Time;
use crate::player::movement::Movement;
use crate::player::springs::{approach, clamp, clamp01, hash_noise, lerp, RecoilAxis, Spring, DEG};
use crate::player::tuning::{Stance, CAMERA, MOVE};
use crate::player::Vec3;

/// `health = { fraction, suppression }`, the shape `camera.js`'s `update`
/// reads off the source's health object (`health.fraction`,
/// `health.suppression ?? 0`). Not a health *system* — just the two fields
/// this file consumes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HealthView {
    pub fraction: f64,
    pub suppression: f64,
}

/// The kick channel published for the viewmodel — `this.viewKick`.
/// `camera.js:81`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewKick {
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
    pub punch: f64,
}

/// The composed camera rotation — `this.rotation` (a `THREE.Euler(x, y, z,
/// 'YXZ')`). Order matters for whatever consumes this: apply yaw, then pitch,
/// then roll, exactly as `'YXZ'` names.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Euler {
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
}

/// `class CameraRig`. `camera.js:30-356`.
pub struct CameraRig {
    pub eye: f64,
    pub crouch_blend: f64,

    bob_phase: f64,
    bob_weight: f64,
    bob_roll: f64,
    bob_pitch: f64,

    // `pub`, matching the source: `this.dip`/`this.step`/etc are ordinary
    // JS object properties with no privacy, and `weapons` reads/kicks some of
    // these channels directly (`addRecoil`/`addKick` are the documented
    // entry points, but the springs themselves were never hidden).
    pub dip: Spring,
    pub step: Spring,
    pub recoil_pitch: RecoilAxis,
    pub recoil_yaw: RecoilAxis,
    pub recoil_roll: RecoilAxis,
    pub punch: Spring,
    pub kick_pitch: RecoilAxis,
    pub kick_yaw: RecoilAxis,
    pub kick_roll: RecoilAxis,

    strafe_roll: f64,
    turn_roll: f64,
    slide_roll: f64,
    air_roll: f64,

    pub trauma: f64,
    shake_time: f64,

    breath_phase: f64,

    pub base_fov: f64,
    pub fov: f64,
    fov_move: f64,
    fov_ads: f64,

    pub slide_blend: f64,
    slide_side: f64,

    pub view_kick: ViewKick,
    bob_offset: Vec3,
    pub offset: Vec3,
    pub eye_position: Vec3,
    pub rotation: Euler,
    /// Only ever equals its constructed default here — see the module doc
    /// comment on why `applyTo` (the source's only writer) is not ported.
    pub forward: Vec3,
}

impl CameraRig {
    /// `constructor(ctx)`. `camera.js:31-91`.
    pub fn new(fov: f64) -> Self {
        let c = &CAMERA;
        CameraRig {
            eye: 1.66,
            crouch_blend: 0.0,

            bob_phase: 0.0,
            bob_weight: 0.0,
            bob_roll: 0.0,
            bob_pitch: 0.0,

            dip: Spring::new(c.land.freq, c.land.damping, 0.0),
            step: Spring::new(c.step.freq, c.step.damping, 0.0),
            recoil_pitch: RecoilAxis::new(c.recoil.freq, c.recoil.damping, c.recoil.residual_tau, c.recoil.residual_share),
            recoil_yaw: RecoilAxis::new(
                c.recoil.freq * 1.08,
                c.recoil.damping + 0.06,
                c.recoil.residual_tau,
                c.recoil.residual_share,
            ),
            recoil_roll: RecoilAxis::new(c.recoil.freq * 0.86, c.recoil.damping + 0.1, c.recoil.residual_tau, 0.24),
            punch: Spring::new(c.recoil.punch_freq, c.recoil.punch_damping, 0.0),
            // Second, independent channel: `weapons` pushes into this one.
            kick_pitch: RecoilAxis::new(11.0, 0.58, 0.22, 0.28),
            kick_yaw: RecoilAxis::new(11.5, 0.6, 0.22, 0.28),
            kick_roll: RecoilAxis::new(9.0, 0.62, 0.22, 0.22),

            strafe_roll: 0.0,
            turn_roll: 0.0,
            slide_roll: 0.0,
            air_roll: 0.0,

            trauma: 0.0,
            shake_time: 0.0,

            breath_phase: 0.0,

            base_fov: fov,
            fov,
            fov_move: 1.0,
            fov_ads: 1.0,

            slide_blend: 0.0,
            slide_side: 1.0,

            view_kick: ViewKick::default(),
            bob_offset: [0.0, 0.0, 0.0],
            offset: [0.0, 0.0, 0.0],
            eye_position: [0.0, 0.0, 0.0],
            rotation: Euler::default(),
            forward: [0.0, 0.0, -1.0],
        }
    }

    /// `reset(eye)`. `camera.js:93-113`.
    pub fn reset(&mut self, eye: f64) {
        self.eye = eye;
        self.bob_phase = 0.0;
        self.bob_weight = 0.0;
        self.dip.reset(0.0);
        self.step.reset(0.0);
        self.recoil_pitch.reset();
        self.recoil_yaw.reset();
        self.recoil_roll.reset();
        self.kick_pitch.reset();
        self.kick_yaw.reset();
        self.kick_roll.reset();
        self.punch.reset(0.0);
        self.trauma = 0.0;
        self.strafe_roll = 0.0;
        self.turn_roll = 0.0;
        self.slide_roll = 0.0;
        self.slide_blend = 0.0;
        self.fov_move = 1.0;
        self.fov_ads = 1.0;
    }

    /* ==================================================================== */
    /* impulses — the public feel API                                       */
    /* ==================================================================== */

    /// Camera-owned recoil. Angles in radians; `punch` in metres. `addRecoil`.
    /// `camera.js:120-125`.
    pub fn add_recoil(&mut self, pitch: f64, yaw: f64, roll: f64, punch: f64) {
        self.recoil_pitch.kick(pitch);
        self.recoil_yaw.kick(yaw);
        self.recoil_roll.kick(roll);
        if punch != 0.0 {
            self.punch.impulse(-punch * 14.0);
        }
    }

    /// Weapon-driven kick — a separate channel so the two never fight.
    /// `addKick`. `camera.js:128-132`.
    pub fn add_kick(&mut self, pitch: f64, yaw: f64, roll: f64) {
        self.kick_pitch.kick(pitch);
        self.kick_yaw.kick(yaw);
        self.kick_roll.kick(roll);
    }

    /// `addTrauma`. `camera.js:134-136`.
    pub fn add_trauma(&mut self, a: f64) {
        self.trauma = clamp01(self.trauma + a);
    }

    /// `onLand(speed)`. `camera.js:138-149`.
    pub fn on_land(&mut self, speed: f64) -> f64 {
        let l = &CAMERA.land;
        let t = clamp01((speed - l.min_speed) / (l.full_speed - l.min_speed));
        if t <= 0.0 {
            return 0.0;
        }
        // Perceptual curve: a 3 m/s landing should still be felt a little.
        let mag = t.powf(0.72);
        self.dip.impulse(-l.dip_impulse * mag);
        self.recoil_pitch.kick(l.pitch * mag);
        let side = if self.slide_side == 0.0 { 1.0 } else { self.slide_side };
        self.recoil_roll.kick(l.roll * mag * side);
        self.add_trauma(l.trauma * mag * mag);
        mag
    }

    /// `onFootstep(running, stance)`. `camera.js:151-157`.
    pub fn on_footstep(&mut self, running: bool, stance: Stance) {
        let s = &CAMERA.step;
        let mut amp = s.impulse * if running { s.sprint_scale } else { 1.0 };
        if stance == Stance::Crouch {
            amp *= 0.55;
        } else if stance == Stance::Prone {
            amp *= 0.3;
        }
        self.step.impulse(-amp);
    }

    /// `onSlideStart(side)`. `camera.js:159-163`.
    pub fn on_slide_start(&mut self, side: f64) {
        self.slide_side = if side == 0.0 { 1.0 } else { side };
        self.dip.impulse(-0.9);
        self.add_trauma(0.12);
    }

    /* ==================================================================== */
    /* per-frame composition                                                */
    /* ==================================================================== */

    /// `update(dt, m, health)`. `camera.js:174-314`.
    pub fn update(&mut self, dt: f64, m: &mut Movement, health: HealthView, config: &Config, time: &Time) {
        let c = &CAMERA;
        let ads = clamp01(m.ads_amount);

        // ---- stance / eye height --------------------------------------------
        let target_eye = m.eye_height() + if m.sliding { -0.1 } else { 0.0 };
        let growing = target_eye > self.eye;
        let tau = if m.stance == Stance::Prone || self.eye < 0.75 {
            MOVE.stance_tau.prone
        } else if growing {
            MOVE.stance_tau.crouch_stand
        } else {
            MOVE.stance_tau.stand_crouch
        };
        self.eye = approach(self.eye, target_eye, tau, dt);
        self.crouch_blend = clamp01(1.0 - (self.eye - 1.0) / 0.66);

        // ---- slide envelope --------------------------------------------------
        let slide_target = if m.sliding { 1.0 - 0.45 * m.slide_progress() } else { 0.0 };
        self.slide_blend = approach(self.slide_blend, slide_target, if m.sliding { 0.045 } else { 0.09 }, dt);

        // ---- yaw basis -------------------------------------------------------
        let sy = m.yaw.sin();
        let cy = m.yaw.cos();
        let fwd: Vec3 = [-sy, 0.0, -cy];
        let right: Vec3 = [cy, 0.0, -sy];

        // ---- bob -------------------------------------------------------------
        self.update_bob(dt, m, ads);

        // ---- springs ---------------------------------------------------------
        self.dip.step(dt);
        self.step.step(dt);
        self.punch.step(dt);
        self.recoil_pitch.step(dt);
        self.recoil_yaw.step(dt);
        self.recoil_roll.step(dt);
        self.kick_pitch.step(dt);
        self.kick_yaw.step(dt);
        self.kick_roll.step(dt);

        // ---- rolls -----------------------------------------------------------
        let r = &c.roll;
        let strafe_target = -m.cmd.move_x * r.strafe * if m.grounded { 1.0 } else { 0.45 } * (1.0 - 0.6 * ads);
        self.strafe_roll = approach(self.strafe_roll, strafe_target, r.tau, dt);
        let turn_target = clamp(m.yaw_rate * r.yaw_rate, -r.yaw_rate_max, r.yaw_rate_max) * (1.0 - 0.5 * ads);
        self.turn_roll = approach(self.turn_roll, turn_target, r.tau * 1.4, dt);
        let slide_roll_target = if m.sliding { -self.slide_side * r.slide } else { 0.0 };
        self.slide_roll = approach(self.slide_roll, slide_roll_target, 0.1, dt);
        let air_target = if m.grounded {
            0.0
        } else {
            clamp(-m.velocity[1] * 0.02, -1.0, 1.0) * r.air
        };
        self.air_roll = approach(self.air_roll, air_target, 0.22, dt);

        // ---- trauma shake ----------------------------------------------------
        let s = &c.shake;
        self.trauma = (self.trauma - s.decay * dt).max(0.0);
        let shake = self.trauma * self.trauma;
        self.shake_time += dt * s.freq;
        let mut shake_pitch = 0.0;
        let mut shake_yaw = 0.0;
        let mut shake_roll = 0.0;
        let mut shake_x = 0.0;
        let mut shake_y = 0.0;
        if shake > 1e-4 {
            shake_pitch = hash_noise(self.shake_time, 11) * shake * s.rot * DEG;
            shake_yaw = hash_noise(self.shake_time + 31.7, 23) * shake * s.rot * DEG;
            shake_roll = hash_noise(self.shake_time + 57.1, 37) * shake * s.rot * 0.7 * DEG;
            shake_x = hash_noise(self.shake_time * 0.8 + 13.3, 41) * shake * s.pos;
            shake_y = hash_noise(self.shake_time * 0.8 + 71.9, 53) * shake * s.pos;
        }

        // ---- breathing sway --------------------------------------------------
        let b = &c.breath;
        let move_factor = clamp01(m.horizontal_speed / 2.2);
        let mut amp = b.amp;
        amp *= lerp(1.0, b.ads_scale, ads);
        amp *= lerp(1.0, b.low_health_scale, 1.0 - clamp01(health.fraction));
        amp *= lerp(1.0, b.suppression_scale, clamp01(health.suppression));
        amp *= 1.0 - b.move_damp * move_factor;
        self.breath_phase += dt;
        let b_a = (self.breath_phase * std::f64::consts::PI * 2.0 * b.freq_a).sin();
        let b_b = (self.breath_phase * std::f64::consts::PI * 2.0 * b.freq_b + 1.7).sin();
        let breath_pitch = (b_a * 0.7 + b_b * 0.3) * amp;
        let breath_yaw = (b_b * 0.75 - b_a * 0.25) * amp * 1.15;
        let breath_pos = (b_a * 0.6 + b_b * 0.4) * b.pos_amp * (1.0 - 0.8 * move_factor);

        // ---- mantle ----------------------------------------------------------
        let mm = &m.mantle_motion;
        let mantle_y = if mm.active { mm.cam_y } else { 0.0 };
        let mantle_fwd = if mm.active { mm.cam_forward } else { 0.0 };
        let mantle_pitch = if mm.active { mm.cam_pitch } else { 0.0 };
        let mantle_roll = if mm.active { mm.cam_roll } else { 0.0 };

        // ---- assemble position ----------------------------------------------
        let base = m.sample_render(time.alpha);
        let bob_x = self.bob_offset[0];
        let bob_y = self.bob_offset[1];
        let bob_z = self.bob_offset[2];

        // Lean is applied in world space further down (it comes from the
        // validated capsule probe, not from the bob basis).
        let lateral = bob_x + shake_x;
        let vertical =
            bob_y + self.dip.value + self.step.value + shake_y + mantle_y + breath_pos - self.slide_blend * 0.1;
        let forward = bob_z + self.punch.value + mantle_fwd + self.slide_blend * 0.045;

        self.offset = [0.0, 0.0, 0.0];
        self.offset[0] += right[0] * lateral;
        self.offset[1] += right[1] * lateral;
        self.offset[2] += right[2] * lateral;
        self.offset[0] += fwd[0] * forward;
        self.offset[1] += fwd[1] * forward;
        self.offset[2] += fwd[2] * forward;
        self.offset[1] += vertical;

        self.eye_position = [
            base[0] + m.lean_offset_x + self.offset[0],
            base[1] + self.eye + self.offset[1] - m.lean_amount.abs() * MOVE.lean.drop,
            base[2] + m.lean_offset_z + self.offset[2],
        ];

        // ---- assemble rotation ----------------------------------------------
        let pitch = clamp(
            m.pitch + self.recoil_pitch.value + self.kick_pitch.value + breath_pitch + self.bob_pitch + shake_pitch
                + mantle_pitch,
            -CAMERA.pitch_limit,
            CAMERA.pitch_limit,
        );
        let yaw = m.yaw + self.recoil_yaw.value + self.kick_yaw.value + breath_yaw + shake_yaw;
        let roll = self.strafe_roll
            + self.turn_roll
            + self.slide_roll
            + self.air_roll
            + self.bob_roll
            + self.recoil_roll.value
            + self.kick_roll.value
            + shake_roll
            + mantle_roll
            - m.lean_amount * MOVE.lean.roll;

        self.rotation = Euler { pitch, yaw, roll };

        // ---- FOV -------------------------------------------------------------
        let f = &c.fov;
        let move_target = if m.sliding {
            f.slide
        } else if m.tactical_sprint {
            f.tac_sprint
        } else if m.sprinting {
            f.sprint
        } else if !m.grounded && m.velocity[1] < -6.0 {
            f.air
        } else {
            1.0
        };
        self.fov_move = approach(self.fov_move, move_target, f.move_tau, dt);
        self.fov_ads = approach(self.fov_ads, lerp(1.0, f64::from(config.ads_fov_scale.get()), ads), f.ads_tau, dt);
        self.base_fov = f64::from(config.fov);
        self.fov = self.base_fov * self.fov_move * self.fov_ads;

        // ---- publish the kick channel for the viewmodel ----------------------
        self.view_kick.pitch = self.recoil_pitch.value + self.kick_pitch.value;
        self.view_kick.yaw = self.recoil_yaw.value + self.kick_yaw.value;
        self.view_kick.roll = self.recoil_roll.value + self.kick_roll.value;
        self.view_kick.punch = self.punch.value;
    }

    /// `_updateBob`. `camera.js:316-343`.
    fn update_bob(&mut self, dt: f64, m: &Movement, ads: f64) {
        let b = &CAMERA.bob;
        let speed = m.horizontal_speed;

        // Phase comes from the movement machine's gait accumulator (pi per
        // footfall) rather than being integrated here, so the bob can never
        // drift out of sync with the footstep events after a jump or a stance
        // change. The +pi/2 offset puts the horizontal extreme exactly on the
        // footfall.
        self.bob_phase = m.step_phase() + std::f64::consts::PI * 0.5;

        // Weight: speed-scaled (sprint bobs more than a walk, but not
        // linearly), faded out in the air and while sliding or aiming.
        let mut w = b.speed_cap.min((speed / 4.57).powf(b.speed_exp));
        if !m.grounded || m.sliding {
            w = 0.0;
        }
        w *= lerp(1.0, b.ads_scale, ads);
        if m.stance == Stance::Prone {
            w *= 0.35;
        }
        self.bob_weight = approach(self.bob_weight, w, b.air_fade, dt);

        let th = self.bob_phase;
        let wt = self.bob_weight;
        self.bob_offset = [th.sin() * b.amp_x * wt, (th * 2.0).sin() * b.amp_y * wt, (th * 2.0).cos() * b.amp_z * wt];
        self.bob_roll = -th.sin() * b.roll * wt;
        self.bob_pitch = (th * 2.0).cos() * b.pitch * wt;
    }
}
