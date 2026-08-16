//! The four-type lattice a field value may inhabit.

/// The complete type lattice of the field language: a scalar and the three
/// vector widths. The discriminant **is** the type code written to the wire and
/// it indexes [`FieldType::ALL`], so this order is the wire order and must not be
/// reshuffled.
///
/// **Decisions, recorded so they are not relitigated:**
///
/// * **There is no `Color` type.** A colour is a [`FieldType::Vec4`] in *linear
///   RGBA*. Adding `Color` would double every signature row that already accepts
///   a `Vec4` and buys nothing this sentence does not.
/// * **There is no `Mask`/`Bool` type.** A mask is a [`FieldType::Scalar`] in
///   `0..=1`; clamping is a `Clamp` node. A boolean would require comparison and
///   selection operators, and selection is `Mix` — already in the algebra, and
///   branchless by construction.
/// * **There is no `Coordinate` type.** A coordinate is a [`FieldType::Vec3`].
///   The *space* it lives in is a property of the
///   [`crate::EvalContext`] the caller supplies, never of the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum FieldType {
    /// One component.
    Scalar = 0,
    /// Two components: `x`, `y`.
    Vec2 = 1,
    /// Three components: `x`, `y`, `z`.
    Vec3 = 2,
    /// Four components: `x`, `y`, `z`, `w` — and, by convention, linear RGBA.
    Vec4 = 3,
}

impl FieldType {
    /// Every type, in discriminant order. The array **is** the decode table.
    pub const ALL: [FieldType; 4] = [
        FieldType::Scalar,
        FieldType::Vec2,
        FieldType::Vec3,
        FieldType::Vec4,
    ];

    /// The wire code — the discriminant, which is also the index into
    /// [`FieldType::ALL`].
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// The number of meaningful lanes: `1`, `2`, `3`, or `4`.
    pub const fn lanes(self) -> u8 {
        [1, 2, 3, 4][self as usize]
    }

    /// The type a wire code names, or `None` if the code names no type. Table
    /// lookup, so decoding an unknown code is a bounds miss rather than a branch.
    pub fn from_code(code: u16) -> Option<FieldType> {
        FieldType::ALL.get(code as usize).copied()
    }

    /// The type a declared **lane width** names — the rule
    /// [`crate::FieldOp::Compose`] is read by, stated once so the type checker
    /// and the evaluator cannot disagree about what a width word means.
    ///
    /// Total by table: a width of `0` or `1` reads as [`FieldType::Scalar`] and
    /// anything past `4` reads as [`FieldType::Vec4`]. Validation
    /// (`ComposeWidthInvalid`) has already confined a real node's width to
    /// `2..=4`, so the clamped ends exist to keep both readers free of an
    /// unreachable error arm.
    pub(crate) fn of_width(width: u32) -> FieldType {
        const WIDTH_TYPE: [FieldType; 5] = [
            FieldType::Scalar,
            FieldType::Scalar,
            FieldType::Vec2,
            FieldType::Vec3,
            FieldType::Vec4,
        ];
        WIDTH_TYPE[(width as usize).min(4)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_their_table_indices() {
        assert_eq!(FieldType::Scalar as u16, 0);
        assert_eq!(FieldType::Vec2 as u16, 1);
        assert_eq!(FieldType::Vec3 as u16, 2);
        assert_eq!(FieldType::Vec4 as u16, 3);
        FieldType::ALL
            .iter()
            .enumerate()
            .for_each(|(index, ty)| assert_eq!(ty.code() as usize, index));
    }

    #[test]
    fn every_type_reports_its_lane_count() {
        assert_eq!(FieldType::Scalar.lanes(), 1);
        assert_eq!(FieldType::Vec2.lanes(), 2);
        assert_eq!(FieldType::Vec3.lanes(), 3);
        assert_eq!(FieldType::Vec4.lanes(), 4);
    }

    #[test]
    fn a_known_code_decodes_and_an_unknown_one_does_not() {
        assert_eq!(FieldType::from_code(0), Some(FieldType::Scalar));
        assert_eq!(FieldType::from_code(3), Some(FieldType::Vec4));
        assert_eq!(FieldType::from_code(4), None);
        assert_eq!(FieldType::from_code(u16::MAX), None);
    }

    #[test]
    fn a_declared_width_names_the_vector_type_it_builds() {
        assert_eq!(FieldType::of_width(2), FieldType::Vec2);
        assert_eq!(FieldType::of_width(3), FieldType::Vec3);
        assert_eq!(FieldType::of_width(4), FieldType::Vec4);
        // The clamped ends: validation never lets these reach a real node.
        assert_eq!(FieldType::of_width(0), FieldType::Scalar);
        assert_eq!(FieldType::of_width(1), FieldType::Scalar);
        assert_eq!(FieldType::of_width(u32::MAX), FieldType::Vec4);
    }

    #[test]
    fn types_order_and_hash_by_width() {
        assert!(FieldType::Scalar < FieldType::Vec4);
        let mut seen = std::collections::BTreeSet::new();
        FieldType::ALL.iter().for_each(|ty| {
            seen.insert(*ty);
        });
        assert_eq!(seen.len(), 4);
    }
}
