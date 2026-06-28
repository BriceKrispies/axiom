//! The resolved per-draw common attributes every command carries.

use axiom_kernel::{Meters, Ratio};

use crate::rgba::Rgba;

/// An optional soft shadow behind a draw.
///
/// Resolved (no defaults left to a backend): a colour and a blur radius. The
/// presence of a shadow is modelled by `Common2d::shadow` being `Some`/`None`,
/// so a command never carries an ambiguous "zero shadow".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow2d {
    pub color: Rgba,
    pub blur: Meters,
}

impl Shadow2d {
    /// Construct a shadow from a colour and a blur radius.
    pub const fn new(color: Rgba, blur: Meters) -> Self {
        Shadow2d { color, blur }
    }
}

/// The resolved common attributes shared by every 2D draw: its explicit
/// `layer` (z-order the backend must honour), its `alpha`, and an optional
/// `shadow`. Everything here is **already resolved** at the facade — no
/// defaults, no `Option` flow over `layer`/`alpha`, reach a backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Common2d {
    pub layer: i32,
    pub alpha: Ratio,
    pub shadow: Option<Shadow2d>,
}

impl Common2d {
    /// Common attributes with no shadow.
    pub const fn new(layer: i32, alpha: Ratio) -> Self {
        Common2d {
            layer,
            alpha,
            shadow: None,
        }
    }

    /// Common attributes carrying a shadow.
    pub const fn with_shadow(layer: i32, alpha: Ratio, shadow: Shadow2d) -> Self {
        Common2d {
            layer,
            alpha,
            shadow: Some(shadow),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn color() -> Rgba {
        Rgba::new(ratio(0.0), ratio(0.0), ratio(0.0), ratio(0.5))
    }

    #[test]
    fn new_has_no_shadow() {
        let c = Common2d::new(3, ratio(0.8));
        assert_eq!(c.layer, 3);
        assert_eq!(c.alpha, ratio(0.8));
        assert_eq!(c.shadow, None);
    }

    #[test]
    fn with_shadow_carries_the_shadow() {
        let s = Shadow2d::new(color(), meters(2.0));
        let c = Common2d::with_shadow(1, ratio(1.0), s);
        assert_eq!(c.shadow, Some(s));
        assert_eq!(c.shadow.map(|sh| sh.blur), Some(meters(2.0)));
    }
}
