//! The juice stack: presentation effects spawned ONLY by typed simulation
//! events. Fixed capacity, bounded lifetimes, clamped amplitudes, seeded
//! variation (`app seed ^ stable event id`), decay to exactly zero — and no
//! path whatsoever back into simulation state.

use axiom::prelude::Vec3;

use crate::data::JuiceTuning;
use crate::events::{SimEvent, StampedEvent};
use crate::football::BallState;
use crate::identity::PlayerId;
use crate::presentation::snapshot::PresentationSnapshot;

/// What kind of effect one slot holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectKind {
    /// Turf dust burst (tackles, ground impacts).
    Dust,
    /// Expanding ground ring at an impact.
    ImpactRing,
    /// Directional speed streaks behind a hit.
    Streaks,
    /// Catch flash ring at the completion point.
    CatchFlash,
    /// Small release pulse at the throw.
    ThrowPulse,
    /// Transient field-plane wobble (offset applied to turf entities).
    FieldWobble,
    /// Pose compression on one player (landing squash + recoil).
    Squash { player: PlayerId },
}

/// One live effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effect {
    pub kind: EffectKind,
    pub origin: Vec3,
    pub direction: Vec3,
    /// Normalized strength `0..=1` (clamped at spawn).
    pub strength: f32,
    pub start_tick: u64,
    pub life_ticks: u32,
    /// `app seed ^ event id` — the ONLY source of visual variation.
    pub seed: u64,
}

impl Effect {
    /// Progress `0..=1` at `tick` (`1` = expired).
    pub fn progress(&self, tick: u64) -> f32 {
        let age = tick.saturating_sub(self.start_tick) as f32;
        (age / self.life_ticks.max(1) as f32).min(1.0)
    }
}

/// The bounded, event-driven effect stack plus the ball trail ring.
#[derive(Debug)]
pub struct JuiceStack {
    tuning: JuiceTuning,
    seed: u64,
    effects: Vec<Effect>,
    /// Recent airborne ball positions (newest last), bounded.
    trail: Vec<Vec3>,
    last_trail_tick: u64,
}

impl JuiceStack {
    pub fn new(seed: u64, tuning: JuiceTuning) -> Self {
        JuiceStack {
            tuning,
            seed,
            effects: Vec::new(),
            trail: Vec::new(),
            last_trail_tick: 0,
        }
    }

    /// The live effects.
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// The ball trail points (oldest first).
    pub fn trail(&self) -> &[Vec3] {
        &self.trail
    }

    /// The tuning the shapes are rendered with.
    pub fn tuning(&self) -> &JuiceTuning {
        &self.tuning
    }

    /// The squash factor for `player` this tick (max over active squash
    /// effects; decays to exactly zero).
    pub fn squash_for(&self, player: PlayerId, tick: u64) -> f32 {
        self.effects
            .iter()
            .filter_map(|e| match e.kind {
                EffectKind::Squash { player: p } if p == player => {
                    let t = e.progress(tick);
                    Some(self.tuning.squash_amplitude * e.strength * (1.0 - t) * (1.0 - t))
                }
                _ => None,
            })
            .fold(0.0, f32::max)
    }

    /// The field-plane wobble vertical offset this tick (yards; exactly zero
    /// with no active wobble).
    pub fn field_wobble(&self, tick: u64) -> f32 {
        self.effects
            .iter()
            .filter_map(|e| match e.kind {
                EffectKind::FieldWobble => {
                    let t = e.progress(tick);
                    let age = tick.saturating_sub(e.start_tick) as f32;
                    Some(
                        self.tuning.field_wobble_amplitude
                            * e.strength
                            * (1.0 - t)
                            * (1.0 - t)
                            * (age * 0.9).sin(),
                    )
                }
                _ => None,
            })
            .fold(0.0, |a, b| a + b)
            .clamp(
                -self.tuning.field_wobble_amplitude,
                self.tuning.field_wobble_amplitude,
            )
    }

    /// Observe this tick's events + snapshot: spawn effects, advance the
    /// trail, retire the expired. Reads the simulation only through the
    /// immutable snapshot; mutates only itself.
    pub fn step(&mut self, snapshot: &PresentationSnapshot, events: &[StampedEvent]) {
        let tick = snapshot.tick;
        for stamped in events {
            let seed = self.seed ^ stamped.id.0;
            match stamped.event {
                SimEvent::TackleContact {
                    contact_point,
                    contact_direction,
                    strength,
                    target,
                    ..
                } => {
                    self.spawn(
                        EffectKind::Dust,
                        contact_point,
                        contact_direction,
                        strength,
                        tick,
                        seed,
                        self.tuning.dust_life_ticks,
                    );
                    self.spawn(
                        EffectKind::Streaks,
                        contact_point,
                        contact_direction,
                        strength,
                        tick,
                        seed.rotate_left(8),
                        self.tuning.streak_life_ticks,
                    );
                    self.spawn(
                        EffectKind::Squash { player: target },
                        contact_point,
                        contact_direction,
                        strength,
                        tick,
                        seed.rotate_left(16),
                        self.tuning.squash_life_ticks,
                    );
                }
                SimEvent::GroundImpact {
                    player,
                    position,
                    strength,
                } => {
                    self.spawn(
                        EffectKind::Dust,
                        position,
                        Vec3::UNIT_Y,
                        strength,
                        tick,
                        seed,
                        self.tuning.dust_life_ticks,
                    );
                    self.spawn(
                        EffectKind::ImpactRing,
                        position,
                        Vec3::UNIT_Y,
                        strength,
                        tick,
                        seed.rotate_left(8),
                        self.tuning.ring_life_ticks,
                    );
                    self.spawn(
                        EffectKind::FieldWobble,
                        position,
                        Vec3::UNIT_Y,
                        strength,
                        tick,
                        seed.rotate_left(16),
                        self.tuning.field_wobble_life_ticks,
                    );
                    self.spawn(
                        EffectKind::Squash { player },
                        position,
                        Vec3::UNIT_Y,
                        strength,
                        tick,
                        seed.rotate_left(24),
                        self.tuning.squash_life_ticks,
                    );
                }
                SimEvent::Throw {
                    release, velocity, ..
                } => {
                    self.trail.clear();
                    // Flash-intensity accessibility gate: `0` spawns nothing.
                    if self.tuning.flash_scale > 0.0 {
                        self.spawn(
                            EffectKind::ThrowPulse,
                            release,
                            velocity,
                            0.6 * self.tuning.flash_scale,
                            tick,
                            seed,
                            self.tuning.flash_life_ticks,
                        );
                    }
                }
                SimEvent::CatchCompleted { player } => {
                    if self.tuning.flash_scale > 0.0 {
                        let at = snapshot.player(player).pos.add(Vec3::new(0.0, 1.4, 0.0));
                        self.spawn(
                            EffectKind::CatchFlash,
                            at,
                            Vec3::UNIT_Y,
                            0.8 * self.tuning.flash_scale,
                            tick,
                            seed,
                            self.tuning.flash_life_ticks,
                        );
                    }
                }
                SimEvent::PlayStarted { .. } | SimEvent::PlayReset => {
                    self.effects.clear();
                    self.trail.clear();
                }
                _ => {}
            }
        }

        // Ball trail while the pass is in the air.
        if matches!(snapshot.ball.state, BallState::Airborne { .. }) {
            if tick.saturating_sub(self.last_trail_tick)
                >= u64::from(self.tuning.trail_spacing_ticks)
            {
                self.last_trail_tick = tick;
                if self.trail.len() >= self.tuning.trail_points {
                    self.trail.remove(0);
                }
                self.trail.push(snapshot.ball.pos);
            }
        } else if !self.trail.is_empty() && snapshot.possession.is_some() {
            // Fade the trail out after the catch by draining it.
            self.trail.remove(0);
        }

        self.effects.retain(|e| e.progress(tick) < 1.0);
    }

    fn spawn(
        &mut self,
        kind: EffectKind,
        origin: Vec3,
        direction: Vec3,
        strength: f32,
        tick: u64,
        seed: u64,
        life: u32,
    ) {
        if self.effects.len() >= self.tuning.max_effects {
            self.effects.remove(0);
        }
        self.effects.push(Effect {
            kind,
            origin,
            direction,
            strength: strength.clamp(0.0, 1.0),
            start_tick: tick,
            life_ticks: life.max(1),
            seed,
        });
    }
}
