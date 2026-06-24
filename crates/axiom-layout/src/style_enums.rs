//! The flex enums and their branchless selection helpers.
//!
//! Each fieldless enum carries a small `pub(crate)` helper that the solver uses to
//! turn the variant into an arithmetic factor or flag by discriminant-indexing a
//! table — so the solver selects behaviour without any control flow.

use axiom_host::Pixels;

/// How a node arranges its children along its **main axis**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Children flow left → right (main axis horizontal).
    Row,
    /// Children flow top → bottom (main axis vertical).
    Column,
    /// **Mobile-first:** `Row` in landscape, `Column` otherwise (portrait/square),
    /// so side-by-side content stacks vertically on a narrow/upright screen.
    Adaptive,
}

impl Direction {
    /// Whether the resolved main axis is horizontal, given the viewport's
    /// landscape-ness. Branchless: a table indexed by the direction discriminant,
    /// with `Adaptive` reading the landscape flag.
    pub(crate) fn main_is_horizontal(self, is_landscape: bool) -> bool {
        [true, false, is_landscape][self as usize]
    }
}

/// Whether children overflow onto new lines or stay on one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlexWrap {
    /// All children stay on a single line (may overflow the content box).
    NoWrap,
    /// Children that don't fit start a new line along the cross axis.
    Wrap,
}

impl FlexWrap {
    /// Whether new lines are allowed. Branchless: a discriminant comparison.
    pub(crate) fn wraps(self) -> bool {
        (self as usize) != 0
    }
}

/// Distribution of leftover main-axis space *around* the children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Justify {
    /// Pack at the main-axis start.
    Start,
    /// Centre the run of children.
    Center,
    /// Pack at the main-axis end.
    End,
    /// Spread leftover space evenly *between* children.
    SpaceBetween,
}

impl Justify {
    /// The fraction of leftover main space placed *before* the children (the
    /// leading offset). Branchless table by discriminant.
    pub(crate) fn leading_fraction(self) -> f32 {
        [0.0, 0.5, 1.0, 0.0][self as usize]
    }

    /// The fraction of leftover main space inserted *between* each adjacent pair
    /// (only `SpaceBetween` does this). Branchless table by discriminant.
    pub(crate) fn between_fraction(self) -> f32 {
        [0.0, 0.0, 0.0, 1.0][self as usize]
    }
}

/// Cross-axis alignment of children within their line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    /// Align to the cross-axis start.
    Start,
    /// Centre on the cross axis.
    Center,
    /// Align to the cross-axis end.
    End,
    /// Fill the line's cross extent (overrides a fixed cross size).
    Stretch,
}

impl Align {
    /// The fraction of the leftover cross space placed *before* a child (its cross
    /// offset within the line). Branchless table by discriminant.
    pub(crate) fn leading_fraction(self) -> f32 {
        [0.0, 0.5, 1.0, 0.0][self as usize]
    }

    /// Whether children are stretched to fill the line's cross extent. Branchless
    /// discriminant comparison.
    pub(crate) fn stretches(self) -> bool {
        (self as usize) == 3
    }
}

/// A child's size along the **cross axis**: either stretched to the line's cross
/// extent, or a fixed length. Modelled as an `Option<Pixels>` (none = stretch) so
/// the solver resolves it with a branchless `map_or`, not a `match`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossSize(Option<Pixels>);

impl CrossSize {
    /// Fill the line's cross extent.
    pub const fn stretch() -> Self {
        CrossSize(None)
    }

    /// A fixed cross-axis length.
    pub const fn fixed(length: Pixels) -> Self {
        CrossSize(Some(length))
    }

    /// Resolve to a concrete cross length: the fixed length, or `stretched` (the
    /// line's cross extent) when stretching. Branchless via `map_or`.
    pub(crate) fn resolve(self, stretched: f32) -> f32 {
        self.0.map_or(stretched, |length| length.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_resolves_main_axis_branchlessly() {
        assert!(Direction::Row.main_is_horizontal(true));
        assert!(Direction::Row.main_is_horizontal(false));
        assert!(!Direction::Column.main_is_horizontal(true));
        // Adaptive follows the landscape flag: horizontal in landscape, vertical not.
        assert!(Direction::Adaptive.main_is_horizontal(true));
        assert!(!Direction::Adaptive.main_is_horizontal(false));
    }

    #[test]
    fn flex_wrap_reports_whether_it_wraps() {
        assert!(!FlexWrap::NoWrap.wraps());
        assert!(FlexWrap::Wrap.wraps());
    }

    #[test]
    fn justify_factors_match_each_variant() {
        assert_eq!(Justify::Start.leading_fraction(), 0.0);
        assert_eq!(Justify::Center.leading_fraction(), 0.5);
        assert_eq!(Justify::End.leading_fraction(), 1.0);
        assert_eq!(Justify::SpaceBetween.leading_fraction(), 0.0);
        assert_eq!(Justify::SpaceBetween.between_fraction(), 1.0);
        assert_eq!(Justify::Center.between_fraction(), 0.0);
    }

    #[test]
    fn align_factors_and_stretch_flag() {
        assert_eq!(Align::Start.leading_fraction(), 0.0);
        assert_eq!(Align::Center.leading_fraction(), 0.5);
        assert_eq!(Align::End.leading_fraction(), 1.0);
        assert_eq!(Align::Stretch.leading_fraction(), 0.0);
        assert!(Align::Stretch.stretches());
        assert!(!Align::Center.stretches());
    }

    #[test]
    fn cross_size_resolves_stretch_and_fixed() {
        assert_eq!(CrossSize::stretch().resolve(120.0), 120.0);
        assert_eq!(
            CrossSize::fixed(Pixels::new(40.0).unwrap()).resolve(120.0),
            40.0
        );
    }
}
