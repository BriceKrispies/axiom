//! Foley — impacts, footsteps, casings, reloads, explosions, body falls, UI.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/foley.js:1-790` — the whole
//! file.
//!
//! Everything is keyed off the twelve surface names in the source's
//! `ARCHITECTURE.md` so physics, FX, decals and audio always agree about what
//! was hit. The recurring recipe for a physical impact is:
//!
//! ```text
//! transient (contact)  +  body (mass)  +  texture (material)  +  debris
//! ```
//!
//! Which of those four dominates is what makes concrete sound like concrete and
//! flesh sound like flesh; the envelope shapes matter far more than the exact
//! filter frequencies.

use crate::audio::dsp::{
    ad, biquad, clamp, gain, hit, lerp, osc, saturation_curve, semis, shaper, struck, sweep,
    NoiseBank, NoiseKind, Partial,
};
use crate::audio::graph::{AudioGraph, FilterKind, NodeId, Wave};
use crate::audio::weapons::Voice;
use crate::rng::Rng;
pub use crate::world::palette::Surface;

/// A high-Q partial in a surface's ring bank.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ring {
    f: f64,
    q: f64,
    g: f64,
    decay: f64,
}

/// The material texture burst.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tex {
    kind: NoiseKind,
    f: f64,
    q: f64,
    decay: f64,
    level: f64,
    /// Water's upward sweep instead of the usual downward one.
    rise: bool,
}

/// The dust/powder cloud, on the surfaces that make one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Dust {
    f: f64,
    decay: f64,
    level: f64,
}

/// Per-surface impact recipe (`foley.js:29-97`).
///
/// * `body_f`/`body_decay` — the mass thump
/// * `ring` — high-Q partials (metal, glass, wood) or empty
/// * `tex` — the material texture burst
/// * `grains` — number of debris grains
/// * `bright` — transient level 0..1
/// * `wet` — reverb send
#[derive(Debug, Clone, Copy, PartialEq)]
struct Impact {
    bright: f64,
    body_f: f64,
    body_decay: f64,
    ring: &'static [Ring],
    tex: Tex,
    dust: Option<Dust>,
    grains: f64,
    wet: f64,
    bubbles: bool,
    wet_squelch: bool,
}

const fn impact(bright: f64, body_f: f64, body_decay: f64, tex: Tex, grains: f64, wet: f64) -> Impact {
    Impact {
        bright,
        body_f,
        body_decay,
        ring: &[],
        tex,
        dust: None,
        grains,
        wet,
        bubbles: false,
        wet_squelch: false,
    }
}

const fn tex(kind: NoiseKind, f: f64, q: f64, decay: f64, level: f64) -> Tex {
    Tex {
        kind,
        f,
        q,
        decay,
        level,
        rise: false,
    }
}

static METAL_RING: [Ring; 4] = [
    Ring { f: 1750.0, q: 34.0, g: 0.42, decay: 0.28 },
    Ring { f: 3120.0, q: 26.0, g: 0.3, decay: 0.17 },
    Ring { f: 5400.0, q: 18.0, g: 0.18, decay: 0.09 },
    Ring { f: 8100.0, q: 12.0, g: 0.09, decay: 0.05 },
];
static WOOD_RING: [Ring; 3] = [
    Ring { f: 420.0, q: 14.0, g: 0.35, decay: 0.11 },
    Ring { f: 780.0, q: 11.0, g: 0.2, decay: 0.07 },
    Ring { f: 1520.0, q: 8.0, g: 0.1, decay: 0.04 },
];
static GLASS_RING: [Ring; 4] = [
    Ring { f: 3400.0, q: 40.0, g: 0.34, decay: 0.13 },
    Ring { f: 5300.0, q: 34.0, g: 0.26, decay: 0.1 },
    Ring { f: 7900.0, q: 26.0, g: 0.2, decay: 0.07 },
    Ring { f: 11200.0, q: 18.0, g: 0.12, decay: 0.05 },
];
static RUBBER_RING: [Ring; 1] = [Ring { f: 260.0, q: 9.0, g: 0.2, decay: 0.06 }];

/// `IMPACT`, indexed by [`Surface::ALL`] (equivalently, [`Surface::index`]).
static IMPACT: [Impact; 12] = [
    // concrete
    Impact {
        dust: Some(Dust { f: 1200.0, decay: 0.3, level: 0.16 }),
        ..impact(0.85, 180.0, 0.05, tex(NoiseKind::White, 2600.0, 0.9, 0.075, 0.75), 5.0, 0.4)
    },
    // metal
    Impact {
        ring: &METAL_RING,
        ..impact(1.0, 150.0, 0.035, tex(NoiseKind::White, 5200.0, 1.2, 0.03, 0.5), 3.0, 0.5)
    },
    // wood
    Impact {
        ring: &WOOD_RING,
        ..impact(0.6, 320.0, 0.055, tex(NoiseKind::White, 1500.0, 1.0, 0.045, 0.45), 5.0, 0.32)
    },
    // dirt
    Impact {
        dust: Some(Dust { f: 600.0, decay: 0.34, level: 0.2 }),
        ..impact(0.25, 120.0, 0.07, tex(NoiseKind::Brown, 700.0, 0.7, 0.09, 0.7), 4.0, 0.2)
    },
    // sand
    Impact {
        dust: Some(Dust { f: 1000.0, decay: 0.4, level: 0.24 }),
        ..impact(0.18, 105.0, 0.055, tex(NoiseKind::White, 1500.0, 0.5, 0.13, 0.5), 3.0, 0.16)
    },
    // glass
    Impact {
        ring: &GLASS_RING,
        ..impact(1.0, 500.0, 0.02, tex(NoiseKind::Crackle, 6000.0, 0.9, 0.28, 0.6), 11.0, 0.46)
    },
    // water
    Impact {
        bubbles: true,
        ..impact(
            0.3,
            260.0,
            0.03,
            Tex { rise: true, ..tex(NoiseKind::White, 1800.0, 0.8, 0.14, 0.75) },
            4.0,
            0.3,
        )
    },
    // foliage
    impact(0.25, 380.0, 0.02, tex(NoiseKind::Crackle, 2600.0, 0.8, 0.16, 0.6), 7.0, 0.22),
    // fabric
    Impact {
        dust: Some(Dust { f: 700.0, decay: 0.2, level: 0.1 }),
        ..impact(0.2, 150.0, 0.045, tex(NoiseKind::White, 900.0, 0.6, 0.06, 0.4), 2.0, 0.18)
    },
    // flesh
    Impact {
        wet_squelch: true,
        ..impact(0.35, 128.0, 0.06, tex(NoiseKind::White, 620.0, 1.4, 0.055, 0.62), 3.0, 0.24)
    },
    // rubber
    Impact {
        ring: &RUBBER_RING,
        ..impact(0.3, 190.0, 0.04, tex(NoiseKind::White, 1100.0, 0.9, 0.03, 0.3), 1.0, 0.2)
    },
    // plaster
    Impact {
        dust: Some(Dust { f: 900.0, decay: 0.42, level: 0.26 }),
        ..impact(0.7, 220.0, 0.035, tex(NoiseKind::White, 1900.0, 0.8, 0.05, 0.6), 6.0, 0.42)
    },
];

fn impact_for(s: Surface) -> &'static Impact {
    &IMPACT[usize::from(s.index())]
}

/* ------------------------------------------------------------------ */
/* Bullet impacts                                                     */
/* ------------------------------------------------------------------ */

/// A bullet impact (`foley.js:106-215`).
///
/// `energy` is clamped to 0.15..1.6 as in the source.
pub fn surface_impact(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    when: Option<f64>,
    surface: Surface,
    energy: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let s = impact_for(surface);
    let e = clamp(energy, 0.15, 1.6);
    let jit = semis(rng.range(-2.5, 2.5));
    let out = gain(g, 0.22); // VOICE TRIM
    let mut end = t0 + 0.2;

    /* transient */
    if s.bright > 0.05 {
        let rate = rng.range(0.9, 1.35);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let hp = biquad(g, FilterKind::Highpass, 3000.0 * jit, 0.7, 0.0);
        let tg = gain(g, 0.0);
        g.series(&[src, hp, tg, out]);
        let decay = rng.range(0.003, 0.008);
        hit(g, tg.gain(), t0, 0.55 * s.bright * e, decay);
        g.start_source(src, t0, 0.04);
    }

    /* body */
    {
        let b = osc(g, Wave::Sine, s.body_f * jit);
        let bg = gain(g, 0.0);
        let curve = saturation_curve(g, 2.5, 0.4);
        let drv = shaper(g, curve, "2x");
        g.connect(b, bg);
        g.series(&[bg, drv, out]);
        sweep(
            g,
            b.frequency(),
            t0,
            s.body_f * jit * 1.6,
            s.body_f * jit * 0.7,
            s.body_decay * 1.5,
        );
        let decay = s.body_decay * rng.range(0.85, 1.2);
        ad(g, bg.gain(), t0, 0.5 * e, 0.0015, decay);
        g.start(b, t0);
        g.stop(b, t0 + s.body_decay * 2.2 + 0.02);
        end = end.max(t0 + s.body_decay * 2.2);
    }

    /* material texture */
    {
        let tx = s.tex;
        let rate = rng.range(0.8, 1.3);
        let src = bank.one_shot(g, tx.kind, rng, rate);
        let bp = biquad(g, FilterKind::Bandpass, tx.f * jit, tx.q, 0.0);
        let tg = gain(g, 0.0);
        g.series(&[src, bp, tg, out]);
        if tx.rise {
            sweep(g, bp.frequency(), t0, tx.f * 0.4, tx.f * 2.2, tx.decay);
        } else {
            sweep(
                g,
                bp.frequency(),
                t0,
                tx.f * 1.5 * jit,
                tx.f * 0.6 * jit,
                tx.decay * 1.6,
            );
        }
        let attack = if tx.rise { 0.008 } else { 0.0015 };
        let decay = tx.decay * rng.range(0.85, 1.25);
        ad(g, tg.gain(), t0, tx.level * e, attack, decay);
        g.start_source(src, t0, tx.decay * 3.0 + 0.05);
        end = end.max(t0 + tx.decay * 3.0);
    }

    /* resonant ring (metal / glass / wood) */
    if !s.ring.is_empty() {
        let parts: Vec<Partial> = s
            .ring
            .iter()
            .map(|p| {
                Partial::new(
                    p.f * semis(rng.range(-3.0, 3.0)),
                    p.q * rng.range(0.8, 1.25),
                    p.g * e,
                    p.decay * rng.range(0.75, 1.3),
                )
            })
            .collect();
        let r = struck(g, bank, rng, t0, &parts, 0.0035);
        g.connect(r, out);
        end = end.max(t0 + 0.4);
    }

    /* dust / powder cloud */
    if let Some(dust) = s.dust {
        let rate = rng.range(0.7, 1.1);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let lp = biquad(g, FilterKind::Lowpass, dust.f, 0.8, 0.0);
        let dg = gain(g, 0.0);
        g.series(&[src, lp, dg, out]);
        sweep(g, lp.frequency(), t0, dust.f * 1.4, dust.f * 0.5, dust.decay);
        let decay = dust.decay * rng.range(0.8, 1.3);
        ad(g, dg.gain(), t0, dust.level * e, 0.02, decay);
        g.start_source(src, t0, dust.decay * 2.0 + 0.05);
        end = end.max(t0 + dust.decay * 2.0);
    }

    /* debris grains — chips, splinters, glass shards landing */
    let grains = js_round(s.grains * clamp(e, 0.3, 1.4)) as i64;
    for i in 0..grains {
        let gt = t0 + rng.range(0.015, 0.06) + i as f64 * rng.range(0.01, 0.055);
        let part = Partial::new(
            rng.range(1800.0, 9000.0),
            rng.range(12.0, 30.0),
            rng.range(0.02, 0.05) * e,
            rng.range(0.01, 0.05),
        );
        let r = struck(g, bank, rng, gt, &[part], 0.0018);
        g.connect(r, out);
        end = end.max(gt + 0.08);
    }

    /* water bubbles */
    if s.bubbles {
        for _ in 0..4 {
            let bt = t0 + rng.range(0.02, 0.18);
            let b = osc(g, Wave::Sine, rng.range(400.0, 1400.0));
            let bg = gain(g, 0.0);
            g.connect(b, bg);
            g.connect(bg, out);
            let from = rng.range(350.0, 700.0);
            let to = rng.range(900.0, 2200.0);
            sweep(g, b.frequency(), bt, from, to, 0.05);
            let peak = rng.range(0.04, 0.1) * e;
            hit(g, bg.gain(), bt, peak, 0.05);
            g.start(b, bt);
            g.stop(b, bt + 0.12);
            end = end.max(bt + 0.14);
        }
    }

    /* flesh squelch */
    if s.wet_squelch {
        let rate = rng.range(0.7, 1.1);
        let src = bank.one_shot(g, NoiseKind::Pink, rng, rate);
        let bp = biquad(g, FilterKind::Bandpass, 380.0, 2.2, 0.0);
        let sg = gain(g, 0.0);
        g.series(&[src, bp, sg, out]);
        sweep(g, bp.frequency(), t0, 260.0, 900.0, 0.09);
        ad(g, sg.gain(), t0 + 0.004, 0.4 * e, 0.006, 0.1);
        g.start_source(src, t0, 0.25);
    }

    Voice {
        node: out,
        end: end + 0.05,
        send: s.wet,
    }
}

/// `Math.round` — half rounds toward `+Infinity`, where `f64::round` rounds half
/// away from zero. Every argument in this file is non-negative, where the two
/// agree; the distinction is spelled out so the next reader does not have to
/// re-derive it.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/* ------------------------------------------------------------------ */
/* Footsteps                                                          */
/* ------------------------------------------------------------------ */

/// Per-surface footstep character (`foley.js:222-237`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Step {
    body_f: f64,
    body_decay: f64,
    tex_kind: NoiseKind,
    tex_f: f64,
    tex_q: f64,
    tex_decay: f64,
    tex_level: f64,
    scuff: f64,
    grit: u32,
    ring: &'static [Ring],
    splash: bool,
}

const fn step(
    body_f: f64,
    body_decay: f64,
    tex_kind: NoiseKind,
    tex_f: f64,
    tex_q: f64,
    tex_decay: f64,
    tex_level: f64,
    scuff: f64,
    grit: u32,
) -> Step {
    Step {
        body_f,
        body_decay,
        tex_kind,
        tex_f,
        tex_q,
        tex_decay,
        tex_level,
        scuff,
        grit,
        ring: &[],
        splash: false,
    }
}

static METAL_STEP_RING: [Ring; 3] = [
    Ring { f: 620.0, q: 16.0, g: 0.24, decay: 0.16 },
    Ring { f: 1480.0, q: 20.0, g: 0.16, decay: 0.11 },
    Ring { f: 2900.0, q: 14.0, g: 0.08, decay: 0.06 },
];
static WOOD_STEP_RING: [Ring; 2] = [
    Ring { f: 260.0, q: 12.0, g: 0.26, decay: 0.09 },
    Ring { f: 540.0, q: 9.0, g: 0.14, decay: 0.05 },
];

/// `STEP`, indexed by [`Surface::ALL`] (equivalently, [`Surface::index`]).
static STEP: [Step; 12] = [
    // concrete
    step(92.0, 0.055, NoiseKind::White, 2100.0, 0.7, 0.045, 0.5, 0.35, 4),
    // metal
    Step {
        ring: &METAL_STEP_RING,
        ..step(120.0, 0.05, NoiseKind::White, 3200.0, 1.0, 0.04, 0.5, 0.3, 2)
    },
    // wood
    Step {
        ring: &WOOD_STEP_RING,
        ..step(110.0, 0.06, NoiseKind::White, 1300.0, 0.8, 0.04, 0.4, 0.28, 2)
    },
    // dirt
    step(78.0, 0.07, NoiseKind::Brown, 620.0, 0.6, 0.075, 0.62, 0.45, 6),
    // sand
    step(70.0, 0.06, NoiseKind::White, 1500.0, 0.45, 0.14, 0.6, 0.7, 3),
    // glass
    step(96.0, 0.04, NoiseKind::Crackle, 5200.0, 0.8, 0.2, 0.6, 0.3, 9),
    // water
    Step {
        splash: true,
        ..step(88.0, 0.045, NoiseKind::White, 1600.0, 0.7, 0.17, 0.8, 0.5, 3)
    },
    // foliage
    step(84.0, 0.05, NoiseKind::Crackle, 2400.0, 0.7, 0.18, 0.7, 0.5, 6),
    // fabric
    step(82.0, 0.05, NoiseKind::White, 800.0, 0.6, 0.05, 0.3, 0.35, 0),
    // flesh
    step(86.0, 0.055, NoiseKind::White, 520.0, 1.2, 0.05, 0.35, 0.2, 0),
    // rubber
    step(96.0, 0.04, NoiseKind::White, 1000.0, 0.8, 0.03, 0.28, 0.2, 0),
    // plaster
    step(100.0, 0.05, NoiseKind::White, 1800.0, 0.7, 0.05, 0.45, 0.3, 4),
];

fn step_for(s: Surface) -> &'static Step {
    &STEP[usize::from(s.index())]
}

/// How the foot arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    Walk,
    Run,
    Sprint,
    Crouch,
    Land,
}

impl Gait {
    /// `foley.js:247` — the weight multiplier per gait. `walk` is the `??`
    /// default and also the final `: 0.62` arm.
    fn weight(self) -> f64 {
        match self {
            Gait::Sprint => 1.25,
            Gait::Run => 1.0,
            Gait::Land => 1.7,
            Gait::Crouch => 0.42,
            Gait::Walk => 0.62,
        }
    }

    /// `foley.js:323` — the default gear level when the caller does not pass one.
    fn default_gear(self) -> f64 {
        match self {
            Gait::Sprint => 1.0,
            Gait::Run => 0.7,
            Gait::Land => 0.9,
            Gait::Walk | Gait::Crouch => 0.25,
        }
    }

    /// `foley.js:290` — scuff duration by gait.
    fn scuff_dur(self) -> f64 {
        match self {
            Gait::Sprint => 0.13,
            Gait::Run => 0.1,
            _ => 0.07,
        }
    }
}

/// `footstep`'s options bag (`foley.js:240-241`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepOpts {
    pub when: Option<f64>,
    pub surface: Surface,
    pub gait: Gait,
    pub level: f64,
    /// `None` takes [`Gait::default_gear`].
    pub gear: Option<f64>,
}

impl Default for StepOpts {
    fn default() -> Self {
        StepOpts {
            when: None,
            surface: Surface::Concrete,
            gait: Gait::Walk,
            level: 1.0,
            gear: None,
        }
    }
}

/// One footstep (`foley.js:243-344`).
pub fn footstep(g: &mut AudioGraph, bank: &NoiseBank, rng: &mut Rng, o: StepOpts) -> Voice {
    let t0 = o.when.unwrap_or_else(|| g.current_time());
    let s = step_for(o.surface);
    let gait = o.gait;
    let weight = gait.weight();
    let lvl = o.level * weight;
    let jit = semis(rng.range(-3.0, 3.0));
    let out = gain(g, 0.32); // VOICE TRIM
    let mut end = t0 + 0.3;

    // heel/toe transient — two contacts, milliseconds apart, is what reads as a
    // foot rather than a hammer.
    let contacts = if gait == Gait::Land { 1 } else { 2 };
    for c in 0..contacts {
        let ct = t0 + if c == 0 { 0.0 } else { rng.range(0.012, 0.032) };
        let cl = if c == 0 { 1.0 } else { rng.range(0.35, 0.6) };

        let b = osc(g, Wave::Sine, s.body_f * jit);
        let bg = gain(g, 0.0);
        let curve = saturation_curve(g, 1.8, 0.5);
        let drv = shaper(g, curve, "2x");
        g.connect(b, bg);
        g.series(&[bg, drv, out]);
        sweep(
            g,
            b.frequency(),
            ct,
            s.body_f * jit * 1.7,
            s.body_f * jit * 0.75,
            s.body_decay * 1.4,
        );
        let decay = s.body_decay * rng.range(0.85, 1.2);
        ad(g, bg.gain(), ct, 0.42 * lvl * cl, 0.0025, decay);
        g.start(b, ct);
        g.stop(b, ct + s.body_decay * 2.4 + 0.02);

        let rate = rng.range(0.8, 1.25);
        let src = bank.one_shot(g, s.tex_kind, rng, rate);
        let bp = biquad(g, FilterKind::Bandpass, s.tex_f * jit, s.tex_q, 0.0);
        let tg = gain(g, 0.0);
        g.series(&[src, bp, tg, out]);
        sweep(
            g,
            bp.frequency(),
            ct,
            s.tex_f * 1.4 * jit,
            s.tex_f * 0.55 * jit,
            s.tex_decay * 2.0,
        );
        let tdecay = s.tex_decay * rng.range(0.8, 1.3);
        ad(g, tg.gain(), ct, s.tex_level * lvl * cl, 0.002, tdecay);
        g.start_source(src, ct, s.tex_decay * 3.0 + 0.05);
        end = end.max(ct + s.tex_decay * 3.0);

        if !s.ring.is_empty() && c == 0 {
            let parts: Vec<Partial> = s
                .ring
                .iter()
                .map(|p| {
                    Partial::new(
                        p.f * semis(rng.range(-2.0, 2.0)),
                        p.q * rng.range(0.85, 1.2),
                        p.g * lvl,
                        p.decay * rng.range(0.8, 1.25),
                    )
                })
                .collect();
            let r = struck(g, bank, rng, ct, &parts, 0.003);
            g.connect(r, out);
            end = end.max(ct + 0.3);
        }
    }

    /* scuff — the slide of the sole, longer when running */
    if s.scuff > 0.05 {
        let st = t0 + rng.range(0.01, 0.04);
        let dur = gait.scuff_dur() * rng.range(0.8, 1.3);
        let rate = rng.range(0.85, 1.2);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let bp = biquad(
            g,
            FilterKind::Bandpass,
            rng.range(2200.0, 4200.0),
            0.8,
            0.0,
        );
        let sg = gain(g, 0.0);
        g.series(&[src, bp, sg, out]);
        let from = rng.range(2800.0, 4600.0);
        let to = rng.range(1200.0, 2000.0);
        sweep(g, bp.frequency(), st, from, to, dur);
        ad(g, sg.gain(), st, s.scuff * lvl * 0.5, 0.012, dur);
        g.start_source(src, st, dur * 2.0);
        end = end.max(st + dur * 2.0);
    }

    /* grit grains */
    for _ in 0..s.grit {
        if rng.float() > 0.55 {
            continue;
        }
        let gt = t0 + rng.range(0.004, 0.09);
        let part = Partial::new(
            rng.range(2400.0, 9000.0),
            rng.range(10.0, 26.0),
            rng.range(0.015, 0.05) * lvl,
            rng.range(0.008, 0.03),
        );
        let r = struck(g, bank, rng, gt, &[part], 0.0015);
        g.connect(r, out);
    }

    /* water splash */
    if s.splash {
        let rate = rng.range(0.9, 1.2);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let bp = biquad(g, FilterKind::Bandpass, 900.0, 0.7, 0.0);
        let sg = gain(g, 0.0);
        g.series(&[src, bp, sg, out]);
        sweep(g, bp.frequency(), t0, 700.0, 3400.0, 0.16);
        ad(g, sg.gain(), t0 + 0.006, 0.45 * lvl, 0.01, 0.2);
        g.start_source(src, t0, 0.4);
        end = end.max(t0 + 0.42);
    }

    /* gear: sling swivels, mag pouches, buckles — only when moving fast */
    let gear = o.gear.unwrap_or_else(|| gait.default_gear());
    if gear > 0.05 {
        let n = 1 + rng.u32() % 3;
        for _ in 0..n {
            let gt = t0 + rng.range(0.005, 0.11);
            let parts = [
                Partial::new(
                    rng.range(1600.0, 4200.0),
                    rng.range(18.0, 40.0),
                    rng.range(0.03, 0.1) * gear * lvl,
                    rng.range(0.03, 0.12),
                ),
                Partial::new(
                    rng.range(4200.0, 8000.0),
                    rng.range(12.0, 26.0),
                    rng.range(0.01, 0.04) * gear * lvl,
                    rng.range(0.01, 0.05),
                ),
            ];
            let r = struck(g, bank, rng, gt, &parts, 0.002);
            g.connect(r, out);
            end = end.max(gt + 0.18);
        }
        // Cloth/webbing rustle.
        let rate = rng.range(0.7, 1.1);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let bp = biquad(
            g,
            FilterKind::Bandpass,
            rng.range(1400.0, 2600.0),
            0.6,
            0.0,
        );
        let rg = gain(g, 0.0);
        g.series(&[src, bp, rg, out]);
        ad(g, rg.gain(), t0, 0.09 * gear * lvl, 0.02, 0.13);
        g.start_source(src, t0, 0.3);
    }

    Voice {
        node: out,
        end: end + 0.05,
        send: 0.3,
    }
}

/// Cloth movement, used for stance changes and ADS (`foley.js:347-365`).
pub fn cloth(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    when: Option<f64>,
    level: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let out = gain(g, 1.0);
    let dur = rng.range(0.13, 0.26);
    let rate = rng.range(0.7, 1.15);
    let src = bank.one_shot(g, NoiseKind::White, rng, rate);
    let bp = biquad(
        g,
        FilterKind::Bandpass,
        rng.range(1300.0, 2400.0),
        0.55,
        0.0,
    );
    let cg = gain(g, 0.0);
    g.series(&[src, bp, cg, out]);
    let from = rng.range(1000.0, 1600.0);
    let to = rng.range(2200.0, 3400.0);
    sweep(g, bp.frequency(), t0, from, to, dur);
    ad(g, cg.gain(), t0, 0.3 * level, 0.03, dur);
    g.start_source(src, t0, dur * 2.0);
    if rng.float() < 0.6 {
        let t = t0 + rng.range(0.02, 0.1);
        let part = Partial::new(
            rng.range(2200.0, 5200.0),
            26.0,
            0.05 * level,
            rng.range(0.03, 0.09),
        );
        let r = struck(g, bank, rng, t, &[part], 0.002);
        g.connect(r, out);
    }
    Voice {
        node: out,
        end: t0 + dur * 2.0 + 0.15,
        send: 0.2,
    }
}

/* ------------------------------------------------------------------ */
/* Shell casings                                                      */
/* ------------------------------------------------------------------ */

/// A casing bounces 2–4 times with shortening intervals and then rolls
/// (`foley.js:377-413`). Brass on concrete is one of the most recognisable
/// sounds in a shooter; the trick is that each bounce is a *different* set of
/// partials because the shell lands on a different part of itself.
pub fn shell_casing(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    when: Option<f64>,
    surface: Surface,
    level: f64,
    flight: Option<f64>,
) -> Voice {
    let base_when = when.unwrap_or_else(|| g.current_time());
    let t0 = base_when + flight.unwrap_or_else(|| rng.range(0.28, 0.52));
    let hard = matches!(
        surface,
        Surface::Metal | Surface::Concrete | Surface::Glass | Surface::Plaster
    );
    let soft = matches!(
        surface,
        Surface::Dirt | Surface::Sand | Surface::Foliage | Surface::Fabric
    );
    let out = gain(g, 1.0);
    let base = rng.range(2650.0, 4200.0);
    let mut t = t0;
    let mut amp = level * if soft { 0.35 } else { 1.0 } * rng.range(0.8, 1.1);
    // The ternary short-circuits: a soft landing does not draw.
    let bounces = if soft { 1 } else { 2 + rng.u32() % 3 };
    let mut end = t0;
    for i in 0..bounces {
        let detune = semis(rng.range(-4.0, 4.0));
        let parts = [
            Partial::new(
                base * detune,
                rng.range(30.0, 60.0),
                0.95 * amp,
                rng.range(0.05, 0.13) * if hard { 1.0 } else { 0.5 },
            ),
            Partial::new(
                base * detune * 1.87,
                rng.range(24.0, 44.0),
                0.58 * amp,
                rng.range(0.03, 0.08),
            ),
            Partial::new(
                base * detune * 3.1,
                rng.range(16.0, 30.0),
                0.3 * amp,
                rng.range(0.015, 0.04),
            ),
            Partial::new(base * detune * 0.42, 12.0, 0.2 * amp, 0.03),
        ];
        let r = struck(g, bank, rng, t, &parts, 0.0015);
        g.connect(r, out);
        end = t + 0.2;
        t += rng.range(0.045, 0.13) * if i == 0 { 1.0 } else { 0.6 };
        amp *= rng.range(0.38, 0.62);
        if amp < 0.03 {
            break;
        }
    }
    // Roll: a stream of very quiet, very short pings.
    if hard && rng.float() < 0.55 {
        let rolls = 3 + rng.u32() % 5;
        for i in 0..rolls {
            let rt = t + f64::from(i) * rng.range(0.018, 0.05);
            let part = Partial::new(
                base * semis(rng.range(-5.0, 5.0)),
                rng.range(20.0, 44.0),
                rng.range(0.03, 0.09) * level,
                rng.range(0.01, 0.03),
            );
            let r = struck(g, bank, rng, rt, &[part], 0.0012);
            g.connect(r, out);
            end = end.max(rt + 0.06);
        }
    }
    Voice {
        node: out,
        end: end + 0.05,
        send: 0.42,
    }
}

/* ------------------------------------------------------------------ */
/* Reload foley                                                       */
/* ------------------------------------------------------------------ */

/// The four reload phases (`foley.js:425`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadPhase {
    Start,
    MagOut,
    MagIn,
    /// Also the `default:` arm — an unrecognised phase plays the charging handle.
    End,
}

impl ReloadPhase {
    /// `RELOAD_TRIM` — the four phases are wildly different in energy.
    fn trim(self) -> f64 {
        match self {
            ReloadPhase::Start => 3.2,
            ReloadPhase::MagOut => 3.0,
            ReloadPhase::MagIn => 1.0,
            ReloadPhase::End => 1.5,
        }
    }

    pub fn from_str(name: &str) -> ReloadPhase {
        match name {
            "start" => ReloadPhase::Start,
            "magout" => ReloadPhase::MagOut,
            "magin" => ReloadPhase::MagIn,
            _ => ReloadPhase::End,
        }
    }
}

/// `reloadPhase`'s local `metal(t, parts, exc)` closure (`foley.js:432-435`).
#[allow(clippy::too_many_arguments)]
fn reload_metal(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    out: NodeId,
    end: &mut f64,
    t: f64,
    parts: &[Partial],
    exc: f64,
) {
    let r = struck(g, bank, rng, t, parts, exc);
    g.connect(r, out);
    *end = end.max(t + 0.35);
}

/// `reloadPhase`'s local `rustle(t, dur, level, f)` closure (`foley.js:436-443`).
#[allow(clippy::too_many_arguments)]
fn reload_rustle(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    out: NodeId,
    end: &mut f64,
    t: f64,
    dur: f64,
    level: f64,
    f: f64,
) {
    let rate = rng.range(0.8, 1.2);
    let src = bank.one_shot(g, NoiseKind::White, rng, rate);
    let bp = biquad(g, FilterKind::Bandpass, f, 0.6, 0.0);
    let rg = gain(g, 0.0);
    g.series(&[src, bp, rg, out]);
    ad(g, rg.gain(), t, level, 0.02, dur);
    g.start_source(src, t, dur * 2.0 + 0.05);
    *end = end.max(t + dur * 2.0);
}

/// Reload mechanics, one call per `weapon:reload` phase (`foley.js:427-543`).
///
/// Keeping the phases as separate one-shots (instead of one long sound) is what
/// lets the audio stay locked to the animation whatever its length.
pub fn reload_phase(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    phase: ReloadPhase,
    when: Option<f64>,
    heavy: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let out = gain(g, 0.42 * phase.trim()); // VOICE TRIM
    let mut end = t0 + 0.3;

    match phase {
        ReloadPhase::Start => {
            // Hand leaves the grip, palm slaps the magwell, mag catch is pressed.
            let f = rng.range(1400.0, 2200.0);
            reload_rustle(g, bank, rng, out, &mut end, t0, 0.18, 0.2, f);
            let t = t0 + rng.range(0.04, 0.08);
            let parts = [
                Partial::new(2450.0 * semis(rng.range(-2.0, 2.0)), 30.0, 0.55 * heavy, 0.03),
                Partial::new(5100.0, 18.0, 0.25, 0.016),
                Partial::new(780.0, 10.0, 0.3 * heavy, 0.045),
            ];
            reload_metal(g, bank, rng, out, &mut end, t, &parts, 0.0025);
        }

        ReloadPhase::MagOut => {
            // Spring release, mag scrapes out of the well, then plastic hits the deck.
            let parts = [
                Partial::new(1650.0 * semis(rng.range(-2.0, 2.0)), 24.0, 0.65 * heavy, 0.05),
                Partial::new(3400.0, 16.0, 0.35, 0.025),
            ];
            reload_metal(g, bank, rng, out, &mut end, t0, &parts, 0.0025);
            let st = t0 + 0.03;
            let rate = rng.range(0.9, 1.3);
            let src = bank.one_shot(g, NoiseKind::White, rng, rate);
            let bp = biquad(g, FilterKind::Bandpass, 3200.0, 1.1, 0.0);
            let sg = gain(g, 0.0);
            g.series(&[src, bp, sg, out]);
            sweep(g, bp.frequency(), st, 4200.0, 1600.0, 0.12);
            ad(g, sg.gain(), st, 0.2, 0.01, 0.12);
            g.start_source(src, st, 0.3);
            // Empty magazine hitting the ground — polymer, not metal.
            let dt = t0 + rng.range(0.16, 0.3);
            let drop = [
                Partial::new(480.0 * semis(rng.range(-3.0, 3.0)), 9.0, 0.2, 0.05),
                Partial::new(1180.0, 7.0, 0.11, 0.03),
                Partial::new(2600.0, 5.0, 0.05, 0.015),
            ];
            reload_metal(g, bank, rng, out, &mut end, dt, &drop, 0.004);
            end = end.max(dt + 0.3);
        }

        ReloadPhase::MagIn => {
            // Fresh mag guided in, seated with a palm strike: a low thunk plus a
            // sharp latch click. The thunk needs real low end or it feels
            // weightless.
            let f = rng.range(1200.0, 2000.0);
            reload_rustle(g, bank, rng, out, &mut end, t0, 0.12, 0.16, f);
            let it = t0 + rng.range(0.05, 0.1);
            let b = osc(g, Wave::Sine, 190.0 * heavy);
            let bg = gain(g, 0.0);
            let curve = saturation_curve(g, 3.0, 0.5);
            let drv = shaper(g, curve, "2x");
            g.connect(b, bg);
            g.series(&[bg, drv, out]);
            sweep(g, b.frequency(), it, 230.0 * heavy, 110.0 * heavy, 0.06);
            ad(g, bg.gain(), it, 0.4 * heavy, 0.002, 0.055);
            g.start(b, it);
            g.stop(b, it + 0.16);
            let seat = [
                Partial::new(1250.0 * semis(rng.range(-2.0, 2.0)), 20.0, 0.3 * heavy, 0.06),
                Partial::new(2800.0, 26.0, 0.18, 0.03),
                Partial::new(6200.0, 14.0, 0.07, 0.012),
            ];
            reload_metal(g, bank, rng, out, &mut end, it, &seat, 0.003);
            let latch_t = it + rng.range(0.02, 0.05);
            let latch = [
                Partial::new(3600.0, 34.0, 0.16, 0.02),
                Partial::new(7400.0, 20.0, 0.07, 0.01),
            ];
            reload_metal(g, bank, rng, out, &mut end, latch_t, &latch, 0.0015);
        }

        ReloadPhase::End => {
            // Charging handle: scrape, hard rearward stop, spring-driven return,
            // and the bolt slamming into battery.
            let st = t0;
            let rate = rng.range(0.9, 1.25);
            let src = bank.one_shot(g, NoiseKind::White, rng, rate);
            let bp = biquad(g, FilterKind::Bandpass, 2600.0, 1.6, 0.0);
            let sg = gain(g, 0.0);
            g.series(&[src, bp, sg, out]);
            sweep(g, bp.frequency(), st, 1800.0, 4200.0, 0.07);
            ad(g, sg.gain(), st, 0.24, 0.008, 0.07);
            g.start_source(src, st, 0.2);
            let stop_parts = [
                Partial::new(1450.0 * semis(rng.range(-2.0, 2.0)), 22.0, 0.3 * heavy, 0.05),
                Partial::new(3100.0, 18.0, 0.16, 0.022),
            ];
            reload_metal(g, bank, rng, out, &mut end, st + 0.06, &stop_parts, 0.0025);
            // Spring ring — the metallic "zing" behind the clack.
            let spring = [
                Partial::new(4900.0 * semis(rng.range(-3.0, 3.0)), 46.0, 0.09, 0.16),
                Partial::new(7200.0, 38.0, 0.05, 0.1),
            ];
            reload_metal(g, bank, rng, out, &mut end, st + 0.065, &spring, 0.002);
            let bt = st + rng.range(0.1, 0.15);
            let b = osc(g, Wave::Sine, 150.0 * heavy);
            let bg = gain(g, 0.0);
            g.connect(b, bg);
            g.connect(bg, out);
            sweep(g, b.frequency(), bt, 200.0 * heavy, 90.0 * heavy, 0.05);
            ad(g, bg.gain(), bt, 0.38 * heavy, 0.0015, 0.05);
            g.start(b, bt);
            g.stop(b, bt + 0.14);
            let battery = [
                Partial::new(1750.0, 20.0, 0.34 * heavy, 0.05),
                Partial::new(3900.0, 15.0, 0.15, 0.02),
                Partial::new(8200.0, 10.0, 0.05, 0.008),
            ];
            reload_metal(g, bank, rng, out, &mut end, bt, &battery, 0.0035);
        }
    }
    Voice {
        node: out,
        end: end + 0.05,
        send: 0.3,
    }
}

/* ------------------------------------------------------------------ */
/* Explosions                                                         */
/* ------------------------------------------------------------------ */

/// An explosion (`foley.js:554-631`).
///
/// Near: a violent transient, a huge sub sweep and a bright shrapnel spatter.
/// Far: almost no transient, a long rolling low rumble, and a big wet tail.
pub fn explosion(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    when: Option<f64>,
    distance: f64,
    radius: f64,
    level: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let dist = distance.max(0.0);
    let size = clamp(radius / 6.0, 0.5, 2.4);
    let near = clamp(1.0 - dist / 70.0, 0.0, 1.0);
    let far = 1.0 - near;
    let lvl = level * size;
    let out = gain(g, 0.42); // VOICE TRIM
    let mut end = t0 + 1.0;

    /* detonation transient */
    if near > 0.05 {
        let rate = rng.range(0.9, 1.2);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let hp = biquad(g, FilterKind::Highpass, 1800.0, 0.6, 0.0);
        let curve = saturation_curve(g, 14.0, 0.7);
        let drv = shaper(g, curve, "4x");
        let dg = gain(g, 0.0);
        g.series(&[src, hp, drv, dg, out]);
        hit(g, dg.gain(), t0, 0.85 * near * lvl, 0.02);
        g.start_source(src, t0, 0.1);
    }

    /* sub-bass impact: the thing you feel in your chest */
    {
        let s = osc(g, Wave::Sine, 110.0);
        let s2 = osc(g, Wave::Triangle, 62.0);
        let sg = gain(g, 0.0);
        let curve = saturation_curve(g, 4.0, 0.6);
        let drv = shaper(g, curve, "2x");
        let lp = biquad(g, FilterKind::Lowpass, 220.0, 0.9, 0.0);
        g.connect(s, sg);
        g.connect(s2, sg);
        g.series(&[sg, drv, lp, out]);
        let sub_dur = (0.55 + size * 0.35) * rng.range(0.9, 1.15);
        sweep(g, s.frequency(), t0, 130.0 * size, 26.0, sub_dur);
        sweep(g, s2.frequency(), t0, 74.0 * size, 21.0, sub_dur * 1.2);
        ad(
            g,
            sg.gain(),
            t0,
            1.0 * lvl * (0.55 + near * 0.6),
            0.008 + far * 0.05,
            sub_dur,
        );
        g.start(s, t0);
        g.start(s2, t0);
        g.stop(s, t0 + sub_dur * 1.6);
        g.stop(s2, t0 + sub_dur * 1.6);
        end = end.max(t0 + sub_dur * 1.6);
    }

    /* blast body: broadband noise under a fast-falling lowpass */
    {
        let dur = (0.45 + size * 0.5) * (1.0 + far * 1.8);
        let rate = rng.range(0.6, 1.1);
        let src = bank.one_shot(g, NoiseKind::Brown, rng, rate);
        let lp = biquad(g, FilterKind::Lowpass, 6000.0, 0.8, 0.0);
        let curve = saturation_curve(g, 6.0, 0.5);
        let drv = shaper(g, curve, "2x");
        let bg = gain(g, 0.0);
        g.series(&[src, lp, drv, bg, out]);
        sweep(
            g,
            lp.frequency(),
            t0,
            lerp(7000.0, 700.0, far),
            lerp(260.0, 130.0, far),
            dur,
        );
        ad(g, bg.gain(), t0, 0.8 * lvl, 0.01 + far * 0.06, dur);
        g.start_source(src, t0, dur * 1.4 + 0.1);
        end = end.max(t0 + dur * 1.4);
    }

    /* debris / shrapnel: grains scattered over the following second */
    let grains = js_round(lerp(26.0, 4.0, far) * size) as i64;
    for _ in 0..grains {
        let gt = t0 + rng.range(0.02, 0.9) * rng.range(0.3, 1.0);
        let part = Partial::new(
            rng.range(700.0, 7000.0),
            rng.range(8.0, 32.0),
            rng.range(0.02, 0.09) * near * lvl,
            rng.range(0.01, 0.09),
        );
        let r = struck(g, bank, rng, gt, &[part], 0.002);
        g.connect(r, out);
        end = end.max(gt + 0.15);
    }

    /* dust and settling */
    {
        let dur = 1.0 + size * 0.8;
        let rate = rng.range(0.5, 0.9);
        let src = bank.one_shot(g, NoiseKind::Pink, rng, rate);
        let lp = biquad(g, FilterKind::Lowpass, 1400.0, 0.7, 0.0);
        let dg = gain(g, 0.0);
        g.series(&[src, lp, dg, out]);
        sweep(g, lp.frequency(), t0, 1600.0, 320.0, dur);
        ad(
            g,
            dg.gain(),
            t0 + 0.05,
            0.2 * lvl * (0.4 + near * 0.6),
            0.12,
            dur,
        );
        g.start_source(src, t0 + 0.05, dur * 1.3);
        end = end.max(t0 + dur * 1.3);
    }

    Voice {
        node: out,
        end: end + 0.1,
        send: 0.85 + far * 0.5,
    }
}

/// A body hitting the ground: mass, gear, and a wet slap (`foley.js:634-660`).
pub fn body_fall(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    when: Option<f64>,
    level: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let out = gain(g, 0.4); // VOICE TRIM
    let b = osc(g, Wave::Sine, 74.0);
    let bg = gain(g, 0.0);
    let curve = saturation_curve(g, 2.2, 0.55);
    let drv = shaper(g, curve, "2x");
    g.connect(b, bg);
    g.series(&[bg, drv, out]);
    sweep(g, b.frequency(), t0, 96.0, 44.0, 0.12);
    ad(g, bg.gain(), t0, 0.6 * level, 0.004, 0.13);
    g.start(b, t0);
    g.stop(b, t0 + 0.35);

    let rate = rng.range(0.7, 1.1);
    let src = bank.one_shot(g, NoiseKind::White, rng, rate);
    let lp = biquad(g, FilterKind::Lowpass, 900.0, 0.8, 0.0);
    let sg = gain(g, 0.0);
    g.series(&[src, lp, sg, out]);
    ad(g, sg.gain(), t0, 0.35 * level, 0.006, 0.16);
    g.start_source(src, t0, 0.4);

    for _ in 0..5 {
        let gt = t0 + rng.range(0.005, 0.26);
        let part = Partial::new(
            rng.range(1500.0, 5200.0),
            rng.range(16.0, 40.0),
            rng.range(0.03, 0.09) * level,
            rng.range(0.02, 0.1),
        );
        let r = struck(g, bank, rng, gt, &[part], 0.002);
        g.connect(r, out);
    }
    Voice {
        node: out,
        end: t0 + 0.6,
        send: 0.4,
    }
}

/* ------------------------------------------------------------------ */
/* UI                                                                 */
/* ------------------------------------------------------------------ */

/// Non-diegetic feedback (`foley.js:667-769`). Short, dry, deliberately
/// synthetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSound {
    Hitmarker,
    Headshot,
    Kill,
    Damage,
    Armour,
    GrenadeWarn,
    Regen,
    LowHealth,
    /// The `default:` arm — a plain 1.2 kHz blip for any unrecognised name.
    Blip,
}

impl UiSound {
    pub fn as_str(self) -> &'static str {
        match self {
            UiSound::Hitmarker => "hitmarker",
            UiSound::Headshot => "headshot",
            UiSound::Kill => "kill",
            UiSound::Damage => "damage",
            UiSound::Armour => "armour",
            UiSound::GrenadeWarn => "grenade_warn",
            UiSound::Regen => "regen",
            UiSound::LowHealth => "lowhealth",
            UiSound::Blip => "blip",
        }
    }

    pub fn from_str(name: &str) -> UiSound {
        match name {
            "hitmarker" => UiSound::Hitmarker,
            "headshot" => UiSound::Headshot,
            "kill" => UiSound::Kill,
            "damage" => UiSound::Damage,
            "armour" => UiSound::Armour,
            "grenade_warn" => UiSound::GrenadeWarn,
            "regen" => UiSound::Regen,
            "lowhealth" => UiSound::LowHealth,
            _ => UiSound::Blip,
        }
    }
}

pub fn ui_sound(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    kind: UiSound,
    when: Option<f64>,
    level: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let out = gain(g, 1.0);
    let lvl = level;
    match kind {
        UiSound::Hitmarker => {
            let o1 = osc(g, Wave::Square, 2400.0);
            let ug = gain(g, 0.0);
            let lp = biquad(g, FilterKind::Lowpass, 5200.0, 0.7, 0.0);
            g.connect(o1, ug);
            g.series(&[ug, lp, out]);
            hit(g, ug.gain(), t0, 0.55 * lvl, 0.022);
            g.start(o1, t0);
            g.stop(o1, t0 + 0.06);
        }
        UiSound::Headshot => {
            let o1 = osc(g, Wave::Square, 3200.0);
            let o2 = osc(g, Wave::Square, 4800.0);
            let ug = gain(g, 0.0);
            g.connect(o1, ug);
            g.connect(o2, ug);
            g.connect(ug, out);
            hit(g, ug.gain(), t0, 0.34 * lvl, 0.05);
            g.start(o1, t0);
            g.start(o2, t0 + 0.03);
            g.stop(o1, t0 + 0.12);
            g.stop(o2, t0 + 0.14);
        }
        UiSound::Kill => {
            for i in 0..3 {
                let o1 = osc(g, Wave::Triangle, 900.0 * 1.5f64.powi(i));
                let ug = gain(g, 0.0);
                g.connect(o1, ug);
                g.connect(ug, out);
                let at = t0 + f64::from(i) * 0.055;
                ad(g, ug.gain(), at, 0.3 * lvl, 0.004, 0.09);
                g.start(o1, at);
                g.stop(o1, at + 0.2);
            }
        }
        UiSound::Damage => {
            // Directional pain sting: a dissonant low pair, no melody.
            let o1 = osc(g, Wave::Sawtooth, 180.0);
            let o2 = osc(g, Wave::Sawtooth, 191.0);
            let ug = gain(g, 0.0);
            let lp = biquad(g, FilterKind::Lowpass, 1400.0, 1.4, 0.0);
            g.connect(o1, ug);
            g.connect(o2, ug);
            g.series(&[ug, lp, out]);
            ad(g, ug.gain(), t0, 0.42 * lvl, 0.004, 0.22);
            g.start(o1, t0);
            g.start(o2, t0);
            g.stop(o1, t0 + 0.4);
            g.stop(o2, t0 + 0.4);
        }
        UiSound::Armour => {
            // Ceramic plate strike: brighter and harder than a flesh hitmarker.
            let parts = [
                Partial::new(3900.0, 30.0, 0.09 * lvl, 0.045),
                Partial::new(6400.0, 22.0, 0.05 * lvl, 0.025),
            ];
            let r = struck(g, bank, rng, t0, &parts, 0.0015);
            g.connect(r, out);
        }
        UiSound::GrenadeWarn => {
            // Three rising beeps — reads as "danger", not as a notification.
            for i in 0..3 {
                let bt = t0 + f64::from(i) * 0.14;
                let o1 = osc(g, Wave::Square, 1150.0 * 1.19f64.powi(i));
                let lp = biquad(g, FilterKind::Lowpass, 4200.0, 0.8, 0.0);
                let ug = gain(g, 0.0);
                g.connect(o1, ug);
                g.series(&[ug, lp, out]);
                ad(g, ug.gain(), bt, 0.3 * lvl, 0.004, 0.07);
                g.start(o1, bt);
                g.stop(o1, bt + 0.16);
            }
        }
        UiSound::Regen => {
            // Soft filtered swell: the "you are OK now" cue. Deliberately
            // unpitched.
            let src = bank.one_shot(g, NoiseKind::Pink, rng, 0.9);
            let bp = biquad(g, FilterKind::Bandpass, 700.0, 1.1, 0.0);
            let sg = gain(g, 0.0);
            g.series(&[src, bp, sg, out]);
            sweep(g, bp.frequency(), t0, 500.0, 1900.0, 0.5);
            ad(g, sg.gain(), t0, 0.3 * lvl, 0.15, 0.45);
            g.start_source(src, t0, 0.9);
            let o1 = osc(g, Wave::Sine, 420.0);
            let og = gain(g, 0.0);
            g.connect(o1, og);
            g.connect(og, out);
            sweep(g, o1.frequency(), t0, 380.0, 640.0, 0.45);
            ad(g, og.gain(), t0, 0.12 * lvl, 0.14, 0.4);
            g.start(o1, t0);
            g.stop(o1, t0 + 0.8);
        }
        UiSound::LowHealth => {
            let o1 = osc(g, Wave::Sine, 92.0);
            let ug = gain(g, 0.0);
            g.connect(o1, ug);
            g.connect(ug, out);
            ad(g, ug.gain(), t0, 0.45 * lvl, 0.05, 0.55);
            g.start(o1, t0);
            g.stop(o1, t0 + 0.9);
        }
        UiSound::Blip => {
            let o1 = osc(g, Wave::Sine, 1200.0);
            let ug = gain(g, 0.0);
            g.connect(o1, ug);
            g.connect(ug, out);
            hit(g, ug.gain(), t0, 0.26 * lvl, 0.03);
            g.start(o1, t0);
            g.stop(o1, t0 + 0.08);
        }
    }
    Voice {
        node: out,
        end: t0 + 0.9,
        send: 0.0,
    }
}

/// Heartbeat + laboured breathing for low health (`foley.js:775-789`). Returned
/// so the caller can schedule it repeatedly rather than looping a node.
pub fn heartbeat(g: &mut AudioGraph, when: Option<f64>, level: f64) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let out = gain(g, 0.5); // VOICE TRIM
    for i in 0..2 {
        let bt = t0 + f64::from(i) * 0.19;
        let b = osc(g, Wave::Sine, 58.0);
        let bg = gain(g, 0.0);
        g.connect(b, bg);
        g.connect(bg, out);
        sweep(g, b.frequency(), bt, 72.0, 42.0, 0.1);
        let peak = if i == 0 { 0.5 } else { 0.33 } * level;
        ad(g, bg.gain(), bt, peak, 0.008, 0.11);
        g.start(b, bt);
        g.stop(b, bt + 0.3);
    }
    Voice {
        node: out,
        end: t0 + 0.6,
        send: 0.1,
    }
}
