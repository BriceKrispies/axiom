//! The rainbow the two rings are coloured across: one hue angle in, one linear
//! RGB triple out.
//!
//! This is app presentation policy, not a colour space library. `axiom-mesh` and
//! `axiom-mesh-ops` know about triangles; "which colour is the seventh dog"
//! is a question about *this* scene, so the answer is authored here.
//!
//! It is a pure function with no table and no randomness: the same hue gives the
//! same triple in every process, which is what lets a test assert that the ring
//! spans the circle and that no two neighbours share a colour.
//!
//! ## Why the saturation and value are held below 1
//!
//! The engine's materials are **linear** RGB and the scene is lit by a 0.85 sun
//! plus two point fills before it is tone-mapped. A fully saturated primary at
//! value 1 therefore leaves the tone-mapper nothing to do but clip: the lit side
//! of the dog goes to flat white and the hue that was the whole point of the
//! ring survives only in the shadow. Backing both dials off keeps every dog's
//! hue readable on its lit side, which is the only side a camera above the ring
//! ever sees.

/// How far from grey a ring's colours sit.
pub const RING_SATURATION: f32 = 0.68;

/// How bright, in linear RGB, before the light rig and the tone-mapper touch it.
pub const RING_VALUE: f32 = 0.72;

/// The ring palette's colour at `hue`, where `hue` is a turn around the colour
/// circle (`0.0` red, `1/3` green, `2/3` blue, `1.0` red again). Values outside
/// `0..1` wrap, so a caller may hand it `phase + index / count` without folding
/// it first.
pub fn hue_to_rgb(hue: f32) -> [f32; 3] {
    hsv_to_rgb(hue, RING_SATURATION, RING_VALUE)
}

/// Hue/saturation/value to linear RGB — the standard six-sector construction,
/// written out rather than pulled in, because one function is not a dependency.
pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);
    let turned = hue.rem_euclid(1.0) * 6.0;
    let sector = turned.floor();
    let fraction = turned - sector;
    let dark = value * (1.0 - saturation);
    let falling = value * (1.0 - saturation * fraction);
    let rising = value * (1.0 - saturation * (1.0 - fraction));
    match (sector as i32).rem_euclid(6) {
        0 => [value, rising, dark],
        1 => [falling, value, dark],
        2 => [dark, value, rising],
        3 => [dark, falling, value],
        4 => [rising, dark, value],
        _ => [value, dark, falling],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_six_corners_of_the_circle_are_the_six_pure_hues() {
        // Red, yellow, green, cyan, blue, magenta: at every sixth of a turn the
        // triple has one channel at `value`, one at `dark`, and the third at one
        // end or the other — never a muddle in between.
        let dark = RING_VALUE * (1.0 - RING_SATURATION);
        for sixth in 0..6 {
            let rgb = hue_to_rgb(sixth as f32 / 6.0);
            let high = rgb.iter().copied().fold(0.0f32, f32::max);
            let low = rgb.iter().copied().fold(f32::INFINITY, f32::min);
            assert!((high - RING_VALUE).abs() < 1.0e-5, "{sixth}: {rgb:?}");
            assert!((low - dark).abs() < 1.0e-5, "{sixth}: {rgb:?}");
        }
    }

    #[test]
    fn the_hue_wraps_and_every_neighbouring_step_is_a_different_colour() {
        assert_eq!(hue_to_rgb(0.0), hue_to_rgb(1.0));
        assert_eq!(hue_to_rgb(0.25), hue_to_rgb(-0.75));
        let wheel: Vec<[f32; 3]> = (0..12).map(|i| hue_to_rgb(i as f32 / 12.0)).collect();
        for (a, b) in wheel.iter().zip(wheel.iter().skip(1)) {
            let apart: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum();
            assert!(apart > 0.05, "{a:?} and {b:?} are the same colour");
        }
    }

    #[test]
    fn a_grey_has_no_hue_and_the_dials_are_clamped_not_wrapped() {
        let grey = hsv_to_rgb(0.4, 0.0, 0.5);
        assert_eq!(grey, [0.5, 0.5, 0.5]);
        // Out-of-range dials clamp: a colour is never brighter than white or
        // more saturated than pure.
        assert_eq!(hsv_to_rgb(0.0, 4.0, 9.0), [1.0, 0.0, 0.0]);
        assert_eq!(hsv_to_rgb(0.0, -1.0, -1.0), [0.0, 0.0, 0.0]);
    }
}
