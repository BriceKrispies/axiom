//! Pass 9 — the deterministic scoring rules.
//!
//! Fixed base points per result plus deterministic power / placement / streak
//! bonuses. This is **not** a general scoring framework: it is one fixed table
//! and a handful of integer comparisons, specific to this app. No maps, no
//! wall-clock, no randomness, no probability.

use crate::soccer_penalty::penalty_result::{PenaltyShotResult, PenaltyShotResultKind};

// --- base points ------------------------------------------------------------

pub const GOAL_BASE: u32 = 500;
pub const POST_BASE: u32 = 100;
pub const SAVE_BASE: u32 = 0;
pub const MISS_BASE: u32 = 0;

// --- bonuses ----------------------------------------------------------------

pub const POWER_BONUS_SWEET: u32 = 150; // Goal, power in 70..=90
pub const POWER_BONUS_OVER: u32 = 75; // Goal, power > 90
pub const PLACEMENT_UPPER_CORNER: u32 = 250; // Goal, upper corner
pub const PLACEMENT_ANY_CORNER: u32 = 150; // Goal, any (lower) corner
pub const STREAK_STEP: u32 = 100; // per extra consecutive goal

/// Whether the normalized target is in the upper-left corner zone.
pub fn is_upper_left(target_x: i32, target_y: i32) -> bool {
    target_x <= -70 && target_y >= 70
}
/// Upper-right corner zone.
pub fn is_upper_right(target_x: i32, target_y: i32) -> bool {
    target_x >= 70 && target_y >= 70
}
/// Lower-left corner zone.
pub fn is_lower_left(target_x: i32, target_y: i32) -> bool {
    target_x <= -70 && target_y <= 30
}
/// Lower-right corner zone.
pub fn is_lower_right(target_x: i32, target_y: i32) -> bool {
    target_x >= 70 && target_y <= 30
}
/// Either upper corner.
pub fn is_upper_corner(target_x: i32, target_y: i32) -> bool {
    is_upper_left(target_x, target_y) || is_upper_right(target_x, target_y)
}
/// Any of the four corner zones.
pub fn is_any_corner(target_x: i32, target_y: i32) -> bool {
    is_upper_corner(target_x, target_y)
        || is_lower_left(target_x, target_y)
        || is_lower_right(target_x, target_y)
}

/// The breakdown of one score award.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyScoreBreakdown {
    pub base: u32,
    pub power_bonus: u32,
    pub placement_bonus: u32,
    pub streak_bonus: u32,
}

impl PenaltyScoreBreakdown {
    /// The total points from this breakdown.
    pub fn total(&self) -> u32 {
        self.base + self.power_bonus + self.placement_bonus + self.streak_bonus
    }
}

/// The full, deterministic record of one scored shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyScoreAward {
    pub round_number: u32,
    pub result_kind: PenaltyShotResultKind,
    pub base: u32,
    pub power_bonus: u32,
    pub placement_bonus: u32,
    pub streak_bonus: u32,
    pub total: u32,
    pub score_before: u32,
    pub score_after: u32,
    pub streak_before: u32,
    pub streak_after: u32,
}

impl PenaltyScoreAward {
    /// This award's breakdown.
    pub fn breakdown(&self) -> PenaltyScoreBreakdown {
        PenaltyScoreBreakdown {
            base: self.base,
            power_bonus: self.power_bonus,
            placement_bonus: self.placement_bonus,
            streak_bonus: self.streak_bonus,
        }
    }
}

/// The fixed scoring rules (a unit struct namespacing the deterministic rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyScoreRule;

impl PenaltyScoreRule {
    /// The base points for a result kind.
    pub fn base(kind: PenaltyShotResultKind) -> u32 {
        match kind {
            PenaltyShotResultKind::Goal => GOAL_BASE,
            PenaltyShotResultKind::Post => POST_BASE,
            PenaltyShotResultKind::Save => SAVE_BASE,
            PenaltyShotResultKind::Miss => MISS_BASE,
        }
    }

    /// The power-accuracy bonus (Goal only).
    pub fn power_bonus(kind: PenaltyShotResultKind, power: i32) -> u32 {
        if kind == PenaltyShotResultKind::Goal { {
                (70..=90)
                    .contains(&power)
                    .then_some(POWER_BONUS_SWEET)
                    .or((power > 90).then_some(POWER_BONUS_OVER))
                    .unwrap_or(0)
            } } else { 0 }
    }

    /// The placement bonus (Goal only).
    pub fn placement_bonus(kind: PenaltyShotResultKind, target_x: i32, target_y: i32) -> u32 {
        if kind == PenaltyShotResultKind::Goal { {
                is_upper_corner(target_x, target_y)
                    .then_some(PLACEMENT_UPPER_CORNER)
                    .or(is_any_corner(target_x, target_y).then_some(PLACEMENT_ANY_CORNER))
                    .unwrap_or(0)
            } } else { 0 }
    }

    /// The streak *after* this shot (increments on a Goal, resets otherwise).
    pub fn streak_after(kind: PenaltyShotResultKind, streak_before: u32) -> u32 {
        if kind == PenaltyShotResultKind::Goal {
            streak_before + 1
        } else {
            0
        }
    }

    /// The streak bonus: `+STREAK_STEP * (streak_after - 1)` for the 2nd+
    /// consecutive goal, else 0.
    pub fn streak_bonus(kind: PenaltyShotResultKind, streak_before: u32) -> u32 {
        let after = Self::streak_after(kind, streak_before);
        if kind == PenaltyShotResultKind::Goal && after >= 2 { STREAK_STEP * (after - 1) } else { 0 }
    }

    /// Award for one resolved shot: fully deterministic in its inputs.
    pub fn award(
        round_number: u32,
        result: PenaltyShotResult,
        power: i32,
        target_x: i32,
        target_y: i32,
        score_before: u32,
        streak_before: u32,
    ) -> PenaltyScoreAward {
        let kind = result.kind;
        let base = Self::base(kind);
        let power_bonus = Self::power_bonus(kind, power);
        let placement_bonus = Self::placement_bonus(kind, target_x, target_y);
        let streak_bonus = Self::streak_bonus(kind, streak_before);
        let total = base + power_bonus + placement_bonus + streak_bonus;
        PenaltyScoreAward {
            round_number,
            result_kind: kind,
            base,
            power_bonus,
            placement_bonus,
            streak_bonus,
            total,
            score_before,
            score_after: score_before + total,
            streak_before,
            streak_after: Self::streak_after(kind, streak_before),
        }
    }
}
