//! Live, game-agnostic perception for the growth agent — the **heightfield sense
//! adapter**. Native + `agent` feature only, like [`crate::growth::agent`].
//!
//! This is the agnosticism proof for `axiom-perception`. Growth's world is a
//! *procedural heightfield*: there is no scene and there are no entities. Yet the
//! agent senses it with the **same** module retro FPS uses — the same horizontal
//! ray-fan ([`PerceptionApi::ray_fan`]), the same view-cone cull
//! ([`PerceptionApi::in_view`]), and the same neutral `(kind, subject, x, y, z,
//! value)` fact vocabulary. Only the per-game *world probe* differs:
//!
//! * **retro FPS** casts each ray-fan direction against its engine scene
//!   (`raycast_hit`) — a wall is a bounded node.
//! * **Growth** *marches the terrain sampler* ([`GroundSim::ground_abs_at`])
//!   along each direction until the ground rises above the eye — a "wall" here is
//!   the mountain slope ahead.
//!
//! "Visible" entities are the world's named landmarks (the summit, the spawn) —
//! culled to the view cone exactly as retro FPS's enemies are. The facts are produced
//! purely with [`PerceptionApi`]; the [`Sight`] the demo prints is decoded back
//! out of those very facts, so the neutral contract is genuinely on the path.

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
/// are growth's "nouns" (the same role retro FPS's tagged enemies play): the entities
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
/// retro FPS's `raycast_hit`. `None` means open ground all the way to the sight range.
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

/// Decode the neutral facts back into a readable [`Sight`] — the consumer side of
/// the contract (what a brain reads off the observation). The tuple layout is
/// owned by `axiom-perception`; this only maps its decoded fields into the growth
/// app's own `Obstacle` / `Visible` vocabulary (and interprets the raw kind).
fn decode(facts: &[Fact]) -> Sight {
    let obstacles: Vec<Obstacle> = facts
        .iter()
        .filter_map(|&f| PerceptionApi::decode_obstacle(f))
        .map(|(probe, hit, distance)| Obstacle {
            probe,
            distance_m: distance.get(),
            height_m: hit.y,
        })
        .collect();
    let ahead = obstacles.iter().copied().find(|o| o.probe == RAY_COUNT / 2);
    let visible: Vec<Visible> = facts
        .iter()
        .filter_map(|&f| PerceptionApi::decode_visible(f))
        .map(|(subject, pos, kind)| Visible {
            subject,
            x: pos.x,
            z: pos.z,
            kind,
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
/// of the sensor orchestration — ray-fan, view-cone cull, fact assembly — is
/// `axiom-perception`'s [`PerceptionApi::sense_with_probe`]; growth supplies only
/// the heightfield **probe** ([`march`]) and its **landmarks**.
pub fn sense_sim(sim: &GroundSim) -> Sight {
    let (x, z, _yaw, _pitch) = sim.pose();
    let eye = Vec3::new(x, sim.eye_height_m(), z);
    let (fx, fz) = sim.forward_xz();
    let forward = Vec3::new(fx, 0.0, fz);
    let fov = Radians::new(FOV_RADIANS).expect("authored fov is finite");
    let reach = Meters::new(SIGHT_RANGE_M).expect("authored sight range is finite");
    let marks: Vec<(u32, Vec3, u32)> = landmarks(sim)
        .iter()
        .map(|m| (m.id, m.pos, m.kind))
        .collect();
    let facts = PerceptionApi::sense_with_probe(
        eye,
        forward,
        fov,
        reach,
        RAY_COUNT,
        |dir| {
            march(sim, eye, dir)
                .map(|(distance, point)| (Meters::new(distance).expect("a finite march distance"), point))
        },
        &marks,
    );
    decode(&facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::agent::AgentSession;

    #[test]
    fn climbing_the_agent_faces_the_slope_and_sees_the_mountaintop() {
        // Earthlike vista; the climb walks toward the summit. Perception must, at
        // some point, both face rising ground ahead (a slope "wall") and see the
        // mountaintop landmark classified as such — the same ray-fan + cone the
        // retro FPS agent uses, against a heightfield with no entities.
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
        // Climb a while, then assert every probe hit is a finite distance within
        // the sight range with a real terrain height — the geometry is sound.
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
