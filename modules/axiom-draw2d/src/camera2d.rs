//! The resolved 2D camera carried on a [`crate::Draw2dList`].

use axiom_kernel::Ratio;
use axiom_math::Vec2;

/// A resolved 2D camera: the world-space `center` the view is framed on and a
/// `zoom` factor. The list carries it as `Option<Camera2d>` — `None` means the
/// author never set one, so the backend applies its identity framing; the core
/// resolves no default in its place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera2d {
    pub center: Vec2,
    pub zoom: Ratio,
}

impl Camera2d {
    /// Construct a camera from its centre and zoom factor.
    pub const fn new(center: Vec2, zoom: Ratio) -> Self {
        Camera2d { center, zoom }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip() {
        let z = Ratio::new(2.0).unwrap();
        let c = Camera2d::new(Vec2::new(10.0, 20.0), z);
        assert_eq!(c.center, Vec2::new(10.0, 20.0));
        assert_eq!(c.zoom, z);
    }

    #[test]
    fn equality_compares_fields() {
        let z = Ratio::new(1.0).unwrap();
        assert_eq!(Camera2d::new(Vec2::ZERO, z), Camera2d::new(Vec2::ZERO, z));
        assert_ne!(Camera2d::new(Vec2::ZERO, z), Camera2d::new(Vec2::ONE, z));
    }
}
