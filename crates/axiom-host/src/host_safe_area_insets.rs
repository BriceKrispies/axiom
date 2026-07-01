//! Host-supplied safe-area insets (notch / rounded-corner / system-UI margins).

use crate::host_error::HostError;
use crate::host_result::HostResult;
use crate::pixels::Pixels;

/// The inset, in logical pixels, from each edge of the surface to the
/// rectangle that is guaranteed unobscured by system UI — notches, rounded
/// corners, the home indicator, or browser chrome.
///
/// These are **host facts**, validated like any other: a browser adapter reads
/// them from the CSS `env(safe-area-inset-*)` values; a native adapter from the
/// platform display-cutout API; a headless harness supplies [`Self::none`]. The
/// engine never invents them — it receives them as data, exactly as it receives
/// the viewport scale factor. Each inset is a non-negative [`Pixels`] length, so
/// a UI laid out inside `[left, top, width-right, height-bottom]` is always on
/// screen, on a phone with a notch just as on a desktop with none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostSafeAreaInsets {
    top: Pixels,
    right: Pixels,
    bottom: Pixels,
    left: Pixels,
}

impl HostSafeAreaInsets {
    /// Construct from the four edge insets, rejecting any negative value with
    /// [`HostError::invalid_safe_area_insets`]. Finiteness is already
    /// guaranteed by [`Pixels`]; this constructor adds the non-negativity
    /// invariant (an inset *into* the surface cannot be negative).
    pub fn new(top: Pixels, right: Pixels, bottom: Pixels, left: Pixels) -> HostResult<Self> {
        ((top.get() >= 0.0) & (right.get() >= 0.0) & (bottom.get() >= 0.0) & (left.get() >= 0.0))
            .then_some(HostSafeAreaInsets {
                top,
                right,
                bottom,
                left,
            })
            .ok_or_else(|| {
                HostError::invalid_safe_area_insets("safe-area insets must be non-negative")
            })
    }

    /// The zero-inset case: the whole surface is safe. This is the default a
    /// viewport carries until a host supplies cutout information, and the exact
    /// answer on hardware without any system intrusion (most desktops).
    pub fn none() -> Self {
        let zero = Pixels::new(0.0).expect("zero is a finite, non-negative pixel length");
        HostSafeAreaInsets {
            top: zero,
            right: zero,
            bottom: zero,
            left: zero,
        }
    }

    /// The inset from the top edge.
    pub const fn top(&self) -> Pixels {
        self.top
    }

    /// The inset from the right edge.
    pub const fn right(&self) -> Pixels {
        self.right
    }

    /// The inset from the bottom edge.
    pub const fn bottom(&self) -> Pixels {
        self.bottom
    }

    /// The inset from the left edge.
    pub const fn left(&self) -> Pixels {
        self.left
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    fn px(value: f32) -> Pixels {
        Pixels::new(value).unwrap()
    }

    #[test]
    fn valid_insets_round_trip() {
        let insets = HostSafeAreaInsets::new(px(44.0), px(0.0), px(34.0), px(0.0)).unwrap();
        assert_eq!(insets.top(), px(44.0));
        assert_eq!(insets.right(), px(0.0));
        assert_eq!(insets.bottom(), px(34.0));
        assert_eq!(insets.left(), px(0.0));
    }

    #[test]
    fn none_is_all_zero() {
        let insets = HostSafeAreaInsets::none();
        assert_eq!(insets.top(), px(0.0));
        assert_eq!(insets.right(), px(0.0));
        assert_eq!(insets.bottom(), px(0.0));
        assert_eq!(insets.left(), px(0.0));
    }

    #[test]
    fn exactly_zero_is_accepted() {
        // Boundary for `>= 0.0`: zero is a valid inset. A `> 0.0` mutant would
        // wrongly reject the (extremely common) zero edge.
        assert!(HostSafeAreaInsets::new(px(0.0), px(0.0), px(0.0), px(0.0)).is_ok());
    }

    #[test]
    fn negative_top_is_rejected() {
        let err = HostSafeAreaInsets::new(px(-1.0), px(0.0), px(0.0), px(0.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidSafeAreaInsets);
    }

    #[test]
    fn negative_right_is_rejected() {
        let err = HostSafeAreaInsets::new(px(0.0), px(-1.0), px(0.0), px(0.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidSafeAreaInsets);
    }

    #[test]
    fn negative_bottom_is_rejected() {
        let err = HostSafeAreaInsets::new(px(0.0), px(0.0), px(-1.0), px(0.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidSafeAreaInsets);
    }

    #[test]
    fn negative_left_is_rejected() {
        let err = HostSafeAreaInsets::new(px(0.0), px(0.0), px(0.0), px(-1.0)).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidSafeAreaInsets);
    }

    #[test]
    fn insets_are_copy_and_equal() {
        let a = HostSafeAreaInsets::new(px(10.0), px(20.0), px(30.0), px(40.0)).unwrap();
        let b = a;
        assert_eq!(a, b);
    }
}
