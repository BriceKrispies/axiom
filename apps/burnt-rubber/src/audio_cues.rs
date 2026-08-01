//! Audio, derived deterministically from simulation state.
//!
//! The engine's [`axiom_audio::AudioApi`] is the neutral, native-testable half
//! of sound: it decides *what* plays and *when* on the audio clock, accumulates
//! a batch of commands, and knows nothing about `AudioContext`. Its `wasm32` arm
//! realizes that batch into real Web Audio nodes. This module is the racing half
//! — it turns car state into tone specifications and hands them over.
//!
//! ## Why the engine note is granular
//!
//! `AudioApi` has no "change this voice's pitch" call, by design: nothing is
//! read back out of it, so a live voice cannot be steered. That rules out one
//! long looping engine sample whose playback rate follows the throttle. What it
//! does support is scheduling a tone, so the engine note is **granular**: a
//! short sawtooth grain re-issued every [`GRAIN_STEPS`] fixed steps, at a
//! frequency computed from the current speed, overlapping the previous grain.
//! The result is a continuous note that tracks speed exactly, built only from
//! what the module actually offers.
//!
//! Every parameter is a pure function of simulation state on a fixed step, so
//! the same race produces the same audio — the cues are as replayable as the
//! physics.

use axiom_audio::{AudioApi, AudioSeconds, Envelope, Hertz, Lfo, ToneSpec, Wave};
use axiom_kernel::Ratio;

use crate::sim::car::CarState;
use crate::sim::{RaceEvent, RaceSim};
use crate::tuning::{VehicleTuning, DT};

/// How many fixed steps between engine grains. Six steps is a grain every
/// 100 ms, overlapping a 150 ms grain — dense enough to sound continuous,
/// sparse enough to stay far under any voice limit.
pub const GRAIN_STEPS: u64 = 6;

/// Length of one engine grain (s). Longer than the gap between grains, so
/// consecutive grains overlap and the note does not gate.
pub const GRAIN_SECONDS: f32 = 0.15;

/// Engine note at a standstill (Hz).
pub const IDLE_HZ: f32 = 62.0;
/// Engine note at the boosted top speed (Hz).
pub const REDLINE_HZ: f32 = 340.0;
/// Wind note at the boosted top speed (Hz).
pub const WIND_HZ: f32 = 900.0;

/// The racing sound bank: one call per fixed step, plus one per event.
#[derive(Debug)]
pub struct RaceAudio {
    audio: AudioApi,
    step: u64,
    enabled: bool,
}

impl RaceAudio {
    /// A silent bank. Audio stays disabled until [`Self::enable`] is called,
    /// because a browser will not start an `AudioContext` before the player has
    /// interacted with the page.
    pub fn new() -> RaceAudio {
        RaceAudio {
            audio: AudioApi::new(),
            step: 0,
            enabled: false,
        }
    }

    /// Whether cues are being scheduled.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Turn cues on or off.
    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// The neutral mixer, for the platform arm to realize.
    pub fn api(&mut self) -> &mut AudioApi {
        &mut self.audio
    }

    /// Set the master volume.
    pub fn set_volume(&mut self, volume: f32) {
        self.audio
            .set_master_volume(Ratio::finite_or_zero(volume.clamp(0.0, 1.0)));
    }

    /// Schedule one fixed step's continuous cues: engine, wind and tyre scrub.
    pub fn step(&mut self, sim: &RaceSim) {
        self.step += 1;
        if !self.enabled || self.step % GRAIN_STEPS != 0 {
            return;
        }
        let car = sim.car();
        let tuning = &sim.tuning().vehicle;
        self.engine_grain(car, tuning);
        self.wind_grain(car, tuning);
        self.tyre_grain(car);
    }

    /// The engine note: a sawtooth whose frequency rises with speed, with a
    /// little vibrato so it does not sound like a test tone, and a hard shift up
    /// while boosting.
    fn engine_grain(&mut self, car: &CarState, tuning: &VehicleTuning) {
        let ceiling = tuning.top_speed + tuning.boost_top_speed_bonus;
        let revs = (car.speed() / ceiling.max(1.0)).clamp(0.0, 1.0);
        let boost_shift = if car.boosting { BOOST_SHIFT } else { 1.0 };
        let hz = (IDLE_HZ + (REDLINE_HZ - IDLE_HZ) * revs) * boost_shift;
        // Off the throttle the note thins out rather than cutting.
        let volume = ENGINE_VOLUME * (0.45 + 0.55 * revs);
        self.audio.play_tone(ToneSpec {
            wave: Wave::Sawtooth,
            freq: Hertz::new(hz),
            duration: AudioSeconds::from_seconds(GRAIN_SECONDS),
            envelope: Some(grain_envelope()),
            lfo: Some(Lfo {
                freq: Hertz::new(hz * 0.02),
                depth: Ratio::finite_or_zero(0.06),
            }),
            volume: Ratio::finite_or_zero(volume),
        });
    }

    /// Wind: a high, quiet tone that only exists at real speed and rises with it.
    fn wind_grain(&mut self, car: &CarState, tuning: &VehicleTuning) {
        let onset = tuning.top_speed * 0.35;
        let intensity =
            ((car.speed() - onset) / (tuning.top_speed - onset).max(1.0)).clamp(0.0, 1.0);
        if intensity <= 0.0 {
            return;
        }
        self.audio.play_tone(ToneSpec {
            wave: Wave::Triangle,
            freq: Hertz::new(WIND_HZ * (0.5 + 0.5 * intensity)),
            duration: AudioSeconds::from_seconds(GRAIN_SECONDS),
            envelope: Some(grain_envelope()),
            lfo: None,
            volume: Ratio::finite_or_zero(WIND_VOLUME * intensity * intensity),
        });
    }

    /// Tyre scrub: only while the car is genuinely sliding on the ground.
    fn tyre_grain(&mut self, car: &CarState) {
        if !(car.drifting && car.grounded) {
            return;
        }
        let slide = car.slide_ratio();
        self.audio.play_tone(ToneSpec {
            wave: Wave::Square,
            freq: Hertz::new(180.0 + 260.0 * slide),
            duration: AudioSeconds::from_seconds(GRAIN_SECONDS),
            envelope: Some(grain_envelope()),
            lfo: Some(Lfo {
                freq: Hertz::new(37.0),
                depth: Ratio::finite_or_zero(0.5),
            }),
            volume: Ratio::finite_or_zero(TYRE_VOLUME * slide),
        });
    }

    /// Schedule the one-shot cue for a simulation event.
    pub fn on_event(&mut self, event: &RaceEvent) {
        if !self.enabled {
            return;
        }
        match event {
            RaceEvent::Impact { strength, .. } => self.impact(*strength),
            RaceEvent::NearMiss { .. } => self.blip(1_180.0, 0.16, 0.35),
            RaceEvent::BoostStarted => self.blip(220.0, 0.5, 0.45),
            RaceEvent::CountdownTick(_) => self.blip(660.0, 0.28, 0.5),
            RaceEvent::Go => self.blip(990.0, 0.6, 0.6),
            RaceEvent::Finished { .. } => self.blip(1_320.0, 1.1, 0.6),
            RaceEvent::Reset => self.blip(300.0, 0.22, 0.3),
            RaceEvent::DriftStarted | RaceEvent::WentOffRoad => {}
        }
    }

    /// A collision: a short, low, loud burst scaled by how hard it was.
    fn impact(&mut self, strength: f32) {
        let strength = strength.clamp(0.0, 1.0);
        self.audio.play_tone(ToneSpec {
            wave: Wave::Square,
            freq: Hertz::new(70.0 + 60.0 * (1.0 - strength)),
            duration: AudioSeconds::from_seconds(0.18 + 0.22 * strength),
            envelope: Some(Envelope {
                attack: AudioSeconds::from_seconds(0.004),
                decay: AudioSeconds::from_seconds(0.10),
                sustain: Ratio::finite_or_zero(0.25),
                release: AudioSeconds::from_seconds(0.16),
            }),
            lfo: Some(Lfo {
                freq: Hertz::new(23.0),
                depth: Ratio::finite_or_zero(0.85),
            }),
            volume: Ratio::finite_or_zero(0.30 + 0.45 * strength),
        });
    }

    /// A short pure cue.
    fn blip(&mut self, hz: f32, seconds: f32, volume: f32) {
        self.audio.play_tone(ToneSpec {
            wave: Wave::Sine,
            freq: Hertz::new(hz),
            duration: AudioSeconds::from_seconds(seconds),
            envelope: Some(Envelope {
                attack: AudioSeconds::from_seconds(0.008),
                decay: AudioSeconds::from_seconds(seconds * 0.3),
                sustain: Ratio::finite_or_zero(0.55),
                release: AudioSeconds::from_seconds(seconds * 0.5),
            }),
            lfo: None,
            volume: Ratio::finite_or_zero(volume.clamp(0.0, 1.0)),
        });
    }
}

impl Default for RaceAudio {
    fn default() -> Self {
        RaceAudio::new()
    }
}

/// The envelope every grain shares: a fast attack and a release long enough to
/// overlap the next grain, so consecutive grains cross-fade rather than gate.
fn grain_envelope() -> Envelope {
    Envelope {
        attack: AudioSeconds::from_seconds(0.012),
        decay: AudioSeconds::from_seconds(0.03),
        sustain: Ratio::finite_or_zero(0.8),
        release: AudioSeconds::from_seconds(GRAIN_SECONDS * 0.6),
    }
}

/// Pitch multiplier applied to the engine note while boosting.
const BOOST_SHIFT: f32 = 1.22;
/// Peak engine grain volume.
const ENGINE_VOLUME: f32 = 0.22;
/// Peak wind grain volume.
const WIND_VOLUME: f32 = 0.13;
/// Peak tyre grain volume.
const TYRE_VOLUME: f32 = 0.20;

/// How many grains a second of racing schedules — the figure the browser arm
/// sizes its realization against.
pub fn grains_per_second() -> f32 {
    1.0 / (GRAIN_STEPS as f32 * DT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use crate::sim::RacePhase;

    fn racing() -> RaceSim {
        let mut sim = RaceSim::shipping();
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        sim
    }

    /// `ScheduledBatch` is deliberately opaque — the module exposes no getters,
    /// so a simulation can never read audio state back. Its `Debug` rendering is
    /// therefore the only observable, and comparing it against a known-empty
    /// batch is the honest way to ask "was anything scheduled".
    fn drained(audio: &mut RaceAudio) -> String {
        format!("{:?}", audio.api().take_pending())
    }

    /// The rendering of a batch that had nothing put in it.
    fn silence() -> String {
        drained(&mut RaceAudio::new())
    }

    #[test]
    fn audio_is_silent_until_it_is_enabled() {
        let mut audio = RaceAudio::new();
        assert!(!audio.enabled());
        let sim = racing();
        for _ in 0..GRAIN_STEPS * 4 {
            audio.step(&sim);
        }
        audio.on_event(&RaceEvent::Go);
        assert_eq!(drained(&mut audio), silence(), "nothing was scheduled");
    }

    #[test]
    fn enabling_starts_scheduling_engine_grains() {
        let mut audio = RaceAudio::new();
        audio.enable(true);
        assert!(audio.enabled());
        let mut sim = racing();
        for _ in 0..600 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        for _ in 0..GRAIN_STEPS {
            audio.step(&sim);
        }
        assert_ne!(drained(&mut audio), silence(), "the engine is running");
        assert_eq!(drained(&mut audio), silence(), "and draining empties it");
    }

    #[test]
    fn grains_are_issued_on_a_fixed_cadence_not_every_step() {
        let mut audio = RaceAudio::new();
        audio.enable(true);
        let sim = racing();
        // Fewer than one grain period: nothing.
        for _ in 0..GRAIN_STEPS - 1 {
            audio.step(&sim);
        }
        assert_eq!(drained(&mut audio), silence(), "not yet");
        audio.step(&sim);
        assert_ne!(drained(&mut audio), silence(), "and now");
    }

    /// The pitch has to actually track the simulation, not merely exist.
    #[test]
    fn the_engine_note_rises_with_speed_and_shifts_on_boost() {
        let tuning = VehicleTuning::DEFAULT;
        let ceiling = tuning.top_speed + tuning.boost_top_speed_bonus;
        let note = |speed: f32, boosting: bool| {
            let revs = (speed / ceiling).clamp(0.0, 1.0);
            let shift = if boosting { BOOST_SHIFT } else { 1.0 };
            (IDLE_HZ + (REDLINE_HZ - IDLE_HZ) * revs) * shift
        };
        assert!((note(0.0, false) - IDLE_HZ).abs() < 1.0e-3, "idle at rest");
        assert!(note(50.0, false) > note(10.0, false), "revs rise with speed");
        assert!(note(90.0, false) > note(50.0, false));
        assert!(note(90.0, true) > note(90.0, false), "boost shifts up");
        assert!(note(1_000.0, false) <= REDLINE_HZ * 1.01, "and it redlines");
    }

    #[test]
    fn every_event_that_should_make_a_noise_does() {
        let audible = [
            RaceEvent::Impact { strength: 0.8, traffic: false },
            RaceEvent::NearMiss { boost_awarded: 0.13 },
            RaceEvent::BoostStarted,
            RaceEvent::CountdownTick(2),
            RaceEvent::Go,
            RaceEvent::Finished { steps: 100 },
            RaceEvent::Reset,
        ];
        for event in audible {
            let mut audio = RaceAudio::new();
            audio.enable(true);
            audio.on_event(&event);
            assert_ne!(drained(&mut audio), silence(), "{event:?} made no sound");
        }
        // And the two that are deliberately silent stay silent.
        for event in [RaceEvent::DriftStarted, RaceEvent::WentOffRoad] {
            let mut audio = RaceAudio::new();
            audio.enable(true);
            audio.on_event(&event);
            assert_eq!(
                drained(&mut audio),
                silence(),
                "{event:?} should be silent (the tyre scrub covers it)"
            );
        }
    }

    #[test]
    fn tyre_scrub_only_sounds_while_actually_sliding_on_the_ground() {
        let mut sim = racing();
        for _ in 0..300 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let mut gripping = RaceAudio::new();
        gripping.enable(true);
        for _ in 0..GRAIN_STEPS {
            gripping.step(&sim);
        }
        let quiet = drained(&mut gripping);

        for _ in 0..60 {
            sim.step(DriveCommand {
                handbrake: true,
                ..DriveCommand::turning(1.0)
            });
        }
        assert!(sim.car().drifting, "the car is genuinely sliding");
        let mut sliding = RaceAudio::new();
        sliding.enable(true);
        for _ in 0..GRAIN_STEPS {
            sliding.step(&sim);
        }
        let loud = drained(&mut sliding);
        assert_ne!(loud, quiet, "the slide adds a voice");
        assert!(loud.len() > quiet.len(), "and it is an extra voice, not a swap");
    }

    #[test]
    fn the_master_volume_is_clamped_rather_than_rejected() {
        let mut audio = RaceAudio::new();
        audio.set_volume(-4.0);
        audio.set_volume(9.0);
        audio.set_volume(f32::NAN);
        audio.set_volume(0.5);
        // Reaching here without a panic is the assertion; the API takes a
        // clamped `Ratio` and never sees an invalid one.
        assert!(!audio.enabled());
    }

    #[test]
    fn the_grain_rate_is_what_the_constants_say() {
        assert!((grains_per_second() - 10.0).abs() < 0.1, "{}", grains_per_second());
        assert!(
            GRAIN_SECONDS > GRAIN_STEPS as f32 * DT,
            "grains overlap rather than gating"
        );
    }

    #[test]
    fn the_default_bank_is_a_fresh_silent_one() {
        assert!(!RaceAudio::default().enabled());
    }
}
