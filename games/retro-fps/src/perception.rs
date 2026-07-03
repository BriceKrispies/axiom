//! Live, game-agnostic perception for the retro FPS agent — the app-side **sense
//! adapter**. Native + `agent` feature only, like [`crate::agent`].
//! The reusable sensor model and the neutral fact vocabulary live in the
//! `axiom-perception` module (game-agnostic: it owns only the ray-fan geometry,
//! the view-cone cull, the subject-tracking math, and the
//! `(kind, subject, x, y, z, value)` fact shape). This file is the **irreducible
//! per-game part**: it knows how to cast a probe against *retro FPS's* world (the
//! engine scene, via [`RunningApp::raycast_hit`]), how to enumerate retro FPS's enemy
//! candidates, and how to read what a hit *is* off its engine-native [`Tag`]. It
//! produces facts purely with [`PerceptionApi`] and feeds them through the same
//! `axiom-agent` `observe → decide → emit` loop the rest of the agent uses — so
//! the agent genuinely *sees* (a wall at a real distance) and *tracks* (a moving
//! enemy's per-tick velocity) with zero game-specific perception logic.
//! Classification is **entity-native**: each enemy carries a `Tag(KIND_ENEMY)`
//! on its engine node, so a raycast hit classifies itself (an untagged hit is
//! plain level geometry — a wall). This is the "move to the ECS thing": meaning
//! lives on the entity, not in an app-side lookup table.

use std::collections::BTreeMap;

use axiom::prelude::{Meters, RunningApp, Vec3};
use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Radians, Tick};
use axiom_perception::PerceptionApi;
use axiom_runtime::RuntimeStep;

use crate::agent::{
    control_code_of, retro_fps_drive_tick, intent_of_control_code, AGENT_RAW_ID, FIXED_DELTA_NANOS,
};
use crate::{build_retro_fps_app, level::LevelDoc, RetroFpsAssets, RetroFpsGame, Intent};

/// This game's coarse `Tag` kind vocabulary — the codes its entities carry. An
/// enemy node is tagged [`KIND_ENEMY`]; level geometry is left untagged, so a hit
/// with no tag is a wall.
pub const KIND_ENEMY: u32 = 2;
/// The kind a hit reports when nothing is tagged on it — plain level geometry.
pub const KIND_WALL: u32 = 0;

/// The horizontal sight fan: a 90° field of view sampled by five rays (the middle
/// ray, index `RAY_COUNT / 2`, points dead ahead — the "am I facing a wall" probe).
const FOV_RADIANS: f32 = std::f32::consts::FRAC_PI_2;
const RAY_COUNT: u32 = 5;
/// How far the agent can see (world metres).
const SIGHT_RANGE_M: f32 = 16.0;
/// A wall this close dead ahead makes the roaming agent turn away.
const WALL_AVOID_M: f32 = 1.0;
/// Half-angle (radians, small-angle ≈ sine) within which a visible enemy counts
/// as "lined up" — close enough to walk into and fire at rather than keep turning.
const ENGAGE_HALF_ANGLE: f32 = 0.18;

/// A ray probe struck something: which probe, how far (metres), and the kind
/// tagged on what it hit (`None` ⇒ untagged geometry, a wall).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    pub probe: u32,
    pub distance_m: f32,
    pub kind: Option<u32>,
}

/// An entity the agent can see: its stable subject id, its `(x, z)` position, and
/// its coarse kind (what it is).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Visible {
    pub subject: u32,
    pub x: f32,
    pub z: f32,
    pub kind: u32,
}

/// A tracked subject's per-tick velocity in the ground plane — the motion the
/// agent infers from the same subject's position last tick versus this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracked {
    pub subject: u32,
    pub vx: f32,
    pub vz: f32,
}

/// Everything the agent perceives this tick, decoded into readable values: the
/// obstacle dead ahead (if any), every probe hit, every visible entity, and the
/// motion of each tracked subject.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sight {
    pub ahead: Option<Obstacle>,
    pub obstacles: Vec<Obstacle>,
    pub visible: Vec<Visible>,
    pub tracked: Vec<Tracked>,
}

impl Sight {
    /// One human-readable line per perceived thing — what the demo prints so a
    /// person can watch the agent "see".
    pub fn report_lines(&self) -> Vec<String> {
        let ahead = self
            .ahead
            .map(|o| {
                let what = match o.kind {
                    Some(KIND_ENEMY) => "an enemy",
                    Some(_) => "something",
                    None => "a wall",
                };
                format!("facing {what} {:.2} m ahead", o.distance_m)
            })
            .unwrap_or_else(|| "open ahead".to_string());
        let mut lines = vec![ahead];
        for v in &self.visible {
            let what = if v.kind == KIND_ENEMY { "ENEMY" } else { "object" };
            lines.push(format!(
                "  sees {what} subject={} at ({:.2}, {:.2})",
                v.subject, v.x, v.z
            ));
        }
        for t in &self.tracked {
            lines.push(format!(
                "  tracking subject={} moving ({:+.3}, {:+.3}) /tick",
                t.subject, t.vx, t.vz
            ));
        }
        lines
    }
}

/// One neutral observation fact in `axiom-agent`'s tuple shape, as
/// [`PerceptionApi`] builds it.
type Fact = (u16, u32, i64, i64, i64, i64);

/// The player's forward direction from its yaw, in retro FPS's convention (yaw 0 faces
/// `-Z`; matches the hitscan aim in [`RetroFpsGame::fire_shot`]).
fn facing(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

/// Squared ground-plane distance from `eye` to a visible entity.
fn dist2(eye: Vec3, v: &Visible) -> f32 {
    let (dx, dz) = (v.x - eye.x, v.z - eye.z);
    dx * dx + dz * dz
}

/// Cast the perception ray-fan against the app's world and cull its enemy
/// candidates to the view cone — the whole sense step. Returns the neutral facts
/// (for the agent's observation) and the decoded [`Sight`] (for the demo/tests),
/// both built from the same raw hits via [`PerceptionApi`].
fn sense(
    app: &RunningApp,
    eye: Vec3,
    forward: Vec3,
    candidates: &[(u32, Vec3)],
    prior: &BTreeMap<u32, Vec3>,
) -> (Vec<Fact>, Sight) {
    let fov = Radians::new(FOV_RADIANS).expect("authored fov is finite");
    let reach = Meters::new(SIGHT_RANGE_M).expect("authored sight range is finite");
    let center = RAY_COUNT / 2;

    let mut facts: Vec<Fact> = Vec::new();
    let mut obstacles: Vec<Obstacle> = Vec::new();

    // Ray-fan probes: each direction is cast against the world. A hit is a
    // geometric obstacle fact (probe + point + distance) plus a decoded Obstacle
    // that also carries the hit's engine-native Tag (so the demo can name it).
    for (probe, dir) in PerceptionApi::ray_fan(forward, fov, RAY_COUNT)
        .into_iter()
        .enumerate()
    {
        if let Some((node, point)) = app.raycast_hit(eye, dir, reach) {
            let to = point.subtract(eye);
            let distance = to.dot(to).sqrt();
            let metres = Meters::new(distance).expect("a finite hit distance");
            facts.push(PerceptionApi::obstacle_fact(probe as u32, point, metres));
            obstacles.push(Obstacle {
                probe: probe as u32,
                distance_m: distance,
                kind: app.tag_of(node),
            });
        }
    }
    let ahead = obstacles.iter().copied().find(|o| o.probe == center);

    // Visible entities: cull candidates to the forward cone, read each one's Tag
    // (what it is), and — when we saw it last tick — infer its per-tick velocity
    // from the stable subject id.
    let mut visible: Vec<Visible> = Vec::new();
    let mut tracked: Vec<Tracked> = Vec::new();
    for (id, pos) in PerceptionApi::in_view(eye, forward, fov, reach, candidates) {
        let kind = app
            .player_entity(id)
            .and_then(|e| app.tag_of(e))
            .unwrap_or(KIND_WALL);
        facts.push(PerceptionApi::visible_fact(id, pos, kind));
        visible.push(Visible {
            subject: id,
            x: pos.x,
            z: pos.z,
            kind,
        });
        if let Some(&was) = prior.get(&id) {
            let velocity = PerceptionApi::relative_motion(was, pos);
            facts.push(PerceptionApi::tracked_fact(id, velocity));
            tracked.push(Tracked {
                subject: id,
                vx: velocity.x,
                vz: velocity.z,
            });
        }
    }

    (
        facts,
        Sight {
            ahead,
            obstacles,
            visible,
            tracked,
        },
    )
}

/// The control to use when an enemy is in view: turn toward the nearest one, and
/// once it is lined up, walk in and fire. Falls back to walking forward if no
/// *enemy* (only some other tagged thing) is visible.
fn engage_intent(sight: &Sight, eye: Vec3, forward: Vec3) -> Intent {
    let nearest = sight
        .visible
        .iter()
        .filter(|v| v.kind == KIND_ENEMY)
        .min_by(|a, b| dist2(eye, a).total_cmp(&dist2(eye, b)));
    match nearest {
        Some(target) => {
            let to = Vec3::new(target.x - eye.x, 0.0, target.z - eye.z);
            // Right of forward in the ground plane: (cos yaw, 0, -sin yaw).
            let right = Vec3::new(-forward.z, 0.0, forward.x);
            let bearing = to.dot(right);
            let centred =
                to.dot(forward) > 0.0 && bearing.abs() <= ENGAGE_HALF_ANGLE * to.dot(to).sqrt();
            Intent {
                turn_right: bearing > 0.0 && !centred,
                turn_left: bearing < 0.0 && !centred,
                forward: centred,
                fire: centred,
                ..Intent::default()
            }
        }
        None => Intent {
            forward: true,
            ..Intent::default()
        },
    }
}

/// The control to use when no enemy is visible: turn away from a wall dead ahead,
/// otherwise press forward to explore.
fn roam_intent(sight: &Sight) -> Intent {
    let wall_close = sight.ahead.is_some_and(|o| o.distance_m < WALL_AVOID_M);
    Intent {
        turn_left: wall_close,
        forward: !wall_close,
        ..Intent::default()
    }
}

/// Run the neutral facts through `axiom-agent`'s `observe → decide → emit` once
/// and return the lowered retro FPS [`Intent`]. A scripted brain encodes the priority
/// policy — react to a **visible enemy** before a mere **obstacle** — and emits
/// the matching app-computed control; nothing seen at all decodes to idle.
fn decide(facts: &[Fact], engage: &Intent, roam: &Intent, tick: u64) -> Intent {
    let agent_id = AgentApi::create_agent_id(AGENT_RAW_ID);
    let profile = AgentApi::debug_perfect_profile();
    let rules = vec![
        AgentApi::script_rule(
            PerceptionApi::FACT_VISIBLE,
            AgentApi::press_control_intent(control_code_of(engage)),
            AgentApi::REASON_MATCHED_RULE,
        ),
        AgentApi::script_rule(
            PerceptionApi::FACT_OBSTACLE,
            AgentApi::press_control_intent(control_code_of(roam)),
            AgentApi::REASON_MATCHED_RULE,
        ),
    ];
    let mut brain = AgentApi::scripted_brain(rules);
    let mut memory = AgentApi::empty_memory(1);

    let mut builder =
        AgentApi::observation_builder(agent_id, Tick::new(tick), 1, facts.len().max(1), 0);
    builder
        .add_channel(AgentApi::channel_geometric())
        .expect("one channel within the channel bound");
    for &(kind, subject, x, y, z, value) in facts {
        builder
            .add_fact(AgentApi::observation_fact(kind, subject, x, y, z, value))
            .expect("perception fact within the fact bound");
    }
    let observation = builder.build();

    let step = RuntimeStep::new(FrameIndex::new(0), Tick::new(tick), FIXED_DELTA_NANOS, 0);
    let (_report, mut queue) =
        AgentApi::step(agent_id, profile, &mut brain, &observation, &mut memory, step);
    let neutral = queue.pop().unwrap_or_else(AgentApi::noop_intent);
    intent_of_control_code(neutral.control_code())
}

/// Tag every enemy node `KIND_ENEMY` so a raycast hit classifies itself. Re-run
/// each tick because a respawn mints a fresh engine node (idempotent: tagging
/// replaces).
fn tag_enemies(app: &mut RunningApp, game: &RetroFpsGame) {
    for index in 0..game.enemy_count() as u32 {
        if let Some(entity) = app.player_entity(index) {
            app.tag(entity, KIND_ENEMY);
        }
    }
}

/// A live perception-driven retro FPS session: it perceives the world each tick and
/// lets the agent act on what it sees, through the real `RetroFpsGame` → engine path.
#[derive(Debug)]
pub struct RetroFpsPerceiver {
    game: RetroFpsGame,
    app: RunningApp,
    assets: RetroFpsAssets,
    tick: u64,
    /// Where each enemy subject was last tick — the basis for tracking.
    prior: BTreeMap<u32, Vec3>,
}

impl RetroFpsPerceiver {
    /// Start a fresh retro FPS game, bind the enemies to their engine nodes, and tag
    /// them so the agent's hits classify entity-natively from tick 0.
    pub fn new() -> Self {
        let mut game = RetroFpsGame::new();
        let (mut app, assets) = build_retro_fps_app(&LevelDoc::default());
        game.bind_entities(&app);
        tag_enemies(&mut app, &game);
        RetroFpsPerceiver {
            game,
            app,
            assets,
            tick: 0,
            prior: BTreeMap::new(),
        }
    }

    /// The player's eye/cast origin: position at the sight (enemy-centre) height.
    fn eye(&self) -> Vec3 {
        let (px, pz, _, _) = self.game.pose();
        Vec3::new(px, self.game.sight_height(), pz)
    }

    /// The live enemy candidates as `(subject id, world position)` — every bound,
    /// living enemy (a despawned one has no translation and drops out).
    fn candidates(&self) -> Vec<(u32, Vec3)> {
        (0..self.game.enemy_count() as u32)
            .filter_map(|index| self.app.player_translation(index).map(|pos| (index, pos)))
            .collect()
    }

    /// Perceive the world this tick without acting — the decoded [`Sight`].
    pub fn sight(&self) -> Sight {
        let (_, _, yaw, _) = self.game.pose();
        let (_facts, sight) = sense(
            &self.app,
            self.eye(),
            facing(yaw),
            &self.candidates(),
            &self.prior,
        );
        sight
    }

    /// Perceive, decide a reactive intent through the agent substrate, drive one
    /// real tick, and return the [`Sight`] perceived *before* acting.
    pub fn advance(&mut self) -> Sight {
        let (_, _, yaw, _) = self.game.pose();
        let eye = self.eye();
        let forward = facing(yaw);
        let candidates = self.candidates();
        let (facts, sight) = sense(&self.app, eye, forward, &candidates, &self.prior);

        let engage = engage_intent(&sight, eye, forward);
        let roam = roam_intent(&sight);
        let intent = decide(&facts, &engage, &roam, self.tick);

        // Remember this tick's positions so next tick yields each subject's motion.
        self.prior = candidates.into_iter().collect();
        retro_fps_drive_tick(
            &mut self.game,
            &mut self.app,
            &self.assets,
            self.tick,
            intent,
        );
        tag_enemies(&mut self.app, &self.game);
        self.tick += 1;
        sight
    }

    /// The current tick (the number advanced so far).
    pub fn tick(&self) -> u64 {
        self.tick
    }
}

impl Default for RetroFpsPerceiver {
    fn default() -> Self {
        RetroFpsPerceiver::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_zero_yaw_points_down_negative_z() {
        let f = facing(0.0);
        assert!(f.x.abs() < 1.0e-6 && (f.z + 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn from_the_start_the_agent_faces_the_north_wall_at_a_real_distance() {
        // Start is the 'S' cell (col 1, row 8) facing -Z (yaw 0). The nearest wall
        // straight ahead is the top row's cell at col 1, centred at z = 0 with a
        // 0.5 half-extent: its near face is z = 0.5, so the distance is 8 - 0.5.
        let perceiver = RetroFpsPerceiver::new();
        let ahead = perceiver.sight().ahead.expect("a wall is dead ahead");
        assert_eq!(ahead.probe, RAY_COUNT / 2, "the centre ray is the ahead probe");
        assert!(
            (ahead.distance_m - 7.5).abs() < 0.05,
            "north wall ~7.5 m ahead, got {}",
            ahead.distance_m
        );
        assert_eq!(ahead.kind, None, "untagged geometry — a wall, not an enemy");
    }

    #[test]
    fn an_enemy_in_the_cone_is_seen_and_classified_enemy() {
        // Turning the cone toward the room's enemies, perception must classify at
        // least one as KIND_ENEMY (entity-native, off its Tag) and never mislabel
        // a wall as an enemy.
        let mut perceiver = RetroFpsPerceiver::new();
        let mut saw_enemy = false;
        for _ in 0..120 {
            let sight = perceiver.advance();
            assert!(
                sight.visible.iter().all(|v| v.kind == KIND_ENEMY),
                "only tagged enemies are visible candidates"
            );
            saw_enemy |= sight.visible.iter().any(|v| v.kind == KIND_ENEMY);
        }
        assert!(saw_enemy, "the agent saw and classified an enemy");
    }

    #[test]
    fn a_moving_enemy_yields_a_tracked_velocity() {
        // Enemies chase the player, so a subject seen on consecutive ticks reports
        // a non-zero per-tick velocity — the agent tracking a moving object.
        let mut perceiver = RetroFpsPerceiver::new();
        let mut tracked_moving = false;
        for _ in 0..150 {
            let sight = perceiver.advance();
            tracked_moving |= sight
                .tracked
                .iter()
                .any(|t| t.vx.abs() > 1.0e-5 || t.vz.abs() > 1.0e-5);
        }
        assert!(tracked_moving, "the agent tracked a moving enemy's velocity");
    }

    #[test]
    fn perception_is_deterministic() {
        let run = || {
            let mut p = RetroFpsPerceiver::new();
            let mut report = String::new();
            for _ in 0..40 {
                for line in p.advance().report_lines() {
                    report.push_str(&line);
                    report.push('\n');
                }
            }
            report
        };
        assert_eq!(run(), run(), "same perception every run");
    }
}
