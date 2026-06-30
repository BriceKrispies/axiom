//! The presentation-only particle system (§10.1): the live [`ParticleField`] the
//! [`crate::Draw2dApi`] steps on the **presentation** clock and the per-particle
//! integration/fade that produces its drawn quads.
//!
//! Everything here is private to the module. There is **no** public getter that
//! returns particle state — particles are visual only and must never re-enter
//! sim (SPEC-04 §6, §17.5). The field is deterministic *as a function* of its
//! inputs: the same emitter configs, the same emit calls, and the same
//! presentation-`dt` stream always produce a byte-identical set of quads, so two
//! runs hash equal. Per-particle variation comes from a seeded
//! [`DeterministicRng`] derived purely from the emit call index — never from a
//! clock or ambient entropy.

use axiom_host::Rgba;
use axiom_kernel::{DeterministicRng, Meters, Ratio, Seconds};
use axiom_math::Vec2;

use crate::ids::{EmitterConfig, EmitterId, Range};

/// A floor on a particle's lifetime so the `age / lifetime` fade is always a
/// finite division (a zero-lifetime config can never divide by zero).
const MIN_LIFETIME: f32 = 1.0e-6;

/// The jitter draw resolution: a uniform `0..2000` reduced to a signed `[-1, 1]`
/// factor about its midpoint.
const JITTER_SPAN: u64 = 2001;
const JITTER_MID: f32 = 1000.0;

/// The resolution of an in-range pick: a uniform integer draw in `0..PICK_SPAN`
/// normalized to a `t ∈ [0, 1)` fraction. Large enough that distinct draws map to
/// distinct fractions; the open upper end keeps the lerp inside `[min, max)` for a
/// non-degenerate range (and exactly `min` for a degenerate `[v, v]`).
const PICK_SPAN: u64 = 1 << 24;

/// Mixes the emitter id into the per-emit seed so two emitters firing on the same
/// call index still diverge.
const EMIT_SEED_MIX: u64 = 0x9E37_79B9_7F4A_7C15;

/// A kernel quantity (a finite-scalar newtype) a [`Range`] can be picked across:
/// it exposes its raw scalar and a **total** reconstruction from a computed one.
/// Private to the module — the lint's no-naked-`f32` rule is a *public*-API rule,
/// and this is the internal seam the deterministic pick lerps through, in one
/// place, for every ranged emitter field.
trait RangeQuantity: Copy {
    /// The underlying finite scalar.
    fn scalar(self) -> f32;
    /// Reconstruct from a computed scalar, total (non-finite ⇒ zero).
    fn from_scalar(value: f32) -> Self;
}

impl RangeQuantity for Seconds {
    fn scalar(self) -> f32 {
        self.get()
    }
    fn from_scalar(value: f32) -> Self {
        Seconds::finite_or_zero(value)
    }
}

impl RangeQuantity for Meters {
    fn scalar(self) -> f32 {
        self.get()
    }
    fn from_scalar(value: f32) -> Self {
        Meters::finite_or_zero(value)
    }
}

/// A fresh `t ∈ [0, 1)` fraction drawn from the deterministic generator — the
/// per-particle pick parameter. Pure arithmetic over one integer draw.
fn pick01(rng: &mut DeterministicRng) -> f32 {
    rng.next_bounded(PICK_SPAN) as f32 / PICK_SPAN as f32
}

/// The value `range.min + t * (range.max - range.min)` — a branchless arithmetic
/// lerp picked at fraction `t`. A degenerate `[v, v]` yields exactly `v` for any
/// `t` (the span is zero); a `[min, max]` with `t ∈ [0, 1)` stays inside
/// `[min, max)`. The single source of the deterministic in-range pick, shared by
/// every ranged emitter field.
fn pick<T: RangeQuantity>(range: Range<T>, t: f32) -> T {
    let lo = range.min().scalar();
    let hi = range.max().scalar();
    T::from_scalar(lo + (hi - lo) * t)
}

/// One live particle. Carries everything its integration and fade need, so a step
/// is a pure function of the particle and `dt`. Private: never crosses the facade.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Particle {
    position: Vec2,
    velocity: Vec2,
    gravity: Vec2,
    age: f32,
    lifetime: f32,
    size: Meters,
    color_start: Rgba,
    color_end: Rgba,
    layer: i32,
}

impl Particle {
    /// Advance one presentation step (semi-implicit Euler): apply gravity to the
    /// velocity, move by that updated velocity, and age by `dt`. Pure — returns
    /// the stepped particle.
    fn integrate(self, dt: f32) -> Particle {
        let velocity = self.velocity.add(self.gravity.mul_scalar(dt));
        Particle {
            velocity,
            position: self.position.add(velocity.mul_scalar(dt)),
            age: self.age + dt,
            ..self
        }
    }

    /// Whether the particle is still within its lifetime (the cull predicate).
    fn is_alive(&self) -> bool {
        self.age < self.lifetime
    }

    /// Resolve the particle to its drawn quad, fading `color_start` → `color_end`
    /// by normalized age.
    fn quad(&self) -> ParticleQuad {
        ParticleQuad {
            center: self.position,
            size: self.size,
            color: lerp_rgba(self.color_start, self.color_end, self.age / self.lifetime),
            layer: self.layer,
        }
    }
}

/// A resolved particle quad handed back to the builder to append as a
/// `KIND_PARTICLE_QUAD` command. Crate-private (presentation-only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ParticleQuad {
    pub(crate) center: Vec2,
    pub(crate) size: Meters,
    pub(crate) color: Rgba,
    pub(crate) layer: i32,
}

/// Lerp two colours channel-wise by `t`, sanitizing each computed channel through
/// the kernel's total [`Ratio::finite_or_zero`].
fn lerp_rgba(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba::new(
        lerp_ratio(a.r, b.r, t),
        lerp_ratio(a.g, b.g, t),
        lerp_ratio(a.b, b.b, t),
        lerp_ratio(a.a, b.a, t),
    )
}

/// Linear interpolation of two ratios, total via [`Ratio::finite_or_zero`].
fn lerp_ratio(a: Ratio, b: Ratio, t: f32) -> Ratio {
    Ratio::finite_or_zero(a.get() + (b.get() - a.get()) * t)
}

/// Spawn one particle from `config` at `at`, flying along `direction`. Each of
/// the particle's `speed`, `lifetime`, and `size` is **picked deterministically**
/// in its `[min, max]` [`Range`] from `rng` (the seed is a pure function of the
/// emit call index + emitter id), then the perpendicular jitter is drawn — so a
/// burst's particles vary within their ranges yet a replay of the same emit
/// reproduces them byte-for-byte. The draw order (speed, lifetime, size, jitter)
/// is fixed, which is what makes the sequence reproducible.
fn spawn(config: &EmitterConfig, at: Vec2, direction: Vec2, rng: &mut DeterministicRng) -> Particle {
    let speed = pick(config.speed, pick01(rng)).get();
    let lifetime = pick(config.lifetime, pick01(rng)).get();
    let size = pick(config.size, pick01(rng));
    let along = direction.mul_scalar(speed);
    let perpendicular = Vec2::new(-direction.y, direction.x);
    let signed = (rng.next_bounded(JITTER_SPAN) as f32 - JITTER_MID) / JITTER_MID;
    let jitter = config.spread.get() * speed * signed;
    Particle {
        position: at,
        velocity: along.add(perpendicular.mul_scalar(jitter)),
        gravity: config.gravity,
        age: 0.0,
        lifetime: lifetime.max(MIN_LIFETIME),
        size,
        color_start: config.color_start,
        color_end: config.color_end,
        layer: config.layer,
    }
}

/// The live particle set plus its registered emitters. Stepped branchlessly:
/// emit appends a burst, [`Self::advance`] integrates + culls, and [`Self::quads`]
/// resolves the survivors. Deterministic as a function of its inputs.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ParticleField {
    emitters: Vec<EmitterConfig>,
    particles: Vec<Particle>,
    emit_count: u64,
}

impl ParticleField {
    /// Register an emitter description, returning its [`EmitterId`].
    pub(crate) fn create_emitter(&mut self, config: EmitterConfig) -> EmitterId {
        let id = EmitterId::from_raw(self.emitters.len() as u32);
        self.emitters.push(config);
        id
    }

    /// Spawn a burst from emitter `id` at `at` flying along `direction`. An
    /// unknown id is a no-op (the `get` yields `None`). The per-emit seed is a
    /// pure function of the call index and the id, so replaying the same calls
    /// reproduces the identical burst.
    pub(crate) fn emit(&mut self, id: EmitterId, at: Vec2, direction: Vec2) {
        let seed = self.emit_count ^ u64::from(id.raw()).wrapping_mul(EMIT_SEED_MIX);
        self.emit_count += 1;
        let spawned = self.emitters.get(id.raw() as usize).copied().map(|config| {
            let mut rng = DeterministicRng::seeded(seed);
            (0..config.count)
                .map(|_| spawn(&config, at, direction, &mut rng))
                .collect::<Vec<Particle>>()
        });
        spawned.into_iter().flatten().for_each(|p| self.particles.push(p));
    }

    /// Advance every live particle by the presentation delta `dt` and cull the
    /// dead (age past lifetime).
    pub(crate) fn advance(&mut self, dt: Seconds) {
        let step = dt.get();
        self.particles
            .iter_mut()
            .for_each(|p| *p = p.integrate(step));
        self.particles.retain(Particle::is_alive);
    }

    /// Resolve the live particles to their drawn quads, in spawn order.
    pub(crate) fn quads(&self) -> Vec<ParticleQuad> {
        self.particles.iter().map(Particle::quad).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn seconds(v: f32) -> Seconds {
        Seconds::new(v).unwrap()
    }

    fn rgba(r: f32, a: f32) -> Rgba {
        Rgba::new(ratio(r), ratio(0.0), ratio(0.0), ratio(a))
    }

    fn config(count: u32, spread: f32) -> EmitterConfig {
        EmitterConfig {
            count,
            lifetime: Range::exact(seconds(2.0)),
            speed: Range::exact(meters(10.0)),
            spread: ratio(spread),
            gravity: Vec2::new(0.0, -4.0),
            size: Range::exact(meters(0.5)),
            color_start: rgba(1.0, 1.0),
            color_end: rgba(0.0, 0.0),
            layer: 1,
        }
    }

    /// A config whose `size` spans a real `[min, max]` range, so a burst's
    /// particles pick distinct, in-range quad sizes.
    fn sized_config(count: u32, size_min: f32, size_max: f32) -> EmitterConfig {
        EmitterConfig {
            size: Range::new(meters(size_min), meters(size_max)),
            ..config(count, 0.0)
        }
    }

    #[test]
    fn create_emitter_mints_incrementing_ids() {
        let mut field = ParticleField::default();
        assert_eq!(field.create_emitter(config(4, 0.0)), EmitterId::from_raw(0));
        assert_eq!(field.create_emitter(config(4, 0.0)), EmitterId::from_raw(1));
    }

    #[test]
    fn emit_spawns_count_particles_and_unknown_id_is_a_noop() {
        let mut field = ParticleField::default();
        let id = field.create_emitter(config(5, 0.0));
        field.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        assert_eq!(field.quads().len(), 5);
        // An unknown emitter id spawns nothing (the None arm of the lookup).
        field.emit(EmitterId::from_raw(99), Vec2::ZERO, Vec2::new(1.0, 0.0));
        assert_eq!(field.quads().len(), 5);
    }

    #[test]
    fn advance_integrates_position_and_velocity_then_fades_color() {
        let mut field = ParticleField::default();
        // A clean jet (no spread) flying +x at 10 m/s, gravity pulling -y.
        let id = field.create_emitter(config(1, 0.0));
        field.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        let birth = field.quads();
        assert_eq!(birth[0].center, Vec2::ZERO);
        // Colour starts at color_start.
        assert_eq!(birth[0].color, rgba(1.0, 1.0));

        field.advance(seconds(1.0));
        let after = field.quads();
        // Position advanced +x by velocity*dt; gravity bent velocity so y moved.
        assert_eq!(after[0].center.x, 10.0);
        assert!(after[0].center.y < 0.0, "gravity pulled the particle down");
        // Half-way through a 2s life: colour faded half-way toward color_end.
        assert_eq!(after[0].color, rgba(0.5, 0.5));
        assert_eq!(after[0].size, meters(0.5));
        assert_eq!(after[0].layer, 1);
    }

    #[test]
    fn advance_culls_particles_past_their_lifetime() {
        let mut field = ParticleField::default();
        let id = field.create_emitter(config(3, 0.0));
        field.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        assert_eq!(field.quads().len(), 3);
        // Step past the 2s lifetime: every particle is culled.
        field.advance(seconds(3.0));
        assert_eq!(field.quads().len(), 0);
    }

    #[test]
    fn spread_perturbs_velocity_while_a_clean_jet_does_not() {
        // With spread, two particles in a burst get distinct (jittered) velocities,
        // so after a step their positions differ.
        let mut spread_field = ParticleField::default();
        let s = spread_field.create_emitter(config(2, 0.5));
        spread_field.emit(s, Vec2::ZERO, Vec2::new(1.0, 0.0));
        spread_field.advance(seconds(1.0));
        let spread_quads = spread_field.quads();
        assert_ne!(spread_quads[0].center, spread_quads[1].center);

        // With no spread, the whole burst shares one velocity → identical centers.
        let mut clean_field = ParticleField::default();
        let c = clean_field.create_emitter(config(2, 0.0));
        clean_field.emit(c, Vec2::ZERO, Vec2::new(1.0, 0.0));
        clean_field.advance(seconds(1.0));
        let clean_quads = clean_field.quads();
        assert_eq!(clean_quads[0].center, clean_quads[1].center);
    }

    #[test]
    fn ranged_size_picks_distinct_in_range_values() {
        // §10.1 ranged field: a burst from a `[0.5, 1.5]` size range gives each
        // particle a deterministic in-range size — varied, never out of bounds.
        let mut field = ParticleField::default();
        let id = field.create_emitter(sized_config(8, 0.5, 1.5));
        field.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        let sizes: Vec<f32> = field.quads().iter().map(|q| q.size.get()).collect();
        // Every pick lands inside `[min, max)`.
        assert!(
            sizes.iter().all(|&s| (0.5..1.5).contains(&s)),
            "every picked size is in range: {sizes:?}"
        );
        // The picks actually vary (not all the same value) — the range is used.
        let first = sizes[0];
        assert!(sizes.iter().any(|&s| s != first), "sizes vary across the burst: {sizes:?}");
    }

    #[test]
    fn degenerate_range_picks_exactly_the_fixed_value() {
        // A `[v, v]` range yields exactly `v` for every particle, whatever the rng
        // draws — the backward-compatible scalar form (`Range::exact`).
        let mut field = ParticleField::default();
        let id = field.create_emitter(sized_config(5, 0.75, 0.75));
        field.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        assert!(field.quads().iter().all(|q| q.size == meters(0.75)));
    }

    #[test]
    fn ranged_picks_replay_byte_identically() {
        // §6 determinism-as-function: the same emit reproduces the identical set
        // of in-range picks (sizes here) on a second run.
        let run = || {
            let mut field = ParticleField::default();
            let id = field.create_emitter(sized_config(8, 0.5, 1.5));
            field.emit(id, Vec2::new(1.0, 2.0), Vec2::new(0.0, 1.0));
            field.quads().iter().map(|q| q.size.get()).collect::<Vec<f32>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn ranged_speed_and_lifetime_vary_motion_and_fade() {
        // A `[5, 15]` speed range and `[1, 3]` lifetime range give two particles
        // distinct integrated positions (speed) and distinct fades (lifetime).
        let mut field = ParticleField::default();
        let recipe = EmitterConfig {
            speed: Range::new(meters(5.0), meters(15.0)),
            lifetime: Range::new(seconds(1.0), seconds(3.0)),
            ..config(2, 0.0)
        };
        let id = field.create_emitter(recipe);
        field.emit(id, Vec2::ZERO, Vec2::new(1.0, 0.0));
        field.advance(seconds(0.5));
        let quads = field.quads();
        // Distinct speeds ⇒ distinct +x travel; distinct lifetimes ⇒ distinct fade.
        assert_ne!(quads[0].center.x, quads[1].center.x);
        assert_ne!(quads[0].color, quads[1].color);
    }

    #[test]
    fn same_calls_reproduce_byte_identical_fields() {
        let build = || {
            let mut field = ParticleField::default();
            let id = field.create_emitter(config(6, 0.5));
            field.emit(id, Vec2::new(1.0, 2.0), Vec2::new(0.0, 1.0));
            field.advance(seconds(0.5));
            field
        };
        // Determinism: identical construction yields equal fields and quads.
        assert_eq!(build(), build());
        assert_eq!(build().quads(), build().quads());
    }
}
