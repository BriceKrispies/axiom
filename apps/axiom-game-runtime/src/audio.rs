//! Audio (SPEC-08 §4.2) composed into the bridge: the `loadSound` / `playSound` /
//! `playTone` / `playMusic` / `scheduleSound` / `stopVoice` / `setMasterVolume` /
//! `setMuted` surface the TS `HostBridge` audio methods project, every one
//! forwarding to the neutral [`axiom_audio::AudioApi`] core. Audio is
//! presentation-only (SPEC-08 §6): data flows sim/app → core → arm and is never
//! read back, so these all return opaque handles (or nothing) and no audio value
//! ever re-enters a deterministic read.
//!
//! ## Two arms, the same shape as `axiom-audio` / `axiom-windowing`
//! - The **neutral core** ([`AudioApi`]) is native-testable bookkeeping: it
//!   allocates monotonic handles, validates a [`ToneSpec`], folds master
//!   volume/mute, and accumulates a `ScheduledBatch`. The native slice test drives
//!   exactly this — same calls ⇒ same handle sequence.
//! - The **live Web Audio output** is the `#[cfg(target_arch = "wasm32")]` arm:
//!   [`AudioState::realize`] lazily opens a real `AudioContext` and drains the
//!   accumulated batch into it (`AudioApi::realize_into`). Every browser symbol
//!   stays inside that arm; the deterministic core never names one.
//!
//! ## Boundary convention (the established `scalar / string` rule)
//! Handles cross as their raw `u64` id (`f64` at the JS edge); a `Vec3`-free
//! audio call carries only scalars and strings — a sound url as a `String`, a
//! music playlist as a `Vec<String>`, per-voice options as the scalar
//! `(volume, pitch, looping)` / `(at, volume)` triples the contract's `SoundOptions`
//! / `ScheduleOptions` destructure into, and a tone as its
//! `(waveIndex, freq, duration, volume)` quadruple. The TS host edge
//! (`wasm-host.ts`) destructures the option records into these scalar args.
//!
//! ## Known facade scope (documented, not a shortcut)
//! `ToneSpec`'s optional `envelope` / `lfo` are **deferred** at this boundary: the
//! bridge always passes `None`, so a synthesized tone uses the core's default
//! amplitude shape. Wiring them is a pure additive extension of `play_tone`'s
//! scalar arg list (an ADSR quad + an LFO pair) once an author needs them; it adds
//! no new engine capability. `pitch` is typed `Ratio` by `axiom-audio`, so it is
//! carried verbatim as a finite ratio (the core records it; its resample meaning
//! is a wasm-arm choice).

use axiom_audio::{AudioApi, AudioSeconds, Hertz, MusicOpts, PlayOpts, SoundId, ToneSpec, VoiceId, Wave};
use axiom_kernel::Ratio;

use crate::GameBridge;

/// A finite ratio from a boundary scalar; a non-finite value falls back to unit
/// gain (the core additionally clamps volume to `0..1`).
fn ratio(value: f64) -> Ratio {
    Ratio::new(value as f32).unwrap_or_else(|_| Ratio::new(1.0).expect("1.0 is finite"))
}

/// The wave kind for a dense index (`0` sine, `1` square, `2` sawtooth, `3`
/// triangle); an out-of-range index falls back to sine — a table select, never a
/// branch.
fn wave_from_index(index: u32) -> Wave {
    [Wave::Sine, Wave::Square, Wave::Sawtooth, Wave::Triangle]
        .get(index as usize)
        .copied()
        .unwrap_or(Wave::Sine)
}

/// The audio state the bridge owns: the neutral mixer core and (on `wasm32`) the
/// lazily-opened live `AudioContext` the per-frame [`Self::realize`] drains into.
#[derive(Debug)]
pub(crate) struct AudioState {
    api: AudioApi,
    #[cfg(target_arch = "wasm32")]
    ctx: Option<web_sys::AudioContext>,
}

impl AudioState {
    /// A fresh mixer: full master volume, unmuted, nothing pending.
    pub(crate) fn new() -> Self {
        AudioState {
            api: AudioApi::new(),
            #[cfg(target_arch = "wasm32")]
            ctx: None,
        }
    }

    /// Register a sound to load from `url`, returning its raw handle (`loadSound`).
    fn load_sound(&mut self, url: &str) -> u64 {
        self.api.load_sound(url).raw()
    }

    /// Start a sample voice immediately (`playSound`), returning its raw handle.
    fn play_sound(&mut self, sound: u64, volume: f64, pitch: f64, looping: bool) -> u64 {
        self.api
            .play_sound(
                SoundId::from_raw(sound),
                PlayOpts { volume: ratio(volume), pitch: ratio(pitch), looping },
            )
            .raw()
    }

    /// Schedule a sample voice at audio-clock time `at` (`scheduleSound`).
    fn schedule_sound(&mut self, sound: u64, at: f64, volume: f64) -> u64 {
        self.api
            .schedule_sound(
                SoundId::from_raw(sound),
                AudioSeconds::from_seconds(at as f32),
                PlayOpts {
                    volume: ratio(volume),
                    pitch: Ratio::new(1.0).expect("1.0 is finite"),
                    looping: false,
                },
            )
            .raw()
    }

    /// Stop a previously started voice (`stopVoice`).
    fn stop_voice(&mut self, voice: u64) {
        self.api.stop_voice(VoiceId::from_raw(voice));
    }

    /// Stream a crossfaded music playlist as one voice (`playMusic`).
    fn play_music(&mut self, urls: &[String], looping: bool, crossfade: f64) -> u64 {
        let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
        self.api
            .play_music(
                &refs,
                MusicOpts { looping, crossfade: AudioSeconds::from_seconds(crossfade as f32) },
            )
            .raw()
    }

    /// Synthesize and play a tone (`playTone`); the optional envelope/lfo are
    /// deferred (see the module header), so a tone uses the core's default shape.
    fn play_tone(&mut self, wave_index: u32, freq: f64, duration: f64, volume: f64) -> u64 {
        self.api
            .play_tone(ToneSpec {
                wave: wave_from_index(wave_index),
                freq: Hertz::new(freq as f32),
                duration: AudioSeconds::from_seconds(duration as f32),
                envelope: None,
                lfo: None,
                volume: ratio(volume),
            })
            .raw()
    }

    /// Set the master output gain, clamped to `0..1` by the core (`setMasterVolume`).
    fn set_master_volume(&mut self, volume: f64) {
        self.api.set_master_volume(ratio(volume));
    }

    /// Mute or unmute the whole mix (`setMuted`).
    fn set_muted(&mut self, muted: bool) {
        self.api.set_muted(muted);
    }

    /// Drain the accumulated batch into the live `AudioContext` (browser playback),
    /// opening the context on first use. The deterministic core stays untouched;
    /// only the realized Web Audio nodes are a side effect. Native builds have no
    /// audio output, so this is a no-op there.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn realize(&mut self) {
        let ctx = self
            .ctx
            .get_or_insert_with(|| web_sys::AudioContext::new().expect("AudioContext is available"));
        let _ = self.api.realize_into(ctx);
    }

    /// No live audio output on native — the core accumulates its batch and the
    /// slice tests assert on the handles, not on realized sound.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn realize(&mut self) {}
}

impl GameBridge {
    /// Register a sound by url, returning its raw handle (`loadSound`).
    pub fn load_sound(&mut self, url: &str) -> u64 {
        self.audio.load_sound(url)
    }

    /// Start a sample voice immediately (`playSound`).
    pub fn play_sound(&mut self, sound: u64, volume: f64, pitch: f64, looping: bool) -> u64 {
        self.audio.play_sound(sound, volume, pitch, looping)
    }

    /// Schedule a sample voice at audio-clock time `at` (`scheduleSound`).
    pub fn schedule_sound(&mut self, sound: u64, at: f64, volume: f64) -> u64 {
        self.audio.schedule_sound(sound, at, volume)
    }

    /// Stop a previously started voice (`stopVoice`).
    pub fn stop_voice(&mut self, voice: u64) {
        self.audio.stop_voice(voice);
    }

    /// Stream a crossfaded music playlist (`playMusic`).
    pub fn play_music(&mut self, urls: &[String], looping: bool, crossfade: f64) -> u64 {
        self.audio.play_music(urls, looping, crossfade)
    }

    /// Synthesize and play a tone (`playTone`).
    pub fn play_tone(&mut self, wave_index: u32, freq: f64, duration: f64, volume: f64) -> u64 {
        self.audio.play_tone(wave_index, freq, duration, volume)
    }

    /// Set the master output gain (`setMasterVolume`).
    pub fn set_master_volume(&mut self, volume: f64) {
        self.audio.set_master_volume(volume);
    }

    /// Mute or unmute all output (`setMuted`).
    pub fn set_muted(&mut self, muted: bool) {
        self.audio.set_muted(muted);
    }

    /// Drain the audio batch into the live output (browser playback; no-op native).
    pub(crate) fn realize_audio(&mut self) {
        self.audio.realize();
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Register a sound by url (`loadSound`).
        #[wasm_bindgen(js_name = loadSound)]
        pub fn load_sound(&mut self, url: String) -> f64 {
            self.bridge.load_sound(&url) as f64
        }

        /// Start a sample voice immediately (`playSound`).
        #[wasm_bindgen(js_name = playSound)]
        pub fn play_sound(&mut self, sound: f64, volume: f64, pitch: f64, looping: bool) -> f64 {
            self.bridge.play_sound(sound as u64, volume, pitch, looping) as f64
        }

        /// Schedule a sample voice (`scheduleSound`).
        #[wasm_bindgen(js_name = scheduleSound)]
        pub fn schedule_sound(&mut self, sound: f64, at: f64, volume: f64) -> f64 {
            self.bridge.schedule_sound(sound as u64, at, volume) as f64
        }

        /// Stop a voice (`stopVoice`).
        #[wasm_bindgen(js_name = stopVoice)]
        pub fn stop_voice(&mut self, voice: f64) {
            self.bridge.stop_voice(voice as u64);
        }

        /// Stream a music playlist (`playMusic`).
        #[wasm_bindgen(js_name = playMusic)]
        pub fn play_music(&mut self, urls: Vec<String>, looping: bool, crossfade: f64) -> f64 {
            self.bridge.play_music(&urls, looping, crossfade) as f64
        }

        /// Synthesize and play a tone (`playTone`).
        #[wasm_bindgen(js_name = playTone)]
        pub fn play_tone(&mut self, wave_index: u32, freq: f64, duration: f64, volume: f64) -> f64 {
            self.bridge.play_tone(wave_index, freq, duration, volume) as f64
        }

        /// Set the master output gain (`setMasterVolume`).
        #[wasm_bindgen(js_name = setMasterVolume)]
        pub fn set_master_volume(&mut self, volume: f64) {
            self.bridge.set_master_volume(volume);
        }

        /// Mute or unmute all output (`setMuted`).
        #[wasm_bindgen(js_name = setMuted)]
        pub fn set_muted(&mut self, muted: bool) {
            self.bridge.set_muted(muted);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};

    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// Every voice/sound is a fresh, non-zero, monotonic handle; the core never
    /// hands back the invalid `0` sentinel for a real allocation.
    #[test]
    fn audio_allocates_distinct_non_zero_handles() {
        let mut b = bridge();
        let sound = b.load_sound("explosion.wav");
        assert_ne!(sound, 0);
        let voice = b.play_sound(sound, 0.8, 1.0, false);
        let other = b.play_sound(sound, 0.5, 2.0, true);
        assert_ne!(voice, 0);
        assert_ne!(voice, other);
        // The remaining play verbs also mint real voices and never panic.
        assert_ne!(b.play_tone(0, 440.0, 0.5, 0.9), 0);
        assert_ne!(b.play_music(&[String::from("a.ogg"), String::from("b.ogg")], true, 1.5), 0);
        assert_ne!(b.schedule_sound(sound, 2.0, 0.7), 0);
        // The control verbs are clean no-ops over the boundary.
        b.stop_voice(voice);
        b.set_master_volume(0.5);
        b.set_muted(true);
    }

    /// An out-of-range wave index is the table's sine fallback — a real voice, no
    /// panic — and every wave index resolves to a voice.
    #[test]
    fn play_tone_resolves_every_wave_index_and_falls_back_to_sine() {
        let mut b = bridge();
        (0..5u32).for_each(|index| {
            assert_ne!(b.play_tone(index, 220.0, 0.25, 1.0), 0);
        });
    }

    /// The neutral core is deterministic: two fresh bridges driven with the same
    /// audio calls hand back the identical handle sequence (SPEC-08 §6
    /// "same calls ⇒ same batch").
    #[test]
    fn audio_handle_allocation_is_deterministic() {
        let script = || -> Vec<u64> {
            let mut b = bridge();
            let s = b.load_sound("a.wav");
            vec![
                s,
                b.play_sound(s, 1.0, 1.0, false),
                b.play_tone(2, 330.0, 0.1, 0.5),
                b.schedule_sound(s, 1.0, 0.3),
                b.play_music(&[String::from("loop.ogg")], false, 0.0),
            ]
        };
        assert_eq!(script(), script());
    }

    /// `realize` is a clean no-op on native (no `AudioContext`): draining the
    /// batch each "frame" never disturbs the deterministic core, so handle
    /// allocation keeps marching monotonically across realize calls.
    #[test]
    fn realize_does_not_disturb_the_native_core() {
        let mut b = bridge();
        let first = b.play_tone(0, 440.0, 0.5, 1.0);
        b.realize_audio();
        b.realize_audio();
        // After two realize drains, the next voice is still a fresh, larger handle —
        // the core advanced exactly as if realize had not run.
        let next = b.play_tone(0, 440.0, 0.5, 1.0);
        assert_ne!(first, 0);
        assert!(next > first);
    }
}
