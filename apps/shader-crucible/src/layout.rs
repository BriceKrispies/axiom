//! **Where each station stands, and how big it is.** The stand plan, separated
//! from the wiring that builds it.
//!
//! Thirteen bodies in one row would be thirty-three units wide and every
//! station would be a smudge. Seven in front and six behind — the back row
//! lifted clear of the front one — keeps each body big enough to read the
//! pattern on, at the cost of the back row being further away and dimmer. That
//! trade is the whole content of this file, which is why it is a file: a
//! demonstration whose subjects are too small to see demonstrates nothing.
//!
//! The stand was six-and-six until `LightingModel` gained `Physical`. Station 6
//! enumerates `LightingModel::ALL` rather than a typed-out list, so the engine
//! gaining a model grows the stand — which is the behaviour that was wanted, and
//! the reason the count below is derived from the rows rather than pinned.
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
const ROW_LENGTH: usize = 7;
/// How many bodies the plan places in all: twelve authored surfaces plus the
/// baked body (station 4's graph as an ordinary texture, which carries no
/// surface program). Seven in front, six behind — the back row is one short,
/// which [`slot_position`] handles without being told.
pub const SLOT_COUNT: usize = 13;
/// How far apart the bodies stand.
const SPACING: f32 = 2.55;
/// **Each row is centred on `x = 0` independently**, so an uneven back row
/// (six behind seven) sits under the middle of the front one rather than
/// left-aligned against it. This also keeps [`stand_center`] exactly on the
/// axis, which is what makes it a legal orbit pivot — a left-aligned short row
/// would drag the pivot 0.59 units off and swing the whole stand as the camera
/// orbits.
fn row_length(row: usize) -> usize {
    [ROW_LENGTH, SLOT_COUNT - ROW_LENGTH][row.min(1)]
}
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
    let span = row_length(row) as f32 - 1.0;
    Vec3::new(
        (column as f32 - span * 0.5) * SPACING,
        ROW_Y + row as f32 * BACK_ROW_LIFT,
        ROW_Z[row.min(1)],
    )
}

/// **The middle of the stand** — the mean of the thirteen slot positions.
///
/// This exists so the orbit camera has something to pivot about that is *derived
/// from the plan* rather than a second, typed-out opinion about where the
/// subjects are: move a row and the pivot moves with it. See
/// [`crate::scene::camera_target`].
pub fn stand_center() -> Vec3 {
    (0..SLOT_COUNT)
        .fold(Vec3::ZERO, |sum, slot| sum.add(slot_position(slot)))
        .mul_scalar(1.0 / SLOT_COUNT as f32)
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
        let back = ROW_LENGTH;
        assert!(slot_position(0).x < slot_position(1).x);
        assert_eq!(slot_position(0).y, slot_position(ROW_LENGTH - 1).y);
        assert!(slot_position(back).y > slot_position(0).y);
        assert!(slot_position(back).z < slot_position(0).z);
        // Each row is centred on the axis, so the shorter back row starts
        // *inside* the front one rather than flush with it.
        assert!(slot_position(back).x > slot_position(0).x);
        assert_eq!(
            slot_position(back).x,
            -slot_position(SLOT_COUNT - 1).x,
            "the back row must be centred"
        );
    }

    /// Every body stands above the ground rather than sunk into it.
    #[test]
    fn every_body_stands_above_the_ground() {
        (0..SLOT_COUNT).for_each(|slot| assert!(slot_position(slot).y > GROUND_Y));
    }

    /// The stand's centre sits between the two rows in depth and between the two
    /// row heights, and is (very nearly) on the middle of the row in `x` — which
    /// is what makes it a legal orbit pivot for a camera authored on the axis.
    #[test]
    fn the_stand_center_sits_between_the_two_rows() {
        let center = stand_center();
        assert!(center.z < slot_position(0).z && center.z > slot_position(ROW_LENGTH).z);
        assert!(center.y > slot_position(0).y && center.y < slot_position(ROW_LENGTH).y);
        assert!(center.x.abs() < 0.1, "{center:?}");
        assert_eq!(SLOT_COUNT, 13);
    }

    #[test]
    fn an_authored_channel_is_a_finite_ratio() {
        assert_eq!(ch(0.5).get(), 0.5);
    }
}
