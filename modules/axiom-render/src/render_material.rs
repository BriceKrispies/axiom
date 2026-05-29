//! Render-facing material data.

use axiom_math::Vec4;

/// Render-facing material: an opaque id and a base colour. The
/// vertical slice only supports the "basic lit" pipeline; richer
/// material models live in future iterations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderMaterial {
    id: u64,
    base_color: Vec4,
}

impl RenderMaterial {
    pub const fn new(id: u64, base_color: Vec4) -> Self {
        RenderMaterial { id, base_color }
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn base_color(&self) -> Vec4 {
        self.base_color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let m = RenderMaterial::new(3, Vec4::new(0.5, 0.5, 0.5, 1.0));
        assert_eq!(m.id(), 3);
        assert_eq!(m.base_color(), Vec4::new(0.5, 0.5, 0.5, 1.0));
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderMaterial::new(1, Vec4::ONE);
        let b = RenderMaterial::new(1, Vec4::ONE);
        let c = RenderMaterial::new(1, Vec4::ZERO);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
