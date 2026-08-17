//! The closed 27-operator algebra of the field language.

use core::fmt;

/// How many operators the algebra has, and the width of every `const` table
/// indexed by an operator code.
///
/// This is no longer bounded by the `engine_no_large_enums` cap of 24:
/// [`FieldOp`] is a `u16` newtype with a `const` catalog, not an enum, so the
/// count is bounded by the *admission test* in `ARCHITECTURE.md` and by nothing
/// mechanical. That is the point of the newtype — the cap was never a statement
/// about the algebra, only about the shape that carried it.
pub const FIELD_OP_COUNT: usize = 27;

/// One field operator: **a `u16` code with a `const` catalog**, deliberately not
/// an enum.
///
/// The code **is** the operator code stored in an [`axiom_recipe::Node`], and it
/// indexes [`FieldOp::ALL`], the `SIGNATURES` table, the dispatch table and the
/// backend's emission table alike, so this numbering is the dispatch order and
/// **codes are frozen once published**.
///
/// ## Why a newtype and not an enum
///
/// `engine_no_large_enums` caps an enum at 24 variants, and the fix it prescribes
/// — nested sub-enums — resets the count per level, which is exactly wrong here:
/// the dispatch technique needs **one flat discriminant space indexing one
/// `const` table**. A newtype with associated constants is not an enum, the lint
/// does not apply, the code stays the table index, and the shape already has two
/// precedents in this repository (`RenderPipelineKind::{BASIC_LIT, UNLIT}` as
/// `u32` consts, and `axiom_recipe::Node::op`, which has always been a bare
/// `u16`).
///
/// **The wire format did not change when this stopped being an enum.** Codes
/// 0..=22 keep the values their variants had, so every serialized graph, every
/// committed golden and every digest minted before the conversion is still valid.
/// `crates/axiom-field/tests/eval_golden.rs` pins that, and so does the
/// `the_published_operator_codes_are_frozen` test below.
///
/// **What the newtype costs:** an exhaustive `match` over operators. Nothing in
/// the spine matches on a `FieldOp` — dispatch is table-indexed by construction,
/// and the Branchless Law forbids the `match` anyway — so the cost is zero here.
/// Code that wants to enumerate operators uses [`FieldOp::ALL`].
///
/// **The inner code is private**, which is what keeps every `const` table indexed
/// by it total: the only ways to obtain a `FieldOp` are the catalog constants and
/// [`FieldOp::from_code`], and both yield a code below [`FIELD_OP_COUNT`].
///
/// The algebra is **closed**: there is no registry, no runtime-extensible verb
/// and no dynamic dispatch. A new visual effect is a new *graph*, never a new
/// Rust function — that is the entire point of the layer.
///
/// **What is deliberately not an operator, and why:**
///
/// | Excluded | Reason |
/// |---|---|
/// | `Div` | Division by zero is a determinism hazard and a NaN source. Multiply by a constant reciprocal, or use [`FieldOp::Pow`] with a negative exponent, whose zero behaviour is documented. |
/// | `Log` / `Atan2` | No consumer yet. The admission test needs two unrelated ones. |
/// | `Sqrt` | Already reachable as `Pow(x, 0.5)` and as [`FieldOp::Length`]. |
/// | `Step` | [`FieldOp::Smoothstep`] with equal edges. |
/// | `Cross` | Expressible with [`FieldOp::Compose`] plus arithmetic. |
/// | `dpdx`/`dpdy` | Screen-space derivatives are backend-specific, absent on the CPU, and the cause of a real past defect. Height-to-normal is finite differences at a caller-supplied offset. |
/// | `If`/`Select`/`Compare` | Selection is [`FieldOp::Mix`]. A comparison operator is the seed of control flow in a language that must stay branchless end to end. |
/// | `Texture`/`Sample` | A texture is a rendering resource; a field that samples an image is a later, separate decision. |
/// | marble/wood/rust/dirt/asphalt | Library graphs, not primitives. |
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldOp(u16);

/// The operator catalog: one associated constant per operator, in code order.
///
/// The constants keep the operator's proper-noun spelling (`FieldOp::Point`,
/// not `FieldOp::POINT`) rather than the usual constant casing. That spelling
/// **is** the operator's name — it is what [`FieldOp`]'s `Debug` prints, what one
/// line of `FieldGraph::explain` reads, and what every consumer of this layer
/// already writes. Renaming the whole vocabulary to satisfy a casing convention
/// would churn seven crates and make the constants disagree with the language
/// they name.
#[allow(non_upper_case_globals)]
impl FieldOp {
    /// A literal value. Params: `[type, x, y, z, w]`.
    pub const Const: FieldOp = FieldOp(0);
    /// The evaluation context's `point`, a `Vec3`.
    pub const Point: FieldOp = FieldOp(1);
    /// The evaluation context's `uv`, a `Vec2`.
    pub const Uv: FieldOp = FieldOp(2);
    /// The evaluation context's `normal`, a `Vec3`.
    pub const Normal: FieldOp = FieldOp(3);
    /// The evaluation context's `time`, a `Scalar`.
    pub const Time: FieldOp = FieldOp(4);
    /// The value of one parameter-table slot. Params: `[slot, type]`.
    pub const Param: FieldOp = FieldOp(5);

    /// Component-wise sum of two inputs.
    pub const Add: FieldOp = FieldOp(6);
    /// Component-wise difference of two inputs.
    pub const Sub: FieldOp = FieldOp(7);
    /// Component-wise product of two inputs.
    pub const Mul: FieldOp = FieldOp(8);
    /// Component-wise minimum of two inputs.
    pub const Min: FieldOp = FieldOp(9);
    /// Component-wise maximum of two inputs.
    pub const Max: FieldOp = FieldOp(10);
    /// Component-wise absolute value of one input.
    pub const Abs: FieldOp = FieldOp(11);

    /// `clamp(value, lo, hi)` over three inputs.
    pub const Clamp: FieldOp = FieldOp(12);
    /// `mix(a, b, t)` over three inputs — the language's **only** selection.
    pub const Mix: FieldOp = FieldOp(13);
    /// `smoothstep(edge0, edge1, x)` over three inputs.
    pub const Smoothstep: FieldOp = FieldOp(14);

    /// Dot product of two inputs.
    pub const Dot: FieldOp = FieldOp(15);
    /// Euclidean length of one input.
    pub const Length: FieldOp = FieldOp(16);
    /// One input rescaled to unit length.
    pub const Normalize: FieldOp = FieldOp(17);
    /// Build a vector from scalar inputs. Params: `[width]`; the input count is
    /// the declared width.
    pub const Compose: FieldOp = FieldOp(18);
    /// Extract one lane of one input. Params: `[lane]`.
    pub const Component: FieldOp = FieldOp(19);

    /// Single-octave coherent noise at one input point. Params: the two halves
    /// of the `u64` seed.
    pub const Noise: FieldOp = FieldOp(20);
    /// Fractal Brownian motion at one input point. Params: the seed halves
    /// followed by the knobs of an [`axiom_noise::FbmConfig`].
    pub const Fbm: FieldOp = FieldOp(21);
    /// One input point through a matrix. Params: one parameter slot per column
    /// of the matrix.
    pub const Transform: FieldOp = FieldOp(22);

    /// Component-wise `f32::sin` of one input — the **transcendental tier**,
    /// which carries its own measured CPU↔GPU parity tolerance rather than the
    /// rest of the algebra's shared one. See `ARCHITECTURE.md`.
    pub const Sin: FieldOp = FieldOp(23);
    /// Component-wise `f32::cos` of one input. Transcendental tier.
    pub const Cos: FieldOp = FieldOp(24);
    /// Component-wise power of two inputs; a base at or below zero yields `0.0`.
    /// Transcendental tier.
    pub const Pow: FieldOp = FieldOp(25);
    /// Component-wise `f32::exp` of one input. Transcendental tier.
    pub const Exp: FieldOp = FieldOp(26);
}

impl FieldOp {
    /// Every operator, in code order. The array **is** the decode table and its
    /// index **is** the operator code.
    pub const ALL: [FieldOp; FIELD_OP_COUNT] = [
        FieldOp::Const,
        FieldOp::Point,
        FieldOp::Uv,
        FieldOp::Normal,
        FieldOp::Time,
        FieldOp::Param,
        FieldOp::Add,
        FieldOp::Sub,
        FieldOp::Mul,
        FieldOp::Min,
        FieldOp::Max,
        FieldOp::Abs,
        FieldOp::Clamp,
        FieldOp::Mix,
        FieldOp::Smoothstep,
        FieldOp::Dot,
        FieldOp::Length,
        FieldOp::Normalize,
        FieldOp::Compose,
        FieldOp::Component,
        FieldOp::Noise,
        FieldOp::Fbm,
        FieldOp::Transform,
        FieldOp::Sin,
        FieldOp::Cos,
        FieldOp::Pow,
        FieldOp::Exp,
    ];

    /// The operator code — the wire value, which is also the table index.
    pub const fn code(self) -> u16 {
        self.0
    }

    /// The operator a code names, or `None` if the code names no operator.
    pub fn from_code(code: u16) -> Option<FieldOp> {
        FieldOp::ALL.get(code as usize).copied()
    }
}

/// One name per operator, in code order — the operator's proper noun, and the
/// only spelling of it. It is what `Debug` prints and therefore what one line of
/// `FieldGraph::explain` reads, so it is a **stable, human-facing** label even
/// though it is not a wire format.
#[rustfmt::skip]
const NAMES: [&str; FIELD_OP_COUNT] = [
    "Const",
    "Point", "Uv", "Normal", "Time",
    "Param",
    "Add", "Sub", "Mul", "Min", "Max",
    "Abs",
    "Clamp", "Mix", "Smoothstep",
    "Dot", "Length", "Normalize",
    "Compose", "Component",
    "Noise", "Fbm", "Transform",
    "Sin", "Cos", "Pow", "Exp",
];

impl fmt::Debug for FieldOp {
    /// The operator's name — `Mul`, not `FieldOp(8)`.
    ///
    /// Written by hand because the derived `Debug` for a newtype prints the
    /// wire code, and this type's `Debug` is read by humans in explanations and
    /// failure messages. The index is total: the private field can only hold a
    /// code the catalog defines.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(NAMES[self.0 as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_their_dispatch_indices() {
        FieldOp::ALL
            .iter()
            .enumerate()
            .for_each(|(index, op)| assert_eq!(op.code() as usize, index));
    }

    /// **The wire-compatibility pin.** Codes 0..=22 predate the conversion of
    /// `FieldOp` from an enum to a `u16` newtype, and every graph, golden and
    /// digest ever serialized carries them. They are frozen: a change here
    /// silently reinterprets committed bytes.
    #[rustfmt::skip]
    #[test]
    fn the_published_operator_codes_are_frozen() {
        let published = [
            (FieldOp::Const, 0), (FieldOp::Point, 1), (FieldOp::Uv, 2),
            (FieldOp::Normal, 3), (FieldOp::Time, 4), (FieldOp::Param, 5),
            (FieldOp::Add, 6), (FieldOp::Sub, 7), (FieldOp::Mul, 8),
            (FieldOp::Min, 9), (FieldOp::Max, 10), (FieldOp::Abs, 11),
            (FieldOp::Clamp, 12), (FieldOp::Mix, 13), (FieldOp::Smoothstep, 14),
            (FieldOp::Dot, 15), (FieldOp::Length, 16), (FieldOp::Normalize, 17),
            (FieldOp::Compose, 18), (FieldOp::Component, 19), (FieldOp::Noise, 20),
            (FieldOp::Fbm, 21), (FieldOp::Transform, 22),
            // Appended by the transcendental tier, never inserted.
            (FieldOp::Sin, 23), (FieldOp::Cos, 24), (FieldOp::Pow, 25),
            (FieldOp::Exp, 26),
        ];
        assert_eq!(published.len(), FIELD_OP_COUNT);
        published.iter().for_each(|(op, code)| {
            assert_eq!(op.code(), *code);
            assert_eq!(FieldOp::from_code(*code), Some(*op));
        });
    }

    #[test]
    fn the_algebra_is_closed_at_twenty_seven() {
        assert_eq!(FIELD_OP_COUNT, 27);
        assert_eq!(FieldOp::ALL.len(), FIELD_OP_COUNT);
        assert_eq!(NAMES.len(), FIELD_OP_COUNT);
        let distinct: std::collections::BTreeSet<FieldOp> = FieldOp::ALL.iter().copied().collect();
        assert_eq!(distinct.len(), FIELD_OP_COUNT);
        let named: std::collections::BTreeSet<&str> = NAMES.iter().copied().collect();
        assert_eq!(named.len(), FIELD_OP_COUNT, "two operators share a name");
    }

    #[test]
    fn a_known_code_decodes_and_an_unknown_one_does_not() {
        assert_eq!(FieldOp::from_code(0), Some(FieldOp::Const));
        assert_eq!(FieldOp::from_code(22), Some(FieldOp::Transform));
        assert_eq!(FieldOp::from_code(26), Some(FieldOp::Exp));
        assert_eq!(FieldOp::from_code(27), None);
        assert_eq!(FieldOp::from_code(u16::MAX), None);
    }

    #[test]
    fn an_operator_debugs_as_its_name_not_as_its_code() {
        assert_eq!(format!("{:?}", FieldOp::Mul), "Mul");
        assert_eq!(format!("{:?}", FieldOp::Sin), "Sin");
        FieldOp::ALL.iter().enumerate().for_each(|(index, op)| {
            assert_eq!(format!("{op:?}"), NAMES[index]);
        });
    }
}
