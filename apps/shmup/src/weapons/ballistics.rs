//! Projectile ballistics.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/weapons/ballistics.js:1-166` — the
//! whole file.
//!
//! Rounds are simulated, not hitscanned: each shot is a body with a muzzle
//! velocity, gravity and a drag term, stepped at the physics rate. A 9 mm round
//! takes 140 ms to cross a 50 m street and drops about 10 cm doing it, and you
//! can see the tracer travel. Terminal effects (penetration, spall, damage) are
//! handed to the physics binding at the moment of contact so wall penetration
//! and multi-layer hits stay in one place.
//!
//! ## The physics seam
//!
//! The source reaches two capabilities off `ctx.peek('physics')`:
//! `phys.raycast(origin, dir, maxDist, mask)` (the tracer cast and the per-step
//! hit test) and `phys.fireBullet({ … })` (the penetration solver — wall
//! penetration, multi-layer hits and damage application, all out of scope for
//! ballistics itself). Neither exists in this port yet: no physics crate/module
//! has landed. Rather than reach for a concrete type that does not exist,
//! [`ProjectileSim::spawn`] and [`ProjectileSim::fixed_update`] take an
//! `Option<&mut dyn RaycastWorld>` — the trait/callback seam the manifest asked
//! for. Whichever future physics capability lands implements [`RaycastWorld`];
//! everything above this boundary (the integration, the falloff maths, the pool
//! management) is already correct and untouched by that later change. Passing
//! `None` reproduces the source's `if (phys) { … }` guards exactly: with no
//! physics bound, rounds still fly and expire on range/age/altitude, they just
//! never hit anything.
//!
//! One simplification at the seam: the source threads `phys.MASK?.BULLET` — a
//! constant *physics itself* owns — through every `raycast` call. That table
//! does not exist in this port (physics is not ported), so
//! [`RaycastWorld::raycast`] drops the mask parameter; the trait's contract is
//! simply "a segment cast against whatever physics considers bullet-blocking
//! geometry", and the choice of which layers that means is the future physics
//! binding's problem, not ballistics'. The per-projectile `mask` field
//! (`p.mask`, threaded through to `fireBullet` only) is preserved, since that
//! one genuinely varies per shot.
//!
//! ## Precision
//!
//! Every scalar and vector component here is `f64`, matching a JavaScript
//! `number` exactly (never `f32`, unlike the kernel's `Meters`/`Seconds`): the
//! falloff curve and the integration step are pure `+ - * /`, so a caller can
//! golden-check them against values captured from the source with *exact*
//! equality rather than a tolerance.

use crate::events::EventBus;

/// `GRAVITY` (`ballistics.js:14`). Metres per second squared, negative = down.
pub const GRAVITY: f64 = -9.81;

/// `MAX_LIVE` (`ballistics.js:15`) — the pool size.
pub const MAX_LIVE: usize = 96;

/// A minimal position/direction vector.
///
/// Not in the source: `ballistics.js` uses `THREE.Vector3`, mutated in place
/// (`p.vel.multiplyScalar(decay)`, `p.pos.addScaledVector(...)`) to stay
/// allocation-free. This crate has no `THREE.Vector3` and no math layer of its
/// own to reach for (`axiom-kernel` has no vector type — see the port notes),
/// so a small `Copy` struct stands in. `Copy` means the "mutate in place"
/// style translates to "assign the result back", which reads the same at every
/// call site and costs nothing extra: `f64` triples are register-sized.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }

    pub fn sub(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    /// `THREE.Vector3.addScaledVector(v, s)`: `self + v * s`.
    pub fn add_scaled(self, v: Vec3, s: f64) -> Vec3 {
        Vec3::new(self.x + v.x * s, self.y + v.y * s, self.z + v.z * s)
    }

    pub fn scale(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// `THREE.Vector3.normalize()`: `this.divideScalar(this.length() || 1)` — a
    /// zero-length vector divides by 1 (stays zero) rather than producing NaN.
    pub fn normalize(self) -> Vec3 {
        let len = self.length();
        let divisor = if len == 0.0 { 1.0 } else { len };
        self.scale(1.0 / divisor)
    }
}

/// One in-flight (or pooled, inert) round. `ballistics.js:17-34`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projectile {
    pub alive: bool,
    pub pos: Vec3,
    pub prev: Vec3,
    pub vel: Vec3,
    pub dir: Vec3,
    pub damage: f64,
    pub penetration: f64,
    pub drag_k: f64,
    pub travelled: f64,
    pub max_range: f64,
    pub age: f64,
    pub dropoff: f64,
    /// `weapon` in the source is whatever the caller passed (a weapon def or
    /// id); this port's weapon vocabulary ([`crate::weapons::defs`]) keys
    /// weapons by a `&'static str` id, so that is what a projectile carries.
    pub weapon: Option<&'static str>,
    /// `undefined` in the source (`this.mask = undefined`); `None` here.
    pub mask: Option<u32>,
}

impl Default for Projectile {
    fn default() -> Self {
        Projectile {
            alive: false,
            pos: Vec3::ZERO,
            prev: Vec3::ZERO,
            vel: Vec3::ZERO,
            dir: Vec3::ZERO,
            damage: 30.0,
            penetration: 1.0,
            drag_k: 0.3,
            travelled: 0.0,
            max_range: 400.0,
            age: 0.0,
            dropoff: 0.5,
            weapon: None,
            mask: None,
        }
    }
}

/// `spawn(o)`'s parameter object, `ballistics.js:56-58` — origin, dir, and the
/// same `??` defaults the source applies at each field.
#[derive(Debug, Clone, Copy)]
pub struct SpawnParams {
    pub origin: Vec3,
    pub dir: Vec3,
    pub speed: f64,
    pub damage: f64,
    pub penetration: f64,
    pub drag_k: f64,
    pub dropoff: f64,
    pub max_range: f64,
    pub weapon: Option<&'static str>,
    pub mask: Option<u32>,
    pub tracer: bool,
}

impl Default for SpawnParams {
    /// The source's `o.speed ?? 800`, `o.damage ?? 30`, and so on — every
    /// nullish-coalesced default from `spawn`, `ballistics.js:59-92`, so a
    /// caller can build one with only `origin`/`dir` set and get the same
    /// projectile the source would from `{ origin, dir }`.
    fn default() -> Self {
        SpawnParams {
            origin: Vec3::ZERO,
            dir: Vec3::ZERO,
            speed: 800.0,
            damage: 30.0,
            penetration: 1.0,
            drag_k: 0.3,
            dropoff: 0.5,
            max_range: 400.0,
            weapon: None,
            mask: None,
            tracer: false,
        }
    }
}

/// The result of one segment raycast — `hit?.hit` truthy in the source, with
/// the distance the hit landed at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaycastHit {
    pub distance: f64,
}

/// A confirmed impact, handed to the penetration solver. `fireBullet`'s
/// argument object, `ballistics.js:133-141`.
#[derive(Debug, Clone, Copy)]
pub struct FireBulletRequest {
    pub origin: Vec3,
    pub dir: Vec3,
    pub max_dist: f64,
    pub damage: f64,
    pub penetration: f64,
    pub dropoff: f64,
    pub mask: Option<u32>,
}

/// The physics seam. See the module docs for why this is a trait rather than a
/// concrete `axiom-physics` dependency.
pub trait RaycastWorld {
    /// `phys.raycast(origin, dir, maxDist, phys.MASK?.BULLET)`. `None` is the
    /// source's `!hit?.hit`.
    fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f64) -> Option<RaycastHit>;

    /// `phys.fireBullet({ … })`.
    fn fire_bullet(&mut self, request: FireBulletRequest);
}

/// `bullet:tracer` event payload, `ballistics.js:46` (`_tracerPayload`) and
/// `:95-107` (`_emitTracer`).
#[derive(Debug, Clone, Copy)]
pub struct TracerEvent {
    pub from: Vec3,
    pub to: Vec3,
    pub speed: f64,
    pub weapon: Option<&'static str>,
}

/// `this.stats`, `ballistics.js:47`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BallisticsStats {
    pub fired: u32,
    pub impacts: u32,
    pub live: u32,
}

/// Range-normalised progress toward `max_range`, clamped to `[0, 1]`.
/// `ballistics.js:131`: `Math.min(1, p.travelled / p.maxRange)`.
pub fn range01(travelled: f64, max_range: f64) -> f64 {
    (travelled / max_range).min(1.0)
}

/// Damage falloff over range. `ballistics.js:132`:
/// `1 - (1 - p.dropoff) * range01 * range01`.
///
/// `dropoff` is "how much damage survives at max range" (0 = falls to nothing,
/// 1 = no falloff at all), and the curve is quadratic in `range01` — most of
/// the damage holds for the first half of the round's range, then drops away
/// faster near the end.
pub fn falloff(dropoff: f64, range01: f64) -> f64 {
    1.0 - (1.0 - dropoff) * range01 * range01
}

/// `class ProjectileSim`, `ballistics.js:36-166`.
///
/// The pool (`this.pool`) and the live list (`this.live`) are both present in
/// the source, but the source's `live` array holds *references* to the same
/// objects the pool owns — JS objects are reference types, so `this.live[i]`
/// and `this.pool[j]` can be the identical object. Rust's `Projectile` is a
/// plain `Copy` struct with no such aliasing, so the port's `live` holds
/// **indices into `pool`** instead of a second copy of the data. Every read
/// site that would have dereferenced a shared JS object instead indexes
/// `self.pool[i]`; the observable behaviour — which round is "live", what it
/// carries — is identical.
pub struct ProjectileSim {
    pool: [Projectile; MAX_LIVE],
    live: Vec<usize>,
    pub stats: BallisticsStats,
}

impl Default for ProjectileSim {
    fn default() -> Self {
        ProjectileSim {
            pool: [Projectile::default(); MAX_LIVE],
            live: Vec::new(),
            stats: BallisticsStats::default(),
        }
    }
}

impl ProjectileSim {
    pub fn new() -> Self {
        ProjectileSim::default()
    }

    /// How many rounds are currently in flight. Not in the source (JS reads
    /// `this.live.length`); the port needs a way to ask without exposing the
    /// pool indices themselves.
    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// Read a live round by its position in the live list (`0` = oldest).
    /// Not in the source; the port's tests and any future caller need a way to
    /// inspect a spawned round without reaching into pool indices.
    pub fn live_at(&self, i: usize) -> Option<&Projectile> {
        self.live.get(i).map(|&idx| &self.pool[idx])
    }

    /// `spawn(o)`, `ballistics.js:59-92`.
    ///
    /// Returns the live-list position of the spawned round, or `None` only
    /// when the pool is exhausted *and* nothing is live to retire — the
    /// source's `if (!p) return null` at the point where `this.live[0]` is
    /// itself undefined, which cannot happen once anything has ever spawned
    /// but is preserved as a guard exactly as the source has it.
    pub fn spawn(
        &mut self,
        o: SpawnParams,
        world: Option<&mut dyn RaycastWorld>,
        events: Option<&EventBus>,
    ) -> Option<usize> {
        // `for (i) if (!pool[i].alive) { p = pool[i]; break; }`
        let mut slot = (0..self.pool.len()).find(|&i| !self.pool[i].alive);
        if slot.is_none() {
            // "Oldest round yields its slot rather than dropping the shot"
            // (`ballistics.js:67-72`).
            //
            // SOURCE DEFECT, fixed here: the source's
            // `p = this.live[0]; this._retire(p);` never removes that entry
            // from `this.live` before `this.live.push(p)` a few lines below.
            // Because JS objects are references, the recycled round ends up
            // listed *twice* — the stale index-0 slot and the freshly pushed
            // tail slot both name the same object — so the next `fixedUpdate`
            // steps it twice in one frame (its velocity effectively doubles
            // for that tick), and once the first occurrence dies and is
            // spliced out, the surviving stale entry keeps stepping an
            // already-retired (`alive: false`) object. `live_count` can grow
            // past `MAX_LIVE` this way under sustained fire.
            //
            // That is a genuine bug, not the intended behaviour: the source's
            // own comment says the old occupant should merely "yield its
            // slot", which only requires it stop being live and the slot be
            // reused once — not processed twice while also lingering as a
            // phantom entry. Per the port recipe's "fix it, comment why, and
            // cover it" clause, the port removes the oldest entry from `live`
            // before it is (correctly, singly) re-pushed below;
            // `the_oldest_round_yields_its_slot_once_the_pool_is_exhausted`
            // in `tests/weapons_port.rs` pins `live_count() == MAX_LIVE`,
            // which the unfixed behaviour would violate.
            let oldest = self.live.first().copied()?;
            self.live.remove(0);
            self.retire(oldest);
            slot = Some(oldest);
        }
        let idx = slot?;

        let p = &mut self.pool[idx];
        p.alive = true;
        p.pos = o.origin;
        p.prev = o.origin;
        p.dir = o.dir.normalize();
        p.vel = p.dir.scale(o.speed);
        p.damage = o.damage;
        p.penetration = o.penetration;
        p.drag_k = o.drag_k;
        p.dropoff = o.dropoff;
        p.max_range = o.max_range;
        p.travelled = 0.0;
        p.age = 0.0;
        p.weapon = o.weapon;
        p.mask = o.mask;

        // The source pushes the *object* `p` refers to; the port pushes its
        // pool index — see the struct doc for why that is the same thing. An
        // empty-slot find never touched `live` at all, and the recycle branch
        // above already removed its stale entry, so this always lists `idx`
        // exactly once.
        self.live.push(idx);
        self.stats.fired += 1;

        if o.tracer {
            self.emit_tracer(idx, o.speed, world, events);
        }
        Some(self.live.len() - 1)
    }

    /// `_emitTracer(p, speed)`, `ballistics.js:95-107`. One tracer per burst:
    /// muzzle to wherever the round will land (or 260 m, whichever is
    /// shorter).
    fn emit_tracer(
        &self,
        idx: usize,
        speed: f64,
        world: Option<&mut dyn RaycastWorld>,
        events: Option<&EventBus>,
    ) {
        let p = &self.pool[idx];
        let from = p.pos;
        let mut dist = p.max_range.min(260.0);
        if let Some(world) = world {
            if let Some(hit) = world.raycast(p.pos, p.dir, dist) {
                dist = hit.distance;
            }
        }
        let to = p.pos.add_scaled(p.dir, dist);
        let payload = TracerEvent {
            from,
            to,
            speed,
            weapon: p.weapon,
        };
        // The source always has `this.ctx.events` (the engine's event bus is
        // never optional there); the port's `spawn`/`fixed_update` are usable
        // stand-alone, ahead of any engine wiring, so `events` is `Option` and
        // a `None` simply skips the emit rather than failing to compile a
        // caller that has not wired the engine yet.
        if let Some(bus) = events {
            bus.emit("bullet:tracer", &payload);
        }
    }

    fn retire(&mut self, idx: usize) {
        self.pool[idx].alive = false;
        self.pool[idx].weapon = None;
    }

    /// `fixedUpdate(h)`, `ballistics.js:109-155`. Integrates every live round
    /// one physics step, resolves hits, and expires anything past its range,
    /// age or altitude floor.
    pub fn fixed_update(&mut self, h: f64, mut world: Option<&mut dyn RaycastWorld>) {
        // The source walks `this.live` back-to-front so `splice` during the
        // loop never skips an element; the port walks the same index range for
        // the same reason, over `Vec::remove` instead of `splice`.
        let mut i = self.live.len();
        while i > 0 {
            i -= 1;
            let idx = self.live[i];
            let p = &mut self.pool[idx];
            p.prev = p.pos;
            // gravity + a linear drag term (good enough over game distances)
            p.vel.y += GRAVITY * h;
            let decay = (1.0 - p.drag_k * h).max(0.0);
            p.vel = p.vel.scale(decay);
            p.pos = p.pos.add_scaled(p.vel, h);
            p.age += h;

            let seg = p.pos.sub(p.prev);
            let seg_len = seg.length();
            p.travelled += seg_len;

            if seg_len > 1e-6 {
                if let Some(world) = world.as_deref_mut() {
                    let hit_dir = seg.scale(1.0 / seg_len);
                    if let Some(hit) = world.raycast(p.prev, hit_dir, seg_len) {
                        let _ = hit; // the source reads `hit.hit` only; distance is unused here
                        let r01 = range01(p.travelled, p.max_range);
                        let fall = falloff(p.dropoff, r01);
                        world.fire_bullet(FireBulletRequest {
                            origin: p.prev,
                            dir: hit_dir,
                            max_dist: (p.max_range - p.travelled + seg_len).max(1.5).min(24.0),
                            damage: p.damage * fall,
                            penetration: p.penetration,
                            dropoff: 1.0,
                            mask: p.mask,
                        });
                        self.stats.impacts += 1;
                        self.retire(idx);
                        self.live.remove(i);
                        continue;
                    }
                }
            }

            if p.travelled > p.max_range || p.age > 5.0 || p.pos.y < -80.0 {
                self.retire(idx);
                self.live.remove(i);
            }
        }
        self.stats.live = self.live.len() as u32;
    }

    /// `clear()`, `ballistics.js:162-165`.
    pub fn clear(&mut self) {
        // The source iterates `this.live` retiring each, then truncates it.
        // Same order, same effect.
        let live = std::mem::take(&mut self.live);
        live.into_iter().for_each(|idx| self.retire(idx));
    }
}
