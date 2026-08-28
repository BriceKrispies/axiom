//! Deterministic PRNG (xoshiro128\*\*) with a SplitMix32 seed expander.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/core/rng.js:1-95` — the whole file.
//!
//! Gameplay randomness — recoil patterns, spread, particle jitter, AI timing —
//! must run through this so capture mode produces byte-identical frames. The
//! source is already fully seed-driven: one root seed, then a disciplined
//! [`Rng::fork`] per subsystem so editing one system cannot reshuffle another.
//! Reproducing that means reproducing *this generator exactly* — same algorithm,
//! same constants, same state layout, same call order. Every draw site in the
//! rest of the port is a consumer of this sequence; drift here is drift
//! everywhere, silently.
//!
//! **Why this is not the kernel's `DeterministicRng`.** `axiom_kernel::
//! DeterministicRng` is `splitmix64`: 64 bits of state, a different step
//! function, a different output word size. It is an excellent deterministic
//! source and it is not this one. Substituting it would produce a *different
//! sequence* — every spread cone, every scatter placement, every ambience bed in
//! the port would land somewhere else, and no reference frame from the source
//! could ever be matched. So xoshiro128\*\* is ported here, into the app, where
//! game-specific reproducibility belongs. (SplitMix32 does appear below, but only
//! as the seed expander that spreads one 32-bit seed across four state words —
//! it is not the generator.)
//!
//! **Bit-exactness against the JavaScript.** Every operation in the source is
//! performed on the 32-bit representation: `Math.imul` is a wrapping 32-bit
//! multiply, `<<`/`>>>` truncate to 32 bits, and `^` operates on the same bits
//! (JS `^` yields a *signed* int32, so the source's state words go negative
//! after the first draw — that is a display artefact of JS numbers, not a
//! difference in bits). A `u32` implementation with wrapping arithmetic is
//! therefore bit-identical, and `tests/core_port.rs` pins that against sequences
//! captured from the original `rng.js` running under Node.
//!
//! The floating-point derivations ([`Rng::float`] and everything built on it)
//! use `f64` because a JavaScript number *is* an `f64`; narrowing to `f32` here
//! would change the value of every draw.

/// A deterministic xoshiro128\*\* generator.
#[derive(Debug, Clone)]
pub struct Rng {
    s0: u32,
    s1: u32,
    s2: u32,
    s3: u32,
    /// The second Box–Muller sample, cached until the next [`Rng::gauss`].
    /// `undefined` in the source; `None` here.
    spare: Option<f64>,
}

impl Rng {
    /// The source's default seed (`rng.js:7`) — the golden-ratio constant, also
    /// used as the SplitMix32 increment below.
    pub const DEFAULT_SEED: u32 = 0x9e37_79b9;

    /// Build a generator from a 32-bit seed.
    ///
    /// The source's `constructor(seed = 0x9e3779b9)`. Rust has no default
    /// arguments, so [`Rng::default`] carries the defaulted form.
    pub fn new(seed: u32) -> Self {
        let mut rng = Rng {
            s0: 0,
            s1: 0,
            s2: 0,
            s3: 0,
            spare: None,
        };
        rng.seed(seed);
        rng
    }

    /// Re-seed in place. SplitMix32 spreads one 32-bit seed across the four
    /// state words.
    ///
    /// Note what this deliberately does *not* do: it does not clear the cached
    /// Box–Muller `spare`. The source does not either (`rng.js:11-26` touches
    /// only `s0..s3`), so a `gauss()` immediately after a re-seed returns the
    /// spare left over from *before* it. That is observable behaviour and the
    /// port keeps it; `reseed_keeps_the_cached_gauss_spare` pins it.
    pub fn seed(&mut self, s: u32) -> &mut Self {
        let mut z = s;
        let mut next = || {
            z = z.wrapping_add(0x9e37_79b9);
            let mut x = z;
            x = (x ^ (x >> 16)).wrapping_mul(0x21f0_aaad);
            x = (x ^ (x >> 15)).wrapping_mul(0x735a_2d97);
            x ^ (x >> 15)
        };
        self.s0 = next();
        self.s1 = next();
        self.s2 = next();
        self.s3 = next();
        self
    }

    /// Uniform `u32` — the one primitive draw; everything else is derived.
    pub fn u32(&mut self) -> u32 {
        // `Math.imul(rot(Math.imul(s1, 5), 7), 9)` — the xoshiro128** scrambler.
        let result = self.s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s1 << 9;
        self.s2 ^= self.s0;
        self.s3 ^= self.s1;
        self.s1 ^= self.s2;
        self.s0 ^= self.s3;
        self.s2 ^= t;
        self.s3 = self.s3.rotate_left(11);
        result
    }

    /// Uniform `[0,1)`.
    pub fn float(&mut self) -> f64 {
        f64::from(self.u32()) / 4294967296.0
    }

    /// Uniform `[min,max)`.
    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * self.float()
    }

    /// Uniform integer `[min,max]`, inclusive at both ends.
    ///
    /// The source computes `min + (u32() % (max - min + 1))` in double
    /// arithmetic; the span is small and positive in every call site, so a `u32`
    /// modulus is the same value. A `max < min` span is a caller bug: JS would
    /// silently yield `NaN` (modulo zero or negative), this panics on the
    /// division. Being told is better.
    pub fn int(&mut self, min: i32, max: i32) -> i32 {
        let span = (i64::from(max) - i64::from(min) + 1) as u32;
        min + (self.u32() % span) as i32
    }

    /// Uniform `[-1,1]`.
    pub fn signed(&mut self) -> f64 {
        self.float() * 2.0 - 1.0
    }

    /// Standard normal via Box–Muller. One sample is returned; the pair's second
    /// is cached in `spare` and returned by the next call, so two `gauss()` calls
    /// consume exactly two `float()` draws — the call-order detail that keeps a
    /// forked stream aligned with the source.
    pub fn gauss(&mut self) -> f64 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let mut u = 0.0;
        while u == 0.0 {
            u = self.float();
        }
        let r = (-2.0 * u.ln()).sqrt();
        let th = 2.0 * std::f64::consts::PI * self.float();
        self.spare = Some(r * th.sin());
        r * th.cos()
    }

    /// A uniformly-chosen element. Panics on an empty slice, where JS would
    /// quietly hand back `undefined`.
    pub fn pick<'a, T>(&mut self, arr: &'a [T]) -> &'a T {
        &arr[(self.u32() % arr.len() as u32) as usize]
    }

    /// A point uniformly inside the unit disc — bullet spread, particle emission.
    ///
    /// The source writes into a caller-supplied `out = {x, y}` to dodge a JS
    /// allocation per call. Rust returns the pair by value: a two-`f64` tuple is
    /// returned in registers, so the out-parameter it was avoiding does not exist
    /// here and reproducing it would only obscure the call sites.
    pub fn disc(&mut self) -> (f64, f64) {
        let r = self.float().sqrt();
        let a = self.float() * std::f64::consts::PI * 2.0;
        (a.cos() * r, a.sin() * r)
    }

    /// An independent stream derived from this one — lets a subsystem randomise
    /// without perturbing another subsystem's sequence.
    ///
    /// It costs the parent exactly one `u32()` draw, which is what makes the
    /// "fork once at init, in registration order" discipline in the source
    /// reproducible.
    pub fn fork(&mut self) -> Rng {
        Rng::new(self.u32())
    }

    /// The four state words, in order. Not in the source (JS reads the fields
    /// directly); exposed so a test can pin the seed expander independently of
    /// the scrambler, and so a future save/replay can snapshot a live stream.
    pub fn state(&self) -> [u32; 4] {
        [self.s0, self.s1, self.s2, self.s3]
    }
}

impl Default for Rng {
    /// `new Rng()` — the source's defaulted constructor argument.
    fn default() -> Self {
        Rng::new(Rng::DEFAULT_SEED)
    }
}
