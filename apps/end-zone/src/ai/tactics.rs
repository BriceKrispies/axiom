//! The football scoring behind the overseer's calls: which tactical mode the
//! ball state *forces*, and — during a dropback — how strongly each coverage /
//! pressure / contain / bracket mode is warranted by the deterministic read.
//! Each mode is scored by explicit football factors (never one giant
//! conditional), and its plan names the region the adjustment leaves exposed.

use crate::identity::PlayerId;

use super::directive::{CoverageEmphasis, ExposedRegion, SecondaryAdjustment, TacticalMode};
use super::field_read::{DefensiveRead, PocketState};
use super::overseer::PossessionMemory;
use super::perception::PlayPerception;

/// The candidate coverage modes weighed during a pre-throw dropback.
pub const DROPBACK_MODES: [TacticalMode; 7] = [
    TacticalMode::Base,
    TacticalMode::IncreasePressure,
    TacticalMode::ContainQb,
    TacticalMode::ProtectDeep,
    TacticalMode::ProtectMiddle,
    TacticalMode::ProtectOutside,
    TacticalMode::BracketReceiver,
];

/// A chosen mode's parameters before defenders are assigned to its overrides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModePlan {
    pub mode: TacticalMode,
    pub coverage: CoverageEmphasis,
    pub shade_side: f32,
    pub rush_emphasis: f32,
    pub coverage_depth: f32,
    pub risk_tolerance: f32,
    pub exposed: ExposedRegion,
    pub secondary: SecondaryAdjustment,
    pub primary_threat: Option<PlayerId>,
    pub secondary_threat: Option<PlayerId>,
    pub reason: &'static str,
    pub min_ticks: u32,
    pub confidence: f32,
    pub score: f32,
}

/// The tactical mode the ball state forces, overriding ordinary coverage
/// selection (a thrown ball, a caught ball, or a committed run).
pub fn forced_mode(per: &PlayPerception) -> Option<TacticalMode> {
    use crate::football::BallSituation;
    match per.situation {
        BallSituation::InFlight | BallSituation::Contested => Some(TacticalMode::CatchPointCollapse),
        // A committed runner is the coordinated run response; a receiver running
        // after the catch (or a loose ball) is swarm-and-contain.
        BallSituation::QbScramble => Some(TacticalMode::QbRunResponse),
        BallSituation::Caught | BallSituation::LooseBall => Some(TacticalMode::SwarmAndContain),
        _ => None,
    }
}

/// Whether a touchdown is imminent enough to override ordinary commitment.
pub fn is_emergency(per: &PlayPerception, read: &DefensiveRead) -> bool {
    read.touchdown_threat
        && (per.situation.ball_in_air() || per.situation.has_runner() || !per.qb_in_pocket)
}

/// Score one mode from the read (higher = more warranted). Forced modes are not
/// scored here — they are selected by [`forced_mode`].
pub fn score(mode: TacticalMode, read: &DefensiveRead, mem: &PossessionMemory) -> f32 {
    match mode {
        TacticalMode::Base => 0.35,
        TacticalMode::IncreasePressure => {
            // Pressure is warranted only once the quarterback has actually held
            // the ball — comfort scales with how long the pocket has stood.
            let held = (read.ticks_since_snap as f32 / 90.0).min(1.0);
            let comfort = match read.pocket_state {
                PocketState::Stable => 0.7,
                PocketState::Compressing => 0.4,
                PocketState::Broken => 0.0,
            };
            let tend = (mem.long_holds as f32 * 0.1).min(0.2);
            // Do NOT bring pressure if it exposes an immediate deep touchdown.
            let deep_danger = if read.deep_threats >= 2 || read.touchdown_threat {
                0.9
            } else {
                0.0
            };
            let personnel = if read.free_defenders >= 3 { 0.0 } else { 0.4 };
            (comfort * held + tend - deep_danger - personnel).max(0.0)
        }
        TacticalMode::ContainQb => {
            let roll = if read.qb_rollout { 0.75 } else { 0.0 };
            let tend = (mem.scramble_events as f32 * 0.12).min(0.45);
            roll + tend
        }
        TacticalMode::ProtectDeep => {
            let deep = (read.deep_threats as f32 * 0.4).min(0.9);
            let time = if matches!(read.pocket_state, PocketState::Stable) {
                0.25
            } else {
                0.0
            };
            let td = if read.touchdown_threat && read.deep_threats >= 1 {
                0.5
            } else {
                0.0
            };
            deep + time + td
        }
        TacticalMode::ProtectMiddle => {
            if read.crossing {
                0.7
            } else {
                0.0
            }
        }
        TacticalMode::ProtectOutside => {
            let o = read.sideline_overload.abs();
            if o > 0.5 {
                0.3 + (o - 0.5) * 0.8
            } else {
                0.0
            }
        }
        TacticalMode::BracketReceiver => {
            let sep = ((read.danger_separation - 3.5) / 4.0).clamp(0.0, 0.7);
            let tend = read
                .most_dangerous
                .map(|id| (mem.target_counts[id.index()] as f32 * 0.1).min(0.3))
                .unwrap_or(0.0);
            let personnel = if read.free_defenders >= 3 { 0.0 } else { 0.5 };
            (sep + tend - personnel).max(0.0)
        }
        _ => 0.0,
    }
}

/// The full plan for a mode: its emphasis, tradeoff, threats, and commitment.
pub fn plan(mode: TacticalMode, per: &PlayPerception, read: &DefensiveRead, mem: &PossessionMemory) -> ModePlan {
    let s = score(mode, read, mem);
    let base = ModePlan {
        mode,
        coverage: CoverageEmphasis::Balanced,
        shade_side: 0.0,
        rush_emphasis: 0.0,
        coverage_depth: 0.0,
        risk_tolerance: 0.3,
        exposed: ExposedRegion::None,
        secondary: SecondaryAdjustment::None,
        primary_threat: None,
        secondary_threat: None,
        reason: mode.label(),
        min_ticks: 12,
        confidence: s.clamp(0.2, 0.95),
        score: s,
    };
    match mode {
        TacticalMode::Base => base,
        TacticalMode::IncreasePressure => ModePlan {
            rush_emphasis: 0.85,
            coverage_depth: -1.5,
            risk_tolerance: 0.5,
            exposed: ExposedRegion::UnderneathMiddle,
            min_ticks: 24,
            ..base
        },
        TacticalMode::ContainQb => ModePlan {
            secondary: SecondaryAdjustment::SpyQb,
            rush_emphasis: -0.2,
            risk_tolerance: 0.25,
            exposed: ExposedRegion::EscapeLane,
            min_ticks: 24,
            ..base
        },
        TacticalMode::QbRunResponse => ModePlan {
            secondary: SecondaryAdjustment::ExtraDeepHelp,
            risk_tolerance: 0.6,
            min_ticks: 20,
            confidence: 0.9,
            ..base
        },
        TacticalMode::ProtectDeep => ModePlan {
            coverage: CoverageEmphasis::Deep,
            coverage_depth: 3.0,
            rush_emphasis: -0.3,
            secondary: SecondaryAdjustment::ExtraDeepHelp,
            exposed: ExposedRegion::UnderneathMiddle,
            min_ticks: 30,
            ..base
        },
        TacticalMode::ProtectMiddle => ModePlan {
            coverage: CoverageEmphasis::Middle,
            exposed: ExposedRegion::ShortOutside,
            min_ticks: 24,
            ..base
        },
        TacticalMode::ProtectOutside => ModePlan {
            coverage: CoverageEmphasis::Outside,
            shade_side: read.sideline_overload.signum(),
            exposed: ExposedRegion::OppositeSide,
            min_ticks: 24,
            ..base
        },
        TacticalMode::BracketReceiver => ModePlan {
            primary_threat: read.most_dangerous,
            secondary: SecondaryAdjustment::None,
            exposed: ExposedRegion::BacksideReceiver,
            min_ticks: 30,
            ..base
        },
        TacticalMode::CatchPointCollapse => ModePlan {
            risk_tolerance: 0.7,
            min_ticks: 8,
            confidence: 0.9,
            primary_threat: per.intended_receiver,
            ..base
        },
        TacticalMode::SwarmAndContain => ModePlan {
            risk_tolerance: 0.5,
            min_ticks: 16,
            confidence: 0.85,
            primary_threat: per.ground_threat,
            ..base
        },
        TacticalMode::EmergencyTouchdown => ModePlan {
            risk_tolerance: 1.0,
            rush_emphasis: 0.5,
            min_ticks: 6,
            confidence: 0.95,
            reason: "touchdown emergency",
            ..base
        },
    }
}

