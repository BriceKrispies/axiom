//! Render-facing camera data (view + projection matrices only).

use axiom_math::Mat4;

/// A render-facing camera: the view and projection matrices the
/// shader needs. The renderer does not know about scene nodes or
/// camera intrinsics — the app pre-computes both matrices and hands
/// them in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCamera {
    view: Mat4,
    projection: Mat4,
}

impl RenderCamera {
    pub const fn new(view: Mat4, projection: Mat4) -> Self {
        RenderCamera { view, projection }
    }

    pub const fn view(&self) -> Mat4 {
        self.view
    }

    pub const fn projection(&self) -> Mat4 {
        self.projection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let c = RenderCamera::new(Mat4::IDENTITY, Mat4::ZERO);
        assert_eq!(c.view(), Mat4::IDENTITY);
        assert_eq!(c.projection(), Mat4::ZERO);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderCamera::new(Mat4::IDENTITY, Mat4::IDENTITY);
        let b = RenderCamera::new(Mat4::IDENTITY, Mat4::IDENTITY);
        let c = RenderCamera::new(Mat4::ZERO, Mat4::IDENTITY);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
