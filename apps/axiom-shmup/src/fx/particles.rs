//! Ported from Claude-of-Duty `src/fx/particles.js:1-446`.
//!
//! GPU particle system: a fixed-capacity ring buffer of particle records,
//! plus the closed-form integration the source's vertex shader evaluates
//! every frame for every live particle.
//!
//! ## What is ported as data, what is ported as a formula, and what is a seam
//!
//! [`ParticleSpawn`]/[`reset_spawn`] and [`ParticleLayer`] port the CPU side
//! exactly: `emit()` writes 32 floats into a preallocated interleaved array
//! at a wrapping cursor and tracks a dirty span, `flush()` resolves that span
//! and the live/visible state (`particles.js:243-373`). This is real,
//! importable JavaScript, and `tests/fx/capture.mjs` drives the actual
//! `ParticleLayer` class under Node to pin it.
//!
//! [`integrate`] ports `PARTICLE_VERT`'s position/velocity/colour/alpha
//! evaluation (`particles.js:113-159`) as a plain Rust function. **This one
//! is not golden-captured against the original**, and deliberately: the
//! source only ever expresses this as a GLSL string baked into a
//! `THREE.ShaderMaterial` — there is no JavaScript function to import and
//! call, so the golden-capture recipe's premise ("import the original
//! module, call the routine") does not apply. [`integrate`] is instead a
//! direct line-for-line transcription of `PARTICLE_VERT`'s scalar/vector
//! algebra from GLSL to Rust (documented against the exact source lines at
//! each step below) and is pinned by property tests instead: birth/death
//! boundary conditions, and a comparison against small-step numerical
//! (semi-implicit Euler) integration of the same
//! `dv/dt = -k v + g, dx/dt = v` ODE the source's docstring states as the
//! closed form it solves (`particles.js:9-14`).
//!
//! Everything genuinely GPU-only — the vertex/fragment shader **source
//! strings**, the `THREE.InstancedInterleavedBuffer`/`ShaderMaterial`/`Mesh`
//! plumbing, screen-space stretch/rotation (which needs a view matrix) — is
//! not ported. It is presentation work belonging to a future WGSL emission
//! pass in the render pipeline (`docs/work-manifests/shmup-port/
//! 04-remaining-work.md`, "render — Opus / engine"), the same seam the audio
//! port drew around `web_sys` in [`crate::audio::web_audio`].

/// Interleaved record stride, in `f32`s. `particles.js:29` (`STRIDE`).
pub const STRIDE: usize = 32;

/// The alpha below which the source's fragment shader throws the fragment
/// away: `float a = tex.a * vCol.a; if ( a < 0.0035 ) discard;`
/// (`particles.js:190-191`), guarded one line earlier by the stronger
/// `if ( vCol.a <= 0.0 ) discard;` (`particles.js:188`).
///
/// `tex.a` is a coverage channel in `[0, 1]`, so `vCol.a < 0.0035` discards
/// the fragment whatever texel it lands on — which makes this a bound a CPU
/// readback can apply without a texture fetch, and the *only* honest one: a
/// sample under it is not a faint particle, it is a particle the source does
/// not draw at all.
///
/// This is reached on every particle's own birth frame. `PARTICLE_VERT`
/// fades alpha in over the first 4.5% of life
/// (`a = aMisc.z * pow(...) * smoothstep( 0.0, 0.045, n )`, `particles.js:159`),
/// and `smoothstep( 0.0, 0.045, 0.0 )` is exactly zero — so a particle
/// sampled at the instant it was emitted is invisible by construction, in the
/// source as much as here.
pub const DISCARD_ALPHA: f64 = 0.0035;

// Interleaved slot offsets — `particles.js:32-39`.
const O_PS: usize = 0; // pos.xyz, size0
const O_VS: usize = 4; // vel.xyz, size1
const O_LF: usize = 8; // birth, 1/life, drag, gravity
const O_RT: usize = 12; // rot0, spin, stretch, sizeCurve
const O_C0: usize = 16; // colour A rgb, intensity A
const O_C1: usize = 20; // colour B rgb, intensity B
const O_MS: usize = 24; // tile, softness, alpha, alphaCurve
const O_EX: usize = 28; // turbAmp, turbFreq, seed, flags

/// One blend mode — `mode: 'additive'|'lit'`. Ported as an enum rather than a
/// string: the JS mode also selects `#define ADDITIVE`/`#define LIT` at
/// shader-compile time, which has no meaning here since no shader is
/// compiled by this port, so only the discriminant that other CPU-side
/// callers key on (render order, [`crate::fx::system`]'s two layer pools)
/// survives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleMode {
    Additive,
    Lit,
}

/// Reusable spawn descriptor — spawning must never allocate. `particles.js:
/// 44-58` (`SP`) + `61-72` (`resetSpawn`). The source mutates one
/// module-level object and hands back a reference (`resetSpawn()`); this port
/// makes that explicit as `ParticleSpawn::default()`, called fresh at each
/// spawn site the same way the source calls `resetSpawn()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSpawn {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub size0: f64,
    pub size1: f64,
    pub size_curve: f64,
    pub life: f64,
    pub delay: f64,
    pub drag: f64,
    pub gravity: f64,
    pub rot: f64,
    pub spin: f64,
    /// Velocity-aligned smear length multiplier.
    pub stretch: f64,
    pub r0: f64,
    pub g0: f64,
    pub b0: f64,
    pub i0: f64,
    pub r1: f64,
    pub g1: f64,
    pub b1: f64,
    pub i1: f64,
    pub tile: f64,
    pub soft: f64,
    pub alpha: f64,
    pub alpha_curve: f64,
    pub turb: f64,
    pub turb_freq: f64,
    pub seed: f64,
    pub flags: f64,
}

/// `resetSpawn()`, `particles.js:61-72` — the defaults every spawn recipe
/// starts from.
impl Default for ParticleSpawn {
    fn default() -> Self {
        ParticleSpawn {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            size0: 0.2,
            size1: 0.3,
            size_curve: 1.0,
            life: 1.0,
            delay: 0.0,
            drag: 1.4,
            gravity: 0.0,
            rot: 0.0,
            spin: 0.0,
            stretch: 0.0,
            r0: 1.0,
            g0: 1.0,
            b0: 1.0,
            i0: 1.0,
            r1: 1.0,
            g1: 1.0,
            b1: 1.0,
            i1: 0.0,
            tile: 0.0,
            soft: 0.4,
            alpha: 1.0,
            alpha_curve: 1.0,
            turb: 0.0,
            turb_freq: 1.0,
            seed: 0.0,
            flags: 0.0,
        }
    }
}

/// `resetSpawn()` as a free function, matching the source's call shape at
/// spawn sites (`let s = resetSpawn(); s.x = ...;`).
pub fn reset_spawn() -> ParticleSpawn {
    ParticleSpawn::default()
}

/// A fixed-capacity ring of particles backed by one interleaved buffer.
/// `class ParticleLayer`, `particles.js:230-373`. Allocation happens exactly
/// once, in [`ParticleLayer::new`].
pub struct ParticleLayer {
    pub capacity: usize,
    pub mode: ParticleMode,
    cursor: usize,
    high_water: usize,
    expire_at: f64,
    spawned: u64,
    array: Vec<f32>,
    dirty_lo: usize,
    dirty_hi: Option<usize>,
    wrapped: bool,
}

/// What [`ParticleLayer::flush`] resolves per frame — the source's `flush()`
/// side effects (`ibuf.addUpdateRange`/`needsUpdate`, `uTime`,
/// `geometry.instanceCount`, `mesh.visible`), returned instead of mutated
/// into a `THREE` object graph this port does not have.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlushResult {
    /// `(start, count)` in floats, the GPU upload sub-range — `None` when
    /// nothing was dirty this frame.
    pub dirty_range: Option<(usize, usize)>,
    pub instance_count: usize,
    pub visible: bool,
}

impl ParticleLayer {
    /// `constructor(o)`, `particles.js:237-301` — minus every `THREE.*`
    /// object (`InstancedInterleavedBuffer`, `ShaderMaterial`, `Mesh`, the
    /// atlas/uniform wiring): those are the GPU presentation seam described
    /// in the module doc. `capacity` is clamped to a minimum of 16, matching
    /// `Math.max(16, o.capacity | 0)`.
    pub fn new(capacity: usize, mode: ParticleMode) -> Self {
        let capacity = capacity.max(16);
        ParticleLayer {
            capacity,
            mode,
            cursor: 0,
            high_water: 0,
            expire_at: -1.0,
            spawned: 0,
            array: vec![0.0; capacity * STRIDE],
            dirty_lo: usize::MAX,
            dirty_hi: None,
            wrapped: false,
        }
    }

    /// `get active()`, `particles.js:306-308`. Ported against
    /// [`FlushResult::visible`]'s last value rather than `mesh.visible`,
    /// since there is no mesh; see [`ParticleLayer::flush`].
    pub fn active(&self, now: f64) -> bool {
        now < self.expire_at && (self.instance_count() > 0)
    }

    /// How many ring slots a draw is allowed to touch —
    /// `geometry.instanceCount = this._wrapped ? this.capacity : this.highWater`
    /// (`particles.js:430`), set to `0` at construction (`particles.js:286`).
    ///
    /// **Every reader of this layer must bound its slot loop by this, never by
    /// [`ParticleLayer::capacity`].** A slot past it has never been written,
    /// and a zero-filled record is *not* inert: with `birth = 0` and
    /// `1/life = 0` it yields `t = now` and `n = t * 0 = 0`, which sails
    /// straight through the vertex shader's `t < 0.0 || n >= 1.0` early-out
    /// (`particles.js:99`, ported in [`integrate`]) and reads back as a **live**
    /// particle sitting at the world origin with `size = 0` and `alpha = 0`.
    ///
    /// The source is never exposed to that, and not because the shader guards
    /// against it — it does not. The GPU simply never runs the vertex shader on
    /// an instance at or above `instanceCount`. That bound is load-bearing, and
    /// a CPU readback has to reproduce it explicitly or it invents particles
    /// that were never emitted.
    pub fn instance_count(&self) -> usize {
        if self.wrapped {
            self.capacity
        } else {
            self.high_water
        }
    }

    pub fn spawned(&self) -> u64 {
        self.spawned
    }

    /// Raw access to the interleaved storage — the source's `this.array`
    /// (`particles.js:239`), exposed read-only so a caller (or a test) can
    /// inspect exactly what a spawn wrote, the same way the capture script
    /// reads the JS `Float32Array` directly.
    pub fn raw(&self) -> &[f32] {
        &self.array
    }

    /// The atlas tile index a slot was spawned with — `array[slot*STRIDE +
    /// aMisc.x]` (`particles.js:37, 353`). Read-only accessor for tests that
    /// need "which tile did this spawn pick" without re-deriving the
    /// interleave offsets themselves.
    pub fn tile_at(&self, slot: usize) -> f64 {
        f64::from(self.array[slot * STRIDE + O_MS])
    }

    /// The sprite's roll about the view axis at `now`, in radians —
    /// `aRot.x + aRot.y * t` (`particles.js`'s `rot0` + `spin * t`, the term
    /// `PARTICLE_VERT` spins the billboard corners by before projecting them).
    ///
    /// Separate from [`integrate`] because roll is the one term whose *use* is
    /// screen-space: the source rotates the quad's corners in clip space, and a
    /// renderer built on camera-facing world-space quads applies it as a spin
    /// about the camera's forward axis instead. Returning the angle rather than
    /// rotated corners keeps that decision at the renderer, where it belongs.
    ///
    /// Meaningless for a slot [`integrate`] returns `None` for; the caller has
    /// already asked that question by the time it needs this.
    pub fn roll_at(&self, slot: usize, now: f64) -> f64 {
        let b = slot * STRIDE;
        let t = now - f64::from(self.array[b + O_LF]);
        f64::from(self.array[b + O_RT]) + f64::from(self.array[b + O_RT + 1]) * t
    }

    /// Write one particle. `emit(s, now)`, `particles.js:314-360`.
    pub fn emit(&mut self, s: &ParticleSpawn, now: f64) -> usize {
        let i = self.cursor;
        self.cursor = i + 1;
        if self.cursor >= self.capacity {
            self.cursor = 0;
            self.wrapped = true;
        }
        if i + 1 > self.high_water {
            self.high_water = i + 1;
        }

        let b = i * STRIDE;
        let life = s.life.max(0.016);
        let birth = now + s.delay;

        let a = &mut self.array;
        a[b + O_PS] = s.x as f32;
        a[b + O_PS + 1] = s.y as f32;
        a[b + O_PS + 2] = s.z as f32;
        a[b + O_PS + 3] = s.size0 as f32;

        a[b + O_VS] = s.vx as f32;
        a[b + O_VS + 1] = s.vy as f32;
        a[b + O_VS + 2] = s.vz as f32;
        a[b + O_VS + 3] = s.size1 as f32;

        a[b + O_LF] = birth as f32;
        a[b + O_LF + 1] = (1.0 / life) as f32;
        a[b + O_LF + 2] = s.drag as f32;
        a[b + O_LF + 3] = s.gravity as f32;

        a[b + O_RT] = s.rot as f32;
        a[b + O_RT + 1] = s.spin as f32;
        a[b + O_RT + 2] = s.stretch as f32;
        a[b + O_RT + 3] = s.size_curve as f32;

        a[b + O_C0] = s.r0 as f32;
        a[b + O_C0 + 1] = s.g0 as f32;
        a[b + O_C0 + 2] = s.b0 as f32;
        a[b + O_C0 + 3] = s.i0 as f32;

        a[b + O_C1] = s.r1 as f32;
        a[b + O_C1 + 1] = s.g1 as f32;
        a[b + O_C1 + 2] = s.b1 as f32;
        a[b + O_C1 + 3] = s.i1 as f32;

        a[b + O_MS] = s.tile as f32;
        a[b + O_MS + 1] = s.soft as f32;
        a[b + O_MS + 2] = s.alpha as f32;
        a[b + O_MS + 3] = s.alpha_curve as f32;

        a[b + O_EX] = s.turb as f32;
        a[b + O_EX + 1] = s.turb_freq as f32;
        a[b + O_EX + 2] = s.seed as f32;
        a[b + O_EX + 3] = s.flags as f32;

        if i < self.dirty_lo {
            self.dirty_lo = i;
        }
        self.dirty_hi = Some(self.dirty_hi.map_or(i, |hi| hi.max(i)));
        let end = birth + life;
        if end > self.expire_at {
            self.expire_at = end;
        }
        self.spawned += 1;
        i
    }

    /// Upload the dirty span and resolve per-frame state. `flush(now)`,
    /// `particles.js:363-373`.
    pub fn flush(&mut self, now: f64) -> FlushResult {
        let dirty_range = self.dirty_hi.map(|hi| {
            let start = self.dirty_lo * STRIDE;
            let count = (hi - self.dirty_lo + 1) * STRIDE;
            (start, count)
        });
        self.dirty_lo = usize::MAX;
        self.dirty_hi = None;

        let instance_count = self.instance_count();
        let visible = now < self.expire_at && instance_count > 0;
        FlushResult {
            dirty_range,
            instance_count,
            visible,
        }
    }
}

/// One evaluation of `PARTICLE_VERT`'s per-particle position/velocity/colour/
/// alpha (`particles.js:113-159`) at world time `now`, for slot `s` in
/// [`ParticleLayer`]. `None` when the particle is not alive yet or has
/// expired (`t < 0.0 || n >= 1.0`, `particles.js:113-121`) — the vertex
/// shader's early-return-behind-the-far-plane branch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSample {
    pub pos: (f64, f64, f64),
    pub vel: (f64, f64, f64),
    pub size: f64,
    /// `vCol.rgb` — the interpolated colour already multiplied by the
    /// interpolated intensity (and, for a spark, the flicker term).
    pub color: (f64, f64, f64),
    /// `vCol.a`.
    pub alpha: f64,
    /// Normalised age `n = t / life`, in `[0, 1)`.
    pub age: f64,
}

/// Read the raw record written by [`ParticleLayer::emit`] back out of
/// [`ParticleLayer::raw`], and evaluate [`integrate`] against it — the
/// documented split from the module doc: the *record* is golden-captured,
/// the *integration formula* is pinned by property test.
pub fn integrate(layer: &ParticleLayer, slot: usize, now: f64) -> Option<ParticleSample> {
    let b = slot * STRIDE;
    let a = layer.raw();
    integrate_record(
        [a[b + O_PS] as f64, a[b + O_PS + 1] as f64, a[b + O_PS + 2] as f64, a[b + O_PS + 3] as f64],
        [a[b + O_VS] as f64, a[b + O_VS + 1] as f64, a[b + O_VS + 2] as f64, a[b + O_VS + 3] as f64],
        [a[b + O_LF] as f64, a[b + O_LF + 1] as f64, a[b + O_LF + 2] as f64, a[b + O_LF + 3] as f64],
        [a[b + O_RT] as f64, a[b + O_RT + 1] as f64, a[b + O_RT + 2] as f64, a[b + O_RT + 3] as f64],
        [a[b + O_C0] as f64, a[b + O_C0 + 1] as f64, a[b + O_C0 + 2] as f64, a[b + O_C0 + 3] as f64],
        [a[b + O_C1] as f64, a[b + O_C1 + 1] as f64, a[b + O_C1 + 2] as f64, a[b + O_C1 + 3] as f64],
        [a[b + O_MS] as f64, a[b + O_MS + 1] as f64, a[b + O_MS + 2] as f64, a[b + O_MS + 3] as f64],
        [a[b + O_EX] as f64, a[b + O_EX + 1] as f64, a[b + O_EX + 2] as f64, a[b + O_EX + 3] as f64],
        now,
    )
}

/// `PARTICLE_VERT`'s `main()` (`particles.js:100-159`), taking the eight
/// `aPS/aVS/aLife/aRot/aCol0/aCol1/aMisc/aExtra` attributes directly instead
/// of via a live `InterleavedBufferAttribute` read.
#[allow(clippy::too_many_arguments)]
fn integrate_record(
    a_ps: [f64; 4],
    a_vs: [f64; 4],
    a_life: [f64; 4],
    a_rot: [f64; 4],
    a_col0: [f64; 4],
    a_col1: [f64; 4],
    a_misc: [f64; 4],
    a_extra: [f64; 4],
    now: f64,
) -> Option<ParticleSample> {
    let t = now - a_life[0];
    let n = t * a_life[1];
    if t < 0.0 || n >= 1.0 {
        return None;
    }

    let k = a_life[2].max(0.02);
    let e = (-k * t).exp();
    let gk = (0.0, a_life[3] / k, 0.0);
    let wpos0 = (
        a_ps[0] + (a_vs[0] - gk.0) * ((1.0 - e) / k) + gk.0 * t,
        a_ps[1] + (a_vs[1] - gk.1) * ((1.0 - e) / k) + gk.1 * t,
        a_ps[2] + (a_vs[2] - gk.2) * ((1.0 - e) / k) + gk.2 * t,
    );
    let wvel0 = (
        a_vs[0] * e + gk.0 * (1.0 - e),
        a_vs[1] * e + gk.1 * (1.0 - e),
        a_vs[2] * e + gk.2 * (1.0 - e),
    );

    // Turbulence, `particles.js:123-130`.
    let ph = a_extra[2] * std::f64::consts::PI * 2.0;
    let f = a_extra[1];
    let grow = crate::fx::noise::smoothstep(0.0, 0.4, n);
    let amp = a_extra[0] * grow;
    let wpos = (
        wpos0.0 + (t * f * 1.13 + ph).sin() * amp,
        wpos0.1 + (t * f * 0.79 + ph * 2.1).sin() * amp,
        wpos0.2 + (t * f * 1.31 + ph * 1.7).cos() * amp,
    );
    let wvel = (
        wvel0.0 + (t * f * 1.13 + ph).cos() * (amp * f),
        wvel0.1 + (t * f * 0.79 + ph * 2.1).cos() * (amp * f),
        wvel0.2 - (t * f * 1.31 + ph * 1.7).sin() * (amp * f),
    );

    // Size, `particles.js:136`.
    let size = crate::fx::util::lerp(a_ps[3], a_vs[3], n.powf(a_rot[3].max(0.02)));

    // Colour + alpha, `particles.js:161-166`.
    let col = (
        crate::fx::util::lerp(a_col0[0], a_col1[0], n),
        crate::fx::util::lerp(a_col0[1], a_col1[1], n),
        crate::fx::util::lerp(a_col0[2], a_col1[2], n),
    );
    let mut inten = crate::fx::util::lerp(a_col0[3], a_col1[3], n * n);
    if a_extra[3] > 0.5 {
        inten *= 0.72 + 0.28 * (t * 63.0 + ph * 9.0).sin();
    }
    let alpha = a_misc[2] * (1.0 - n).max(0.0).powf(a_misc[3].max(0.02)) * crate::fx::noise::smoothstep(0.0, 0.045, n);

    Some(ParticleSample {
        pos: wpos,
        vel: wvel,
        size,
        color: (col.0 * inten, col.1 * inten, col.2 * inten),
        alpha,
        age: n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression the `instance_count` bound exists for.
    ///
    /// `integrate` **cannot** tell an unwritten slot from a live one, and that
    /// is faithful: neither can `PARTICLE_VERT`. A zero-filled record has
    /// `birth = 0` and `1/life = 0`, so `t = now` and `n = t * 0 = 0`, and the
    /// shader's `t < 0.0 || n >= 1.0` early-out (`particles.js:99`) passes it
    /// straight through. The source is protected by `geometry.instanceCount`
    /// (`particles.js:430`), never by the shader. This pins that the port keeps
    /// that bound available and honest.
    #[test]
    fn an_unemitted_slot_still_integrates_as_a_live_particle() {
        let mut l = ParticleLayer::new(64, ParticleMode::Lit);
        assert_eq!(l.instance_count(), 0, "an untouched layer draws nothing");

        let s = ParticleSpawn { life: 1.0, ..ParticleSpawn::default() };
        l.emit(&s, 0.0);
        assert_eq!(l.instance_count(), 1, "one emit, one drawable instance");

        // Slot 0 is a real particle, halfway through its life.
        let real = integrate(&l, 0, 0.5).expect("slot 0 was emitted and is alive");
        assert!(real.size > 0.0 && real.alpha > 0.0);

        // Slot 1 was never written — and integrates as "alive" anyway. This is
        // the phantom: at the world origin, with no extent and no alpha.
        let phantom = integrate(&l, 1, 0.5).expect(
            "an unwritten slot passes the shader's own early-out; if this ever \
             returns None the bound below is no longer what protects callers",
        );
        assert_eq!(phantom.pos, (0.0, 0.0, 0.0));
        assert_eq!(phantom.size, 0.0);
        assert_eq!(phantom.alpha, 0.0);

        // So a caller that respects the bound never sees it, and one that
        // walks `capacity` sees 63 of them.
        assert_eq!(
            (0..l.instance_count())
                .filter(|&i| integrate(&l, i, 0.5).is_some())
                .count(),
            1
        );
        assert_eq!(
            (0..l.capacity)
                .filter(|&i| integrate(&l, i, 0.5).is_some())
                .count(),
            64
        );
    }

    /// A particle is invisible on its own birth frame, by construction:
    /// `smoothstep( 0.0, 0.045, 0.0 )` is zero (`particles.js:159`) and the
    /// fragment shader discards on it (`particles.js:188`). Any readback that
    /// reports drawable particles must apply [`DISCARD_ALPHA`].
    #[test]
    fn a_particle_has_no_alpha_at_the_instant_it_is_emitted() {
        let mut l = ParticleLayer::new(16, ParticleMode::Lit);
        let s = ParticleSpawn { life: 1.0, alpha: 1.0, ..ParticleSpawn::default() };
        l.emit(&s, 4.0);

        let born = integrate(&l, 0, 4.0).expect("it exists at its own birth time");
        assert_eq!(born.alpha, 0.0, "the fade-in starts at exactly zero");
        assert!(born.alpha < DISCARD_ALPHA, "and so the source discards it");

        // One 60 Hz frame later it is genuinely on screen.
        let next = integrate(&l, 0, 4.0 + 1.0 / 60.0).expect("still alive");
        assert!(next.alpha > DISCARD_ALPHA, "the fade-in has started");
    }

    #[test]
    fn capacity_is_clamped_to_a_minimum_of_sixteen() {
        let l = ParticleLayer::new(4, ParticleMode::Additive);
        assert_eq!(l.capacity, 16);
    }

    #[test]
    fn emit_writes_the_expected_record_layout() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let mut s = reset_spawn();
        s.x = 1.0;
        s.y = 2.0;
        s.z = 3.0;
        s.size0 = 0.5;
        let slot = l.emit(&s, 10.0);
        assert_eq!(slot, 0);
        let raw = l.raw();
        assert_eq!(raw[O_PS], 1.0);
        assert_eq!(raw[O_PS + 1], 2.0);
        assert_eq!(raw[O_PS + 2], 3.0);
        assert_eq!(raw[O_PS + 3], 0.5);
        assert_eq!(raw[O_LF], 10.0); // birth == now (no delay)
    }

    #[test]
    fn cursor_wraps_and_marks_wrapped() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let s = reset_spawn();
        // `cursor = i + 1; if cursor >= capacity { cursor = 0; wrapped = true; }`
        // (`particles.js:318-321`) fires on the write that fills the last
        // slot, so `wrapped` is already true after exactly `capacity` emits.
        for _ in 0..15 {
            l.emit(&s, 0.0);
        }
        assert!(!l.wrapped);
        l.emit(&s, 0.0);
        assert!(l.wrapped);
        assert_eq!(l.cursor, 0);
        l.emit(&s, 0.0);
        assert_eq!(l.cursor, 1);
    }

    #[test]
    fn flush_reports_the_dirty_range_and_clears_it() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let s = reset_spawn();
        l.emit(&s, 0.0);
        l.emit(&s, 0.0);
        let r = l.flush(0.0);
        assert_eq!(r.dirty_range, Some((0, 2 * STRIDE)));
        let r2 = l.flush(0.0);
        assert_eq!(r2.dirty_range, None);
    }

    #[test]
    fn flush_visible_tracks_expire_at() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let mut s = reset_spawn();
        s.life = 1.0;
        l.emit(&s, 0.0);
        assert!(l.flush(0.5).visible);
        assert!(!l.flush(2.0).visible);
    }

    #[test]
    fn pool_never_exceeds_its_capacity() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let s = reset_spawn();
        for i in 0..1000 {
            l.emit(&s, i as f64);
        }
        let r = l.flush(1000.0);
        assert!(r.instance_count <= l.capacity);
        assert_eq!(r.instance_count, l.capacity);
    }

    #[test]
    fn integrate_is_none_before_birth_and_after_death() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let mut s = reset_spawn();
        s.delay = 1.0;
        s.life = 0.5;
        l.emit(&s, 0.0);
        assert!(integrate(&l, 0, 0.5).is_none()); // before birth (birth=1.0)
        assert!(integrate(&l, 0, 1.0).is_some());
        assert!(integrate(&l, 0, 1.6).is_none()); // after death
    }

    #[test]
    fn integrate_starts_exactly_at_the_spawn_position_and_velocity() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let mut s = reset_spawn();
        s.x = 1.0;
        s.y = 2.0;
        s.z = 3.0;
        s.vx = 0.5;
        s.vy = -0.5;
        s.vz = 0.1;
        s.drag = 2.0;
        s.gravity = -9.0;
        s.life = 1.0;
        l.emit(&s, 0.0);
        let sample = integrate(&l, 0, 0.0).unwrap();
        assert!((sample.pos.0 - 1.0).abs() < 1e-6);
        assert!((sample.pos.1 - 2.0).abs() < 1e-6);
        assert!((sample.pos.2 - 3.0).abs() < 1e-6);
        assert!((sample.vel.0 - 0.5).abs() < 1e-6);
        assert!((sample.vel.1 - (-0.5)).abs() < 1e-6);
    }

    /// Cross-checks [`integrate`]'s closed form against small-step
    /// semi-implicit Euler integration of the same
    /// `dv/dt = -k v + g, dx/dt = v` ODE the source's module doc states
    /// (`particles.js:9-14`) — the property-test pin described in the module
    /// doc, standing in for a golden capture the GLSL-only source has no
    /// JavaScript function to provide.
    #[test]
    fn integrate_matches_numerical_integration_of_the_stated_ode() {
        let mut l = ParticleLayer::new(16, ParticleMode::Additive);
        let mut s = reset_spawn();
        s.x = 0.0;
        s.y = 0.0;
        s.z = 0.0;
        s.vx = 3.0;
        s.vy = 1.0;
        s.vz = -0.5;
        s.drag = 2.5;
        s.gravity = -9.81;
        s.life = 1.0;
        l.emit(&s, 0.0);

        let k = s.drag;
        let g = s.gravity;
        let steps = 200_000;
        let t_end = 0.4;
        let dt = t_end / steps as f64;
        let (mut vx, mut vy, mut vz) = (s.vx, s.vy, s.vz);
        let (mut px, mut py, mut pz) = (s.x, s.y, s.z);
        for _ in 0..steps {
            // semi-implicit Euler: dv/dt = -k v + g (g only on y)
            vx += (-k * vx) * dt;
            vy += (-k * vy + g) * dt;
            vz += (-k * vz) * dt;
            px += vx * dt;
            py += vy * dt;
            pz += vz * dt;
        }

        let sample = integrate(&l, 0, t_end).unwrap();
        assert!((sample.pos.0 - px).abs() < 1e-3, "x: {} vs {}", sample.pos.0, px);
        assert!((sample.pos.1 - py).abs() < 1e-3, "y: {} vs {}", sample.pos.1, py);
        assert!((sample.pos.2 - pz).abs() < 1e-3, "z: {} vs {}", sample.pos.2, pz);
        assert!((sample.vel.0 - vx).abs() < 1e-3);
        assert!((sample.vel.1 - vy).abs() < 1e-3);
    }
}
