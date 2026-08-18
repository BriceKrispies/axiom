//! Ambience — three continuous beds plus a positioned one-shot scheduler.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/ambience.js:1-370` — the whole
//! file.
//!
//! Everything is driven by audio-rate LFOs rather than per-frame automation, so
//! the beds cost nothing on the main thread, and every scheduled event's time,
//! position, pitch and level comes from the rng — the beds are literally never
//! in the same state twice, which is what kills the "looping wav" tell.
//!
//! The beds also react to the space probe: walking inside drops the wind and
//! closes a lowpass over the outdoor content, which is a huge part of why a
//! doorway feels like a doorway.

use crate::audio::dsp::{ad, biquad, clamp, gain, lerp, osc, struck, sweep, NoiseBank, NoiseKind, Partial};
use crate::audio::graph::{AudioGraph, FilterKind, NodeId, ParamRef, Wave};
use crate::audio::mixer::{Bus, Mixer};
use crate::audio::weapons::Voice;
use crate::rng::Rng;

/// What the bed scheduler asked the audio system to fire this frame.
///
/// **Divergence, and why.** The source passes `update(dt, api)` an object of
/// four callbacks that reach straight back into `AudioSystem`
/// (`index.js:243-251`) — a cycle Rust will not let a `&mut self` method build.
/// The timers are the interesting part and they are unchanged; the calls come
/// back as a list the caller dispatches immediately, in the same order, which is
/// observationally identical because the JavaScript calls are synchronous inside
/// `update` too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbienceCue {
    DistantVolley,
    DistantBoom,
    OneShot,
    DistantChatter,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Timers {
    gust: f64,
    volley: f64,
    boom: f64,
    oneshot: f64,
    chatter: f64,
}

#[derive(Debug, Clone, Copy)]
struct WindLayer {
    g: NodeId,
    lp: NodeId,
}

pub struct Ambience {
    nodes: Vec<NodeId>,
    pub started: bool,
    pub enclosure: f64,
    /// Scales the distant-battle scheduler.
    pub intensity: f64,
    timers: Timers,
    rng: Rng,

    outdoor_lp: Option<NodeId>,
    outdoor_gain: Option<NodeId>,
    wind_gain: Option<NodeId>,
    wind_layers: Vec<WindLayer>,
}

impl Ambience {
    pub fn new(rng: Rng) -> Self {
        Ambience {
            nodes: Vec::new(),
            started: false,
            enclosure: 0.0,
            intensity: 1.0,
            timers: Timers {
                gust: 2.0,
                volley: 4.0,
                boom: 18.0,
                oneshot: 6.0,
                chatter: 25.0,
            },
            rng,
            outdoor_lp: None,
            outdoor_gain: None,
            wind_gain: None,
            wind_layers: Vec::new(),
        }
    }

    /// Build the beds (`ambience.js:33-115`). Called once, after the graph is
    /// live.
    pub fn start(&mut self, g: &mut AudioGraph, bank: &NoiseBank, mixer: &Mixer) {
        if self.started {
            return;
        }
        self.started = true;

        let bus = mixer.bus(Bus::Ambience);
        let outdoor_lp = biquad(g, FilterKind::Lowpass, 20000.0, 0.6, 0.0);
        let outdoor_gain = gain(g, 1.0);
        g.series(&[outdoor_lp, outdoor_gain, bus]);
        self.outdoor_lp = Some(outdoor_lp);
        self.outdoor_gain = Some(outdoor_gain);
        self.nodes.push(outdoor_lp);
        self.nodes.push(outdoor_gain);

        // A little of the bed goes through the reverb so interiors get a wash of
        // outside noise rather than a dead room.
        let send_tap = gain(g, 0.22);
        g.connect(outdoor_gain, send_tap);
        g.connect(send_tap, mixer.reverb_send);
        self.nodes.push(send_tap);

        /* ---- wind: two decorrelated brown-noise layers ---------------- */
        let wind_gain = gain(g, 0.5);
        g.connect(wind_gain, outdoor_lp);
        self.nodes.push(wind_gain);
        self.wind_gain = Some(wind_gain);
        for i in 0..2 {
            let rate = self.rng.range(0.82, 1.15);
            let src = bank.source(g, NoiseKind::Brown, Some(&mut self.rng), rate, true);
            let lp = biquad(
                g,
                FilterKind::Lowpass,
                self.rng.range(260.0, 520.0),
                0.6,
                0.0,
            );
            let hp = biquad(g, FilterKind::Highpass, 40.0, 0.7, 0.0);
            let lg = gain(g, 0.5);
            let pan = g.create_stereo_panner(if i == 0 { -0.55 } else { 0.55 });
            g.series(&[src, hp, lp, lg, pan, wind_gain]);
            g.start_source_open(src, 0.0);

            // Two incommensurate LFOs per layer: the sum never repeats audibly.
            let d0 = self.rng.range(0.18, 0.3);
            self.lfo(g, 0.041 + f64::from(i) * 0.017, d0, lg.gain());
            let d1 = self.rng.range(0.08, 0.16);
            self.lfo(g, 0.0917 + f64::from(i) * 0.031, d1, lg.gain());
            let d2 = self.rng.range(80.0, 170.0);
            self.lfo(g, 0.037 + f64::from(i) * 0.023, d2, lp.frequency());
            self.nodes.extend_from_slice(&[src, lp, hp, lg, pan]);
            self.wind_layers.push(WindLayer { g: lg, lp });
        }

        /* ---- wind whistle through edges and wires --------------------- */
        {
            let src = bank.source(g, NoiseKind::White, Some(&mut self.rng), 1.0, true);
            let bp = biquad(g, FilterKind::Bandpass, 820.0, 7.0, 0.0);
            let wg = gain(g, 0.012);
            g.series(&[src, bp, wg, wind_gain]);
            g.start_source_open(src, 0.0);
            self.lfo(g, 0.053, 640.0, bp.frequency());
            self.lfo(g, 0.071, 0.011, wg.gain());
            self.nodes.extend_from_slice(&[src, bp, wg]);
        }

        /* ---- distant city: traffic hum, HVAC, indistinct life --------- */
        {
            let src = bank.source(g, NoiseKind::Pink, Some(&mut self.rng), 0.9, true);
            let lp = biquad(g, FilterKind::Lowpass, 480.0, 0.7, 0.0);
            let hp = biquad(g, FilterKind::Highpass, 70.0, 0.7, 0.0);
            let cg = gain(g, 0.06);
            g.series(&[src, hp, lp, cg, outdoor_lp]);
            g.start_source_open(src, 0.0);
            self.lfo(g, 0.023, 0.025, cg.gain());
            self.lfo(g, 0.0311, 120.0, lp.frequency());
            self.nodes.extend_from_slice(&[src, lp, hp, cg]);
        }

        /* ---- distant war rumble: sub-100 Hz, always there ------------- */
        {
            let src = bank.source(g, NoiseKind::Brown, Some(&mut self.rng), 0.7, true);
            let lp = biquad(g, FilterKind::Lowpass, 105.0, 0.9, 0.0);
            let wg = gain(g, 0.05);
            g.series(&[src, lp, wg, outdoor_lp]);
            g.start_source_open(src, 0.0);
            self.lfo(g, 0.0137, 0.035, wg.gain());
            self.nodes.extend_from_slice(&[src, lp, wg]);
        }

        self.reseed_timers();
    }

    /// Attach a slow oscillator to an `AudioParam` (`ambience.js:118-126`).
    fn lfo(&mut self, g: &mut AudioGraph, freq: f64, depth: f64, param: ParamRef) -> NodeId {
        let o = osc(g, Wave::Sine, freq);
        let lg = gain(g, depth);
        g.connect(o, lg);
        g.connect_param(lg, param);
        g.start(o, self.rng.range(0.0, 10.0)); // random phase so nothing lines up
        self.nodes.push(o);
        self.nodes.push(lg);
        o
    }

    fn reseed_timers(&mut self) {
        let r = &mut self.rng;
        self.timers.gust = r.range(4.0, 14.0);
        self.timers.volley = r.range(3.0, 11.0);
        self.timers.boom = r.range(14.0, 44.0);
        self.timers.oneshot = r.range(5.0, 17.0);
        self.timers.chatter = r.range(18.0, 50.0);
    }

    /// Outdoor content is filtered and dropped when the listener is enclosed
    /// (`ambience.js:138-145`).
    pub fn set_enclosure(&mut self, g: &mut AudioGraph, v: f64) {
        self.enclosure = clamp(v, 0.0, 1.0);
        if !self.started {
            return;
        }
        let t = g.current_time();
        if let Some(lp) = self.outdoor_lp {
            g.set_target_at_time(
                lp.frequency(),
                lerp(20000.0, 620.0, self.enclosure),
                t,
                0.6,
            );
        }
        if let Some(og) = self.outdoor_gain {
            g.set_target_at_time(og.gain(), lerp(1.0, 0.45, self.enclosure), t, 0.6);
        }
        if let Some(wg) = self.wind_gain {
            g.set_target_at_time(wg.gain(), lerp(0.5, 0.12, self.enclosure), t, 0.8);
        }
    }

    /// `ambience.js:147-181`. Returns the cues the scheduler fired, in order.
    pub fn update(&mut self, g: &mut AudioGraph, dt: f64) -> Vec<AmbienceCue> {
        let mut cues = Vec::new();
        if !self.started {
            return cues;
        }
        let intensity = clamp(self.intensity, 0.25, 2.0);

        self.timers.gust -= dt;
        if self.timers.gust <= 0.0 {
            self.timers.gust = self.rng.range(5.0, 16.0);
            self.gust(g);
        }

        self.timers.volley -= dt;
        if self.timers.volley <= 0.0 {
            self.timers.volley = self.rng.range(2.5, 12.0) / intensity;
            cues.push(AmbienceCue::DistantVolley);
        }

        self.timers.boom -= dt;
        if self.timers.boom <= 0.0 {
            self.timers.boom = self.rng.range(16.0, 50.0) / intensity;
            cues.push(AmbienceCue::DistantBoom);
        }

        self.timers.oneshot -= dt;
        if self.timers.oneshot <= 0.0 {
            self.timers.oneshot = self.rng.range(6.0, 20.0);
            cues.push(AmbienceCue::OneShot);
        }

        self.timers.chatter -= dt;
        if self.timers.chatter <= 0.0 {
            self.timers.chatter = self.rng.range(20.0, 60.0);
            cues.push(AmbienceCue::DistantChatter);
        }
        cues
    }

    /// A gust: level swell plus the lowpass opening as the air speeds up
    /// (`ambience.js:184-197`).
    fn gust(&mut self, g: &mut AudioGraph) {
        let t = g.current_time();
        let dur = self.rng.range(2.2, 6.5);
        let strength = self.rng.range(0.25, 1.0) * lerp(1.0, 0.25, self.enclosure);
        for l in self.wind_layers.clone() {
            let peak = 0.5 + 0.5 * strength * self.rng.range(0.7, 1.2);
            let at = t + self.rng.range(0.0, 0.5);
            g.set_target_at_time(l.g.gain(), peak, at, dur * 0.28);
            g.set_target_at_time(l.g.gain(), 0.5, t + dur * 0.55, dur * 0.4);
            let f = match g.node(l.lp) {
                crate::audio::graph::NodeKind::Biquad { frequency, .. } => *frequency,
                _ => 0.0,
            };
            g.set_target_at_time(l.lp.frequency(), f * (1.0 + strength * 0.9), t, dur * 0.3);
            g.set_target_at_time(l.lp.frequency(), f, t + dur * 0.6, dur * 0.5);
        }
    }

    pub fn dispose(&mut self, g: &mut AudioGraph) {
        for n in &self.nodes {
            g.disconnect_all(*n);
        }
        self.nodes.clear();
        self.wind_layers.clear();
        self.started = false;
    }
}

/* ------------------------------------------------------------------ */
/* Positioned ambient one-shots                                       */
/* ------------------------------------------------------------------ */

/// The weighted table the scheduler picks from (`ambience.js:214`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneShot {
    Dog,
    Siren,
    Creak,
    Settle,
    Birds,
    Vehicle,
    Heli,
    /// Also the `default:` arm.
    Shout,
}

impl OneShot {
    pub const ALL: [OneShot; 8] = [
        OneShot::Dog,
        OneShot::Siren,
        OneShot::Creak,
        OneShot::Settle,
        OneShot::Birds,
        OneShot::Vehicle,
        OneShot::Heli,
        OneShot::Shout,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            OneShot::Dog => "dog",
            OneShot::Siren => "siren",
            OneShot::Creak => "creak",
            OneShot::Settle => "settle",
            OneShot::Birds => "birds",
            OneShot::Vehicle => "vehicle",
            OneShot::Heli => "heli",
            OneShot::Shout => "shout",
        }
    }
}

/// `ambience.js:216-369`.
pub fn ambient_one_shot(
    g: &mut AudioGraph,
    bank: &NoiseBank,
    rng: &mut Rng,
    kind: OneShot,
    when: Option<f64>,
    level: f64,
) -> Voice {
    let t0 = when.unwrap_or_else(|| g.current_time());
    let out = gain(g, 0.55); // VOICE TRIM
    let lvl = level;

    match kind {
        OneShot::Dog => {
            // Two or three barks, each a short formant-ish yelp.
            let n = 2 + rng.u32() % 2;
            let mut end = t0 + 1.0;
            for i in 0..n {
                let bt = t0 + f64::from(i) * rng.range(0.24, 0.44);
                let o1 = osc(g, Wave::Sawtooth, rng.range(220.0, 340.0));
                let bp = biquad(g, FilterKind::Bandpass, rng.range(700.0, 1200.0), 2.2, 0.0);
                let dg = gain(g, 0.0);
                g.series(&[o1, bp, dg, out]);
                let from = rng.range(300.0, 420.0);
                let to = rng.range(150.0, 220.0);
                sweep(g, o1.frequency(), bt, from, to, 0.11);
                ad(g, dg.gain(), bt, 0.5 * lvl, 0.01, 0.1);
                g.start(o1, bt);
                g.stop(o1, bt + 0.3);
                let ns = bank.one_shot(g, NoiseKind::White, rng, 1.0);
                let nbp = biquad(g, FilterKind::Bandpass, 2400.0, 1.2, 0.0);
                let ng = gain(g, 0.0);
                g.series(&[ns, nbp, ng, out]);
                ad(g, ng.gain(), bt, 0.12 * lvl, 0.008, 0.08);
                g.start_source(ns, bt, 0.2);
                end = bt + 0.4;
            }
            Voice {
                node: out,
                end,
                send: 0.7,
            }
        }
        OneShot::Siren => {
            // Distant two-tone, wailing, drifting in and out.
            let dur = rng.range(4.0, 9.0);
            let o1 = osc(g, Wave::Sine, 620.0);
            let o2 = osc(g, Wave::Sine, 930.0);
            let sg = gain(g, 0.0);
            let lp = biquad(g, FilterKind::Lowpass, 1800.0, 0.8, 0.0);
            g.connect(o1, sg);
            g.connect(o2, sg);
            g.series(&[sg, lp, out]);
            let wob = osc(g, Wave::Sine, rng.range(0.35, 0.6));
            let wg = gain(g, 110.0);
            g.connect(wob, wg);
            g.connect_param(wg, o1.frequency());
            g.connect_param(wg, o2.frequency());
            g.start(wob, t0);
            ad(g, sg.gain(), t0, 0.022 * lvl, dur * 0.3, dur * 0.7);
            g.start(o1, t0);
            g.start(o2, t0);
            g.stop(o1, t0 + dur + 0.5);
            g.stop(o2, t0 + dur + 0.5);
            g.stop(wob, t0 + dur + 0.5);
            Voice {
                node: out,
                end: t0 + dur + 0.6,
                send: 1.1,
            }
        }
        OneShot::Creak => {
            // Metal fatigue: a high-Q band swept slowly, plus a final pop.
            let dur = rng.range(0.9, 2.4);
            let rate = rng.range(0.6, 1.0);
            let src = bank.one_shot(g, NoiseKind::White, rng, rate);
            let bp = biquad(g, FilterKind::Bandpass, 900.0, 22.0, 0.0);
            let cg = gain(g, 0.0);
            g.series(&[src, bp, cg, out]);
            let from = rng.range(500.0, 900.0);
            let to = rng.range(1100.0, 2200.0);
            sweep(g, bp.frequency(), t0, from, to, dur);
            ad(g, cg.gain(), t0, 0.3 * lvl, dur * 0.3, dur * 0.8);
            g.start_source(src, t0, dur * 1.5);
            let part = Partial::new(rng.range(400.0, 1400.0), 20.0, 0.18 * lvl, 0.1);
            let r = struck(g, bank, rng, t0 + dur * 0.9, &[part], 0.003);
            g.connect(r, out);
            Voice {
                node: out,
                end: t0 + dur * 1.6,
                send: 0.8,
            }
        }
        OneShot::Settle => {
            // Rubble shifting: a handful of grains and a soft low thump.
            for _ in 0..7 {
                let at = t0 + rng.range(0.0, 0.7);
                let part = Partial::new(
                    rng.range(600.0, 5000.0),
                    rng.range(8.0, 26.0),
                    rng.range(0.02, 0.09) * lvl,
                    rng.range(0.01, 0.07),
                );
                let r = struck(g, bank, rng, at, &[part], 0.002);
                g.connect(r, out);
            }
            let b = osc(g, Wave::Sine, 90.0);
            let bg = gain(g, 0.0);
            g.connect(b, bg);
            g.connect(bg, out);
            sweep(g, b.frequency(), t0, 110.0, 55.0, 0.15);
            ad(g, bg.gain(), t0, 0.14 * lvl, 0.01, 0.16);
            g.start(b, t0);
            g.stop(b, t0 + 0.4);
            Voice {
                node: out,
                end: t0 + 1.1,
                send: 0.6,
            }
        }
        OneShot::Birds => {
            let n = 3 + rng.u32() % 5;
            for _ in 0..n {
                let bt = t0 + rng.range(0.0, 1.4);
                let o1 = osc(g, Wave::Sine, 3200.0);
                let bg = gain(g, 0.0);
                g.connect(o1, bg);
                g.connect(bg, out);
                let up = rng.float() < 0.5;
                sweep(
                    g,
                    o1.frequency(),
                    bt,
                    if up { 2600.0 } else { 4400.0 },
                    if up { 4600.0 } else { 2700.0 },
                    0.06,
                );
                ad(g, bg.gain(), bt, 0.05 * lvl, 0.008, 0.06);
                g.start(o1, bt);
                g.stop(o1, bt + 0.2);
            }
            Voice {
                node: out,
                end: t0 + 1.8,
                send: 0.9,
            }
        }
        OneShot::Vehicle => {
            // A truck passing somewhere out of sight.
            let dur = rng.range(3.5, 7.0);
            let rate = rng.range(0.7, 1.0);
            let src = bank.one_shot(g, NoiseKind::Brown, rng, rate);
            let lp = biquad(g, FilterKind::Lowpass, 300.0, 0.9, 0.0);
            let vg = gain(g, 0.0);
            g.series(&[src, lp, vg, out]);
            sweep(g, lp.frequency(), t0, 200.0, 460.0, dur * 0.5);
            sweep(g, lp.frequency(), t0 + dur * 0.5, 460.0, 180.0, dur * 0.5);
            ad(g, vg.gain(), t0, 0.16 * lvl, dur * 0.45, dur * 0.55);
            g.start_source(src, t0, dur * 1.2);
            // Engine order: a low buzz that follows the same envelope.
            let e = osc(g, Wave::Sawtooth, rng.range(52.0, 78.0));
            let eg = gain(g, 0.0);
            let elp = biquad(g, FilterKind::Lowpass, 240.0, 1.2, 0.0);
            g.connect(e, eg);
            g.series(&[eg, elp, out]);
            ad(g, eg.gain(), t0, 0.035 * lvl, dur * 0.45, dur * 0.55);
            g.start(e, t0);
            g.stop(e, t0 + dur * 1.2);
            Voice {
                node: out,
                end: t0 + dur * 1.3,
                send: 0.7,
            }
        }
        OneShot::Heli => {
            // Rotor thump: an amplitude-modulated dark noise bed, no sample
            // needed.
            let dur = rng.range(6.0, 12.0);
            let rate = rng.range(0.8, 1.1);
            let src = bank.one_shot(g, NoiseKind::Brown, rng, rate);
            let lp = biquad(g, FilterKind::Lowpass, 420.0, 0.9, 0.0);
            let hg = gain(g, 0.0);
            // Blade-pass modulation: a separate multiplier, because an LFO
            // connected to a gain param sums with the envelope instead of
            // scaling it.
            let am = gain(g, 0.45);
            g.series(&[src, lp, hg, am, out]);
            ad(g, hg.gain(), t0, 2.1 * lvl, dur * 0.4, dur * 0.6);
            g.start_source(src, t0, dur * 1.2);
            let thump = osc(g, Wave::Sine, rng.range(4.6, 6.4));
            let tg = gain(g, 0.5);
            g.connect(thump, tg);
            g.connect_param(tg, am.gain());
            g.start(thump, t0);
            g.stop(thump, t0 + dur * 1.2);
            // Turbine whine an octave-ish above the blade rate harmonics.
            let w = osc(g, Wave::Sawtooth, rng.range(280.0, 420.0));
            let wbp = biquad(g, FilterKind::Bandpass, 1400.0, 6.0, 0.0);
            let wg = gain(g, 0.0);
            g.series(&[w, wbp, wg, out]);
            ad(g, wg.gain(), t0, 0.11 * lvl, dur * 0.4, dur * 0.6);
            g.start(w, t0);
            g.stop(w, t0 + dur * 1.2);
            Voice {
                node: out,
                end: t0 + dur * 1.3,
                send: 0.9,
            }
        }
        OneShot::Shout => {
            // Unintelligible distant shouting — deliberately just contour, no
            // words.
            let dur = rng.range(0.3, 0.7);
            let o1 = osc(g, Wave::Sawtooth, rng.range(110.0, 160.0));
            let bp1 = biquad(g, FilterKind::Bandpass, rng.range(600.0, 900.0), 4.0, 0.0);
            let bp2 = biquad(g, FilterKind::Bandpass, rng.range(1300.0, 2000.0), 5.0, 0.0);
            let sg = gain(g, 0.0);
            g.connect(o1, bp1);
            g.connect(o1, bp2);
            g.connect(bp1, sg);
            g.connect(bp2, sg);
            let lp = biquad(g, FilterKind::Lowpass, 2600.0, 0.8, 0.0);
            g.series(&[sg, lp, out]);
            let from = rng.range(130.0, 170.0);
            let to = rng.range(95.0, 125.0);
            sweep(g, o1.frequency(), t0, from, to, dur);
            ad(g, sg.gain(), t0, 0.2 * lvl, 0.05, dur);
            g.start(o1, t0);
            g.stop(o1, t0 + dur + 0.2);
            Voice {
                node: out,
                end: t0 + dur + 0.3,
                send: 1.2,
            }
        }
    }
}
