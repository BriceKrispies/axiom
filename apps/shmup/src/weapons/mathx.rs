//! Ported from Claude-of-Duty `src/weapons/mathx.js:1-230`.
//!
//! A small deterministic math kit for the viewmodel rig: critically-damped
//! springs, one-dimensional fbm noise for idle sway, and the easing curves the
//! weapon system slaps recoil/ADS/bob transitions through. Everything here is
//! pure math over `f64`/`f32` scalars — no allocation after construction
//! ([`Noise1::new`] allocates its table once; every other call here is
//! allocation-free, matching the source's "no `new` inside `update()`"
//! discipline).
//!
//! Golden values below were captured by running the original `mathx.js` under
//! Node (v24), printed as `JSON.stringify(..., null, 2)`. Anything reachable by
//! `+ - * /` and comparisons only is asserted exactly; anything touching
//! `sin`/`cos`/`ln`/`sqrt`/`exp` (the springs, `easeInOutSine`, the noise field)
//! is asserted within `1e-12`, the tolerance established in
//! `tests/core_port.rs` for the same reason: those are not bit-guaranteed
//! across libm implementations.

use crate::rng::Rng;

/// `mathx.js:9`.
pub const TAU: f64 = std::f64::consts::TAU;
/// `mathx.js:10`.
pub const DEG: f64 = std::f64::consts::PI / 180.0;

/// `mathx.js:12-14`.
pub fn clamp(v: f64, a: f64, b: f64) -> f64 {
    if v < a {
        a
    } else if v > b {
        b
    } else {
        v
    }
}

/// `mathx.js:16-18`.
pub fn clamp01(v: f64) -> f64 {
    clamp(v, 0.0, 1.0)
}

/// `mathx.js:20-22`.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// `mathx.js:24-27`. `b - a || 1e-6` guards a degenerate `a == b` range; ported
/// as an explicit zero check since Rust has no truthiness coercion.
pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let span = b - a;
    let span = if span == 0.0 { 1e-6 } else { span };
    let t = clamp01((x - a) / span);
    t * t * (3.0 - 2.0 * t)
}

/// 5th-order smootherstep — zero 1st AND 2nd derivative at both ends.
/// `mathx.js:30-33`.
pub fn smootherstep(a: f64, b: f64, x: f64) -> f64 {
    let span = b - a;
    let span = if span == 0.0 { 1e-6 } else { span };
    let t = clamp01((x - a) / span);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// The source's default `k` for [`ease_out_back`] (`mathx.js:36`, `k = 1.6`).
pub const EASE_OUT_BACK_DEFAULT_K: f64 = 1.6;

/// Slight overshoot ease used for mag slaps and bolt releases.
/// `mathx.js:36-39`. Rust has no default parameters, so callers matching the
/// source's defaulted `easeOutBack(t)` pass [`EASE_OUT_BACK_DEFAULT_K`]
/// explicitly.
pub fn ease_out_back(t: f64, k: f64) -> f64 {
    let p = t - 1.0;
    1.0 + p * p * ((k + 1.0) * p + k)
}

/// `mathx.js:41-44`.
pub fn ease_out_cubic(t: f64) -> f64 {
    let p = 1.0 - t;
    1.0 - p * p * p
}

/// `mathx.js:46-48`.
pub fn ease_in_cubic(t: f64) -> f64 {
    t * t * t
}

/// `mathx.js:50-52`.
pub fn ease_in_out_sine(t: f64) -> f64 {
    0.5 - 0.5 * (std::f64::consts::PI * clamp01(t)).cos()
}

/// Frame-rate independent exponential approach. `rate` is the reciprocal of
/// the time constant: how many e-folds per second. `mathx.js:58-60`.
pub fn damp(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    target + (current - target) * (-rate * dt).exp()
}

/// Critically-ish damped spring on a scalar. `f` is the natural frequency in
/// Hz, `z` the damping ratio (1 = no overshoot, 0.5 = lively, >1 = sluggish).
/// Semi-implicit integration so it stays stable at large `dt`.
///
/// Ported from `mathx.js:67-98`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub f: f64,
    pub z: f64,
    pub x: f64,
    pub v: f64,
    pub target: f64,
}

impl Spring {
    /// `constructor(f = 12, z = 1, value = 0)`. Rust has no default
    /// arguments; use [`Spring::default`] for the source's defaulted form.
    pub fn new(f: f64, z: f64, value: f64) -> Self {
        Spring {
            f,
            z,
            x: value,
            v: 0.0,
            target: value,
        }
    }

    /// `mathx.js:76-81`.
    pub fn set(&mut self, v: f64) -> &mut Self {
        self.x = v;
        self.v = 0.0;
        self.target = v;
        self
    }

    /// Instantaneous velocity kick — the recoil impulse path. `mathx.js:84-87`.
    pub fn kick(&mut self, dv: f64) -> &mut Self {
        self.v += dv;
        self
    }

    /// `step(dt, target = this.target)`. `mathx.js:89-97`.
    pub fn step(&mut self, dt: f64, target: f64) -> f64 {
        self.target = target;
        let w = TAU * self.f;
        // Semi-implicit Euler: solve for v(n+1) then integrate x with it.
        let denom = 1.0 + 2.0 * self.z * w * dt + w * w * dt * dt;
        self.v = (self.v + w * w * dt * (target - self.x)) / denom;
        self.x += self.v * dt;
        self.x
    }

    /// `step(dt)` with the defaulted `target = this.target` the source's
    /// signature allows and Rust's does not.
    pub fn step_to_target(&mut self, dt: f64) -> f64 {
        let target = self.target;
        self.step(dt, target)
    }
}

impl Default for Spring {
    /// `new Spring()` — the source's defaulted constructor arguments
    /// (`f = 12, z = 1, value = 0`).
    fn default() -> Self {
        Spring::new(12.0, 1.0, 0.0)
    }
}

/// Three independent springs sharing frequency/damping — position or euler.
/// Ported from `mathx.js:101-164`.
///
/// **Source quirk, preserved exactly:** the class declares `get z()` twice —
/// once returning the damping ratio (`this.a.z`, `mathx.js:120-122`) and again
/// returning the z-position component (`this.c.x`, `mathx.js:153-155`). In a
/// JS class body the later method wins, so the damping-ratio getter is dead:
/// reading `.z` always yields the position, while *writing* `.z` still sets
/// the damping ratio (there is only one setter). `viewmodel.js` exploits
/// exactly this split — e.g. `this.recPos.z = r.damping` followed later by
/// `pz += this.recPos.z` reading the position back — so this is load-bearing
/// behaviour, not a bug to fix quietly. [`Spring3::set_z`] is the setter
/// (damping ratio); [`Spring3::z`] is the position getter, grouped with
/// [`Spring3::x`]/[`Spring3::y`] rather than with `set_z` to keep the split
/// visible at the call site instead of hiding it behind one deceptive name.
/// The damping ratio, once set, remains readable directly off the field:
/// `spring3.a.z`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring3 {
    pub a: Spring,
    pub b: Spring,
    pub c: Spring,
}

impl Spring3 {
    /// `constructor(f = 12, z = 1)`.
    pub fn new(f: f64, z: f64) -> Self {
        Spring3 {
            a: Spring::new(f, z, 0.0),
            b: Spring::new(f, z, 0.0),
            c: Spring::new(f, z, 0.0),
        }
    }

    /// `get f()` (`mathx.js:112-114`) — unlike `z`, there is no name
    /// collision, so this reads the damping-unrelated frequency as written.
    pub fn f(&self) -> f64 {
        self.a.f
    }

    /// `set f(v)` (`mathx.js:108-110`).
    pub fn set_f(&mut self, v: f64) {
        self.a.f = v;
        self.b.f = v;
        self.c.f = v;
    }

    /// `set z(v)` (`mathx.js:116-118`) — sets the damping ratio on all three
    /// springs. See the struct-level doc for why this is paired with
    /// [`Spring3::z`] reading something else entirely.
    pub fn set_z(&mut self, v: f64) {
        self.a.z = v;
        self.b.z = v;
        self.c.z = v;
    }

    /// `mathx.js:124-129`.
    pub fn kick(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.a.kick(x);
        self.b.kick(y);
        self.c.kick(z);
        self
    }

    /// `mathx.js:131-136`.
    pub fn reset(&mut self) -> &mut Self {
        self.a.set(0.0);
        self.b.set(0.0);
        self.c.set(0.0);
        self
    }

    /// `step(dt, tx = 0, ty = 0, tz = 0)`. `mathx.js:138-143`.
    pub fn step(&mut self, dt: f64, tx: f64, ty: f64, tz: f64) -> &mut Self {
        self.a.step(dt, tx);
        self.b.step(dt, ty);
        self.c.step(dt, tz);
        self
    }

    /// `mathx.js:145-147`.
    pub fn x(&self) -> f64 {
        self.a.x
    }

    /// `mathx.js:149-151`.
    pub fn y(&self) -> f64 {
        self.b.x
    }

    /// `mathx.js:153-155` — the getter that shadows the damping-ratio getter.
    /// See the struct-level doc.
    pub fn z(&self) -> f64 {
        self.c.x
    }

    /// Copy the spring state into a THREE.Vector3-like target. `mathx.js:158-163`.
    ///
    /// The source writes into a caller-supplied `{x, y, z}` object; nothing in
    /// this port has a shared vector type yet (`viewmodel.js`, the only
    /// consumer, is not ported), so this takes three `&mut f64` out-parameters
    /// instead.
    pub fn write_to(&self, x: &mut f64, y: &mut f64, z: &mut f64, scale: f64) {
        *x = self.a.x * scale;
        *y = self.b.x * scale;
        *z = self.c.x * scale;
    }
}

impl Default for Spring3 {
    /// `new Spring3()` — the source's defaulted constructor arguments
    /// (`f = 12, z = 1`).
    fn default() -> Self {
        Spring3::new(12.0, 1.0)
    }
}

/// The source's default table size for [`Noise1::new`] (`mathx.js:175`,
/// `size = 512`).
pub const NOISE1_DEFAULT_SIZE: usize = 512;

/// The source's default octave count for [`Noise1::fbm`] (`mathx.js:210`,
/// `oct = 3`).
pub const NOISE1_DEFAULT_OCTAVES: u32 = 3;
/// The source's default gain for [`Noise1::fbm`] (`mathx.js:210`, `gain = 0.5`).
pub const NOISE1_DEFAULT_GAIN: f64 = 0.5;

/// Layered 1-D value noise with cubic interpolation.
///
/// Idle sway needs to never visibly loop, so each octave gets its own
/// incommensurate rate and a table long enough that the pattern does not
/// repeat inside a play session. Sampling is a table lookup + a lerp: cheap
/// enough to run a dozen of these every frame.
///
/// Ported from `mathx.js:174-223`. The source's `Float32Array` table is a
/// `Vec<f32>` here: every table write in the source narrows to 32 bits on
/// assignment (`Float32Array` coercion), so the port narrows explicitly at the
/// same two sites (initial fill, smoothing pass) and widens back to `f64` for
/// every read — matching the source's mixed precision exactly rather than
/// silently upgrading the table to `f64`.
#[derive(Debug, Clone)]
pub struct Noise1 {
    /// `this.size` (`mathx.js:176`) — a plain public field in the source, kept
    /// public here.
    pub size: usize,
    /// `this.t` (`mathx.js:177`) — the source's `Float32Array` table, also a
    /// plain public field.
    pub t: Vec<f32>,
}

impl Noise1 {
    /// `constructor(rng, size = 512)`. `mathx.js:175-186`.
    pub fn new(rng: &mut Rng, size: usize) -> Self {
        let raw: Vec<f32> = (0..size).map(|_| rng.signed() as f32).collect();
        // Smooth the table once so the low octaves are gentle rather than
        // jittery. Computed in f64 (as the source does, reading the
        // Float32Array widens to a JS double) then narrowed back to f32 on
        // write, exactly as `tmp[i] = ...` coerces on assignment.
        let smoothed: Vec<f32> = (0..size)
            .map(|i| {
                let prev = f64::from(raw[(i + size - 1) % size]);
                let mid = f64::from(raw[i]);
                let next = f64::from(raw[(i + 1) % size]);
                ((prev + mid * 2.0 + next) * 0.25) as f32
            })
            .collect();
        Noise1 {
            size,
            t: smoothed,
        }
    }

    /// `mathx.js:188-207`.
    pub fn at(&self, x: f64) -> f64 {
        let size = self.size;
        let fx = x - x.floor();
        // `((Math.floor(x) % size) + size) % size` — floor-mod so negative x
        // wraps into range instead of yielding a negative index.
        let size_i = size as i64;
        let floor_i = x.floor() as i64;
        let i = (((floor_i % size_i) + size_i) % size_i) as usize;
        let a = f64::from(self.t[(i + size - 1) % size]);
        let b = f64::from(self.t[i]);
        let c = f64::from(self.t[(i + 1) % size]);
        let d = f64::from(self.t[(i + 2) % size]);
        // Catmull-Rom keeps the curve C1 so the weapon never ticks.
        let t = fx;
        let t2 = t * t;
        let t3 = t2 * t;
        0.5 * ((2.0 * b)
            + (-a + c) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
            + (-a + 3.0 * b - 3.0 * c + d) * t3)
    }

    /// fBm over `oct` octaves; irrational lacunarity keeps octaves out of
    /// phase. `mathx.js:209-222`.
    pub fn fbm(&self, x: f64, oct: u32, gain: f64) -> f64 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        let mut norm = 0.0;
        let mut freq = 1.0;
        for i in 0..oct {
            sum += self.at(x * freq + f64::from(i) * 37.19) * amp;
            norm += amp;
            amp *= gain;
            freq *= 2.117_13;
        }
        // `norm || 1` — falls back to 1 when `oct == 0` leaves norm at 0.
        sum / if norm == 0.0 { 1.0 } else { norm }
    }
}

/// Wrap an angle into (-PI, PI]. `mathx.js:226-230`.
pub fn wrap_pi(a: f64) -> f64 {
    let mut a = (a + std::f64::consts::PI) % TAU;
    if a < 0.0 {
        a += TAU;
    }
    a - std::f64::consts::PI
}
