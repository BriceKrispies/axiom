//! The per-node layout style: container behaviour + how the node sizes as a child.

use axiom_host::Pixels;
use axiom_kernel::Ratio;

use crate::insets::Insets;
use crate::style_enums::{Align, CrossSize, Direction, FlexWrap, Justify};

/// The layout style of one node — both how it arranges its own children (the
/// container fields) and how it sizes inside its parent (the item fields).
///
/// Start from [`Self::new`] (mobile-first defaults: a stretching row, no grow, no
/// padding) and set the fields you need. Every length is a host [`Pixels`] and
/// every ratio/weight a kernel [`Ratio`], so no naked float reaches the public
/// surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutStyle {
    /// How this node arranges its children along the main axis.
    pub direction: Direction,
    /// Whether children overflow onto new lines.
    pub wrap: FlexWrap,
    /// Distribution of leftover main-axis space.
    pub justify: Justify,
    /// Cross-axis alignment of children.
    pub align: Align,
    /// Space inserted between adjacent children.
    pub gap: Pixels,
    /// Inset between this node's box and the content box its children fill.
    pub padding: Insets,
    /// This node's preferred main-axis size inside its parent (before grow).
    pub basis: Pixels,
    /// This node's share of its parent's leftover main-axis space.
    pub grow: Ratio,
    /// Lower clamp on this node's main-axis size.
    pub min_main: Pixels,
    /// Upper clamp on this node's main-axis size (`None` = unbounded).
    pub max_main: Option<Pixels>,
    /// This node's cross-axis size inside its parent.
    pub cross: CrossSize,
    /// When set, the node's final box is letterboxed to this width:height ratio,
    /// centred — so e.g. a square board stays square inside whatever cell it gets.
    pub aspect: Option<Ratio>,
}

impl LayoutStyle {
    /// A style with mobile-first defaults: a single-line row that stretches its
    /// children on the cross axis, no grow, no padding, no aspect constraint.
    pub fn new() -> Self {
        let zero = Pixels::new(0.0).expect("zero is a finite pixel length");
        LayoutStyle {
            direction: Direction::Row,
            wrap: FlexWrap::NoWrap,
            justify: Justify::Start,
            align: Align::Stretch,
            gap: zero,
            padding: Insets::zero(),
            basis: zero,
            grow: Ratio::new(0.0).expect("zero is a finite ratio"),
            min_main: zero,
            max_main: None,
            cross: CrossSize::stretch(),
            aspect: None,
        }
    }
}

impl Default for LayoutStyle {
    fn default() -> Self {
        LayoutStyle::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_mobile_first_defaults() {
        let s = LayoutStyle::new();
        assert_eq!(s.direction, Direction::Row);
        assert_eq!(s.wrap, FlexWrap::NoWrap);
        assert_eq!(s.justify, Justify::Start);
        assert_eq!(s.align, Align::Stretch);
        assert_eq!(s.gap.get(), 0.0);
        assert_eq!(s.basis.get(), 0.0);
        assert_eq!(s.grow.get(), 0.0);
        assert_eq!(s.min_main.get(), 0.0);
        assert!(s.max_main.is_none());
        assert_eq!(s.cross, CrossSize::stretch());
        assert!(s.aspect.is_none());
        assert_eq!(s.padding, Insets::zero());
    }

    #[test]
    fn default_matches_new_and_fields_are_settable() {
        assert_eq!(LayoutStyle::default(), LayoutStyle::new());
        let mut s = LayoutStyle::new();
        s.direction = Direction::Column;
        s.grow = Ratio::new(1.0).unwrap();
        s.basis = Pixels::new(332.0).unwrap();
        s.aspect = Some(Ratio::new(1.5).unwrap());
        assert_eq!(s.direction, Direction::Column);
        assert_eq!(s.grow.get(), 1.0);
        assert_eq!(s.basis.get(), 332.0);
        assert_eq!(s.aspect.map(|r| r.get()), Some(1.5));
    }
}
