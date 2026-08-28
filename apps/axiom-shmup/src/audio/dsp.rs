//! The DSP toolkit every synthesis voice is built from.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/dsp.js:1-331` — the whole file.
//!
//! The source's own framing, kept verbatim in spirit: *everything here is
//! written against `BaseAudioContext` so the exact same code path renders in an
//! `OfflineAudioContext` as in the live `AudioContext` — that is how this
//! subsystem is verified without a user gesture or a speaker.* The port's
//! [`AudioGraph`](super::graph::AudioGraph) is that same `BaseAudioContext`
//! seam, one step further: it records instead of rendering, so the whole toolkit
//! is exercised natively with no browser at all.
//!
//! Rules honoured here, from the source header:
//!  - no randomness except through an injected [`Rng`]
//!  - buffers and curve tables are built once and shared
//!  - every node a voice creates hangs off a single top gain, so the caller can
//!    disconnect the whole voice in one call when its tail has decayed

use crate::audio::graph::{AudioGraph, BufferId, FilterKind, NodeId, ParamRef, Wave};
use crate::rng::Rng;

/// `dsp.js:17` — m/s, 20 C dry air.
pub const SPEED_OF_SOUND: f64 = 343.0;

/* ------------------------------------------------------------------ */
/* Noise                                                              */
/* ------------------------------------------------------------------ */

/// The classic noise colours (`dsp.js:30-86`).
///
/// **Divergence:** the source keys these by string and both `fillNoise` and
/// `NoiseBank.source` carry a fall-back-to-white arm for a name they do not
/// recognise. An enum makes an unrecognised colour unconstructible, so those two
/// arms have no counterpart here — every call site in the source passes one of
/// these four literals, so nothing is lost but the defensive dead code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoiseKind {
    /// Flat spectrum — the raw material of cracks and hiss.
    White,
    /// −3 dB/oct, Paul Kellet's economy filter. City beds, tails.
    Pink,
    /// −6 dB/oct leaky integrator. Wind and rumble.
    Brown,
    /// Sparse impulsive grains. Debris and foliage.
    Crackle,
}

impl NoiseKind {
    /// The four colours, in the order `NoiseBank` builds them — which fixes both
    /// the buffer indices and the `rng` consumption order.
    pub const ALL: [NoiseKind; 4] = [
        NoiseKind::White,
        NoiseKind::Pink,
        NoiseKind::Brown,
        NoiseKind::Crackle,
    ];
}

/// Fill `out` with one of the noise colours (`dsp.js:30-86`).
///
/// `out` is `f32` because the destination is a `Float32Array` channel: the pink
/// and brown fills store a rounded value each sample, and the crackle fill reads
/// its own stores back to accumulate overlapping grains and again to normalise.
pub fn fill_noise(out: &mut [f32], kind: NoiseKind, rng: &mut Rng) {
    let n = out.len();
    match kind {
        NoiseKind::Pink => {
            let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
                (0.0f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            for slot in out.iter_mut() {
                let w = rng.signed();
                b0 = 0.99886 * b0 + w * 0.0555179;
                b1 = 0.99332 * b1 + w * 0.0750759;
                b2 = 0.969 * b2 + w * 0.153852;
                b3 = 0.8665 * b3 + w * 0.3104856;
                b4 = 0.55 * b4 + w * 0.5329522;
                b5 = -0.7616 * b5 - w * 0.016898;
                *slot = ((b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11) as f32;
                b6 = w * 0.115926;
            }
        }
        NoiseKind::Brown => {
            let mut last = 0.0f64;
            for slot in out.iter_mut() {
                let w = rng.signed();
                last = (last + 0.019 * w) * 0.9985;
                *slot = (last * 5.2) as f32;
            }
        }
        NoiseKind::Crackle => {
            out.fill(0.0);
            // Poisson-ish grain train; each grain is a decaying two-pole ping so
            // the buffer already has material character rather than pure clicks.
            let mut i: usize = 0;
            while i < n {
                i += 12 + (rng.u32() % 260) as usize;
                if i >= n {
                    break;
                }
                let amp = rng.range(0.25, 1.0) * if rng.float() < 0.12 { 1.8 } else { 0.7 };
                let w = rng.range(0.05, 0.45); // radians/sample
                let dec = (-rng.range(0.004, 0.05)).exp();
                let mut a = amp;
                let mut k = 0usize;
                while k < 220 && i + k < n {
                    out[i + k] = (f64::from(out[i + k]) + (w * k as f64).sin() * a) as f32;
                    a *= dec;
                    if a < 1e-4 {
                        break;
                    }
                    k += 1;
                }
            }
            // Keep the peak sane; grains overlap.
            let mut peak = 1e-6f64;
            for &v in out.iter() {
                peak = peak.max(f64::from(v).abs());
            }
            let g = 0.9 / peak;
            for slot in out.iter_mut() {
                *slot = (f64::from(*slot) * g) as f32;
            }
        }
        NoiseKind::White => {
            for slot in out.iter_mut() {
                *slot = rng.signed() as f32;
            }
        }
    }
}

/// A small library of long noise buffers (`dsp.js:93-125`).
///
/// Voices take a random slice at a random playback rate, which is what keeps
/// automatic fire from sounding like a loop while costing nothing at runtime.
#[derive(Debug, Clone)]
pub struct NoiseBank {
    /// Parallel to [`NoiseKind::ALL`], so a lookup is an index rather than a map
    /// probe — and so the buffer ids stay in the source's creation order.
    buffers: [BufferId; 4],
    duration: f64,
}

impl NoiseBank {
    /// `new NoiseBank(actx, rng, seconds)`. The source defaults `seconds` to
    /// 2.2; the two live call sites pass 2.4 (`index.js:166`) and 1.2
    /// (`selftest.js:72`), so the port takes it as a required argument.
    pub fn new(graph: &mut AudioGraph, rng: &mut Rng, seconds: f64) -> Self {
        let sr = graph.sample_rate();
        let len = ((sr * seconds).floor() as usize).max(1);
        let mut buffers = [BufferId(0); 4];
        for (slot, kind) in buffers.iter_mut().zip(NoiseKind::ALL) {
            let id = graph.create_buffer(2, len, sr);
            // Two decorrelated channels so wide beds get real stereo width.
            let mut ch0 = std::mem::take(&mut graph.buffer_mut(id).channels[0]);
            fill_noise(&mut ch0, kind, rng);
            graph.buffer_mut(id).channels[0] = ch0;
            let mut ch1 = std::mem::take(&mut graph.buffer_mut(id).channels[1]);
            fill_noise(&mut ch1, kind, rng);
            graph.buffer_mut(id).channels[1] = ch1;
            *slot = id;
        }
        NoiseBank {
            buffers,
            duration: len as f64 / sr,
        }
    }

    /// `buf.duration` of every bank buffer — they are all the same length.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn buffer(&self, kind: NoiseKind) -> BufferId {
        self.buffers[NoiseKind::ALL.iter().position(|&k| k == kind).unwrap_or(0)]
    }

    /// A one-shot source reading from a random offset. Caller starts/stops it.
    ///
    /// `rng: None` is the source's `rng ? … : 0` arm — an offset of zero.
    pub fn source(
        &self,
        graph: &mut AudioGraph,
        kind: NoiseKind,
        rng: Option<&mut Rng>,
        rate: f64,
        looping: bool,
    ) -> NodeId {
        let buffer = self.buffer(kind);
        let offset = rng.map_or(0.0, |r| r.range(0.0, self.duration * 0.7));
        let (loop_start, loop_end) = if looping {
            (0.0, self.duration)
        } else {
            (0.0, 0.0)
        };
        graph.create_buffer_source(buffer, rate, looping, loop_start, loop_end, offset)
    }

    /// The common case: a non-looping one-shot at `rate`.
    pub fn one_shot(
        &self,
        graph: &mut AudioGraph,
        kind: NoiseKind,
        rng: &mut Rng,
        rate: f64,
    ) -> NodeId {
        self.source(graph, kind, Some(rng), rate, false)
    }
}

/* ------------------------------------------------------------------ */
/* Envelopes                                                          */
/* ------------------------------------------------------------------ */

const FLOOR: f64 = 1e-4;

/// `dsp.js:138-140`. Guard: eleven subsystems can reach audio, and one NaN
/// position turns into a non-finite schedule time that throws inside Web Audio.
/// Envelopes refuse garbage instead of taking the whole frame down with them.
fn ok(t0: f64, peak: f64) -> bool {
    t0.is_finite() && peak.is_finite() && t0 >= 0.0
}

/// Instant-attack exponential decay — the workhorse for transients.
pub fn hit(g: &mut AudioGraph, param: ParamRef, t0: f64, peak: f64, decay: f64) -> f64 {
    if !ok(t0, peak) {
        return t0;
    }
    let p = peak.max(FLOOR * 4.0);
    g.set_value_at_time(param, p, t0);
    g.exponential_ramp_to_value_at_time(param, FLOOR, t0 + decay);
    g.set_value_at_time(param, 0.0, t0 + decay + 0.002);
    t0 + decay + 0.002
}

/// Attack/decay with an exponential contour on both halves.
pub fn ad(g: &mut AudioGraph, param: ParamRef, t0: f64, peak: f64, attack: f64, decay: f64) -> f64 {
    if !ok(t0, peak) {
        return t0;
    }
    let p = peak.max(FLOOR * 4.0);
    g.set_value_at_time(param, FLOOR, t0);
    if attack > 0.0008 {
        g.exponential_ramp_to_value_at_time(param, p, t0 + attack);
    } else {
        g.set_value_at_time(param, p, t0 + 0.0004);
    }
    g.exponential_ramp_to_value_at_time(param, FLOOR, t0 + attack + decay);
    g.set_value_at_time(param, 0.0, t0 + attack + decay + 0.002);
    t0 + attack + decay + 0.002
}

/// Full ADSR for sustained material (voices, wind gusts).
#[allow(clippy::too_many_arguments)]
pub fn adsr(
    g: &mut AudioGraph,
    param: ParamRef,
    t0: f64,
    peak: f64,
    a: f64,
    d: f64,
    s: f64,
    sustain_level: f64,
    r: f64,
) -> f64 {
    if !ok(t0, peak) {
        return t0;
    }
    let p = peak.max(FLOOR * 4.0);
    let sl = (p * sustain_level).max(FLOOR * 4.0);
    g.set_value_at_time(param, FLOOR, t0);
    g.exponential_ramp_to_value_at_time(param, p, t0 + a);
    g.exponential_ramp_to_value_at_time(param, sl, t0 + a + d);
    g.set_value_at_time(param, sl, t0 + a + d + s);
    g.exponential_ramp_to_value_at_time(param, FLOOR, t0 + a + d + s + r);
    g.set_value_at_time(param, 0.0, t0 + a + d + s + r + 0.002);
    t0 + a + d + s + r + 0.002
}

/// Exponential parameter sweep, guarded against zero/negative targets.
pub fn sweep(g: &mut AudioGraph, param: ParamRef, t0: f64, from: f64, to: f64, dur: f64) -> f64 {
    if !ok(t0, from) || !to.is_finite() || !dur.is_finite() {
        return t0;
    }
    g.set_value_at_time(param, from.max(1e-3), t0);
    g.exponential_ramp_to_value_at_time(param, to.max(1e-3), t0 + dur.max(0.001));
    t0 + dur
}

/* ------------------------------------------------------------------ */
/* Nodes                                                              */
/* ------------------------------------------------------------------ */

/// `dsp.js:190-197`. Note where the clamp lives: on the *constructed* value, so
/// a later `frequency` automation is free to sweep outside it — which several
/// voices do on purpose.
pub fn biquad(
    g: &mut AudioGraph,
    kind: FilterKind,
    freq: f64,
    q: f64,
    gain_db: f64,
) -> NodeId {
    let ceiling = 20000.0f64.min(g.sample_rate() * 0.48);
    let f = clamp(freq, 10.0, ceiling);
    g.create_biquad(kind, f, q, gain_db)
}

/// `biquad(actx, type, freq)` — the source's `Q = 0.7071` default.
pub fn biquad_default_q(g: &mut AudioGraph, kind: FilterKind, freq: f64) -> NodeId {
    biquad(g, kind, freq, 0.7071, 0.0)
}

pub fn gain(g: &mut AudioGraph, value: f64) -> NodeId {
    g.create_gain(value)
}

pub fn osc(g: &mut AudioGraph, wave: Wave, freq: f64) -> NodeId {
    g.create_oscillator(wave, freq, 0.0)
}

/* ------------------------------------------------------------------ */
/* Waveshaping                                                        */
/* ------------------------------------------------------------------ */

/// tanh-style saturation (`dsp.js:230-245`). `drive` 0 is nearly clean, 20 is
/// aggressive. `asym` adds even harmonics — that is what gives a muzzle blast
/// its "chuff" rather than a symmetric fuzz-pedal buzz.
///
/// The two-decimal cache key is the source's, and it is load-bearing: two
/// slightly different drives round to the same key and therefore *share a
/// curve*, so the first caller's exact drive is the one that shapes both. That
/// is invisible in the audio and visible in a graph diff, so it is reproduced
/// rather than tidied away.
pub fn saturation_curve(g: &mut AudioGraph, drive: f64, asym: f64) -> crate::audio::graph::CurveId {
    let key = format!("{drive:.2}:{asym:.2}");
    g.cached_curve(key, || {
        let n = 2048usize;
        let k = 1.0 + drive;
        let norm = k.tanh();
        (0..n)
            .map(|i| {
                let x = (i as f64 / (n - 1) as f64) * 2.0 - 1.0;
                let xa = x + asym * x * x * if x < 0.0 { -1.0 } else { 1.0 } * 0.5;
                ((k * xa).tanh() / norm) as f32
            })
            .collect()
    })
}

/// Hard-knee-free soft clip for the very last stage of the master bus
/// (`dsp.js:248-264`).
pub fn limiter_curve(g: &mut AudioGraph) -> crate::audio::graph::CurveId {
    g.cached_curve("__limit".to_string(), || {
        let n = 4096usize;
        (0..n)
            .map(|i| {
                let x = (i as f64 / (n - 1) as f64) * 2.0 - 1.0;
                // Cubic soft clip up to 0.66, then tanh — transparent below -6 dBFS.
                let a = x.abs();
                let y = if a < 0.66 {
                    x
                } else {
                    js_sign(x) * (0.66 + (1.0 - 0.66) * ((a - 0.66) / (1.0 - 0.66)).tanh())
                };
                (y * 0.985) as f32
            })
            .collect()
    })
}

/// `Math.sign` — three-valued, unlike [`f64::signum`]. `dsp.js` calls it
/// directly; the transcription lives once in [`crate::jsmath`], which is
/// pinned bit-for-bit against V8 (including `-0` and `NaN`, which the local
/// copy this replaced flattened to `+0`).
use crate::jsmath::sign as js_sign;

pub fn shaper(
    g: &mut AudioGraph,
    curve: crate::audio::graph::CurveId,
    oversample: &'static str,
) -> NodeId {
    g.create_wave_shaper(curve, oversample)
}

/* ------------------------------------------------------------------ */
/* Resonators                                                         */
/* ------------------------------------------------------------------ */

/// One partial of a [`struck_resonator`] bank (`dsp.js:281`'s
/// `{ f, q, g, decay }`).
///
/// `q`/`g`/`decay` are optional because the source defaults them with `??`.
/// Every in-game call site fills all four; the defaults are kept because they
/// are the documented contract of the function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partial {
    pub f: f64,
    pub q: Option<f64>,
    pub g: Option<f64>,
    pub decay: Option<f64>,
}

impl Partial {
    /// All four fields given — what every call site in the game does.
    pub const fn new(f: f64, q: f64, g: f64, decay: f64) -> Self {
        Partial {
            f,
            q: Some(q),
            g: Some(g),
            decay: Some(decay),
        }
    }

    /// Only the frequency; `q`, `g` and `decay` take their `??` defaults.
    pub const fn at(f: f64) -> Self {
        Partial {
            f,
            q: None,
            g: None,
            decay: None,
        }
    }
}

/// Excite a bank of high-Q bandpasses with a short noise burst
/// (`dsp.js:282-302`): the cheapest convincing model of a struck metal/glass/
/// wood object. Returns the sum node.
pub fn struck_resonator(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    t0: f64,
    partials: &[Partial],
    excite_dur: f64,
    excite_kind: NoiseKind,
) -> NodeId {
    let out = gain(g, 1.0);
    let rate = rng.range(0.85, 1.2);
    let src = bank.one_shot(g, excite_kind, rng, rate);
    let exc = gain(g, 0.0);
    hit(g, exc.gain(), t0, 1.0, excite_dur);
    g.connect(src, exc);
    for p in partials {
        let q = p.q.unwrap_or(22.0);
        let bp = biquad(g, FilterKind::Bandpass, p.f, q, 0.0);
        let vg = gain(g, 0.0);
        // A bandpass only passes f/Q of the excitation's bandwidth, so a high-Q
        // partial fed a 2 ms noise burst is ~20 dB quieter than a low-Q one.
        // Without this makeup every metallic sound in the game sits inaudibly
        // low in the mix.
        hit(
            g,
            vg.gain(),
            t0,
            p.g.unwrap_or(0.5) * q.sqrt() * 0.85,
            p.decay.unwrap_or(0.12),
        );
        g.connect(exc, bp);
        g.connect(bp, vg);
        g.connect(vg, out);
    }
    g.start_source(src, t0, excite_dur + 0.02);
    out
}

/// `struckResonator(actx, bank, rng, t0, partials, exciteDur)` — the source's
/// `exciteKind = 'white'` default, which every call site takes.
pub fn struck(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    t0: f64,
    partials: &[Partial],
    excite_dur: f64,
) -> NodeId {
    struck_resonator(g, bank, rng, t0, partials, excite_dur, NoiseKind::White)
}

/* ------------------------------------------------------------------ */
/* Misc                                                               */
/* ------------------------------------------------------------------ */

/// `dsp.js:308-310`. Not `f64::clamp`: this is the source's ternary chain, which
/// propagates a NaN `v` as `v` where `f64::clamp` panics on a NaN bound and
/// returns the *upper* bound for a NaN value.
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn db_to_gain(db: f64) -> f64 {
    10.0f64.powf(db / 20.0)
}

/// Semitone ratio — pitch jitter is expressed musically, not as a raw factor.
pub fn semis(n: f64) -> f64 {
    2.0f64.powf(n / 12.0)
}

/// Air absorption: how much high end survives `dist` metres of atmosphere
/// (`dsp.js:326-330`).
///
/// ~ −1.5 dB/100 m at 1 kHz, far more at 8 kHz. Tuned by ear against real
/// long-range gunfire recordings: 50 m still bright, 300 m is all boom.
pub fn air_cutoff(dist: f64) -> f64 {
    clamp(20500.0 / (1.0 + dist * 0.055), 260.0, 20000.0)
}
