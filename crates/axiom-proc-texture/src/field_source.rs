//! The **Field** source operator: bake an [`axiom_field::FieldGraph`] to RGBA8.
//!
//! The eleven operators beside this one are *fixed* generators — a recipe can ask
//! for bricks, or for a checker, and nothing else. This operator is the one whose
//! shape a recipe author chooses: the pattern is an expression, carried as a
//! value, and a new visual effect is a new graph rather than a new Rust function.
//!
//! ## Why the graph is a table index and not inlined parameter words
//!
//! A [`axiom_recipe::Param`] is one `u32` word and `axiom_recipe::MAX_NODES` is
//! 256, so packing a graph's canonical bytes into a node's parameter list would
//! spend the whole node budget on a single operator and make the recipe
//! unreadable. Instead the graph travels **beside** the recipe, in the field table
//! [`crate::ProcTextureApi::bake_with_fields`] takes, and the node carries only
//! the index that names it. A recipe stays a small list of words; the expressions
//! it points at stay hashable, diffable, canonically serializable values of their
//! own.
//!
//! ## The sampling convention
//!
//! One evaluation per texel, at the texel **centre** — `uv = ((x + 0.5) / width,
//! (y + 0.5) / height)` — which is the convention the existing generators already
//! sample on, and `point = (uv.x, uv.y, 0)` so a graph written against `Point`
//! and a graph written against `Uv` see the same place. The normal is `+Y` and
//! time is zero: a baked texture has no surface to take a normal from and is not
//! animated.
//!
//! ## Output types
//!
//! A `Vec4` field is linear RGBA and writes all four channels; a `Scalar` field
//! is a mask or a height and writes greyscale at full alpha. A `Vec2` or `Vec3`
//! field names no pixel — there is no defensible rule for which channels two or
//! three lanes mean — so the node fails, which the executor reports as
//! `ProcError::OpFailed`.

use axiom_field::{EvalContext, FieldGraph, FieldValue};
use axiom_kernel::Seconds;
use axiom_math::{Vec2, Vec3};
use axiom_proc_core::NodeEval;

use crate::texture_buffer::{TextureBuffer, MAX_DIM};

/// One channel of a field value as a byte: clamped into `[0, 1]` and rounded to
/// the nearest of 256 levels, the same rounding [`crate::color_math::lerp_u8`]
/// applies so a field-baked ramp and a `Gradient`-baked one agree.
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The pixel one field value names, or `None` when its type names no pixel.
///
/// A table indexed by the value's type code, so the mapping is a lookup and the
/// unmapped widths are a `None` entry rather than a branch.
fn pixel(value: FieldValue) -> Option<[u8; 4]> {
    let lanes = value.as_vec4();
    let grey = channel(lanes.x);
    [
        Some([grey, grey, grey, u8::MAX]),
        None,
        None,
        Some([
            channel(lanes.x),
            channel(lanes.y),
            channel(lanes.z),
            channel(lanes.w),
        ]),
    ]
    .get(usize::from(value.ty().code()))
    .copied()
    .flatten()
}

/// Every texel of `graph` at `width x height`, or `None` when any texel fails to
/// evaluate or the field's type names no pixel.
///
/// The whole buffer is computed before any of it is published, so a failing graph
/// fails the node rather than producing a half-baked texture.
fn texels(graph: &FieldGraph, width: u32, height: u32) -> Option<Vec<[u8; 4]>> {
    (0..width * height)
        .map(|index| {
            let uv = Vec2::new(
                ((index % width) as f32 + 0.5) / width as f32,
                ((index / width) as f32 + 0.5) / height as f32,
            );
            graph
                .evaluate(&EvalContext::new(
                    Vec3::new(uv.x, uv.y, 0.0),
                    uv,
                    Vec3::UNIT_Y,
                    Seconds::finite_or_zero(0.0),
                ))
                .ok()
                .and_then(pixel)
        })
        .collect()
}

/// **Field** — bake the field named by `field_index` into an RGBA8 buffer.
/// Params: `[width, height, field_index]`.
pub(crate) fn field(
    ctx: NodeEval<'_, TextureBuffer>,
    fields: &[FieldGraph],
) -> Option<TextureBuffer> {
    let p = ctx.params();
    p.first()
        .zip(p.get(1))
        .zip(
            p.get(2)
                .map(|slot| slot.as_int() as usize)
                .and_then(|index| fields.get(index)),
        )
        .and_then(|((w, h), graph)| {
            let width = w.as_int().clamp(1, MAX_DIM);
            let height = h.as_int().clamp(1, MAX_DIM);
            texels(graph, width, height).map(|pixels| {
                TextureBuffer::from_fn(width, height, move |x, y| pixels[(y * width + x) as usize])
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc_texture_api::ProcTextureApi;
    use crate::texture_op::TextureOp;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldType};
    use axiom_noise::{FbmConfig, Frequency};
    use axiom_recipe::{Color, Param, RecipeGraph, RecipeId, Scalar};

    /// A recipe holding one `Field` node of the given size at the given table
    /// index.
    fn recipe(width: u32, height: u32, index: u32) -> RecipeGraph {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(
            TextureOp::Field as u16,
            vec![Param::int(width), Param::int(height), Param::int(index)],
            vec![],
        );
        g
    }

    /// `uv.x` — a horizontal ramp, as a `Scalar` field.
    fn ramp() -> FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("proc-texture/test/ramp"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, x) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        builder.build(x)
    }

    /// A `Vec4` field: `(uv.x, uv.y, fbm(point) remapped to 0..1, 1)`.
    fn gradient_times_fbm() -> FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("proc-texture/test/grad-fbm"), 1)
            .push(FieldOp::Uv, Vec::new(), Vec::new());
        let (builder, u) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let (builder, v) = builder.push(FieldOp::Component, vec![Param::int(1)], vec![uv]);
        let (builder, point) = builder.push(FieldOp::Point, Vec::new(), Vec::new());
        let (builder, fractal) = builder.push_fbm(
            0x5EED,
            FbmConfig::new(3, Frequency::finite_or_zero(4.0)),
            point,
        );
        let (builder, half) = builder.push_const(FieldValue::scalar(Scalar::new(0.5)));
        let (builder, scaled) = builder.push(FieldOp::Mul, Vec::new(), vec![fractal, half]);
        let (builder, shifted) = builder.push(FieldOp::Add, Vec::new(), vec![scaled, half]);
        let (builder, one) = builder.push_const(FieldValue::scalar(Scalar::new(1.0)));
        let (builder, rgba) = builder.push(
            FieldOp::Compose,
            vec![Param::int(4)],
            vec![u, v, shifted, one],
        );
        builder.build(rgba)
    }

    /// A field whose declared output names a node it does not contain.
    fn dangling() -> FieldGraph {
        let (_, node) = FieldBuilder::new(FieldId::of_name("proc-texture/test/other"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        FieldBuilder::new(FieldId::of_name("proc-texture/test/dangling"), 1).build(node)
    }

    #[test]
    fn a_scalar_field_bakes_greyscale_at_texel_centres() {
        let ramp = ramp();
        assert_eq!(ramp.validate(), Ok(()));
        let baked = ProcTextureApi::new()
            .bake_with_fields(&recipe(4, 2, 0), 0, &[ramp])
            .unwrap();
        assert_eq!((baked.width(), baked.height()), (4, 2));
        // uv.x at the four texel centres is 0.125, 0.375, 0.625, 0.875.
        assert_eq!(baked.texel(0, 0), [32, 32, 32, 255]);
        assert_eq!(baked.texel(1, 0), [96, 96, 96, 255]);
        assert_eq!(baked.texel(2, 0), [159, 159, 159, 255]);
        assert_eq!(baked.texel(3, 0), [223, 223, 223, 255]);
        // The ramp is horizontal: the second row repeats the first.
        assert_eq!(baked.texel(2, 1), baked.texel(2, 0));
    }

    #[test]
    fn a_vec4_field_bakes_rgba_and_is_byte_identical_across_runs() {
        let api = ProcTextureApi::new();
        let fields = [gradient_times_fbm()];
        assert_eq!(fields[0].validate(), Ok(()));
        let once = api.bake_with_fields(&recipe(8, 8, 0), 0, &fields).unwrap();
        let twice = api.bake_with_fields(&recipe(8, 8, 0), 9, &fields).unwrap();
        // The seed is the recipe's entropy seed; a field carries its own, so the
        // bake does not move with it.
        assert_eq!(once, twice);
        // The committed golden: red and green are the uv ramps, blue is the
        // remapped fbm, alpha is opaque.
        assert_eq!(
            [
                once.texel(0, 0),
                once.texel(7, 0),
                once.texel(0, 7),
                once.texel(7, 7)
            ],
            [
                [16, 16, 136, 255],
                [239, 16, 159, 255],
                [16, 239, 103, 255],
                [239, 239, 142, 255],
            ]
        );
    }

    #[test]
    fn a_field_whose_type_names_no_pixel_fails_the_node() {
        // `Uv` is a Vec2 and `Point` is a Vec3: neither names a pixel.
        let two = FieldBuilder::new(FieldId::of_name("proc-texture/test/uv"), 1)
            .push(FieldOp::Uv, Vec::new(), Vec::new());
        let three = FieldBuilder::new(FieldId::of_name("proc-texture/test/point"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let fields = [two.0.build(two.1), three.0.build(three.1)];
        let api = ProcTextureApi::new();
        assert!(api.bake_with_fields(&recipe(2, 2, 0), 0, &fields).is_err());
        assert!(api.bake_with_fields(&recipe(2, 2, 1), 0, &fields).is_err());
    }

    #[test]
    fn a_field_that_does_not_evaluate_fails_the_node() {
        assert!(ProcTextureApi::new()
            .bake_with_fields(&recipe(2, 2, 0), 0, &[dangling()])
            .is_err());
    }

    #[test]
    fn a_missing_field_or_a_short_parameter_list_fails_the_node() {
        let api = ProcTextureApi::new();
        // Index 3 names nothing in a one-entry table.
        assert!(api.bake_with_fields(&recipe(2, 2, 3), 0, &[ramp()]).is_err());
        // No field table at all.
        assert!(api.bake(&recipe(2, 2, 0), 0).is_err());
        // Fewer than three words.
        let mut short = RecipeGraph::new(RecipeId::from_raw(1), 1);
        short.add(
            TextureOp::Field as u16,
            vec![Param::int(2), Param::int(2)],
            vec![],
        );
        assert!(api.bake_with_fields(&short, 0, &[ramp()]).is_err());
    }

    #[test]
    fn dimensions_are_clamped_like_every_other_source() {
        let baked = ProcTextureApi::new()
            .bake_with_fields(&recipe(0, 9999, 0), 0, &[ramp()])
            .unwrap();
        assert_eq!((baked.width(), baked.height()), (1, MAX_DIM));
    }

    #[test]
    fn a_field_texture_composes_with_the_fixed_operators() {
        // A Field source feeding a Blur: the expression and the eleven fixed
        // operators are one vocabulary, not two.
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let source = g.add(
            TextureOp::Field as u16,
            vec![Param::int(8), Param::int(8), Param::int(0)],
            vec![],
        );
        g.add(TextureOp::Blur as u16, vec![Param::int(1)], vec![source]);
        let blurred = ProcTextureApi::new()
            .bake_with_fields(&g, 0, &[ramp()])
            .unwrap();
        assert_eq!((blurred.width(), blurred.height()), (8, 8));
        // Blurring a monotonic ramp keeps it monotonic but softens the ends.
        assert!(blurred.texel(0, 0)[0] < blurred.texel(4, 0)[0]);
        assert!(blurred.texel(4, 0)[0] < blurred.texel(7, 0)[0]);
    }

    #[test]
    fn a_parameter_slot_the_declared_type_covers_is_read_from_the_table() {
        // A `Param` node reads the field's own parameter table, which is what
        // makes a baked texture retunable without re-authoring its graph.
        let (builder, slot) = FieldBuilder::new(FieldId::of_name("proc-texture/test/tint"), 1)
            .declare("tint", FieldValue::scalar(Scalar::new(0.25)));
        let (builder, node) = builder.push_param(slot, FieldType::Scalar);
        let baked = ProcTextureApi::new()
            .bake_with_fields(&recipe(2, 2, 0), 0, &[builder.build(node)])
            .unwrap();
        assert_eq!(baked.texel(0, 0), [64, 64, 64, 255]);
        assert_eq!(baked.texel(1, 1), [64, 64, 64, 255]);
    }

    #[test]
    fn an_empty_field_table_still_lets_the_fixed_operators_bake() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(
            TextureOp::Solid as u16,
            vec![
                Param::int(2),
                Param::int(2),
                Param::color(Color::rgba(1, 2, 3, 4)),
            ],
            vec![],
        );
        let api = ProcTextureApi::new();
        assert_eq!(
            api.bake_with_fields(&g, 0, &[]).unwrap(),
            api.bake(&g, 0).unwrap()
        );
    }
}
