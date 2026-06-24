//! The deterministic solver: viewport facts + a flat tree → one rect per node.
//!
//! Single pass, top-down, **recursion-free and branchless**. Nodes are visited in
//! index order (the tree guarantees parent-before-child), and each node lays out
//! its direct children into the accumulating result — so a child's rect is set
//! before the child is itself visited, mirroring the scene layer's transform
//! propagation. Every selection (axis, justify, align, wrap, clamps) is arithmetic
//! or a discriminant-indexed table; every division is guarded against zero.

use axiom_host::{HostViewport, Orientation};

use crate::layout_rect::LayoutRect;
use crate::layout_style::LayoutStyle;
use crate::node_id::NodeId;
use crate::result::LayoutResult;
use crate::tree::{LayoutNode, LayoutTree};

/// Floor for divisors derived from weights/extents, so a zero grow-sum or a
/// degenerate extent can never divide by zero.
const TINY: f32 = 1.0e-6;

/// Solve `tree` against `viewport`: every node gets a placed [`LayoutRect`] in
/// logical pixels. The root fills the viewport inset by its safe area; children are
/// distributed by their styles. Returns an empty result for an empty tree.
pub fn solve(viewport: &HostViewport, tree: &LayoutTree) -> LayoutResult {
    let nodes = tree.nodes();
    let is_landscape = viewport.orientation() == Orientation::Landscape;
    // Seed the root rect (if any), then fill in every node's children in order.
    let seeded = nodes
        .first()
        .map(|root| {
            let mut result = LayoutResult::new();
            result.insert(root.id(), root_rect(viewport));
            result
        })
        .unwrap_or_default();
    (0..nodes.len()).fold(seeded, |result, i| place_children(result, nodes, i, is_landscape))
}

/// The root content rect: the viewport's logical box, inset by the safe area.
fn root_rect(viewport: &HostViewport) -> LayoutRect {
    let insets = viewport.safe_area_insets();
    let left = insets.left().get();
    let top = insets.top().get();
    let width = (viewport.logical_width() as f32 - left - insets.right().get()).max(0.0);
    let height = (viewport.logical_height() as f32 - top - insets.bottom().get()).max(0.0);
    LayoutRect::from_edges(left, top, width, height)
}

/// Lay out node `i`'s direct children into `result`. A node whose own rect is not
/// yet placed (only possible for a malformed tree) contributes nothing.
fn place_children(
    mut result: LayoutResult,
    nodes: &[LayoutNode],
    i: usize,
    is_landscape: bool,
) -> LayoutResult {
    let node = nodes[i];
    let children: Vec<(NodeId, LayoutStyle)> = nodes
        .iter()
        .filter(|child| child.parent() == Some(i))
        .map(|child| (child.id(), *child.style()))
        .collect();
    let placed = result
        .rect(node.id())
        .map(|rect| {
            flex_layout(
                inset_rect(rect, node.style()),
                node.style(),
                &children,
                is_landscape,
            )
        })
        .unwrap_or_default();
    placed
        .into_iter()
        .for_each(|(id, rect)| result.insert(id, rect));
    result
}

/// A node's content box (origin + extents) after subtracting its padding.
fn inset_rect(rect: LayoutRect, style: &LayoutStyle) -> (f32, f32, f32, f32) {
    let p = style.padding;
    let x = rect.left().get() + p.left.get();
    let y = rect.top().get() + p.top.get();
    let width = (rect.width().get() - p.left.get() - p.right.get()).max(0.0);
    let height = (rect.height().get() - p.top.get() - p.bottom.get()).max(0.0);
    (x, y, width, height)
}

/// A node's main-axis size clamped into `[min_main, max_main]`. `max_main` defaults
/// to unbounded; the upper bound is floored at the lower so the clamp is total.
fn clamp_main(value: f32, style: &LayoutStyle) -> f32 {
    let lo = style.min_main.get();
    let hi = style.max_main.map_or(f32::INFINITY, |m| m.get()).max(lo);
    value.max(lo).min(hi)
}

/// A child's starting main size: its basis clamped to its min/max.
fn child_base(style: &LayoutStyle) -> f32 {
    clamp_main(style.basis.get(), style)
}

/// Lay out a parent's children within its content box `(cx, cy, cw, ch)`.
fn flex_layout(
    content: (f32, f32, f32, f32),
    parent: &LayoutStyle,
    children: &[(NodeId, LayoutStyle)],
    is_landscape: bool,
) -> Vec<(NodeId, LayoutRect)> {
    let horizontal = parent.direction.main_is_horizontal(is_landscape);
    let (cx, cy, cw, ch) = content;
    let main_extent = [ch, cw][horizontal as usize];
    let cross_extent = [cw, ch][horizontal as usize];
    let main_origin = [cy, cx][horizontal as usize];
    let cross_origin = [cx, cy][horizontal as usize];
    let gap = parent.gap.get();

    let lines = assign_lines(children, parent.wrap.wraps(), main_extent, gap);
    let num_lines = lines.iter().copied().max().map_or(0, |m| m + 1);
    let line_cross = cross_extent / (num_lines.max(1) as f32);

    (0..num_lines)
        .flat_map(|line| {
            layout_line(
                line,
                &lines,
                children,
                (main_origin, cross_origin, main_extent, line_cross),
                gap,
                parent,
                horizontal,
            )
        })
        .collect()
}

/// Assign each child a line index. Without wrap, every child is on line 0. With
/// wrap, a child starts a new line when adding it (with its leading gap) would
/// overflow the main extent — unless it is the first on the current line.
fn assign_lines(
    children: &[(NodeId, LayoutStyle)],
    wraps: bool,
    main_extent: f32,
    gap: f32,
) -> Vec<usize> {
    children
        .iter()
        .scan((0.0_f32, 0_usize), |(used, line), (_, style)| {
            let base = child_base(style);
            let first_in_line = *used <= 0.0;
            let lead = ((!first_in_line) as u32 as f32) * gap;
            let prospective = *used + lead + base;
            let overflow = wraps & !first_in_line & (prospective > main_extent);
            *line += overflow as usize;
            *used = [prospective, base][overflow as usize];
            Some(*line)
        })
        .collect()
}

/// Place the members of one line. `geometry` is `(main_origin, cross_origin,
/// main_extent, line_cross)`. Distributes free main space by grow weight, then the
/// leftover by `justify`; sizes/positions on the cross axis by `align`; preserves
/// each child's aspect.
fn layout_line(
    line: usize,
    lines: &[usize],
    children: &[(NodeId, LayoutStyle)],
    geometry: (f32, f32, f32, f32),
    gap: f32,
    parent: &LayoutStyle,
    horizontal: bool,
) -> Vec<(NodeId, LayoutRect)> {
    let (main_origin, cross_origin, main_extent, line_cross) = geometry;
    let members: Vec<usize> = (0..children.len()).filter(|&j| lines[j] == line).collect();
    let count = members.len();
    let gaps_total = gap * (count.saturating_sub(1) as f32);

    let bases: Vec<f32> = members.iter().map(|&j| child_base(&children[j].1)).collect();
    let sum_grow: f32 = members
        .iter()
        .map(|&j| children[j].1.grow.get().max(0.0))
        .sum();
    let free = main_extent - bases.iter().sum::<f32>() - gaps_total;
    let mains: Vec<f32> = members
        .iter()
        .zip(bases.iter())
        .map(|(&j, &base)| {
            let style = &children[j].1;
            clamp_main(base + free * style.grow.get().max(0.0) / sum_grow.max(TINY), style)
        })
        .collect();

    let free_after = (main_extent - mains.iter().sum::<f32>() - gaps_total).max(0.0);
    let leading = parent.justify.leading_fraction() * free_after;
    let between = parent.justify.between_fraction() * free_after
        / (count.saturating_sub(1).max(1) as f32);
    let line_cross_origin = cross_origin + (line as f32) * line_cross;

    members
        .iter()
        .zip(mains.iter())
        .enumerate()
        .scan(main_origin + leading, |cursor, (k, (&j, &main_size))| {
            let main_pos = *cursor;
            let trailing = ((k + 1 < count) as u32 as f32) * (gap + between);
            *cursor = main_pos + main_size + trailing;
            let style = &children[j].1;
            let cross_size = [style.cross.resolve(line_cross), line_cross]
                [parent.align.stretches() as usize];
            let cross_pos =
                line_cross_origin + parent.align.leading_fraction() * (line_cross - cross_size);
            let rect = apply_aspect(
                rect_from_axis(horizontal, main_pos, cross_pos, main_size, cross_size),
                style,
            );
            Some((children[j].0, rect))
        })
        .collect()
}

/// Build a screen rect from main/cross coordinates, branchlessly selecting which
/// axis is x vs y by the orientation flag.
fn rect_from_axis(
    horizontal: bool,
    main_pos: f32,
    cross_pos: f32,
    main_size: f32,
    cross_size: f32,
) -> LayoutRect {
    let h = horizontal as usize;
    LayoutRect::from_edges(
        [cross_pos, main_pos][h],
        [main_pos, cross_pos][h],
        [cross_size, main_size][h].max(0.0),
        [main_size, cross_size][h].max(0.0),
    )
}

/// If a node declares an aspect, letterbox its box to that `width:height` ratio,
/// centred. A non-positive ratio is ignored. Branchless via `Option` combinators.
fn apply_aspect(rect: LayoutRect, style: &LayoutStyle) -> LayoutRect {
    style
        .aspect
        .map(|r| r.get())
        .filter(|&a| a > 0.0)
        .map(|aspect| {
            let (x, y) = (rect.left().get(), rect.top().get());
            let (w, h) = (rect.width().get(), rect.height().get());
            let fit_w = w.min(h * aspect);
            let fit_h = fit_w / aspect;
            LayoutRect::from_edges(x + (w - fit_w) * 0.5, y + (h - fit_h) * 0.5, fit_w, fit_h)
        })
        .unwrap_or(rect)
}
