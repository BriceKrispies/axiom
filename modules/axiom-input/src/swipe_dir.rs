//! [`SwipeDir`] — the four-way discrete form a completed touch gesture's unit
//! direction is quantized into at the sampling boundary.

use axiom_math::Vec2;

/// The cardinal direction of a completed swipe (screen space: `+x` right, `+y`
/// down). The discriminated form the touch synthesizer's unit `Vec2` becomes, so
/// a turn-based game reads a direction, never a vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwipeDir {
    /// Toward the top of the surface (negative `y`).
    Up,
    /// Toward the bottom of the surface (positive `y`).
    Down,
    /// Toward the left of the surface (negative `x`).
    Left,
    /// Toward the right of the surface (positive `x`).
    Right,
}

impl SwipeDir {
    /// Quantize a unit drag direction to its dominant cardinal. Branchless: the
    /// dominant axis and each axis's sign select one of the four variants by
    /// table index. A tie (`|x| == |y|`) resolves to the horizontal axis.
    pub(crate) fn from_unit(direction: Vec2) -> Self {
        let horizontal = usize::from(direction.x.abs() >= direction.y.abs());
        let positive_x = usize::from(direction.x >= 0.0);
        let positive_y = usize::from(direction.y >= 0.0);
        let across = [SwipeDir::Left, SwipeDir::Right][positive_x];
        let along = [SwipeDir::Up, SwipeDir::Down][positive_y];
        [along, across][horizontal]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizes_each_cardinal_unit_direction() {
        assert_eq!(SwipeDir::from_unit(Vec2::new(1.0, 0.0)), SwipeDir::Right);
        assert_eq!(SwipeDir::from_unit(Vec2::new(-1.0, 0.0)), SwipeDir::Left);
        // Screen +y is down, so +y is a downward swipe and -y an upward one.
        assert_eq!(SwipeDir::from_unit(Vec2::new(0.0, 1.0)), SwipeDir::Down);
        assert_eq!(SwipeDir::from_unit(Vec2::new(0.0, -1.0)), SwipeDir::Up);
    }

    #[test]
    fn dominant_axis_decides_a_diagonal() {
        assert_eq!(SwipeDir::from_unit(Vec2::new(-0.8, 0.6)), SwipeDir::Left);
        assert_eq!(SwipeDir::from_unit(Vec2::new(0.6, -0.8)), SwipeDir::Up);
        assert_eq!(SwipeDir::from_unit(Vec2::new(0.5, 0.5)), SwipeDir::Right);
    }
}
