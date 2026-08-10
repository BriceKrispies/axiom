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

use crate::course::specification::BoostTier;
use crate::sim::car::CarState;
use crate::sim::contact::Severity;
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

    /// The cue for starting a race from the pre-race screen.
    ///
    /// Scheduled directly rather than as a [`RaceEvent`], because it is not one:
    /// a `RaceEvent` is something that happened *in a race*, and this happens
    /// before there is one. A short rising arpeggio — unmistakably a go-ahead,
    /// and deliberately not an alarm.
    pub fn on_race_start(&mut self) {
        if !self.enabled {
            return;
        }
        self.blip(440.0, 0.14, 0.34);
        self.blip(880.0, 0.20, 0.30);
        self.blip(1_320.0, 0.26, 0.22);
    }

    /// Schedule the one-shot cue for a simulation event.
    pub fn on_event(&mut self, event: &RaceEvent) {
        if !self.enabled {
            return;
        }
        match event {
            RaceEvent::Impact { severity, strength, .. } => self.impact(*severity, *strength),
            RaceEvent::NearMiss { .. } => self.blip(1_180.0, 0.16, 0.35),
            // Pitched **up** the tier ladder, and longer with it. A player who
            // can hear which colour they just took has not had to look away
            // from the road to read the bar, which is the whole reason the
            // event carries the tier at all.
            RaceEvent::PickupCollected { tier, .. } => {
                let (hz, seconds) = match tier {
                    BoostTier::Small => (784.0, 0.16),
                    BoostTier::Medium => (988.0, 0.22),
                    BoostTier::Large => (1_318.0, 0.34),
                };
                self.blip(hz, seconds, 0.42);
                // The upper octave, softer: two tones read as a *chime* rather
                // than as the same blip a near miss uses, and the ear separates
                // "the course gave me this" from "I earned this".
                self.blip(hz * 2.0, seconds * 0.7, 0.20);
            }
            RaceEvent::BoostStarted => self.blip(220.0, 0.5, 0.45),
            RaceEvent::CountdownTick(_) => self.blip(660.0, 0.28, 0.5),
            RaceEvent::Go => self.blip(990.0, 0.6, 0.6),
            RaceEvent::Finished { .. } => self.blip(1_320.0, 1.1, 0.6),
            RaceEvent::Reset => self.blip(300.0, 0.22, 0.3),
            RaceEvent::DriftStarted | RaceEvent::WentOffRoad => {}
        }
    }

    /// A collision, voiced by severity.
    ///
    /// # What was wrong with the old one
    ///
    /// One sound served every contact: a **square** wave at 70–130 Hz, up to
    /// 0.4 s long, at up to 0.75 volume, with a 23 Hz LFO at 0.85 depth. Every
    /// one of those choices points the same way. A square wave is all odd
    /// harmonics, so a 70 Hz fundamental puts real energy at 210 and 350 Hz. An
    /// LFO at 23 Hz is below the pitch floor, so it is not heard as vibrato but
    /// as *amplitude pulsing* — twenty-three times a second, at near-full depth.
    /// A harsh buzz, pulsing, held for four tenths of a second, is not the sound
    /// of an impact; it is the sound of an alarm. And it fired **every fixed
    /// step** the boxes overlapped, so a graze along a car was that alarm at
    /// 60 Hz.
    ///
    /// # What replaces it
    ///
    /// A body and a detail, per severity. The body is a **sine** — a thud is a
    /// low-frequency displacement of air, and a sine is what that is — short,
    /// with a fast attack and a decay that is over before it can ring. The
    /// detail is a brief high triangle, the metal, quiet enough to colour the
    /// thud rather than compete with it. Neither has an LFO: the thing that made
    /// the old cue an alarm is simply gone. Pitch varies with `strength`, which
    /// is measured, so identical runs make identical noises.
    ///
    /// Rate limiting is structural rather than a matter for this function: a
    /// contact episode emits one fresh impact and then rate-limited scrape cues
    /// (see [`crate::sim::contact`]), so there is no path by which one collision
    /// can schedule two thuds.
    fn impact(&mut self, severity: Severity, strength: f32) {
        let strength = strength.clamp(0.0, 1.0);
        let voice = IMPACT_VOICES[severity.index()];
        // A harder hit is a *lower*, longer thud — bigger things resonate lower.
        let body_hz = voice.body_hz - voice.body_drop * strength;
        self.audio.play_tone(ToneSpec {
            wave: voice.body_wave,
            freq: Hertz::new(body_hz),
            duration: AudioSeconds::from_seconds(voice.body_seconds),
            envelope: Some(thump_envelope(voice.body_seconds)),
            lfo: None,
            volume: Ratio::finite_or_zero(voice.body_volume * (0.55 + 0.45 * strength)),
        });
        // The metallic detail: short, bright, and quiet. Its pitch rises with
        // the hit so a scrape tinkles and a crash clangs.
        self.audio.play_tone(ToneSpec {
            wave: Wave::Triangle,
            freq: Hertz::new(voice.detail_hz * (1.0 + DETAIL_RISE * strength)),
            duration: AudioSeconds::from_seconds(voice.detail_seconds),
            envelope: Some(thump_envelope(voice.detail_seconds)),
            lfo: None,
            volume: Ratio::finite_or_zero(voice.detail_volume * (0.5 + 0.5 * strength)),
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

/// One severity's impact voice: a low body and a brief metallic detail.
#[derive(Debug, Clone, Copy)]
struct ImpactVoice {
    body_wave: Wave,
    /// Body pitch (Hz) at zero strength.
    body_hz: f32,
    /// How far the body pitch falls (Hz) at full strength.
    body_drop: f32,
    body_seconds: f32,
    body_volume: f32,
    detail_hz: f32,
    detail_seconds: f32,
    detail_volume: f32,
}

/// The three impact voices, indexed by [`Severity::index`].
///
/// Read down the columns: everything gets lower, longer and louder as the
/// severity rises, and nothing gets *long* — the worst crash in the game is a
/// 0.22 s body under a 0.09 s clang, because a tail is what turns an impact into
/// an alarm. A scrape uses a **triangle** body rather than a sine, which is the
/// only place a harmonic is wanted: friction is a rasp, and at 0.06 volume for
/// 60 ms a rasp is a hiss.
const IMPACT_VOICES: [ImpactVoice; 3] = [
    // Scrape: quiet, short, metallic friction.
    ImpactVoice {
        body_wave: Wave::Triangle,
        body_hz: 2_300.0,
        body_drop: 700.0,
        body_seconds: 0.06,
        body_volume: 0.07,
        detail_hz: 3_400.0,
        detail_seconds: 0.04,
        detail_volume: 0.045,
    },
    // Bump: a compact thud with restrained metallic detail.
    ImpactVoice {
        body_wave: Wave::Sine,
        body_hz: 118.0,
        body_drop: 34.0,
        body_seconds: 0.13,
        body_volume: 0.30,
        detail_hz: 780.0,
        detail_seconds: 0.055,
        detail_volume: 0.10,
    },
    // Major crash: deeper and stronger, and still over quickly.
    ImpactVoice {
        body_wave: Wave::Sine,
        body_hz: 78.0,
        body_drop: 22.0,
        body_seconds: 0.22,
        body_volume: 0.50,
        detail_hz: 560.0,
        detail_seconds: 0.09,
        detail_volume: 0.18,
    },
];

/// How much the metallic detail's pitch rises across a severity's strength band.
const DETAIL_RISE: f32 = 0.35;

/// The envelope every impact shares: an attack fast enough to read as a hit, and
/// a decay that is over inside the tone's own duration.
///
/// `sustain` is zero on purpose. A sustained impact is a note; an impact with no
/// sustain is a hit.
fn thump_envelope(seconds: f32) -> Envelope {
    Envelope {
        attack: AudioSeconds::from_seconds(0.003),
        decay: AudioSeconds::from_seconds(seconds * 0.45),
        sustain: Ratio::finite_or_zero(0.0),
        release: AudioSeconds::from_seconds(seconds * 0.4),
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
            RaceEvent::Impact {
                severity: Severity::Bump,
                strength: 0.8,
                traffic: false,
                fresh: true,
            },
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

    /// The impact voices encode the design brief as an ordering, and the
    /// ordering is the thing a player actually hears.
    #[test]
    fn the_impact_voices_get_lower_louder_and_longer_with_severity_and_never_ring() {
        let voices = IMPACT_VOICES;
        let ladder = [Severity::Scrape, Severity::Bump, Severity::MajorCrash];
        assert_eq!(
            ladder.map(|s| s.index()),
            [0, 1, 2],
            "the table is indexed by the severity ladder"
        );
        // The body drops in pitch, rises in volume and lengthens with severity.
        let bodies = [voices[1], voices[2]];
        assert!(bodies[0].body_hz > bodies[1].body_hz, "a worse hit is deeper");
        assert!(bodies[0].body_volume < bodies[1].body_volume);
        assert!(bodies[0].body_seconds < bodies[1].body_seconds);
        // A scrape is a different creature: high, quiet, and the shortest of
        // the three, because friction is not a thud.
        assert!(voices[0].body_hz > voices[1].body_hz * 5.0, "a scrape is a hiss");
        assert!(voices[0].body_volume < voices[1].body_volume * 0.5, "and quiet");

        for (index, voice) in voices.iter().enumerate() {
            // **No long ringing tail.** This is the specific failure the old cue
            // had, and it is the one thing that turns an impact into an alarm.
            assert!(
                voice.body_seconds <= 0.25,
                "voice {index} rings for {} s",
                voice.body_seconds
            );
            assert!(voice.detail_seconds < voice.body_seconds, "the detail is briefer");
            // Volumes stay inside a bounded, non-shouting range.
            assert!((0.0..=0.6).contains(&voice.body_volume), "voice {index}");
            assert!(voice.detail_volume < voice.body_volume, "the detail colours, not competes");
            assert!(voice.body_drop < voice.body_hz, "pitch can never go negative");
            assert!(voice.detail_hz > voice.body_hz.min(200.0), "the detail is the bright part");
        }
    }

    /// **The alarm is gone.** The old cue's character came from three specific
    /// choices, and every one of them is now absent from every impact.
    #[test]
    fn no_impact_cue_pulses_buzzes_or_sustains() {
        for voice in IMPACT_VOICES {
            // A sustain of zero is what makes it a hit rather than a note.
            let envelope = thump_envelope(voice.body_seconds);
            assert_eq!(
                envelope.sustain,
                Ratio::finite_or_zero(0.0),
                "an impact that sustains is a note"
            );
            // The decay and release both finish inside the tone's own length.
            let tail = envelope.decay.seconds() + envelope.release.seconds();
            assert!(
                tail <= voice.body_seconds,
                "the envelope tail ({tail} s) outlasts the {} s tone",
                voice.body_seconds
            );
            // No square wave anywhere: odd harmonics on a low fundamental are
            // what made the old cue buzz.
            assert_ne!(voice.body_wave, Wave::Square, "a square body buzzes");
        }
    }

    /// The three severities genuinely schedule three different sounds, and a
    /// harder hit within one severity is a different sound again.
    #[test]
    fn each_severity_and_strength_schedules_a_distinguishable_cue() {
        let voiced = |severity: Severity, strength: f32| {
            let mut audio = RaceAudio::new();
            audio.enable(true);
            audio.on_event(&RaceEvent::Impact {
                severity,
                strength,
                traffic: true,
                fresh: true,
            });
            drained(&mut audio)
        };
        let scrape = voiced(Severity::Scrape, 0.5);
        let bump = voiced(Severity::Bump, 0.5);
        let crash = voiced(Severity::MajorCrash, 0.5);
        assert_ne!(scrape, bump, "a scrape and a bump sound different");
        assert_ne!(bump, crash, "a bump and a crash sound different");
        assert_ne!(scrape, crash);
        assert_ne!(scrape, silence(), "and all three make a sound");
        // Pitch varies with the measured strength, deterministically — the same
        // hit always sounds the same, a harder one does not.
        assert_ne!(
            voiced(Severity::Bump, 0.1),
            voiced(Severity::Bump, 0.9),
            "strength colours the cue"
        );
        assert_eq!(
            voiced(Severity::Bump, 0.42),
            voiced(Severity::Bump, 0.42),
            "and it is derived, never random"
        );
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
