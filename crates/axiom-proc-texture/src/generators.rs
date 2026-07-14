//! The source texture operators (no inputs): Solid, Gradient, Noise, Bricks,
//! Checker, Spots.

use axiom_math::Vec3;
use axiom_noise::value_noise;
use axiom_proc_core::NodeEval;

use crate::color_math::{lerp_rgba, rgba};
use crate::texture_buffer::{TextureBuffer, MAX_DIM};

/// **Solid** — fill the whole texture with one color. Params: `[width, height,
/// color]`.
pub(crate) fn solid(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let p = ctx.params();
    (p.len() >= 3).then(|| {
        let color = rgba(p[2].as_color());
        TextureBuffer::from_fn(p[0].as_int(), p[1].as_int(), move |_, _| color)
    })
}

/// **Gradient** — a horizontal ramp from `color_a` (left) to `color_b` (right).
/// Params: `[width, height, color_a, color_b]`.
pub(crate) fn gradient(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let p = ctx.params();
    (p.len() >= 4).then(|| {
        let a = rgba(p[2].as_color());
        let b = rgba(p[3].as_color());
        let cw = p[0].as_int().clamp(1, MAX_DIM);
        TextureBuffer::from_fn(cw, p[1].as_int(), move |x, _| {
            let denom = (cw.max(2) - 1) as f32;
            lerp_rgba(a, b, x as f32 / denom)
        })
    })
}

/// **Noise** — value noise remapped between `color_lo` and `color_hi`, at
/// `scale` cells across the texture. The noise seed is drawn from the node's
/// deterministic entropy stream. Params: `[width, height, scale, color_lo,
/// color_hi]`.
pub(crate) fn noise(mut ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    // Draw the seed first so the mutable stream borrow ends before the parameter
    // words are read (a purely deterministic draw, keyed by the node's address).
    let seed = ctx.stream().next_u64();
    let p = ctx.params();
    (p.len() >= 5).then(|| {
        let scale = p[2].as_int().max(1) as f32;
        let lo = rgba(p[3].as_color());
        let hi = rgba(p[4].as_color());
        let cw = p[0].as_int().clamp(1, MAX_DIM);
        let ch = p[1].as_int().clamp(1, MAX_DIM);
        TextureBuffer::from_fn(cw, ch, move |x, y| {
            let fx = x as f32 / cw as f32 * scale;
            let fy = y as f32 / ch as f32 * scale;
            let n = value_noise(seed, Vec3::new(fx, fy, 0.0)).get();
            lerp_rgba(lo, hi, n * 0.5 + 0.5)
        })
    })
}

/// **Bricks** — a staggered brick pattern: `rows`×`cols` bricks separated by a
/// `mortar`-pixel gap. Params: `[width, height, rows, cols, mortar, brick_color,
/// mortar_color]`.
pub(crate) fn bricks(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let p = ctx.params();
    (p.len() >= 7).then(|| {
        let rows = p[2].as_int().max(1);
        let cols = p[3].as_int().max(1);
        let mortar = p[4].as_int();
        let brick = rgba(p[5].as_color());
        let mort = rgba(p[6].as_color());
        let cw = p[0].as_int().clamp(1, MAX_DIM);
        let ch = p[1].as_int().clamp(1, MAX_DIM);
        TextureBuffer::from_fn(cw, ch, move |x, y| {
            let brick_h = (ch / rows).max(1);
            let brick_w = (cw / cols).max(1);
            let row = y / brick_h;
            let offset = (row & 1) * (brick_w / 2);
            let sx = (x + offset) % brick_w;
            let sy = y % brick_h;
            let in_mortar = (sy < mortar) | (sx < mortar);
            [brick, mort][in_mortar as usize]
        })
    })
}

/// **Checker** — an alternating 2-color grid of `cell`-pixel squares, the classic
/// tile / calibration primitive. Params: `[width, height, cell, color_a,
/// color_b]`.
pub(crate) fn checker(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let p = ctx.params();
    (p.len() >= 5).then(|| {
        let cell = p[2].as_int().max(1);
        let a = rgba(p[3].as_color());
        let b = rgba(p[4].as_color());
        TextureBuffer::from_fn(p[0].as_int(), p[1].as_int(), move |x, y| {
            [a, b][((x / cell + y / cell) & 1) as usize]
        })
    })
}

/// **Spots** — a `base_color` fill stamped with filled circles. Params:
/// `[width, height, base_color, spot_color, count, cx0, cy0, r0, …]`: the 5-word
/// header is followed by `count` `(center_x, center_y, radius)` texel-space
/// triples. A texel inside any spot's radius is `spot_color`, else `base_color`.
/// The declared `count` is clamped to the triples actually present, so a short
/// parameter list can never read past the end. Unlike a `Checker`/`Bricks` grid
/// (many small cells that alias into speckle under downsampling), a handful of
/// large spots survives a mip cleanly — the primitive for painting the soccer
/// ball's dark pentagon rosette directly onto the sphere's UVs so the panels are
/// part of the surface and move with it.
pub(crate) fn spots(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let p = ctx.params();
    (p.len() >= 5).then(|| {
        let base = rgba(p[2].as_color());
        let spot = rgba(p[3].as_color());
        let avail = p.len().saturating_sub(5) / 3;
        let n = (p[4].as_int() as usize).min(avail);
        let cw = p[0].as_int().clamp(1, MAX_DIM);
        let ch = p[1].as_int().clamp(1, MAX_DIM);
        // Snapshot the (cx, cy, r) triples as i64 so the closure owns them (the
        // params borrow ends here) and the distance test can't underflow.
        let discs: Vec<(i64, i64, i64)> = (0..n)
            .map(|k| {
                (
                    p[5 + k * 3].as_int() as i64,
                    p[6 + k * 3].as_int() as i64,
                    p[7 + k * 3].as_int() as i64,
                )
            })
            .collect();
        TextureBuffer::from_fn(cw, ch, move |x, y| {
            let hit = discs.iter().any(|&(cx, cy, r)| {
                let (dx, dy) = (x as i64 - cx, y as i64 - cy);
                dx * dx + dy * dy <= r * r
            });
            [base, spot][hit as usize]
        })
    })
}

#[cfg(test)]
mod tests {
    use crate::dispatch::texture_eval;
    use crate::texture_buffer::TextureBuffer;
    use crate::texture_op::TextureOp;
    use axiom_proc_core::ProcCore;
    use axiom_recipe::{Color, Param, RecipeGraph, RecipeId};
    use axiom_space::SpaceApi;

    /// Evaluate a single source-op recipe through the real executor + evaluator.
    fn run(op: TextureOp, params: Vec<Param>) -> Option<TextureBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(op as u16, params, vec![]);
        ProcCore::new()
            .execute(&g, 7, &SpaceApi::root(), texture_eval)
            .ok()
    }

    fn c(packed: u32) -> Param {
        Param::color(Color::from_packed(packed))
    }

    #[test]
    fn solid_fills_one_color_and_needs_three_params() {
        let t = run(
            TextureOp::Solid,
            vec![Param::int(2), Param::int(2), c(0x11_22_33_44)],
        )
        .unwrap();
        assert_eq!(t.texel(0, 0), [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(t.texel(1, 1), [0x11, 0x22, 0x33, 0x44]);
        assert!(run(TextureOp::Solid, vec![Param::int(2)]).is_none());
    }

    #[test]
    fn checker_alternates_cells_and_needs_five_params() {
        let t = run(
            TextureOp::Checker,
            vec![
                Param::int(4),
                Param::int(4),
                Param::int(2),
                c(0x00_00_00_FF),
                c(0xFF_FF_FF_FF),
            ],
        )
        .unwrap();
        // Cell (0,0) is color_a; the neighbor cell (2,0) is color_b.
        assert_eq!(t.texel(0, 0), [0, 0, 0, 255]);
        assert_eq!(t.texel(2, 0), [255, 255, 255, 255]);
        assert_eq!(t.texel(2, 2), [0, 0, 0, 255]); // diagonal returns to color_a
        assert!(run(TextureOp::Checker, vec![Param::int(4)]).is_none());
    }

    #[test]
    fn gradient_ramps_left_to_right() {
        let t = run(
            TextureOp::Gradient,
            vec![
                Param::int(3),
                Param::int(1),
                c(0x00_00_00_FF),
                c(0xFF_FF_FF_FF),
            ],
        )
        .unwrap();
        assert_eq!(t.texel(0, 0), [0, 0, 0, 255]);
        assert_eq!(t.texel(2, 0), [255, 255, 255, 255]);
        assert!(run(TextureOp::Gradient, vec![Param::int(3)]).is_none());
        // width 1 must not divide by zero.
        assert!(run(
            TextureOp::Gradient,
            vec![Param::int(1), Param::int(1), c(0), c(0xFFFFFFFF)]
        )
        .is_some());
    }

    #[test]
    fn noise_is_deterministic_and_needs_five_params() {
        let params = vec![
            Param::int(8),
            Param::int(8),
            Param::int(4),
            c(0),
            c(0xFFFFFFFF),
        ];
        let a = run(TextureOp::Noise, params.clone()).unwrap();
        let b = run(TextureOp::Noise, params).unwrap();
        assert_eq!(a, b);
        assert!(run(TextureOp::Noise, vec![Param::int(8)]).is_none());
    }

    #[test]
    fn bricks_has_mortar_gaps_and_needs_seven_params() {
        let t = run(
            TextureOp::Bricks,
            vec![
                Param::int(8),
                Param::int(8),
                Param::int(2),
                Param::int(2),
                Param::int(1),
                c(0xAA_AA_AA_FF),
                c(0x22_22_22_FF),
            ],
        )
        .unwrap();
        assert_eq!(t.texel(0, 0), [0x22, 0x22, 0x22, 0xFF]);
        assert_eq!(t.texel(3, 3), [0xAA, 0xAA, 0xAA, 0xFF]);
        assert!(run(TextureOp::Bricks, vec![Param::int(8)]).is_none());
    }

    #[test]
    fn spots_paints_filled_discs_and_needs_five_params() {
        // One radius-1 spot centred at (2, 2) on a 6x6 white field.
        let t = run(
            TextureOp::Spots,
            vec![
                Param::int(6),
                Param::int(6),
                c(0xFF_FF_FF_FF),
                c(0x00_00_00_FF),
                Param::int(1),
                Param::int(2),
                Param::int(2),
                Param::int(1),
            ],
        )
        .unwrap();
        assert_eq!(t.texel(2, 2), [0, 0, 0, 255]); // spot centre
        assert_eq!(t.texel(3, 2), [0, 0, 0, 255]); // within radius 1
        assert_eq!(t.texel(0, 0), [255, 255, 255, 255]); // far corner is base
        assert_eq!(t.texel(5, 5), [255, 255, 255, 255]);
        // Fewer than five params fails the node.
        assert!(run(TextureOp::Spots, vec![Param::int(6), Param::int(6)]).is_none());
    }

    #[test]
    fn spots_clamps_declared_count_to_available_triples() {
        // Declares five spots but supplies only one triple: the extra count is
        // clamped away (no read past the end), so only the one spot paints.
        let one = run(
            TextureOp::Spots,
            vec![
                Param::int(4),
                Param::int(4),
                c(0xFF_FF_FF_FF),
                c(0x00_00_00_FF),
                Param::int(5),
                Param::int(0),
                Param::int(0),
                Param::int(0),
            ],
        )
        .unwrap();
        assert_eq!(one.texel(0, 0), [0, 0, 0, 255]);
        assert_eq!(one.texel(3, 3), [255, 255, 255, 255]);
        // A zero count leaves the field entirely base-colored.
        let none = run(
            TextureOp::Spots,
            vec![
                Param::int(4),
                Param::int(4),
                c(0xFF_FF_FF_FF),
                c(0x00_00_00_FF),
                Param::int(0),
            ],
        )
        .unwrap();
        assert_eq!(none.texel(0, 0), [255, 255, 255, 255]);
        assert_eq!(none.texel(2, 2), [255, 255, 255, 255]);
    }
}
