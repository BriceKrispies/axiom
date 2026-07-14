//! Deterministic effect shapes: pure functions from an [`Effect`] + tick to a
//! bounded list of instance transforms. Variation comes from the effect's
//! seed through `DeterministicRng` — never a per-frame random value — so the
//! same impact event always produces the same dust.

use axiom::prelude::Vec3;
use axiom_kernel::DeterministicRng;
use axiom_math::{Quat, Transform};

use crate::data::JuiceTuning;

use super::juice::{Effect, EffectKind};

/// Which pooled material an instance uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectMaterial {
    Dust,
    Ring,
    Streak,
    Flash,
    Trail,
}

/// One renderable effect instance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectInstance {
    pub transform: Transform,
    pub material: EffectMaterial,
}

/// Append this effect's instances at `tick` (bounded by the tuning caps).
pub fn effect_instances(
    effect: &Effect,
    tick: u64,
    tuning: &JuiceTuning,
    out: &mut Vec<EffectInstance>,
) {
    let t = effect.progress(tick);
    if t >= 1.0 {
        return;
    }
    let fade = (1.0 - t) * (1.0 - t);
    match effect.kind {
        EffectKind::Dust => dust(effect, t, fade, tuning, out),
        EffectKind::ImpactRing => ring(effect, t, fade, tuning, out),
        EffectKind::Streaks => streaks(effect, t, fade, tuning, out),
        EffectKind::CatchFlash | EffectKind::ThrowPulse => flash(effect, t, fade, out),
        // Wobble and squash contribute through the scene/rig, not instances.
        EffectKind::FieldWobble | EffectKind::Squash { .. } => {}
    }
}

/// A seeded unit angle stream for one effect.
fn rng_of(effect: &Effect, salt: u64) -> DeterministicRng {
    DeterministicRng::seeded(effect.seed ^ salt)
}

fn dust(effect: &Effect, t: f32, fade: f32, tuning: &JuiceTuning, out: &mut Vec<EffectInstance>) {
    let mut rng = rng_of(effect, 0xD057);
    for _ in 0..tuning.dust_particles {
        let angle = (rng.next_bounded(6283) as f32) / 1000.0;
        let pace = 0.5 + (rng.next_bounded(500) as f32) / 1000.0;
        let radius = tuning.dust_radius * effect.strength.max(0.25) * t * pace;
        let rise = (t * (1.0 - t) * 4.0) * (0.5 + pace * 0.6) * effect.strength;
        let size = (0.16 + 0.14 * effect.strength) * fade * pace;
        let center = effect.origin.add(Vec3::new(
            angle.cos() * radius,
            0.08 + rise,
            angle.sin() * radius,
        ));
        out.push(EffectInstance {
            transform: Transform::new(center, Quat::IDENTITY, Vec3::new(size, size, size)),
            material: EffectMaterial::Dust,
        });
    }
}

fn ring(effect: &Effect, t: f32, fade: f32, tuning: &JuiceTuning, out: &mut Vec<EffectInstance>) {
    let segments = 12usize;
    let radius = tuning.ring_radius * (0.3 + 0.7 * effect.strength) * t;
    let thickness = 0.10 * fade + 0.02;
    for segment in 0..segments {
        let angle = segment as f32 / segments as f32 * core::f32::consts::TAU;
        let center = effect
            .origin
            .add(Vec3::new(angle.cos() * radius, 0.05, angle.sin() * radius));
        let seg_len = radius * 0.55 + 0.05;
        out.push(EffectInstance {
            transform: Transform::new(
                center,
                Quat::from_euler_xyz(0.0, -angle, 0.0),
                Vec3::new(seg_len, thickness, thickness),
            ),
            material: EffectMaterial::Ring,
        });
    }
}

fn streaks(
    effect: &Effect,
    t: f32,
    fade: f32,
    tuning: &JuiceTuning,
    out: &mut Vec<EffectInstance>,
) {
    let mut rng = rng_of(effect, 0x57EA);
    let dir = {
        let d = Vec3::new(effect.direction.x, 0.0, effect.direction.z);
        let len = d.length();
        if len > 1.0e-4 {
            d.mul_scalar(1.0 / len)
        } else {
            Vec3::UNIT_Z
        }
    };
    let yaw = dir.x.atan2(dir.z);
    for index in 0..tuning.streak_count {
        let spread = ((rng.next_bounded(1000) as f32) / 1000.0 - 0.5) * 1.2;
        let lift = 0.4 + (rng.next_bounded(900) as f32) / 1000.0;
        let side = Vec3::new(dir.z, 0.0, -dir.x).mul_scalar(spread);
        let along = dir.mul_scalar(-(0.6 + index as f32 * 0.35) * (0.4 + t));
        let length = (1.1 * effect.strength + 0.3) * fade;
        out.push(EffectInstance {
            transform: Transform::new(
                effect
                    .origin
                    .add(side)
                    .add(along)
                    .add(Vec3::new(0.0, lift, 0.0)),
                Quat::from_euler_xyz(0.0, yaw, 0.0),
                Vec3::new(0.05, 0.05, length.max(0.02)),
            ),
            material: EffectMaterial::Streak,
        });
    }
}

fn flash(effect: &Effect, t: f32, fade: f32, out: &mut Vec<EffectInstance>) {
    let size = (0.25 + t * 1.6 * effect.strength) * fade + 0.02;
    out.push(EffectInstance {
        transform: Transform::new(
            effect.origin,
            Quat::from_euler_xyz(0.0, t * 2.4, 0.0),
            Vec3::new(size, size, size),
        ),
        material: EffectMaterial::Flash,
    });
}

/// The ball-trail instances (oldest points smallest).
pub fn trail_instances(trail: &[Vec3], out: &mut Vec<EffectInstance>) {
    let count = trail.len().max(1) as f32;
    for (index, point) in trail.iter().enumerate() {
        let age = (index as f32 + 1.0) / count;
        let size = 0.05 + age * 0.13;
        out.push(EffectInstance {
            transform: Transform::new(*point, Quat::IDENTITY, Vec3::new(size, size, size)),
            material: EffectMaterial::Trail,
        });
    }
}
