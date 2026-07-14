//! The transform texture operators (1..2 inputs): Blur, Blend, ColorRamp,
//! HeightToNormal.

use axiom_proc_core::NodeEval;

use crate::color_math::{lerp_rgba, luminance, rgba};
use crate::texture_buffer::TextureBuffer;

/// The largest blur radius an operator will honor, bounding its cost.
pub(crate) const MAX_BLUR: u32 = 8;

/// **Blur** — a box blur of `radius` pixels (clamped to [`MAX_BLUR`]) over one
/// input. Params: `[radius]`.
pub(crate) fn blur(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let radius = ctx.params().first().map(|p| p.as_int().min(MAX_BLUR));
    ctx.inputs().first().zip(radius).map(|(src, r)| {
        TextureBuffer::from_fn(src.width(), src.height(), |x, y| {
            box_blur_pixel(src, x, y, r)
        })
    })
}

/// The average of the `(2r+1)²` clamped neighbors of `(x, y)`.
fn box_blur_pixel(src: &TextureBuffer, x: u32, y: u32, r: u32) -> [u8; 4] {
    let side = 2 * r + 1;
    let count = side * side;
    let sum = (0..count).fold([0_u32; 4], |mut acc, k| {
        let dx = (k % side) as i64 - i64::from(r);
        let dy = (k / side) as i64 - i64::from(r);
        let sx = (i64::from(x) + dx).clamp(0, i64::from(src.width()) - 1) as u32;
        let sy = (i64::from(y) + dy).clamp(0, i64::from(src.height()) - 1) as u32;
        let px = src.texel(sx, sy);
        acc[0] += u32::from(px[0]);
        acc[1] += u32::from(px[1]);
        acc[2] += u32::from(px[2]);
        acc[3] += u32::from(px[3]);
        acc
    });
    [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
        (sum[3] / count) as u8,
    ]
}

/// **Blend** — per-pixel mix of two equally-sized inputs by `factor` (0 = input
/// A, 1 = input B). Params: `[factor]`. Fails if the inputs differ in size.
pub(crate) fn blend(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let a = ctx.inputs().first();
    let b = ctx.inputs().get(1);
    let factor = ctx.params().first().map(|p| p.as_scalar().get());
    a.zip(b)
        .zip(factor)
        .filter(|((a, b), _)| (a.width() == b.width()) & (a.height() == b.height()))
        .map(|((a, b), f)| {
            TextureBuffer::from_fn(a.width(), a.height(), |x, y| {
                lerp_rgba(a.texel(x, y), b.texel(x, y), f)
            })
        })
}

/// **ColorRamp** — remap one input's luminance across `color_lo`..`color_hi`.
/// Params: `[color_lo, color_hi]`.
pub(crate) fn color_ramp(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let p = ctx.params();
    let colors = (p.len() >= 2).then(|| (rgba(p[0].as_color()), rgba(p[1].as_color())));
    ctx.inputs().first().zip(colors).map(|(src, (lo, hi))| {
        TextureBuffer::from_fn(src.width(), src.height(), |x, y| {
            lerp_rgba(lo, hi, luminance(src.texel(x, y)))
        })
    })
}

/// **HeightToNormal** — derive a tangent-space normal map from one input's
/// luminance (central differences × `strength`). Params: `[strength]`.
pub(crate) fn height_to_normal(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let strength = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(strength).map(|(src, s)| {
        TextureBuffer::from_fn(src.width(), src.height(), |x, y| normal_pixel(src, x, y, s))
    })
}

/// The encoded normal at `(x, y)` from the height field's local slope.
fn normal_pixel(src: &TextureBuffer, x: u32, y: u32, strength: f32) -> [u8; 4] {
    let hl = luminance(src.texel(x.saturating_sub(1), y));
    let hr = luminance(src.texel(x + 1, y));
    let hu = luminance(src.texel(x, y.saturating_sub(1)));
    let hd = luminance(src.texel(x, y + 1));
    let dx = (hr - hl) * strength;
    let dy = (hd - hu) * strength;
    let len = (dx * dx + dy * dy + 1.0).sqrt();
    [encode(-dx / len), encode(-dy / len), encode(1.0 / len), 255]
}

/// Encode a normal component in `[-1, 1]` to a `[0, 255]` byte.
fn encode(n: f32) -> u8 {
    ((n * 0.5 + 0.5) * 255.0).clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use crate::dispatch::texture_eval;
    use crate::texture_buffer::TextureBuffer;
    use crate::texture_op::TextureOp;
    use axiom_proc_core::ProcCore;
    use axiom_recipe::{Color, Param, RecipeGraph, RecipeId, Scalar};
    use axiom_space::SpaceApi;

    fn c(packed: u32) -> Param {
        Param::color(Color::from_packed(packed))
    }

    /// A recipe: a `2x2` source (op `src_op`, `src_params`) feeding one filter
    /// (op `filter_op`, `filter_params`) with `input_count` links to the source.
    fn run(
        src_op: TextureOp,
        src_params: Vec<Param>,
        filter_op: TextureOp,
        filter_params: Vec<Param>,
        input_count: usize,
    ) -> Option<TextureBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let s = g.add(src_op as u16, src_params, vec![]);
        let inputs = (0..input_count).map(|_| s).collect();
        g.add(filter_op as u16, filter_params, inputs);
        ProcCore::new()
            .execute(&g, 7, &SpaceApi::root(), texture_eval)
            .ok()
    }

    fn checker() -> (TextureOp, Vec<Param>) {
        // A horizontal black→white gradient makes a good filter input.
        (
            TextureOp::Gradient,
            vec![
                Param::int(4),
                Param::int(4),
                c(0x00_00_00_FF),
                c(0xFF_FF_FF_FF),
            ],
        )
    }

    #[test]
    fn blur_averages_and_needs_an_input_and_radius() {
        let (op, p) = checker();
        assert!(run(op, p.clone(), TextureOp::Blur, vec![Param::int(1)], 1).is_some());
        // Missing the input link → op fails.
        assert!(run(op, p.clone(), TextureOp::Blur, vec![Param::int(1)], 0).is_none());
        // Missing the radius param → op fails.
        assert!(run(op, p, TextureOp::Blur, vec![], 1).is_none());
    }

    #[test]
    fn blend_mixes_two_equal_inputs() {
        // factor 1.0 → the second input verbatim (both are the same source here).
        let (op, p) = checker();
        let out = run(
            op,
            p.clone(),
            TextureOp::Blend,
            vec![Param::scalar(Scalar::new(1.0))],
            2,
        )
        .unwrap();
        assert_eq!(out.width(), 4);
        // One input is not enough for a blend.
        assert!(run(
            op,
            p,
            TextureOp::Blend,
            vec![Param::scalar(Scalar::new(0.5))],
            1
        )
        .is_none());
    }

    #[test]
    fn blend_rejects_mismatched_sizes() {
        // A 4x4 gradient blended with a 2x2 solid: different sizes → op fails.
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let a = g.add(
            TextureOp::Gradient as u16,
            vec![Param::int(4), Param::int(4), c(0), c(0xFFFFFFFF)],
            vec![],
        );
        let b = g.add(
            TextureOp::Solid as u16,
            vec![Param::int(2), Param::int(2), c(0xFFFFFFFF)],
            vec![],
        );
        g.add(
            TextureOp::Blend as u16,
            vec![Param::scalar(Scalar::new(0.5))],
            vec![a, b],
        );
        assert!(ProcCore::new()
            .execute(&g, 7, &SpaceApi::root(), texture_eval)
            .is_err());
    }

    #[test]
    fn color_ramp_remaps_luminance() {
        let (op, p) = checker();
        // Ramp black..red: the dark end stays black, the light end goes red.
        let out = run(
            op,
            p.clone(),
            TextureOp::ColorRamp,
            vec![c(0x00_00_00_FF), c(0xFF_00_00_FF)],
            1,
        )
        .unwrap();
        assert_eq!(out.texel(0, 0), [0, 0, 0, 255]);
        assert_eq!(out.texel(3, 0)[0], 255);
        assert!(run(op, p, TextureOp::ColorRamp, vec![c(0)], 1).is_none()); // needs 2 colors
    }

    #[test]
    fn height_to_normal_flat_input_points_up() {
        // A flat (solid) height field has zero slope → normal ≈ (0,0,1) ≈ blue up.
        let out = run(
            TextureOp::Solid,
            vec![Param::int(4), Param::int(4), c(0x80_80_80_FF)],
            TextureOp::HeightToNormal,
            vec![Param::scalar(Scalar::new(1.0))],
            1,
        )
        .unwrap();
        let n = out.texel(2, 2);
        assert_eq!((n[0], n[1]), (127, 127)); // flat → centered x/y
        assert!(n[2] >= 250); // strong +Z
        assert!(run(
            TextureOp::Solid,
            vec![Param::int(4), Param::int(4), c(0)],
            TextureOp::HeightToNormal,
            vec![],
            1
        )
        .is_none());
    }
}
