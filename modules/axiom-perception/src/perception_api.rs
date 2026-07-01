//! [`PerceptionApi`] — the module's single public facade.
//!
//! It owns the game-agnostic sensor geometry and the neutral fact vocabulary. An
//! app casts the [`PerceptionApi::ray_fan`] directions against its own world,
//! culls candidate entities with [`PerceptionApi::in_view`], and packs the hits
//! into agent observation facts with the `*_fact` builders. Everything crosses
//! the boundary as primitives, dimensioned kernel quantities ([`Radians`] /
//! [`Meters`]), and `Vec3` geometry — never a foreign module type.
//!
//! The code is branchless (the engine spine invariant): conditional logic is
//! expressed as iterator/`Option` combinators and boolean algebra, never
//! `if`/`match`/`&&`.

use axiom_kernel::{Meters, Radians};
use axiom_math::{Quat, Vec3};

/// The reusable, game-agnostic perception facade.
///
/// Stateless: every method is a pure transform of its inputs.
#[derive(Debug)]
pub struct PerceptionApi;

impl PerceptionApi {
    // --- the neutral fact-kind vocabulary (the agent's `kind_code`) ---

    /// A ray probe struck something solid: `subject` = probe index, `x/y/z` = the
    /// world-space hit point (micro-units), `value` = the distance (micro-units).
    pub const FACT_OBSTACLE: u16 = 200;
    /// An entity is in view: `subject` = its stable id, `x/y/z` = its position
    /// (micro-units), `value` = its coarse kind code (what it *is*).
    pub const FACT_VISIBLE: u16 = 201;
    /// A tracked subject's motion: `subject` = its id, `x/y/z` = its per-tick
    /// velocity (micro-units), `value` = `0`.
    pub const FACT_TRACKED: u16 = 202;

    /// One micro-unit per millionth of a world unit — the fixed-point convention
    /// for the integer fact coordinates the agent consumes.
    const MICRO: f64 = 1_000_000.0;

    /// A world-unit `f32` as fixed-point micro-units.
    fn micro(value: f32) -> i64 {
        (f64::from(value) * Self::MICRO) as i64
    }

    /// Fixed-point micro-units back to a world-unit `f32` — the inverse of
    /// [`Self::micro`], used to reconstruct geometry when decoding a fact.
    fn from_micro(value: i64) -> f32 {
        (value as f64 / Self::MICRO) as f32
    }

    /// Generate `count` ray directions fanned **horizontally** (about world +Y)
    /// across the angular span `fov`, centred on `forward`. The app casts each
    /// against its world (scene raycast / heightfield march); the centre ray is
    /// the "am I facing something" probe. `count == 0` yields no rays.
    ///
    /// Directions sample the centres of `count` equal angular cells across `fov`,
    /// so the fan is symmetric about `forward` and a single ray lands dead centre.
    pub fn ray_fan(forward: Vec3, fov: Radians, count: u32) -> Vec<Vec3> {
        let span = fov.get();
        let half = span * 0.5;
        let n = count as f32;
        (0..count)
            .map(|i| {
                let angle = -half + (i as f32 + 0.5) * span / n;
                let h = angle * 0.5;
                Quat::new(0.0, h.sin(), 0.0, h.cos()).rotate(forward)
            })
            .collect()
    }

    /// Cull candidate `(id, world position)` pairs to those an agent at `eye`
    /// facing `forward` can see: within the forward cone of half-angle `fov/2`
    /// and within `range`. A zero/degenerate `forward` sees nothing.
    pub fn in_view(
        eye: Vec3,
        forward: Vec3,
        fov: Radians,
        range: Meters,
        candidates: &[(u32, Vec3)],
    ) -> Vec<(u32, Vec3)> {
        let cos_half = (fov.get() * 0.5).cos();
        let range2 = range.get() * range.get();
        forward.normalize().ok().map_or(Vec::new(), |facing| {
            candidates
                .iter()
                .copied()
                .filter(|(_, position)| {
                    let to = position.subtract(eye);
                    let dist2 = to.dot(to);
                    let in_range = dist2 <= range2;
                    // cos(angle) >= cos(half) without a divide: compare the dot
                    // against cos_half · ‖to‖ (a candidate at the eye, ‖to‖ = 0,
                    // satisfies 0 >= 0 and is "in view").
                    let in_cone = facing.dot(to) >= cos_half * dist2.sqrt();
                    in_range & in_cone
                })
                .collect()
        })
    }

    /// Build an **obstacle** fact from a ray probe's hit: which probe, where it
    /// struck, and how far. The agent reads `value` as "distance to the thing in
    /// front of me".
    pub fn obstacle_fact(
        probe_index: u32,
        hit_point: Vec3,
        distance: Meters,
    ) -> (u16, u32, i64, i64, i64, i64) {
        (
            Self::FACT_OBSTACLE,
            probe_index,
            Self::micro(hit_point.x),
            Self::micro(hit_point.y),
            Self::micro(hit_point.z),
            Self::micro(distance.get()),
        )
    }

    /// Build a **visible** fact from a seen entity: its stable id (so the agent
    /// tracks it across ticks), its position, and its coarse kind (what it is).
    pub fn visible_fact(
        subject_id: u32,
        position: Vec3,
        kind_code: u32,
    ) -> (u16, u32, i64, i64, i64, i64) {
        (
            Self::FACT_VISIBLE,
            subject_id,
            Self::micro(position.x),
            Self::micro(position.y),
            Self::micro(position.z),
            i64::from(kind_code),
        )
    }

    /// Decode an **obstacle** fact (the inverse of [`Self::obstacle_fact`]) into
    /// `(probe_index, hit_point, distance)` — the producer owning its own tuple
    /// layout so a consumer never re-derives the encoding. `None` for any fact of
    /// another kind, so a caller can `filter_map` a mixed fact stream.
    pub fn decode_obstacle(fact: (u16, u32, i64, i64, i64, i64)) -> Option<(u32, Vec3, Meters)> {
        (fact.0 == Self::FACT_OBSTACLE).then(|| {
            (
                fact.1,
                Vec3::new(
                    Self::from_micro(fact.2),
                    Self::from_micro(fact.3),
                    Self::from_micro(fact.4),
                ),
                Meters::finite_or_zero(Self::from_micro(fact.5)),
            )
        })
    }

    /// Decode a **visible** fact (the inverse of [`Self::visible_fact`]) into
    /// `(subject_id, position, value)`. The raw `value` is returned uninterpreted —
    /// its coarse-kind meaning is the caller's vocabulary, not the sensor's. `None`
    /// for any fact of another kind.
    pub fn decode_visible(fact: (u16, u32, i64, i64, i64, i64)) -> Option<(u32, Vec3, u32)> {
        (fact.0 == Self::FACT_VISIBLE).then(|| {
            (
                fact.1,
                Vec3::new(
                    Self::from_micro(fact.2),
                    Self::from_micro(fact.3),
                    Self::from_micro(fact.4),
                ),
                fact.5 as u32,
            )
        })
    }

    /// The whole sensor cycle behind one call: fan the ray probes, cull the
    /// landmarks to the view cone, and assemble the neutral facts — the module
    /// owning the orchestration DOOM and growth used to hand-roll. The game passes
    /// only a **probe** (`probe(dir) -> Some((distance, hit_point))` when that
    /// direction strikes its world — a scene raycast or a heightfield march) and
    /// its **landmarks** (`(subject_id, world position, coarse kind code)`); the
    /// kind is passed through untouched into each visible fact's `value`.
    ///
    /// Landmarks are flattened to the eye height for the cone test, so a towering
    /// nearby summit is not pushed out of view by its own altitude, yet each
    /// visible fact still carries the landmark's **true** position.
    pub fn sense_with_probe(
        eye: Vec3,
        forward: Vec3,
        fov: Radians,
        range: Meters,
        count: u32,
        probe: impl Fn(Vec3) -> Option<(Meters, Vec3)>,
        landmarks: &[(u32, Vec3, u32)],
    ) -> Vec<(u16, u32, i64, i64, i64, i64)> {
        // Ray-fan probes: each direction that strikes the world is an obstacle fact.
        let obstacles = Self::ray_fan(forward, fov, count)
            .into_iter()
            .enumerate()
            .filter_map(|(index, dir)| {
                probe(dir).map(|(distance, hit)| Self::obstacle_fact(index as u32, hit, distance))
            });

        // Landmarks culled on their horizontal bearing (altitude flattened to the
        // eye) then emitted with their true position + pass-through kind.
        let flattened: Vec<(u32, Vec3)> = landmarks
            .iter()
            .map(|&(id, pos, _)| (id, Vec3::new(pos.x, eye.y, pos.z)))
            .collect();
        let visible = Self::in_view(eye, forward, fov, range, &flattened)
            .into_iter()
            .filter_map(|(id, _)| {
                landmarks
                    .iter()
                    .find(|(landmark_id, ..)| *landmark_id == id)
                    .map(|&(landmark_id, pos, kind)| Self::visible_fact(landmark_id, pos, kind))
            });

        obstacles.chain(visible).collect()
    }

    /// The per-tick velocity of a tracked subject from its previous and current
    /// positions — the app stores the prior position (e.g. in agent memory) and
    /// passes it back next tick. Tracking falls out of the stable subject id.
    pub fn relative_motion(prior: Vec3, current: Vec3) -> Vec3 {
        current.subtract(prior)
    }

    /// Build a **tracked** fact carrying a subject's per-tick velocity.
    pub fn tracked_fact(subject_id: u32, velocity: Vec3) -> (u16, u32, i64, i64, i64, i64) {
        (
            Self::FACT_TRACKED,
            subject_id,
            Self::micro(velocity.x),
            Self::micro(velocity.y),
            Self::micro(velocity.z),
            0,
        )
    }

    /// The nearest probe hit among `(probe index, distance)` pairs — "what is the
    /// closest thing I'm facing, and how far". `None` if there were no hits.
    pub fn nearest_obstacle(probes: &[(u32, Meters)]) -> Option<(u32, Meters)> {
        probes
            .iter()
            .copied()
            .min_by(|a, b| a.1.get().total_cmp(&b.1.get()))
    }
}
