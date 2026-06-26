//! End-to-end solver tests: build a viewport + tree, `solve`, assert the rects.
//! These exercise every branch of the solver through its public surface.

use axiom_host::{HostSafeAreaInsets, HostViewport, Pixels};
use axiom_kernel::Ratio;
use axiom_layout::{
    solve, Align, CrossSize, Direction, FlexWrap, Insets, Justify, LayoutRect, LayoutStyle,
    LayoutTreeBuilder, NodeId,
};

fn vp(w: u32, h: u32) -> HostViewport {
    HostViewport::new(w, h, Ratio::new(1.0).unwrap()).unwrap()
}

fn px(v: f32) -> Pixels {
    Pixels::new(v).unwrap()
}

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn id(n: u32) -> NodeId {
    NodeId::from_raw(n)
}

#[track_caller]
fn assert_rect(rect: LayoutRect, x: f32, y: f32, w: f32, h: f32) {
    let got = (
        rect.left().get(),
        rect.top().get(),
        rect.width().get(),
        rect.height().get(),
    );
    let near = (got.0 - x).abs() < 0.05
        && (got.1 - y).abs() < 0.05
        && (got.2 - w).abs() < 0.05
        && (got.3 - h).abs() < 0.05;
    assert!(near, "rect {got:?} != expected ({x}, {y}, {w}, {h})");
}

/// A grow=g, basis=b item style (a child that takes a share of free space).
fn item(basis: f32, grow: f32) -> LayoutStyle {
    let mut s = LayoutStyle::new();
    s.basis = px(basis);
    s.grow = ratio(grow);
    s
}

#[test]
fn empty_tree_solves_to_nothing() {
    let tree = LayoutTreeBuilder::new().build();
    let result = solve(&vp(800, 600), &tree);
    assert!(result.is_empty());
    assert_eq!(result.rect(id(0)), None);
}

#[test]
fn lone_root_fills_the_viewport() {
    let mut b = LayoutTreeBuilder::new();
    b.root(id(0), LayoutStyle::new());
    let result = solve(&vp(800, 600), &b.build());
    assert_eq!(result.len(), 1);
    assert_rect(result.rect(id(0)).unwrap(), 0.0, 0.0, 800.0, 600.0);
}

#[test]
fn root_is_inset_by_the_safe_area() {
    let viewport = vp(400, 800).with_safe_area_insets(
        HostSafeAreaInsets::new(px(44.0), px(0.0), px(34.0), px(0.0)).unwrap(),
    );
    let mut b = LayoutTreeBuilder::new();
    b.root(id(0), LayoutStyle::new());
    let result = solve(&viewport, &b.build());
    // 44px reserved at the top, 34px at the bottom; full width.
    assert_rect(
        result.rect(id(0)).unwrap(),
        0.0,
        44.0,
        400.0,
        800.0 - 44.0 - 34.0,
    );
}

#[test]
fn row_grow_splits_the_width_evenly() {
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), LayoutStyle::new());
    b.child(root, id(1), item(0.0, 1.0));
    b.child(root, id(2), item(0.0, 1.0));
    let result = solve(&vp(1000, 600), &b.build());
    assert_rect(result.rect(id(1)).unwrap(), 0.0, 0.0, 500.0, 600.0);
    assert_rect(result.rect(id(2)).unwrap(), 500.0, 0.0, 500.0, 600.0);
}

#[test]
fn column_grow_splits_the_height() {
    let mut root_style = LayoutStyle::new();
    root_style.direction = Direction::Column;
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), root_style);
    b.child(root, id(1), item(0.0, 1.0));
    b.child(root, id(2), item(0.0, 3.0));
    let result = solve(&vp(1000, 800), &b.build());
    // 1:3 split of 800 = 200 / 600, stacked vertically, full width.
    assert_rect(result.rect(id(1)).unwrap(), 0.0, 0.0, 1000.0, 200.0);
    assert_rect(result.rect(id(2)).unwrap(), 0.0, 200.0, 1000.0, 600.0);
}

#[test]
fn basis_plus_grow_is_the_roomed_puzzle_shape_landscape() {
    // Adaptive root in landscape → row: board grows, panel is a fixed 332px column.
    let mut root_style = LayoutStyle::new();
    root_style.direction = Direction::Adaptive;
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), root_style);
    b.child(root, id(1), item(0.0, 1.0)); // board
    b.child(root, id(2), item(332.0, 0.0)); // panel
    let result = solve(&vp(1000, 600), &b.build());
    assert_rect(result.rect(id(1)).unwrap(), 0.0, 0.0, 668.0, 600.0);
    assert_rect(result.rect(id(2)).unwrap(), 668.0, 0.0, 332.0, 600.0);
}

#[test]
fn adaptive_stacks_the_panel_below_the_board_in_portrait() {
    // Same tree, portrait viewport → column: panel reflows BELOW the board.
    let mut root_style = LayoutStyle::new();
    root_style.direction = Direction::Adaptive;
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), root_style);
    b.child(root, id(1), item(0.0, 1.0)); // board
    b.child(root, id(2), item(332.0, 0.0)); // panel
    let result = solve(&vp(600, 1000), &b.build());
    assert_rect(result.rect(id(1)).unwrap(), 0.0, 0.0, 600.0, 668.0);
    assert_rect(result.rect(id(2)).unwrap(), 0.0, 668.0, 600.0, 332.0);
}

#[test]
fn justify_center_end_and_space_between() {
    let make = |justify: Justify| {
        let mut root_style = LayoutStyle::new();
        root_style.justify = justify;
        let mut b = LayoutTreeBuilder::new();
        let root = b.root(id(0), root_style);
        b.child(root, id(1), item(100.0, 0.0));
        b.child(root, id(2), item(100.0, 0.0));
        solve(&vp(600, 200), &b.build())
    };
    // 200px of content, 400px free. Center → 200 leading. End → 400 leading.
    assert_rect(
        make(Justify::Center).rect(id(1)).unwrap(),
        200.0,
        0.0,
        100.0,
        200.0,
    );
    assert_rect(
        make(Justify::End).rect(id(1)).unwrap(),
        400.0,
        0.0,
        100.0,
        200.0,
    );
    // SpaceBetween → first at start, second pushed to the far end (400 between).
    let sb = make(Justify::SpaceBetween);
    assert_rect(sb.rect(id(1)).unwrap(), 0.0, 0.0, 100.0, 200.0);
    assert_rect(sb.rect(id(2)).unwrap(), 500.0, 0.0, 100.0, 200.0);
}

#[test]
fn align_positions_a_fixed_cross_child() {
    let make = |align: Align| {
        let mut root_style = LayoutStyle::new();
        root_style.align = align;
        let mut child = item(0.0, 1.0);
        child.cross = CrossSize::fixed(px(100.0));
        let mut b = LayoutTreeBuilder::new();
        let root = b.root(id(0), root_style);
        b.child(root, id(1), child);
        solve(&vp(400, 300), &b.build())
    };
    // 100px-tall child in a 300px line: Start → y0, Center → y100, End → y200.
    assert_rect(
        make(Align::Start).rect(id(1)).unwrap(),
        0.0,
        0.0,
        400.0,
        100.0,
    );
    assert_rect(
        make(Align::Center).rect(id(1)).unwrap(),
        0.0,
        100.0,
        400.0,
        100.0,
    );
    assert_rect(
        make(Align::End).rect(id(1)).unwrap(),
        0.0,
        200.0,
        400.0,
        100.0,
    );
    // Stretch overrides the fixed cross size, filling the line.
    assert_rect(
        make(Align::Stretch).rect(id(1)).unwrap(),
        0.0,
        0.0,
        400.0,
        300.0,
    );
}

#[test]
fn gap_and_padding_are_applied() {
    let mut root_style = LayoutStyle::new();
    root_style.gap = px(20.0);
    root_style.padding = Insets::uniform(px(10.0));
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), root_style);
    b.child(root, id(1), item(0.0, 1.0));
    b.child(root, id(2), item(0.0, 1.0));
    let result = solve(&vp(1000, 600), &b.build());
    // Content box is inset 10 each side → 980×580 at (10,10); 20px gap between two
    // equal children: each = (980-20)/2 = 480.
    assert_rect(result.rect(id(1)).unwrap(), 10.0, 10.0, 480.0, 580.0);
    assert_rect(
        result.rect(id(2)).unwrap(),
        10.0 + 480.0 + 20.0,
        10.0,
        480.0,
        580.0,
    );
}

#[test]
fn min_and_max_clamp_the_main_size() {
    // A grow child capped by max; another floored by min.
    let mut capped = item(0.0, 1.0);
    capped.max_main = Some(px(200.0));
    let mut floored = item(0.0, 1.0);
    floored.min_main = px(400.0);
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), LayoutStyle::new());
    b.child(root, id(1), capped);
    b.child(root, id(2), floored);
    let result = solve(&vp(1000, 100), &b.build());
    // Equal grow would give 500 each; capped clamps to 200, floored to >=400 (gets 500).
    assert_eq!(result.rect(id(1)).unwrap().width().get(), 200.0);
    assert!(result.rect(id(2)).unwrap().width().get() >= 400.0);
}

#[test]
fn aspect_letterboxes_a_square_into_a_wide_cell() {
    let mut board = item(0.0, 1.0);
    board.aspect = Some(ratio(1.0)); // square
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), LayoutStyle::new());
    b.child(root, id(1), board);
    let result = solve(&vp(1000, 600), &b.build());
    // The 1000×600 cell fits a 600×600 square, centred horizontally (x = 200).
    assert_rect(result.rect(id(1)).unwrap(), 200.0, 0.0, 600.0, 600.0);
}

#[test]
fn non_positive_aspect_is_ignored() {
    let mut board = item(0.0, 1.0);
    board.aspect = Some(ratio(0.0)); // ignored
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), LayoutStyle::new());
    b.child(root, id(1), board);
    let result = solve(&vp(800, 400), &b.build());
    assert_rect(result.rect(id(1)).unwrap(), 0.0, 0.0, 800.0, 400.0);
}

#[test]
fn wrap_breaks_children_into_equal_height_rows() {
    let mut root_style = LayoutStyle::new();
    root_style.wrap = FlexWrap::Wrap;
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), root_style);
    // Three 400px-wide items in a 1000px row → 2 fit on line 0, the third wraps.
    b.child(root, id(1), item(400.0, 0.0));
    b.child(root, id(2), item(400.0, 0.0));
    b.child(root, id(3), item(400.0, 0.0));
    let result = solve(&vp(1000, 600), &b.build());
    // 2 lines, each 300px tall. Items 1 & 2 on the top line, item 3 on the bottom.
    assert_eq!(result.rect(id(1)).unwrap().top().get(), 0.0);
    assert_eq!(result.rect(id(2)).unwrap().top().get(), 0.0);
    assert_eq!(result.rect(id(3)).unwrap().top().get(), 300.0);
    assert_eq!(result.rect(id(1)).unwrap().height().get(), 300.0);
}

#[test]
fn nested_containers_propagate_top_down() {
    // root → middle (a column container) → two grandchildren.
    let mut b = LayoutTreeBuilder::new();
    let root = b.root(id(0), LayoutStyle::new());
    let middle = b.child(root, id(1), {
        let mut s = item(0.0, 1.0);
        s.direction = Direction::Column;
        s
    });
    b.child(middle, id(2), item(0.0, 1.0));
    b.child(middle, id(3), item(0.0, 1.0));
    let result = solve(&vp(400, 800), &b.build());
    // middle fills the viewport; its two children split the height.
    assert_rect(result.rect(id(1)).unwrap(), 0.0, 0.0, 400.0, 800.0);
    assert_rect(result.rect(id(2)).unwrap(), 0.0, 0.0, 400.0, 400.0);
    assert_rect(result.rect(id(3)).unwrap(), 0.0, 400.0, 400.0, 400.0);
}

#[test]
fn a_second_root_and_its_children_are_left_unplaced() {
    // A malformed multi-root tree: only the first root is seeded, so the second
    // root (no parent set its rect) and its child are dropped — exercising the
    // "parent rect missing" path branchlessly.
    let mut b = LayoutTreeBuilder::new();
    b.root(id(0), LayoutStyle::new());
    let orphan_root = b.root(id(1), LayoutStyle::new());
    b.child(orphan_root, id(2), item(0.0, 1.0));
    let result = solve(&vp(800, 600), &b.build());
    assert!(result.rect(id(0)).is_some());
    assert_eq!(result.rect(id(1)), None);
    assert_eq!(result.rect(id(2)), None);
}
