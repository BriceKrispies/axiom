//! Spatialisation — a pool of reusable 3D emitter chains.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/spatial.js:1-348` — the whole
//! file.
//!
//! Each chain is:
//!
//! ```text
//!   input ─► occlusionLP ─► airLP ─► distanceGain ─┬─► panner (HRTF) ─► bus
//!                                                  └─► sendGain ─► reverb send
//! ```
//!
//! The design decisions, because they are not the obvious ones, and all four are
//! reproduced exactly:
//!
//!  - The panner's own distance model is switched OFF (`rolloffFactor` 0) and
//!    attenuation is applied by `distGain` instead. That is what lets the reverb
//!    send be *post* distance attenuation but *pre* panning, which is how a far
//!    source correctly ends up wetter than a near one.
//!  - `airLP` is air absorption, `occlusionLP` is geometry. They are separate so
//!    a distant *and* occluded source stacks both losses, as it should.
//!  - The whole chain is built once and only the panner→bus edge is connected
//!    while the emitter is in use; a free emitter is detached from the graph so
//!    the (expensive) HRTF convolution is not evaluated for silence.
//!  - Propagation delay is not a delay node: every voice is *scheduled* at
//!    `now + dist/343`, which is sample-accurate and free.

use crate::audio::dsp::{air_cutoff, biquad, clamp, gain};
use crate::audio::foley::Surface;
use crate::audio::graph::{AudioGraph, FilterKind, NodeId, Sink};
use crate::audio::mixer::{Bus, Mixer};

const MAX_EMITTERS: usize = 40;

/// Reference distance for the attenuation curve, in metres.
const REF: f64 = 2.0;

/// Which collision layer a probe ray tests against — the source's
/// `phys.MASK.SIGHT` / `phys.MASK.WORLD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RayMask {
    Sight,
    World,
}

/// What a probe ray found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub distance: f64,
    pub surface: Surface,
}

/// The one capability audio needs from the world: cast a ray, tell me what it
/// hit.
///
/// **This is the port's one invented seam, and it is deliberate.** The source
/// duck-types the physics subsystem — `ctx.peek('physics')?.raycast` — and
/// silently degrades to "no occlusion, everything is open ground" when it is
/// absent (`spatial.js:207-208`, `index.js:305`). `physics` is not ported yet,
/// and inventing a whole physics facade here to satisfy audio would be exactly
/// the wrong shape. Naming the single method audio actually calls keeps the
/// degrade-gracefully behaviour (`None` probe == no physics) and hands the
/// physics arm of the port one obvious trait to implement.
pub trait WorldProbe {
    /// `phys.raycast(origin, dir, maxDist, mask)`. `dir` is **not** normalised
    /// by the caller — the source passes an un-normalised delta and a length,
    /// exactly as reproduced in [`SpatialField::occlusion_at`].
    fn raycast(
        &self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        mask: RayMask,
    ) -> Option<RayHit>;
}

/// One pooled emitter chain (`spatial.js:31-133`).
#[derive(Debug, Clone, Copy)]
pub struct Emitter {
    pub input: NodeId,
    pub occ_lp: NodeId,
    pub occ_hs: NodeId,
    pub air_lp: NodeId,
    pub dist_gain: NodeId,
    pub send_gain: NodeId,
    pub panner: NodeId,

    pub free: bool,
    pub end_time: f64,
    pub priority: f64,
    pub bus_name: Bus,
    pub attached: Option<NodeId>,
    pub tracked: bool,
    pub pos: [f64; 3],
    pub user_gain: f64,
    connected: Option<NodeId>,
    send_connected: Option<NodeId>,
}

impl Emitter {
    fn new(g: &mut AudioGraph) -> Self {
        let input = gain(g, 1.0);
        let occ_lp = biquad(g, FilterKind::Lowpass, 20000.0, 0.4, 0.0);
        let occ_hs = biquad(g, FilterKind::Highshelf, 2200.0, 0.7, 0.0);
        let air_lp = biquad(g, FilterKind::Lowpass, 20000.0, 0.5, 0.0);
        let dist_gain = gain(g, 1.0);
        let send_gain = gain(g, 0.0);
        let panner = g.create_panner();

        g.connect(input, occ_lp);
        g.connect(occ_lp, occ_hs);
        g.connect(occ_hs, air_lp);
        g.connect(air_lp, dist_gain);
        g.connect(dist_gain, panner);
        g.connect(dist_gain, send_gain);

        Emitter {
            input,
            occ_lp,
            occ_hs,
            air_lp,
            dist_gain,
            send_gain,
            panner,
            free: true,
            end_time: 0.0,
            priority: 0.0,
            bus_name: Bus::Foley,
            attached: None,
            tracked: false,
            pos: [0.0; 3],
            user_gain: 1.0,
            connected: None,
            send_connected: None,
        }
    }

    fn set_pos(&mut self, g: &mut AudioGraph, x: f64, y: f64, z: f64, when: f64) {
        for (axis, v) in [x, y, z].into_iter().enumerate() {
            g.set_value_at_time(self.panner.position(axis), v, when);
        }
        self.pos = [x, y, z];
    }

    /// Smoothly move a long-lived emitter (ambience beds, voices, loops).
    pub fn move_to(&mut self, g: &mut AudioGraph, x: f64, y: f64, z: f64, smooth: f64) {
        let t = g.current_time();
        for (axis, v) in [x, y, z].into_iter().enumerate() {
            g.set_target_at_time(self.panner.position(axis), v, t, smooth);
        }
        self.pos = [x, y, z];
    }

    fn connect_out(&mut self, g: &mut AudioGraph, bus_node: NodeId, send_node: NodeId) {
        if self.connected.is_none() {
            g.connect(self.panner, bus_node);
            self.connected = Some(bus_node);
        }
        if self.send_connected.is_none() {
            g.connect(self.send_gain, send_node);
            self.send_connected = Some(send_node);
        }
    }

    fn detach(&mut self, g: &mut AudioGraph) {
        if let Some(node) = self.connected.take() {
            g.disconnect(self.panner, Sink::Node(node));
        }
        if let Some(node) = self.send_connected.take() {
            g.disconnect(self.send_gain, Sink::Node(node));
        }
        if let Some(node) = self.attached.take() {
            g.disconnect_all(node);
        }
        self.tracked = false;
        self.free = true;
    }

    fn dispose(&mut self, g: &mut AudioGraph) {
        self.detach(g);
        g.disconnect_all(self.input);
        g.disconnect_all(self.occ_lp);
        g.disconnect_all(self.occ_hs);
        g.disconnect_all(self.air_lp);
        g.disconnect_all(self.dist_gain);
        g.disconnect_all(self.send_gain);
        g.disconnect_all(self.panner);
    }
}

/// `acquire`'s options bag (`spatial.js:237`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcquireOpts {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub when: Option<f64>,
    pub bus: Bus,
    pub send: f64,
    pub priority: f64,
    pub end_time: Option<f64>,
    /// `None` runs the raycast; `Some` is the caller asserting a value (the
    /// distant-volley path passes 0 — "it is over the rooftops, not through
    /// them").
    pub occlusion: Option<f64>,
    pub dist: Option<f64>,
    pub atten: Option<f64>,
    pub gain: f64,
    pub tracked: bool,
}

impl Default for AcquireOpts {
    fn default() -> Self {
        AcquireOpts {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            when: None,
            bus: Bus::Foley,
            send: 0.25,
            priority: 0.5,
            end_time: None,
            occlusion: None,
            dist: None,
            atten: None,
            gain: 1.0,
            tracked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldStats {
    pub active: usize,
    pub stolen: usize,
    pub dropped: usize,
    pub occlusion_rays: usize,
}

pub struct SpatialField {
    emitters: Vec<Emitter>,
    /// Preallocated scratch in the source; a plain field here.
    listener_pos: [f64; 3],
    track_cursor: usize,
    pub stats: FieldStats,
    pub occlusion_enabled: bool,
}

impl SpatialField {
    pub fn new(g: &mut AudioGraph) -> Self {
        let emitters = (0..MAX_EMITTERS).map(|_| Emitter::new(g)).collect();
        SpatialField {
            emitters,
            listener_pos: [0.0, 1.6, 0.0],
            track_cursor: 0,
            stats: FieldStats::default(),
            occlusion_enabled: true,
        }
    }

    /// Feed the audio listener from the render camera (`spatial.js:158-178`).
    /// Called once per frame.
    ///
    /// `setTargetAtTime` rather than a hard set: the doppler-free smoothing
    /// kills the zipper noise you otherwise get from a 60 Hz position update.
    #[allow(clippy::too_many_arguments)]
    pub fn set_listener(
        &mut self,
        g: &mut AudioGraph,
        pos: [f64; 3],
        forward: [f64; 3],
        up: [f64; 3],
    ) {
        self.listener_pos = pos;
        g.set_listener(pos, forward, up);
    }

    pub fn listener_pos(&self) -> [f64; 3] {
        self.listener_pos
    }

    pub fn distance_to(&self, x: f64, y: f64, z: f64) -> f64 {
        let l = self.listener_pos;
        hypot3(x - l[0], y - l[1], z - l[2])
    }

    /// Attenuation curve (`spatial.js:194-198`).
    ///
    /// Deliberately gentler than 1/r beyond ~40 m: real gunfire at 150 m is
    /// still clearly audible, and pure inverse-distance makes a level feel dead.
    /// Below 40 m it is very close to physical.
    pub fn attenuation(&self, dist: f64) -> f64 {
        let near = REF / (REF + 0.85 * (dist - REF).max(0.0));
        let far = 0.055 * (60.0 / dist.max(60.0)).powf(0.55);
        clamp(near.max(if dist > 45.0 { far } else { 0.0 }), 0.0, 1.0)
    }

    /// Occlusion test (`spatial.js:205-231`): how much geometry is between the
    /// listener and a point. Returns 0 (clear) .. 1 (thick wall).
    ///
    /// Two rays — ear height and a raised one — so a low crate does not fully
    /// mute a source behind it.
    pub fn occlusion_at(&mut self, probe: Option<&dyn WorldProbe>, x: f64, y: f64, z: f64) -> f64 {
        if !self.occlusion_enabled {
            return 0.0;
        }
        let Some(phys) = probe else {
            return 0.0;
        };
        let l = self.listener_pos;
        let d = hypot3(x - l[0], y - l[1], z - l[2]);
        if d < 0.8 {
            return 0.0;
        }
        let mut blocked = 0.0f64;
        for i in 0..2 {
            let lift = if i == 0 { 0.0 } else { 0.55 };
            let o = [l[0], l[1] + lift, l[2]];
            let dir = [x - o[0], y + lift * 0.5 - o[1], z - o[2]];
            let len = hypot3(dir[0], dir[1], dir[2]);
            if len < 1e-4 {
                continue;
            }
            self.stats.occlusion_rays += 1;
            if let Some(hit) = phys.raycast(o, dir, len - 0.25, RayMask::Sight) {
                // A thin partition muffles less than a bunker wall: use how far
                // past the first hit the ray continued as a crude thickness
                // proxy.
                blocked += if hit.distance < len * 0.9 { 1.0 } else { 0.5 };
            }
        }
        clamp(blocked / 2.0, 0.0, 1.0)
    }

    /// Grab an emitter (`spatial.js:239-291`). Returns `None` when the budget is
    /// full and the new sound is less important than everything already playing.
    pub fn acquire(
        &mut self,
        g: &mut AudioGraph,
        mixer: &Mixer,
        probe: Option<&dyn WorldProbe>,
        opts: AcquireOpts,
    ) -> Option<usize> {
        let now = g.current_time();
        let mut em = self.emitters.iter().position(|e| e.free);
        if em.is_none() {
            // Steal the least important voice that is closest to finishing.
            let mut worst: Option<usize> = None;
            let mut worst_score = f64::INFINITY;
            let pri = opts.priority;
            for (i, e) in self.emitters.iter().enumerate() {
                if e.tracked {
                    continue; // never steal a bed/loop
                }
                let score = e.priority * 4.0 + (e.end_time - now).max(0.0);
                if score < worst_score {
                    worst_score = score;
                    worst = Some(i);
                }
            }
            match worst {
                Some(i) if self.emitters[i].priority <= pri + 0.25 => {
                    self.emitters[i].detach(g);
                    self.stats.stolen += 1;
                    em = Some(i);
                }
                _ => {
                    self.stats.dropped += 1;
                    return None;
                }
            }
        }
        let idx = em?;

        let t = opts.when.unwrap_or(now);
        let dist = opts
            .dist
            .unwrap_or_else(|| self.distance_to(opts.x, opts.y, opts.z));
        let occ = match opts.occlusion {
            Some(v) => v,
            None => self.occlusion_at(probe, opts.x, opts.y, opts.z),
        };
        let atten = opts.atten.unwrap_or_else(|| self.attenuation(dist)) * (1.0 - 0.62 * occ);

        let e = &mut self.emitters[idx];
        e.free = false;
        e.priority = opts.priority;
        e.end_time = opts.end_time.unwrap_or(now + 1.0);
        e.bus_name = opts.bus;
        e.tracked = opts.tracked;
        e.user_gain = opts.gain;
        e.set_pos(g, opts.x, opts.y, opts.z, t);

        // Air absorption + occlusion filtering.
        g.set_value_at_time(e.air_lp.frequency(), air_cutoff(dist), t);
        let occ_cut = 20000.0 * 0.021f64.powf(occ); // 1.0 -> ~420 Hz
        g.set_value_at_time(e.occ_lp.frequency(), clamp(occ_cut, 300.0, 20000.0), t);
        g.set_value_at_time(e.occ_hs.gain(), -26.0 * occ, t);
        g.set_value_at_time(e.dist_gain.gain(), clamp(atten * opts.gain, 0.0, 4.0), t);

        // Farther and more occluded => proportionally wetter.
        let send = opts.send * (0.5 + dist.min(90.0) * 0.022) * (1.0 + occ * 0.7);
        g.set_value_at_time(e.send_gain.gain(), clamp(send, 0.0, 3.0), t);

        let bus_node = mixer.bus(e.bus_name);
        e.connect_out(g, bus_node, mixer.reverb_send);
        Some(idx)
    }

    /// Update an in-flight tracked emitter's occlusion/distance (beds, voices)
    /// (`spatial.js:294-305`).
    pub fn refresh(&mut self, g: &mut AudioGraph, probe: Option<&dyn WorldProbe>, idx: usize) {
        if self.emitters[idx].free {
            return;
        }
        let t = g.current_time();
        let p = self.emitters[idx].pos;
        let dist = self.distance_to(p[0], p[1], p[2]);
        let occ = self.occlusion_at(probe, p[0], p[1], p[2]);
        let atten = self.attenuation(dist) * (1.0 - 0.62 * occ);
        let e = self.emitters[idx];
        g.set_target_at_time(e.air_lp.frequency(), air_cutoff(dist), t, 0.12);
        g.set_target_at_time(
            e.occ_lp.frequency(),
            clamp(20000.0 * 0.021f64.powf(occ), 300.0, 20000.0),
            t,
            0.12,
        );
        g.set_target_at_time(e.occ_hs.gain(), -26.0 * occ, t, 0.12);
        g.set_target_at_time(
            e.dist_gain.gain(),
            clamp(atten * e.user_gain, 0.0, 4.0),
            t,
            0.1,
        );
    }

    /// Hand a voice's top node to an emitter and set its teardown time
    /// (`spatial.js:308-312`).
    pub fn hold(&mut self, g: &mut AudioGraph, idx: usize, node: NodeId, end_time: f64) {
        g.connect(node, self.emitters[idx].input);
        self.emitters[idx].attached = Some(node);
        self.emitters[idx].end_time = end_time;
    }

    pub fn emitter(&self, idx: usize) -> &Emitter {
        &self.emitters[idx]
    }

    pub fn emitter_mut(&mut self, idx: usize) -> &mut Emitter {
        &mut self.emitters[idx]
    }

    /// `spatial.js:314-341`.
    pub fn update(&mut self, g: &mut AudioGraph, probe: Option<&dyn WorldProbe>) {
        let now = g.current_time();
        let mut active = 0;
        for i in 0..self.emitters.len() {
            let e = &self.emitters[i];
            if e.free {
                continue;
            }
            if !e.tracked && now > e.end_time {
                self.emitters[i].detach(g);
                continue;
            }
            active += 1;
        }
        self.stats.active = active;

        // Re-evaluate one tracked emitter per frame: 40 emitters at 60 fps is a
        // 1.5 Hz refresh worst case, which is plenty for beds and walking NPCs
        // and costs at most two raycasts a frame.
        let n = self.emitters.len();
        for _ in 0..n {
            self.track_cursor = (self.track_cursor + 1) % n;
            let cursor = self.track_cursor;
            let e = &self.emitters[cursor];
            if !e.free && e.tracked {
                self.refresh(g, probe, cursor);
                break;
            }
        }
    }

    pub fn dispose(&mut self, g: &mut AudioGraph) {
        for i in 0..self.emitters.len() {
            let mut e = self.emitters[i];
            e.dispose(g);
            self.emitters[i] = e;
        }
        self.emitters.clear();
    }
}

/// `Math.hypot(a, b, c)` — [`crate::jsmath::hypot3`].
///
/// This module used to define the plain root here, on the reasoning that it
/// "agrees with `Math.hypot` to within a couple of ULP over the metre-scale
/// distances the game deals in". That reasoning was wrong twice over. It is
/// measurably false — the plain root disagrees with V8 on 1,538 of 4,096
/// sampled metre-scale triples (37.5%), per `tests/jsmath/capture.mjs` — and
/// `ai/geo.rs` went on to cite this module's comment as its own justification
/// for shipping the same wrong form. See [`crate::jsmath`]'s module doc.
use crate::jsmath::hypot3;
