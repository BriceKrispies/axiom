//! The audio module's **identity & value vocabulary** — the pure value-type
//! nouns a caller must name to drive the [`AudioApi`](crate::AudioApi) facade.
//!
//! Per Module Law #8 these are re-exported from `lib.rs` alongside the single
//! behavioral facade via one `pub use ids::{…}` line. They carry no behavior
//! beyond construction/inspection: opaque handles ([`SoundId`], [`VoiceId`],
//! [`AudioInput`]), the audio-clock newtype ([`AudioSeconds`]), the frequency
//! newtype ([`Hertz`]), and the synthesis description ([`ToneSpec`] with its
//! [`Wave`] discriminant, [`Envelope`], and [`Lfo`]) plus the playback option
//! records ([`PlayOpts`], [`MusicOpts`]).
//!
//! Two type walls live here on purpose:
//! - [`AudioSeconds`] is the **audio clock**, deliberately *not* a sim `Tick` or
//!   the sim `Seconds`: a scheduling time can never be confused with a tick.
//! - volumes / sustain / LFO depth are kernel [`Ratio`]s, never naked `f32`,
//!   and frequencies are [`Hertz`], so no bare float crosses the public surface.

use axiom_kernel::HandleId;

/// Define an opaque, presentation-side audio handle as a newtype over the
/// kernel's [`HandleId`]. Each is a distinct type so a sound handle can never be
/// passed where a voice handle is expected. Construction (`from_raw`) and the raw
/// accessor (`raw`, which the wasm playback arm uses to marshal across the
/// boundary) are the entire surface — these are nouns, not behavior.
macro_rules! audio_handle {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(HandleId);

        impl $name {
            /// Wrap a raw kernel handle value as this audio handle.
            pub const fn from_raw(raw: u64) -> Self {
                $name(HandleId::from_raw(raw))
            }

            /// The raw kernel handle value backing this handle.
            pub const fn raw(self) -> u64 {
                self.0.raw()
            }
        }
    };
}

audio_handle! {
    /// A loaded sound sample. Returned by `load_sound`, consumed by
    /// `play_sound` / `schedule_sound`.
    SoundId
}

audio_handle! {
    /// A playing (or scheduled) voice — one sound/tone/music instance. Returned
    /// by every `play_*` / `schedule_*` call, consumed by `stop_voice`.
    VoiceId
}

audio_handle! {
    /// A live audio capture stream (microphone / system), §13.1. Produced only by
    /// the `wasm32` capture arm; it is part of the vocabulary so callers can name
    /// what `createAnalyser` consumes.
    AudioInput
}

/// A time on the **audio clock**, in real seconds — the clock the platform
/// `AudioContext` runs on, independent of the fixed sim tick. A distinct newtype
/// so a schedule time can never be mistaken for a `Tick`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioSeconds(f32);

impl AudioSeconds {
    /// The audio-clock origin — "as soon as possible" for an immediate play.
    pub const ZERO: AudioSeconds = AudioSeconds(0.0);

    /// A point on the audio clock, in seconds.
    pub const fn from_seconds(seconds: f32) -> Self {
        AudioSeconds(seconds)
    }

    /// The underlying second count.
    pub const fn seconds(self) -> f32 {
        self.0
    }
}

/// A frequency, in Hertz. Negative and NaN inputs are sanitized to `0.0` (a
/// silent, finite tone) so no invalid frequency crosses the boundary — the
/// branchless `max(0.0)` does both (`NaN.max(0.0) == 0.0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hertz(f32);

impl Hertz {
    /// A frequency in Hz, clamped to a finite, non-negative value.
    pub fn new(hz: f32) -> Self {
        Hertz(hz.max(0.0))
    }

    /// The underlying frequency in Hz.
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// The oscillator wave kind, carried as a **data discriminant** on [`ToneSpec`].
/// Selecting on a wave is therefore a table index ([`Wave::label`] /
/// [`Wave::index`]), never a `match` — which is exactly what keeps the synthesis
/// core branchless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Wave {
    /// A pure sine tone.
    Sine = 0,
    /// A square wave.
    Square = 1,
    /// A sawtooth wave.
    Sawtooth = 2,
    /// A triangle wave.
    Triangle = 3,
}

impl Wave {
    /// The wave's canonical index `0..4` (its data discriminant).
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The Web-Audio oscillator-type label for this wave, by **table index** on
    /// the discriminant — the one place the core "selects" on a wave, kept
    /// branchless by indexing rather than matching.
    pub fn label(self) -> &'static str {
        ["sine", "square", "sawtooth", "triangle"][self.index() as usize]
    }
}

/// An ADSR amplitude envelope for a synthesized tone. `sustain` is a [`Ratio`]
/// (the held level, 0..1); attack/decay/release are audio-clock durations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Envelope {
    /// Time to ramp from silence to full amplitude.
    pub attack: AudioSeconds,
    /// Time to fall from full amplitude to the sustain level.
    pub decay: AudioSeconds,
    /// The held amplitude level (0..1), clamped on resolution.
    pub sustain: axiom_kernel::Ratio,
    /// Time to fall from the sustain level back to silence.
    pub release: AudioSeconds,
}

/// A low-frequency oscillator modulating a tone's frequency. `depth` is a
/// [`Ratio`] (0..1 modulation amount), clamped on resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lfo {
    /// The modulation rate.
    pub freq: Hertz,
    /// The modulation depth (0..1), clamped on resolution.
    pub depth: axiom_kernel::Ratio,
}

/// A neutral synthesis description: a validated tone built entirely from data.
/// The `wave` field is the discriminant; the optional `envelope` / `lfo` are
/// present-or-absent as data, so resolving a `ToneSpec` never branches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneSpec {
    /// The oscillator wave kind (data discriminant).
    pub wave: Wave,
    /// The base frequency.
    pub freq: Hertz,
    /// How long the tone sounds.
    pub duration: AudioSeconds,
    /// An optional ADSR amplitude envelope.
    pub envelope: Option<Envelope>,
    /// An optional frequency-modulation LFO.
    pub lfo: Option<Lfo>,
    /// The tone's own volume (0..1), clamped on resolution.
    pub volume: axiom_kernel::Ratio,
}

/// Per-voice options for sample playback. `volume`/`pitch` are [`Ratio`]s; the
/// core records them verbatim (clamping `volume` to 0..1) and never interprets
/// `pitch` — its realization (resample vs preserve-pitch) is a wasm-arm choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayOpts {
    /// The voice volume (0..1), clamped on resolution.
    pub volume: axiom_kernel::Ratio,
    /// The playback pitch ratio (1.0 = unchanged), recorded verbatim.
    pub pitch: axiom_kernel::Ratio,
    /// Whether the voice loops.
    pub looping: bool,
}

/// Options for a streamed music playlist. The core records the requested
/// `crossfade` seconds; the equal-power-vs-linear realization is a wasm-arm
/// choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MusicOpts {
    /// Whether the playlist loops at the end.
    pub looping: bool,
    /// The crossfade duration between consecutive tracks.
    pub crossfade: AudioSeconds,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_round_trip_and_are_distinct_types() {
        let s = SoundId::from_raw(7);
        let v = VoiceId::from_raw(7);
        let i = AudioInput::from_raw(9);
        assert_eq!(s.raw(), 7);
        assert_eq!(v.raw(), 7);
        assert_eq!(i.raw(), 9);
        // Same-type equality holds; Debug renders the wrapper name.
        assert_eq!(s, SoundId::from_raw(7));
        assert_ne!(s, SoundId::from_raw(8));
        assert_eq!(v, VoiceId::from_raw(7));
        assert_ne!(i, AudioInput::from_raw(1));
        assert!(format!("{s:?}").starts_with("SoundId"));
        assert!(format!("{v:?}").starts_with("VoiceId"));
        assert!(format!("{i:?}").starts_with("AudioInput"));
        // Copy semantics: using the value twice is fine.
        let copy = s;
        assert_eq!(copy, s);
    }

    #[test]
    fn audio_seconds_is_a_plain_clock_value() {
        assert_eq!(AudioSeconds::ZERO.seconds(), 0.0);
        assert_eq!(AudioSeconds::from_seconds(1.5).seconds(), 1.5);
        assert_eq!(AudioSeconds::from_seconds(1.5), AudioSeconds::from_seconds(1.5));
        assert!(format!("{:?}", AudioSeconds::ZERO).starts_with("AudioSeconds"));
    }

    #[test]
    fn hertz_sanitizes_negative_and_nan_to_zero() {
        assert_eq!(Hertz::new(440.0).get(), 440.0);
        assert_eq!(Hertz::new(-5.0).get(), 0.0);
        assert_eq!(Hertz::new(f32::NAN).get(), 0.0);
        assert_eq!(Hertz::new(440.0), Hertz::new(440.0));
    }

    #[test]
    fn every_wave_has_a_stable_index_and_label() {
        // Cover every wave kind's data dispatch (the branchless table).
        assert_eq!(Wave::Sine.index(), 0);
        assert_eq!(Wave::Square.index(), 1);
        assert_eq!(Wave::Sawtooth.index(), 2);
        assert_eq!(Wave::Triangle.index(), 3);
        assert_eq!(Wave::Sine.label(), "sine");
        assert_eq!(Wave::Square.label(), "square");
        assert_eq!(Wave::Sawtooth.label(), "sawtooth");
        assert_eq!(Wave::Triangle.label(), "triangle");
        assert_eq!(Wave::Sine, Wave::Sine);
        assert_ne!(Wave::Sine, Wave::Square);
    }
}
