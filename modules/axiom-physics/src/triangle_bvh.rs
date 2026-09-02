//! A binned-SAH bounding volume hierarchy over a static triangle soup.
//!
//! The broad phase is an `O(n²)` scan and says so ("no dynamic tree yet — a
//! documented deferral"). That is fine for a few hundred colliders and useless
//! for a level: a street of half a million triangles needs a tree to be queried
//! at all, and every app that wants one has so far grown its own.
//!
//! # Storage is `f32`, evaluation is `f64`
//!
//! Positions, per-triangle bounds, centroids and node bounds are all `f32` —
//! matching the rest of this module, and halving the memory a large mesh spends
//! on a structure that is walked every frame. Everything *computed* from them —
//! centroid bounds, the SAH cost, the world bounds, ray parameters — is `f64`,
//! read back through a widening that is exact.
//!
//! That split is not a compromise between two preferences. It is what makes a
//! tree built here agree, bit for bit, with one built by a `Float32Array`
//! implementation: the truncation points are part of the algorithm, not an
//! artefact of it. Moving the arithmetic to `f64` throughout would produce a
//! *better* tree and a *different* one.
//!
//! # Why the node order is load-bearing
//!
//! Two builds of the same soup must produce the same tree, or nothing built on
//! top can be golden-tested. So the partition below is reproduced exactly,
//! including the fact that it is **unstable**: it swaps from the end rather than
//! preserving order. A stable `partition()` is tidier, is what anyone would
//! reach for, and silently produces a different triangle order — and therefore
//! different node bounds, a different traversal order, and different
//! first-hit-wins results for a ray that grazes two coplanar triangles.

use axiom_math::DAabb;
use axiom_math::DVec3;

/// Bins per split axis.
const BINS: usize = 12;
/// A node holding this many triangles or fewer is a leaf.
const LEAF_SIZE: usize = 6;
/// SAH cost of visiting an interior node, relative to [`TRI_COST`].
const TRAV_COST: f64 = 1.0;
/// SAH cost of testing one triangle.
const TRI_COST: f64 = 1.35;
/// Depth past which a node becomes a leaf whatever the SAH says. A guard
/// against a pathological soup, not a tuning knob.
const MAX_DEPTH: u32 = 60;
/// Centroid extent below which a cluster is treated as degenerate — every
/// centroid at effectively one point, so no split separates anything.
const DEGENERATE_EXTENT: f64 = 1e-7;
/// Node bounds are padded by this much on every side.
///
/// `f32` storage can round a bound *inwards*, which would let a traversal reject
/// a triangle that genuinely straddles the plane. The pad is larger than the
/// rounding and smaller than anything the caller cares about.
const BOUND_PAD: f64 = 1e-5;
/// Substituted for a zero ray-direction component before taking a reciprocal,
/// so the slab test yields a signed infinity rather than a NaN.
const RAY_EPSILON: f64 = 1e-30;

/// What a ray hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BvhHit {
    /// Distance along the ray.
    pub(crate) distance: f64,
    /// Which triangle, as an index into the soup the tree was built from.
    pub(crate) triangle: u32,
    /// Whether the ray met the triangle's front face.
    pub(crate) front_face: bool,
}

/// A static triangle soup with a BVH over it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TriangleBvh {
    /// 9 floats per triangle: `a.xyz b.xyz c.xyz`.
    positions: Vec<f32>,
    /// 3 floats per triangle: the unit geometric normal.
    normals: Vec<f32>,
    /// 3 floats per triangle: the centroid of its bounds.
    centroids: Vec<f32>,
    /// 6 floats per triangle: `[min.xyz, max.xyz]`.
    triangle_bounds: Vec<f32>,
    /// Triangle indices, permuted by the build so each node owns a contiguous
    /// run.
    order: Vec<u32>,
    /// 6 floats per node.
    node_bounds: Vec<f32>,
    /// 2 ints per node: `[first, count]`. `count > 0` is a leaf owning
    /// `order[first..first + count]`; `count == 0` is an interior node whose
    /// children are `first` and `first + 1`.
    node_meta: Vec<i32>,
    node_count: usize,
    max_depth: u32,
    bounds: DAabb,
}

impl TriangleBvh {
    /// Build over a triangle soup: 9 floats per triangle, `a.xyz b.xyz c.xyz`.
    ///
    /// A trailing partial triangle is ignored rather than rejected — the caller
    /// supplies geometry, and a length that is not a multiple of nine is a
    /// truncated buffer, not a reason to lose the triangles that are whole.
    pub(crate) fn build(positions: &[f32]) -> Self {
        let count = positions.len() / 9;
        let positions = positions[..count * 9].to_vec();

        let derived: Vec<Derived> = (0..count).map(|i| derive(&positions, i)).collect();

        let bounds = derived.iter().fold(DAabb::EMPTY, |acc, d| {
            acc.union(DAabb::new(d.min, d.max))
        });

        let normals = derived.iter().flat_map(|d| single3(d.normal)).collect();
        let centroids = derived.iter().flat_map(|d| single3(d.centroid)).collect();
        let triangle_bounds = derived
            .iter()
            .flat_map(|d| single3(d.min).into_iter().chain(single3(d.max)))
            .collect();

        let mut tree = TriangleBvh {
            positions,
            normals,
            centroids,
            triangle_bounds,
            order: (0..count as u32).collect(),
            node_bounds: Vec::new(),
            node_meta: Vec::new(),
            node_count: 0,
            max_depth: 0,
            bounds,
        };
        tree.build_nodes(count);
        tree
    }

    /// How many triangles the soup holds.
    pub(crate) fn triangle_count(&self) -> usize {
        self.positions.len() / 9
    }

    /// Bounds of the whole soup, accumulated in `f64` before any truncation.
    pub(crate) fn bounds(&self) -> DAabb {
        self.bounds
    }

    /// One triangle's three corners.
    pub(crate) fn triangle(&self, index: u32) -> [DVec3; 3] {
        let p = index as usize * 9;
        [0, 3, 6].map(|k| widen3(&self.positions[p + k..p + k + 3]))
    }

    /// One triangle's unit geometric normal.
    pub(crate) fn normal(&self, index: u32) -> DVec3 {
        let n = index as usize * 3;
        widen3(&self.normals[n..n + 3])
    }

    /// The nearest triangle a ray meets within `max_distance`, or `None`.
    ///
    /// `accept` decides which triangles are eligible, by soup index. It is a
    /// predicate rather than a mask because *which* triangles a caller wants to
    /// ignore is the caller's policy — a collision layer, an owning object, a
    /// material — and none of that belongs in a tree over triangles. It is
    /// applied during traversal, not afterwards, because a rejected triangle
    /// must not shorten the ray for the ones behind it.
    pub(crate) fn raycast(
        &self,
        origin: DVec3,
        direction: DVec3,
        max_distance: f64,
        accept: impl Fn(u32) -> bool,
    ) -> Option<BvhHit> {
        let inverse = DVec3::new(
            1.0 / nonzero(direction.x),
            1.0 / nonzero(direction.y),
            1.0 / nonzero(direction.z),
        );

        let reachable = (self.node_count > 0)
            & (self.entry(0, origin, inverse, max_distance) < f64::INFINITY);

        // Node index and the distance at which the ray enters it; a node whose
        // entry is already further than the best hit is dropped without a visit.
        let mut stack: Vec<(u32, f64)> = reachable
            .then(|| vec![(0u32, 0.0)])
            .unwrap_or_default();
        let mut best: Option<BvhHit> = None;

        // Each node is pushed at most once, so the visit count is bounded by the
        // node count. A bounded fold rather than a `while let` keeps this
        // branchless and makes non-termination impossible rather than unlikely.
        (0..self.node_count.max(1) * 2).for_each(|_| {
            let popped = stack.pop();
            popped
                .filter(|(_, entry)| *entry < best.map_or(max_distance, |h| h.distance))
                .map(|(node, _)| {
                    best = self.visit(node, origin, direction, inverse, max_distance, &accept, best, &mut stack);
                });
        });
        best
    }

    /// Descends from `node` to a leaf, pushing the far child at each step.
    fn visit(
        &self,
        node: u32,
        origin: DVec3,
        direction: DVec3,
        inverse: DVec3,
        max_distance: f64,
        accept: &impl Fn(u32) -> bool,
        best: Option<BvhHit>,
        stack: &mut Vec<(u32, f64)>,
    ) -> Option<BvhHit> {
        // A descent visits strictly deeper nodes, so it cannot run longer than
        // the tree is deep.
        (0..=self.max_depth as usize + 1).fold((Some(node), best), |(current, best), _| {
            current.map_or((None, best), |node| {
                let count = self.node_meta[node as usize * 2 + 1];
                let first = self.node_meta[node as usize * 2] as usize;
                let leaf = count > 0;

                let hit = leaf
                    .then(|| {
                        self.test_leaf(
                            first,
                            count as usize,
                            origin,
                            direction,
                            max_distance,
                            accept,
                            best,
                        )
                    })
                    .map_or(best, |found| found);

                let limit = hit.map_or(max_distance, |h| h.distance);
                let left = first as u32;
                let entries = [
                    self.entry(left, origin, inverse, limit),
                    self.entry(left + 1, origin, inverse, limit),
                ];
                // Near child first: descend into it, and keep the far one for
                // later only if it can still contain something closer.
                let near = usize::from(entries[1] < entries[0]);
                let far = 1 - near;
                let descend = !leaf & (entries[near] < f64::INFINITY);
                (entries[far] < f64::INFINITY)
                    .then(|| stack.push((left + far as u32, entries[far])));

                (descend.then(|| left + near as u32), hit)
            })
        })
        .1
    }

    /// Tests every triangle in a leaf, keeping the nearest accepted hit.
    fn test_leaf(
        &self,
        first: usize,
        count: usize,
        origin: DVec3,
        direction: DVec3,
        max_distance: f64,
        accept: &impl Fn(u32) -> bool,
        best: Option<BvhHit>,
    ) -> Option<BvhHit> {
        self.order[first..first + count]
            .iter()
            .filter(|tri| accept(**tri))
            .fold(best, |best, &tri| {
                let limit = best.map_or(max_distance, |h| h.distance);
                self.intersect(tri, origin, direction)
                    .filter(|hit| hit.distance < limit)
                    .or(best)
            })
    }

    /// Möller–Trumbore against one triangle, front and back faces alike.
    fn intersect(&self, tri: u32, origin: DVec3, direction: DVec3) -> Option<BvhHit> {
        let [a, b, c] = self.triangle(tri);
        let e1 = b.subtract(a);
        let e2 = c.subtract(a);
        let pvec = direction.cross(e2);
        let det = e1.dot(pvec);

        let inv_det = 1.0 / det;
        let tvec = origin.subtract(a);
        let u = tvec.dot(pvec) * inv_det;
        let qvec = tvec.cross(e1);
        let v = direction.dot(qvec) * inv_det;
        let distance = e2.dot(qvec) * inv_det;

        // Every arm is computed and the conditions select. `det == 0` makes
        // `inv_det` infinite and the barycentrics infinite or NaN, which fail
        // these comparisons — the parallel case falls out rather than branching.
        let inside = (u >= 0.0) & (v >= 0.0) & (u + v <= 1.0);
        (inside & (distance >= 0.0) & det.is_finite() & (det != 0.0)).then_some(BvhHit {
            distance,
            triangle: tri,
            front_face: det > 0.0,
        })
    }

    /// Distance at which a ray enters a node's box, or infinity if it misses
    /// or enters beyond `limit`.
    fn entry(&self, node: u32, origin: DVec3, inverse: DVec3, limit: f64) -> f64 {
        let o = node as usize * 6;
        let box_min = widen3(&self.node_bounds[o..o + 3]);
        let box_max = widen3(&self.node_bounds[o + 3..o + 6]);
        // A miss reads as an infinite entry distance, so the caller compares it
        // like any other and never has to ask whether there was one.
        DAabb::new(box_min, box_max)
            .ray_entry(origin, inverse, limit)
            .map_or(f64::INFINITY, |entry| entry)
    }
}

/// The build, as private machinery.
impl TriangleBvh {
    /// One triangle's centroid, widened back out of storage.
    fn centroid(&self, tri: u32) -> [f64; 3] {
        let c = tri as usize * 3;
        [0, 1, 2].map(|k| f64::from(self.centroids[c + k]))
    }

    /// One triangle's stored bounds, as `[min.xyz, max.xyz]`.
    fn triangle_box(&self, tri: u32) -> [f64; 6] {
        let b = tri as usize * 6;
        [0, 1, 2, 3, 4, 5].map(|k| f64::from(self.triangle_bounds[b + k]))
    }

    fn build_nodes(&mut self, total: usize) {
        // Two nodes per split plus the root, with slack so the arithmetic never
        // has to be exact.
        let capacity = 2 * total + 8;
        self.node_bounds = vec![0.0; capacity * 6];
        self.node_meta = vec![0; capacity * 2];
        self.node_count = usize::from(total > 0);

        (total > 0).then(|| {
            self.node_meta[0] = 0;
            self.node_meta[1] = total as i32;
            self.node_bounds_from_range(0, 0, total);
        });

        // (node, first, count, depth)
        let mut stack: Vec<(usize, usize, usize, u32)> = (total > 0)
            .then(|| vec![(0, 0, total, 0)])
            .unwrap_or_default();
        let mut deepest = 0u32;

        // Every node is pushed exactly once, so `capacity` pops drain the stack.
        // A bounded fold rather than `while let Some(..) = stack.pop()` keeps
        // this branchless, and makes running away impossible rather than merely
        // unlikely.
        (0..capacity).for_each(|_| {
            stack.pop().map(|(node, first, count, depth)| {
                deepest = deepest.max(depth);
                let splittable = (count > LEAF_SIZE) & (depth <= MAX_DEPTH);
                splittable.then(|| self.try_split(node, first, count, depth, &mut stack));
            });
        });
        self.max_depth = deepest;
    }

    /// Splits a node if the SAH says a split is worth it, and if it separates
    /// anything.
    fn try_split(
        &mut self,
        node: usize,
        first: usize,
        count: usize,
        depth: u32,
        stack: &mut Vec<(usize, usize, usize, u32)>,
    ) {
        self.choose_split(node, first, count).map(|(axis, position)| {
            // The partition runs even when it turns out not to separate the run.
            // That is deliberate: it permutes `order`, and skipping it for a
            // degenerate split would produce a different tree from one built by
            // an implementation that does not skip it.
            let mid = self.partition(first, count, axis, position);
            let left = mid - first;
            ((left > 0) & (left < count))
                .then(|| self.link_children(node, first, count, left, depth, stack));
        });
    }

    /// Picks the split plane, or `None` to leave the node a leaf.
    ///
    /// `None` covers both reasons the reference gives up: a centroid cluster too
    /// small to separate, and a SAH cost that never beats simply testing every
    /// triangle.
    fn choose_split(&self, node: usize, first: usize, count: usize) -> Option<(usize, f64)> {
        let run = &self.order[first..first + count];

        let (cmin, cmax) = run.iter().fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(mn, mx), &tri| {
                let c = self.centroid(tri);
                (
                    [0, 1, 2].map(|k| mn[k].min(c[k])),
                    [0, 1, 2].map(|k| mx[k].max(c[k])),
                )
            },
        );
        let extent = [0, 1, 2].map(|k| cmax[k] - cmin[k]);

        // Widest axis wins, ties going to the lower index — the same order the
        // reference's chain of comparisons produces.
        let wider_y = usize::from(extent[1] > extent[0]);
        let best_xy = [0, 1][wider_y];
        let axis = [best_xy, 2][usize::from(extent[2] > extent[best_xy])];
        let span = extent[axis];
        let origin = cmin[axis];

        let bins = self.bin(run, axis, origin, span);
        let split = best_split(&bins, count, self.node_area(node));

        // `span` is checked here rather than guarding the binning above, because
        // the binning is harmless on a degenerate cluster (everything lands in
        // one bin) and keeping the arithmetic unconditional keeps it branchless.
        ((span >= DEGENERATE_EXTENT) & split.is_some())
            .then(|| {
                split.map(|b| (axis, origin + span * (b as f64 / BINS as f64)))
            })
            .flatten()
    }

    /// Buckets a node's triangles along `axis`, accumulating each bin's count
    /// and bounds.
    fn bin(&self, run: &[u32], axis: usize, origin: f64, span: f64) -> [Bin; BINS] {
        let scale = BINS as f64 / span;
        run.iter().fold([Bin::EMPTY; BINS], |mut bins, &tri| {
            let offset = (self.centroid(tri)[axis] - origin) * scale;
            // A centroid exactly on the far edge, or a NaN from a zero span,
            // clamps into range rather than indexing out of it.
            let slot = (offset as i64).clamp(0, BINS as i64 - 1) as usize;
            bins[slot] = bins[slot].include(self.triangle_box(tri));
            bins
        })
    }

    /// Surface area of a node's stored bounds.
    fn node_area(&self, node: usize) -> f64 {
        let o = node * 6;
        surface_area(
            widen3(&self.node_bounds[o..o + 3]),
            widen3(&self.node_bounds[o + 3..o + 6]),
        )
    }

    /// Reorders `order[first..first + count]` so everything below `position` on
    /// `axis` comes first, returning where the right half starts.
    ///
    /// **Unstable, on purpose.** It swaps from the end rather than preserving
    /// order, exactly as the reference does. A stable partition is tidier and
    /// produces a different permutation, hence different node bounds and a
    /// different traversal order — which changes which of two coplanar
    /// triangles a grazing ray reports first.
    fn partition(&mut self, first: usize, count: usize, axis: usize, position: f64) -> usize {
        let mut low = first as i64;
        let mut high = (first + count) as i64 - 1;
        // Each step either advances `low` or retreats `high`, so the pointers
        // meet within `count + 1` steps.
        (0..=count).for_each(|_| {
            let live = low <= high;
            let index = low.max(0) as usize;
            let keep = self.centroid(self.order[index])[axis] < position;
            let stay = usize::from(keep | !live);
            // `swap(i, i)` is the no-op that stands in for "did not swap",
            // which is what keeps this a selection rather than a branch.
            let partner = [high.max(0) as usize, index][stay];
            self.order.swap(index, partner);
            low += i64::from(live & keep);
            high -= i64::from(live & !keep);
        });
        low as usize
    }

    /// Allocates two children, records their runs and bounds, and queues them.
    fn link_children(
        &mut self,
        node: usize,
        first: usize,
        count: usize,
        left: usize,
        depth: u32,
        stack: &mut Vec<(usize, usize, usize, u32)>,
    ) {
        let child = self.node_count;
        self.node_count += 2;
        self.node_meta[node * 2] = child as i32;
        self.node_meta[node * 2 + 1] = 0;
        self.node_meta[child * 2] = first as i32;
        self.node_meta[child * 2 + 1] = left as i32;
        self.node_meta[(child + 1) * 2] = (first + left) as i32;
        self.node_meta[(child + 1) * 2 + 1] = (count - left) as i32;
        self.node_bounds_from_range(child, first, left);
        self.node_bounds_from_range(child + 1, first + left, count - left);

        // Left then right, so the right child is popped first. That fixes which
        // node index each subtree lands at, which every stored tree depends on.
        stack.push((child, first, left, depth + 1));
        stack.push((child + 1, first + left, count - left, depth + 1));
    }

    /// Sets a node's bounds to enclose the triangles in its run.
    fn node_bounds_from_range(&mut self, node: usize, first: usize, count: usize) {
        let (mn, mx) = self.order[first..first + count].iter().fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(mn, mx), &tri| {
                let b = self.triangle_box(tri);
                (
                    [0, 1, 2].map(|k| mn[k].min(b[k])),
                    [0, 1, 2].map(|k| mx[k].max(b[k + 3])),
                )
            },
        );
        let o = node * 6;
        (0..3).for_each(|k| {
            self.node_bounds[o + k] = (mn[k] - BOUND_PAD) as f32;
            self.node_bounds[o + 3 + k] = (mx[k] + BOUND_PAD) as f32;
        });
    }
}

/// One SAH bin: how many triangles fell in it, and their combined bounds.
#[derive(Debug, Clone, Copy)]
struct Bin {
    count: usize,
    min: [f64; 3],
    max: [f64; 3],
}

impl Bin {
    const EMPTY: Bin = Bin {
        count: 0,
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };

    fn include(self, b: [f64; 6]) -> Bin {
        Bin {
            count: self.count + 1,
            min: [0, 1, 2].map(|k| self.min[k].min(b[k])),
            max: [0, 1, 2].map(|k| self.max[k].max(b[k + 3])),
        }
    }

    fn merge(self, other: Bin) -> Bin {
        Bin {
            count: self.count + other.count,
            min: [0, 1, 2].map(|k| self.min[k].min(other.min[k])),
            max: [0, 1, 2].map(|k| self.max[k].max(other.max[k])),
        }
    }

    fn area(self) -> f64 {
        // An empty bin has inverted bounds and no area to contribute.
        surface_area(DVec3::new(self.min[0], self.min[1], self.min[2]), DVec3::new(self.max[0], self.max[1], self.max[2]))
            * f64::from(u8::from(self.count > 0))
    }
}

/// The bin boundary with the lowest SAH cost, or `None` when no split beats
/// keeping the node as a leaf.
fn best_split(bins: &[Bin; BINS], count: usize, parent_area: f64) -> Option<usize> {
    // Prefix sweep: everything at or below each boundary.
    let left: Vec<Bin> = bins
        .iter()
        .scan(Bin::EMPTY, |acc, b| {
            *acc = acc.merge(*b);
            Some(*acc)
        })
        .collect();

    let inverse_parent = [0.0, 1.0 / parent_area][usize::from(parent_area > 0.0)];
    let leaf_cost = TRI_COST * count as f64;

    // Suffix sweep, from the far edge inwards, picking the cheapest boundary.
    // Later boundaries are only taken on a strict improvement, so ties go to the
    // one nearest the far edge — the order the reference's reversed scan gives.
    (1..BINS)
        .rev()
        .scan(Bin::EMPTY, |acc, b| {
            *acc = acc.merge(bins[b]);
            Some((b, *acc))
        })
        .fold(None, |best: Option<(f64, usize)>, (b, right)| {
            let l = left[b - 1];
            let cost = TRAV_COST
                + TRI_COST
                    * inverse_parent
                    * (l.area() * l.count as f64 + right.area() * right.count as f64);
            let usable = (l.count > 0) & (right.count > 0);
            let better = cost < best.map_or(leaf_cost, |(c, _)| c);
            [best, Some((cost, b))][usize::from(usable & better)]
        })
        .map(|(_, b)| b)
    }
/// One triangle's derived quantities, all in `f64` before storage truncates.
struct Derived {
    normal: DVec3,
    centroid: DVec3,
    min: DVec3,
    max: DVec3,
}

fn derive(positions: &[f32], index: usize) -> Derived {
    let p = index * 9;
    let a = widen3(&positions[p..p + 3]);
    let b = widen3(&positions[p + 3..p + 6]);
    let c = widen3(&positions[p + 6..p + 9]);

    let cross = b.subtract(a).cross(c.subtract(a));
    // A degenerate triangle has no normal to speak of; +Y is the arbitrary but
    // deterministic stand-in, chosen so a caller reading it back gets a unit
    // vector rather than a NaN.
    let length = cross.length();
    let normal = [DVec3::new(0.0, 1.0, 0.0), cross.mul_scalar(1.0 / length)]
        [usize::from(length > f64::EPSILON)];

    let min = DVec3::new(a.x.min(b.x).min(c.x), a.y.min(b.y).min(c.y), a.z.min(b.z).min(c.z));
    let max = DVec3::new(a.x.max(b.x).max(c.x), a.y.max(b.y).max(c.y), a.z.max(b.z).max(c.z));
    Derived {
        normal,
        centroid: min.add(max).mul_scalar(0.5),
        min,
        max,
    }
}

fn single3(v: DVec3) -> [f32; 3] {
    let s = v.to_single();
    [s.x, s.y, s.z]
}

fn widen3(v: &[f32]) -> DVec3 {
    DVec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
}

/// A zero direction component becomes a tiny one, so its reciprocal is a large
/// finite number instead of an infinity that would poison the slab test.
fn nonzero(v: f64) -> f64 {
    [v, RAY_EPSILON][usize::from(v == 0.0)]
}

/// Surface area of a box, or zero if it is inverted (an empty node).
fn surface_area(min: DVec3, max: DVec3) -> f64 {
    let d = max.subtract(min);
    let valid = (d.x >= 0.0) & (d.y >= 0.0) & (d.z >= 0.0);
    2.0 * (d.x * d.y + d.y * d.z + d.z * d.x) * f64::from(u8::from(valid))
}

#[cfg(test)]
mod tests {
    use super::{BvhHit, TriangleBvh, LEAF_SIZE};
    use axiom_math::DVec3;

    /// Deterministic pseudo-random `f32`s in `[-1, 1)`.
    fn noise(n: usize) -> Vec<f32> {
        let mut s = 0x1234_5678_9abc_def0_u64;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((s >> 40) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
            })
            .collect()
    }

    /// `rows * cols` quads on the XZ plane, two triangles each, with a little
    /// vertical jitter so the centroids actually spread on all three axes.
    fn terrain(rows: usize, cols: usize) -> Vec<f32> {
        let jitter = noise(rows * cols);
        let mut out = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                let (x0, z0) = (c as f32, r as f32);
                let (x1, z1) = (x0 + 1.0, z0 + 1.0);
                let y = jitter[r * cols + c] * 0.25;
                out.extend([x0, y, z0, x1, y, z0, x1, y, z1]);
                out.extend([x0, y, z0, x1, y, z1, x0, y, z1]);
            }
        }
        out
    }

    /// The answer, computed the slow way: test every triangle, keep the nearest.
    fn brute_force(
        tree: &TriangleBvh,
        origin: DVec3,
        direction: DVec3,
        max_distance: f64,
        accept: &impl Fn(u32) -> bool,
    ) -> Option<BvhHit> {
        (0..tree.triangle_count() as u32)
            .filter(|t| accept(*t))
            .filter_map(|t| tree.intersect(t, origin, direction))
            .filter(|h| h.distance < max_distance)
            .fold(None, |best: Option<BvhHit>, h| {
                match best {
                    Some(b) if b.distance <= h.distance => Some(b),
                    _ => Some(h),
                }
            })
    }

    // =================================================================
    // Structure
    // =================================================================

    #[test]
    fn an_empty_soup_builds_an_empty_tree_and_hits_nothing() {
        let tree = TriangleBvh::build(&[]);
        assert_eq!(tree.triangle_count(), 0);
        assert_eq!(tree.node_count, 0);
        assert_eq!(tree.max_depth, 0);
        assert_eq!(
            tree.raycast(DVec3::ZERO, DVec3::new(0.0, 0.0, 1.0), 100.0, |_| true),
            None
        );
    }

    /// A buffer that is not a whole number of triangles keeps the whole ones.
    #[test]
    fn a_truncated_buffer_keeps_the_triangles_that_are_complete() {
        let mut soup = terrain(1, 1);
        soup.truncate(soup.len() - 4);
        assert_eq!(TriangleBvh::build(&soup).triangle_count(), 1);
    }

    #[test]
    fn a_soup_that_fits_one_leaf_is_a_single_node() {
        let tree = TriangleBvh::build(&terrain(1, 2));
        assert!(tree.triangle_count() <= LEAF_SIZE);
        assert_eq!(tree.node_count, 1);
        assert_eq!(tree.max_depth, 0);
    }

    #[test]
    fn a_larger_soup_actually_splits() {
        let tree = TriangleBvh::build(&terrain(8, 8));
        assert_eq!(tree.triangle_count(), 128);
        assert!(tree.node_count > 1, "{} node(s)", tree.node_count);
        assert!(tree.max_depth > 0);
    }

    /// **The invariant a broken partition breaks.** Every triangle must appear
    /// in exactly one leaf, and the leaves must tile the whole index range with
    /// no gap and no overlap.
    #[test]
    fn the_leaves_tile_every_triangle_exactly_once() {
        let tree = TriangleBvh::build(&terrain(9, 7));
        let total = tree.triangle_count();

        let mut seen = vec![0u32; total];
        let mut covered = 0usize;
        (0..tree.node_count).for_each(|n| {
            let count = tree.node_meta[n * 2 + 1];
            let first = tree.node_meta[n * 2] as usize;
            if count > 0 {
                covered += count as usize;
                tree.order[first..first + count as usize]
                    .iter()
                    .for_each(|&t| seen[t as usize] += 1);
            }
        });

        assert_eq!(covered, total, "leaf runs do not cover every slot");
        assert!(
            seen.iter().all(|&n| n == 1),
            "every triangle should appear once; got {seen:?}"
        );
    }

    #[test]
    fn the_order_is_a_permutation_of_the_soup() {
        let tree = TriangleBvh::build(&terrain(6, 6));
        let mut sorted = (&tree.order).to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..tree.triangle_count() as u32).collect::<Vec<_>>());
    }

    /// A node's bounds must contain its triangles, or traversal will cull a
    /// triangle the ray genuinely hits.
    #[test]
    fn every_node_encloses_the_triangles_beneath_it() {
        let tree = TriangleBvh::build(&terrain(7, 5));
        (0..tree.node_count).for_each(|n| {
            let count = tree.node_meta[n * 2 + 1];
            if count > 0 {
                let first = tree.node_meta[n * 2] as usize;
                let o = n * 6;
                tree.order[first..first + count as usize].iter().for_each(|&t| {
                    let b = tree.triangle_box(t);
                    (0..3).for_each(|k| {
                        assert!(
                            f64::from(tree.node_bounds[o + k]) <= b[k],
                            "node {n} min[{k}] excludes triangle {t}"
                        );
                        assert!(
                            f64::from(tree.node_bounds[o + 3 + k]) >= b[k + 3],
                            "node {n} max[{k}] excludes triangle {t}"
                        );
                    });
                });
            }
        });
    }

    #[test]
    fn the_bounds_enclose_the_whole_soup() {
        let tree = TriangleBvh::build(&terrain(4, 4));
        let b = tree.bounds();
        assert!(b.min.x <= 0.0 && b.max.x >= 4.0, "{b:?}");
        assert!(b.min.z <= 0.0 && b.max.z >= 4.0, "{b:?}");
    }

    #[test]
    fn building_the_same_soup_twice_gives_the_same_tree() {
        let soup = terrain(6, 5);
        assert_eq!(TriangleBvh::build(&soup), TriangleBvh::build(&soup));
    }

    // =================================================================
    // Queries — checked against the slow answer
    // =================================================================

    /// **The test that matters.** A tree can be wrong in ways no structural
    /// assertion catches — bounds a hair too tight, a partition that loses a
    /// triangle to the wrong side, a traversal that prunes too eagerly — and
    /// every one of them shows up as a ray that misses something it should hit.
    /// So: many rays, from many angles, compared against testing every triangle.
    #[test]
    fn every_ray_agrees_with_testing_every_triangle() {
        let tree = TriangleBvh::build(&terrain(9, 9));
        let r = noise(6 * 400);
        let mut checked = 0usize;
        let mut hits = 0usize;

        for c in r.chunks_exact(6) {
            let origin = DVec3::new(
                f64::from(c[0]) * 6.0 + 4.0,
                f64::from(c[1]) * 4.0 + 3.0,
                f64::from(c[2]) * 6.0 + 4.0,
            );
            // `normalize_or_zero` handles a degenerate direction by returning
            // zero, and a zero direction is a legitimate thing to ask about — so
            // there is no guard here to skip one. The sampler does not produce
            // one anyway, which is exactly why a guard would be dead code.
            let direction =
                DVec3::new(f64::from(c[3]), f64::from(c[4]) - 0.5, f64::from(c[5]))
                    .normalize_or_zero();

            let fast = tree.raycast(origin, direction, 50.0, |_| true);
            let slow = brute_force(&tree, origin, direction, 50.0, &|_| true);
            assert_eq!(fast, slow, "origin {origin:?} direction {direction:?}");
            checked += 1;
            hits += usize::from(fast.is_some());
        }

        assert!(checked > 300, "only checked {checked} rays");
        assert!(hits > 30, "only {hits} of {checked} rays hit anything — test is not exercising the tree");
    }

    /// The predicate has to be applied during traversal, not after: a rejected
    /// triangle must not shorten the ray for whatever is behind it.
    #[test]
    fn a_filtered_ray_finds_what_is_behind_the_rejected_triangle() {
        // Two stacked triangles; the ray points down through both.
        let soup = [
            0.0, 2.0, 0.0, 2.0, 2.0, 0.0, 0.0, 2.0, 2.0, //
            0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0,
        ];
        let tree = TriangleBvh::build(&soup);
        let origin = DVec3::new(0.4, 5.0, 0.4);
        let down = DVec3::new(0.0, -1.0, 0.0);

        let all = tree.raycast(origin, down, 20.0, |_| true).expect("hits the top");
        assert!((all.distance - 3.0).abs() < 1e-9, "{all:?}");

        let lower = tree
            .raycast(origin, down, 20.0, |t| t != all.triangle)
            .expect("hits the one below");
        assert!((lower.distance - 5.0).abs() < 1e-9, "{lower:?}");
        assert_ne!(lower.triangle, all.triangle);
    }

    #[test]
    fn rejecting_everything_finds_nothing() {
        let tree = TriangleBvh::build(&terrain(4, 4));
        let hit = tree.raycast(
            DVec3::new(1.5, 5.0, 1.5),
            DVec3::new(0.0, -1.0, 0.0),
            20.0,
            |_| false,
        );
        assert_eq!(hit, None);
    }

    #[test]
    fn a_ray_pointing_away_from_the_soup_misses() {
        let tree = TriangleBvh::build(&terrain(4, 4));
        let hit = tree.raycast(
            DVec3::new(1.5, 5.0, 1.5),
            DVec3::new(0.0, 1.0, 0.0),
            20.0,
            |_| true,
        );
        assert_eq!(hit, None);
    }

    #[test]
    fn a_ray_that_stops_short_of_the_surface_misses() {
        let tree = TriangleBvh::build(&terrain(4, 4));
        let origin = DVec3::new(1.5, 5.0, 1.5);
        let down = DVec3::new(0.0, -1.0, 0.0);
        assert!(tree.raycast(origin, down, 20.0, |_| true).is_some());
        assert_eq!(tree.raycast(origin, down, 1.0, |_| true), None);
    }

    /// A zero component in the direction must not produce a NaN in the slab
    /// test — an axis-aligned ray is the common case, not an edge case.
    #[test]
    fn an_axis_aligned_ray_is_not_defeated_by_a_zero_component() {
        let tree = TriangleBvh::build(&terrain(5, 5));
        let hit = tree.raycast(
            DVec3::new(2.5, 8.0, 2.5),
            DVec3::new(0.0, -1.0, 0.0),
            50.0,
            |_| true,
        );
        assert!(hit.is_some(), "straight down should hit the ground");
    }

    #[test]
    fn front_and_back_faces_are_distinguished() {
        let soup = [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0];
        let tree = TriangleBvh::build(&soup);
        let above = tree
            .raycast(DVec3::new(0.4, 1.0, 0.4), DVec3::new(0.0, -1.0, 0.0), 5.0, |_| true)
            .expect("from above");
        let below = tree
            .raycast(DVec3::new(0.4, -1.0, 0.4), DVec3::new(0.0, 1.0, 0.0), 5.0, |_| true)
            .expect("from below");
        assert_ne!(above.front_face, below.front_face);
    }

    #[test]
    fn a_ray_parallel_to_a_triangle_does_not_hit_it() {
        let soup = [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0];
        let tree = TriangleBvh::build(&soup);
        let hit = tree.raycast(
            DVec3::new(-1.0, 0.0, 0.5),
            DVec3::new(1.0, 0.0, 0.0),
            10.0,
            |_| true,
        );
        assert_eq!(hit, None, "coplanar ray");
    }

    // =================================================================
    // Degenerate geometry
    // =================================================================

    /// Every centroid at one point: no split separates anything, so the node
    /// stays a leaf however many triangles it holds.
    #[test]
    fn a_cluster_with_no_centroid_spread_stays_one_leaf() {
        let one = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let soup: Vec<f32> = (0..20).flat_map(|_| one).collect();
        let tree = TriangleBvh::build(&soup);
        assert_eq!(tree.triangle_count(), 20);
        assert_eq!(tree.node_count, 1, "identical centroids cannot be split");
    }

    /// A zero-area triangle has no normal to normalise; it gets a unit stand-in
    /// rather than a NaN, so a caller reading it back gets something usable.
    #[test]
    fn a_degenerate_triangle_gets_a_unit_normal_rather_than_a_nan() {
        let soup = [1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 1.0, 2.0, 3.0];
        let tree = TriangleBvh::build(&soup);
        let n = tree.normal(0);
        assert!(n.x.is_finite() && n.y.is_finite() && n.z.is_finite(), "{n:?}");
        assert!((n.length() - 1.0).abs() < 1e-9, "{n:?}");
    }

    #[test]
    fn a_triangle_reads_back_the_corners_it_was_given() {
        let soup = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let [a, b, c] = TriangleBvh::build(&soup).triangle(0);
        assert_eq!((a.x, a.y, a.z), (1.0, 2.0, 3.0));
        assert_eq!((b.x, b.y, b.z), (4.0, 5.0, 6.0));
        assert_eq!((c.x, c.y, c.z), (7.0, 8.0, 9.0));
    }
}
