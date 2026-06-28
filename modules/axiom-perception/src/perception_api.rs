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
