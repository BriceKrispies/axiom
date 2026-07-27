//! The **read**: what the player is actually being asked to judge, and when
//! that judgement is worth interrupting the play for.
//!
//! This is the prototype's tactical core and it is deliberately a pure function
//! of simulation state — same field, same read, every time. Nothing here rolls
//! dice or peeks at the outcome; it measures the three things a quarterback
//! measures (how open each receiver is, how developed his route is, how close
//! the rush is) and decides whether *right now* is the dramatic moment.

use axiom::prelude::Vec3;

use crate::ai::assignment::offense_player;
use crate::ai::RoleState;
use crate::data::prototype::{concept, READ_COUNT};
use crate::identity::PlayerId;
use crate::state::SimState;

use super::phase::WindowTrigger;

/// Separation at which a receiver is considered fully covered, yards.
const COVERED_SEPARATION: f32 = 1.6;
/// Separation at which a receiver is considered wide open, yards.
const OPEN_SEPARATION: f32 = 5.0;
/// Distance from the quarterback at which the pocket is comfortable, yards.
const POCKET_SAFE: f32 = 7.5;
/// Distance at which a rusher is effectively on top of the quarterback, yards.
const POCKET_LOST: f32 = 1.8;
/// Openness a read must reach to be worth stopping the play for.
const OPEN_TRIGGER: f32 = 0.55;
/// Pressure at which the window opens whether or not anyone is open.
const PRESSURE_TRIGGER: f32 = 0.62;

/// One eligible target's live state, as the player would see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReadState {
    /// Read index `0..3` — 0 is the short route, 2 is the deep one.
    pub read: usize,
    pub id: PlayerId,
    pub pos: Vec3,
    /// Yards past the line of scrimmage.
    pub depth: f32,
    /// Planar distance to the nearest defender who can act, yards.
    pub separation: f32,
    /// Whether the route has broken (past its stem).
    pub broken: bool,
    /// Whether this target can still legally be thrown to.
    pub live: bool,
    /// `0..1` — how open this read looks right now.
    pub openness: f32,
}

/// The whole read for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayRead {
    /// Which concept these reads belong to (indexes `data::prototype::concept`).
    /// Carried on the read rather than looked up globally, because "read 2" now
    /// means a different receiver and a different route per concept.
    pub concept: usize,
    pub reads: [ReadState; READ_COUNT],
    /// Distance from the quarterback to the nearest live defender, yards.
    pub nearest_rush: f32,
    /// `0..1` — how far gone the pocket is.
    pub pressure: f32,
    /// The read with the highest openness × reward this tick.
    pub best: usize,
}

impl PlayRead {
    /// The state of one read.
    pub fn read(&self, read: usize) -> &ReadState {
        &self.reads[read.min(READ_COUNT - 1)]
    }

    /// The player id of one read (what a choice resolves to).
    pub fn target(&self, read: usize) -> PlayerId {
        self.read(read).id
    }
}

/// Gate state the controller carries between ticks: when a window may open,
/// when one opens regardless, how many have been spent, and which read the
/// last one was about (so an unchanged picture does not re-ask the same
/// question immediately).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGate {
    pub armed_at: u64,
    pub deadline: u64,
    pub windows_used: u32,
    pub last_best: Option<usize>,
}

impl WindowGate {
    /// A gate that can never fire — the state between attempts, before a snap
    /// has armed it. Nothing that survives a reset can open a window.
    pub fn closed() -> Self {
        WindowGate {
            armed_at: u64::MAX,
            deadline: u64::MAX,
            windows_used: 0,
            last_best: None,
        }
    }
}

fn flat_len(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

/// Distance from `pos` to the nearest opponent of `team` who can still act.
fn nearest_opponent(sim: &SimState, pos: Vec3, team: crate::identity::TeamId) -> f32 {
    sim.players
        .iter()
        .filter(|p| p.team != team && p.anim.can_act())
        .map(|p| flat_len(pos, p.pos))
        .fold(f32::MAX, f32::min)
}

/// Measure one read.
fn measure(sim: &SimState, concept_index: usize, read: usize) -> ReadState {
    let id = offense_player(&sim.play, concept(concept_index).read_slots[read]);
    let player = &sim.players[id.index()];
    let depth = (player.pos.z - sim.frame.line_of_scrimmage_z) * sim.frame.direction.sign();
    let separation = nearest_opponent(sim, player.pos, player.team);
    // "Broken" means the receiver has cleared his stem and is into the part of
    // the route that actually beats coverage. The route runner's own role state
    // already tracks this, so the read can never disagree with the route.
    let broken = match sim.roles[id.index()] {
        RoleState::Route { index } => index >= 1,
        RoleState::RouteDone | RoleState::CatchWork => true,
        _ => false,
    };
    let live = player.anim.can_act() && !player.anim.is_down();
    let spread = (OPEN_SEPARATION - COVERED_SEPARATION).max(0.01);
    let separation_score = ((separation - COVERED_SEPARATION) / spread).clamp(0.0, 1.0);
    // An unbroken route is not a read yet, however much cushion it has: the
    // cushion on a receiver still running his stem is about to disappear.
    let development = match broken {
        true => 1.0,
        false => (depth / 6.0).clamp(0.0, 1.0) * 0.45,
    };
    let openness = match live {
        true => separation_score * development,
        false => 0.0,
    };
    ReadState {
        read,
        id,
        pos: player.pos,
        depth,
        separation,
        broken,
        live,
        openness,
    }
}

/// Build this tick's read from the authoritative simulation.
pub fn read_play(sim: &SimState, concept_index: usize) -> PlayRead {
    let reads: [ReadState; READ_COUNT] = core::array::from_fn(|read| measure(sim, concept_index, read));
    let qb = &sim.players[sim.quarterback.index()];
    let nearest_rush = nearest_opponent(sim, qb.pos, qb.team);
    let span = (POCKET_SAFE - POCKET_LOST).max(0.01);
    let pressure = ((POCKET_SAFE - nearest_rush) / span).clamp(0.0, 1.0);
    // Value-weighted so the trigger fires on the read that is worth taking, not
    // merely on whoever happens to have the most grass. The deep route needs
    // less separation to be worth asking about than the checkdown does.
    let rewards = concept(concept_index).read_rewards;
    let max_reward = rewards[READ_COUNT - 1].max(1.0);
    let value = |r: &ReadState| r.openness * (0.55 + 0.45 * rewards[r.read] / max_reward);
    let best = reads
        .iter()
        .enumerate()
        .fold((0usize, -1.0f32), |(bi, bv), (i, r)| {
            let v = value(r);
            match v > bv {
                true => (i, v),
                false => (bi, bv),
            }
        })
        .0;
    PlayRead {
        concept: concept_index,
        reads,
        nearest_rush,
        pressure,
        best,
    }
}

/// Should a decision window open this tick, and why?
///
/// Ordered by urgency: a collapsing pocket forces the question before a pretty
/// read does, and the deadline guarantees the question is always asked at least
/// once per attempt — the window can never silently fail to appear.
pub fn window_trigger(read: &PlayRead, tick: u64, gate: &WindowGate) -> Option<WindowTrigger> {
    if gate.windows_used >= super::MAX_WINDOWS || tick < gate.armed_at {
        return None;
    }
    if read.pressure >= PRESSURE_TRIGGER {
        return Some(WindowTrigger::Pressure);
    }
    if tick >= gate.deadline {
        return Some(WindowTrigger::Deadline);
    }
    let open = read.reads[read.best].openness >= OPEN_TRIGGER;
    // Re-asking about the same receiver the player just declined is nagging;
    // a *different* read coming open is genuinely new information.
    let fresh = gate.last_best != Some(read.best);
    (open && fresh).then_some(WindowTrigger::ReadOpen)
}
