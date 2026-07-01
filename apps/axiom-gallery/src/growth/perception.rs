//! Live, game-agnostic perception for the growth agent — the heightfield sense
//! adapter. Native + `agent` feature only, like [`crate::growth::agent`].
//!
//! Growth's world is a procedural heightfield with no scene and no entities, so
//! it senses via the same [`PerceptionApi`] ray-fan/view-cone/fact vocabulary
//! DOOM uses, but marches the terrain sampler ([`GroundSim::ground_abs_at`])
//! along each ray instead of casting against a scene: a "wall" here is the
//! mountain slope ahead. Visible landmarks (the summit, the spawn) are culled to
//! the view cone the same way DOOM's enemies are.

use axiom_kernel::{Meters, Radians};
use axiom_math::Vec3;
use axiom_perception::PerceptionApi;

use crate::growth::ground::GroundSim;

/// Growth's landmark kind vocabulary — what a visible fact's `value` carries.
pub const KIND_MOUNTAINTOP: u32 = 10;
pub const KIND_SPAWN: u32 = 11;

/// The horizontal sight fan: a 90° field of view sampled by five rays (the middle
/// ray, index `RAY_COUNT / 2`, points dead ahead — the "am I facing a slope" probe).
const FOV_RADIANS: f32 = std::f32::consts::FRAC_PI_2;
const RAY_COUNT: u32 = 5;
/// How far the agent can see (world metres) — the vista is mountain-scale.
const SIGHT_RANGE_M: f32 = 5000.0;
/// The march step (metres) along each ray when probing the terrain.
const MARCH_STEP_M: f32 = 10.0;

/// A neutral observation fact in `axiom-agent`'s tuple shape, as [`PerceptionApi`]
/// builds it.
type Fact = (u16, u32, i64, i64, i64, i64);

/// One micro-unit per millionth of a metre — the fixed-point fact convention.
fn metres(micro: i64) -> f32 {
    micro as f32 / 1_000_000.0
}

/// A ray probe struck rising ground: which probe, how far ahead (metres), and the
/// absolute terrain height there (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    pub probe: u32,
    pub distance_m: f32,
    pub height_m: f32,
}

/// A landmark the agent can see: its stable subject id, its `(x, z)` position, and
/// its coarse kind (what it is).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Visible {
    pub subject: u32,
    pub x: f32,
    pub z: f32,
    pub kind: u32,
}

/// Everything the agent perceives this tick, decoded from the neutral facts: the
/// slope dead ahead (if any), every probe hit, and every visible landmark.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sight {
    pub ahead: Option<Obstacle>,
    pub obstacles: Vec<Obstacle>,
    pub visible: Vec<Visible>,
}

impl Sight {
    /// One human-readable line per perceived thing — what the demo prints so a
    /// person can watch the agent "see" the mountain.
    pub fn report_lines(&self) -> Vec<String> {
        let ahead = self
            .ahead
            .map(|o| {
                format!(
                    "facing a slope {:.0} m ahead (terrain rises to {:.0} m)",
                    o.distance_m, o.height_m
                )
            })
            .unwrap_or_else(|| "open vista ahead".to_string());
        let mut lines = vec![ahead];
        for v in &self.visible {
            let what = match v.kind {
                KIND_MOUNTAINTOP => "MOUNTAINTOP",
                KIND_SPAWN => "spawn",
                _ => "landmark",
            };
            lines.push(format!(
                "  sees {what} subject={} at ({:.0}, {:.0})",
                v.subject, v.x, v.z
            ));
        }
        lines
    }
}

/// A landmark candidate: its id, its true world position (with real height), and
/// its kind.
#[derive(Debug, Clone, Copy)]
struct Landmark {
    id: u32,
    pos: Vec3,
    kind: u32,
}

/// The world's named landmarks, in metres — the summit and the spawn shelf. These
/// are growth's "nouns" (the same role DOOM's tagged enemies play): the entities
/// the agent can recognise and report.
fn landmarks(sim: &GroundSim) -> Vec<Landmark> {
    let (peak_x, peak_z) = sim.peak_xz();
    let (spawn_x, spawn_z) = sim.spawn_xz();
    vec![
        Landmark {
            id: 0,
            pos: Vec3::new(peak_x, sim.peak_height_m(), peak_z),
            kind: KIND_MOUNTAINTOP,
        },
        Landmark {
            id: 1,
            pos: Vec3::new(spawn_x, sim.shelf_height_m(), spawn_z),
            kind: KIND_SPAWN,
        },
    ]
}

/// March the terrain sampler forward along `dir` from `eye` and return the first
/// sample where the ground rises to or above the eye line — the growth analogue of
/// DOOM's `raycast_hit`. `None` means open ground all the way to the sight range.
fn march(sim: &GroundSim, eye: Vec3, dir: Vec3) -> Option<(f32, Vec3)> {
    let steps = (SIGHT_RANGE_M / MARCH_STEP_M) as u32;
    (1..=steps).find_map(|i| {
        let distance = i as f32 * MARCH_STEP_M;
        let x = eye.x + dir.x * distance;
        let z = eye.z + dir.z * distance;
        let height = sim.ground_abs_at(x, z);
        (height >= eye.y).then_some((distance, Vec3::new(x, height, z)))
    })
}

/// Sense the heightfield through `axiom-perception`: cast the ray-fan (by marching
/// the terrain), cull the landmarks to the view cone, and produce the neutral
/// facts. Returns the facts (for any agent observation) — the [`Sight`] is decoded
/// from them by [`decode`].
fn sense(sim: &GroundSim, eye: Vec3, forward: Vec3) -> Vec<Fact> {
    let fov = Radians::new(FOV_RADIANS).expect("authored fov is finite");
    let reach = Meters::new(SIGHT_RANGE_M).expect("authored sight range is finite");
    let mut facts: Vec<Fact> = Vec::new();

    // Ray-fan probes: march the terrain along each direction; rising ground is a
    // geometric obstacle fact (probe + hit point + distance).
    for (probe, dir) in PerceptionApi::ray_fan(forward, fov, RAY_COUNT)
        .into_iter()
        .enumerate()
    {
        if let Some((distance, point)) = march(sim, eye, dir) {
            let metres = Meters::new(distance).expect("a finite march distance");
            facts.push(PerceptionApi::obstacle_fact(probe as u32, point, metres));
        }
    }

    // Visible landmarks: cull on the *horizontal* bearing (ground-project each to
    // the eye height, so a towering nearby summit is not pushed out of the cone by
    // its altitude), then emit a visible fact carrying its true position and kind.
    let marks = landmarks(sim);
    let projected: Vec<(u32, Vec3)> = marks
        .iter()
        .map(|m| (m.id, Vec3::new(m.pos.x, eye.y, m.pos.z)))
        .collect();
    for (id, _) in PerceptionApi::in_view(eye, forward, fov, reach, &projected) {
        if let Some(mark) = marks.iter().find(|m| m.id == id) {
            facts.push(PerceptionApi::visible_fact(id, mark.pos, mark.kind));
        }
    }
    facts
}

/// Decode the neutral facts back into a readable [`Sight`] — the consumer side of
/// the contract (what a brain reads off the observation).
fn decode(facts: &[Fact]) -> Sight {
    let obstacles: Vec<Obstacle> = facts
        .iter()
        .filter(|f| f.0 == PerceptionApi::FACT_OBSTACLE)
        .map(|f| Obstacle {
            probe: f.1,
            distance_m: metres(f.5),
            height_m: metres(f.3),
        })
        .collect();
    let ahead = obstacles.iter().copied().find(|o| o.probe == RAY_COUNT / 2);
    let visible: Vec<Visible> = facts
        .iter()
        .filter(|f| f.0 == PerceptionApi::FACT_VISIBLE)
        .map(|f| Visible {
            subject: f.1,
            x: metres(f.2),
            z: metres(f.4),
            kind: f.5 as u32,
        })
        .collect();
    Sight {
        ahead,
        obstacles,
        visible,
    }
}

/// Perceive a ground sim's world this tick: cast the ray-fan against the
/// heightfield and cull its landmarks, returning the decoded [`Sight`]. The whole
/// of the sensor model is `axiom-perception`; this only supplies the heightfield
/// probe.
pub fn sense_sim(sim: &GroundSim) -> Sight {
    let (x, z, _yaw, _pitch) = sim.pose();
    let eye = Vec3::new(x, sim.eye_height_m(), z);
    let (fx, fz) = sim.forward_xz();
    decode(&sense(sim, eye, Vec3::new(fx, 0.0, fz)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::agent::AgentSession;

    #[test]
    fn climbing_the_agent_faces_the_slope_and_sees_the_mountaintop() {
        let mut session = AgentSession::earthlike();
        let mut faced_slope = false;
        let mut saw_summit = false;
        for _ in 0..200 {
            session.step(&crate::growth::agent::Action::seek());
            let sight = session.sight();
            faced_slope |= sight.ahead.is_some();
            saw_summit |= sight
                .visible
                .iter()
                .any(|v| v.kind == KIND_MOUNTAINTOP);
        }
        assert!(faced_slope, "the agent faced a rising slope while climbing");
        assert!(saw_summit, "the agent saw and classified the mountaintop");
    }

    #[test]
    fn every_obstacle_has_a_real_distance_within_sight() {
        let mut session = AgentSession::earthlike();
        for _ in 0..40 {
            session.step(&crate::growth::agent::Action::seek());
        }
        let sight = session.sight();
        for o in &sight.obstacles {
            assert!(
                o.distance_m > 0.0 && o.distance_m <= SIGHT_RANGE_M,
                "probe {} distance {} out of range",
                o.probe,
                o.distance_m
            );
            assert!(o.height_m.is_finite(), "slope height is a real metre value");
        }
    }

    #[test]
    fn perception_is_deterministic() {
        let run = || {
            let mut s = AgentSession::earthlike();
            let mut report = String::new();
            for _ in 0..30 {
                s.step(&crate::growth::agent::Action::seek());
                for line in s.sight().report_lines() {
                    report.push_str(&line);
                    report.push('\n');
                }
            }
            report
        };
        assert_eq!(run(), run(), "same perception every run");
    }
}
