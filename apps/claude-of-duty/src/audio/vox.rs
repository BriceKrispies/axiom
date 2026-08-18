//! Voice — formant synthesis for enemy barks.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/vox.js:1-323` — the whole file.
//!
//! No speech samples, so barks are built the way a vocal tract works:
//!
//! ```text
//! glottal pulse train (PeriodicWave, 1/n^1.15 harmonics)
//!   + aspiration noise
//!   ─► three parallel band-passes at the formant frequencies F1..F3
//!   ─► chest/throat shaping, presence peak, mild saturation (shouting)
//!   + separately mixed consonant bursts (plosives and fricatives)
//! ```
//!
//! The formant centres are ramped between vowels, the f0 follows a per-syllable
//! pitch contour, and both are jittered every ~25 ms. That jitter is the single
//! most important ingredient: without it the result is a Speak&Spell, with it a
//! player reads it as a human shouting a word they cannot quite make out —
//! which is exactly the goal for enemy chatter at 30 m.

use crate::audio::dsp::{
    ad, adsr, biquad, clamp, gain, hit, saturation_curve, shaper, sweep, NoiseBank, NoiseKind,
};
use crate::audio::graph::{AudioGraph, FilterKind, Wave};
use crate::audio::weapons::Voice;
use crate::rng::Rng;

/// F1, F2, F3 (Hz) and their bandwidths, adult male, shouted register
/// (`vox.js:22-31`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vowel {
    /// "father"
    A,
    /// "bed"
    E,
    /// "see"
    I,
    /// "law"
    O,
    /// "boot"
    U,
    Ah,
    /// "her"
    Ehr,
    Ohh,
}

impl Vowel {
    /// `[F1, F2, F3, B1, B2, B3]`.
    fn formants(self) -> [f64; 6] {
        match self {
            Vowel::A => [730.0, 1090.0, 2440.0, 110.0, 130.0, 180.0],
            Vowel::E => [530.0, 1840.0, 2480.0, 90.0, 120.0, 170.0],
            Vowel::I => [300.0, 2290.0, 3010.0, 70.0, 130.0, 190.0],
            Vowel::O => [570.0, 840.0, 2410.0, 90.0, 110.0, 170.0],
            Vowel::U => [325.0, 700.0, 2530.0, 70.0, 100.0, 170.0],
            Vowel::Ah => [640.0, 1200.0, 2500.0, 110.0, 140.0, 190.0],
            Vowel::Ehr => [490.0, 1350.0, 1690.0, 100.0, 130.0, 180.0],
            Vowel::Ohh => [450.0, 900.0, 2300.0, 95.0, 115.0, 175.0],
        }
    }
}

/// A syllable's onset consonant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Onset {
    /// `'p'` — a highpassed burst.
    Plosive,
    /// `'f'` — a band-passed fricative, leading the vowel further.
    Fricative,
    /// `'n'` — a hum through a low formant instead of a burst.
    Nasal,
}

/// One syllable of a bark script (`vox.js:34-37`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Syllable {
    /// Vowel.
    pub v: Vowel,
    /// Duration.
    pub d: f64,
    /// Amplitude.
    pub a: f64,
    /// Pitch multiplier.
    pub p: f64,
    /// Onset consonant.
    pub on: Option<Onset>,
    /// Gap after the syllable.
    pub g: f64,
}

const fn syl(v: Vowel, d: f64, a: f64, p: f64, on: Option<Onset>, g: f64) -> Syllable {
    Syllable { v, d, a, p, on, g }
}

/// A bark script.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarkSpec {
    pub f0: f64,
    pub drive: f64,
    /// `None` takes the 0.16 `??` default.
    pub breath: Option<f64>,
    pub tremolo: Option<f64>,
    pub dying: bool,
    pub syl: &'static [Syllable],
}

const fn spec(f0: f64, drive: f64, syl: &'static [Syllable]) -> BarkSpec {
    BarkSpec {
        f0,
        drive,
        breath: None,
        tremolo: None,
        dying: false,
        syl,
    }
}

/// The bark vocabulary (`vox.js:38-120`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bark {
    /// "CONTACT!"
    Contact,
    /// "ENEMY SPOTTED"
    Spotted,
    /// "RELOADING!"
    Reloading,
    /// "GRENADE!" — panicked, pitch climbs hard.
    Grenade,
    /// "FLANKING!"
    Flanking,
    /// "SUPPRESSING FIRE!"
    Suppressing,
    /// "MOVE UP!"
    Moveup,
    /// Wordless taking-fire grunt.
    Hit,
    /// Pain, longer, wavering.
    Pain,
    /// Death: pitch collapses, breath takes over, ends in an exhale.
    Death,
    /// Short affirmative, for squad chatter.
    Copy,
}

use Onset::{Fricative as F, Nasal as N, Plosive as P};
use Vowel::{Ah, Ehr, Ohh, A, E, I, O, U};

static SYL_CONTACT: [Syllable; 2] = [
    syl(O, 0.13, 1.0, 1.06, Some(P), 0.012),
    syl(A, 0.19, 1.0, 1.16, Some(P), 0.0),
];
static SYL_SPOTTED: [Syllable; 5] = [
    syl(E, 0.1, 0.9, 1.05, None, 0.01),
    syl(A, 0.08, 0.7, 1.0, Some(N), 0.01),
    syl(I, 0.1, 0.8, 0.95, None, 0.06),
    syl(A, 0.12, 1.0, 1.1, Some(F), 0.02),
    syl(E, 0.13, 0.75, 0.9, Some(P), 0.0),
];
static SYL_RELOADING: [Syllable; 3] = [
    syl(I, 0.09, 0.8, 1.0, None, 0.01),
    syl(Ohh, 0.16, 1.0, 1.12, None, 0.015),
    syl(I, 0.13, 0.7, 0.9, Some(P), 0.0),
];
static SYL_GRENADE: [Syllable; 2] = [
    syl(E, 0.1, 0.9, 1.0, Some(P), 0.012),
    syl(A, 0.26, 1.15, 1.35, Some(N), 0.0),
];
static SYL_FLANKING: [Syllable; 2] = [
    syl(A, 0.16, 1.0, 1.1, Some(F), 0.015),
    syl(I, 0.13, 0.8, 0.95, Some(N), 0.0),
];
static SYL_SUPPRESSING: [Syllable; 4] = [
    syl(U, 0.09, 0.75, 0.98, Some(F), 0.01),
    syl(E, 0.14, 1.0, 1.12, Some(P), 0.02),
    syl(I, 0.1, 0.7, 0.9, None, 0.05),
    syl(A, 0.18, 0.95, 1.05, Some(F), 0.0),
];
static SYL_MOVEUP: [Syllable; 2] = [
    syl(U, 0.16, 1.0, 1.08, Some(N), 0.03),
    syl(A, 0.14, 0.9, 1.0, None, 0.0),
];
static SYL_HIT: [Syllable; 1] = [syl(Ah, 0.16, 1.1, 1.2, Some(P), 0.0)];
static SYL_PAIN: [Syllable; 1] = [syl(Ah, 0.34, 0.95, 1.0, None, 0.0)];
static SYL_DEATH: [Syllable; 2] = [
    syl(Ah, 0.3, 1.0, 1.15, None, 0.02),
    syl(Ehr, 0.42, 0.6, 0.62, None, 0.0),
];
static SYL_COPY: [Syllable; 2] = [
    syl(A, 0.1, 0.85, 1.0, Some(P), 0.02),
    syl(I, 0.12, 0.7, 0.88, Some(P), 0.0),
];

impl Bark {
    pub const ALL: [Bark; 11] = [
        Bark::Contact,
        Bark::Spotted,
        Bark::Reloading,
        Bark::Grenade,
        Bark::Flanking,
        Bark::Suppressing,
        Bark::Moveup,
        Bark::Hit,
        Bark::Pain,
        Bark::Death,
        Bark::Copy,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Bark::Contact => "contact",
            Bark::Spotted => "spotted",
            Bark::Reloading => "reloading",
            Bark::Grenade => "grenade",
            Bark::Flanking => "flanking",
            Bark::Suppressing => "suppressing",
            Bark::Moveup => "moveup",
            Bark::Hit => "hit",
            Bark::Pain => "pain",
            Bark::Death => "death",
            Bark::Copy => "copy",
        }
    }

    pub fn spec(self) -> BarkSpec {
        match self {
            Bark::Contact => spec(1.18, 1.25, &SYL_CONTACT),
            Bark::Spotted => spec(1.1, 1.1, &SYL_SPOTTED),
            Bark::Reloading => spec(1.05, 1.0, &SYL_RELOADING),
            Bark::Grenade => spec(1.3, 1.5, &SYL_GRENADE),
            Bark::Flanking => spec(1.12, 1.2, &SYL_FLANKING),
            Bark::Suppressing => spec(1.08, 1.15, &SYL_SUPPRESSING),
            Bark::Moveup => spec(1.1, 1.2, &SYL_MOVEUP),
            Bark::Hit => BarkSpec {
                breath: Some(0.5),
                ..spec(1.25, 1.6, &SYL_HIT)
            },
            Bark::Pain => BarkSpec {
                breath: Some(0.65),
                tremolo: Some(14.0),
                ..spec(1.15, 1.3, &SYL_PAIN)
            },
            Bark::Death => BarkSpec {
                breath: Some(1.0),
                tremolo: Some(22.0),
                dying: true,
                ..spec(1.05, 1.4, &SYL_DEATH)
            },
            Bark::Copy => spec(1.0, 0.9, &SYL_COPY),
        }
    }
}

/// Glottal-ish pulse: strong fundamental, 1/n^1.15 rolloff, alternating phase
/// (`vox.js:125-137`). Cached per context.
fn glottal_wave(g: &mut AudioGraph) -> crate::audio::graph::WaveId {
    g.cached_wave(|| {
        const N: usize = 40;
        let real = vec![0.0f32; N];
        let mut imag = vec![0.0f32; N];
        for (n, slot) in imag.iter_mut().enumerate().skip(1) {
            *slot = ((1.0 / (n as f64).powf(1.15)) * if n % 2 == 0 { -0.75 } else { 1.0 }) as f32;
        }
        (real, imag)
    })
}

/// `bark`'s options bag (`vox.js:142-143`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarkOpts {
    pub when: Option<f64>,
    pub bark: Bark,
    /// Base Hz. `None` draws `rng.range(96, 132)`.
    pub f0: Option<f64>,
    /// 0.9..1.1. `None` draws `rng.range(0.94, 1.07)`.
    pub tract: Option<f64>,
    pub level: f64,
    /// Squad-comms treatment.
    pub radio: bool,
}

impl Default for BarkOpts {
    fn default() -> Self {
        BarkOpts {
            when: None,
            bark: Bark::Contact,
            f0: None,
            tract: None,
            level: 1.0,
            radio: false,
        }
    }
}

/// Synthesize a bark (`vox.js:145-306`).
pub fn bark(g: &mut AudioGraph, bank: &NoiseBank, rng: &mut Rng, o: BarkOpts) -> Voice {
    let t0 = o.when.unwrap_or_else(|| g.current_time());
    let spec = o.bark.spec();
    let tract = o.tract.unwrap_or_else(|| rng.range(0.94, 1.07));
    let f0 = o.f0.unwrap_or_else(|| rng.range(96.0, 132.0)) * spec.f0;
    let level = o.level;
    let out = gain(g, 0.2); // VOICE TRIM

    let total: f64 = spec.syl.iter().map(|x| x.d + x.g).sum();

    /* ---- source ---------------------------------------------------- */
    let wave = glottal_wave(g);
    let src = g.create_periodic_oscillator(wave);
    let src_gain = gain(g, 0.0);
    g.connect(src, src_gain);

    // Aspiration: always a little, a lot when hurt or dying.
    let breath_level = spec.breath.unwrap_or(0.16) * rng.range(0.8, 1.25);
    let rate = rng.range(0.9, 1.2);
    let noise = bank.one_shot(g, NoiseKind::White, rng, rate);
    let noise_bp = biquad(g, FilterKind::Bandpass, 1400.0, 0.6, 0.0);
    let noise_gain = gain(g, 0.0);
    g.series(&[noise, noise_bp, noise_gain]);

    let excite = gain(g, 1.0);
    g.connect(src_gain, excite);
    g.connect(noise_gain, excite);

    /* ---- formant bank ---------------------------------------------- */
    let first = spec.syl[0].v.formants();
    let mut fs = Vec::with_capacity(3);
    for i in 0..3 {
        let f = first[i] * tract;
        let bw = first[i + 3];
        let bp = biquad(g, FilterKind::Bandpass, f, clamp(f / bw, 1.5, 14.0), 0.0);
        let fg = gain(g, [1.0, 0.55, 0.24][i]);
        g.connect(excite, bp);
        g.connect(bp, fg);
        fs.push((bp, fg));
    }

    /* ---- vocal tract output shaping -------------------------------- */
    let throat = biquad(g, FilterKind::Peaking, 480.0, 1.1, 4.0); // chest resonance
    let presence = biquad(g, FilterKind::Peaking, 2600.0, 1.4, 5.0); // shout presence
    let hp = biquad(g, FilterKind::Highpass, 150.0, 0.7, 0.0);
    let lp = biquad(g, FilterKind::Lowpass, 5200.0, 0.7, 0.0);
    let curve = saturation_curve(g, 1.6 * spec.drive, 0.35);
    let drv = shaper(g, curve, "2x");
    let body_gain = gain(g, 1.5 * level);
    for &(_, fg) in &fs {
        g.connect(fg, throat);
    }
    g.series(&[throat, presence, hp, lp, drv, body_gain, out]);

    /* ---- tremolo (pain / death gargle) ----------------------------- */
    if let Some(tremolo) = spec.tremolo {
        let trem = crate::audio::dsp::osc(g, Wave::Sine, tremolo * rng.range(0.85, 1.15));
        let tg = gain(g, 0.35);
        g.connect(trem, tg);
        g.connect_param(tg, body_gain.gain());
        g.start(trem, t0);
        g.stop(trem, t0 + total + 0.4);
    }

    /* ---- per-syllable automation ----------------------------------- */
    let mut t = t0;
    g.set_value_at_time(src.frequency(), f0 * spec.syl[0].p, t0);
    let n_syl = spec.syl.len();
    for (i, s) in spec.syl.iter().enumerate() {
        let v = s.v.formants();
        let amp = s.a * 0.5;

        /* onset consonant, mixed straight to the output */
        if let Some(on) = s.on {
            // Onsets lead the vowel; never let that run off the start of the
            // timeline.
            let lead = if on == Onset::Fricative { 0.055 } else { 0.018 };
            let ct = (t - lead).max(0.0);
            let crate_ = rng.range(0.9, 1.3);
            let cs = bank.one_shot(g, NoiseKind::White, rng, crate_);
            let (kind, freq, q) = if on == Onset::Fricative {
                (FilterKind::Bandpass, rng.range(3800.0, 6500.0), 1.1)
            } else {
                (FilterKind::Highpass, rng.range(1400.0, 2600.0), 0.7)
            };
            let cbp = biquad(g, kind, freq, q, 0.0);
            let cg = gain(g, 0.0);
            g.series(&[cs, cbp, cg, out]);
            match on {
                Onset::Fricative => {
                    ad(g, cg.gain(), ct, 0.1 * level, 0.012, 0.05);
                    g.start_source(cs, ct, 0.12);
                }
                Onset::Nasal => {
                    // Nasal: hum through a low formant instead of a burst.
                    ad(g, cg.gain(), ct, 0.02 * level, 0.01, 0.04);
                    g.start_source(cs, ct, 0.08);
                    g.set_value_at_time(fs[0].0.frequency(), 260.0 * tract, ct);
                }
                Onset::Plosive => {
                    hit(g, cg.gain(), ct, 0.16 * level, 0.014);
                    g.start_source(cs, ct, 0.05);
                }
            }
        }

        /* formant glide into this vowel — 35 ms transition reads as articulation */
        for k in 0..3 {
            let f = v[k] * tract * (1.0 + rng.range(-0.02, 0.02));
            let bw = v[k + 3];
            let at = (t - 0.03).max(t0);
            g.set_target_at_time(fs[k].0.frequency(), f, at, 0.014);
            g.set_target_at_time(fs[k].0.q(), clamp(f / bw, 1.5, 14.0), at, 0.02);
        }

        /* pitch contour: rise into the stressed syllable, sag at the end */
        let p_target = f0 * s.p;
        g.set_target_at_time(src.frequency(), p_target, t, 0.03);
        if spec.dying && i == n_syl - 1 {
            sweep(
                g,
                src.frequency(),
                t + 0.05,
                p_target,
                p_target * 0.45,
                s.d,
            );
        } else {
            g.set_target_at_time(src.frequency(), p_target * 0.94, t + s.d * 0.6, 0.06);
        }

        /* amplitude: fast onset, held, quick release; last syllable decays longer */
        let last = i == n_syl - 1;
        let rel = if last {
            if spec.dying {
                s.d * 0.9
            } else {
                0.055
            }
        } else {
            0.028
        };
        adsr(
            g,
            src_gain.gain(),
            t,
            amp * level,
            0.014,
            s.d * 0.22,
            s.d * 0.5,
            0.72,
            rel,
        );
        ad(
            g,
            noise_gain.gain(),
            t,
            amp * breath_level * level,
            0.02,
            s.d + rel,
        );

        t += s.d + s.g;
    }

    /* ---- dying exhale ---------------------------------------------- */
    if spec.dying {
        let et = t + 0.05;
        let rate = rng.range(0.6, 0.9);
        let es = bank.one_shot(g, NoiseKind::White, rng, rate);
        let ebp = biquad(g, FilterKind::Bandpass, 700.0, 0.55, 0.0);
        let eg = gain(g, 0.0);
        g.series(&[es, ebp, eg, out]);
        sweep(g, ebp.frequency(), et, 900.0, 380.0, 0.6);
        ad(g, eg.gain(), et, 0.16 * level, 0.08, 0.6);
        g.start_source(es, et, 0.9);
        t = et + 0.7;
    }

    let end = t + 0.35;
    let src_start = (t0 - 0.01).max(0.0);
    g.start(src, src_start);
    g.stop(src, end);
    g.start_source(noise, src_start, end - src_start + 0.05);

    /* ---- radio treatment (squad comms) ----------------------------- */
    if o.radio {
        let rbp1 = biquad(g, FilterKind::Highpass, 420.0, 0.8, 0.0);
        let rbp2 = biquad(g, FilterKind::Lowpass, 3200.0, 0.9, 0.0);
        let rcurve = saturation_curve(g, 7.0, 0.3);
        let rdrv = shaper(g, rcurve, "2x");
        let rg = gain(g, 1.1);
        let radio_out = gain(g, 1.0);
        g.series(&[out, rbp1, rbp2, rdrv, rg, radio_out]);
        // Squelch click at both ends of the transmission.
        for st in [(t0 - 0.05).max(0.0), end - 0.2] {
            let cs = bank.one_shot(g, NoiseKind::White, rng, 1.1);
            let cbp = biquad(g, FilterKind::Bandpass, 2600.0, 1.6, 0.0);
            let cg = gain(g, 0.0);
            g.series(&[cs, cbp, cg, radio_out]);
            hit(g, cg.gain(), st, 0.09, 0.03);
            g.start_source(cs, st, 0.06);
        }
        return Voice {
            node: radio_out,
            end: end + 0.1,
            send: 0.05,
        };
    }

    Voice {
        node: out,
        end: end + 0.1,
        send: 0.45,
    }
}

/// What the AI asked for, in its own vocabulary (`vox.js:309-322`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarkRequest {
    Spot,
    Reload,
    Grenade,
    Flank,
    Suppress,
    Advance,
    Hurt,
    Death,
    Copy,
    /// The `default:` arm.
    Other,
}

impl BarkRequest {
    pub fn from_str(name: &str) -> BarkRequest {
        match name {
            "spot" => BarkRequest::Spot,
            "reload" => BarkRequest::Reload,
            "grenade" => BarkRequest::Grenade,
            "flank" => BarkRequest::Flank,
            "suppress" => BarkRequest::Suppress,
            "advance" => BarkRequest::Advance,
            "hurt" => BarkRequest::Hurt,
            "death" => BarkRequest::Death,
            "copy" => BarkRequest::Copy,
            _ => BarkRequest::Other,
        }
    }
}

/// Pick a plausible bark for an AI event without the `ai` agent knowing our
/// list (`vox.js:309-322`).
///
/// Two arms draw from `rng` and the rest do not — that asymmetry is load-bearing
/// for stream alignment, so it is kept exactly.
pub fn bark_for(kind: BarkRequest, rng: &mut Rng) -> Bark {
    match kind {
        BarkRequest::Spot => {
            if rng.float() < 0.5 {
                Bark::Contact
            } else {
                Bark::Spotted
            }
        }
        BarkRequest::Reload => Bark::Reloading,
        BarkRequest::Grenade => Bark::Grenade,
        BarkRequest::Flank => Bark::Flanking,
        BarkRequest::Suppress => Bark::Suppressing,
        BarkRequest::Advance => Bark::Moveup,
        BarkRequest::Hurt => {
            if rng.float() < 0.5 {
                Bark::Hit
            } else {
                Bark::Pain
            }
        }
        BarkRequest::Death => Bark::Death,
        BarkRequest::Copy => Bark::Copy,
        BarkRequest::Other => Bark::Contact,
    }
}
