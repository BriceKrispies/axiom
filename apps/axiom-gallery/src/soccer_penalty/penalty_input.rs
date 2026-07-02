//! Pass 4 — the app-local deterministic input intent.
//!
//! `PenaltyInputIntent` is the soccer app's tiny, deterministic input *contract*:
//! the abstract "what the player is asking for this tick", decoupled from any
//! device. It reads **no** browser/host APIs itself — a future host/browser
//! layer translates real keyboard/gamepad/touch input into this struct, and the
//! app core consumes only the struct. That keeps the interaction model fully
//! deterministic and testable (no wall-clock, no randomness, fixed ticks).
//!
//! This is intentionally *not* a general input-mapping framework — it is one
//! fixed struct for one app.

/// Inclusive bounds for the aim axes.
pub const AIM_AXIS_MIN: i32 = -100;
pub const AIM_AXIS_MAX: i32 = 100;

/// Clamp a raw axis value into `-100..=100`.
pub fn clamp_axis(v: i32) -> i32 {
    v.clamp(AIM_AXIS_MIN, AIM_AXIS_MAX)
}

/// One tick of deterministic player intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyInputIntent {
    /// Horizontal aim, `-100..=100` (negative = left, positive = right).
    pub aim_x_axis: i32,
    /// Vertical aim, `-100..=100` (negative = down, positive = up).
    pub aim_y_axis: i32,
    /// The player is holding the shot button (charge power).
    pub charge_pressed: bool,
    /// The player released the shot button this tick (freeze / lock).
    pub release_pressed: bool,
    /// The player asked to reset aim + power to the start.
    pub reset_pressed: bool,
    /// The player asked to continue from a between-rounds prompt (Pass 9).
    pub continue_pressed: bool,
}

impl PenaltyInputIntent {
    /// No input this tick.
    pub const NEUTRAL: Self = Self {
        aim_x_axis: 0,
        aim_y_axis: 0,
        charge_pressed: false,
        release_pressed: false,
        reset_pressed: false,
        continue_pressed: false,
    };

    /// No input this tick (same as [`Self::NEUTRAL`]).
    pub const fn neutral() -> Self {
        Self::NEUTRAL
    }

    /// Move the aim with the given axes (clamped), no buttons.
    pub fn aiming(aim_x_axis: i32, aim_y_axis: i32) -> Self {
        Self { aim_x_axis: clamp_axis(aim_x_axis), aim_y_axis: clamp_axis(aim_y_axis), ..Self::NEUTRAL }
    }

    /// Hold charge while (optionally) moving the aim.
    pub fn charging(aim_x_axis: i32, aim_y_axis: i32) -> Self {
        Self {
            aim_x_axis: clamp_axis(aim_x_axis),
            aim_y_axis: clamp_axis(aim_y_axis),
            charge_pressed: true,
            ..Self::NEUTRAL
        }
    }

    /// Release the shot (freeze into a locked preview).
    pub fn releasing() -> Self {
        Self { release_pressed: true, ..Self::NEUTRAL }
    }

    /// Reset aim + power back to the start.
    pub fn resetting() -> Self {
        Self { reset_pressed: true, ..Self::NEUTRAL }
    }

    /// Continue from a between-rounds prompt (Pass 9).
    pub fn continuing() -> Self {
        Self { continue_pressed: true, ..Self::NEUTRAL }
    }
}

impl Default for PenaltyInputIntent {
    fn default() -> Self {
        Self::NEUTRAL
    }
}
