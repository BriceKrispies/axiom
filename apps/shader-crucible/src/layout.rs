//! **Where each station stands, and how big it is.** The stand plan, separated
//! from the wiring that builds it.
//!
//! Twelve bodies in one row would be twenty-eight units wide and every station
//! would be a smudge. Two rows of six — the back one lifted clear of the front
//! one — keeps each body big enough to read the pattern on, at the cost of the
//! back row being further away and dimmer. That trade is the whole content of
//! this file, which is why it is a file: a demonstration whose subjects are too
//! small to see demonstrates nothing.
//!
//! ## `+x` renders screen-RIGHT here
//!
//! Measured, from the capture, not assumed: with the crucible's camera the body
//! at the **highest** `x` lands on the right of the frame. So a row a viewer
//! reads left to right in station order runs from `-x` **up**, and
//! [`slot_position`] is written that way. (The opposite convention holds in
//! other apps in this repo under other camera rigs; the only reliable way to
//! know is to read the screenshot, which is what was done.)

use axiom::prelude::*;

/// The authoring / capture size, and the window the app requests.
pub const WIDTH: u32 = 1280;
/// See [`WIDTH`].
pub const HEIGHT: u32 = 640;

/// How many bodies stand in the front row; the rest go behind it.
const ROW_LENGTH: usize = 6;
/// How far apart the bodies stand.
const SPACING: f32 = 2.55;
/// The `x` of the **first** body of a row — the leftmost on screen.
const ROW_START: f32 = -6.4;
/// The `y` a front-row body's centre sits at.
const ROW_Y: f32 = 0.0;
/// The ground plane's height.
pub const GROUND_Y: f32 = -1.2;
/// The `z` of the front row, and of the back row.
const ROW_Z: [f32; 2] = [1.9, -3.4];
/// How much higher the back row stands, so the front row does not occlude it.
const BACK_ROW_LIFT: f32 = 2.7;

/// Where the body in `slot` stands.
pub fn slot_position(slot: usize) -> Vec3 {
    let row = slot / ROW_LENGTH;
    let column = slot % ROW_LENGTH;
    Vec3::new(
        ROW_START + column as f32 * SPACING,
        ROW_Y + row as f32 * BACK_ROW_LIFT,
        ROW_Z[row.min(1)],
    )
}

/// A linear colour channel from a known-finite authored literal.
pub fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("an authored colour channel is finite")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows run left to right in station order, and the back row is lifted
    /// and pushed back so the front one does not occlude it.
    #[test]
    fn the_rows_run_left_to_right_in_station_order() {
        assert!(slot_position(0).x < slot_position(1).x);
        assert_eq!(slot_position(0).y, slot_position(5).y);
        assert!(slot_position(6).y > slot_position(0).y);
        assert!(slot_position(6).z < slot_position(0).z);
        assert_eq!(slot_position(6).x, slot_position(0).x);
    }

    /// Every body stands above the ground rather than sunk into it.
    #[test]
    fn every_body_stands_above_the_ground() {
        (0..12).for_each(|slot| assert!(slot_position(slot).y > GROUND_Y));
    }

    #[test]
    fn an_authored_channel_is_a_finite_ratio() {
        assert_eq!(ch(0.5).get(), 0.5);
    }
}
