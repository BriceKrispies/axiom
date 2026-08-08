//! The two kits.
//!
//! One figure, two palettes, indexed by the same opaque part tags — which is
//! exactly what the tag being opaque to `axiom-figure` buys: the module never
//! learns what a shirt is, and the game gets a keeper by changing seven colours.

use crate::figure::model::TAG_COUNT;

/// A kit: one colour per part tag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kit {
    pub slots: [[f32; 3]; TAG_COUNT],
}

/// The striker: a deep crimson shirt over white shorts, crimson socks, black
/// boots. Warm against a green pitch, and dark enough that the white ball reads
/// against it at speed.
pub fn kicker_kit() -> Kit {
    Kit {
        slots: [
            [0.74, 0.11, 0.18], // shirt
            [0.93, 0.94, 0.96], // shorts
            [0.74, 0.11, 0.18], // socks
            [0.76, 0.55, 0.39], // skin
            [0.07, 0.08, 0.10], // boots
            [0.15, 0.11, 0.09], // hair
            [0.76, 0.55, 0.39], // bare hands
        ],
    }
}

/// The keeper: high-visibility volt over black, with pale gloves. A keeper has
/// to be the most legible thing inside the frame — the player is reading its
/// dive, not admiring it — so it is the one saturated colour in the scene.
pub fn keeper_kit() -> Kit {
    Kit {
        slots: [
            [0.78, 0.90, 0.16], // shirt
            [0.10, 0.11, 0.13], // shorts
            [0.78, 0.90, 0.16], // socks
            [0.72, 0.51, 0.36], // skin
            [0.07, 0.08, 0.10], // boots
            [0.20, 0.15, 0.10], // hair
            [0.95, 0.97, 0.90], // gloves
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::model::{TAG_HANDS, TAG_SHIRT, TAG_SKIN};

    #[test]
    fn the_two_kits_are_different_and_both_are_in_gamut() {
        let (a, b) = (kicker_kit(), keeper_kit());
        assert_ne!(a, b);
        assert_ne!(a.slots[TAG_SHIRT as usize], b.slots[TAG_SHIRT as usize]);
        [a, b].iter().for_each(|kit| {
            kit.slots.iter().flatten().for_each(|c| {
                assert!((0.0..=1.0).contains(c), "{c} is out of gamut");
            });
        });
    }

    #[test]
    fn the_keeper_wears_gloves_and_the_striker_does_not() {
        let striker = kicker_kit();
        assert_eq!(
            striker.slots[TAG_HANDS as usize],
            striker.slots[TAG_SKIN as usize],
            "bare hands are the same colour as the arms"
        );
        let keeper = keeper_kit();
        assert_ne!(
            keeper.slots[TAG_HANDS as usize],
            keeper.slots[TAG_SKIN as usize],
            "the keeper's hands are gloves"
        );
    }
}
