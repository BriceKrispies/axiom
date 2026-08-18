//! Ported from `three/src/extras/lib/earcut.js` (Three.js r180, MIT licensed
//! — a vendored copy of `mapbox/earcut` v3.0.1, ISC licensed), which
//! `THREE.ShapeUtils.triangulateShape` calls to cap `ExtrudeGeometry`'s front
//! and back faces. `extrude()` (`extrude.rs`) needs that exact triangulation
//! — vertex order and ear-selection order decide the index buffer a golden
//! test compares — so the ear-clipping algorithm is ported whole, not
//! approximated with a different triangulator.
//!
//! Specialized two ways relative to the original, both harmless because
//! every caller in this kit only ever needs them:
//!
//! - **`dim` is fixed at 2.** Every shape `extrude()` sees is a flat 2-D
//!   profile, so the generic `data`-plus-`dim` flat-array indexing collapses
//!   to a plain `&[(f64, f64)]` slice — no `i / dim`, no `i * dim` scattered
//!   through the port.
//! - **`hole_indices` are point indices, not flat coordinate offsets.** The
//!   original takes flat offsets and immediately multiplies by `dim`
//!   (`holeIndices[0] * dim`) to get a point index; `dim = 2` here makes that
//!   multiply a no-op, so the caller (`triangulate_shape` in `extrude.rs`,
//!   mirroring `ShapeUtils.triangulateShape`) just hands over point indices
//!   directly.
//!
//! The algorithm itself — ear slicing with a z-order-curve spatial index,
//! self-intersection curing, and polygon splitting as a last resort, plus
//! hole elimination via bridge edges — is untouched. It is pure `+ - * /`
//! and comparisons over `f64` (never `sin`/`cos`/`sqrt`), so given the same
//! input points it reproduces the exact same triangle index sequence as the
//! JavaScript, bit for bit.

/// One node of the circular doubly-linked polygon ring `earcut` operates on.
/// The original mutates a graph of `{i, x, y, prev, next, z, prevZ, nextZ,
/// steiner}` object references; here that graph is an arena (`Vec<Node>`)
/// addressed by index, since Rust has no cheap mutable node-graph of
/// object references. `prev`/`next` are always valid indices (every node is
/// inserted into a circular list immediately, self-linked if it is the
/// first), matching the source's `createNode`/`insertNode` split; `prev_z`/
/// `next_z` start `None`, matching the source's `null` defaults.
#[derive(Clone, Copy)]
struct Node {
    /// Index into the caller's `points` slice — the value every emitted
    /// triangle index actually is.
    i: u32,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
    z: i64,
    prev_z: Option<usize>,
    next_z: Option<usize>,
    steiner: bool,
}

/// `Earcut.triangulate(data, holeIndices)` (`earcut.js:5-42`), flattened for
/// `dim = 2` and point-indexed holes (see module docs). Returns the flat
/// triangle index list, three `u32`s (indices into `points`) per triangle.
pub(super) fn earcut(points: &[(f64, f64)], hole_indices: &[usize]) -> Vec<u32> {
    let mut arena: Vec<Node> = Vec::new();
    let has_holes = !hole_indices.is_empty();
    let outer_len = if has_holes {
        hole_indices[0]
    } else {
        points.len()
    };
    let mut outer_node = linked_list(&mut arena, points, 0, outer_len, true);
    let mut triangles: Vec<u32> = Vec::new();

    let degenerate = outer_node.is_none_or(|n| arena[n].next == arena[n].prev);
    if degenerate {
        return triangles;
    }

    let mut min_x = 0.0;
    let mut min_y = 0.0;
    let mut inv_size = 0.0;

    if has_holes {
        outer_node = Some(eliminate_holes(
            &mut arena,
            points,
            hole_indices,
            outer_node.expect("checked non-degenerate above"),
        ));
    }

    // `data.length > 80 * dim` (`earcut.js:19`) in point terms: more than 80
    // points overall. Below that the plain (unhashed) ear test is cheap
    // enough that the z-order spatial index isn't worth building.
    if points.len() > 80 {
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        min_x = f64::INFINITY;
        min_y = f64::INFINITY;
        // The source's bbox loop starts at `i = dim` (point index 1), not 0
        // (`earcut.js:25`) — the first point never widens the box. Preserved
        // exactly: it feeds the z-order hash, and the hash feeds ear-search
        // order, which a golden test compares index-for-index.
        (1..outer_len).for_each(|i| {
            let (x, y) = points[i];
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        });
        let span = (max_x - min_x).max(max_y - min_y);
        inv_size = if span != 0.0 { 32767.0 / span } else { 0.0 };
    }

    earcut_linked(&mut arena, outer_node, &mut triangles, min_x, min_y, inv_size, 0);
    triangles
}

fn create_node(arena: &mut Vec<Node>, i: u32, x: f64, y: f64) -> usize {
    arena.push(Node {
        i,
        x,
        y,
        prev: 0,
        next: 0,
        z: 0,
        prev_z: None,
        next_z: None,
        steiner: false,
    });
    arena.len() - 1
}

fn insert_node(arena: &mut Vec<Node>, i: u32, x: f64, y: f64, last: Option<usize>) -> usize {
    let idx = create_node(arena, i, x, y);
    match last {
        None => {
            arena[idx].prev = idx;
            arena[idx].next = idx;
        }
        Some(l) => {
            let ln = arena[l].next;
            arena[idx].next = ln;
            arena[idx].prev = l;
            arena[ln].prev = idx;
            arena[l].next = idx;
        }
    }
    idx
}

fn remove_node(arena: &mut Vec<Node>, p: usize) {
    let next = arena[p].next;
    let prev = arena[p].prev;
    arena[next].prev = prev;
    arena[prev].next = next;
    if let Some(pz) = arena[p].prev_z {
        arena[pz].next_z = arena[p].next_z;
    }
    if let Some(nz) = arena[p].next_z {
        arena[nz].prev_z = arena[p].prev_z;
    }
}

fn equals_node(arena: &[Node], p: usize, q: usize) -> bool {
    arena[p].x == arena[q].x && arena[p].y == arena[q].y
}

fn area(arena: &[Node], p: usize, q: usize, r: usize) -> f64 {
    (arena[q].y - arena[p].y) * (arena[r].x - arena[q].x)
        - (arena[q].x - arena[p].x) * (arena[r].y - arena[q].y)
}

fn signed_area(points: &[(f64, f64)], start: usize, end: usize) -> f64 {
    let mut sum = 0.0;
    let mut j = end - 1;
    (start..end).for_each(|i| {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        sum += (xj - xi) * (yi + yj);
        j = i;
    });
    sum
}

fn linked_list(
    arena: &mut Vec<Node>,
    points: &[(f64, f64)],
    start: usize,
    end: usize,
    clockwise: bool,
) -> Option<usize> {
    let mut last: Option<usize> = None;
    if clockwise == (signed_area(points, start, end) > 0.0) {
        (start..end).for_each(|i| {
            last = Some(insert_node(arena, i as u32, points[i].0, points[i].1, last));
        });
    } else {
        (start..end).rev().for_each(|i| {
            last = Some(insert_node(arena, i as u32, points[i].0, points[i].1, last));
        });
    }
    last.and_then(|l| {
        let ln = arena[l].next;
        if equals_node(arena, l, ln) {
            remove_node(arena, l);
            Some(ln)
        } else {
            Some(l)
        }
    })
}

fn filter_points(arena: &mut Vec<Node>, start: usize, end: Option<usize>) -> usize {
    let mut end = end.unwrap_or(start);
    let mut p = start;
    loop {
        let mut again = false;
        let steiner = arena[p].steiner;
        let pn = arena[p].next;
        let pp = arena[p].prev;
        if !steiner && (equals_node(arena, p, pn) || area(arena, pp, p, pn) == 0.0) {
            remove_node(arena, p);
            p = pp;
            end = p;
            if p == arena[p].next {
                break;
            }
            again = true;
        } else {
            p = pn;
        }
        if !(again || p != end) {
            break;
        }
    }
    end
}

fn earcut_linked(
    arena: &mut Vec<Node>,
    ear: Option<usize>,
    triangles: &mut Vec<u32>,
    min_x: f64,
    min_y: f64,
    inv_size: f64,
    pass: u8,
) {
    let mut ear = match ear {
        Some(e) => e,
        None => return,
    };

    if pass == 0 && inv_size != 0.0 {
        index_curve(arena, ear, min_x, min_y, inv_size);
    }

    let mut stop = ear;
    loop {
        if arena[ear].prev == arena[ear].next {
            break;
        }
        let prev = arena[ear].prev;
        let next = arena[ear].next;

        let is_ear_result = if inv_size != 0.0 {
            is_ear_hashed(arena, ear, min_x, min_y, inv_size)
        } else {
            is_ear(arena, ear)
        };

        if is_ear_result {
            triangles.push(arena[prev].i);
            triangles.push(arena[ear].i);
            triangles.push(arena[next].i);

            remove_node(arena, ear);

            ear = arena[next].next;
            stop = arena[next].next;
            continue;
        }

        ear = next;

        if ear == stop {
            if pass == 0 {
                let filtered = filter_points(arena, ear, None);
                earcut_linked(arena, Some(filtered), triangles, min_x, min_y, inv_size, 1);
            } else if pass == 1 {
                let filtered = filter_points(arena, ear, None);
                let cured = cure_local_intersections(arena, filtered, triangles);
                earcut_linked(arena, Some(cured), triangles, min_x, min_y, inv_size, 2);
            } else {
                split_earcut(arena, ear, triangles, min_x, min_y, inv_size);
            }
            break;
        }
    }
}

fn is_ear(arena: &[Node], ear: usize) -> bool {
    let a = arena[ear].prev;
    let b = ear;
    let c = arena[ear].next;
    if area(arena, a, b, c) >= 0.0 {
        return false;
    }

    let (ax, ay) = (arena[a].x, arena[a].y);
    let (bx, by) = (arena[b].x, arena[b].y);
    let (cx, cy) = (arena[c].x, arena[c].y);
    let x0 = ax.min(bx).min(cx);
    let y0 = ay.min(by).min(cy);
    let x1 = ax.max(bx).max(cx);
    let y1 = ay.max(by).max(cy);

    let mut p = arena[c].next;
    while p != a {
        if arena[p].x >= x0
            && arena[p].x <= x1
            && arena[p].y >= y0
            && arena[p].y <= y1
            && point_in_triangle_except_first(ax, ay, bx, by, cx, cy, arena[p].x, arena[p].y)
            && area(arena, arena[p].prev, p, arena[p].next) >= 0.0
        {
            return false;
        }
        p = arena[p].next;
    }
    true
}

#[allow(clippy::too_many_lines)]
fn is_ear_hashed(arena: &[Node], ear: usize, min_x: f64, min_y: f64, inv_size: f64) -> bool {
    let a = arena[ear].prev;
    let b = ear;
    let c = arena[ear].next;
    if area(arena, a, b, c) >= 0.0 {
        return false;
    }

    let (ax, ay) = (arena[a].x, arena[a].y);
    let (bx, by) = (arena[b].x, arena[b].y);
    let (cx, cy) = (arena[c].x, arena[c].y);
    let x0 = ax.min(bx).min(cx);
    let y0 = ay.min(by).min(cy);
    let x1 = ax.max(bx).max(cx);
    let y1 = ay.max(by).max(cy);

    let min_z = z_order(x0, y0, min_x, min_y, inv_size);
    let max_z = z_order(x1, y1, min_x, min_y, inv_size);

    let inside = |n: usize| -> bool {
        n != a
            && n != c
            && arena[n].x >= x0
            && arena[n].x <= x1
            && arena[n].y >= y0
            && arena[n].y <= y1
            && point_in_triangle_except_first(ax, ay, bx, by, cx, cy, arena[n].x, arena[n].y)
            && area(arena, arena[n].prev, n, arena[n].next) >= 0.0
    };

    let mut p = arena[ear].prev_z;
    let mut n = arena[ear].next_z;

    loop {
        let (Some(pp), Some(nn)) = (p, n) else { break };
        if !(arena[pp].z >= min_z && arena[nn].z <= max_z) {
            break;
        }
        if inside(pp) {
            return false;
        }
        p = arena[pp].prev_z;

        if inside(nn) {
            return false;
        }
        n = arena[nn].next_z;
    }

    while let Some(pp) = p {
        if arena[pp].z < min_z {
            break;
        }
        if inside(pp) {
            return false;
        }
        p = arena[pp].prev_z;
    }

    while let Some(nn) = n {
        if arena[nn].z > max_z {
            break;
        }
        if inside(nn) {
            return false;
        }
        n = arena[nn].next_z;
    }

    true
}

fn cure_local_intersections(arena: &mut Vec<Node>, start: usize, triangles: &mut Vec<u32>) -> usize {
    let mut p = start;
    let mut loop_start = start;
    loop {
        let a = arena[p].prev;
        let pn = arena[p].next;
        let b = arena[pn].next;

        if !equals_node(arena, a, b)
            && intersects(arena, a, p, pn, b)
            && locally_inside(arena, a, b)
            && locally_inside(arena, b, a)
        {
            triangles.push(arena[a].i);
            triangles.push(arena[p].i);
            triangles.push(arena[b].i);

            remove_node(arena, p);
            remove_node(arena, pn);

            p = b;
            loop_start = b;
        }
        p = arena[p].next;
        if p == loop_start {
            break;
        }
    }
    filter_points(arena, p, None)
}

fn split_earcut(
    arena: &mut Vec<Node>,
    start: usize,
    triangles: &mut Vec<u32>,
    min_x: f64,
    min_y: f64,
    inv_size: f64,
) {
    let mut a = start;
    loop {
        let mut b = arena[arena[a].next].next;
        while b != arena[a].prev {
            if arena[a].i != arena[b].i && is_valid_diagonal(arena, a, b) {
                let c = split_polygon(arena, a, b);
                let a2 = filter_points(arena, a, Some(arena[a].next));
                let c2 = filter_points(arena, c, Some(arena[c].next));
                earcut_linked(arena, Some(a2), triangles, min_x, min_y, inv_size, 0);
                earcut_linked(arena, Some(c2), triangles, min_x, min_y, inv_size, 0);
                return;
            }
            b = arena[b].next;
        }
        a = arena[a].next;
        if a == start {
            break;
        }
    }
}

fn eliminate_holes(
    arena: &mut Vec<Node>,
    points: &[(f64, f64)],
    hole_indices: &[usize],
    outer_node: usize,
) -> usize {
    let len = hole_indices.len();
    let mut queue: Vec<usize> = Vec::with_capacity(len);
    (0..len).for_each(|i| {
        let start = hole_indices[i];
        let end = if i < len - 1 {
            hole_indices[i + 1]
        } else {
            points.len()
        };
        if let Some(list) = linked_list(arena, points, start, end, false) {
            if arena[list].next == list {
                arena[list].steiner = true;
            }
            queue.push(get_leftmost(arena, list));
        }
    });

    queue.sort_by(|&a, &b| compare_xy_slope(arena, a, b));

    let mut outer_node = outer_node;
    queue.into_iter().for_each(|q| {
        outer_node = eliminate_hole(arena, q, outer_node);
    });
    outer_node
}

fn compare_xy_slope(arena: &[Node], a: usize, b: usize) -> std::cmp::Ordering {
    let mut result = arena[a].x - arena[b].x;
    if result == 0.0 {
        result = arena[a].y - arena[b].y;
        if result == 0.0 {
            let an = arena[a].next;
            let bn = arena[b].next;
            let a_slope = (arena[an].y - arena[a].y) / (arena[an].x - arena[a].x);
            let b_slope = (arena[bn].y - arena[b].y) / (arena[bn].x - arena[b].x);
            result = a_slope - b_slope;
        }
    }
    result.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
}

fn eliminate_hole(arena: &mut Vec<Node>, hole: usize, outer_node: usize) -> usize {
    match find_hole_bridge(arena, hole, outer_node) {
        None => outer_node,
        Some(bridge) => {
            let bridge_reverse = split_polygon(arena, bridge, hole);
            let brn = arena[bridge_reverse].next;
            filter_points(arena, bridge_reverse, Some(brn));
            let bn = arena[bridge].next;
            filter_points(arena, bridge, Some(bn))
        }
    }
}

#[allow(clippy::too_many_lines)]
fn find_hole_bridge(arena: &[Node], hole: usize, outer_node: usize) -> Option<usize> {
    let hx = arena[hole].x;
    let hy = arena[hole].y;
    let mut qx = f64::NEG_INFINITY;
    let mut m: Option<usize> = None;

    if equals_node(arena, hole, outer_node) {
        return Some(outer_node);
    }

    let mut p = outer_node;
    loop {
        let pn = arena[p].next;
        if equals_node(arena, hole, pn) {
            return Some(pn);
        }
        let py = arena[p].y;
        let pny = arena[pn].y;
        if hy <= py && hy >= pny && pny != py {
            let x = arena[p].x + (hy - py) * (arena[pn].x - arena[p].x) / (pny - py);
            if x <= hx && x > qx {
                qx = x;
                m = Some(if arena[p].x < arena[pn].x { p } else { pn });
                if x == hx {
                    return m;
                }
            }
        }
        p = pn;
        if p == outer_node {
            break;
        }
    }

    let mut m = m?;

    let stop = m;
    let mx = arena[m].x;
    let my = arena[m].y;
    let mut tan_min = f64::INFINITY;
    let mut p = m;
    loop {
        let px = arena[p].x;
        let py = arena[p].y;
        if hx >= px && px >= mx && hx != px {
            let ax = if hy < my { hx } else { qx };
            let cx = if hy < my { qx } else { hx };
            if point_in_triangle(ax, hy, mx, my, cx, hy, px, py) {
                let tan = (hy - py).abs() / (hx - px);
                let better = tan < tan_min
                    || (tan == tan_min
                        && (px > arena[m].x || (px == arena[m].x && sector_contains_sector(arena, m, p))));
                if locally_inside(arena, p, hole) && better {
                    m = p;
                    tan_min = tan;
                }
            }
        }
        p = arena[p].next;
        if p == stop {
            break;
        }
    }
    Some(m)
}

fn sector_contains_sector(arena: &[Node], m: usize, p: usize) -> bool {
    area(arena, arena[m].prev, m, arena[p].prev) < 0.0
        && area(arena, arena[p].next, m, arena[m].next) < 0.0
}

fn index_curve(arena: &mut Vec<Node>, start: usize, min_x: f64, min_y: f64, inv_size: f64) {
    let mut p = start;
    loop {
        if arena[p].z == 0 {
            arena[p].z = z_order(arena[p].x, arena[p].y, min_x, min_y, inv_size);
        }
        arena[p].prev_z = Some(arena[p].prev);
        arena[p].next_z = Some(arena[p].next);
        p = arena[p].next;
        if p == start {
            break;
        }
    }
    let prev_z = arena[p].prev_z.expect("just assigned above");
    arena[prev_z].next_z = None;
    arena[p].prev_z = None;

    sort_linked(arena, p);
}

/// Simon Tatham's linked-list merge sort, ordering the z-index list by
/// [`Node::z`]. `earcut.js:391-443`.
fn sort_linked(arena: &mut Vec<Node>, list_start: usize) {
    let mut list: Option<usize> = Some(list_start);
    let mut in_size: usize = 1;

    loop {
        let mut p = list;
        list = None;
        let mut tail: Option<usize> = None;
        let mut num_merges: u32 = 0;

        while let Some(_head) = p {
            num_merges += 1;
            let mut q = p;
            let mut p_size = 0usize;
            for _ in 0..in_size {
                p_size += 1;
                q = q.and_then(|n| arena[n].next_z);
                if q.is_none() {
                    break;
                }
            }
            let mut q_size = in_size;

            while p_size > 0 || (q_size > 0 && q.is_some()) {
                let take_p = p_size != 0
                    && (q_size == 0
                        || q.is_none()
                        || arena[p.expect("p_size > 0")].z <= arena[q.expect("checked some")].z);
                let e = if take_p {
                    let e = p.expect("p_size > 0");
                    p = arena[e].next_z;
                    p_size -= 1;
                    e
                } else {
                    let e = q.expect("q branch taken");
                    q = arena[e].next_z;
                    q_size -= 1;
                    e
                };

                match tail {
                    Some(t) => arena[t].next_z = Some(e),
                    None => list = Some(e),
                }
                arena[e].prev_z = tail;
                tail = Some(e);
            }

            p = q;
        }

        if let Some(t) = tail {
            arena[t].next_z = None;
        }
        in_size *= 2;

        if num_merges <= 1 {
            break;
        }
    }
}

fn z_order(x: f64, y: f64, min_x: f64, min_y: f64, inv_size: f64) -> i64 {
    let mut xi = ((x - min_x) * inv_size) as i32;
    let mut yi = ((y - min_y) * inv_size) as i32;

    xi = (xi | (xi << 8)) & 0x00FF_00FF;
    xi = (xi | (xi << 4)) & 0x0F0F_0F0F;
    xi = (xi | (xi << 2)) & 0x3333_3333;
    xi = (xi | (xi << 1)) & 0x5555_5555;

    yi = (yi | (yi << 8)) & 0x00FF_00FF;
    yi = (yi | (yi << 4)) & 0x0F0F_0F0F;
    yi = (yi | (yi << 2)) & 0x3333_3333;
    yi = (yi | (yi << 1)) & 0x5555_5555;

    i64::from(xi | (yi << 1))
}

fn get_leftmost(arena: &[Node], start: usize) -> usize {
    let mut p = start;
    let mut leftmost = start;
    loop {
        if arena[p].x < arena[leftmost].x || (arena[p].x == arena[leftmost].x && arena[p].y < arena[leftmost].y) {
            leftmost = p;
        }
        p = arena[p].next;
        if p == start {
            break;
        }
    }
    leftmost
}

fn point_in_triangle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py)
        && (ax - px) * (by - py) >= (bx - px) * (ay - py)
        && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}

#[allow(clippy::too_many_arguments)]
fn point_in_triangle_except_first(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    cx: f64,
    cy: f64,
    px: f64,
    py: f64,
) -> bool {
    !(ax == px && ay == py) && point_in_triangle(ax, ay, bx, by, cx, cy, px, py)
}

fn is_valid_diagonal(arena: &[Node], a: usize, b: usize) -> bool {
    let an = arena[a].next;
    let ap = arena[a].prev;
    let bn = arena[b].next;
    let bp = arena[b].prev;
    let cond1 = arena[an].i != arena[b].i
        && arena[ap].i != arena[b].i
        && !intersects_polygon(arena, a, b)
        && (locally_inside(arena, a, b)
            && locally_inside(arena, b, a)
            && middle_inside(arena, a, b)
            && (area(arena, ap, a, bp) != 0.0 || area(arena, a, bp, b) != 0.0));
    let cond2 =
        equals_node(arena, a, b) && area(arena, ap, a, an) > 0.0 && area(arena, bp, b, bn) > 0.0;
    cond1 || cond2
}

fn intersects_polygon(arena: &[Node], a: usize, b: usize) -> bool {
    let mut p = a;
    loop {
        let pn = arena[p].next;
        if arena[p].i != arena[a].i
            && arena[pn].i != arena[a].i
            && arena[p].i != arena[b].i
            && arena[pn].i != arena[b].i
            && intersects(arena, p, pn, a, b)
        {
            return true;
        }
        p = pn;
        if p == a {
            break;
        }
    }
    false
}

fn intersects(arena: &[Node], p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
    let o1 = sign(area(arena, p1, q1, p2));
    let o2 = sign(area(arena, p1, q1, q2));
    let o3 = sign(area(arena, p2, q2, p1));
    let o4 = sign(area(arena, p2, q2, q1));

    (o1 != o2 && o3 != o4)
        || (o1 == 0 && on_segment(arena, p1, p2, q1))
        || (o2 == 0 && on_segment(arena, p1, q2, q1))
        || (o3 == 0 && on_segment(arena, p2, p1, q2))
        || (o4 == 0 && on_segment(arena, p2, q1, q2))
}

fn sign(x: f64) -> i32 {
    let positive = i32::from(x > 0.0);
    let negative = i32::from(x < 0.0);
    positive - negative
}

fn on_segment(arena: &[Node], p: usize, q: usize, r: usize) -> bool {
    arena[q].x <= arena[p].x.max(arena[r].x)
        && arena[q].x >= arena[p].x.min(arena[r].x)
        && arena[q].y <= arena[p].y.max(arena[r].y)
        && arena[q].y >= arena[p].y.min(arena[r].y)
}

fn locally_inside(arena: &[Node], a: usize, b: usize) -> bool {
    let ap = arena[a].prev;
    let an = arena[a].next;
    if area(arena, ap, a, an) < 0.0 {
        area(arena, a, b, an) >= 0.0 && area(arena, a, ap, b) >= 0.0
    } else {
        area(arena, a, b, ap) < 0.0 || area(arena, a, an, b) < 0.0
    }
}

fn middle_inside(arena: &[Node], a: usize, b: usize) -> bool {
    let px = (arena[a].x + arena[b].x) / 2.0;
    let py = (arena[a].y + arena[b].y) / 2.0;
    let mut p = a;
    let mut inside = false;
    loop {
        let pn = arena[p].next;
        let py_p = arena[p].y;
        let pny = arena[pn].y;
        if ((py_p > py) != (pny > py))
            && pny != py_p
            && px < (arena[pn].x - arena[p].x) * (py - py_p) / (pny - py_p) + arena[p].x
        {
            inside = !inside;
        }
        p = pn;
        if p == a {
            break;
        }
    }
    inside
}

fn split_polygon(arena: &mut Vec<Node>, a: usize, b: usize) -> usize {
    let a2 = create_node(arena, arena[a].i, arena[a].x, arena[a].y);
    let b2 = create_node(arena, arena[b].i, arena[b].x, arena[b].y);
    let an = arena[a].next;
    let bp = arena[b].prev;

    arena[a].next = b;
    arena[b].prev = a;

    arena[a2].next = an;
    arena[an].prev = a2;

    arena[b2].next = a2;
    arena[a2].prev = b2;

    arena[bp].next = b2;
    arena[b2].prev = bp;

    b2
}
