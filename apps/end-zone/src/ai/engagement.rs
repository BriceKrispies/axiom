//! Line engagements: the blocking contest between an offensive blocker and the
//! rusher he has latched. Blocking *is* the engagement system, so it lives here
//! rather than in the general contact framework ([`crate::player::contact`],
//! which keeps tackle/dive/fall).
//!
//! Each engagement is keyed by blocker id and advances a deterministic
//! `advantage` (spec §6): negative when the blocker is winning (he anchors and
//! drives the rusher off his lane), positive when the rusher is winning (the
//! pocket compresses), and a *shed* once it crosses the threshold — the rusher
//! breaks free to pursue. Pressure comes from the contest **progressing toward a
//! result**, never a global speed boost, and a strong blocker keeps the
//! advantage low, delaying or preventing the shed. Written by the contact stage,
//! read by the AI the next tick.

use axiom::prelude::Vec3;

use crate::config::DT;
use crate::data::BehaviorTuning;
use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::PlayerIntent;

/// The visible physical state of one block (spec §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngagementState {
    /// No live engagement.
    #[default]
    Idle,
    /// Closing on the rusher, not yet in contact.
    Square,
    /// In contact, contesting on even terms.
    Contact,
    /// Holding ground against a rusher who is pressing.
    Anchor,
    /// Winning — driving the rusher off his lane.
    Redirect,
    /// Lost leverage; the blocker is resetting.
    Recover,
    /// The rusher has broken free of this block.
    Shed,
}

impl EngagementState {
    pub fn label(self) -> &'static str {
        match self {
            EngagementState::Idle => "idle",
            EngagementState::Square => "square",
            EngagementState::Contact => "contact",
            EngagementState::Anchor => "anchor",
            EngagementState::Redirect => "redirect",
            EngagementState::Recover => "recover",
            EngagementState::Shed => "shed",
        }
    }
}

/// The rush lane a defender is trying to win around a blocker (spec §6). It is
/// recorded geometrically per engagement for readability + the debug view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RushLane {
    /// Straight through the blocker's chest.
    #[default]
    Power,
    /// Under/inside the blocker toward the middle.
    Inside,
    /// Around the blocker's outside shoulder.
    Outside,
    /// Hold the edge and keep the runner inside.
    Contain,
}

impl RushLane {
    pub fn label(self) -> &'static str {
        match self {
            RushLane::Power => "power",
            RushLane::Inside => "inside",
            RushLane::Outside => "outside",
            RushLane::Contain => "contain",
        }
    }
}

/// One live block, keyed by its blocker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Engagement {
    pub rusher: PlayerId,
    pub state: EngagementState,
    /// `-1..=1`: negative = blocker winning, positive = rusher winning; the
    /// rusher sheds at `shed_threshold`.
    pub advantage: f32,
    pub ticks: u32,
    pub lane: RushLane,
}

/// Per-blocker engagement slot (indexed by blocker id).
pub type EngagementLink = Option<Engagement>;

/// Whether a rusher has broken free of every block (any engagement naming him
/// is shed, or no blocker is engaging him). The rusher's brain reads this to
/// unlock a stronger pursuit of the quarterback.
pub fn rusher_is_free(links: &[EngagementLink], rusher: PlayerId) -> bool {
    !links.iter().flatten().any(|e| {
        e.rusher == rusher && !matches!(e.state, EngagementState::Shed | EngagementState::Idle)
    })
}

/// The engagement a rusher is currently held in (if any) — the debug view and
/// the rusher's shed candidate read its advantage.
pub fn engagement_on(links: &[EngagementLink], rusher: PlayerId) -> Option<Engagement> {
    links
        .iter()
        .flatten()
        .copied()
        .find(|e| e.rusher == rusher && !matches!(e.state, EngagementState::Shed))
}

/// Advance every block one tick: resolve pairings from the `Block` intents,
/// evolve each engagement's advantage/state deterministically, and apply the
/// physical effect (velocity resist + controlled displacement of the rusher off
/// his lane). Returns the pairs in contact this tick (the sim announces NEW
/// pairs as `BlockEngaged`). Replaces the old velocity-only `resolve_blocks`.
pub fn advance_engagements(
    links: &mut [EngagementLink],
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    protect: Vec3,
    tuning: &BehaviorTuning,
) -> Vec<(PlayerId, PlayerId)> {
    let mut pairs = Vec::new();
    for index in 0..players.len() {
        let PlayerIntent::Block { target, .. } = intents[index] else {
            // Not blocking this tick: the engagement ends.
            links[index] = None;
            continue;
        };
        let blocker_ok = players[index].anim.can_act();
        let rusher_ok = players[target.index()].anim.can_act();
        if !blocker_ok || !rusher_ok {
            links[index] = None;
            continue;
        }

        let blocker_pos = players[index].pos;
        let rusher_pos = players[target.index()].pos;
        let separation = flat(rusher_pos.subtract(blocker_pos)).length();
        let in_contact = separation <= tuning.block_engage_range;

        // Persist the engagement across ticks while the target is unchanged, so
        // advantage accumulates; reset it when the blocker latches a new rusher.
        let mut engagement = match links[index] {
            Some(e) if e.rusher == target => e,
            _ => Engagement {
                rusher: target,
                state: EngagementState::Square,
                advantage: 0.0,
                ticks: 0,
                lane: rush_lane(blocker_pos, rusher_pos, protect),
            },
        };
        engagement.ticks = engagement.ticks.saturating_add(1);

        if in_contact {
            // Advantage evolves from the strength edge plus a small base gain, so
            // even a parity block eventually yields — the pass rush wins if the
            // quarterback holds forever. A stronger blocker drives it negative.
            let edge = players[target.index()].archetype.block_strength
                - players[index].archetype.block_strength;
            let delta = tuning.engage_advantage_rate * (tuning.engage_base_gain + edge);
            engagement.advantage = (engagement.advantage + delta).clamp(-1.0, 1.0);

            let blocker_win = (0.5 - 0.5 * engagement.advantage).clamp(0.0, 1.0);
            let resist = 1.0 - tuning.block_resist * blocker_win;
            let rusher = &mut players[target.index()];
            rusher.vel = rusher.vel.mul_scalar(resist);
            rusher.balance = (rusher.balance - 0.02).max(0.2);
            // A winning blocker drives the rusher off his lane, away from the
            // protected point — a controlled displacement, not a teleport.
            if engagement.advantage < 0.0 {
                let away = flat(rusher_pos.subtract(protect))
                    .normalize()
                    .unwrap_or_else(|_| flat(rusher_pos.subtract(blocker_pos)).normalize().unwrap_or(Vec3::UNIT_Z));
                let drive = tuning.block_drive * (-engagement.advantage) * DT;
                rusher.pos = rusher.pos.add(away.mul_scalar(drive));
            }
            engagement.state = contact_state(&engagement, tuning);
            pairs.push((players[index].id, target));
        } else {
            // Closing to the block: no physical contest yet, advantage relaxes.
            engagement.advantage = (engagement.advantage * 0.96).clamp(-1.0, 1.0);
            engagement.state = EngagementState::Square;
        }

        links[index] = Some(engagement);
    }
    pairs
}

/// The engagement state while in contact, from advantage + tenure.
fn contact_state(engagement: &Engagement, tuning: &BehaviorTuning) -> EngagementState {
    if engagement.advantage >= tuning.shed_threshold {
        EngagementState::Shed
    } else if engagement.advantage <= -0.3 {
        EngagementState::Redirect
    } else if engagement.ticks < tuning.engage_square_ticks {
        EngagementState::Square
    } else if engagement.advantage <= 0.1 {
        EngagementState::Anchor
    } else {
        EngagementState::Contact
    }
}

/// Which lane the rusher is winning, from his alignment on the blocker relative
/// to the protected point.
fn rush_lane(blocker: Vec3, rusher: Vec3, protect: Vec3) -> RushLane {
    let inside = (protect.x - blocker.x).signum();
    let side = (rusher.x - blocker.x) * inside;
    if side > 0.4 {
        RushLane::Inside
    } else if side < -0.4 {
        RushLane::Outside
    } else {
        RushLane::Power
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
