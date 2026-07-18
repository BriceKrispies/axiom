//! The gameplay HUD view model: the minimal arcade read-out derived purely
//! from the authoritative [`DriveState`]. It carries exactly the five things
//! the in-game HUD shows — score, down, yards to go, the line-to-gain
//! indicator, and heat — and nothing else. The platform edge renders these
//! strings; it never computes them.

use crate::drive::DriveState;

/// The formatted HUD read-out for one tick of a live run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudView {
    /// `SCORE 012500`.
    pub score: String,
    /// `2ND & 6`, or `1ST & GOAL` near the end zone.
    pub down_distance: String,
    /// The line-to-gain indicator: `TO GAIN 6` or `GOAL LINE`.
    pub to_gain: String,
    /// `HEAT 3`.
    pub heat: String,
}

impl HudView {
    /// Derive the read-out from authoritative drive state.
    pub fn from_drive(drive: &DriveState) -> Self {
        let distance = drive.yards_to_go().round() as u32;
        let distance = distance.max(u32::from(drive.yards_to_go() > 0.05));
        let down_distance = if drive.goal_to_go() {
            format!("{} & GOAL", ordinal(drive.down))
        } else {
            format!("{} & {}", ordinal(drive.down), distance)
        };
        let to_gain = if drive.goal_to_go() {
            "GOAL LINE".to_string()
        } else {
            format!("TO GAIN {distance}")
        };
        HudView {
            score: format!("SCORE {:06}", drive.score),
            down_distance,
            to_gain,
            heat: format!("HEAT {}", drive.heat),
        }
    }
}

/// The arcade ordinal for a down (`1..=4`).
fn ordinal(down: u8) -> &'static str {
    match down {
        1 => "1ST",
        2 => "2ND",
        3 => "3RD",
        _ => "4TH",
    }
}
