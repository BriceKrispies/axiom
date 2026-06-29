//! The single audio facade: pure, deterministic bookkeeping of *what plays and
//! when*, with no Web Audio object in sight.
//!
//! [`AudioApi`] allocates handles, validates/clamps a [`ToneSpec`], folds master
//! volume + mute into one effective gain, and accumulates an ordered
//! [`ScheduledBatch`] of commands. A `wasm32`-only arm (compiled out on native,
//! see [`web`]) drains that batch into a real `AudioContext`. Every decision the
//! core makes is data in / data out — branchless and 100% testable on native,
//! exactly like `axiom-windowing`'s presentation core.
//!
//! Determinism rule (§6): data flows **only** sim/app -> core -> arm. The core
//! exposes no getter a fixed update could sample (no "is playing", no "position")
//! — a [`ScheduledBatch`] is consumed by the platform arm and never read back
//! into a sim.

use axiom_kernel::Ratio;

use crate::ids::{
    AnalyserId, AudioInput, AudioSeconds, Band, BandLayout, Envelope, Hertz, Lfo, MusicOpts,
    PlayOpts, SoundId, ToneSpec, VoiceId,
};

// The `wasm32`-only live Web Audio arm: it realizes a `ScheduledBatch` into real
// `AudioContext` oscillator/gain/buffer nodes and (§13.1) opens a capture stream
// + `AnalyserNode`. Gated on wasm32 so none of it compiles (or is coverage-gated)
// on native; the deterministic, fully-covered core below stays browser-free. It
// adds no public surface — only further `impl AudioApi` blocks driven from the
// core's data.
#[cfg(target_arch = "wasm32")]
mod web;

/// A single realized audio command in a [`ScheduledBatch`]. The neutral
/// core->arm contract: ordered, value-only, and never read back. Crate-internal
/// (the platform arm and tests read it); callers receive only the opaque
/// `ScheduledBatch` that carries it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AudioCommand {
    /// Fetch + decode the sample behind `sound` from `url` (the arm owns the
    /// network + decode; the core only names the handle and forwards the url).
    Load {
        /// The handle the decoded buffer is cached under.
        sound: SoundId,
        /// Where to fetch the sample from.
        url: String,
    },
    /// Start (or schedule) one sample voice at audio-clock time `at`.
    PlaySample {
        /// The voice this play allocates.
        voice: VoiceId,
        /// The sample to play.
        sound: SoundId,
        /// When to start, on the audio clock (`ZERO` = as soon as possible).
        at: AudioSeconds,
        /// The voice volume (0..1), already clamped.
        volume: Ratio,
        /// The playback pitch ratio, recorded verbatim.
        pitch: Ratio,
        /// Whether the voice loops.
        looping: bool,
    },
    /// Stop a previously started voice.
    Stop {
        /// The voice to stop.
        voice: VoiceId,
    },
    /// Stream a music playlist as one voice.
    PlayMusic {
        /// The voice this playlist allocates.
        voice: VoiceId,
        /// The ordered track urls (the arm streams + crossfades them).
        tracks: Vec<String>,
        /// Whether the playlist loops at the end.
        looping: bool,
        /// The requested crossfade between consecutive tracks.
        crossfade: AudioSeconds,
    },
    /// Play a synthesized, validated tone as one voice.
    PlayTone {
        /// The voice this tone allocates.
        voice: VoiceId,
        /// The resolved (clamped, wave-labelled) tone.
        tone: ResolvedTone,
    },
    /// Open a live capture stream (§13.1). The arm performs the actual
    /// `getUserMedia`; the core only names the input handle, exactly once.
    OpenInput {
        /// The capture-stream handle this open allocates.
        input: AudioInput,
    },
    /// Create a frequency analyser over a capture `input` (§13.1). The arm builds
    /// the `AnalyserNode`; the core only names the handles. The analyser's live
    /// magnitudes never flow back through the core (§17.5 determinism wall).
    CreateAnalyser {
        /// The analyser handle this creation allocates.
        id: AnalyserId,
        /// The capture input the analyser observes.
        input: AudioInput,
    },
}

/// A validated, clamped tone ready for the oscillator arm. `wave` is the
/// Web-Audio oscillator label resolved by the branchless [`Wave::label`] table —
/// the one place the core "selects" on a wave kind, kept branchless by indexing.
///
/// [`Wave::label`]: crate::ids::Wave::label
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedTone {
    /// The oscillator-type label (`"sine"`/`"square"`/`"sawtooth"`/`"triangle"`).
    pub(crate) wave: &'static str,
    /// The base frequency.
    pub(crate) freq: Hertz,
    /// How long the tone sounds.
    pub(crate) duration: AudioSeconds,
    /// An optional ADSR amplitude envelope (sustain clamped).
    pub(crate) envelope: Option<ResolvedEnvelope>,
    /// An optional frequency-modulation LFO (depth clamped).
    pub(crate) lfo: Option<ResolvedLfo>,
    /// The tone volume (0..1), already clamped.
    pub(crate) volume: Ratio,
}

/// A resolved ADSR envelope with its sustain clamped to 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedEnvelope {
    /// Attack time.
    pub(crate) attack: AudioSeconds,
    /// Decay time.
    pub(crate) decay: AudioSeconds,
    /// Held amplitude (0..1), clamped.
    pub(crate) sustain: Ratio,
    /// Release time.
    pub(crate) release: AudioSeconds,
}

/// A resolved LFO with its depth clamped to 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedLfo {
    /// Modulation rate.
    pub(crate) freq: Hertz,
    /// Modulation depth (0..1), clamped.
    pub(crate) depth: Ratio,
}

/// The complete core->arm contract for one drain: the ordered commands to
/// realize plus the folded master gain to apply. Opaque to callers (they hold it
/// and hand it to the platform arm, or compare two for a determinism proof); its
/// contents are read only by the `wasm32` arm and tests. Nothing flows back.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledBatch {
    /// The ordered commands the arm realizes this drain.
    pub(crate) commands: Vec<AudioCommand>,
    /// The effective master gain (master volume folded with mute) the arm applies
    /// to its master node this drain.
    pub(crate) master_gain: Ratio,
}

/// The deterministic audio bookkeeper for one mix.
///
/// It owns the handle allocator, the master volume/mute mix state, and the
/// pending command list. Plain calls in, a replayable [`ScheduledBatch`] out — no
/// `AudioContext`, no nodes. Two `AudioApi`s driven with the same calls produce
/// byte-equal batches (the core's determinism property).
#[derive(Debug)]
pub struct AudioApi {
    /// Monotonic handle counter; `0` is the kernel's reserved null, so the first
    /// allocated handle is `1` and every handle is valid.
    next_handle: u64,
    /// Master volume (0..1), clamped on every set.
    master_volume: Ratio,
    /// Whether the mix is muted.
    muted: bool,
    /// Commands accumulated since the last drain.
    pending: Vec<AudioCommand>,
    /// The single opened capture input (§13.1), allocated lazily by
    /// [`Self::open_audio_input`] so opening is idempotent — one stream per mix.
    input: Option<AudioInput>,
}

/// Clamp a ratio into `0..1`, branchlessly. The input is already finite (it came
/// through [`Ratio::new`]), so `clamp(0.0, 1.0)` is total (no NaN, `min < max`)
/// and the re-wrap never fails.
fn clamp01(r: Ratio) -> Ratio {
    Ratio::new(r.get().clamp(0.0, 1.0)).expect("a clamped finite ratio is finite")
}

/// Resolve a public [`Envelope`] into a clamped [`ResolvedEnvelope`].
fn resolve_envelope(e: Envelope) -> ResolvedEnvelope {
    ResolvedEnvelope {
        attack: e.attack,
        decay: e.decay,
        sustain: clamp01(e.sustain),
        release: e.release,
    }
}

/// Resolve a public [`Lfo`] into a clamped [`ResolvedLfo`].
fn resolve_lfo(l: Lfo) -> ResolvedLfo {
    ResolvedLfo {
        freq: l.freq,
        depth: clamp01(l.depth),
    }
}

/// Validate + clamp a [`ToneSpec`] into a [`ResolvedTone`]. The wave kind becomes
/// its oscillator label by table index (branchless); the optional envelope/LFO
/// are resolved by `Option::map` (no `if let`); volume/sustain/depth are clamped.
fn resolve_tone(spec: ToneSpec) -> ResolvedTone {
    ResolvedTone {
        wave: spec.wave.label(),
        freq: spec.freq,
        duration: spec.duration,
        envelope: spec.envelope.map(resolve_envelope),
        lfo: spec.lfo.map(resolve_lfo),
        volume: clamp01(spec.volume),
    }
}

/// The low edge of the analyser's fixed audible range (§13.1 band layout), in Hz.
const BAND_MIN_HZ: f32 = 20.0;
/// The high edge of the analyser's fixed audible range (§13.1 band layout), in Hz.
const BAND_MAX_HZ: f32 = 20_000.0;

/// The log-spaced position of band edge `index` of `count`:
/// `min * (max/min)^(index/count)`. Computed in `f64` for accuracy and narrowed
/// to the `Hertz` boundary. Only ever called with `count >= 1` (the `0..count`
/// range is empty when `count == 0`), so the division is always well-defined.
fn band_edge(index: u32, count: u32) -> Hertz {
    let fraction = f64::from(index) / f64::from(count);
    let ratio = f64::from(BAND_MAX_HZ) / f64::from(BAND_MIN_HZ);
    Hertz::new((f64::from(BAND_MIN_HZ) * ratio.powf(fraction)) as f32)
}

impl AudioApi {
    /// A fresh mixer: full master volume, unmuted, nothing pending, handles at 0.
    pub fn new() -> Self {
        AudioApi {
            next_handle: 0,
            master_volume: Ratio::new(1.0).expect("1.0 is a finite ratio"),
            muted: false,
            pending: Vec::new(),
            input: None,
        }
    }

    /// Allocate the next valid raw handle value (monotonic, never `0`).
    fn alloc_handle(&mut self) -> u64 {
        self.next_handle += 1;
        self.next_handle
    }

    /// Register a sound to load from `url`, returning its handle immediately. The
    /// fetch + decode is the arm's job (§9 decode-ownership) — the core only names
    /// the handle and forwards the url through the batch.
    pub fn load_sound(&mut self, url: &str) -> SoundId {
        let sound = SoundId::from_raw(self.alloc_handle());
        self.pending.push(AudioCommand::Load {
            sound,
            url: url.to_string(),
        });
        sound
    }

    /// Start a sample voice immediately. Equivalent to scheduling it at the audio
    /// clock origin, so it shares one realization path with [`Self::schedule_sound`].
    pub fn play_sound(&mut self, id: SoundId, opts: PlayOpts) -> VoiceId {
        self.schedule_sound(id, AudioSeconds::ZERO, opts)
    }

    /// Schedule a sample voice to start at audio-clock time `at`, clamping its
    /// volume to 0..1 and recording its pitch verbatim.
    pub fn schedule_sound(&mut self, id: SoundId, at: AudioSeconds, opts: PlayOpts) -> VoiceId {
        let voice = VoiceId::from_raw(self.alloc_handle());
        self.pending.push(AudioCommand::PlaySample {
            voice,
            sound: id,
            at,
            volume: clamp01(opts.volume),
            pitch: opts.pitch,
            looping: opts.looping,
        });
        voice
    }

    /// Stop a previously started voice.
    pub fn stop_voice(&mut self, id: VoiceId) {
        self.pending.push(AudioCommand::Stop { voice: id });
    }

    /// Stream a music playlist as one voice. The track urls flow through the
    /// batch; the crossfade is recorded verbatim (its realization is a wasm-arm
    /// choice, §9).
    pub fn play_music(&mut self, urls: &[&str], opts: MusicOpts) -> VoiceId {
        let voice = VoiceId::from_raw(self.alloc_handle());
        let tracks = urls.iter().map(|u| (*u).to_string()).collect::<Vec<String>>();
        self.pending.push(AudioCommand::PlayMusic {
            voice,
            tracks,
            looping: opts.looping,
            crossfade: opts.crossfade,
        });
        voice
    }

    /// Play a synthesized, validated tone as one voice.
    pub fn play_tone(&mut self, spec: ToneSpec) -> VoiceId {
        let voice = VoiceId::from_raw(self.alloc_handle());
        self.pending.push(AudioCommand::PlayTone {
            voice,
            tone: resolve_tone(spec),
        });
        voice
    }

    /// Open the live capture input (§13.1), returning its handle. Opening is
    /// **idempotent** — one capture stream per mix: the first call allocates a
    /// handle and records one [`AudioCommand::OpenInput`]; later calls return the
    /// same handle and record nothing. (The real `getUserMedia` is the wasm arm,
    /// which later drains the `OpenInput` request.) Branchless: a fresh handle is
    /// allocated only when none exists (`Option::then`), the request is pushed only
    /// on that drain (`Option::into_iter().for_each`), and the resolved handle is
    /// the existing one or the fresh one (`Option::or`).
    pub fn open_audio_input(&mut self) -> AudioInput {
        let fresh = self
            .input
            .is_none()
            .then(|| AudioInput::from_raw(self.alloc_handle()));
        fresh
            .into_iter()
            .for_each(|input| self.pending.push(AudioCommand::OpenInput { input }));
        let resolved = self.input.or(fresh).expect("an input handle exists after open");
        self.input = Some(resolved);
        resolved
    }

    /// Create a frequency analyser over a capture `input` (§13.1), returning its
    /// handle and recording one [`AudioCommand::CreateAnalyser`]. The core only
    /// names the handles; the `wasm32` arm realizes the `AnalyserNode`. The
    /// analyser's live magnitudes never flow back through the core (§17.5 wall) —
    /// the only analysis math the core owns is the deterministic [`Self::band_layout`].
    pub fn create_analyser(&mut self, input: AudioInput) -> AnalyserId {
        let id = AnalyserId::from_raw(self.alloc_handle());
        self.pending.push(AudioCommand::CreateAnalyser { id, input });
        id
    }

    /// The deterministic, log-spaced frequency-band layout for an analyser of
    /// `count` bands across the audible range `[20 Hz, 20 kHz]` — the core's only
    /// analysis math. Each band spans an equal frequency *ratio* (the perceptual
    /// spacing an analyser wants); band `i` covers `[edge(i), edge(i+1))`. This is
    /// pure layout metadata (identical for a given `count`), **not** live analysis
    /// values, so it does not breach the §17.5 determinism wall.
    ///
    /// `count == 0` is rejected branchlessly: the range `0..0` is empty, so a
    /// zero-band request yields an empty [`BandLayout`] with no control flow.
    pub fn band_layout(count: u32) -> BandLayout {
        let bands = (0..count)
            .map(|i| Band {
                low: band_edge(i, count),
                high: band_edge(i + 1, count),
            })
            .collect::<Vec<Band>>();
        BandLayout::from_bands(bands)
    }

    /// Set the master volume, clamped to 0..1 and folded into the mix state.
    pub fn set_master_volume(&mut self, v: Ratio) {
        self.master_volume = clamp01(v);
    }

    /// Mute or unmute the whole mix.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// The effective master gain: master volume folded with mute, branchlessly
    /// (`* 0.0` when muted, `* 1.0` otherwise). The single scalar the arm applies.
    fn effective_master_gain(&self) -> Ratio {
        Ratio::new(self.master_volume.get() * f32::from(!self.muted))
            .expect("a finite volume times a 0/1 mute factor is finite")
    }

    /// Drain everything scheduled since the last call into a [`ScheduledBatch`]
    /// the platform arm realizes: the accumulated commands plus the current folded
    /// master gain. The pending list is emptied; the master mix state persists.
    pub fn take_pending(&mut self) -> ScheduledBatch {
        ScheduledBatch {
            commands: std::mem::take(&mut self.pending),
            master_gain: self.effective_master_gain(),
        }
    }
}

impl Default for AudioApi {
    fn default() -> Self {
        AudioApi::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Wave;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).expect("finite test ratio")
    }

    fn play_opts(volume: f32) -> PlayOpts {
        PlayOpts {
            volume: ratio(volume),
            pitch: ratio(1.0),
            looping: false,
        }
    }

    fn basic_tone(wave: Wave) -> ToneSpec {
        ToneSpec {
            wave,
            freq: Hertz::new(440.0),
            duration: AudioSeconds::from_seconds(0.5),
            envelope: None,
            lfo: None,
            volume: ratio(0.8),
        }
    }

    #[test]
    fn new_is_full_volume_unmuted_and_empty() {
        let mut a = AudioApi::new();
        let batch = a.take_pending();
        assert!(batch.commands.is_empty());
        assert_eq!(batch.master_gain, ratio(1.0));
        // Default matches new (compared through an observable drain), Debug works.
        let mut d = AudioApi::default();
        assert_eq!(d.take_pending(), batch);
        assert!(format!("{a:?}").starts_with("AudioApi"));
    }

    #[test]
    fn handles_are_monotonic_and_valid() {
        let mut a = AudioApi::new();
        let s0 = a.load_sound("a.ogg");
        let s1 = a.load_sound("b.ogg");
        let v = a.play_sound(s0, play_opts(1.0));
        // First handle is 1 (0 is the reserved null), each subsequent +1.
        assert_eq!(s0.raw(), 1);
        assert_eq!(s1.raw(), 2);
        assert_eq!(v.raw(), 3);
    }

    #[test]
    fn load_sound_forwards_the_url() {
        let mut a = AudioApi::new();
        let s = a.load_sound("kick.wav");
        let batch = a.take_pending();
        assert_eq!(
            batch.commands,
            vec![AudioCommand::Load {
                sound: s,
                url: "kick.wav".to_string(),
            }]
        );
    }

    #[test]
    fn play_sound_schedules_at_zero_with_clamped_volume() {
        let mut a = AudioApi::new();
        let s = a.load_sound("s.ogg");
        let _ = a.take_pending();
        let v = a.play_sound(s, play_opts(1.5)); // volume clamps to 1.0
        let batch = a.take_pending();
        assert_eq!(
            batch.commands,
            vec![AudioCommand::PlaySample {
                voice: v,
                sound: s,
                at: AudioSeconds::ZERO,
                volume: ratio(1.0),
                pitch: ratio(1.0),
                looping: false,
            }]
        );
    }

    #[test]
    fn schedule_sound_records_the_audio_clock_time() {
        let mut a = AudioApi::new();
        let s = a.load_sound("s.ogg");
        let _ = a.take_pending();
        let opts = PlayOpts {
            volume: ratio(-0.3), // clamps to 0.0
            pitch: ratio(2.0),
            looping: true,
        };
        let v = a.schedule_sound(s, AudioSeconds::from_seconds(2.5), opts);
        let batch = a.take_pending();
        assert_eq!(
            batch.commands,
            vec![AudioCommand::PlaySample {
                voice: v,
                sound: s,
                at: AudioSeconds::from_seconds(2.5),
                volume: ratio(0.0),
                pitch: ratio(2.0),
                looping: true,
            }]
        );
    }

    #[test]
    fn stop_voice_records_a_stop() {
        let mut a = AudioApi::new();
        let s = a.load_sound("s.ogg");
        let v = a.play_sound(s, play_opts(1.0));
        let _ = a.take_pending();
        a.stop_voice(v);
        let batch = a.take_pending();
        assert_eq!(batch.commands, vec![AudioCommand::Stop { voice: v }]);
    }

    #[test]
    fn play_music_collects_the_playlist() {
        let mut a = AudioApi::new();
        let v = a.play_music(
            &["one.ogg", "two.ogg"],
            MusicOpts {
                looping: true,
                crossfade: AudioSeconds::from_seconds(1.5),
            },
        );
        let batch = a.take_pending();
        assert_eq!(
            batch.commands,
            vec![AudioCommand::PlayMusic {
                voice: v,
                tracks: vec!["one.ogg".to_string(), "two.ogg".to_string()],
                looping: true,
                crossfade: AudioSeconds::from_seconds(1.5),
            }]
        );
    }

    #[test]
    fn play_tone_resolves_every_wave_kind() {
        // Cover every wave kind through the branchless label table.
        [
            (Wave::Sine, "sine"),
            (Wave::Square, "square"),
            (Wave::Sawtooth, "sawtooth"),
            (Wave::Triangle, "triangle"),
        ]
        .into_iter()
        .for_each(|(wave, label)| {
            let mut a = AudioApi::new();
            let v = a.play_tone(basic_tone(wave));
            let batch = a.take_pending();
            assert_eq!(
                batch.commands,
                vec![AudioCommand::PlayTone {
                    voice: v,
                    tone: ResolvedTone {
                        wave: label,
                        freq: Hertz::new(440.0),
                        duration: AudioSeconds::from_seconds(0.5),
                        envelope: None,
                        lfo: None,
                        volume: ratio(0.8),
                    },
                }]
            );
        });
    }

    #[test]
    fn play_tone_resolves_envelope_and_lfo_when_present_and_clamps_them() {
        let mut a = AudioApi::new();
        let spec = ToneSpec {
            wave: Wave::Square,
            freq: Hertz::new(220.0),
            duration: AudioSeconds::from_seconds(1.0),
            envelope: Some(Envelope {
                attack: AudioSeconds::from_seconds(0.01),
                decay: AudioSeconds::from_seconds(0.1),
                sustain: ratio(1.7), // clamps to 1.0
                release: AudioSeconds::from_seconds(0.2),
            }),
            lfo: Some(Lfo {
                freq: Hertz::new(5.0),
                depth: ratio(-0.4), // clamps to 0.0
            }),
            volume: ratio(0.5),
        };
        let v = a.play_tone(spec);
        let batch = a.take_pending();
        assert_eq!(
            batch.commands,
            vec![AudioCommand::PlayTone {
                voice: v,
                tone: ResolvedTone {
                    wave: "square",
                    freq: Hertz::new(220.0),
                    duration: AudioSeconds::from_seconds(1.0),
                    envelope: Some(ResolvedEnvelope {
                        attack: AudioSeconds::from_seconds(0.01),
                        decay: AudioSeconds::from_seconds(0.1),
                        sustain: ratio(1.0),
                        release: AudioSeconds::from_seconds(0.2),
                    }),
                    lfo: Some(ResolvedLfo {
                        freq: Hertz::new(5.0),
                        depth: ratio(0.0),
                    }),
                    volume: ratio(0.5),
                },
            }]
        );
    }

    #[test]
    fn master_volume_and_mute_fold_into_effective_gain() {
        let mut a = AudioApi::new();
        // Clamped above 1.0.
        a.set_master_volume(ratio(2.0));
        assert_eq!(a.take_pending().master_gain, ratio(1.0));
        // A plain 0.5 passes through.
        a.set_master_volume(ratio(0.5));
        assert_eq!(a.take_pending().master_gain, ratio(0.5));
        // Muting folds the gain to 0 without losing the stored volume.
        a.set_muted(true);
        assert_eq!(a.take_pending().master_gain, ratio(0.0));
        // Unmuting restores the stored 0.5.
        a.set_muted(false);
        assert_eq!(a.take_pending().master_gain, ratio(0.5));
    }

    #[test]
    fn take_pending_drains_but_keeps_master_state() {
        let mut a = AudioApi::new();
        a.set_master_volume(ratio(0.25));
        let s = a.load_sound("s.ogg");
        let _ = a.play_sound(s, play_opts(1.0));
        let first = a.take_pending();
        assert_eq!(first.commands.len(), 2);
        // Second drain is empty, but the master gain persists.
        let second = a.take_pending();
        assert!(second.commands.is_empty());
        assert_eq!(second.master_gain, ratio(0.25));
    }

    /// A fixed sequence of play_tone / schedule_sound / set_master_volume /
    /// set_muted replays to a byte-identical batch (the core's determinism
    /// property, asserted on the whole value).
    fn golden_run() -> ScheduledBatch {
        let mut a = AudioApi::new();
        let s = a.load_sound("hit.ogg");
        let _ = a.play_tone(basic_tone(Wave::Triangle));
        let _ = a.schedule_sound(s, AudioSeconds::from_seconds(0.75), play_opts(0.6));
        a.set_master_volume(ratio(0.4));
        a.set_muted(true);
        a.take_pending()
    }

    #[test]
    fn identical_call_sequences_replay_to_identical_batches() {
        let first = golden_run();
        let second = golden_run();
        // Whole-value equality (covers Clone + PartialEq across the variants used).
        assert_eq!(first, second.clone());
        assert_eq!(second, second.clone());
    }

    #[test]
    fn a_changed_call_sequence_produces_a_different_batch() {
        let baseline = golden_run();
        // Same calls but one different field => a different batch (exercises the
        // not-equal arms of the derived comparisons, incl. cross-variant).
        let mut a = AudioApi::new();
        let s = a.load_sound("hit.ogg");
        let _ = a.play_tone(basic_tone(Wave::Sine)); // wave differs
        let _ = a.schedule_sound(s, AudioSeconds::from_seconds(0.75), play_opts(0.6));
        a.set_master_volume(ratio(0.4));
        a.set_muted(true);
        assert_ne!(a.take_pending(), baseline);
        // A different command *kind* in the same slot also differs.
        let mut b = AudioApi::new();
        let _ = b.load_sound("hit.ogg"); // Load, not PlayTone
        let other = b.take_pending();
        assert_ne!(other, golden_run());
    }

    #[test]
    fn commands_are_debug_printable_for_every_variant() {
        // Build one of every command kind and confirm the Debug contract renders
        // each variant (covers the derived Debug arms).
        let mut a = AudioApi::new();
        let s = a.load_sound("s.ogg");
        let v = a.play_sound(s, play_opts(1.0));
        a.stop_voice(v);
        let _ = a.play_music(&["m.ogg"], MusicOpts {
            looping: false,
            crossfade: AudioSeconds::ZERO,
        });
        let _ = a.play_tone(basic_tone(Wave::Sine));
        let input = a.open_audio_input();
        let _ = a.create_analyser(input);
        let rendered = format!("{:?}", a.take_pending());
        [
            "Load",
            "PlaySample",
            "Stop",
            "PlayMusic",
            "PlayTone",
            "OpenInput",
            "CreateAnalyser",
            "ScheduledBatch",
        ]
        .into_iter()
        .for_each(|needle| {
            assert!(rendered.contains(needle), "Debug missing `{needle}`: {rendered}");
        });
    }

    #[test]
    fn open_audio_input_is_idempotent_and_records_open_once() {
        let mut a = AudioApi::new();
        // First open allocates a handle (the first valid handle is 1) and records
        // exactly one OpenInput command.
        let first = a.open_audio_input();
        assert_eq!(first.raw(), 1);
        let batch = a.take_pending();
        assert_eq!(batch.commands, vec![AudioCommand::OpenInput { input: first }]);
        // Re-opening returns the same handle and records nothing: one stream / mix.
        let second = a.open_audio_input();
        assert_eq!(second, first);
        assert!(a.take_pending().commands.is_empty());
    }

    #[test]
    fn create_analyser_allocates_distinct_handles_and_records_the_request() {
        let mut a = AudioApi::new();
        let input = a.open_audio_input();
        let _ = a.take_pending();
        let an0 = a.create_analyser(input);
        let an1 = a.create_analyser(input);
        // Analyser handles are monotonic and distinct (input=1, then 2, then 3).
        assert_eq!(an0.raw(), 2);
        assert_eq!(an1.raw(), 3);
        assert_ne!(an0, an1);
        let batch = a.take_pending();
        assert_eq!(
            batch.commands,
            vec![
                AudioCommand::CreateAnalyser { id: an0, input },
                AudioCommand::CreateAnalyser { id: an1, input },
            ]
        );
    }

    #[test]
    fn band_layout_produces_ordered_log_spaced_bands_and_rejects_zero() {
        // A zero-band request is rejected branchlessly: empty layout, no edges.
        assert!(AudioApi::band_layout(0).bands().is_empty());
        assert_eq!(AudioApi::band_layout(0).count(), 0);

        // `count` bands => `count` bands whose edges chain contiguously and ascend
        // across the fixed audible range [20 Hz, 20 kHz].
        let layout = AudioApi::band_layout(4);
        let bands = layout.bands();
        assert_eq!(layout.count(), 4);
        assert_eq!(bands.len(), 4);
        // First low edge is 20 Hz; last high edge is 20 kHz (the range ends).
        assert!((bands[0].low.get() - 20.0).abs() < 1e-3);
        assert!((bands[3].high.get() - 20_000.0).abs() < 1e-2);
        // Each band's high edge is the next band's low edge (contiguous), and every
        // edge strictly ascends (log spacing over a positive range).
        bands.windows(2).for_each(|pair| {
            assert_eq!(pair[0].high, pair[1].low);
        });
        bands.iter().for_each(|b| {
            assert!(b.high.get() > b.low.get());
        });
        // Equal frequency *ratios* per band (the log-spacing property): every
        // band spans the same multiplicative factor (here 20000/20 = 1000, ^(1/4)).
        let factor = bands[0].high.get() / bands[0].low.get();
        bands.iter().for_each(|b| {
            assert!((b.high.get() / b.low.get() - factor).abs() < 1e-3);
        });
    }

    #[test]
    fn band_layout_is_deterministic_for_a_given_count() {
        // Pure layout math: same count => byte-equal layout (the determinism the
        // core owns; live magnitudes never come from here).
        assert_eq!(AudioApi::band_layout(8), AudioApi::band_layout(8));
        assert_ne!(AudioApi::band_layout(8), AudioApi::band_layout(16));
    }
}
