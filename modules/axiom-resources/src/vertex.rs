//! One vertex of a CPU-side mesh.

use axiom_math::{Vec2, Vec3, Vec4};

/// One CPU-side mesh vertex: position, normal, uv, and a per-vertex
/// colour. Plain data — no GPU layout assumptions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    position: Vec3,
    normal: Vec3,
    uv: Vec2,
    color: Vec4,
}

impl Vertex {
    pub const fn new(position: Vec3, normal: Vec3, uv: Vec2, color: Vec4) -> Self {
        Vertex {
            position,
            normal,
            uv,
            color,
        }
    }

    pub const fn position(&self) -> Vec3 {
        self.position
    }

    pub const fn normal(&self) -> Vec3 {
        self.normal
    }

    pub const fn uv(&self) -> Vec2 {
        self.uv
    }

    pub const fn color(&self) -> Vec4 {
        self.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let v = Vertex::new(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(0.5, 0.5),
            Vec4::new(1.0, 0.0, 0.0, 1.0),
        );
        assert_eq!(v.position(), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(v.normal(), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(v.uv(), Vec2::new(0.5, 0.5));
        assert_eq!(v.color(), Vec4::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = Vertex::new(Vec3::ZERO, Vec3::ZERO, Vec2::ZERO, Vec4::ZERO);
        let b = Vertex::new(Vec3::ZERO, Vec3::ZERO, Vec2::ZERO, Vec4::ZERO);
        let c = Vertex::new(Vec3::ONE, Vec3::ZERO, Vec2::ZERO, Vec4::ZERO);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
