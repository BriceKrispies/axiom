//! Procedural impulse responses, and the listener's space classifier.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/ir.js:1-251` — the whole file.
//!
//! There are no `.wav` files in the project, so every reverb is a synthesized
//! impulse response rendered into a buffer at load time and handed to a
//! convolver. An IR here is built from three physically motivated parts:
//!
//!   1. **discrete early reflections** — computed from a box-ish room's image
//!      sources, each one filtered by the absorption of the wall it bounced off.
//!      These are what tell you the size and shape of a room.
//!   2. **a diffuse late field** — noise whose density ramps in and whose
//!      envelope decays exponentially, with a *frequency dependent* decay so the
//!      high end dies first (real rooms always do).
//!   3. **a slap/flutter component** — for streets and corridors, regular-ish
//!      repeats that give a gunshot its characteristic "crack-tack-tack" tail.
//!
//! [`generate_ir`] is dependency-free: it takes a sample rate, an [`IrSpec`] and
//! an [`Rng`], and returns sample buffers. Nothing about it is a browser
//! concern — the convolver only appears at the very edge, in
//! [`Mixer::build_reverbs`](crate::audio::mixer::Mixer::build_reverbs). That is
//! what lets `tests/audio_port.rs` compare every sample of a rendered IR against
//! the same IR rendered by the original JavaScript under Node.

use crate::audio::dsp::clamp;
use crate::audio::graph::{AudioBuffer, AudioGraph, BufferId};
use crate::rng::Rng;

/// The blendable space names, in a fixed order (`ir.js:40`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Space {
    Tight,
    Room,
    Street,
    Tunnel,
    Open,
}

impl Space {
    /// `SPACE_KEYS` — the fixed order the weights are normalised over.
    pub const ALL: [Space; 5] = [
        Space::Tight,
        Space::Room,
        Space::Street,
        Space::Tunnel,
        Space::Open,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Space::Tight => "tight",
            Space::Room => "room",
            Space::Street => "street",
            Space::Tunnel => "tunnel",
            Space::Open => "open",
        }
    }

    /// The spec this space renders from.
    pub fn spec(self) -> &'static IrSpec {
        &IR_SPECS[Space::ALL.iter().position(|&s| s == self).unwrap_or(0)]
    }
}

/// `ir.js:24-37`'s `IRSpec` typedef.
#[derive(Debug, Clone, PartialEq)]
pub struct IrSpec {
    /// Total length.
    pub seconds: f64,
    /// −60 dB time of the diffuse field.
    pub rt60: f64,
    /// Seconds before the first reflection.
    pub predelay: f64,
    /// 0..1, how much faster the high end decays.
    pub hf_damp: f64,
    /// 0..1, spectral tilt of the whole tail.
    pub bright: f64,
    /// 0..1, 0 = discrete taps only, 1 = dense wash.
    pub diffusion: f64,
    /// 0..1, inter-channel decorrelation.
    pub width: f64,
    /// Early reflection delays in seconds.
    pub taps: &'static [f64],
    /// Overall early reflection level.
    pub tap_gain: f64,
    /// Number of regular flutter repeats.
    pub slaps: u32,
    /// Spacing of the flutter repeats.
    pub slap_time: f64,
}

/// The five spaces the probe can blend between (`ir.js:46-77`), in
/// [`Space::ALL`] order.
pub static IR_SPECS: [IrSpec; 5] = [
    // Small hard room: bathroom-tile slap, almost no tail. Indoors, tight.
    IrSpec {
        seconds: 0.5,
        rt60: 0.34,
        predelay: 0.0022,
        hf_damp: 0.45,
        bright: 0.62,
        diffusion: 0.55,
        width: 0.4,
        taps: &[0.0035, 0.0061, 0.0092, 0.0134, 0.0177, 0.0231, 0.0288],
        tap_gain: 0.85,
        slaps: 0,
        slap_time: 0.0,
    },
    // Concrete room / warehouse interior.
    IrSpec {
        seconds: 1.5,
        rt60: 1.05,
        predelay: 0.006,
        hf_damp: 0.55,
        bright: 0.5,
        diffusion: 0.8,
        width: 0.6,
        taps: &[0.009, 0.0143, 0.021, 0.0296, 0.0381, 0.0492, 0.0613, 0.078],
        tap_gain: 0.6,
        slaps: 0,
        slap_time: 0.0,
    },
    // Street canyon: two parallel façades — slapback plus a medium tail.
    IrSpec {
        seconds: 2.0,
        rt60: 1.45,
        predelay: 0.012,
        hf_damp: 0.42,
        bright: 0.56,
        diffusion: 0.62,
        width: 0.85,
        taps: &[
            0.017, 0.029, 0.046, 0.063, 0.088, 0.112, 0.147, 0.19, 0.24,
        ],
        tap_gain: 0.72,
        slaps: 7,
        slap_time: 0.058,
    },
    // Long corridor / tunnel: strong flutter echo, dark.
    IrSpec {
        seconds: 2.2,
        rt60: 1.8,
        predelay: 0.004,
        hf_damp: 0.7,
        bright: 0.3,
        diffusion: 0.5,
        width: 0.35,
        taps: &[0.006, 0.012, 0.019, 0.027, 0.037, 0.049, 0.064],
        tap_gain: 0.9,
        slaps: 12,
        slap_time: 0.031,
    },
    // Open ground: only far, dark, sparse returns — the rolling boom.
    IrSpec {
        seconds: 2.8,
        rt60: 1.15,
        predelay: 0.05,
        hf_damp: 0.9,
        bright: 0.16,
        diffusion: 0.9,
        width: 1.0,
        taps: &[0.07, 0.115, 0.18, 0.26, 0.35, 0.48, 0.62],
        tap_gain: 0.3,
        slaps: 0,
        slap_time: 0.0,
    },
];

/// Render one IR (`ir.js:83-158`). Stereo, decorrelated channels, peak-
/// normalised then trimmed to a sane send level so swapping spaces never
/// changes perceived loudness much.
///
/// Pure: sample rate in, samples out. No context, no node, no browser.
pub fn generate_ir(sample_rate: f64, rng: &mut Rng, spec: &IrSpec) -> AudioBuffer {
    let sr = sample_rate;
    let len = ((spec.seconds * sr).floor() as usize).max(64);
    let mut channels = vec![vec![0.0f32; len], vec![0.0f32; len]];

    for (ch, d) in channels.iter_mut().enumerate() {
        // Channel-specific jitter gives width without comb filtering the centre.
        let wob = 1.0 + if ch == 0 { -1.0 } else { 1.0 } * spec.width * 0.06;

        /* ---- diffuse late field ------------------------------------- */
        // Two one-pole lowpasses in series, whose cutoff falls with time, model
        // a frequency-dependent RT60 much more cheaply than a filterbank.
        let (mut lp1, mut lp2, mut hp, mut hp_prev) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let decay_k = 1e-3f64.ln() / (spec.rt60 * sr); // per-sample ln amplitude
        for (i, slot) in d.iter_mut().enumerate() {
            let t = i as f64 / sr;
            // Build-up: energy ramps in over the predelay + a few ms of diffusion.
            let build = clamp(
                (t - spec.predelay) / (0.004 + spec.diffusion * 0.05),
                0.0,
                1.0,
            );
            let env = (decay_k * i as f64).exp() * build * build;
            if env < 1e-6 {
                *slot = 0.0;
                continue;
            }
            let mut x = rng.signed();
            // Sparse early / dense late: thin the noise out near the onset.
            if rng.float() > 0.25 + 0.75 * clamp(t / (spec.rt60 * 0.5), 0.0, 1.0) {
                x *= 0.25;
            }
            // Falling cutoff -> high end dies first.
            let norm = clamp(t / spec.rt60, 0.0, 1.4);
            let a = clamp(
                0.9 * (1.0 - spec.hf_damp * norm) * spec.bright + 0.06,
                0.02,
                0.95,
            );
            lp1 += a * (x - lp1);
            lp2 += a * (lp1 - lp2);
            // Gentle DC/rumble removal so the convolver never pumps the sub bus.
            let y = lp2;
            hp = y - hp_prev + 0.995 * hp;
            hp_prev = y;
            *slot = (hp * env * spec.diffusion) as f32;
        }

        /* ---- discrete early reflections ------------------------------ */
        for (k, tap) in spec.taps.iter().enumerate() {
            let tt = (spec.predelay + tap) * wob * rng.range(0.97, 1.03);
            let idx = (tt * sr).floor() as usize;
            if idx + 8 >= len {
                continue;
            }
            // 1/r falloff plus per-bounce wall absorption.
            let g = (spec.tap_gain / (1.0 + k as f64 * 0.55))
                * rng.range(0.65, 1.0)
                * if rng.float() < 0.5 { -1.0 } else { 1.0 };
            // Smear each tap over a few samples: real walls are not mirrors.
            let smear = 3 + (spec.diffusion * 22.0).floor() as usize;
            let mut acc = 0.0f64;
            for s in 0..smear {
                if idx + s >= len {
                    break;
                }
                acc = acc * 0.6 + rng.signed() * 0.4;
                let w = 1.0 - s as f64 / smear as f64;
                let add = g * if s == 0 { 1.0 } else { acc * w * w };
                d[idx + s] = (f64::from(d[idx + s]) + add) as f32;
            }
        }

        /* ---- flutter / slapback -------------------------------------- */
        for k in 1..=spec.slaps {
            let tt = (spec.predelay + spec.slap_time * f64::from(k) * rng.range(0.985, 1.015)) * wob;
            let idx = (tt * sr).floor() as usize;
            if idx + 64 >= len {
                continue;
            }
            let g = 0.55 * 0.68f64.powi(k as i32) * if rng.float() < 0.5 { -1.0 } else { 1.0 };
            // Slaps are band-limited: a street echo is mid-heavy, never a click.
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for s in 0..400usize {
                if idx + s >= len {
                    break;
                }
                let x = rng.signed() * (-(s as f64) / 90.0).exp();
                s1 += 0.35 * (x - s1);
                s2 += 0.35 * (s1 - s2);
                d[idx + s] = (f64::from(d[idx + s]) + g * s2 * 3.2) as f32;
            }
        }
    }

    let mut buf = AudioBuffer {
        channels,
        sample_rate: sr,
    };
    normalise(&mut buf, 0.42);
    buf
}

/// Peak-normalise both channels together to `target` (`ir.js:161-175`).
fn normalise(buf: &mut AudioBuffer, target: f64) {
    let mut peak = 1e-9f64;
    for d in &buf.channels {
        for &v in d {
            let a = f64::from(v).abs();
            if a > peak {
                peak = a;
            }
        }
    }
    let g = target / peak;
    for d in &mut buf.channels {
        for v in d.iter_mut() {
            *v = (f64::from(*v) * g) as f32;
        }
    }
}

/// Render an IR straight into a graph buffer — the one place the pure generator
/// meets the audio context.
pub fn generate_ir_buffer(graph: &mut AudioGraph, rng: &mut Rng, spec: &IrSpec) -> BufferId {
    let buf = generate_ir(graph.sample_rate(), rng, spec);
    let id = graph.create_buffer(2, buf.length(), buf.sample_rate);
    *graph.buffer_mut(id) = buf;
    id
}

/// Blend weights over [`IR_SPECS`], plus the readouts the ambience bed and the
/// debug overlay take off the same probe (`ir.js:220`'s `w` object).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpaceWeights {
    pub tight: f64,
    pub room: f64,
    pub street: f64,
    pub tunnel: f64,
    pub open: f64,
    pub enclosure: f64,
    pub mean_free: f64,
    pub ceiling: f64,
    pub close_sides: f64,
    pub median: f64,
}

impl SpaceWeights {
    /// The starting weights `index.js:91-94` seeds `_space` with: outdoors,
    /// mostly open, a little street.
    pub fn outdoors(probe_dist: f64) -> Self {
        SpaceWeights {
            tight: 0.0,
            room: 0.0,
            street: 0.35,
            tunnel: 0.0,
            open: 0.65,
            enclosure: 0.0,
            mean_free: probe_dist,
            ceiling: probe_dist,
            close_sides: 0.0,
            median: 0.0,
        }
    }

    pub fn get(&self, space: Space) -> f64 {
        match space {
            Space::Tight => self.tight,
            Space::Room => self.room,
            Space::Street => self.street,
            Space::Tunnel => self.tunnel,
            Space::Open => self.open,
        }
    }

    pub fn set(&mut self, space: Space, v: f64) {
        match space {
            Space::Tight => self.tight = v,
            Space::Room => self.room = v,
            Space::Street => self.street = v,
            Space::Tunnel => self.tunnel = v,
            Space::Open => self.open = v,
        }
    }

    /// The dominant space — `index.js:321-324`'s `best`.
    pub fn dominant(&self) -> Space {
        let mut best = Space::Open;
        let mut bv = -1.0;
        for s in Space::ALL {
            if self.get(s) > bv {
                bv = self.get(s);
                best = s;
            }
        }
        best
    }
}

impl Default for SpaceWeights {
    fn default() -> Self {
        SpaceWeights {
            tight: 0.0,
            room: 0.0,
            street: 0.0,
            tunnel: 0.0,
            open: 0.0,
            enclosure: 0.0,
            mean_free: 0.0,
            ceiling: 0.0,
            close_sides: 0.0,
            median: 0.0,
        }
    }
}

/// Classify the space around the listener from a set of raycast distances and
/// turn it into blend weights over [`IR_SPECS`] (`ir.js:184-250`).
///
/// `hits` is a flat array of ray distances (infinite/`max_dist` when nothing was
/// hit), in the order produced by the probe directions: 8 around the horizon,
/// 1 up. Allocation-free: the insertion sort works in a fixed 16-slot scratch,
/// exactly as the source's module-level `SORT` does — but on the stack, because
/// a module-level mutable scratch is hidden global state and the array is 128
/// bytes.
pub fn classify_space(hits: &[f64], max_dist: f64, out: &mut SpaceWeights) {
    let mut sort = [0.0f64; 16];
    let horiz = hits.len() - 1;
    let (mut sum, mut close, mut min_d, mut max_d) = (0.0f64, 0.0f64, max_dist, 0.0f64);
    for &h in &hits[..horiz] {
        let d = h.min(max_dist);
        sum += d;
        if d < 12.0 {
            close += 1.0;
        }
        min_d = min_d.min(d);
        max_d = max_d.max(d);
    }
    let mean = sum / horiz as f64;
    let ceil = hits[horiz].min(max_dist);
    let close_sides = close / horiz as f64;

    // Median horizontal distance: the characteristic size of the space. The
    // mean is useless here because a single ray escaping through a doorway
    // drags it from 3 m to 8 m and a small room stops reading as small.
    for i in 0..horiz {
        sort[i] = hits[i].min(max_dist);
    }
    for i in 1..horiz {
        let v = sort[i];
        let mut j = i as isize - 1;
        while j >= 0 && sort[j as usize] > v {
            sort[j as usize + 1] = sort[j as usize];
            j -= 1;
        }
        sort[(j + 1) as usize] = v;
    }
    let median = if horiz & 1 == 1 {
        sort[horiz >> 1]
    } else {
        (sort[horiz / 2 - 1] + sort[horiz / 2]) * 0.5
    };

    // A ceiling within a few metres is the single most reliable indoor signal:
    // outdoors that ray goes to the sky. Everything else only decides *which*
    // kind of space it is.
    let roofed = 1.0 - clamp((ceil - 2.8) / 7.0, 0.0, 1.0);
    // Relative spread of the horizon: a corridor is long one way, tight the other.
    let elong = clamp((max_d - min_d) / max_d.max(1.0), 0.0, 1.0);
    let small = clamp(1.0 - median / 9.0, 0.0, 1.0);

    // Weights deliberately overlap: real spaces are blends, and blending the
    // convolvers is also what stops an audible switch in a doorway.
    let indoor = roofed;
    let outdoor = 1.0 - indoor;
    // Eight rays cannot reliably tell a corridor from a room with an open door,
    // so flutter is capped: it colours the tail, it never owns it. Getting this
    // wrong is far more audible than under-reading a real corridor.
    let tunnel = indoor * elong.powi(2) * 0.55;
    let rest = (indoor - tunnel).max(0.0);
    out.tunnel = tunnel;
    out.tight = rest * small;
    out.room = rest * (1.0 - small);
    out.street = outdoor * clamp(close_sides * 2.2, 0.0, 1.0);
    out.open = outdoor * clamp(1.0 - close_sides * 1.8, 0.06, 1.0);

    // Normalise over the space weights ONLY — `out` also carries the
    // enclosure/meanFree/ceiling readouts.
    let mut tot = 0.0;
    for s in Space::ALL {
        tot += out.get(s);
    }
    if tot < 1e-4 {
        out.open = 1.0;
        tot = 1.0;
    }
    for s in Space::ALL {
        let v = out.get(s) / tot;
        out.set(s, v);
    }
    // Readouts for the ambience bed and the debug overlay.
    out.enclosure = clamp(roofed * (0.45 + 0.55 * close_sides), 0.0, 1.0);
    out.mean_free = mean;
    out.ceiling = ceil;
    out.close_sides = close_sides;
    out.median = median;
}
