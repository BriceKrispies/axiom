//! The closed 23-operator algebra of the field language.

/// How many operators the algebra has. The `engine_no_large_enums` cap is 24, so
/// there is exactly **one** spare slot. Do not spend it casually: a 25th operator
/// means moving the discriminant to a bare `u16` code with a `const` catalog —
/// the `axiom-recipe` shape.
pub const FIELD_OP_COUNT: usize = 23;

/// The twenty-three field operators.
///
/// The discriminant **is** the operator code stored in a
/// [`axiom_recipe::Node`], and it indexes both [`FieldOp::ALL`] and the
/// `SIGNATURES` table, so this order is the dispatch order and must not be
/// reshuffled.
///
/// The algebra is **closed**: there is no registry, no runtime-extensible verb
/// and no dynamic dispatch. A new visual effect is a new *graph*, never a new
/// Rust function — that is the entire point of the layer.
///
/// **What is deliberately not an operator, and why:**
///
/// | Excluded | Reason |
/// |---|---|
/// | `Div` | Division by zero is a determinism hazard and a NaN source. Multiply by a constant reciprocal, or add a guarded op later with an explicit fallback value. |
/// | `Pow`/`Exp`/`Log`/`Sin`/`Cos` | Transcendentals differ between CPU and GPU `f32` by more than the parity tolerance, and nothing needs one yet. |
/// | `Step` | [`FieldOp::Smoothstep`] with equal edges. |
/// | `Cross` | Expressible with [`FieldOp::Compose`] plus arithmetic. |
/// | `dpdx`/`dpdy` | Screen-space derivatives are backend-specific, absent on the CPU, and the cause of a real past defect. Height-to-normal is finite differences at a caller-supplied offset. |
/// | `If`/`Select`/`Compare` | Selection is [`FieldOp::Mix`]. A comparison operator is the seed of control flow in a language that must stay branchless end to end. |
/// | `Texture`/`Sample` | A texture is a rendering resource; a field that samples an image is a later, separate decision. |
/// | marble/wood/rust/dirt/asphalt | Library graphs, not primitives. |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum FieldOp {
    /// A literal value. Params: `[type, x, y, z, w]`.
    Const = 0,
    /// The evaluation context's `point`, a `Vec3`.
    Point = 1,
    /// The evaluation context's `uv`, a `Vec2`.
    Uv = 2,
    /// The evaluation context's `normal`, a `Vec3`.
    Normal = 3,
    /// The evaluation context's `time`, a `Scalar`.
    Time = 4,
    /// The value of one parameter-table slot. Params: `[slot, type]`.
    Param = 5,

    /// Component-wise sum of two inputs.
    Add = 6,
    /// Component-wise difference of two inputs.
    Sub = 7,
    /// Component-wise product of two inputs.
    Mul = 8,
    /// Component-wise minimum of two inputs.
    Min = 9,
    /// Component-wise maximum of two inputs.
    Max = 10,
    /// Component-wise absolute value of one input.
    Abs = 11,

    /// `clamp(value, lo, hi)` over three inputs.
    Clamp = 12,
    /// `mix(a, b, t)` over three inputs — the language's **only** selection.
    Mix = 13,
    /// `smoothstep(edge0, edge1, x)` over three inputs.
    Smoothstep = 14,

    /// Dot product of two inputs.
    Dot = 15,
    /// Euclidean length of one input.
    Length = 16,
    /// One input rescaled to unit length.
    Normalize = 17,
    /// Build a vector from scalar inputs. Params: `[width]`; the input count is
    /// the declared width.
    Compose = 18,
    /// Extract one lane of one input. Params: `[lane]`.
    Component = 19,

    /// Single-octave coherent noise at one input point. Params: the two halves
    /// of the `u64` seed.
    Noise = 20,
    /// Fractal Brownian motion at one input point. Params: the seed halves
    /// followed by the knobs of an [`axiom_noise::FbmConfig`].
    Fbm = 21,
    /// One input point through a matrix. Params: one parameter slot per column
    /// of the matrix.
    Transform = 22,
}

impl FieldOp {
    /// Every operator, in discriminant order. The array **is** the decode table
    /// and its index **is** the operator code.
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
    ];

    /// The operator code — the discriminant, which is also the table index.
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// The operator a code names, or `None` if the code names no operator.
    pub fn from_code(code: u16) -> Option<FieldOp> {
        FieldOp::ALL.get(code as usize).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_their_dispatch_indices() {
        assert_eq!(FieldOp::Const as u16, 0);
        assert_eq!(FieldOp::Param as u16, 5);
        assert_eq!(FieldOp::Add as u16, 6);
        assert_eq!(FieldOp::Abs as u16, 11);
        assert_eq!(FieldOp::Clamp as u16, 12);
        assert_eq!(FieldOp::Smoothstep as u16, 14);
        assert_eq!(FieldOp::Dot as u16, 15);
        assert_eq!(FieldOp::Component as u16, 19);
        assert_eq!(FieldOp::Noise as u16, 20);
        assert_eq!(FieldOp::Transform as u16, 22);
        FieldOp::ALL
            .iter()
            .enumerate()
            .for_each(|(index, op)| assert_eq!(op.code() as usize, index));
    }

    #[test]
    fn the_algebra_is_closed_at_twenty_three() {
        assert_eq!(FIELD_OP_COUNT, 23);
        assert_eq!(FieldOp::ALL.len(), FIELD_OP_COUNT);
        let distinct: std::collections::BTreeSet<FieldOp> = FieldOp::ALL.iter().copied().collect();
        assert_eq!(distinct.len(), FIELD_OP_COUNT);
    }

    #[test]
    fn a_known_code_decodes_and_an_unknown_one_does_not() {
        assert_eq!(FieldOp::from_code(0), Some(FieldOp::Const));
        assert_eq!(FieldOp::from_code(22), Some(FieldOp::Transform));
        assert_eq!(FieldOp::from_code(23), None);
        assert_eq!(FieldOp::from_code(u16::MAX), None);
    }
}
