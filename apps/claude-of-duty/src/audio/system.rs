//! The audio subsystem facade and its event wiring.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/index.js:1-868` — the whole
//! file.
//!
//! Everything is generated. There is not a single audio file in the project.
//!
//! ```text
//! PUBLIC API   const audio = ctx.get('audio')
//!   audio.running                      graph is live (needs a user gesture)
//!   audio.start()                      force-start
//!   audio.deafness                     0..1 concussion; UI/post may read this
//!   audio.play(kind, position, opts)   one-shot; position None = head-locked
//!   audio.bark(kind, position, opts)   enemy voice
//!   audio.ui(kind)                     'hitmarker'|'headshot'|'kill'|'damage'
//!   audio.set_master_volume(v)  audio.set_bus_volume(bus, v)
//!   audio.set_ambience_intensity(v)    scales the distant-battle scheduler
//!   audio.report()                     diagnostics snapshot
//! ```
//!
//! All of it is a no-op — never an error — before the graph exists, so callers
//! never have to check whether audio started.
//!
//! Driven off the canonical events: `weapon:fire`, `weapon:reload`,
//! `weapon:shell`, `bullet:impact`, `bullet:tracer`, `damage:dealt`,
//! `damage:taken`, `actor:death`, `player:land`, `player:footstep`,
//! `player:state`, `explosion`, and the optional `ai:bark`.
//!
//! ## The three seams this port had to name
//!
//! 1. **The listener basis.** The source reads `ctx.camera.matrixWorld` every
//!    frame (`index.js:221-227`). There is no camera in the port yet, and
//!    inventing one here would put render policy in the audio system. The basis
//!    arrives through [`AudioCore::set_listener_basis`] instead, called by
//!    whoever owns the camera. Same nine numbers, same frame position.
//! 2. **The physics raycast.** `ctx.peek('physics')?.raycast` is duck-typed and
//!    already degrades to "no occlusion" when physics is absent; the port names
//!    that one method as [`WorldProbe`] and keeps the degradation.
//! 3. **Shared mutable state.** JavaScript's `this` is reachable from both the
//!    frame loop and every event handler at once. [`EventBus`] handlers are
//!    `Fn`, so the state they mutate lives behind an `Rc<RefCell<AudioCore>>`
//!    that [`AudioSystem`] also holds — which *is* what `this` is, spelled out.
//!    Dispatch stays synchronous, so a shot fired inside a fixed step is still
//!    scheduled at that instant's context time rather than a frame later.

// Two things in `index.js` have no counterpart here, both for the same reason.
//
// `play(kind, position, opts)` (`index.js:448-456`) begins by untangling three
// different calling conventions other subsystems use — a number where a
// position goes, an options bag where a position goes, a bare gain. Rust's
// signature *is* that disambiguation, so the shuffling has nothing to do.
//
// `_error(err)` (`index.js:288-296`) counts exceptions and disables audio after
// forty. Nothing in the ported synthesis can throw: it is arithmetic over a
// `Vec`, the envelope helpers refuse non-finite input by value rather than by
// exception (see `dsp::ok`), and the one fallible thing left — constructing an
// `AudioContext` — lives at the platform edge in `web_audio`. A counter of
// impossible failures is not a safety net, it is decoration.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use axiom_kernel::Seconds;

use crate::audio::ambience::{ambient_one_shot, Ambience, AmbienceCue, OneShot};
use crate::audio::dsp::{clamp, gain as mk_gain, NoiseBank, SPEED_OF_SOUND};
use crate::audio::foley::{
    body_fall, cloth, explosion, footstep, heartbeat, reload_phase, shell_casing, surface_impact,
    ui_sound, Gait, ReloadPhase, StepOpts, Surface, UiSound,
};
use crate::audio::graph::{AudioGraph, NodeId};
use crate::audio::ir::{classify_space, Space, SpaceWeights};
use crate::audio::mixer::{Bus, Mixer};
use crate::audio::spatial::{AcquireOpts, RayMask, SpatialField, WorldProbe};
use crate::audio::vox::{bark as vox_bark, bark_for, Bark, BarkOpts, BarkRequest};
use crate::audio::weapons::{
    bullet_whizz, dry_fire, resolve_profile, weapon_shot, RoundRobinBank, ShotOpts, Voice,
    WeaponProfile, RIFLE, SUPPRESSED,
};
use crate::engine::Ctx;
use crate::events::SubscriptionId;
use crate::registry::{Phase, Subsystem};
use crate::rng::Rng;

const PROBE_RAYS: usize = 9;
const PROBE_DIST: f64 = 40.0;
const DRY_SLOTS: usize = 48;

/// A world position. The source's `{x, y, z}` object literals.
pub type Vec3 = [f64; 3];

/// `index.js:65-67` — one NaN from any subsystem must not throw.
fn is_vec(p: Vec3) -> bool {
    p[0].is_finite() && p[1].is_finite() && p[2].is_finite()
}

/* ================================================================ */
/* Event payloads                                                   */
/* ================================================================ */

/// `weapon:fire`.
#[derive(Debug, Clone, Default)]
pub struct WeaponFire {
    /// `p.weapon` collapsed to the name `resolveProfile` would see: the string
    /// itself, or `w.audio ?? w.id ?? w.name ?? w.kind`.
    pub weapon: Option<String>,
    /// `w.suppressed` — forces the suppressed profile regardless of name.
    pub suppressed: bool,
    pub empty: bool,
    pub origin: Option<Vec3>,
    /// `None` means "decide from distance", the source's `p.firstPerson ?? dist < 2.6`.
    pub first_person: Option<bool>,
}

/// `weapon:reload`.
#[derive(Debug, Clone, Default)]
pub struct WeaponReload {
    pub weapon: Option<String>,
    /// `None` is the source's `p?.phase ?? 'end'`.
    pub phase: Option<ReloadPhase>,
    pub position: Option<Vec3>,
}

/// `weapon:shell`.
#[derive(Debug, Clone, Default)]
pub struct WeaponShell {
    pub position: Option<Vec3>,
}

/// `bullet:impact`.
#[derive(Debug, Clone, Default)]
pub struct BulletImpact {
    pub point: Option<Vec3>,
    pub surface: Option<Surface>,
    pub damage: Option<f64>,
    /// Only the entry side gets a sound.
    pub exit: bool,
}

/// `bullet:tracer`.
#[derive(Debug, Clone, Copy)]
pub struct BulletTracer {
    pub from: Vec3,
    pub to: Vec3,
    pub speed: Option<f64>,
}

/// `explosion`.
#[derive(Debug, Clone, Copy)]
pub struct ExplosionEvent {
    pub position: Vec3,
    pub radius: Option<f64>,
}

/// `player:footstep`.
#[derive(Debug, Clone, Default)]
pub struct PlayerFootstep {
    pub position: Option<Vec3>,
    pub surface: Option<Surface>,
    /// `None` is the source's `p?.running ? 'run' : p?.crouched ? 'crouch' : 'walk'`.
    pub gait: Option<Gait>,
    pub level: Option<f64>,
}

/// `player:land`.
#[derive(Debug, Clone, Default)]
pub struct PlayerLand {
    /// `Math.abs(...)` of a scalar velocity or the `.y` of a vector one; the
    /// source's `?? 4` default lives in the caller.
    pub velocity: Option<f64>,
    pub surface: Option<Surface>,
}

/// `player:state`.
#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    pub stance: Option<String>,
    pub ads: Option<bool>,
}

/// `damage:dealt`.
#[derive(Debug, Clone, Default)]
pub struct DamageDealt {
    /// The source's three-way `t === 'player' || t?.isPlayer === true ||
    /// t === ctx.peek('player')` test, decided by the emitter.
    pub target_is_player: bool,
    pub has_target: bool,
    pub headshot: bool,
    pub killed: bool,
    pub point: Option<Vec3>,
}

/// `damage:taken`.
#[derive(Debug, Clone, Default)]
pub struct DamageTaken {
    pub amount: Option<f64>,
    pub health: Option<f64>,
}

/// `actor:death`.
#[derive(Debug, Clone, Default)]
pub struct ActorDeath {
    pub point: Option<Vec3>,
    pub actor_id: i64,
}

/// `ai:bark` — optional, emitted by `ai` if it wants scripted chatter.
#[derive(Debug, Clone, Default)]
pub struct AiBark {
    pub kind: Option<BarkRequest>,
    pub position: Option<Vec3>,
    pub voice: i64,
}

/* ================================================================ */
/* Voice dispatch                                                   */
/* ================================================================ */

/// `_build`'s `kind` (`index.js:338-358`), with the per-kind options the source
/// pulls out of one untyped bag.
#[derive(Debug, Clone)]
pub enum VoiceKind {
    Shot {
        profile: &'static WeaponProfile,
        first_person: bool,
    },
    Whizz {
        miss: f64,
        gain: f64,
    },
    DryFire,
    Impact {
        surface: Surface,
        energy: f64,
    },
    Step(StepOpts),
    Shell {
        surface: Surface,
        level: f64,
        flight: Option<f64>,
    },
    Reload {
        phase: ReloadPhase,
        heavy: f64,
    },
    Explosion {
        radius: f64,
        level: f64,
    },
    BodyFall {
        level: f64,
    },
    Cloth {
        level: f64,
    },
    Heartbeat {
        level: f64,
    },
    Bark(BarkOpts),
    Ambient {
        which: OneShot,
        level: f64,
    },
    Ui {
        kind: UiSound,
        level: f64,
    },
}

impl VoiceKind {
    /// `BUS_FOR` (`index.js:57-62`) — the default bus per voice kind, so callers
    /// do not have to know the mix layout.
    pub fn default_bus(&self) -> Bus {
        match self {
            VoiceKind::Shot { .. } | VoiceKind::Explosion { .. } | VoiceKind::DryFire => {
                Bus::Weapons
            }
            VoiceKind::Ui { kind, .. } => match kind {
                UiSound::Hitmarker
                | UiSound::Headshot
                | UiSound::Kill
                | UiSound::Armour
                | UiSound::Damage
                | UiSound::GrenadeWarn
                | UiSound::Regen
                | UiSound::LowHealth => Bus::Ui,
                UiSound::Blip => Bus::Foley,
            },
            VoiceKind::Bark(_) => Bus::Voice,
            VoiceKind::Ambient { .. } => Bus::Ambience,
            _ => Bus::Foley,
        }
    }
}

/// The tail of `_playAt`/`_playDry`'s options bag that is not voice-specific.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayOpts {
    pub gain: f64,
    /// Overrides the voice's own `send`.
    pub send: Option<f64>,
    pub max_dist: f64,
    pub no_delay: bool,
    pub extra_delay: f64,
    pub occlusion: Option<f64>,
    pub tracked: bool,
}

impl Default for PlayOpts {
    fn default() -> Self {
        PlayOpts {
            gain: 1.0,
            send: None,
            max_dist: 320.0,
            no_delay: false,
            extra_delay: 0.0,
            occlusion: None,
            tracked: false,
        }
    }
}

/// `index.js:114-117`'s stats block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioStats {
    pub voices: usize,
    pub dropped: usize,
    pub stolen: usize,
    pub rays: usize,
    pub deafness: f64,
    pub space: Space,
    pub started: bool,
    pub events: usize,
}

impl Default for AudioStats {
    fn default() -> Self {
        AudioStats {
            voices: 0,
            dropped: 0,
            stolen: 0,
            rays: 0,
            deafness: 0.0,
            space: Space::Open,
            started: false,
            events: 0,
        }
    }
}

/// `report()` (`index.js:848-867`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioReport {
    pub running: bool,
    pub sample_rate: f64,
    pub voices: usize,
    pub dropped: usize,
    pub stolen: usize,
    pub occlusion_rays: usize,
    pub space: Space,
    pub space_weights: SpaceWeights,
    pub enclosure: f64,
    pub mean_free: f64,
    pub deafness: f64,
    pub limiter_reduction: f64,
    pub events: usize,
}

/// A head-locked voice's bookkeeping slot (`index.js:101`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct DrySlot {
    node: Option<NodeId>,
    send: Option<NodeId>,
    end: f64,
}

/// The live graph and everything hanging off it.
struct Live {
    graph: AudioGraph,
    bank: NoiseBank,
    mixer: Mixer,
    field: SpatialField,
    ambience: Ambience,
}

/// The mutable guts — JavaScript's `this`, made explicit.
pub struct AudioCore {
    pub running: bool,
    live: Option<Live>,
    rng: Rng,
    rr: RoundRobinBank,
    probe: Option<Rc<dyn WorldProbe>>,

    /* preallocated scratch */
    probe_dirs: [Vec3; PROBE_RAYS],
    probe_hits: [f64; PROBE_RAYS],
    space: SpaceWeights,
    probe_timer: f64,
    last_probe: Vec3,
    listener: ([f64; 3], [f64; 3], [f64; 3]),

    dry: [DrySlot; DRY_SLOTS],
    dry_cursor: usize,

    /* per-frame rate limits */
    budget_impact: u32,
    budget_step: u32,
    budget_shell: u32,
    budget_whizz: u32,
    last_bark_time: f64,
    last_enemy_fire: f64,

    health: f64,
    heart_timer: f64,
    stance: Option<String>,
    ads: bool,

    pub deafness: f64,
    pub stats: AudioStats,
}

impl AudioCore {
    /// `new AudioSystem()` plus `init(ctx)`'s `this.rng = ctx.rng.fork()`
    /// (`index.js:73-130`).
    pub fn new(rng: Rng) -> Self {
        // 8 rays around the horizon, 1 up.
        let mut probe_dirs = [[0.0; 3]; PROBE_RAYS];
        for (i, dir) in probe_dirs.iter_mut().enumerate().take(PROBE_RAYS - 1) {
            let a = (i as f64 / (PROBE_RAYS - 1) as f64) * std::f64::consts::PI * 2.0;
            *dir = [a.cos(), 0.06, a.sin()];
        }
        probe_dirs[PROBE_RAYS - 1] = [0.0, 1.0, 0.0];

        AudioCore {
            running: false,
            live: None,
            rng,
            rr: RoundRobinBank::new(),
            probe: None,
            probe_dirs,
            probe_hits: [PROBE_DIST; PROBE_RAYS],
            space: SpaceWeights::outdoors(PROBE_DIST),
            probe_timer: 0.0,
            last_probe: [1e9, 0.0, 0.0],
            listener: ([0.0, 1.6, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            dry: [DrySlot {
                node: None,
                send: None,
                end: 0.0,
            }; DRY_SLOTS],
            dry_cursor: 0,
            budget_impact: 0,
            budget_step: 0,
            budget_shell: 0,
            budget_whizz: 0,
            last_bark_time: -99.0,
            last_enemy_fire: -99.0,
            health: 100.0,
            heart_timer: 0.0,
            stance: None,
            ads: false,
            deafness: 0.0,
            stats: AudioStats::default(),
        }
    }

    /// Install the world probe that occlusion and the space classifier raycast
    /// against. `None` (the default) is the source's "physics is not registered"
    /// path: no occlusion, everything reads as open ground.
    pub fn set_world_probe(&mut self, probe: Option<Rc<dyn WorldProbe>>) {
        self.probe = probe;
    }

    /// The camera basis, in place of `ctx.camera.matrixWorld` (`index.js:221-227`).
    pub fn set_listener_basis(&mut self, position: Vec3, forward: Vec3, up: Vec3) {
        self.listener = (position, forward, up);
    }

    /// Build the graph (`index.js:157-186`).
    ///
    /// The source's gesture arming, `AudioContext` construction and `resume()`
    /// are the platform edge and live in [`crate::audio::web_audio`]; what is
    /// left — and what actually is the subsystem — is everything below.
    pub fn start(&mut self, sample_rate: f64) -> bool {
        if self.running {
            return true;
        }
        let mut graph = AudioGraph::new(sample_rate);
        let bank = NoiseBank::new(&mut graph, &mut self.rng.fork(), 2.4);
        let mut mixer = Mixer::new(&mut graph, self.rng.fork(), 0.95);
        mixer.build_reverbs(&mut graph);
        let field = SpatialField::new(&mut graph);
        let mut ambience = Ambience::new(self.rng.fork());
        ambience.start(&mut graph, &bank, &mixer);
        let space = self.space;
        mixer.set_space(&mut graph, &space, 0.001);
        self.live = Some(Live {
            graph,
            bank,
            mixer,
            field,
            ambience,
        });
        self.running = true;
        self.stats.started = true;
        true
    }

    fn teardown(&mut self) {
        if let Some(live) = self.live.as_mut() {
            live.ambience.dispose(&mut live.graph);
            live.field.dispose(&mut live.graph);
            live.mixer.dispose(&mut live.graph);
        }
        self.live = None;
        self.running = false;
    }

    pub fn dispose(&mut self) {
        self.teardown();
    }

    /// Read-only access to the recorded graph — how a native test inspects what
    /// the subsystem actually built.
    pub fn graph(&self) -> Option<&AudioGraph> {
        self.live.as_ref().map(|l| &l.graph)
    }

    pub fn space(&self) -> SpaceWeights {
        self.space
    }

    /// Advance the context clock. In the browser the audio device does this; a
    /// native test drives it.
    pub fn advance(&mut self, dt: f64) {
        if let Some(live) = self.live.as_mut() {
            let t = live.graph.current_time();
            live.graph.set_current_time(t + dt);
        }
    }

    /* ================================================================ */
    /* frame                                                            */
    /* ================================================================ */

    /// `update(dt, ctx)` (`index.js:213-286`).
    pub fn update(&mut self, dt: f64) {
        if !self.running {
            return;
        }

        /* ---- listener from the render camera ----------------------- */
        let (pos, fwd, up) = self.listener;
        if let Some(live) = self.live.as_mut() {
            live.field.set_listener(&mut live.graph, pos, fwd, up);
        }

        /* ---- space probe ------------------------------------------- */
        self.probe_timer -= dt;
        let moved = (pos[0] - self.last_probe[0]).abs()
            + (pos[1] - self.last_probe[1]).abs()
            + (pos[2] - self.last_probe[2]).abs();
        if self.probe_timer <= 0.0 || moved > 1.6 {
            self.probe_timer = 0.45;
            self.last_probe = pos;
            self.probe_space(pos);
        }

        /* ---- subsystems -------------------------------------------- */
        let cues = {
            let probe = self.probe.clone();
            let Some(live) = self.live.as_mut() else {
                return;
            };
            live.mixer.update(&mut live.graph, dt);
            live.field
                .update(&mut live.graph, probe.as_deref());
            self.deafness = live.mixer.deafness;
            live.ambience.update(&mut live.graph, dt)
        };
        for cue in cues {
            match cue {
                AmbienceCue::DistantVolley => self.distant_volley(),
                AmbienceCue::DistantBoom => self.distant_boom(),
                AmbienceCue::OneShot => self.ambient_one_shot(),
                AmbienceCue::DistantChatter => self.distant_chatter(),
            }
        }

        /* ---- head-locked voice teardown ---------------------------- */
        let now = self.live.as_ref().map_or(0.0, |l| l.graph.current_time());
        for i in 0..DRY_SLOTS {
            let d = self.dry[i];
            let Some(node) = d.node else { continue };
            if now < d.end {
                continue;
            }
            if let Some(live) = self.live.as_mut() {
                live.graph.disconnect_all(node);
                if let Some(s) = d.send {
                    live.graph.disconnect_all(s);
                }
            }
            self.dry[i] = DrySlot {
                node: None,
                send: None,
                end: 0.0,
            };
        }

        /* ---- low-health heartbeat ---------------------------------- */
        if self.health < 34.0 {
            self.heart_timer -= dt;
            if self.heart_timer <= 0.0 {
                self.heart_timer = 0.62 + (self.health / 34.0) * 0.45;
                let level = clamp(1.0 - self.health / 34.0, 0.2, 1.0);
                self.play_dry(
                    VoiceKind::Heartbeat { level },
                    PlayOpts::default(),
                    Bus::Foley,
                    0.1,
                );
            }
        }

        /* ---- reset per-frame budgets ------------------------------- */
        self.budget_impact = 0;
        self.budget_step = 0;
        self.budget_shell = 0;
        self.budget_whizz = 0;

        if let Some(live) = self.live.as_ref() {
            self.stats.voices = live.field.stats.active;
            self.stats.dropped = live.field.stats.dropped;
            self.stats.stolen = live.field.stats.stolen;
            self.stats.rays = live.field.stats.occlusion_rays;
        }
        self.stats.deafness = self.deafness;
    }

    /* ================================================================ */
    /* environment probe                                                */
    /* ================================================================ */

    /// `_probeSpace` (`index.js:302-325`).
    fn probe_space(&mut self, origin: Vec3) {
        match self.probe.as_deref() {
            Some(phys) => {
                for i in 0..PROBE_RAYS {
                    let h = phys.raycast(origin, self.probe_dirs[i], PROBE_DIST, RayMask::World);
                    self.probe_hits[i] = h.map_or(PROBE_DIST, |h| h.distance);
                }
            }
            None => self.probe_hits = [PROBE_DIST; PROBE_RAYS],
        }
        let hits = self.probe_hits;
        classify_space(&hits, PROBE_DIST, &mut self.space);
        let space = self.space;
        if let Some(live) = self.live.as_mut() {
            live.mixer.set_space(&mut live.graph, &space, 0.4);
            live.ambience
                .set_enclosure(&mut live.graph, space.enclosure);
        }
        self.stats.space = space.dominant();
    }

    /* ================================================================ */
    /* voice plumbing                                                   */
    /* ================================================================ */

    /// Build a voice by kind (`index.js:335-359`). `when` is absolute context
    /// time, `dist` is metres from the listener — voices use it to rebalance
    /// their own layers.
    fn build(&mut self, kind: &VoiceKind, when: f64, dist: f64) -> Voice {
        let space = self.space;
        let live = self.live.as_mut().expect("built only while running");
        let g = &mut live.graph;
        let bank = &live.bank;
        let rng = &mut self.rng;
        let w = Some(when);
        match kind {
            VoiceKind::Shot {
                profile,
                first_person,
            } => weapon_shot(
                g,
                bank,
                rng,
                &mut self.rr,
                profile,
                ShotOpts {
                    when: w,
                    distance: dist,
                    first_person: *first_person,
                    echo_boost: 0.75
                        + space.street * 0.7
                        + space.tight * 0.35
                        + space.tunnel * 0.8
                        + space.open * 0.2,
                },
            ),
            VoiceKind::Whizz { miss, gain } => bullet_whizz(g, bank, rng, w, *miss, *gain),
            VoiceKind::DryFire => dry_fire(g, bank, rng, w),
            VoiceKind::Impact { surface, energy } => {
                surface_impact(g, bank, rng, w, *surface, *energy)
            }
            VoiceKind::Step(o) => footstep(g, bank, rng, StepOpts { when: w, ..*o }),
            VoiceKind::Shell {
                surface,
                level,
                flight,
            } => shell_casing(g, bank, rng, w, *surface, *level, *flight),
            VoiceKind::Reload { phase, heavy } => reload_phase(g, bank, rng, *phase, w, *heavy),
            VoiceKind::Explosion { radius, level } => {
                explosion(g, bank, rng, w, dist, *radius, *level)
            }
            VoiceKind::BodyFall { level } => body_fall(g, bank, rng, w, *level),
            VoiceKind::Cloth { level } => cloth(g, bank, rng, w, *level),
            VoiceKind::Heartbeat { level } => heartbeat(g, w, *level),
            VoiceKind::Bark(o) => vox_bark(g, bank, rng, BarkOpts { when: w, ..*o }),
            VoiceKind::Ambient { which, level } => {
                ambient_one_shot(g, bank, rng, *which, w, *level)
            }
            VoiceKind::Ui { kind, level } => ui_sound(g, bank, rng, *kind, w, *level),
        }
    }

    /// Spatialised one-shot: propagation delay, occlusion, air absorption and
    /// the reverb send (`index.js:365-394`). Returns false when the voice budget
    /// refused it.
    pub fn play_at(
        &mut self,
        kind: VoiceKind,
        at: Vec3,
        o: PlayOpts,
        bus: Bus,
        priority: f64,
    ) -> bool {
        if !self.running {
            return false;
        }
        if !is_vec(at) {
            return self.play_dry(kind, o, bus, o.send.unwrap_or(0.15));
        }
        let dist = self
            .live
            .as_ref()
            .map_or(0.0, |l| l.field.distance_to(at[0], at[1], at[2]));
        if dist > o.max_dist {
            return false;
        }
        // Propagation delay is *scheduling*, not a delay node: sample-accurate
        // and free.
        let delay = if o.no_delay {
            0.0
        } else {
            dist / SPEED_OF_SOUND
        };
        let now = self.live.as_ref().map_or(0.0, |l| l.graph.current_time());
        let when = now + delay + o.extra_delay;
        let voice = self.build(&kind, when, dist);

        let probe = self.probe.clone();
        let live = self.live.as_mut().expect("running");
        let acquired = live.field.acquire(
            &mut live.graph,
            &live.mixer,
            probe.as_deref(),
            AcquireOpts {
                x: at[0],
                y: at[1],
                z: at[2],
                when: Some(when),
                dist: Some(dist),
                bus,
                priority,
                send: o.send.unwrap_or(voice.send),
                gain: o.gain,
                end_time: Some(voice.end),
                occlusion: o.occlusion,
                tracked: o.tracked,
                atten: None,
            },
        );
        match acquired {
            Some(idx) => {
                live.field.hold(&mut live.graph, idx, voice.node, voice.end);
                self.stats.events += 1;
                true
            }
            None => {
                live.graph.disconnect_all(voice.node);
                false
            }
        }
    }

    /// Head-locked one-shot: own weapon, UI, player grunts, heartbeat
    /// (`index.js:397-436`).
    pub fn play_dry(&mut self, kind: VoiceKind, o: PlayOpts, bus: Bus, send: f64) -> bool {
        if !self.running {
            return false;
        }
        let now = self.live.as_ref().map_or(0.0, |l| l.graph.current_time());
        let when = now + o.extra_delay;
        let voice = self.build(&kind, when, 0.0);

        let live = self.live.as_mut().expect("running");
        let g = &mut live.graph;
        let out = mk_gain(g, o.gain);
        g.connect(voice.node, out);
        g.connect(out, live.mixer.bus(bus));
        let send_level = o.send.unwrap_or(send) * voice.send;
        let send_node = (send_level > 0.001).then(|| {
            let s = mk_gain(g, send_level);
            g.connect(out, s);
            g.connect(s, live.mixer.reverb_send);
            s
        });

        // Claim a bookkeeping slot; steal the oldest if all are busy.
        let free = (0..DRY_SLOTS)
            .map(|i| (self.dry_cursor + i) % DRY_SLOTS)
            .find(|&idx| self.dry[idx].node.is_none());
        let slot = match free {
            Some(idx) => {
                self.dry_cursor = (idx + 1) % DRY_SLOTS;
                idx
            }
            None => {
                let idx = self.dry_cursor;
                if let Some(node) = self.dry[idx].node {
                    g.disconnect_all(node);
                }
                if let Some(s) = self.dry[idx].send {
                    g.disconnect_all(s);
                }
                self.dry_cursor = (idx + 1) % DRY_SLOTS;
                idx
            }
        };
        self.dry[slot] = DrySlot {
            node: Some(out),
            send: send_node,
            end: voice.end + 0.05,
        };
        self.stats.events += 1;
        true
    }

    /* ================================================================ */
    /* public helpers                                                   */
    /* ================================================================ */

    /// Fire a one-shot (`index.js:448-463`). `position: None` is head-locked.
    pub fn play(&mut self, kind: VoiceKind, position: Option<Vec3>, o: PlayOpts) -> bool {
        let bus = kind.default_bus();
        match position {
            Some(p) => self.play_at(kind, p, o, bus, 0.5),
            None => {
                let send = o.send.unwrap_or(0.15);
                self.play_dry(kind, o, bus, send)
            }
        }
    }

    /// `ui(kind, level)` (`index.js:465-468`).
    pub fn ui(&mut self, kind: UiSound, level: f64) -> bool {
        let voice = VoiceKind::Ui { kind, level };
        let bus = voice.default_bus();
        self.play_dry(voice, PlayOpts::default(), bus, 0.0)
    }

    /// Enemy vocalisation (`index.js:489-505`). `kind` is semantic.
    pub fn bark(
        &mut self,
        kind: BarkRequest,
        position: Option<Vec3>,
        level: f64,
        radio: bool,
        voice_seed: i64,
        force: bool,
    ) -> bool {
        if !self.running {
            return false;
        }
        let now = self.live.as_ref().map_or(0.0, |l| l.graph.current_time());
        if now - self.last_bark_time < 0.42 && !force {
            return false; // no mush
        }
        self.last_bark_time = now;
        let seed = voice_seed;
        let o = BarkOpts {
            when: None,
            bark: bark_for(kind, &mut self.rng),
            f0: Some(96.0 + ((seed * 37) % 41) as f64),
            tract: Some(0.95 + ((seed * 13) % 11) as f64 / 100.0),
            level,
            radio,
        };
        match position {
            Some(p) => self.play_at(VoiceKind::Bark(o), p, PlayOpts::default(), Bus::Voice, 0.85),
            None => self.play_dry(VoiceKind::Bark(o), PlayOpts::default(), Bus::Voice, 0.25),
        }
    }

    pub fn set_master_volume(&mut self, v: f64) {
        if let Some(live) = self.live.as_mut() {
            live.mixer.set_master_volume(&mut live.graph, v);
        }
    }

    pub fn set_bus_volume(&mut self, bus: Bus, v: f64) {
        if let Some(live) = self.live.as_mut() {
            live.mixer.set_bus_volume(&mut live.graph, bus, v);
        }
    }

    pub fn set_ambience_intensity(&mut self, v: f64) {
        if let Some(live) = self.live.as_mut() {
            live.ambience.intensity = clamp(v, 0.0, 3.0);
        }
    }

    pub fn set_occlusion_enabled(&mut self, v: bool) {
        if let Some(live) = self.live.as_mut() {
            live.field.occlusion_enabled = v;
        }
    }

    /* ================================================================ */
    /* events                                                           */
    /* ================================================================ */

    /// `_onFire` (`index.js:536-573`).
    pub fn on_fire(&mut self, p: &WeaponFire) {
        if !self.running {
            return;
        }
        let mut profile = resolve_profile(p.weapon.as_deref());
        if p.suppressed {
            profile = &SUPPRESSED;
        }

        if p.empty {
            self.play_dry(VoiceKind::DryFire, PlayOpts::default(), Bus::Weapons, 0.15);
            return;
        }

        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let at = p.origin.unwrap_or(lp);
        let dist = self
            .live
            .as_ref()
            .map_or(0.0, |l| l.field.distance_to(at[0], at[1], at[2]));
        let first_person = p.first_person.unwrap_or(dist < 2.6);

        if first_person {
            // Own weapon: no propagation delay, mostly dry, and the send level
            // is driven by the space probe so the *room* answers the shot — a
            // tight slap indoors, a long crack down the street.
            let s = self.space;
            let echo = 0.35
                + s.tight * 0.5
                + s.street * 0.9
                + s.tunnel * 1.0
                + s.room * 0.75;
            self.play_dry(
                VoiceKind::Shot {
                    profile,
                    first_person: true,
                },
                PlayOpts::default(),
                Bus::Weapons,
                echo * 0.6,
            );
            if let Some(live) = self.live.as_mut() {
                live.mixer.duck(&mut live.graph, 0.55, 0.1);
            }
        } else {
            self.play_at(
                VoiceKind::Shot {
                    profile,
                    first_person: false,
                },
                at,
                PlayOpts::default(),
                Bus::Weapons,
                0.95,
            );
            let amount = clamp(0.5 - dist * 0.004, 0.12, 0.5);
            if let Some(live) = self.live.as_mut() {
                live.mixer.duck(&mut live.graph, amount, 0.08);
            }
            // Enemies opening fire get occasional chatter, so firefights feel
            // alive even before `ai` grows its own bark logic.
            let now = self.live.as_ref().map_or(0.0, |l| l.graph.current_time());
            if now - self.last_enemy_fire > 4.5 && self.rng.float() < 0.45 {
                self.last_enemy_fire = now;
                let which = if self.rng.float() < 0.6 {
                    BarkRequest::Spot
                } else {
                    BarkRequest::Suppress
                };
                self.bark(which, p.origin, 0.9, false, 0, false);
            }
        }
    }

    /// `_onReload` (`index.js:575-586`).
    pub fn on_reload(&mut self, p: &WeaponReload) {
        if !self.running {
            return;
        }
        let name = p.weapon.clone().unwrap_or_default().to_lowercase();
        let heavy = if ["lmg", "shot", "snip", "m249", "pkm"]
            .iter()
            .any(|k| name.contains(k))
        {
            1.35
        } else {
            1.0
        };
        let phase = p.phase.unwrap_or(ReloadPhase::End);
        let voice = VoiceKind::Reload { phase, heavy };
        match p.position {
            Some(pos) => {
                self.play_at(voice, pos, PlayOpts::default(), Bus::Foley, 0.6);
            }
            None => {
                self.play_dry(voice, PlayOpts::default(), Bus::Foley, 0.22);
            }
        }
    }

    /// `_onShell` (`index.js:588-608`).
    pub fn on_shell(&mut self, p: &WeaponShell) {
        if !self.running {
            return;
        }
        // The source writes `if (this._budget.shell++ > 2) return;` — a *post*
        // increment, so the comparison sees the old count and three casings get
        // through. Incrementing first and comparing against one more admits
        // exactly the same three; every budget below reads the same way.
        self.budget_shell += 1;
        if self.budget_shell > 3 {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let at = p.position.unwrap_or(lp);
        let dist = self
            .live
            .as_ref()
            .map_or(0.0, |l| l.field.distance_to(at[0], at[1], at[2]));
        if dist > 22.0 {
            return;
        }
        // Find what it will land on, so brass on sand does not ring like concrete.
        // Note the source calls `phys.raycast(x, y + 0.2, z, 0, -1, 0, 6, mask)`
        // here — eight scalars — while `spatial.js` calls the same duck-typed
        // method as `raycast(origin, dir, len, mask)`. One of the two is wrong
        // against whatever `physics` will actually expose; the port uses the
        // vector form everywhere, which is the one the occlusion path (the
        // heavily-exercised one) uses.
        let surface = self
            .probe
            .as_deref()
            .and_then(|phys| {
                phys.raycast(
                    [at[0], at[1] + 0.2, at[2]],
                    [0.0, -1.0, 0.0],
                    6.0,
                    RayMask::World,
                )
            })
            .map_or(Surface::Concrete, |h| h.surface);
        let flight = 0.25 + self.rng.range(0.0, 0.3);
        self.play_at(
            VoiceKind::Shell {
                surface,
                level: clamp(1.0 - dist * 0.02, 0.3, 1.0),
                flight: Some(flight),
            },
            [at[0], at[1] - 0.6, at[2]],
            PlayOpts::default(),
            Bus::Foley,
            0.25,
        );
    }

    /// `_onImpact` (`index.js:610-626`).
    ///
    /// Note what the source does *not* do: the crack-past whizz this raises is
    /// not charged to the whizz budget, which caps `bullet:tracer` alone. Five
    /// impacts inside 6 m therefore make ten voices.
    pub fn on_impact(&mut self, p: &BulletImpact) {
        if !self.running {
            return;
        }
        if p.exit {
            return; // only the entry side gets a sound
        }
        self.budget_impact += 1;
        if self.budget_impact > 5 {
            return;
        }
        let Some(pt) = p.point else { return };
        let dist = self
            .live
            .as_ref()
            .map_or(0.0, |l| l.field.distance_to(pt[0], pt[1], pt[2]));
        if dist > 90.0 {
            return;
        }
        self.play_at(
            VoiceKind::Impact {
                surface: p.surface.unwrap_or(Surface::Concrete),
                energy: clamp(p.damage.unwrap_or(30.0) / 34.0, 0.35, 1.5),
            },
            pt,
            PlayOpts::default(),
            Bus::Foley,
            0.55,
        );
        // The round cracking past you is a separate sound from the impact itself.
        if dist < 6.0 {
            self.play_at(
                VoiceKind::Whizz {
                    miss: dist,
                    gain: 1.0,
                },
                pt,
                PlayOpts {
                    no_delay: true,
                    ..PlayOpts::default()
                },
                Bus::Foley,
                0.7,
            );
        }
    }

    /// `_onTracer` (`index.js:628-644`).
    pub fn on_tracer(&mut self, p: &BulletTracer) {
        if !self.running {
            return;
        }
        self.budget_whizz += 1;
        if self.budget_whizz > 3 {
            return;
        }
        // Closest approach of the trajectory to the listener.
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let a = p.from;
        let d = [p.to[0] - a[0], p.to[1] - a[1], p.to[2] - a[2]];
        let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if len2 < 1e-6 {
            return;
        }
        let t = clamp(
            ((lp[0] - a[0]) * d[0] + (lp[1] - a[1]) * d[1] + (lp[2] - a[2]) * d[2]) / len2,
            0.0,
            1.0,
        );
        let c = [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t];
        let miss = ((lp[0] - c[0]).powi(2) + (lp[1] - c[1]).powi(2) + (lp[2] - c[2]).powi(2)).sqrt();
        if miss > 5.0 {
            return;
        }
        let from_ear =
            ((lp[0] - a[0]).powi(2) + (lp[1] - a[1]).powi(2) + (lp[2] - a[2]).powi(2)).sqrt();
        if from_ear < 3.0 {
            return; // our own muzzle
        }
        let flight = (len2.sqrt() * t) / p.speed.unwrap_or(850.0);
        self.play_at(
            VoiceKind::Whizz { miss, gain: 1.0 },
            c,
            PlayOpts {
                no_delay: true,
                extra_delay: flight,
                ..PlayOpts::default()
            },
            Bus::Foley,
            0.75,
        );
    }

    /// `_onExplosion` (`index.js:646-657`).
    pub fn on_explosion(&mut self, p: &ExplosionEvent) {
        if !self.running {
            return;
        }
        let pos = p.position;
        let dist = self
            .live
            .as_ref()
            .map_or(0.0, |l| l.field.distance_to(pos[0], pos[1], pos[2]));
        self.play_at(
            VoiceKind::Explosion {
                radius: p.radius.unwrap_or(6.0),
                level: 1.0,
            },
            pos,
            PlayOpts {
                send: Some(1.0),
                ..PlayOpts::default()
            },
            Bus::Weapons,
            1.0,
        );
        if let Some(live) = self.live.as_mut() {
            live.mixer.duck(&mut live.graph, 0.85, 0.35);
        }
        // Concussion: total inside ~4 m, nothing past ~22 m.
        let near = clamp(1.0 - dist / 22.0, 0.0, 1.0);
        if near > 0.1 {
            let level = near.powf(1.4);
            if let Some(live) = self.live.as_mut() {
                live.mixer.concuss(&mut live.graph, level);
            }
        }
    }

    /// `_onFootstep` (`index.js:659-672`).
    pub fn on_footstep(&mut self, p: &PlayerFootstep) {
        if !self.running {
            return;
        }
        self.budget_step += 1;
        if self.budget_step > 4 {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let at = p
            .position
            .unwrap_or([lp[0], lp[1] - 1.6, lp[2]]);
        let dist = self
            .live
            .as_ref()
            .map_or(0.0, |l| l.field.distance_to(at[0], at[1], at[2]));
        if dist > 45.0 {
            return;
        }
        self.play_at(
            VoiceKind::Step(StepOpts {
                when: None,
                surface: p.surface.unwrap_or(Surface::Concrete),
                gait: p.gait.unwrap_or(Gait::Walk),
                level: p
                    .level
                    .unwrap_or(if dist < 2.0 { 0.72 } else { 1.0 }),
                gear: None,
            }),
            at,
            PlayOpts::default(),
            Bus::Foley,
            0.4,
        );
    }

    /// `_onLand` (`index.js:674-683`).
    pub fn on_land(&mut self, p: &PlayerLand) {
        if !self.running {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let v = p.velocity.unwrap_or(4.0).abs();
        self.play_at(
            VoiceKind::Step(StepOpts {
                when: None,
                surface: p.surface.unwrap_or(Surface::Concrete),
                gait: Gait::Land,
                level: clamp(v / 7.0, 0.35, 1.7),
                gear: Some(1.0),
            }),
            [lp[0], lp[1] - 1.6, lp[2]],
            PlayOpts::default(),
            Bus::Foley,
            0.7,
        );
        if v > 8.5 {
            self.play_dry(
                VoiceKind::Cloth { level: 0.8 },
                PlayOpts::default(),
                Bus::Foley,
                0.15,
            );
        }
    }

    /// `_onPlayerState` (`index.js:685-695`).
    pub fn on_player_state(&mut self, p: &PlayerState) {
        if !self.running {
            return;
        }
        if p.stance.is_some() && p.stance != self.stance {
            self.stance = p.stance.clone();
            self.play_dry(
                VoiceKind::Cloth { level: 0.9 },
                PlayOpts::default(),
                Bus::Foley,
                0.12,
            );
        }
        if let Some(ads) = p.ads {
            if ads != self.ads {
                self.ads = ads;
                self.play_dry(
                    VoiceKind::Cloth { level: 0.45 },
                    PlayOpts::default(),
                    Bus::Foley,
                    0.1,
                );
            }
        }
    }

    /// `_onDamageDealt` (`index.js:697-709`).
    ///
    /// "Damage dealt TO p.target" — `ai` also uses this for rounds that hit the
    /// player, and a hitmarker tick for being shot at is backwards. Incoming
    /// damage is handled by [`AudioCore::on_damage_taken`].
    pub fn on_damage_dealt(&mut self, p: &DamageDealt) {
        if !self.running {
            return;
        }
        if p.target_is_player {
            return;
        }
        self.ui(
            if p.headshot {
                UiSound::Headshot
            } else {
                UiSound::Hitmarker
            },
            1.0,
        );
        if p.killed {
            self.ui(UiSound::Kill, 1.0);
        } else if p.point.is_some() && p.has_target && self.rng.float() < 0.3 {
            self.bark(BarkRequest::Hurt, p.point, 0.85, false, 0, false);
        }
    }

    /// `_onDamageTaken` (`index.js:711-718`).
    pub fn on_damage_taken(&mut self, p: &DamageTaken) {
        if !self.running {
            return;
        }
        if let Some(h) = p.health {
            self.health = h;
        }
        self.ui(
            UiSound::Damage,
            clamp(p.amount.unwrap_or(20.0) / 25.0, 0.4, 1.4),
        );
        if p.amount.unwrap_or(0.0) > 12.0 && self.rng.float() < 0.5 {
            self.play_dry(
                VoiceKind::Bark(BarkOpts {
                    bark: Bark::Hit,
                    level: 0.5,
                    f0: Some(108.0),
                    ..BarkOpts::default()
                }),
                PlayOpts::default(),
                Bus::Voice,
                0.1,
            );
        }
    }

    /// `_onDeath` (`index.js:720-728`).
    pub fn on_death(&mut self, p: &ActorDeath) {
        if !self.running {
            return;
        }
        let Some(pt) = p.point else { return };
        self.bark(BarkRequest::Death, Some(pt), 1.0, false, p.actor_id, true);
        let extra_delay = 0.45 + self.rng.range(0.0, 0.4);
        self.play_at(
            VoiceKind::BodyFall { level: 1.0 },
            pt,
            PlayOpts {
                extra_delay,
                ..PlayOpts::default()
            },
            Bus::Foley,
            0.6,
        );
    }

    /* ================================================================ */
    /* ambience callbacks                                               */
    /* ================================================================ */

    /// A burst of gunfire a long way off, with correct propagation delay
    /// (`index.js:735-754`).
    fn distant_volley(&mut self) {
        if !self.running {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let a = self.rng.range(0.0, std::f64::consts::PI * 2.0);
        let d = self.rng.range(70.0, 240.0);
        let x = lp[0] + a.cos() * d;
        let z = lp[2] + a.sin() * d;
        let y = lp[1] + self.rng.range(-2.0, 6.0);
        let pool = [
            &crate::audio::weapons::AK,
            &RIFLE,
            &crate::audio::weapons::LMG,
            &crate::audio::weapons::SNIPER,
        ];
        let profile: &'static WeaponProfile = *self.rng.pick(&pool);
        let rounds = 1 + self.rng.u32() % 6;
        let rate = self.rng.range(0.075, 0.13);
        for i in 0..rounds {
            let jitter = self.rng.range(0.9, 1.1);
            self.play_at(
                VoiceKind::Shot {
                    profile,
                    first_person: false,
                },
                [x, y, z],
                PlayOpts {
                    extra_delay: f64::from(i) * rate * jitter,
                    max_dist: 400.0,
                    gain: 4.5,
                    // It is over the rooftops, not through them.
                    occlusion: Some(0.0),
                    ..PlayOpts::default()
                },
                Bus::Weapons,
                0.2,
            );
        }
    }

    /// `_distantBoom` (`index.js:756-765`).
    fn distant_boom(&mut self) {
        if !self.running {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let a = self.rng.range(0.0, std::f64::consts::PI * 2.0);
        let d = self.rng.range(120.0, 330.0);
        let y = lp[1] + self.rng.range(0.0, 8.0);
        let radius = self.rng.range(6.0, 16.0);
        self.play_at(
            VoiceKind::Explosion { radius, level: 1.0 },
            [lp[0] + a.cos() * d, y, lp[2] + a.sin() * d],
            PlayOpts {
                max_dist: 400.0,
                occlusion: Some(0.0),
                gain: 6.0,
                ..PlayOpts::default()
            },
            Bus::Weapons,
            0.25,
        );
    }

    /// `_ambientOneShot` (`index.js:767-784`).
    fn ambient_one_shot(&mut self) {
        if !self.running {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let which = *self.rng.pick(&OneShot::ALL);
        let far = which == OneShot::Heli || which == OneShot::Siren;
        let d = if far {
            self.rng.range(90.0, 260.0)
        } else {
            self.rng.range(14.0, 90.0)
        };
        let a = self.rng.range(0.0, std::f64::consts::PI * 2.0);
        let y = lp[1] + self.rng.range(-1.0, if which == OneShot::Heli { 28.0 } else { 5.0 });
        let level = self.rng.range(0.55, 1.0);
        self.play_at(
            VoiceKind::Ambient { which, level },
            [lp[0] + a.cos() * d, y, lp[2] + a.sin() * d],
            PlayOpts {
                max_dist: 400.0,
                occlusion: if far { Some(0.0) } else { None },
                gain: if far { 14.0 } else { 2.5 },
                ..PlayOpts::default()
            },
            Bus::Ambience,
            0.15,
        );
    }

    /// `_distantChatter` (`index.js:786-795`).
    fn distant_chatter(&mut self) {
        if !self.running {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let a = self.rng.range(0.0, std::f64::consts::PI * 2.0);
        let d = self.rng.range(25.0, 75.0);
        let pool = [
            BarkRequest::Advance,
            BarkRequest::Flank,
            BarkRequest::Copy,
            BarkRequest::Spot,
        ];
        let which = *self.rng.pick(&pool);
        let voice = i64::from(self.rng.int(0, 9));
        self.bark(
            which,
            Some([lp[0] + a.cos() * d, lp[1], lp[2] + a.sin() * d]),
            0.85,
            false,
            voice,
            false,
        );
    }

    /* ================================================================ */
    /* debug                                                            */
    /* ================================================================ */

    /// Snapshot for the dev overlay and the probe script (`index.js:848-867`).
    pub fn report(&self) -> AudioReport {
        AudioReport {
            running: self.running,
            sample_rate: self.live.as_ref().map_or(0.0, |l| l.graph.sample_rate()),
            voices: self.live.as_ref().map_or(0, |l| l.field.stats.active),
            dropped: self.live.as_ref().map_or(0, |l| l.field.stats.dropped),
            stolen: self.live.as_ref().map_or(0, |l| l.field.stats.stolen),
            occlusion_rays: self
                .live
                .as_ref()
                .map_or(0, |l| l.field.stats.occlusion_rays),
            space: self.stats.space,
            space_weights: self
                .live
                .as_ref()
                .map_or(SpaceWeights::default(), |l| l.mixer.space_weights),
            enclosure: self.space.enclosure,
            mean_free: self.space.mean_free,
            deafness: self.deafness,
            limiter_reduction: self.live.as_ref().map_or(0.0, |l| l.mixer.reduction()),
            events: self.stats.events,
        }
    }

    /// Fire one of everything (`index.js:806-845`). Used by the source's
    /// `probe.mjs` to prove the live graph runs without throwing; here it is
    /// also how a test proves the same about the recorded one.
    pub fn debug_storm(&mut self) {
        if !self.running {
            return;
        }
        let lp = self
            .live
            .as_ref()
            .map_or([0.0; 3], |l| l.field.listener_pos());
        let at = |dx: f64, dy: f64, dz: f64| [lp[0] + dx, lp[1] + dy, lp[2] + dz];

        for (weapon, origin) in [
            ("rifle", at(0.2, -0.1, -0.3)),
            ("ak", at(14.0, 0.0, -22.0)),
            ("sniper", at(-70.0, 3.0, 90.0)),
            ("shotgun", at(3.0, 0.0, -4.0)),
        ] {
            self.on_fire(&WeaponFire {
                weapon: Some(weapon.to_string()),
                origin: Some(origin),
                ..WeaponFire::default()
            });
        }
        self.on_fire(&WeaponFire {
            weapon: Some("mp5".to_string()),
            suppressed: true,
            origin: Some(at(-2.0, 0.0, -3.0)),
            ..WeaponFire::default()
        });
        self.on_fire(&WeaponFire {
            weapon: Some("rifle".to_string()),
            empty: true,
            origin: Some(at(0.2, -0.1, -0.3)),
            ..WeaponFire::default()
        });

        for s in Surface::ALL {
            let point = at(
                self.rng.range(-6.0, 6.0),
                self.rng.range(0.0, 2.0),
                self.rng.range(-8.0, -2.0),
            );
            self.on_impact(&BulletImpact {
                point: Some(point),
                surface: Some(s),
                damage: Some(32.0),
                exit: false,
            });
            self.on_footstep(&PlayerFootstep {
                position: Some(at(0.0, -1.6, 0.0)),
                surface: Some(s),
                gait: Some(Gait::Run),
                level: None,
            });
        }
        self.on_shell(&WeaponShell {
            position: Some(at(0.3, -0.2, -0.2)),
        });
        for ph in [
            ReloadPhase::Start,
            ReloadPhase::MagOut,
            ReloadPhase::MagIn,
            ReloadPhase::End,
        ] {
            self.on_reload(&WeaponReload {
                weapon: Some("rifle".to_string()),
                phase: Some(ph),
                position: None,
            });
        }
        self.on_tracer(&BulletTracer {
            from: at(-30.0, 0.0, -30.0),
            to: at(2.0, 0.0, 2.0),
            speed: Some(880.0),
        });
        self.on_land(&PlayerLand {
            velocity: Some(9.0),
            surface: Some(Surface::Concrete),
        });
        self.on_player_state(&PlayerState {
            stance: Some("crouch".to_string()),
            ads: Some(true),
        });
        self.on_damage_dealt(&DamageDealt {
            target_is_player: false,
            has_target: true,
            headshot: true,
            killed: false,
            point: Some(at(4.0, 0.0, -9.0)),
        });
        self.on_damage_taken(&DamageTaken {
            amount: Some(28.0),
            health: Some(24.0),
        });
        self.on_death(&ActorDeath {
            point: Some(at(4.0, -1.2, -9.0)),
            actor_id: 3,
        });
        for k in [
            BarkRequest::Spot,
            BarkRequest::Reload,
            BarkRequest::Grenade,
            BarkRequest::Flank,
            BarkRequest::Suppress,
            BarkRequest::Advance,
            BarkRequest::Hurt,
            BarkRequest::Copy,
        ] {
            let pos = at(self.rng.range(-9.0, 9.0), 0.0, self.rng.range(-9.0, 9.0));
            let voice = i64::from(self.rng.int(0, 9));
            self.bark(k, Some(pos), 1.0, false, voice, true);
        }
        for w in OneShot::ALL {
            self.play_at(
                VoiceKind::Ambient {
                    which: w,
                    level: 0.6,
                },
                [lp[0] + 20.0, lp[1] + 2.0, lp[2] - 20.0],
                PlayOpts::default(),
                Bus::Ambience,
                0.1,
            );
        }
        self.distant_volley();
        self.distant_boom();
        self.distant_chatter();
        self.on_explosion(&ExplosionEvent {
            position: at(6.0, 0.0, -7.0),
            radius: Some(8.0),
        });
    }
}

/* ================================================================ */
/* The Subsystem wrapper                                            */
/* ================================================================ */

/// The registered subsystem. `static id = 'audio'`, `static deps = []`.
pub struct AudioSystem {
    core: Rc<RefCell<AudioCore>>,
    offs: Vec<(&'static str, SubscriptionId)>,
}

impl AudioSystem {
    pub fn new(rng: Rng) -> Self {
        AudioSystem {
            core: Rc::new(RefCell::new(AudioCore::new(rng))),
            offs: Vec::new(),
        }
    }

    /// The shared guts — what `window.__AUDIO__` is in the source
    /// (`index.js:144`), as a handle rather than a global.
    pub fn core(&self) -> Rc<RefCell<AudioCore>> {
        Rc::clone(&self.core)
    }

    /// `_wireEvents(ctx)` (`index.js:516-534`).
    pub fn wire_events(&mut self, ctx: &Ctx<'_>) {
        macro_rules! on {
            ($name:literal, $payload:ty, $method:ident) => {{
                let core = Rc::clone(&self.core);
                let id = ctx.events.on($name, move |p: &dyn Any| {
                    if let Some(p) = p.downcast_ref::<$payload>() {
                        core.borrow_mut().$method(p);
                    }
                    Ok(())
                });
                self.offs.push(($name, id));
            }};
        }
        on!("weapon:fire", WeaponFire, on_fire);
        on!("weapon:reload", WeaponReload, on_reload);
        on!("weapon:shell", WeaponShell, on_shell);
        on!("bullet:impact", BulletImpact, on_impact);
        on!("bullet:tracer", BulletTracer, on_tracer);
        on!("explosion", ExplosionEvent, on_explosion);
        on!("player:footstep", PlayerFootstep, on_footstep);
        on!("player:land", PlayerLand, on_land);
        on!("player:state", PlayerState, on_player_state);
        on!("damage:dealt", DamageDealt, on_damage_dealt);
        on!("damage:taken", DamageTaken, on_damage_taken);
        on!("actor:death", ActorDeath, on_death);
        // Optional: emitted by `ai` if it wants scripted chatter.
        {
            let core = Rc::clone(&self.core);
            let id = ctx.events.on("ai:bark", move |p: &dyn Any| {
                if let Some(p) = p.downcast_ref::<AiBark>() {
                    core.borrow_mut().bark(
                        p.kind.unwrap_or(BarkRequest::Spot),
                        p.position,
                        1.0,
                        false,
                        p.voice,
                        false,
                    );
                }
                Ok(())
            });
            self.offs.push(("ai:bark", id));
        }
    }
}

impl Subsystem for AudioSystem {
    fn id(&self) -> &'static str {
        "audio"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Update]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), crate::error::CoreError> {
        self.wire_events(ctx);
        Ok(())
    }

    fn update(&mut self, dt: Seconds, _ctx: &Ctx<'_>) {
        self.core.borrow_mut().update(f64::from(dt.get()));
    }

    fn dispose(&mut self) {
        self.offs.clear();
        self.core.borrow_mut().dispose();
    }
}
