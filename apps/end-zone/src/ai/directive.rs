//! The compact defensive directive the [`DefensiveOverseer`](super::overseer)
//! issues and the individual player AI consumes. The overseer decides *what
//! collective problem the defense must solve*; the directive is how it says so —
//! a tactical mode, coverage/rush emphasis, per-defender assignment overrides,
//! and the *exposed region* the adjustment gives up. It never contains a
//! position, velocity, or steering vector: players convert it into movement
//! through their existing arbitration.

use crate::config::PLAYER_COUNT;
use crate::identity::PlayerId;

/// The one major tactical mode active this call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TacticalMode {
    /// Hold the base assignments and formation.
    #[default]
    Base,
    /// Commit extra pressure at the quarterback.
    IncreasePressure,
    /// Close the rollout / scramble lanes.
    ContainQb,
    /// Coordinate the pursuit of a committed runner.
    QbRunResponse,
    /// Rotate help over the top against vertical threats.
    ProtectDeep,
    /// Compress the interior against crossers.
    ProtectMiddle,
    /// Shade coverage to an overloaded sideline.
    ProtectOutside,
    /// Double the single most dangerous receiver.
    BracketReceiver,
    /// Collapse on the projected catch point after a throw.
    CatchPointCollapse,
    /// Rally to a live carrier after a catch.
    SwarmAndContain,
    /// Abandon shape to stop an imminent touchdown.
    EmergencyTouchdown,
}

impl TacticalMode {
    pub fn label(self) -> &'static str {
        match self {
            TacticalMode::Base => "base",
            TacticalMode::IncreasePressure => "pressure",
            TacticalMode::ContainQb => "contain-qb",
            TacticalMode::QbRunResponse => "qb-run",
            TacticalMode::ProtectDeep => "protect-deep",
            TacticalMode::ProtectMiddle => "protect-middle",
            TacticalMode::ProtectOutside => "protect-outside",
            TacticalMode::BracketReceiver => "bracket",
            TacticalMode::CatchPointCollapse => "catch-point",
            TacticalMode::SwarmAndContain => "swarm",
            TacticalMode::EmergencyTouchdown => "emergency-td",
        }
    }
}

/// A compatible secondary adjustment layered on the major mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecondaryAdjustment {
    #[default]
    None,
    /// Keep one extra defender deep as insurance.
    ExtraDeepHelp,
    /// Tighten the edge contain.
    TightenContain,
    /// Spy the quarterback with a linebacker.
    SpyQb,
}

impl SecondaryAdjustment {
    pub fn label(self) -> &'static str {
        match self {
            SecondaryAdjustment::None => "-",
            SecondaryAdjustment::ExtraDeepHelp => "+deep",
            SecondaryAdjustment::TightenContain => "+contain",
            SecondaryAdjustment::SpyQb => "+spy",
        }
    }
}

/// Where coverage is emphasised (and, by implication, thinned elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverageEmphasis {
    #[default]
    Balanced,
    Deep,
    Middle,
    Outside,
}

impl CoverageEmphasis {
    pub fn label(self) -> &'static str {
        match self {
            CoverageEmphasis::Balanced => "balanced",
            CoverageEmphasis::Deep => "deep",
            CoverageEmphasis::Middle => "middle",
            CoverageEmphasis::Outside => "outside",
        }
    }
}

/// The region the current adjustment leaves weaker — the readable tradeoff (and
/// a real weakness the offense can attack).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExposedRegion {
    #[default]
    None,
    UnderneathMiddle,
    ShortOutside,
    Deep,
    BacksideReceiver,
    OppositeSide,
    EscapeLane,
}

impl ExposedRegion {
    pub fn label(self) -> &'static str {
        match self {
            ExposedRegion::None => "-",
            ExposedRegion::UnderneathMiddle => "underneath-middle",
            ExposedRegion::ShortOutside => "short-outside",
            ExposedRegion::Deep => "deep",
            ExposedRegion::BacksideReceiver => "backside-receiver",
            ExposedRegion::OppositeSide => "opposite-side",
            ExposedRegion::EscapeLane => "escape-lane",
        }
    }
}

/// A per-defender assignment override the overseer stamps onto a directive. The
/// bracket / spy target is the directive's `primary_threat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AssignmentOverride {
    #[default]
    None,
    /// Spy the quarterback: mirror him, attack only once he breaks the pocket.
    Spy,
    /// An extra rusher committed at the quarterback.
    Blitz,
    /// Primary man coverage on the bracketed receiver.
    BracketPrimary,
    /// Over-the-top help on the bracketed receiver.
    BracketHelp,
    /// Hold the edge / escape boundary.
    ContainEdge,
}

impl AssignmentOverride {
    pub fn label(self) -> &'static str {
        match self {
            AssignmentOverride::None => "none",
            AssignmentOverride::Spy => "spy",
            AssignmentOverride::Blitz => "blitz",
            AssignmentOverride::BracketPrimary => "bracket",
            AssignmentOverride::BracketHelp => "help",
            AssignmentOverride::ContainEdge => "contain",
        }
    }
}

/// The compact directive the whole defense reads this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefensiveDirective {
    pub mode: TacticalMode,
    pub secondary: SecondaryAdjustment,
    pub primary_threat: Option<PlayerId>,
    pub secondary_threat: Option<PlayerId>,
    pub coverage: CoverageEmphasis,
    /// Sideline to shade toward for `ProtectOutside` (-1 left, +1 right, 0 none).
    pub shade_side: f32,
    /// Extra rush urgency `0..=1` the rushers add.
    pub rush_emphasis: f32,
    /// Coverage depth bias, yards (+ deeper, − shallower).
    pub coverage_depth: f32,
    /// How much risk the defense will accept `0..=1` (dives, jumping routes).
    pub risk_tolerance: f32,
    /// The overseer's confidence in this read `0..=1`.
    pub confidence: f32,
    /// Tick the directive was issued (drives the commitment window).
    pub since_tick: u64,
    /// Minimum ticks before a non-emergency change.
    pub min_ticks: u32,
    pub reason: &'static str,
    pub exposed: ExposedRegion,
    pub overrides: [AssignmentOverride; PLAYER_COUNT],
}

impl DefensiveDirective {
    /// The neutral base directive.
    pub fn base(tick: u64) -> Self {
        DefensiveDirective {
            mode: TacticalMode::Base,
            secondary: SecondaryAdjustment::None,
            primary_threat: None,
            secondary_threat: None,
            coverage: CoverageEmphasis::Balanced,
            shade_side: 0.0,
            rush_emphasis: 0.0,
            coverage_depth: 0.0,
            risk_tolerance: 0.3,
            confidence: 0.5,
            since_tick: tick,
            min_ticks: 1,
            reason: "base",
            exposed: ExposedRegion::None,
            overrides: [AssignmentOverride::None; PLAYER_COUNT],
        }
    }

    /// This defender's assignment override.
    pub fn override_for(&self, player: PlayerId) -> AssignmentOverride {
        self.overrides[player.index()]
    }

    /// Ticks of commitment `tick` still owes this directive.
    pub fn commitment_left(&self, tick: u64) -> u32 {
        let elapsed = tick.saturating_sub(self.since_tick) as u32;
        self.min_ticks.saturating_sub(elapsed)
    }
}
