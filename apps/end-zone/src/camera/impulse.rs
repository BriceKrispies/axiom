//! The additive camera impulse stack: bounded, seeded, decays to exactly
//! zero. Impulses are sampled per tick and ADDED to the smoothed base pose —
//! the base rig itself is never written, so shake can never drift the camera.

use axiom::prelude::Vec3;
use axiom_kernel::DeterministicRng;

/// Maximum simultaneous impulses (older ones are dropped first).
pub const MAX_IMPULSES: usize = 8;
/// Hard amplitude cap, yards.
pub const MAX_AMPLITUDE: f32 = 1.6;
/// Hard FOV-kick cap, degrees.
pub const MAX_FOV_KICK: f32 = 10.0;

/// One camera impulse: a decaying oscillation along a seeded direction with a
/// rotational wobble and a field-of-view kick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraImpulse {
    pub direction: Vec3,
    pub amplitude: f32,
    pub fov_kick: f32,
    /// Oscillation frequency, radians per tick.
    pub frequency: f32,
    pub age: u32,
    pub life: u32,
    /// Seed phase (derived from `app seed ^ event id` — never ambient).
    pub phase: f32,
}

impl CameraImpulse {
    /// Build a seeded impulse. Amplitudes are clamped to the hard caps.
    pub fn seeded(seed: u64, direction: Vec3, amplitude: f32, fov_kick: f32, life: u32) -> Self {
        let mut rng = DeterministicRng::seeded(seed);
        let phase = (rng.next_bounded(6283) as f32) / 1000.0;
        let frequency = 0.55 + (rng.next_bounded(400) as f32) / 1000.0;
        let dir = {
            let len = direction.length();
            if len > 1.0e-4 {
                direction.mul_scalar(1.0 / len)
            } else {
                Vec3::new(0.0, 1.0, 0.0)
            }
        };
        CameraImpulse {
            direction: dir,
            amplitude: amplitude.clamp(0.0, MAX_AMPLITUDE),
            fov_kick: fov_kick.clamp(0.0, MAX_FOV_KICK),
            frequency,
            age: 0,
            life: life.max(1),
            phase,
        }
    }

    /// The decay envelope: `(1 - t)²`, exactly `0` at end of life.
    fn envelope(&self) -> f32 {
        let t = (self.age as f32 / self.life as f32).clamp(0.0, 1.0);
        (1.0 - t) * (1.0 - t)
    }

    fn sample(&self) -> ImpulseSample {
        let envelope = self.envelope();
        let wave = (self.age as f32 * self.frequency + self.phase).sin();
        let offset = self.direction.mul_scalar(self.amplitude * envelope * wave);
        // A perpendicular wobble reads as rotation without touching the base.
        let wobble = Vec3::new(-self.direction.z, 0.35, self.direction.x).mul_scalar(
            self.amplitude
                * 0.55
                * envelope
                * (self.age as f32 * self.frequency * 1.7 + self.phase).cos(),
        );
        ImpulseSample {
            eye_offset: offset,
            target_offset: offset.mul_scalar(0.4).add(wobble),
            fov_kick: self.fov_kick * envelope,
        }
    }
}

/// The summed additive contribution this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpulseSample {
    pub eye_offset: Vec3,
    pub target_offset: Vec3,
    pub fov_kick: f32,
}

impl ImpulseSample {
    pub const ZERO: ImpulseSample = ImpulseSample {
        eye_offset: Vec3::ZERO,
        target_offset: Vec3::ZERO,
        fov_kick: 0.0,
    };
}

/// The bounded impulse stack.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImpulseStack {
    impulses: Vec<CameraImpulse>,
}

impl ImpulseStack {
    pub fn new() -> Self {
        ImpulseStack::default()
    }

    /// Push an impulse (dropping the oldest beyond the cap).
    pub fn push(&mut self, impulse: CameraImpulse) {
        if self.impulses.len() >= MAX_IMPULSES {
            self.impulses.remove(0);
        }
        self.impulses.push(impulse);
    }

    /// Advance one tick and return the summed additive sample. Expired
    /// impulses are removed; with none active the sample is exactly zero.
    pub fn step(&mut self) -> ImpulseSample {
        let mut sum = ImpulseSample::ZERO;
        for impulse in &self.impulses {
            let sample = impulse.sample();
            sum.eye_offset = sum.eye_offset.add(sample.eye_offset);
            sum.target_offset = sum.target_offset.add(sample.target_offset);
            sum.fov_kick += sample.fov_kick;
        }
        for impulse in &mut self.impulses {
            impulse.age += 1;
        }
        // Keep an impulse through `age == life`: its envelope is exactly zero
        // there, so the LAST sampled contribution is exactly zero — decay
        // provably reaches 0.0, never "small and then removed".
        self.impulses.retain(|impulse| impulse.age <= impulse.life);
        sum
    }

    /// Active impulse count (debug overlay row).
    pub fn active(&self) -> usize {
        self.impulses.len()
    }

    /// Remove everything (play reset).
    pub fn clear(&mut self) {
        self.impulses.clear();
    }
}
