//! Weapon fire — the seven-layer additive gunshot.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/weapons.js:1-363` — the whole
//! file.
//!
//! A gunshot is not one sound. Every layer below exists in real recordings and
//! removing any one of them is immediately audible:
//!
//!   1. **TRANSIENT** sub-millisecond click — the pressure step. Gives the shot
//!      its "instant" feel; without it the gun sounds like a firework.
//!   2. **BODY** a fast downward-swept sine/triangle pair, saturated. This is
//!      the chest thump, the layer people describe as "punch".
//!   3. **CRACK** resonant band-passed noise around 1.5–3.5 kHz driven into
//!      saturation. Calibre character lives here.
//!   4. **MID** a short 500–900 Hz noise body that glues 2 and 3 together.
//!   5. **TAIL** a broadband burst under a falling lowpass, fed hard into the
//!      reverb send — this is what the *room* hears.
//!   6. **MECH** the bolt/action: a separate, drier, later metallic layer. It is
//!      what makes a weapon feel mechanical rather than sampled.
//!   7. **BOOM** (distance only) a slow, dark, rolling low-frequency swell plus
//!      a ground-bounce repeat.
//!
//! Variation: each profile owns a round-robin table of 6 timbre variants, and on
//! top of that every shot gets fresh pitch/level/decay jitter from the rng. Two
//! consecutive rounds are never the same waveform, which is the single biggest
//! difference between "synthesized game audio" and "a looping sample".

use std::collections::HashMap;

use crate::audio::dsp::{
    ad, biquad, clamp, gain, hit, lerp, osc, saturation_curve, semis, shaper, struck,
    sweep, NoiseBank, NoiseKind, Partial,
};
use crate::audio::graph::{AudioGraph, FilterKind, NodeId, Wave};
use crate::rng::Rng;

/// Per-weapon character (`weapons.js:36-87`). Frequencies in Hz, times in
/// seconds. `level` is a linear trim; the mix expects ~1.0 for a 5.56 rifle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponProfile {
    /// The `WEAPON_PROFILES` key. Not a field in the source — the port needs a
    /// name to key the round-robin table on, because the source caches that
    /// table by mutating the profile object itself (see [`RoundRobinBank`]).
    pub name: &'static str,
    pub level: f64,
    pub body_f: f64,
    pub body_f2: f64,
    pub body_decay: f64,
    pub sub_f: f64,
    pub sub_decay: f64,
    pub crack_f: f64,
    pub crack_q: f64,
    pub crack_decay: f64,
    pub drive: f64,
    pub asym: f64,
    pub mid_f: f64,
    pub mid_decay: f64,
    pub tail_decay: f64,
    pub tail_f: f64,
    pub tail_end_f: f64,
    pub mech_delay: f64,
    pub mech_level: f64,
    pub mech_partials: [f64; 3],
    pub send: f64,
    /// `profile.pellets` — absent (falsy) on every profile but the shotgun, so
    /// zero stands in for `undefined`.
    pub pellets: u32,
    pub suppressed: bool,
}

/// A profile with the fields every entry shares, so the eight tables below read
/// as the eight lines of data they are in the source.
const fn profile(name: &'static str) -> WeaponProfile {
    WeaponProfile {
        name,
        level: 1.0,
        body_f: 0.0,
        body_f2: 0.0,
        body_decay: 0.0,
        sub_f: 0.0,
        sub_decay: 0.0,
        crack_f: 0.0,
        crack_q: 0.0,
        crack_decay: 0.0,
        drive: 0.0,
        asym: 0.0,
        mid_f: 0.0,
        mid_decay: 0.0,
        tail_decay: 0.0,
        tail_f: 0.0,
        tail_end_f: 0.0,
        mech_delay: 0.0,
        mech_level: 0.0,
        mech_partials: [0.0; 3],
        send: 0.0,
        pellets: 0,
        suppressed: false,
    }
}

pub static RIFLE: WeaponProfile = WeaponProfile {
    level: 1.0, body_f: 148.0, body_f2: 56.0, body_decay: 0.085, sub_f: 62.0, sub_decay: 0.12,
    crack_f: 2450.0, crack_q: 0.95, crack_decay: 0.055, drive: 6.0, asym: 0.35,
    mid_f: 780.0, mid_decay: 0.05, tail_decay: 0.3, tail_f: 5200.0, tail_end_f: 700.0,
    mech_delay: 0.028, mech_level: 0.42, mech_partials: [1880.0, 3260.0, 5400.0], send: 0.5,
    ..profile("rifle")
};

pub static AK: WeaponProfile = WeaponProfile {
    level: 1.1, body_f: 124.0, body_f2: 46.0, body_decay: 0.105, sub_f: 52.0, sub_decay: 0.15,
    crack_f: 1780.0, crack_q: 0.9, crack_decay: 0.07, drive: 7.5, asym: 0.5,
    mid_f: 640.0, mid_decay: 0.06, tail_decay: 0.42, tail_f: 4200.0, tail_end_f: 560.0,
    mech_delay: 0.034, mech_level: 0.55, mech_partials: [1420.0, 2650.0, 4300.0], send: 0.55,
    ..profile("ak")
};

pub static SMG: WeaponProfile = WeaponProfile {
    level: 0.84, body_f: 172.0, body_f2: 72.0, body_decay: 0.06, sub_f: 78.0, sub_decay: 0.08,
    crack_f: 3050.0, crack_q: 1.05, crack_decay: 0.04, drive: 5.0, asym: 0.3,
    mid_f: 900.0, mid_decay: 0.035, tail_decay: 0.19, tail_f: 6200.0, tail_end_f: 900.0,
    mech_delay: 0.021, mech_level: 0.5, mech_partials: [2200.0, 3900.0, 6300.0], send: 0.44,
    ..profile("smg")
};

pub static PISTOL: WeaponProfile = WeaponProfile {
    level: 0.74, body_f: 186.0, body_f2: 84.0, body_decay: 0.05, sub_f: 92.0, sub_decay: 0.07,
    crack_f: 2750.0, crack_q: 1.15, crack_decay: 0.035, drive: 4.5, asym: 0.28,
    mid_f: 950.0, mid_decay: 0.03, tail_decay: 0.16, tail_f: 6800.0, tail_end_f: 1000.0,
    mech_delay: 0.038, mech_level: 0.46, mech_partials: [2450.0, 4200.0, 6900.0], send: 0.4,
    ..profile("pistol")
};

pub static SHOTGUN: WeaponProfile = WeaponProfile {
    level: 1.18, body_f: 108.0, body_f2: 40.0, body_decay: 0.13, sub_f: 44.0, sub_decay: 0.19,
    crack_f: 1450.0, crack_q: 0.7, crack_decay: 0.09, drive: 9.0, asym: 0.6,
    mid_f: 520.0, mid_decay: 0.08, tail_decay: 0.5, tail_f: 3600.0, tail_end_f: 460.0,
    mech_delay: 0.16, mech_level: 0.7, mech_partials: [980.0, 1760.0, 3050.0], send: 0.6,
    pellets: 6,
    ..profile("shotgun")
};

pub static SNIPER: WeaponProfile = WeaponProfile {
    level: 1.3, body_f: 96.0, body_f2: 34.0, body_decay: 0.16, sub_f: 38.0, sub_decay: 0.24,
    crack_f: 1320.0, crack_q: 0.8, crack_decay: 0.11, drive: 10.0, asym: 0.55,
    mid_f: 470.0, mid_decay: 0.1, tail_decay: 0.95, tail_f: 3300.0, tail_end_f: 380.0,
    mech_delay: 0.19, mech_level: 0.65, mech_partials: [1150.0, 2050.0, 3400.0], send: 0.72,
    ..profile("sniper")
};

pub static LMG: WeaponProfile = WeaponProfile {
    level: 1.14, body_f: 118.0, body_f2: 44.0, body_decay: 0.11, sub_f: 50.0, sub_decay: 0.16,
    crack_f: 1920.0, crack_q: 0.85, crack_decay: 0.075, drive: 8.0, asym: 0.45,
    mid_f: 610.0, mid_decay: 0.065, tail_decay: 0.5, tail_f: 4000.0, tail_end_f: 520.0,
    mech_delay: 0.03, mech_level: 0.6, mech_partials: [1330.0, 2480.0, 4100.0], send: 0.58,
    ..profile("lmg")
};

pub static SUPPRESSED: WeaponProfile = WeaponProfile {
    level: 0.5, body_f: 132.0, body_f2: 64.0, body_decay: 0.055, sub_f: 70.0, sub_decay: 0.07,
    crack_f: 900.0, crack_q: 0.6, crack_decay: 0.03, drive: 2.5, asym: 0.2,
    mid_f: 430.0, mid_decay: 0.05, tail_decay: 0.1, tail_f: 1800.0, tail_end_f: 400.0,
    mech_delay: 0.019, mech_level: 0.85, mech_partials: [2100.0, 3700.0, 5900.0], send: 0.18,
    suppressed: true,
    ..profile("suppressed")
};

/// `WEAPON_PROFILES`, in declaration order — which is the order `resolveProfile`
/// probes for an exact key match.
pub static WEAPON_PROFILES: [&WeaponProfile; 8] = [
    &RIFLE,
    &AK,
    &SMG,
    &PISTOL,
    &SHOTGUN,
    &SNIPER,
    &LMG,
    &SUPPRESSED,
];

/// Map whatever the weapons subsystem calls its guns onto a profile
/// (`weapons.js:90-102`).
///
/// The pattern order is the source's and it matters: `/ak|7\.?62|akm|scar/` is
/// a *substring* test, so it claims anything containing "ak" that did not
/// already match "suppress"/"silenc". That is a real property of the original —
/// "breaker" resolves to an AK — and it is kept.
pub fn resolve_profile(name: Option<&str>) -> &'static WeaponProfile {
    let Some(name) = name else {
        return &RIFLE;
    };
    if name.is_empty() {
        return &RIFLE;
    }
    let k = name.to_lowercase();
    if let Some(p) = WEAPON_PROFILES.iter().find(|p| p.name == k) {
        return p;
    }
    let any = |pats: &[&str]| pats.iter().any(|p| k.contains(p));
    if any(&["suppress", "silenc"]) {
        return &SUPPRESSED;
    }
    // `7\.?62` — an optional dot, which is two literals rather than a regex.
    if any(&["ak", "762", "7.62", "akm", "scar"]) {
        return &AK;
    }
    if any(&["mp5", "mp7", "smg", "ump", "vector", "uzi"]) {
        return &SMG;
    }
    if any(&["pistol", "glock", "m19", "deagle", "handgun", "sidearm"]) {
        return &PISTOL;
    }
    if any(&["shot", "pump", "12g", "benelli", "spas"]) {
        return &SHOTGUN;
    }
    if any(&[
        "snip",
        "dmr",
        "awp",
        "barrett",
        "338",
        "intervention",
        "marksman",
    ]) {
        return &SNIPER;
    }
    if any(&["lmg", "mg4", "m249", "pkm", "saw", "minigun"]) {
        return &LMG;
    }
    &RIFLE
}

/* ------------------------------------------------------------------ */
/* Round robin                                                        */
/* ------------------------------------------------------------------ */

const RR_SLOTS: usize = 6;

/// One round-robin timbre slot (`weapons.js:115-126`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Variant {
    pub body: f64,
    pub crack: f64,
    pub crack_q: f64,
    pub tail: f64,
    pub drive: f64,
    pub mid: f64,
    pub level: f64,
    pub mech: f64,
    /// Slight per-slot spectral tilt: microphone/room position variance.
    pub tilt: f64,
}

/// The lazily-built round-robin tables, one per profile.
///
/// **Divergence, and why.** The source caches the table by writing `_rr` and
/// `_rrIndex` *onto the profile object* — module-level mutable state hidden
/// inside what reads as a constant table. That is not expressible over a
/// `&'static WeaponProfile`, and it should not be: it means two independent
/// audio contexts (the live one and an offline self-test) silently share and
/// advance one another's round robin. The tables live here instead, owned by
/// whoever owns the audio graph, keyed by profile name. Same lazy build, same
/// six slots, same advance-then-read order, no global.
#[derive(Debug, Clone, Default)]
pub struct RoundRobinBank {
    tables: HashMap<&'static str, (Vec<Variant>, usize)>,
}

impl RoundRobinBank {
    pub fn new() -> Self {
        RoundRobinBank::default()
    }

    /// Build (once, lazily) the round-robin timbre table for a profile
    /// (`weapons.js:111-131`), then advance and return the selected slot —
    /// `roundRobin(...)` plus the `_rrIndex` advance that always follows it at
    /// `weapons.js:149-150`.
    fn advance(&mut self, profile: &'static WeaponProfile, rng: &mut Rng) -> Variant {
        let entry = self.tables.entry(profile.name).or_insert_with(|| {
            let rr: Vec<Variant> = (0..RR_SLOTS)
                .map(|_| Variant {
                    body: semis(rng.range(-1.1, 1.1)),
                    crack: semis(rng.range(-1.7, 1.7)),
                    crack_q: rng.range(0.85, 1.2),
                    tail: rng.range(0.86, 1.18),
                    drive: rng.range(0.85, 1.2),
                    mid: semis(rng.range(-2.0, 2.0)),
                    level: rng.range(0.93, 1.07),
                    mech: rng.range(0.8, 1.25),
                    tilt: rng.range(-2.5, 2.5),
                })
                .collect();
            let index = (rng.u32() % RR_SLOTS as u32) as usize;
            (rr, index)
        });
        entry.1 = (entry.1 + 1) % RR_SLOTS;
        entry.0[entry.1]
    }
}

/// What a voice hands back: its top gain node, when its tail has decayed, and
/// how much of it should reach the reverb send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Voice {
    pub node: NodeId,
    pub end: f64,
    pub send: f64,
}

/// `weaponShot`'s options bag (`weapons.js:140`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotOpts {
    /// `None` takes `actx.currentTime`.
    pub when: Option<f64>,
    pub distance: f64,
    pub first_person: bool,
    pub echo_boost: f64,
}

impl Default for ShotOpts {
    fn default() -> Self {
        ShotOpts {
            when: None,
            distance: 0.0,
            first_person: false,
            echo_boost: 1.0,
        }
    }
}

/// Synthesize one shot (`weapons.js:143-320`).
pub fn weapon_shot(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    rr_bank: &mut RoundRobinBank,
    profile: &'static WeaponProfile,
    o: ShotOpts,
) -> Voice {
    let t0 = o.when.unwrap_or_else(|| g.current_time());
    let dist = o.distance.max(0.0);
    let fp = o.first_person;

    let v = rr_bank.advance(profile, rng);

    // Per-shot jitter on top of the round-robin slot — the fine grain.
    let j_b = v.body * semis(rng.range(-0.45, 0.45));
    let j_c = v.crack * semis(rng.range(-0.8, 0.8));
    let j_t = v.tail * rng.range(0.94, 1.07);
    let j_l = v.level * rng.range(0.95, 1.05);

    // Distance mixing. Near = all crack and click; far = all boom and tail.
    let near = clamp(1.0 - dist / 42.0, 0.0, 1.0);
    let near_p = near.powf(0.7);
    let far = 1.0 - near;

    // VOICE TRIM — the gunshot is the loudest thing in the game and defines the
    // reference the rest of the mix is staged against.
    let out = gain(g, 0.46);
    let mut end = t0 + 0.2;

    /* ---- 1. transient --------------------------------------------- */
    if near_p > 0.05 {
        let tg = gain(g, 0.0);
        let rate = rng.range(0.9, 1.3);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let hp = biquad(g, FilterKind::Highpass, 2600.0, 0.6, 0.0);
        let pk = biquad(g, FilterKind::Peaking, 6200.0 * j_c, 1.1, 8.0 + v.tilt);
        g.series(&[src, hp, pk, tg, out]);
        let suppress = if profile.suppressed { 0.35 } else { 1.0 };
        hit(g, tg.gain(), t0, 0.9 * near_p * j_l * suppress, 0.0075);
        g.start_source(src, t0, 0.05);
        // A single-cycle sine at the top of the click adds the "snap" that pure
        // noise cannot produce.
        let clk = osc(g, Wave::Triangle, 1750.0 * j_c);
        let cg = gain(g, 0.0);
        g.connect(clk, cg);
        g.connect(cg, out);
        hit(g, cg.gain(), t0, 0.35 * near_p * j_l, 0.004);
        g.start(clk, t0);
        g.stop(clk, t0 + 0.02);
    }

    /* ---- 2. body + sub -------------------------------------------- */
    {
        let body_level = (0.85 + far * 0.5) * j_l * profile.level;
        let b1 = osc(g, Wave::Sine, profile.body_f * j_b);
        let b2 = osc(g, Wave::Triangle, profile.body_f * j_b * 0.5);
        let bg = gain(g, 0.0);
        let curve = saturation_curve(g, profile.drive * v.drive * 0.5, profile.asym);
        let drv = shaper(g, curve, "2x");
        let body_lp = biquad(g, FilterKind::Lowpass, lerp(2200.0, 700.0, far), 0.9, 0.0);
        g.connect(b1, bg);
        g.connect(b2, bg);
        g.series(&[bg, drv, body_lp, out]);
        sweep(
            g,
            b1.frequency(),
            t0,
            profile.body_f * j_b,
            profile.body_f2 * j_b,
            profile.body_decay * 1.4,
        );
        sweep(
            g,
            b2.frequency(),
            t0,
            profile.body_f * j_b * 0.5,
            profile.body_f2 * j_b * 0.55,
            profile.body_decay * 1.6,
        );
        let decay = profile.body_decay * rng.range(0.9, 1.15);
        ad(g, bg.gain(), t0, body_level, 0.0012, decay);
        g.start(b1, t0);
        g.start(b2, t0);
        let b_end = t0 + profile.body_decay * 1.8 + 0.02;
        g.stop(b1, b_end);
        g.stop(b2, b_end);
        end = end.max(b_end);

        // Sub thump — this is the one that moves air; keep it out of the reverb.
        let s = osc(g, Wave::Sine, profile.sub_f * j_b);
        let sg = gain(g, 0.0);
        g.connect(s, sg);
        g.connect(sg, out);
        sweep(
            g,
            s.frequency(),
            t0,
            profile.sub_f * j_b * 1.5,
            profile.sub_f * j_b * 0.8,
            profile.sub_decay,
        );
        ad(
            g,
            sg.gain(),
            t0,
            (0.5 + far * 0.55) * profile.level,
            0.004,
            profile.sub_decay * 1.3,
        );
        g.start(s, t0);
        g.stop(s, t0 + profile.sub_decay * 2.0 + 0.05);
        end = end.max(t0 + profile.sub_decay * 2.0 + 0.05);
    }

    /* ---- 3. crack -------------------------------------------------- */
    if near_p > 0.03 {
        let rate = rng.range(0.85, 1.25);
        let src = bank.one_shot(g, NoiseKind::White, rng, rate);
        let bp = biquad(
            g,
            FilterKind::Bandpass,
            profile.crack_f * j_c,
            profile.crack_q * v.crack_q,
            0.0,
        );
        let res = biquad(
            g,
            FilterKind::Peaking,
            profile.crack_f * j_c * 1.9,
            1.6,
            6.0 + v.tilt,
        );
        let curve = saturation_curve(g, profile.drive * v.drive, profile.asym * 0.6);
        let drv = shaper(g, curve, "2x");
        let cg = gain(g, 0.0);
        g.series(&[src, bp, res, drv, cg, out]);
        // The crack's own band sweeps down a little: the shock front decays.
        sweep(
            g,
            bp.frequency(),
            t0,
            profile.crack_f * j_c * 1.35,
            profile.crack_f * j_c * 0.8,
            profile.crack_decay * 2.0,
        );
        let decay = profile.crack_decay * rng.range(0.85, 1.2);
        ad(
            g,
            cg.gain(),
            t0,
            1.05 * near_p * j_l * profile.level,
            0.0015,
            decay,
        );
        g.start_source(src, t0, profile.crack_decay * 3.0 + 0.05);
        end = end.max(t0 + profile.crack_decay * 3.0);
    }

    /* ---- 4. mid body ---------------------------------------------- */
    {
        let rate = rng.range(0.8, 1.25);
        let src = bank.one_shot(g, NoiseKind::Pink, rng, rate);
        let bp = biquad(g, FilterKind::Bandpass, profile.mid_f * v.mid, 1.1, 0.0);
        let mg = gain(g, 0.0);
        g.series(&[src, bp, mg, out]);
        ad(
            g,
            mg.gain(),
            t0,
            (0.5 + far * 0.35) * j_l * profile.level,
            0.002,
            profile.mid_decay * 1.4,
        );
        g.start_source(src, t0, profile.mid_decay * 4.0 + 0.05);
    }

    /* ---- 5. tail --------------------------------------------------- */
    {
        let tail_dur = profile.tail_decay * j_t * (1.0 + far * 1.6);
        let rate = rng.range(0.7, 1.15);
        let src = bank.one_shot(g, NoiseKind::Pink, rng, rate);
        let lp = biquad(g, FilterKind::Lowpass, profile.tail_f, 0.6, 0.0);
        let hp = biquad(g, FilterKind::Highpass, lerp(160.0, 70.0, far), 0.7, 0.0);
        let tg = gain(g, 0.0);
        g.series(&[src, hp, lp, tg, out]);
        sweep(
            g,
            lp.frequency(),
            t0,
            profile.tail_f * lerp(1.0, 0.35, far),
            profile.tail_end_f * lerp(1.0, 0.6, far),
            tail_dur,
        );
        ad(
            g,
            tg.gain(),
            t0,
            (0.42 + far * 0.5) * j_l * profile.level,
            0.006,
            tail_dur,
        );
        g.start_source(src, t0, tail_dur * 1.3 + 0.05);
        end = end.max(t0 + tail_dur * 1.3);
    }

    /* ---- 6. mechanical / bolt ------------------------------------- */
    // Only audible close up — a rifle 40 m away has no audible action noise, and
    // spending nodes on it would be waste.
    if dist < 14.0 && profile.mech_level > 0.0 {
        let md = profile.mech_delay * rng.range(0.85, 1.2);
        let lvl = profile.mech_level
            * v.mech
            * if fp { 1.0 } else { 0.6 }
            * clamp(1.0 - dist / 14.0, 0.15, 1.0);
        let p = profile.mech_partials;
        let bolt_parts = [
            Partial::new(p[0] * rng.range(0.96, 1.05), 26.0, 0.5 * lvl, 0.055),
            Partial::new(p[1] * rng.range(0.96, 1.05), 20.0, 0.34 * lvl, 0.035),
            Partial::new(p[2] * rng.range(0.96, 1.05), 14.0, 0.2 * lvl, 0.02),
        ];
        let bolt = struck(g, bank, rng, t0 + md, &bolt_parts, 0.0035);
        g.connect(bolt, out);
        // Return-to-battery: a second, softer clack a few ms later.
        let back_parts = [
            Partial::new(p[0] * 0.88, 18.0, 0.3 * lvl, 0.04),
            Partial::new(p[1] * 1.12, 12.0, 0.16 * lvl, 0.022),
        ];
        let back = struck(g, bank, rng, t0 + md * 2.1, &back_parts, 0.003);
        g.connect(back, out);
        // Spring/gas hiss.
        let rate = rng.range(1.0, 1.4);
        let hs = bank.one_shot(g, NoiseKind::White, rng, rate);
        let hbp = biquad(
            g,
            FilterKind::Bandpass,
            4200.0 * rng.range(0.9, 1.1),
            1.4,
            0.0,
        );
        let hg = gain(g, 0.0);
        g.series(&[hs, hbp, hg, out]);
        ad(g, hg.gain(), t0 + md * 0.6, 0.12 * lvl, 0.006, 0.05);
        g.start_source(hs, t0 + md * 0.6, 0.12);
        end = end.max(t0 + md * 2.1 + 0.1);
    }

    /* ---- 7. distant rolling boom ---------------------------------- */
    if far > 0.12 {
        let boom_dur = 0.28 + dist * 0.0055;
        let rate = rng.range(0.6, 1.0);
        let src = bank.one_shot(g, NoiseKind::Brown, rng, rate);
        let lp = biquad(g, FilterKind::Lowpass, 420.0, 0.8, 0.0);
        let bg = gain(g, 0.0);
        g.series(&[src, lp, bg, out]);
        sweep(g, lp.frequency(), t0, 620.0, 190.0, boom_dur);
        ad(
            g,
            bg.gain(),
            t0,
            0.95 * far * far * profile.level,
            0.012 + dist * 0.0004,
            boom_dur,
        );
        g.start_source(src, t0, boom_dur * 1.4 + 0.05);
        end = end.max(t0 + boom_dur * 1.4);

        // Ground/terrain bounce: one discrete slap after the direct sound. This
        // is the detail that makes long-range fire read as *outdoors*.
        let bounce_t = t0 + clamp(dist * 0.0022, 0.012, 0.12);
        let rate2 = rng.range(0.6, 0.9);
        let b2 = bank.one_shot(g, NoiseKind::Pink, rng, rate2);
        let blp = biquad(g, FilterKind::Lowpass, 900.0, 0.7, 0.0);
        let b2g = gain(g, 0.0);
        g.series(&[b2, blp, b2g, out]);
        ad(
            g,
            b2g.gain(),
            bounce_t,
            0.3 * far,
            0.004,
            0.12 + dist * 0.001,
        );
        g.start_source(b2, bounce_t, 0.4);
    }

    /* ---- shotgun pellet spatter ----------------------------------- */
    if profile.pellets > 0 && near_p > 0.2 {
        for _ in 0..profile.pellets {
            let pt = t0 + rng.range(0.0004, 0.006);
            let rate = rng.range(0.9, 1.4);
            let src = bank.one_shot(g, NoiseKind::White, rng, rate);
            let bp = biquad(
                g,
                FilterKind::Bandpass,
                rng.range(2600.0, 6200.0),
                1.8,
                0.0,
            );
            let pg = gain(g, 0.0);
            g.series(&[src, bp, pg, out]);
            let decay = rng.range(0.004, 0.014);
            hit(g, pg.gain(), pt, 0.1 * near_p, decay);
            g.start_source(src, pt, 0.05);
        }
    }

    let send = profile.send * (1.0 + far * 1.4) * o.echo_boost;
    Voice {
        node: out,
        end: end + 0.05,
        send,
    }
}

/// Supersonic round passing near the listener (`weapons.js:326-349`). Tiny,
/// cheap, and enormously effective at making incoming fire feel dangerous.
pub fn bullet_whizz(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    when: Option<f64>,
    miss: f64,
    user_gain: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let miss = clamp(miss, 0.15, 6.0); // metres from the ear
    let level = clamp(1.1 - miss / 6.0, 0.1, 1.0) * user_gain;
    let out = gain(g, 3.2); // VOICE TRIM
    let rate = rng.range(0.9, 1.2);
    let src = bank.one_shot(g, NoiseKind::White, rng, rate);
    let bp = biquad(g, FilterKind::Bandpass, 2400.0, 3.2, 0.0);
    let wg = gain(g, 0.0);
    g.series(&[src, bp, wg, out]);
    // The N-wave's apparent pitch drops sharply as the round passes — Doppler on
    // a Mach 2.5 projectile is violent.
    let dur = 0.055 + miss * 0.012;
    let from = rng.range(3600.0, 5200.0);
    let to = rng.range(900.0, 1500.0);
    sweep(g, bp.frequency(), t0, from, to, dur);
    ad(g, wg.gain(), t0, 1.5 * level, 0.004, dur);
    g.start_source(src, t0, dur * 2.0);
    // Snap of the shock front.
    let s2 = bank.one_shot(g, NoiseKind::White, rng, 1.2);
    let hp = biquad(g, FilterKind::Highpass, 4000.0, 0.7, 0.0);
    let g2 = gain(g, 0.0);
    g.series(&[s2, hp, g2, out]);
    hit(g, g2.gain(), t0, 0.85 * level, 0.006);
    g.start_source(s2, t0, 0.03);
    Voice {
        node: out,
        end: t0 + dur * 2.0 + 0.05,
        send: 0.25,
    }
}

/// Dry-fire click when the magazine is empty (`weapons.js:352-362`).
pub fn dry_fire(g: &mut AudioGraph, bank: &NoiseBank, rng: &mut Rng, when: Option<f64>) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let out = gain(g, 1.0);
    let parts = [
        Partial::new(2600.0 * rng.range(0.95, 1.05), 24.0, 1.2, 0.035),
        Partial::new(4700.0, 16.0, 0.66, 0.02),
        Partial::new(860.0, 10.0, 0.5, 0.05),
    ];
    let r = struck(g, bank, rng, t0, &parts, 0.0025);
    g.connect(r, out);
    Voice {
        node: out,
        end: t0 + 0.14,
        send: 0.2,
    }
}
