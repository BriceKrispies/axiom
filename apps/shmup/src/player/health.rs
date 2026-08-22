//! Health, regeneration, suppression and the damage-direction model.
//!
//! Ported from Claude-of-Duty `src/player/health.js:1-239` — the whole file.
//!
//! Behaviour matches the CoD contract: no health pickups, a delay after the
//! last hit, then a fast refill. Damage arriving from a direction produces an
//! indicator (angle in *view* space, so the HUD can draw it without knowing
//! anything about the player's transform) and a matching camera impulse, so a
//! hit is felt before it is read.
//!
//! Suppression is a separate 0..1 pool fed by near misses, hits and blasts. It
//! widens the breathing sway and adds a little shake — the same trick CoD uses
//! to make being shot at feel dangerous without taking control away.
//!
//! ## Divergences, and why
//!
//! * **`ctx` and `rig` are parameters, not fields.** The source's `Health`
//!   holds `this.ctx` and `this.rig` and reaches through them
//!   (`this.rig.addRecoil(...)`, `this.ctx.events.emit(...)`,
//!   `this.ctx.time.elapsed`, `this.ctx.camera.position`). Rust cannot hold a
//!   `&mut CameraRig` inside a struct that [`crate::player::system::PlayerCore`]
//!   also owns, so every method that needs one takes it. The call order is
//!   unchanged.
//! * **`this.rig` is never null at any construction site.** The source guards
//!   with `if (this.rig)`; both writers (`new Health(ctx, this.rig)`) always
//!   pass one, so the guard is dead and the parameter here is not an `Option`.
//! * **`_payload` and `_beat` are constructed per emit.** They are
//!   preallocated in the source purely to avoid a per-frame allocation, and
//!   every field of both is written before every emit, so no state crosses
//!   between emits. `_statePayload` is different — [`Health::emit_state`]
//!   reads its *previous* `low` to compute `changedLowState` — so that one is
//!   a field, exactly as in the source.

use crate::events::EventBus;
use crate::player::camera::CameraRig;
use crate::player::springs::{approach, clamp01, lerp, DEG};
use crate::player::tuning::HEALTH;
use crate::player::Vec3;

/// `HEALTH.indicatorMax`. `health.js:33`.
pub const INDICATOR_MAX: usize = HEALTH.indicator_max as usize;

/// The `opts.type` string `player/index.js` tags each damage source with.
/// `health.js` never reads it — the field is set by every caller and consumed
/// by nobody — but dead computation in the source is still part of the source,
/// and a future HUD/FX listener is the obvious consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DamageKind {
    #[default]
    Unspecified,
    /// `{ type: 'bullet' }` — `index.js:423`.
    Bullet,
    /// `{ type: 'explosion' }` — `index.js:438`.
    Explosion,
    /// `{ type: 'fall' }` — `index.js:336`.
    Fall,
}

/// `damage(amount, from, opts = {})`'s third argument. The source's JSDoc also
/// names `suppress`, which nothing ever passes and nothing ever reads; it is
/// not carried here.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DamageOpts {
    /// `opts.yaw ?? this.ctx.camera.rotation.y`.
    pub yaw: Option<f64>,
    pub kind: DamageKind,
}

/// One direction indicator. `health.js:34`. Angle is radians, 0 = straight
/// ahead.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DamageIndicator {
    pub active: bool,
    pub angle: f64,
    pub amount: f64,
    pub life: f64,
    pub world_x: f64,
    pub world_y: f64,
    pub world_z: f64,
}

/// `damage:taken`. `health.js:42`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DamageTakenEvent {
    pub amount: f64,
    pub from: Vec3,
    pub health: f64,
    pub direction: f64,
    pub critical: bool,
}

/// `player:health`. `health.js:43-46` plus the two fields `_emitState` adds on
/// the way out (`changedLowState`, `forced`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HealthStateEvent {
    pub health: f64,
    pub fraction: f64,
    pub low: bool,
    pub critical: bool,
    pub regenerating: bool,
    pub suppression: f64,
    pub dead: bool,
    pub changed_low_state: bool,
    pub forced: bool,
}

impl Default for HealthStateEvent {
    fn default() -> Self {
        HealthStateEvent {
            health: HEALTH.max,
            fraction: 1.0,
            low: false,
            critical: false,
            regenerating: false,
            suppression: 0.0,
            dead: false,
            changed_low_state: false,
            forced: false,
        }
    }
}

/// `player:heartbeat`. `health.js:49`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HeartbeatEvent {
    pub strength: f64,
    pub fraction: f64,
}

/// `player:death`. `health.js:128`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerDeathEvent {
    pub position: Vec3,
}

/// `class Health`. `health.js:19-239`.
#[derive(Debug, Clone, PartialEq)]
pub struct Health {
    pub max: f64,
    pub value: f64,
    pub dead: bool,
    pub regenerating: bool,
    pub last_damage_time: f64,
    pub suppression: f64,
    pub hit_flash: f64,

    /// Direction indicators, oldest first.
    pub indicators: [DamageIndicator; INDICATOR_MAX],

    /// Heartbeat phase, 0..1 per beat, with a double-thump envelope.
    pub beat_phase: f64,
    pub pulse: f64,
    /// 0..1 overall low-health treatment weight.
    pub effect: f64,

    state_payload: HealthStateEvent,
    emit_timer: f64,
    last_emit_health: f64,
}

impl Default for Health {
    fn default() -> Self {
        Health::new()
    }
}

impl Health {
    /// `constructor(ctx, rig)`. `health.js:20-50`.
    pub fn new() -> Self {
        Health {
            max: HEALTH.max,
            value: HEALTH.max,
            dead: false,
            regenerating: false,
            last_damage_time: -100.0,
            suppression: 0.0,
            hit_flash: 0.0,
            indicators: [DamageIndicator::default(); INDICATOR_MAX],
            beat_phase: 0.0,
            pulse: 0.0,
            effect: 0.0,
            state_payload: HealthStateEvent::default(),
            emit_timer: 0.0,
            last_emit_health: HEALTH.max,
        }
    }

    /// `get fraction()`. `health.js:52-54`.
    pub fn fraction(&self) -> f64 {
        clamp01(self.value / self.max)
    }

    /// `get low()`. `health.js:56-58`.
    pub fn low(&self) -> bool {
        self.fraction() < HEALTH.low_threshold
    }

    /// `get critical()`. `health.js:60-62`.
    pub fn critical(&self) -> bool {
        self.fraction() < HEALTH.critical_threshold
    }

    /// `reset(full = true)`. `health.js:64-71`.
    pub fn reset(&mut self, full: bool) {
        if full {
            self.value = self.max;
        }
        self.dead = false;
        self.suppression = 0.0;
        self.hit_flash = 0.0;
        self.last_damage_time = -100.0;
        for i in self.indicators.iter_mut() {
            i.active = false;
        }
    }

    /// `damage(amount, from, opts = {})`. `health.js:80-133`.
    ///
    /// `now` is `this.ctx.time.elapsed`; `camera_position`/`camera_yaw` are
    /// `this.ctx.camera.position` / `.rotation.y` — see the module doc comment
    /// for why they arrive as parameters. Returns the damage actually dealt.
    #[allow(clippy::too_many_arguments)]
    pub fn damage(
        &mut self,
        amount: f64,
        from: Option<Vec3>,
        opts: DamageOpts,
        rig: &mut CameraRig,
        camera_position: Vec3,
        camera_yaw: f64,
        now: f64,
        events: &EventBus,
    ) -> f64 {
        if self.dead || amount <= 0.0 {
            return 0.0;
        }
        let before = self.value;
        self.value = 0.0f64.max(self.value - amount);
        self.last_damage_time = now;
        self.regenerating = false;
        let dealt = before - self.value;

        // ---- direction in view space ---------------------------------------
        let mut angle = 0.0;
        if let Some(from) = from {
            let yaw = opts.yaw.unwrap_or(camera_yaw);
            let dx = from[0] - camera_position[0];
            let dz = from[2] - camera_position[2];
            // Forward at yaw is (-sin, -cos); right is (cos, -sin).
            let f = -yaw.sin() * dx - yaw.cos() * dz;
            let r = yaw.cos() * dx - yaw.sin() * dz;
            angle = r.atan2(f);
            self.push_indicator(angle, dealt, from);
        }

        // ---- felt response --------------------------------------------------
        let severity = clamp01(dealt / 45.0);
        self.hit_flash = clamp01(self.hit_flash + HEALTH.effect.hit_flash * (0.4 + severity));
        self.add_suppression(HEALTH.suppression.per_hit * (0.5 + severity));
        // Punch the camera away from the hit: pitch up, yaw and roll off-axis.
        let s = 0.6 + severity * 1.9;
        rig.add_recoil(
            (1.1 + severity) * DEG * s * 0.7,
            -angle.sin() * (1.4 * DEG) * s,
            -angle.sin() * (2.2 * DEG) * s,
            0.008 * s,
        );
        rig.add_trauma(0.22 * s);

        let payload = DamageTakenEvent {
            amount: dealt,
            health: self.value,
            direction: angle,
            critical: self.critical(),
            from: from.unwrap_or(camera_position),
        };
        events.emit("damage:taken", &payload);

        if self.value <= 0.0 {
            self.dead = true;
            events.emit(
                "player:death",
                &PlayerDeathEvent {
                    position: camera_position,
                },
            );
        }
        self.emit_state(true, events);
        dealt
    }

    /// `heal(amount)`. `health.js:135-137`.
    pub fn heal(&mut self, amount: f64) {
        self.value = self.max.min(self.value + amount);
    }

    /// `addSuppression(a)`. `health.js:139-141`.
    pub fn add_suppression(&mut self, a: f64) {
        self.suppression = clamp01(self.suppression + a);
    }

    /// `_pushIndicator(angle, amount, from)`. `health.js:143-159`.
    ///
    /// **Source quirk, ported as-is (recipe rule 7):** `slot.active = true` is
    /// assigned on the line *above* `slot.amount = Math.max(slot.active ?
    /// slot.amount * 0.5 : 0, amount)`, so the ternary's condition is always
    /// true and the `: 0` arm is unreachable. A slot that was *inactive* — and
    /// therefore still carries the previous occupant's `amount` — has that
    /// stale value halved and max'd in rather than being replaced outright.
    /// Pinned by `indicator_reuse_halves_a_stale_amount_source_quirk` in
    /// `tests/player_system_port.rs`.
    fn push_indicator(&mut self, angle: f64, amount: f64, from: Vec3) {
        // Reuse the slot pointing the most similar way, else the oldest.
        let mut slot: Option<usize> = None;
        let mut oldest: Option<usize> = None;
        for k in 0..self.indicators.len() {
            let i = self.indicators[k];
            if !i.active {
                slot = Some(k);
                break;
            }
            if (angle - i.angle).abs() < 0.5 {
                slot = Some(k);
                break;
            }
            if oldest.is_none_or(|o| i.life > self.indicators[o].life) {
                oldest = Some(k);
            }
        }
        let k = slot.or(oldest).unwrap_or(0);
        let s = &mut self.indicators[k];
        s.active = true;
        s.angle = angle;
        s.amount = (s.amount * 0.5).max(amount);
        s.life = 0.0;
        s.world_x = from[0];
        s.world_y = from[1];
        s.world_z = from[2];
    }

    /// `update(dt)`. `health.js:163-222`. `now` is `this.ctx.time.elapsed`.
    pub fn update(&mut self, dt: f64, now: f64, rig: &mut CameraRig, events: &EventBus) {
        let h = &HEALTH;

        // ---- regeneration ---------------------------------------------------
        let since = now - self.last_damage_time;
        if !self.dead && self.value < self.max && since > h.regen_delay {
            self.regenerating = true;
            // Ramp in so the recovery has a shape rather than a step.
            let ramp = clamp01((since - h.regen_delay) / h.regen_ramp);
            self.value = self.max.min(self.value + h.regen_rate * ramp * dt);
        } else if self.value >= self.max {
            self.regenerating = false;
        }

        // ---- pools ----------------------------------------------------------
        self.suppression = 0.0f64.max(self.suppression - h.suppression.decay * dt);
        self.hit_flash = approach(self.hit_flash, 0.0, h.effect.hit_flash_tau, dt);

        for i in self.indicators.iter_mut() {
            if !i.active {
                continue;
            }
            i.life += dt;
            if i.life > h.indicator_time {
                i.active = false;
            }
        }

        // ---- low-health treatment weight ------------------------------------
        let f = self.fraction();
        let target = clamp01((h.low_threshold - f) / h.low_threshold);
        self.effect = approach(self.effect, target, 0.25, dt);

        // ---- heartbeat ------------------------------------------------------
        if self.effect > 0.02 {
            let freq = lerp(
                h.effect.heartbeat_min,
                h.effect.heartbeat_max,
                clamp01(1.0 - f / h.low_threshold),
            );
            self.beat_phase += dt * freq;
            if self.beat_phase >= 1.0 {
                self.beat_phase -= self.beat_phase.floor();
                events.emit(
                    "player:heartbeat",
                    &HeartbeatEvent {
                        strength: self.effect,
                        fraction: f,
                    },
                );
            }
            // lub-dub: two gaussian thumps 0.16 of a cycle apart
            let t = self.beat_phase;
            let thump = |c: f64, w: f64, g: f64| g * (-((t - c) * (t - c)) / (2.0 * w * w)).exp();
            self.pulse = (thump(0.06, 0.035, 1.0) + thump(0.22, 0.045, 0.62)) * self.effect;
        } else {
            self.beat_phase = 0.0;
            self.pulse = 0.0;
        }

        // ---- suppression feel ------------------------------------------------
        if self.suppression > 0.02 {
            rig.add_trauma(self.suppression * h.suppression.shake_scale * dt);
        }

        self.emit_timer -= dt;
        if self.emit_timer <= 0.0 {
            self.emit_timer = 0.1;
            if (self.value - self.last_emit_health).abs() > 0.4 {
                self.emit_state(false, events);
            }
        }
    }

    /// `_emitState(force)`. `health.js:224-238`.
    ///
    /// `wasLow` is read off the retained payload *before* it is overwritten —
    /// which is why `state_payload` is a field rather than a fresh value.
    fn emit_state(&mut self, force: bool, events: &EventBus) {
        let was_low = self.state_payload.low;
        let s = HealthStateEvent {
            health: self.value,
            fraction: self.fraction(),
            low: self.low(),
            critical: self.critical(),
            regenerating: self.regenerating,
            suppression: self.suppression,
            dead: self.dead,
            changed_low_state: was_low != self.low(),
            forced: force,
        };
        self.last_emit_health = self.value;
        self.state_payload = s;
        events.emit("player:health", &s);
    }

    /// The last `player:health` payload, retained exactly as the source's
    /// `_statePayload` object is. Not in the source's public surface; the port
    /// needs a way to observe the object the source mutates in place.
    pub fn last_state(&self) -> HealthStateEvent {
        self.state_payload
    }
}
