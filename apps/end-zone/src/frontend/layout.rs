//! Responsive frontend layout built on the engine's `axiom-layout` flex
//! solver: the screen shell (header / content / footer) is solved from the
//! real viewport with safe margins and bounded content width; widget-level
//! rows and stacks are small deterministic helpers over the solved regions.
//! Everything is in logical UI pixels (the presenter multiplies by the UI
//! scale), `+y` down.

use axiom_host::{HostViewport, Pixels};
use axiom_interface::{UiRect, UiUnit};
use axiom_kernel::Ratio;
use axiom_layout::{
    solve, Align, CrossSize, Direction, Insets, Justify, LayoutStyle, LayoutTreeBuilder, NodeId,
};

/// Total pixel constructor: sanitizes to a finite non-negative value first,
/// so `Pixels::new` (which rejects only non-finite input) always succeeds;
/// the recursive arm re-sanitizes and is unreachable in practice.
fn px(value: f32) -> Pixels {
    let sanitized = if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    };
    Pixels::new(sanitized).unwrap_or_else(|_| px(0.0))
}

fn ratio(value: f32) -> Ratio {
    Ratio::finite_or_zero(value)
}

/// One logical-pixel rectangle helper.
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> UiRect {
    UiRect::new(
        UiUnit::new(x),
        UiUnit::new(y),
        UiUnit::new(w),
        UiUnit::new(h),
    )
}

/// The solved shell regions every screen composes into.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellRegions {
    pub header: UiRect,
    pub content: UiRect,
    pub footer: UiRect,
}

/// The frontend layout context: logical viewport + orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutContext {
    pub width: f32,
    pub height: f32,
    pub portrait: bool,
}

/// Minimum viable logical viewport (guards degenerate sizes).
const MIN_SIDE: f32 = 240.0;

impl LayoutContext {
    /// Build from the presenter's logical size (CSS pixels ÷ UI scale).
    pub fn new(width: f32, height: f32) -> Self {
        let width = if width.is_finite() {
            width.max(MIN_SIDE)
        } else {
            MIN_SIDE
        };
        let height = if height.is_finite() {
            height.max(MIN_SIDE)
        } else {
            MIN_SIDE
        };
        LayoutContext {
            width,
            height,
            portrait: height > width,
        }
    }

    /// Solve the screen shell through the engine layout solver: a padded
    /// column of header (fixed), content (grows), footer (fixed), with the
    /// content clamped to a readable maximum width and centred.
    pub fn shell(&self) -> ShellRegions {
        const ROOT: NodeId = NodeId::from_raw(1);
        const HEADER: NodeId = NodeId::from_raw(2);
        const CONTENT: NodeId = NodeId::from_raw(3);
        const FOOTER: NodeId = NodeId::from_raw(4);

        let margin = if self.portrait { 12.0 } else { 24.0 };
        let header_h = if self.portrait { 72.0 } else { 92.0 };
        let footer_h = if self.portrait { 56.0 } else { 48.0 };

        let mut tree = LayoutTreeBuilder::new();
        let root = tree.root(
            ROOT,
            LayoutStyle {
                direction: Direction::Column,
                justify: Justify::Start,
                align: Align::Stretch,
                gap: px(8.0),
                padding: Insets::uniform(px(margin)),
                ..LayoutStyle::new()
            },
        );
        let child = |basis: f32, grow: f32| LayoutStyle {
            direction: Direction::Row,
            basis: px(basis),
            grow: ratio(grow),
            cross: CrossSize::stretch(),
            ..LayoutStyle::new()
        };
        tree.child(root, HEADER, child(header_h, 0.0));
        tree.child(root, CONTENT, child(0.0, 1.0));
        tree.child(root, FOOTER, child(footer_h, 0.0));

        let viewport = HostViewport::new(
            self.width.max(MIN_SIDE) as u32,
            self.height.max(MIN_SIDE) as u32,
            ratio(1.0),
        );
        let solved = viewport
            .ok()
            .map(|viewport| solve(&viewport, &tree.build()));
        let take = |id: NodeId, fallback: UiRect| -> UiRect {
            solved
                .as_ref()
                .and_then(|s| s.rect(id))
                .map(|r| {
                    rect(
                        r.left().get(),
                        r.top().get(),
                        r.width().get(),
                        r.height().get(),
                    )
                })
                .unwrap_or(fallback)
        };
        ShellRegions {
            header: take(HEADER, rect(0.0, 0.0, self.width, header_h)),
            content: take(
                CONTENT,
                rect(0.0, header_h, self.width, self.height - header_h - footer_h),
            ),
            footer: take(
                FOOTER,
                rect(0.0, self.height - footer_h, self.width, footer_h),
            ),
        }
    }

    /// Clamp a region to a bounded content width, centred.
    pub fn bounded(&self, region: UiRect, max_width: f32) -> UiRect {
        let w = region.w.get().min(max_width);
        let x = region.x.get() + (region.w.get() - w) / 2.0;
        rect(x, region.y.get(), w, region.h.get())
    }
}

/// Stack `count` fixed-height rows inside `region`, top-aligned with `gap`.
pub fn stack_rows(region: UiRect, row_h: f32, gap: f32, count: usize) -> Vec<UiRect> {
    (0..count)
        .map(|i| {
            rect(
                region.x.get(),
                region.y.get() + i as f32 * (row_h + gap),
                region.w.get(),
                row_h,
            )
        })
        .collect()
}

/// Stack `count` centred rows vertically in the middle of `region`.
pub fn centered_rows(
    region: UiRect,
    row_w: f32,
    row_h: f32,
    gap: f32,
    count: usize,
) -> Vec<UiRect> {
    let total = count as f32 * row_h + (count.saturating_sub(1)) as f32 * gap;
    let x = region.x.get() + (region.w.get() - row_w) / 2.0;
    let y0 = region.y.get() + (region.h.get() - total) / 2.0;
    (0..count)
        .map(|i| rect(x, y0 + i as f32 * (row_h + gap), row_w, row_h))
        .collect()
}

/// Split `region` into weighted columns with `gap` (weights normalized).
pub fn split_columns(region: UiRect, weights: &[f32], gap: f32) -> Vec<UiRect> {
    let total: f32 = weights.iter().sum::<f32>().max(1.0e-3);
    let inner = region.w.get() - gap * weights.len().saturating_sub(1) as f32;
    let mut x = region.x.get();
    weights
        .iter()
        .map(|w| {
            let width = (inner * w / total).max(0.0);
            let r = rect(x, region.y.get(), width, region.h.get());
            x += width + gap;
            r
        })
        .collect()
}

/// Split `region` into weighted stacked bands with `gap` (portrait fallback).
pub fn split_bands(region: UiRect, weights: &[f32], gap: f32) -> Vec<UiRect> {
    let total: f32 = weights.iter().sum::<f32>().max(1.0e-3);
    let inner = region.h.get() - gap * weights.len().saturating_sub(1) as f32;
    let mut y = region.y.get();
    weights
        .iter()
        .map(|w| {
            let height = (inner * w / total).max(0.0);
            let r = rect(region.x.get(), y, region.w.get(), height);
            y += height + gap;
            r
        })
        .collect()
}

/// Whether `point` (logical px) is inside `r` (half-open, matching the
/// interface layer's `UiRect::contains`).
pub fn contains(r: UiRect, x: f32, y: f32) -> bool {
    r.contains(UiUnit::new(x), UiUnit::new(y))
}
