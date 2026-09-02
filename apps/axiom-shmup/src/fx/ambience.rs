//! Ported from Claude-of-Duty `src/fx/ambience.js:1-340` — the whole file.
//!
//! Always-on atmosphere: the last unported file of the source's `src/fx/`
//! tree. Three things, all subtle enough that you notice them only when they
//! are gone:
//!
//! * **Dust motes** ([`Ambience::motes`]) — a population of tiny
//!   forward-scattering specks kept alive in a box that follows the camera.
//! * **Heat shimmer** ([`Ambience::shimmer`]) — refraction sprites laid on
//!   the ground ahead of the player while the sun is high.
//! * **Smoke sources** ([`Ambience::add_column`]/[`Ambience::add_source`]) —
//!   long-lived emitters. `world` can tag any object with
//!   `userData.fxSmoke = { radius, rate }` and it starts smoking without
//!   either subsystem knowing about the other; explosions use the same pool
//!   for their smoke column.
//!
//! ## How this hangs off [`FxSystem`]
//!
//! The source holds it as `fx.ambience` (`index.js:121`) and drives it from
//! `FxSystem.update` (`index.js:786-787`), and so does this port — same
//! construction point (right after `ShellSystem::new`), same drive point (last,
//! after `_runScript`).
//!
//! One shape differs, and it has to. The source calls
//! `this.ambience.update( this, … )` — the ambience gets a mutable reference to
//! its own owner, because every spawner here draws off `fx.rng` and the draw
//! order IS the behaviour. Rust will not alias that, so `FxSystem` holds the
//! field as an `Option` and vacates it for the duration of its own update. That
//! is the honest encoding of the source: while the ambience is running, it is
//! not reachable through the system.
//!
//! ## Determinism: this file is nothing but draw order
//!
//! Every visible thing here is seeded scatter off [`FxSystem::rng`]. There is
//! no `fork()` and no literal seed in `ambience.js`; **the constructor spends
//! no RNG at all** (golden-pinned: `construction[*].rngBefore ==
//! rngAfter`), which closes the divergence [`crate::fx::system`]'s module doc
//! flagged as "the one place this port's RNG stream can diverge from the real
//! game's". It cannot: `new Ambience(...)` is pure state initialisation.
//!
//! Each spawner's draw sequence is commented against its source line below,
//! and `tests/ambience_port.rs` ends every block on an exact, zero-tolerance
//! `rng.float()`.
//!
//! ## Two source defects, ported faithfully and pinned by name
//!
//! 1. **The `resetSpawn()` aliasing bug** (`ambience.js:170-172`).
//!    `resetSpawn()` returns the *single module-level* `SP` object
//!    (`particles.js:56-71`), so `_puff`'s second `resetSpawn()` for the
//!    ember spark **resets the same object** the smoke puff was just built
//!    in. `t.x = s.x` and `t.z = s.z` therefore read the freshly-zeroed
//!    fields, not the puff's position: every ember in the game flies up from
//!    world `x = 0, z = 0`. Only `t.y = e.y` (which reads the *emitter*, a
//!    different object) survives. This port's [`reset_spawn`] returns a fresh
//!    value, so a literal transcription would silently *fix* the bug and
//!    diverge — see [`Ambience::puff`] for the explicit assignment that keeps
//!    it. `ambience.js` is the only file in `src/fx/` where two spawn
//!    descriptors are live at once, so it is the only site this defect can
//!    occur at.
//!
//! 2. **The dead mote-delay branch** (`ambience.js:236`). `_warm` saturates
//!    at 2 (it is only incremented under `if (this._warm < 2)`) and the
//!    ternary tests `this._warm <= 2`, so the condition is **always true**
//!    and the `: -rng.float() * dt` arm is unreachable. Every mote — warm
//!    fill and steady-state trickle alike — gets its delay spread across
//!    `-life * 0.95`, not across one frame. Both arms draw exactly one float,
//!    so the RNG stream is unaffected; only the value differs. Kept, with the
//!    dead arm written out, because dead computation in the source is still
//!    part of the source.
//!
//! ## The two seams
//!
//! * **The camera** is a [`CameraFrame`] (the same matrix pair
//!   [`crate::fx::system`] already takes), not a `THREE.PerspectiveCamera`.
//!   `camera.getWorldDirection(v)` is [`camera_world_direction`].
//! * **The scene graph** is [`AmbienceScene`]. `_scan`'s `scene.traverse`,
//!   `e.object.parent` and `setFromMatrixPosition(o.matrixWorld)` are the
//!   only three things `ambience.js` asks of a `THREE.Object3D`, and that
//!   trait names exactly those three. Same precedent as
//!   [`crate::fx::world::FxWorld`].

use std::collections::HashSet;

use crate::fx::atlas::p;
use crate::fx::particles::reset_spawn;
use crate::fx::system::{CameraFrame, FxSystem, SmokeColumnOpts};
use crate::fx::util::cone;
use crate::weapons::rig_math::{M4, V3};

/// `TWO_PI`, `ambience.js:24`.
const TWO_PI: f64 = std::f64::consts::PI * 2.0;

/// `MAX_EMITTERS`, `ambience.js:25`.
pub const MAX_EMITTERS: usize = 24;

/* ==================================================================== */
/* The scene-graph seam                                                 */
/* ==================================================================== */

/// Identity of one `THREE.Object3D` the scene handed out. The source keys
/// `_tracked` on object identity (`Set<Object3D>`); this is that identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

/// One object's `userData.fxSmoke`, `ambience.js:299`. Every field is
/// optional because `_scan` applies its own `??` default to each
/// (`ambience.js:304-310`) — and those defaults are **not** the ones
/// [`Ambience::add_source`] would apply (`rate` 4 vs 4.5, `ember` 0.2 vs
/// 0.25, `haze` 0.3 vs 0.35), so the distinction is load-bearing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SmokeTag {
    pub radius: Option<f64>,
    pub rate: Option<f64>,
    pub rise: Option<f64>,
    pub dark: Option<f64>,
    pub life: Option<f64>,
    pub ember: Option<f64>,
    pub haze: Option<f64>,
}

/// Everything `ambience.js` asks of the `THREE` scene graph — three calls,
/// named once. See the module doc.
///
/// `Debug` is a supertrait so [`crate::fx::system::FxFrame`], a plain per-frame
/// value bag, can keep deriving it.
pub trait AmbienceScene: core::fmt::Debug {
    /// `scene.traverse((o) => { const cfg = o.userData?.fxSmoke; ... })`,
    /// `ambience.js:298-300`, reduced to the objects that carry the tag.
    /// **Traversal order is part of the contract** — it decides the order
    /// emitters are acquired in and therefore every subsequent draw — so an
    /// implementer must yield depth-first, parent before children, in
    /// `children` order, exactly as `Object3D.traverse` does.
    fn smoke_sources(&self) -> Vec<(ObjectId, SmokeTag)>;

    /// `!!o.parent` — `ambience.js:322`. `false` deactivates the emitter.
    fn attached(&self, object: ObjectId) -> bool;

    /// `o.updateWorldMatrix(true, false); tmp.setFromMatrixPosition(
    /// o.matrixWorld)` — `ambience.js:301-302, 327-328`.
    fn world_position(&self, object: ObjectId) -> (f64, f64, f64);
}

/* ==================================================================== */
/* Option bags                                                          */
/* ==================================================================== */

/// [`Ambience::add_column`]'s `o`, `ambience.js:84-102`. Every field is
/// `Option` because every one is read through `??`: collapsing them to plain
/// values would bake `add_column`'s defaults in at the call site and make
/// [`Ambience::add_source`]'s *different* defaults unreachable.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ColumnOpts {
    pub duration: Option<f64>,
    pub rate: Option<f64>,
    pub radius: Option<f64>,
    pub rise: Option<f64>,
    pub dark: Option<f64>,
    pub life: Option<f64>,
    pub growth: Option<f64>,
    pub ember: Option<f64>,
    pub haze: Option<f64>,
}

/// `explosions.js:193-201`'s bag, as [`crate::fx::system`] already types it.
/// Every one of the seven is supplied explicitly there, so none of
/// [`Ambience::add_column`]'s `??` defaults fires — and `ember`/`haze`, which
/// that bag has no field for, stay `0`.
impl From<SmokeColumnOpts> for ColumnOpts {
    fn from(o: SmokeColumnOpts) -> ColumnOpts {
        ColumnOpts {
            duration: Some(o.duration),
            rate: Some(o.rate),
            radius: Some(o.radius),
            rise: Some(o.rise),
            dark: Some(o.dark),
            life: Some(o.life),
            growth: Some(o.growth),
            ember: None,
            haze: None,
        }
    }
}

/// [`Ambience::add_source`]'s `o`, `ambience.js:105-120`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SourceOpts {
    pub duration: Option<f64>,
    pub rate: Option<f64>,
    pub radius: Option<f64>,
    pub rise: Option<f64>,
    pub dark: Option<f64>,
    pub life: Option<f64>,
    pub growth: Option<f64>,
    pub ember: Option<f64>,
    pub haze: Option<f64>,
    /// `o.object` — the `Object3D` this source follows.
    pub object: Option<ObjectId>,
}

/// `new Ambience(fx, opts)`'s `opts`, `ambience.js:48-49, 55, 58, 62`.
/// `index.js:121-124` passes `{ motes: mote, shimmer: budget >= 4000 }`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AmbienceInit {
    /// `opts.motes ?? 240`.
    pub motes: Option<f64>,
    /// `opts.box ?? 22` (`box` is a Rust keyword-adjacent name; renamed).
    pub box_size: Option<f64>,
    /// `opts.shimmer !== false` — note the strict compare: `None` is
    /// **enabled**, only an explicit `Some(false)` disables.
    pub shimmer: Option<bool>,
}

/* ==================================================================== */
/* Emitter                                                              */
/* ==================================================================== */

/// `class Emitter`, `ambience.js:27-45`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emitter {
    pub active: bool,
    pub age: f64,
    pub duration: f64,
    pub acc: f64,
    pub rate: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub radius: f64,
    pub rise: f64,
    pub dark: f64,
    pub life: f64,
    pub growth: f64,
    pub ember: f64,
    pub haze: f64,
    pub object: Option<ObjectId>,
    /// `0` on a never-used slot — and [`Ambience::remove`] matches on tag
    /// with no active check, so `remove(0)` clears every untouched slot.
    /// `_tag` starts at 1 so no issued tag is ever `0`.
    pub tag: u64,
}

/// `constructor()`, `ambience.js:28-44`.
impl Default for Emitter {
    fn default() -> Self {
        Emitter {
            active: false,
            age: 0.0,
            duration: f64::INFINITY,
            acc: 0.0,
            rate: 6.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            radius: 0.25,
            rise: 1.2,
            dark: 0.16,
            life: 2.6,
            growth: 3.0,
            ember: 0.0,
            haze: 0.0,
            object: None,
            tag: 0,
        }
    }
}

/* ==================================================================== */
/* The camera seam                                                      */
/* ==================================================================== */

/// `camera.getWorldDirection(v)` for a `THREE.Camera`.
///
/// `Object3D.getWorldDirection` is `set(e[8], e[9], e[10]).normalize_or_zero()` — the
/// **+Z column** of the column-major world matrix — and `Camera` overrides it
/// to `super.getWorldDirection(target).negate()` (`Camera.js:100-103`). The
/// negate happens **after** the normalize, and this reproduces that order.
///
/// It does *not* matter numerically, and an earlier version of this comment
/// claimed it did ("normalising first and negating second is not the same
/// rounding as negating a vector and normalising that"). IEEE-754 negation is
/// exact — it flips a sign bit — so the two orders are bit-identical. The
/// order is kept because it is the source's, not because reversing it would
/// drift. Left as a warning: the code is right, and the old justification
/// invited someone to "fix" it on a premise that was never true.
///
/// `normalize()` is `divideScalar(length() || 1)` — [`V3::normalize`] already
/// carries the `|| 1`.
pub fn camera_world_direction(m: M4) -> V3 {
    let n = V3::new(m.e[8], m.e[9], m.e[10]).normalize_or_zero();
    V3::new(-n.x, -n.y, -n.z)
}

/* ==================================================================== */
/* Ambience                                                             */
/* ==================================================================== */

/// `class Ambience`, `ambience.js:47-340`.
pub struct Ambience {
    pub emitters: Vec<Emitter>,
    /// `this._tag`, `ambience.js:52` — starts at 1.
    pub tag: u64,

    /// `this.moteCount`. Kept as `f64` because it is divided by `moteLife` to
    /// get a rate and compared against a float accumulator; every call site
    /// supplies an integer.
    pub mote_count: f64,
    /// `this.moteLife`, a constant `9` — not an option.
    pub mote_life: f64,
    pub mote_acc: f64,
    pub mote_box: f64,
    pub mote_enabled: bool,
    /// `this.sunFactor` — written by the owner every frame from
    /// `FxSystem._sunFactor` (`index.js:786`), never computed here.
    pub sun_factor: f64,

    pub shimmer_acc: f64,
    pub shimmer_enabled: bool,

    /// `this._scanTimer`, `ambience.js:65`.
    pub scan_timer: f64,
    /// `this._tracked` — a `Set` of already-discovered smoke objects. Never
    /// pruned in the source either: an object removed from the scene keeps
    /// its entry, so re-adding it never re-registers it.
    pub tracked: HashSet<ObjectId>,
    /// `this._warm`, `ambience.js:68`. Saturates at 2 — see the module doc's
    /// second defect.
    pub warm: u32,
}

impl Ambience {
    /// `constructor(fx, opts = {})`, `ambience.js:48-69`. **Spends no RNG**
    /// (golden-pinned), which is why it does not take one.
    pub fn new(opts: &AmbienceInit) -> Self {
        let mote_count = opts.motes.unwrap_or(240.0);
        Ambience {
            emitters: vec![Emitter::default(); MAX_EMITTERS],
            tag: 1,
            mote_count,
            mote_life: 9.0,
            mote_acc: 0.0,
            mote_box: opts.box_size.unwrap_or(22.0),
            mote_enabled: mote_count > 0.0,
            sun_factor: 1.0,
            shimmer_acc: 0.0,
            // `opts.shimmer !== false`: `undefined` is enabled.
            shimmer_enabled: opts.shimmer != Some(false),
            scan_timer: 0.0,
            tracked: HashSet::new(),
            warm: 0,
        }
    }

    /* ----------------------------------------------------------------- */
    /*  emitters                                                          */
    /* ----------------------------------------------------------------- */

    /// `_acquire()`, `ambience.js:75-82`. Returns the index of the first
    /// inactive emitter, else the one with the largest `age / duration`
    /// (which is `0` for every `duration: Infinity` source, so a pool full of
    /// persistent sources always recycles slot 0).
    fn acquire(&self) -> usize {
        let mut oldest: Option<usize> = None;
        for (i, e) in self.emitters.iter().enumerate() {
            if !e.active {
                return i;
            }
            let better = match oldest {
                None => true,
                Some(o) => e.age / e.duration > self.emitters[o].age / self.emitters[o].duration,
            };
            if better {
                oldest = Some(i);
            }
        }
        // `MAX_EMITTERS` is a non-zero constant, so the loop always ran and
        // `oldest` is always `Some`.
        oldest.unwrap_or(0)
    }

    /// Finite-duration smoke column (explosions, burning wreck).
    /// `addColumn(x, y, z, o = {})`, `ambience.js:85-102`. Returns the tag.
    pub fn add_column(&mut self, x: f64, y: f64, z: f64, o: &ColumnOpts) -> u64 {
        let i = self.acquire();
        let tag = self.tag;
        // `e.tag = this._tag++` — post-increment: the tag is the pre-value.
        self.tag += 1;
        let e = &mut self.emitters[i];
        e.active = true;
        e.age = 0.0;
        e.acc = 0.0;
        e.duration = o.duration.unwrap_or(1.5);
        e.rate = o.rate.unwrap_or(8.0);
        e.x = x;
        e.y = y;
        e.z = z;
        e.radius = o.radius.unwrap_or(0.5);
        e.rise = o.rise.unwrap_or(1.5);
        e.dark = o.dark.unwrap_or(0.14);
        e.life = o.life.unwrap_or(3.2);
        e.growth = o.growth.unwrap_or(3.0);
        e.ember = o.ember.unwrap_or(0.0);
        e.haze = o.haze.unwrap_or(0.0);
        e.object = None;
        e.tag = tag;
        tag
    }

    /// Persistent source; pass an object to have it follow that object.
    /// `addSource(position, o = {})`, `ambience.js:105-120`.
    ///
    /// Note it supplies **all nine** of [`add_column`](Ambience::add_column)'s
    /// options explicitly, so none of that method's own defaults fires here —
    /// in particular `duration` becomes `Infinity`, not `1.5`.
    pub fn add_source(&mut self, position: (f64, f64, f64), o: &SourceOpts) -> u64 {
        let tag = self.add_column(
            position.0,
            position.1,
            position.2,
            &ColumnOpts {
                duration: Some(o.duration.unwrap_or(f64::INFINITY)),
                rate: Some(o.rate.unwrap_or(4.5)),
                radius: Some(o.radius.unwrap_or(0.35)),
                rise: Some(o.rise.unwrap_or(1.1)),
                dark: Some(o.dark.unwrap_or(0.13)),
                life: Some(o.life.unwrap_or(3.4)),
                growth: Some(o.growth.unwrap_or(3.4)),
                ember: Some(o.ember.unwrap_or(0.25)),
                haze: Some(o.haze.unwrap_or(0.35)),
            },
        );
        if let Some(obj) = o.object {
            for e in self.emitters.iter_mut() {
                if e.tag == tag {
                    e.object = Some(obj);
                }
            }
        }
        tag
    }

    /// `remove(tag)`, `ambience.js:123-130`. Matches on tag alone with no
    /// active check and no early exit, so it hits every slot carrying that
    /// tag — including all 24 untouched slots when called with `0`.
    pub fn remove(&mut self, tag: u64) {
        for e in self.emitters.iter_mut() {
            if e.tag == tag {
                e.active = false;
                e.object = None;
            }
        }
    }

    /// `_puff(e, now, dt)`, `ambience.js:132-186`.
    ///
    /// The source's `now` parameter is never read: `fx.emitLit`/`fx.emitAdd`
    /// stamp the birth from `fx.now` (`index.js:216-217`), not from it.
    ///
    /// Draw order, all off `fx.rng`: `cone` (2) · spawn x/y/z (3) · vx/vy/vz
    /// (3) · tile (1) · size0/size1 (2) · life (1) · delay (1) · rot (1) ·
    /// spin (1) · alpha (1) · seed (1) = **17**; then, only when
    /// `e.ember > 0`, the ember roll (1) and — if it passes — 7 more; then,
    /// only when `e.haze > 0`, the haze roll (1) and — if it passes — the one
    /// `fx.haze` draws for its seed.
    fn puff(&mut self, fx: &mut FxSystem, index: usize, dt: f64) {
        let e = self.emitters[index];
        // `cone(V, rng, 0, 1, 0, 0.6, 0.7)` — `V.y` is written and never
        // read (the source takes `vy` from `e.rise` instead), but the two
        // draws are live.
        let v = cone(&mut fx.rng, 0.0, 1.0, 0.0, 0.6, 0.7);
        let mut s = reset_spawn();
        let r = e.radius;
        s.x = e.x + fx.rng.signed() * r * 0.6;
        s.y = e.y + fx.rng.range(0.0, r * 0.4);
        s.z = e.z + fx.rng.signed() * r * 0.6;
        s.vx = v.0 * e.rise * 0.5 + fx.rng.signed() * 0.25;
        s.vy = e.rise * fx.rng.range(0.7, 1.25);
        s.vz = v.2 * e.rise * 0.5 + fx.rng.signed() * 0.25;
        s.tile = (if fx.rng.float() < 0.5 { p::SMOKE_A } else { p::SMOKE_B }) as f64;
        s.size0 = r * fx.rng.range(0.7, 1.2);
        s.size1 = r * e.growth * fx.rng.range(0.8, 1.25);
        s.size_curve = 0.7;
        s.life = e.life * fx.rng.range(0.75, 1.25);
        s.delay = -fx.rng.float() * dt;
        s.drag = 0.75;
        s.gravity = 0.42; // buoyant, keeps accelerating upward
        s.rot = fx.rng.float() * TWO_PI;
        s.spin = fx.rng.signed() * 0.35;
        let d = e.dark;
        s.r0 = d;
        s.g0 = d * 0.97;
        s.b0 = d * 0.94;
        s.r1 = d * 1.9;
        s.g1 = d * 1.86;
        s.b1 = d * 1.8;
        s.alpha = fx.rng.range(0.3, 0.55);
        s.alpha_curve = 1.7;
        s.soft = 0.8;
        s.turb = r * 0.5;
        s.turb_freq = 0.55;
        s.seed = fx.rng.float();
        fx.emit_lit(&s);

        if e.ember > 0.0 && fx.rng.float() < e.ember {
            let mut t = reset_spawn();
            // ---- SOURCE DEFECT, ported deliberately ----------------------
            // `ambience.js:171` reads `t.x = s.x; t.y = e.y; t.z = s.z;`.
            // In the source `s` and `t` are THE SAME OBJECT (`resetSpawn()`
            // hands back the one module-level `SP`, `particles.js:56-71`), so
            // the `resetSpawn()` on the line above has already zeroed `s.x`
            // and `s.z`. Every ember spark therefore spawns at world
            // `x = 0, z = 0` — not above the smoke it came from. Only `t.y`
            // is right, because `e` is the emitter, a different object.
            // Golden-pinned by `ambience_port::ember_sparks_spawn_at_the_
            // world_origin_reset_spawn_aliasing`.
            t.x = 0.0; // `t.x = s.x`, and `s.x` has just been reset to 0
            t.y = e.y;
            t.z = 0.0; // `t.z = s.z`, likewise
            // --------------------------------------------------------------
            t.vx = fx.rng.signed() * 0.5;
            t.vy = fx.rng.range(1.2, 3.2);
            t.vz = fx.rng.signed() * 0.5;
            t.tile = p::SPARK as f64;
            t.size0 = fx.rng.range(0.006, 0.014);
            t.size1 = t.size0 * 0.5;
            t.life = fx.rng.range(0.8, 1.9);
            t.drag = 0.9;
            t.gravity = 1.2;
            t.r0 = 1.0;
            t.g0 = 0.5;
            t.b0 = 0.16;
            t.i0 = fx.rng.range(3.0, 9.0);
            t.r1 = 0.9;
            t.g1 = 0.14;
            t.b1 = 0.02;
            t.i1 = 0.1;
            t.flags = 1.0;
            t.alpha_curve = 1.2;
            t.turb = 0.12;
            t.turb_freq = 1.6;
            t.soft = 0.1;
            t.seed = fx.rng.float();
            fx.emit_add(&t);
        }
        if e.haze > 0.0 && fx.rng.float() < 0.35 {
            fx.haze(e.x, e.y + r * 0.6, e.z, r * 1.4, 2.4, 0.9, e.haze, p::SMOKE_A);
        }
    }

    /* ----------------------------------------------------------------- */
    /*  motes + shimmer                                                   */
    /* ----------------------------------------------------------------- */

    /// `_motes(dt, now, camera)`, `ambience.js:192-251`. `now` is unread in
    /// the source too.
    ///
    /// 14 draws per mote, in the order below. See the module doc for the
    /// dead `delay` arm.
    fn motes(&mut self, fx: &mut FxSystem, dt: f64, camera: &CameraFrame) {
        // Keep the population at `moteCount` by replacing what expires.
        let rate = self.mote_count / self.mote_life;
        // On the first couple of frames fill the volume in one go so the air
        // is never visibly empty when a shot is captured.
        let n = if self.warm < 2 {
            self.warm += 1;
            self.mote_count
        } else {
            self.mote_acc += rate * dt;
            let n = 64.0_f64.min(self.mote_acc.floor());
            self.mote_acc -= n;
            n
        };
        if n <= 0.0 {
            return;
        }
        let fwd = camera_world_direction(camera.matrix_world);
        let pos = camera.position();
        let cx = pos.x + fwd.x * self.mote_box * 0.22;
        let cy = pos.y + fwd.y * self.mote_box * 0.1;
        let cz = pos.z + fwd.z * self.mote_box * 0.22;
        let half = self.mote_box * 0.5;
        let bright = 0.16 * self.sun_factor;
        for _ in 0..(n as usize) {
            let mut s = reset_spawn();
            s.x = cx + fx.rng.signed() * half;
            s.y = cy + fx.rng.signed() * half * 0.42;
            s.z = cz + fx.rng.signed() * half;
            s.vx = fx.rng.signed() * 0.09;
            s.vy = fx.rng.range(-0.05, 0.06);
            s.vz = fx.rng.signed() * 0.09;
            s.tile = p::MOTE as f64;
            s.size0 = fx.rng.range(0.0035, 0.011);
            s.size1 = s.size0;
            s.life = self.mote_life * fx.rng.range(0.55, 1.45);
            // Spread the first fill through the lifetime so they do not all
            // die at once and pulse the whole volume. `self.warm <= 2` is
            // ALWAYS true (it saturates at 2) — the `else` arm is dead in the
            // source and is kept written out for exactly that reason.
            s.delay = if self.warm <= 2 {
                -fx.rng.float() * s.life * 0.95
            } else {
                -fx.rng.float() * dt
            };
            s.drag = 0.22;
            s.gravity = -0.02;
            let b = bright * fx.rng.range(0.35, 1.5);
            s.r0 = 1.0;
            s.g0 = 0.96;
            s.b0 = 0.9;
            s.i0 = b;
            s.r1 = 1.0;
            s.g1 = 0.94;
            s.b1 = 0.88;
            s.i1 = b * 0.6;
            s.alpha = fx.rng.range(0.25, 0.7);
            s.alpha_curve = 1.1;
            s.soft = 0.05;
            s.turb = fx.rng.range(0.05, 0.22);
            s.turb_freq = fx.rng.range(0.15, 0.5);
            s.seed = fx.rng.float();
            fx.emit_mote(&s);
        }
    }

    /// `_shimmer(dt, now, camera)`, `ambience.js:253-290`.
    ///
    /// Eight draws, and the ninth is the seed `fx.haze` itself takes
    /// (`index.js:612-614`). **JS evaluates call arguments left to right**,
    /// so the four `rng.range` calls and the tile coin-flip inside the
    /// `fx.haze(...)` argument list are drawn in written order, before the
    /// call — which is why they are hoisted into named locals here rather
    /// than left inline (Rust's evaluation order would agree, but only by
    /// coincidence of it also being left-to-right).
    fn shimmer(&mut self, fx: &mut FxSystem, dt: f64, camera: &CameraFrame) {
        if !self.shimmer_enabled || self.sun_factor < 0.35 {
            return;
        }
        self.shimmer_acc += dt;
        if self.shimmer_acc < 0.22 {
            return;
        }
        self.shimmer_acc = 0.0;
        let fwd = camera_world_direction(camera.matrix_world);
        let pos = camera.position();
        let d = fx.rng.range(3.5, 15.0);
        let sx = pos.x + fwd.x * d + fx.rng.signed() * 4.0;
        let sz = pos.z + fwd.z * d + fx.rng.signed() * 4.0;
        let mut gy = pos.y - 1.6;
        // `if (fx.physics?.groundHeight) { const h = ...; if
        // (Number.isFinite(h)) gy = h; }`. The `is_finite` check is kept even
        // though `FxWorld::ground_height` already returns `None` for a
        // non-finite probe: the guard is in the source at *this* site, and an
        // implementer that returns `Some(NaN)` must not move the sprite.
        if let Some(w) = fx.world.as_ref() {
            if let Some(h) = w.ground_height(sx, sz, pos.y + 6.0) {
                if h.is_finite() {
                    gy = h;
                }
            }
        }
        let arg_y = gy + fx.rng.range(0.15, 0.6);
        let arg_radius = fx.rng.range(0.5, 1.2);
        let arg_life = fx.rng.range(1.1, 2.0);
        let arg_strength = fx.rng.range(0.12, 0.3) * self.sun_factor;
        let arg_tile = if fx.rng.float() < 0.5 { p::SMOKE_A } else { p::MIST };
        fx.haze(sx, arg_y, sz, arg_radius, 1.9, arg_life, arg_strength, arg_tile);
    }

    /* ----------------------------------------------------------------- */

    /// Discover objects the world subsystem tagged as smoking.
    /// `_scan(scene)`, `ambience.js:295-313`.
    ///
    /// Each `??` here is applied **before** [`add_source`](Ambience::add_source)
    /// sees the value, so these defaults win over that method's — and three
    /// of them differ (`rate` 4 vs 4.5, `ember` 0.2 vs 0.25, `haze` 0.3 vs
    /// 0.35). `duration` and `growth` are *not* passed, so those do fall
    /// through to `add_source`'s `Infinity` and `3.4`.
    fn scan(&mut self, scene: &dyn AmbienceScene) {
        for (id, cfg) in scene.smoke_sources() {
            if self.tracked.contains(&id) {
                continue;
            }
            self.tracked.insert(id);
            let position = scene.world_position(id);
            self.add_source(
                position,
                &SourceOpts {
                    duration: None,
                    rate: Some(cfg.rate.unwrap_or(4.0)),
                    radius: Some(cfg.radius.unwrap_or(0.35)),
                    rise: Some(cfg.rise.unwrap_or(1.1)),
                    dark: Some(cfg.dark.unwrap_or(0.13)),
                    life: Some(cfg.life.unwrap_or(3.4)),
                    growth: None,
                    ember: Some(cfg.ember.unwrap_or(0.2)),
                    haze: Some(cfg.haze.unwrap_or(0.3)),
                    object: Some(id),
                },
            );
        }
    }

    /// `update(dt, now, camera, scene)`, `ambience.js:315-340`.
    ///
    /// `now` is unread in the source: every emit stamps its birth from
    /// `fx.now`, which the owner sets first (`index.js:785-787`). It is kept
    /// in the signature so the call shape matches.
    ///
    /// **Seam narrowing, stated plainly:** the source holds `e.object` as a
    /// direct `Object3D` reference, so following it does not need the
    /// `scene` argument; here it does. With `scene == None` an object-bound
    /// emitter keeps its last position instead of following or deactivating.
    /// The game always passes `ctx.scene`, so this only shows up in tests
    /// that deliberately pass `None`.
    pub fn update(
        &mut self,
        fx: &mut FxSystem,
        dt: f64,
        now: f64,
        camera: &CameraFrame,
        scene: Option<&dyn AmbienceScene>,
    ) {
        let _ = now;
        for i in 0..self.emitters.len() {
            if !self.emitters[i].active {
                continue;
            }
            self.emitters[i].age += dt;
            if self.emitters[i].age > self.emitters[i].duration {
                self.emitters[i].active = false;
                continue;
            }
            if let Some(obj) = self.emitters[i].object {
                if let Some(sc) = scene {
                    if !sc.attached(obj) {
                        self.emitters[i].active = false;
                        self.emitters[i].object = None;
                        continue;
                    }
                    let p = sc.world_position(obj);
                    self.emitters[i].x = p.0;
                    self.emitters[i].y = p.1;
                    self.emitters[i].z = p.2;
                }
            }
            self.emitters[i].acc += self.emitters[i].rate * dt;
            // `let guard = 8; while (e.acc >= 1 && guard-- > 0)` — at most 8
            // puffs per emitter per frame; the remainder stays in `acc`.
            let mut guard = 8i32;
            while self.emitters[i].acc >= 1.0 && guard > 0 {
                guard -= 1;
                self.emitters[i].acc -= 1.0;
                self.puff(fx, i, dt);
            }
        }

        if self.mote_enabled {
            self.motes(fx, dt, camera);
        }
        self.shimmer(fx, dt, camera);

        self.scan_timer += dt;
        // `if (this._scanTimer > 2 && scene)` — a falsy scene does NOT reset
        // the timer, so it keeps accumulating and the scan fires on the first
        // update that supplies one.
        if self.scan_timer > 2.0 {
            if let Some(sc) = scene {
                self.scan_timer = 0.0;
                self.scan(sc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scene fixture with no tagged objects — the `_scan` no-op case.
    #[derive(Debug)]
    struct EmptyScene;
    impl AmbienceScene for EmptyScene {
        fn smoke_sources(&self) -> Vec<(ObjectId, SmokeTag)> {
            Vec::new()
        }
        fn attached(&self, _object: ObjectId) -> bool {
            true
        }
        fn world_position(&self, _object: ObjectId) -> (f64, f64, f64) {
            (0.0, 0.0, 0.0)
        }
    }

    fn init(motes: f64, shimmer: bool) -> Ambience {
        Ambience::new(&AmbienceInit {
            motes: Some(motes),
            box_size: None,
            shimmer: Some(shimmer),
        })
    }

    #[test]
    fn constructor_defaults() {
        let a = Ambience::new(&AmbienceInit::default());
        assert_eq!(a.mote_count, 240.0);
        assert_eq!(a.mote_box, 22.0);
        assert_eq!(a.mote_life, 9.0);
        assert!(a.mote_enabled);
        // `opts.shimmer !== false`: absent means enabled.
        assert!(a.shimmer_enabled);
        assert_eq!(a.tag, 1);
        assert_eq!(a.emitters.len(), MAX_EMITTERS);
        assert_eq!(a.warm, 0);
    }

    #[test]
    fn shimmer_is_only_disabled_by_an_explicit_false() {
        assert!(Ambience::new(&AmbienceInit {
            shimmer: None,
            ..AmbienceInit::default()
        })
        .shimmer_enabled);
        assert!(Ambience::new(&AmbienceInit {
            shimmer: Some(true),
            ..AmbienceInit::default()
        })
        .shimmer_enabled);
        assert!(!init(0.0, false).shimmer_enabled);
    }

    #[test]
    fn zero_motes_disables_the_mote_pass() {
        assert!(!init(0.0, false).mote_enabled);
    }

    #[test]
    fn add_column_tags_start_at_one_and_increment() {
        let mut a = init(0.0, false);
        assert_eq!(a.add_column(0.0, 0.0, 0.0, &ColumnOpts::default()), 1);
        assert_eq!(a.add_column(0.0, 0.0, 0.0, &ColumnOpts::default()), 2);
        assert_eq!(a.tag, 3);
    }

    #[test]
    fn add_source_overrides_every_add_column_default() {
        let mut a = init(0.0, false);
        a.add_source((1.0, 2.0, 3.0), &SourceOpts::default());
        let e = a.emitters[0];
        assert!(e.duration.is_infinite(), "duration was {}", e.duration);
        assert_eq!(e.rate, 4.5);
        assert_eq!(e.radius, 0.35);
        assert_eq!(e.ember, 0.25);
        assert_eq!(e.haze, 0.35);
        assert_eq!((e.x, e.y, e.z), (1.0, 2.0, 3.0));
    }

    #[test]
    fn remove_zero_clears_every_untouched_slot() {
        let mut a = init(0.0, false);
        a.add_column(0.0, 0.0, 0.0, &ColumnOpts::default());
        a.remove(0);
        // Slot 0 has tag 1 and survives; every other slot still carries 0.
        assert!(a.emitters[0].active);
        assert!(!a.emitters[1].active);
    }

    #[test]
    fn acquire_recycles_the_largest_age_over_duration() {
        let mut a = init(0.0, false);
        for i in 0..MAX_EMITTERS {
            a.add_column(
                i as f64,
                0.0,
                0.0,
                &ColumnOpts {
                    duration: Some(10.0),
                    rate: Some(0.0),
                    ..ColumnOpts::default()
                },
            );
        }
        // Age slot 3 the most by hand, then overflow.
        a.emitters[3].age = 5.0;
        let tag = a.add_column(99.0, 0.0, 0.0, &ColumnOpts::default());
        assert_eq!(a.emitters[3].tag, tag);
        assert_eq!(a.emitters[3].x, 99.0);
    }

    #[test]
    fn smoke_column_opts_convert_with_every_field_supplied() {
        let c = ColumnOpts::from(SmokeColumnOpts {
            radius: 1.4,
            duration: 1.5,
            rate: 9.0,
            rise: 1.6,
            dark: 0.12,
            life: 3.4,
            growth: 3.2,
        });
        assert_eq!(c.radius, Some(1.4));
        assert_eq!(c.ember, None);
        assert_eq!(c.haze, None);
    }

    #[test]
    fn camera_world_direction_negates_after_normalising() {
        // Identity world matrix: the +Z column is (0,0,1), negated to (0,0,-1).
        let d = camera_world_direction(M4::IDENTITY);
        assert_eq!((d.x, d.y, d.z), (0.0, 0.0, -1.0));
    }

    #[test]
    fn a_falsy_scene_does_not_reset_the_scan_timer() {
        let mut fx = FxSystem::test_instance(7);
        let mut a = init(0.0, false);
        let cam = CameraFrame::default();
        for _ in 0..6 {
            a.update(&mut fx, 0.5, 0.0, &cam, None);
        }
        assert!(a.scan_timer >= 3.0, "scan_timer was {}", a.scan_timer);
        a.update(&mut fx, 0.5, 0.0, &cam, Some(&EmptyScene));
        assert_eq!(a.scan_timer, 0.0);
    }

    #[test]
    fn the_guard_caps_a_burst_at_eight_puffs() {
        let mut fx = FxSystem::test_instance(11);
        let mut a = init(0.0, false);
        a.add_column(
            0.0,
            0.0,
            0.0,
            &ColumnOpts {
                duration: Some(10.0),
                rate: Some(200.0),
                ember: Some(0.0),
                haze: Some(0.0),
                ..ColumnOpts::default()
            },
        );
        a.update(&mut fx, 0.1, 0.0, &CameraFrame::default(), None);
        assert_eq!(fx.lit.spawned(), 8);
        // 200 * 0.1 = 20 accumulated, 8 spent.
        assert!((a.emitters[0].acc - 12.0).abs() < 1e-9, "acc was {}", a.emitters[0].acc);
    }
}
