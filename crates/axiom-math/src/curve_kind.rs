//! Which parametric family a [`crate::Curve`] belongs to.

/// The parametric family of a [`crate::Curve`].
///
/// The discriminant is stable and load-bearing: it is the index into the
/// `const` evaluation / derivative function tables in `curve.rs`, so curve
/// dispatch is a table lookup rather than control flow. Appending a variant
/// therefore means appending a table entry at the same index — never
/// reordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum CurveKind {
    /// Piecewise-linear through every point. Needs `>= 2` points.
    Polyline = 0,
    /// Chained cubic Bézier segments, each consuming `points[3i ..= 3i + 3]`
    /// (the last control point of a segment is the first of the next). Needs
    /// `3n + 1` points with `n >= 1`.
    CubicBezier = 1,
    /// Uniform Catmull-Rom through the interior points; the first and last
    /// points are tangent controls only. Needs `>= 4` points.
    CatmullRom = 2,
}

impl CurveKind {
    /// The stable numeric discriminant, which is also the dispatch-table index.
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable_and_dense() {
        assert_eq!(CurveKind::Polyline.raw(), 0);
        assert_eq!(CurveKind::CubicBezier.raw(), 1);
        assert_eq!(CurveKind::CatmullRom.raw(), 2);
    }

    #[test]
    fn variants_are_distinct_and_ordered() {
        assert_ne!(CurveKind::Polyline, CurveKind::CubicBezier);
        assert_ne!(CurveKind::CubicBezier, CurveKind::CatmullRom);
        assert!(CurveKind::Polyline < CurveKind::CatmullRom);
    }

    #[test]
    fn debug_names_the_variant() {
        assert_eq!(format!("{:?}", CurveKind::CatmullRom), "CatmullRom");
    }

    #[test]
    fn raw_indexes_a_three_entry_table() {
        // The contract `curve.rs` relies on: every discriminant is a valid
        // index into a three-entry dispatch table.
        const NAMES: [&str; 3] = ["polyline", "bezier", "catmull"];
        assert_eq!(NAMES[CurveKind::Polyline.raw() as usize], "polyline");
        assert_eq!(NAMES[CurveKind::CubicBezier.raw() as usize], "bezier");
        assert_eq!(NAMES[CurveKind::CatmullRom.raw() as usize], "catmull");
    }
}
