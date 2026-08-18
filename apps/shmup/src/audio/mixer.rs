//! The mixer — buses, ducking, reverb sends, concussion deafening.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/mixer.js:1-348` — the whole
//! file.
//!
//! The signal path, top down:
//!
//! ```text
//!   voices ──► bus (weapons | foley | ambience | voice | ui)
//!                │           each bus: trim ──► bus compressor
//!                ▼
//!   ┌── worldSum ─► muffleLP ─► muffleGain ──┐        (deafening / concussion)
//!   │      ▲                                 │
//!   │   reverb returns                       ▼
//!   │      ▲                              masterSum ─► masterComp ─► softClip ─► out
//!   │   convolver x5 (space blend)           ▲
//!   │      ▲                                 │
//!   └── reverbSend ◄─ per-voice send    ui bus + tinnitus (bypass the muffle,
//!                                        so HUD and the ring stay audible)
//! ```
//!
//! Ducking is a manual sidechain: gunfire pushes `ambience`/`foley` down with a
//! fast `setTargetAtTime` and lets them float back over ~400 ms. That is exactly
//! what a real game mix does, and it is far more predictable than trying to make
//! a `DynamicsCompressor` listen to another bus.

use crate::audio::dsp::{biquad, clamp, gain, limiter_curve, osc, shaper};
use crate::audio::graph::{AudioGraph, FilterKind, NodeId, Sink, Wave};
use crate::audio::ir::{Space, SpaceWeights, IR_SPECS};
use crate::rng::Rng;

/// The five buses (`mixer.js:27-33`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bus {
    Weapons,
    Foley,
    Ambience,
    Voice,
    Ui,
}

impl Bus {
    pub const ALL: [Bus; 5] = [Bus::Weapons, Bus::Foley, Bus::Ambience, Bus::Voice, Bus::Ui];

    pub fn as_str(self) -> &'static str {
        match self {
            Bus::Weapons => "weapons",
            Bus::Foley => "foley",
            Bus::Ambience => "ambience",
            Bus::Voice => "voice",
            Bus::Ui => "ui",
        }
    }

    /// `BUS_DEFS[name].trim`.
    fn trim(self) -> f64 {
        match self {
            Bus::Weapons => 0.95,
            Bus::Foley => 0.9,
            Bus::Ambience => 0.5,
            Bus::Voice => 0.85,
            Bus::Ui => 1.6,
        }
    }

    /// `BUS_DEFS[name].comp` — `(threshold, knee, ratio, attack, release)`, or
    /// `None` for the `ui` bus, which is deliberately uncompressed.
    fn comp(self) -> Option<(f64, f64, f64, f64, f64)> {
        match self {
            Bus::Weapons => Some((-7.0, 8.0, 2.6, 0.003, 0.16)),
            Bus::Foley => Some((-14.0, 10.0, 2.0, 0.004, 0.2)),
            Bus::Ambience => Some((-24.0, 12.0, 2.0, 0.05, 0.5)),
            Bus::Voice => Some((-18.0, 8.0, 3.0, 0.006, 0.22)),
            Bus::Ui => None,
        }
    }

    /// The source's non-throwing `this.buses[name] ?? this.buses.foley`.
    pub fn from_str(name: &str) -> Bus {
        Bus::ALL
            .into_iter()
            .find(|b| b.as_str() == name)
            .unwrap_or(Bus::Foley)
    }
}

#[derive(Debug, Clone, Copy)]
struct BusStrip {
    input: NodeId,
    duck: NodeId,
    trim: NodeId,
    comp: Option<NodeId>,
    base_trim: f64,
    duck_amount: f64,
    duck_hold: f64,
}

#[derive(Debug, Clone, Copy)]
struct SpaceSlot {
    conv: NodeId,
    gain: NodeId,
    live: bool,
}

#[derive(Debug, Clone)]
struct Tinnitus {
    g: NodeId,
    nodes: [NodeId; 8],
    /// Which of `nodes` are sources that must be `stop()`ed — `n.stop?.(t)` in
    /// the source, which is a runtime "does this node have a stop method" test.
    /// The port knows statically: the four oscillators.
    sources: usize,
    until: f64,
}

pub struct Mixer {
    pub master_volume: f64,

    /* master chain */
    pub master_sum: NodeId,
    pub pre_gain: NodeId,
    pub master_comp: NodeId,
    pub soft_clip: NodeId,
    pub master_gain: NodeId,

    /* concussion / deafening path */
    pub world_sum: NodeId,
    pub muffle_lp: NodeId,
    pub muffle_hs: NodeId,
    pub muffle_gain: NodeId,

    buses: [BusStrip; 5],

    /* reverb */
    pub reverb_send: NodeId,
    pub send_hp: NodeId,
    pub send_lp: NodeId,
    pub reverb_return: NodeId,
    spaces: [Option<SpaceSlot>; 5],
    ir_ready: bool,
    pub space_weights: SpaceWeights,

    /// 0..1, public: UI/render may read this.
    pub deafness: f64,
    tin: Option<Tinnitus>,

    /// The stream `buildReverbs` forks each IR's generator from
    /// (`mixer.js:147`).
    rng: Rng,
}

impl Mixer {
    /// `new Mixer(actx, rng, opts)` (`mixer.js:40-133`).
    ///
    /// `master_volume` defaults to 0.95, the source's `opts.masterVolume ?? 0.95`.
    pub fn new(g: &mut AudioGraph, rng: Rng, master_volume: f64) -> Self {
        /* ---- master chain (built first, everything hangs off it) ------ */
        let master_sum = gain(g, 1.0);
        // Headroom stage. It sits BEFORE the compressor/clipper on purpose: a
        // post-limiter volume control only scales an already-squashed signal,
        // and that is what destroys the difference between a footstep and a
        // gunshot.
        let pre_gain = gain(g, 0.22);
        // Safety net only: a single gunshot should barely touch it, a firefight
        // plus a grenade should be held together by it.
        let master_comp = g.create_dynamics_compressor(-2.0, 3.0, 4.0, 0.0035, 0.14);
        let curve = limiter_curve(g);
        let soft_clip = shaper(g, curve, "4x");
        let master_gain = gain(g, master_volume);

        g.connect(master_sum, pre_gain);
        g.connect(pre_gain, master_comp);
        g.connect(master_comp, soft_clip);
        g.connect(soft_clip, master_gain);
        g.connect(master_gain, AudioGraph::DESTINATION);

        /* ---- concussion / deafening path ----------------------------- */
        // Everything diegetic runs through this so a nearby explosion can muffle
        // the whole world without touching the HUD or the tinnitus tone.
        let world_sum = gain(g, 1.0);
        let muffle_lp = biquad(g, FilterKind::Lowpass, 20000.0, 0.5, 0.0);
        let muffle_hs = biquad(g, FilterKind::Highshelf, 3500.0, 0.7, 0.0);
        let muffle_gain = gain(g, 1.0);
        g.connect(world_sum, muffle_lp);
        g.connect(muffle_lp, muffle_hs);
        g.connect(muffle_hs, muffle_gain);
        g.connect(muffle_gain, master_sum);

        /* ---- buses ---------------------------------------------------- */
        let buses = Bus::ALL.map(|name| {
            let input = gain(g, 1.0); // voices connect here
            let duck = gain(g, 1.0); // sidechain victim
            let trim = gain(g, name.trim()); // static balance
            g.connect(input, duck);
            g.connect(duck, trim);
            let comp = name.comp().map(|(threshold, knee, ratio, attack, release)| {
                let c = g.create_dynamics_compressor(threshold, knee, ratio, attack, release);
                g.connect(trim, c);
                c
            });
            let tail = comp.unwrap_or(trim);
            // ui bypasses the muffle: menu clicks must survive a grenade.
            g.connect(tail, if name == Bus::Ui { master_sum } else { world_sum });
            BusStrip {
                input,
                duck,
                trim,
                comp,
                base_trim: name.trim(),
                duck_amount: 0.0,
                duck_hold: 0.0,
            }
        });

        /* ---- reverb ---------------------------------------------------- */
        // One send bus fanning into five convolvers; the space probe crossfades
        // between them. Sharing the send keeps the cost fixed no matter how many
        // voices are alive.
        let reverb_send = gain(g, 1.0);
        // Pre-send shaping: no sub into the convolvers (muddy) and no fizz above
        // 9 kHz (real rooms do not reflect that back at you from 30 m away).
        let send_hp = biquad(g, FilterKind::Highpass, 170.0, 0.7, 0.0);
        let send_lp = biquad(g, FilterKind::Lowpass, 9000.0, 0.7, 0.0);
        g.connect(reverb_send, send_hp);
        g.connect(send_hp, send_lp);

        let reverb_return = gain(g, 0.9);
        g.connect(reverb_return, world_sum);

        Mixer {
            // Headroom matters more than loudness: at 0.62 a single gunshot
            // peaks near -3 dBFS and the limiter is only reached by dense
            // combat, so the mix keeps its dynamic range instead of being
            // flattened.
            master_volume,
            master_sum,
            pre_gain,
            master_comp,
            soft_clip,
            master_gain,
            world_sum,
            muffle_lp,
            muffle_hs,
            muffle_gain,
            buses,
            reverb_send,
            send_hp,
            send_lp,
            reverb_return,
            spaces: [None; 5],
            ir_ready: false,
            space_weights: SpaceWeights {
                street: 0.35,
                open: 0.65,
                ..SpaceWeights::default()
            },
            deafness: 0.0,
            tin: None,
            rng,
        }
    }

    fn strip(&mut self, bus: Bus) -> &mut BusStrip {
        let i = Bus::ALL.iter().position(|&b| b == bus).unwrap_or(1);
        &mut self.buses[i]
    }

    /// Render the impulse responses (`mixer.js:140-160`).
    ///
    /// Split out from the constructor because it is the single most expensive
    /// thing this subsystem does and we only want to pay it once audio is
    /// actually going to be heard.
    ///
    /// The source wraps `generateIR` in a `try`/`catch` that logs and skips the
    /// space. Nothing in [`generate_ir`](crate::audio::ir::generate_ir) can
    /// fail — it is arithmetic over a `Vec` — so the catch has no counterpart
    /// and the `Option` in `spaces` only ever means "not built yet".
    pub fn build_reverbs(&mut self, g: &mut AudioGraph) {
        if self.ir_ready {
            return;
        }
        for (slot, name) in self.spaces.iter_mut().zip(Space::ALL) {
            let conv = g.create_convolver(false);
            let mut sub = self.rng.fork();
            let idx = Space::ALL.iter().position(|&s| s == name).unwrap_or(0);
            let buffer = crate::audio::ir::generate_ir_buffer(g, &mut sub, &IR_SPECS[idx]);
            g.set_convolver_buffer(conv, Some(buffer));
            let w = self.space_weights.get(name);
            let sg = gain(g, w);
            let live = w > 0.012;
            if live {
                g.connect(self.send_lp, conv);
            }
            g.connect(conv, sg);
            g.connect(sg, self.reverb_return);
            *slot = Some(SpaceSlot {
                conv,
                gain: sg,
                live,
            });
        }
        self.ir_ready = true;
    }

    pub fn reverbs_built(&self) -> bool {
        self.ir_ready
    }

    /// Blend weights from the space probe, ramped so doorways do not click
    /// (`mixer.js:170-188`).
    ///
    /// Convolvers whose weight has fallen to nothing are unplugged from the
    /// send: a 2.8 s stereo convolution is the most expensive node in the graph
    /// and there is no reason to compute three of them into a zero gain.
    /// Typically two of the five are live at any moment.
    pub fn set_space(&mut self, g: &mut AudioGraph, weights: &SpaceWeights, smooth: f64) {
        let t = g.current_time();
        for (i, name) in Space::ALL.into_iter().enumerate() {
            let w = clamp(weights.get(name), 0.0, 1.0);
            self.space_weights.set(name, w);
            let Some(s) = self.spaces[i].as_mut() else {
                continue;
            };
            g.set_target_at_time(s.gain.gain(), w, t, smooth);
            let want = w > 0.012;
            if want == s.live {
                continue;
            }
            if want {
                g.connect(self.send_lp, s.conv);
            } else {
                // Its gain is already ~0, so cutting the input is inaudible.
                g.disconnect(self.send_lp, Sink::Node(s.conv));
            }
            s.live = want;
        }
    }

    /// Bus input node a voice should connect to (`mixer.js:191-193`).
    pub fn bus(&self, name: Bus) -> NodeId {
        let i = Bus::ALL.iter().position(|&b| b == name).unwrap_or(1);
        self.buses[i].input
    }

    /// Sidechain duck (`mixer.js:199-214`). `amount` 0..1 of gain reduction,
    /// `hold` seconds before it starts floating back. Called on every gunshot
    /// and explosion.
    pub fn duck(&mut self, g: &mut AudioGraph, amount: f64, hold: f64) {
        let t = g.current_time();
        for (name, scale) in [(Bus::Ambience, 1.0), (Bus::Foley, 0.55), (Bus::Voice, 0.4)] {
            let b = self.strip(name);
            let a = clamp(amount * scale, 0.0, 0.92);
            if a <= b.duck_amount + 0.01 {
                continue; // an existing deeper duck wins
            }
            b.duck_amount = a;
            b.duck_hold = hold;
            let duck = b.duck;
            g.cancel_scheduled_values(duck.gain(), t);
            g.set_target_at_time(duck.gain(), 1.0 - a, t, 0.012);
        }
    }

    /// Temporary hearing damage (`mixer.js:220-231`). `level` 0..1. Muffles the
    /// world, dips its level and starts a tinnitus tone that outlives the
    /// muffling.
    pub fn concuss(&mut self, g: &mut AudioGraph, level: f64) {
        let level = clamp(level, 0.0, 1.0);
        if level <= self.deafness {
            return;
        }
        self.deafness = level;
        let t = g.current_time();
        let cutoff = 20000.0 * 0.024f64.powf(level); // 1.0 -> ~480 Hz
        g.cancel_scheduled_values(self.muffle_lp.frequency(), t);
        g.set_target_at_time(
            self.muffle_lp.frequency(),
            clamp(cutoff, 320.0, 20000.0),
            t,
            0.02,
        );
        g.set_target_at_time(self.muffle_hs.gain(), -22.0 * level, t, 0.02);
        g.set_target_at_time(self.muffle_gain.gain(), 1.0 - 0.55 * level, t, 0.02);
        self.start_tinnitus(g, level);
    }

    fn start_tinnitus(&mut self, g: &mut AudioGraph, level: f64) {
        let t = g.current_time();
        if let Some(tin) = self.tin.as_mut() {
            // Re-trigger: just push the envelope back up.
            let tg = tin.g;
            tin.until = t + 4.0 + 7.0 * level;
            g.cancel_scheduled_values(tg.gain(), t);
            g.set_target_at_time(tg.gain(), 0.05 * level, t, 0.03);
            return;
        }
        let tg = gain(g, 0.0);
        // Two close tones plus a third an octave up: a single sine reads as a
        // test tone, three beating partials read as ringing ears.
        let o1 = osc(g, Wave::Sine, 3980.0);
        let o2 = osc(g, Wave::Sine, 4130.0);
        let o3 = osc(g, Wave::Sine, 7420.0);
        let g1 = gain(g, 0.6);
        let g2 = gain(g, 0.45);
        let g3 = gain(g, 0.18);
        g.connect(o1, g1);
        g.connect(o2, g2);
        g.connect(o3, g3);
        g.connect(g1, tg);
        g.connect(g2, tg);
        g.connect(g3, tg);
        // Slow wobble so it never sounds like a stuck oscillator.
        let lfo = osc(g, Wave::Sine, 0.23);
        let lfo_g = gain(g, 26.0);
        g.connect(lfo, lfo_g);
        g.connect_param(lfo_g, o2.frequency());
        g.connect(tg, self.master_sum); // post-muffle on purpose
        g.start(o1, t);
        g.start(o2, t);
        g.start(o3, t);
        g.start(lfo, t);
        g.set_target_at_time(tg.gain(), 0.05 * level, t, 0.03);
        self.tin = Some(Tinnitus {
            g: tg,
            nodes: [o1, o2, o3, lfo, g1, g2, g3, lfo_g],
            sources: 4,
            until: t + 4.0 + 7.0 * level,
        });
    }

    /// Per-frame housekeeping: duck recovery, deafening recovery, tinnitus
    /// teardown (`mixer.js:264-303`).
    pub fn update(&mut self, g: &mut AudioGraph, dt: f64) {
        let t = g.current_time();

        for i in 0..self.buses.len() {
            let b = &mut self.buses[i];
            if b.duck_amount <= 0.0 {
                continue;
            }
            if b.duck_hold > 0.0 {
                b.duck_hold -= dt;
                continue;
            }
            b.duck_amount = (b.duck_amount - dt * 2.6).max(0.0);
            let duck = b.duck;
            let amount = b.duck_amount;
            g.set_target_at_time(duck.gain(), 1.0 - amount, t, 0.09);
            if amount < 0.01 {
                self.buses[i].duck_amount = 0.0;
                g.set_target_at_time(duck.gain(), 1.0, t, 0.09);
            }
        }

        if self.deafness > 0.0 {
            // Recovery is slow at first then quick — matches how temporary
            // threshold shift actually behaves, and it feels dramatic.
            self.deafness = (self.deafness - dt * (0.1 + self.deafness * 0.22)).max(0.0);
            let cutoff = 20000.0 * 0.024f64.powf(self.deafness);
            g.set_target_at_time(
                self.muffle_lp.frequency(),
                clamp(cutoff, 320.0, 20000.0),
                t,
                0.25,
            );
            g.set_target_at_time(self.muffle_hs.gain(), -22.0 * self.deafness, t, 0.25);
            g.set_target_at_time(
                self.muffle_gain.gain(),
                1.0 - 0.55 * self.deafness,
                t,
                0.25,
            );
        }

        if let Some(tin) = self.tin.clone() {
            if t > tin.until - 3.5 {
                g.set_target_at_time(tin.g.gain(), 0.0, t, 1.1);
            }
            if t > tin.until {
                for (i, n) in tin.nodes.iter().enumerate() {
                    if i < tin.sources {
                        g.stop(*n, t);
                    }
                    g.disconnect_all(*n);
                }
                g.disconnect_all(tin.g);
                self.tin = None;
            }
        }
    }

    pub fn set_master_volume(&mut self, g: &mut AudioGraph, v: f64) {
        self.master_volume = clamp(v, 0.0, 1.0);
        let t = g.current_time();
        g.set_target_at_time(self.master_gain.gain(), self.master_volume, t, 0.03);
    }

    pub fn set_bus_volume(&mut self, g: &mut AudioGraph, name: Bus, v: f64) {
        let b = *self.strip(name);
        let t = g.current_time();
        g.set_target_at_time(b.trim.gain(), clamp(v, 0.0, 2.0) * b.base_trim, t, 0.05);
    }

    /// Rough gain-reduction readout, handy for the debug overlay
    /// (`mixer.js:317-319`).
    ///
    /// `DynamicsCompressorNode.reduction` is a live audio-thread measurement,
    /// so a *recorded* graph has nothing to report and this is zero. The wasm
    /// binding reads the real node and overrides it — the one number in this
    /// file that only exists once the graph is really running.
    pub fn reduction(&self) -> f64 {
        0.0
    }

    /// Tear the whole graph down (`mixer.js:321-346`).
    pub fn dispose(&mut self, g: &mut AudioGraph) {
        if let Some(tin) = self.tin.take() {
            for (i, n) in tin.nodes.iter().enumerate() {
                if i < tin.sources {
                    g.stop(*n, g.current_time());
                }
                g.disconnect_all(*n);
            }
            g.disconnect_all(tin.g);
        }
        for slot in self.spaces.iter_mut() {
            if let Some(s) = slot.take() {
                g.disconnect_all(s.conv);
                g.set_convolver_buffer(s.conv, None);
                g.disconnect_all(s.gain);
            }
        }
        for b in self.buses {
            g.disconnect_all(b.input);
            g.disconnect_all(b.duck);
            g.disconnect_all(b.trim);
            if let Some(c) = b.comp {
                g.disconnect_all(c);
            }
        }
        g.disconnect_all(self.reverb_send);
        g.disconnect_all(self.send_hp);
        g.disconnect_all(self.send_lp);
        g.disconnect_all(self.reverb_return);
        g.disconnect_all(self.world_sum);
        g.disconnect_all(self.muffle_lp);
        g.disconnect_all(self.muffle_hs);
        g.disconnect_all(self.muffle_gain);
        g.disconnect_all(self.master_sum);
        g.disconnect_all(self.pre_gain);
        g.disconnect_all(self.master_comp);
        g.disconnect_all(self.soft_clip);
        g.disconnect_all(self.master_gain);
    }
}
