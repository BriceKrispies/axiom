//! Scalar maths + spring integrators used by the player controller.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/player/springs.js:1-177` — the whole
//! file.
//!
//! Everything here is allocation-free after construction and framerate
//! independent: [`Spring::step`] sub-steps internally so an 8 ms physics tick
//! and a 33 ms hitch produce the same visible motion (with one documented
//! exception — see [`Spring::step`]'s doc comment).
//!
//! `f64` throughout, matching the source: a JavaScript number *is* an `f64`,
//! and every value here eventually feeds a golden comparison captured from the
//! original running under Node — narrowing to `f32` would change the value of
//! every draw. This is the same reasoning `rng.rs` and `weapons/mathx.rs` give
//! for the same choice.

/// `springs.js:9`.
pub const TAU: f64 = std::f64::consts::TAU;
/// `springs.js:10`.
pub const DEG: f64 = std::f64::consts::PI / 180.0;

/// `springs.js:12-14`.
pub fn clamp(v: f64, a: f64, b: f64) -> f64 {
    if v < a {
        a
    } else if v > b {
        b
    } else {
        v
    }
}

/// `springs.js:16-18`.
pub fn clamp01(v: f64) -> f64 {
    clamp(v, 0.0, 1.0)
}

/// `springs.js:20-22`.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// `springs.js:24-27`.
pub fn smoothstep(t: f64) -> f64 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}

/// C2-continuous ease — used for rooted mantle curves where velocity must not
/// pop. `springs.js:29-33`.
pub fn smootherstep(t: f64) -> f64 {
    let t = clamp01(t);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// `springs.js:35-39`.
pub fn ease_out_cubic(t: f64) -> f64 {
    let t = clamp01(t);
    let u = 1.0 - t;
    1.0 - u * u * u
}

/// `springs.js:41-43`.
pub fn ease_in_out_sine(t: f64) -> f64 {
    0.5 - 0.5 * (clamp01(t) * std::f64::consts::PI).cos()
}

/// Exponential approach with a real time constant. `tau` is the 63 % time, so
/// "reach it in about a tenth of a second" is `tau = 0.1 / 2.3`.
/// `springs.js:45-52`.
pub fn approach(current: f64, target: f64, tau: f64, dt: f64) -> f64 {
    if tau <= 1e-6 {
        return target;
    }
    target + (current - target) * (-dt / tau).exp()
}

/// Constant-rate move, for things that must not have an asymptotic tail.
/// `springs.js:54-61`.
pub fn move_toward(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    let d = target - current;
    let step = rate * dt;
    if d > step {
        current + step
    } else if d < -step {
        current - step
    } else {
        target
    }
}

/// Shortest signed angular difference, radians. `springs.js:63-69`.
pub fn angle_delta(from: f64, to: f64) -> f64 {
    let mut d = (to - from) % TAU;
    if d > std::f64::consts::PI {
        d -= TAU;
    } else if d < -std::f64::consts::PI {
        d += TAU;
    }
    d
}

/// Deterministic value noise in 1D — camera shake without touching any RNG.
/// `springs.js:71-84`.
///
/// The inner hash closure is inlined as [`hash_noise_lattice`] since Rust
/// closures cannot recurse into a named helper as tersely as the source's
/// arrow function; the maths is unchanged.
pub fn hash_noise(x: f64, seed: i32) -> f64 {
    let xi = x.floor();
    let f = x - xi;
    let u = f * f * (3.0 - 2.0 * f);
    hash_noise_lattice(xi, seed) * (1.0 - u) + hash_noise_lattice(xi + 1.0, seed) * u
}

/// The source's `h(i)` closure (`springs.js:75-81`), lifted to a named
/// function. `i | 0` is JavaScript's `ToInt32` ([`to_int32`]); `Math.imul` is
/// a wrapping 32-bit multiply (`wrapping_mul`); `>>> 0`/`>>> n` reinterpret the
/// int32 bit pattern as `u32` before shifting, exactly as `as u32` does here.
fn hash_noise_lattice(i: f64, seed: i32) -> f64 {
    let mut n = to_int32(i) ^ seed.wrapping_mul(374761393);
    n = (n ^ (((n as u32) >> 15) as i32)).wrapping_mul(0x2c1b3c6d_u32 as i32);
    n = (n ^ (((n as u32) >> 12) as i32)).wrapping_mul(0x297a2d39_u32 as i32);
    n ^= ((n as u32) >> 15) as i32;
    ((n as u32) as f64 / 4294967296.0) * 2.0 - 1.0
}

/// JavaScript's `ToInt32` abstract operation: truncate toward zero, then wrap
/// into the 32-bit signed range. `i32 as f64 as i32` (a plain `as` cast) is
/// **not** the same operation — Rust saturates an out-of-range float-to-int
/// cast instead of wrapping — so `hash_noise_lattice` goes through this
/// instead. `xi`/`xi + 1` are always integral by construction (`x.floor()` and
/// one more), so only the truncate-and-wrap tail of the spec actually applies.
fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc().rem_euclid(4294967296.0); // wrap into [0, 2^32)
    if m >= 2147483648.0 {
        (m - 4294967296.0) as i32
    } else {
        m as i32
    }
}

/// Sub-steps larger than this are split. `springs.js:86`.
const MAX_SUB_DT: f64 = 1.0 / 360.0;

/// Damped harmonic oscillator, driven by frequency (Hz) and damping ratio.
///   - `zeta < 1` under-damped, overshoots — good for punchy recoil
///   - `zeta = 1` critically damped, fastest non-overshooting — good for
///     FOV/ADS
///
/// [`Spring::impulse`] injects velocity (the physical way to kick a spring),
/// [`Spring::set`] displaces it instantly. `springs.js:88-142`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub freq: f64,
    pub damping: f64,
    pub value: f64,
    pub velocity: f64,
    pub target: f64,
}

impl Spring {
    pub fn new(freq: f64, damping: f64, value: f64) -> Self {
        Spring {
            freq,
            damping,
            value,
            velocity: 0.0,
            target: 0.0,
        }
    }

    pub fn reset(&mut self, value: f64) -> &mut Self {
        self.value = value;
        self.velocity = 0.0;
        self
    }

    pub fn impulse(&mut self, v: f64) -> &mut Self {
        self.velocity += v;
        self
    }

    pub fn set(&mut self, v: f64) -> &mut Self {
        self.value = v;
        self
    }

    /// `springs.js:120-141`.
    ///
    /// **Source quirk, ported and pinned as-is (recipe rule 7):** the sub-step
    /// loop caps at `guard < 24` iterations. At `MAX_SUB_DT = 1/360 s` that
    /// bounds a single `step()` call to at most `24/360 ≈ 66.7 ms` of *actually
    /// integrated* time — a `dt` hitch larger than that is **not** fully
    /// simulated; the un-integrated remainder is silently dropped rather than
    /// carried to the next call. `tests/player_port.rs` pins this exact
    /// under-integration against a 0.2 s hitch (see
    /// `spring_step_caps_substeps_at_24_and_drops_the_remainder_on_a_big_hitch`)
    /// rather than "fixing" it — a bigger guard changes the settled value for
    /// every recorded golden and is out of scope for a faithfulness port.
    pub fn step(&mut self, dt: f64) -> f64 {
        if dt <= 0.0 {
            return self.value;
        }
        let w = TAU * self.freq;
        let k = w * w;
        let c = 2.0 * self.damping * w;
        let mut remaining = dt;
        let mut guard = 0;
        while remaining > 1e-7 && guard < 24 {
            guard += 1;
            let h = if remaining > MAX_SUB_DT {
                MAX_SUB_DT
            } else {
                remaining
            };
            remaining -= h;
            let a = -k * (self.value - self.target) - c * self.velocity;
            self.velocity += a * h;
            self.value += self.velocity * h;
        }
        // Kill denormal ringing so idle frames are bit-stable for capture.
        if (self.value - self.target).abs() < 1e-7 && self.velocity.abs() < 1e-6 {
            self.value = self.target;
            self.velocity = 0.0;
        }
        self.value
    }
}

/// Two-layer response: a fast under-damped spring plus a slow exponential
/// residual. Real weapon/camera recoil rises instantly, snaps most of the way
/// back, then settles — a single spring can only do two of those three.
/// `springs.js:144-177`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoilAxis {
    pub spring: Spring,
    pub residual: f64,
    pub residual_tau: f64,
    pub residual_share: f64,
    pub value: f64,
}

impl RecoilAxis {
    pub fn new(freq: f64, damping: f64, residual_tau: f64, residual_share: f64) -> Self {
        RecoilAxis {
            spring: Spring::new(freq, damping, 0.0),
            residual: 0.0,
            residual_tau,
            residual_share,
            value: 0.0,
        }
    }

    /// The source's defaulted constructor (`freq=9.5, damping=0.52,
    /// residualTau=0.3, residualShare=0.34`), for call sites that want it.
    pub fn default_tuned() -> Self {
        RecoilAxis::new(9.5, 0.52, 0.3, 0.34)
    }

    pub fn reset(&mut self) {
        self.spring.reset(0.0);
        self.residual = 0.0;
        self.value = 0.0;
    }

    /// `amount` is an angle in radians (or metres for a positional axis).
    /// A displacement kick reads snappier than a velocity kick for recoil.
    pub fn kick(&mut self, amount: f64) {
        self.spring.value += amount * (1.0 - self.residual_share);
        self.residual += amount * self.residual_share;
    }

    pub fn step(&mut self, dt: f64) -> f64 {
        self.spring.step(dt);
        self.residual = approach(self.residual, 0.0, self.residual_tau, dt);
        self.value = self.spring.value + self.residual;
        self.value
    }
}
