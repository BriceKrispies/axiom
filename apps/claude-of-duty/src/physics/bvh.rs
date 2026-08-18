//! Ported from Claude-of-Duty `src/physics/bvh.js:1-933`.
//!
//! Static world: triangle soup + binned-SAH BVH. The source's own header:
//! "`world` registers meshes through `PhysicsSystem.addStatic()`; we bake them
//! into world space once, concatenate everything into flat typed arrays, and
//! build a BVH over the result. Nothing here allocates after `build()` —
//! queries run on preallocated stacks and write into caller-supplied
//! records."
//!
//! ## What is ported, and what is not
//!
//! Ported: [`StaticWorld`]'s registration (`add_triangles`/`remove_object`),
//! `build()` and its binned-SAH node construction, and every query —
//! [`StaticWorld::raycast`], [`StaticWorld::raycast_any`],
//! [`StaticWorld::query_aabb`], [`StaticWorld::overlap_capsule`], and the
//! conservative-advancement [`StaticWorld::sweep_capsule`].
//!
//! **Not ported: `bakeMesh` and `StaticWorld::addMesh`** (`bvh.js:104-125,
//! 836-933`). Both flatten a live `THREE.Mesh`/`InstancedMesh` — reading
//! `geometry.attributes.position`, `geometry.groups`,
//! `mesh.updateWorldMatrix`, per-instance matrices — into the same flat
//! triangle layout `addTriangles` accepts directly. This app has no
//! `THREE.Mesh` (or Axiom mesh) scene-graph arm yet; when one lands, its baker
//! should reproduce `bakeMesh`'s algorithm (per-group surface resolution via
//! [`crate::physics::surfaces::guess_surface`], degenerate-triangle drop,
//! instance flattening) writing into the same `positions`/`count`/`surface`
//! shape `add_triangles` already takes — the BVH and every query below are
//! unchanged either way, which is exactly why the recipe calls this "a pure
//! algorithm over flat typed arrays with no rendering contact."
//!
//! ## Return values, not out-parameters
//!
//! As in [`crate::physics::math`], every query here returns its result
//! instead of writing into a caller-supplied "out" record — the source's
//! allocation-avoidance convention has no equivalent GC pressure to avoid in
//! Rust. `Vec`-backed storage (triangle arrays, the node arrays, BVH
//! traversal stacks, [`Contacts`]) similarly grows on demand rather than
//! replicating the source's manual capacity-doubling; the resulting values are
//! identical, only the allocation strategy differs.
//!
//! ## Computes in `f64`, stores `f32`
//!
//! The source's *baked world data* — `pos`, `nrm`, `nodeBounds`, and the two
//! scratch arrays `_cent`/`_taabb` — are all `Float32Array`
//! (`bvh.js:49-57, 66-67, 271-273`), even though every arithmetic operation
//! that touches them runs as an ordinary JavaScript double: a read out of a
//! `Float32Array` widens to a full-precision `f64` with the value's `f32`
//! content, arithmetic proceeds in double precision, and the *write* back
//! into that array is what re-truncates to `f32`. `overlapCapsule`'s contact
//! buffer (`this.contacts`, `bvh.js:77-90`) is the same shape:
//! `nx/ny/nz/px/py/pz/depth/s` are `Float32Array`, `tri` is `Int32Array`.
//! `HitRecord` (`makeHitRecord()`, `math.js:29-45`) is the one exception —
//! it's a plain object literal, not a typed array, so a query *result* keeps
//! full `f64` precision even though the geometry it was computed from did
//! not.
//!
//! This matters beyond bookkeeping: `node_bounds_from_range` pads every node
//! AABB by `1e-5` (`bvh.js:421`), a constant with no exact binary
//! representation in *either* width, so the `f32`-truncated bound and the
//! `f64` value it was computed from are genuinely different numbers, not just
//! different encodings of the same one. A pure-`f64` port would silently drift
//! from the source's actual stored geometry. This file therefore stores
//! [`StaticWorld::pos`]/`nrm`/`node_bounds`/`cent`/`taabb` and every
//! [`Contacts`] field except `tri` as `f32` — computing every intermediate in
//! `f64` (matching JS) and narrowing only at the point the source's own
//! `Float32Array` would — exactly the "computes in `f64`, stores `f32`"
//! discipline already established for the weapon geometry port
//! (`apps/claude-of-duty/src/weapons/geometry`, commit `2fc45570`). Public
//! accessors ([`StaticWorld::node_bounds`], [`StaticWorld::aabb`]-adjacent
//! reads) still return `f64` — widening a stored `f32` back to `f64` is
//! exactly what a JS caller sees when it reads `world.nodeBounds[i]`, so the
//! golden captures compare directly against these accessors with no further
//! conversion.
//!
//! `StaticObject::tris` (this port's `add_triangles` input, staged before
//! `build()` copies it into `pos`) is kept at full `f64` — the source's own
//! `addTriangles(positions, ...)` accepts whatever typed array a caller
//! passes (its real caller, `bakeMesh`, happens to already hand it
//! `Float32Array`, but the function itself does not require that); the
//! `f32` truncation that matters is the one `build()` performs when it
//! copies triangle data into `this.pos`, which is exactly where this port
//! applies it too.
//!
//! ## What is numerically exact vs. tolerance-bound
//!
//! `build()`'s binned-SAH construction (bin assignment, surface-area sweeps,
//! the split decision) is built entirely from `+ - * /` and comparisons — no
//! `sqrt`/`sin`/`cos` — so for triangle geometry whose vertex coordinates and
//! derived AABBs are exactly representable in `f32` (as every golden fixture
//! in `tests/physics_port.rs` is, deliberately), node counts, node bounds and
//! the resulting tree shape are exact-equality goldens: the `f32` truncation
//! points above are no-ops for exactly-representable inputs, so there is
//! nothing to lose. [`StaticWorld::sweep_capsule`] and
//! [`StaticWorld::overlap_capsule`] both call `f64::sqrt` (via
//! `seg_triangle_closest`'s distance and the normal-length normalisation), so
//! their numeric outputs (`t`, contact `depth`, normals) are pinned with the
//! established `1e-12` tolerance instead.

use crate::physics::math::{ray_aabb, ray_triangle, seg_triangle_closest, HitRecord, EPS};
use crate::world::palette::Surface;

/// `bvh.js:33`.
pub const BINS: usize = 12;
/// `bvh.js:34`.
pub const LEAF_SIZE: usize = 6;
/// `bvh.js:35`.
pub const TRAV_COST: f64 = 1.0;
/// `bvh.js:36`.
pub const TRI_COST: f64 = 1.35;
/// Conservative-advancement tolerance, metres. `bvh.js:38`.
pub const CA_TOL: f64 = 1e-4;
/// `bvh.js:39`.
pub const CA_ITERS: u32 = 48;
/// `overlapCapsule`'s shared contact buffer capacity (`bvh.js:79`,
/// `this.contacts.capacity`). The source stops collecting once this many
/// contacts are found, even if more candidate triangles remain; this port
/// preserves that early exit.
pub const CONTACTS_CAPACITY: usize = 256;

/// World-space AABB. Mirrors `this.aabb` (`bvh.js:92`) — a plain object in
/// the source, not a typed array, so (unlike `node_bounds`) this stays `f64`
/// with no truncation; see the module doc comment.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Aabb {
    pub minx: f64,
    pub miny: f64,
    pub minz: f64,
    pub maxx: f64,
    pub maxy: f64,
    pub maxz: f64,
}

/// Penetration contacts collected by [`StaticWorld::overlap_capsule`].
/// Mirrors `this.contacts` (`bvh.js:77-90`): every field but `tri` is `f32`
/// storage (see the module doc comment), index-aligned — entry `i` is one
/// contact. Normals point out of the surface, towards the capsule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contacts {
    pub nx: Vec<f32>,
    pub ny: Vec<f32>,
    pub nz: Vec<f32>,
    pub px: Vec<f32>,
    pub py: Vec<f32>,
    pub pz: Vec<f32>,
    pub depth: Vec<f32>,
    /// Parameter along the query segment where the contact sits, 0..1.
    pub s: Vec<f32>,
    pub tri: Vec<i32>,
}

impl Contacts {
    pub fn count(&self) -> usize {
        self.tri.len()
    }
}

/// One registered triangle batch. Mirrors an entry of `this.objects`
/// (`bvh.js:45`), minus `mesh`/`userData` — there is no live mesh object to
/// reference in this port slice (see the module doc comment). `Option::None`
/// in `StaticWorld::objects` stands in for the source's `alive = false` *and*
/// `objects[id] = null` together: a freed slot is simply absent.
struct StaticObject {
    id: i32,
    #[allow(dead_code)] // carried for parity with the source; not yet read by any caller.
    name: String,
    surfaces: Vec<Surface>,
    mask: u16,
    /// Full `f64` precision, as supplied by the caller — see the module doc
    /// comment for why the `f32` truncation happens later, in `build()`.
    tris: Vec<f64>,
    tri_count: usize,
}

/// Static world: triangle soup + binned-SAH BVH. `bvh.js:43-823`
/// (`class StaticWorld`).
pub struct StaticWorld {
    objects: Vec<Option<StaticObject>>,
    free_ids: Vec<i32>,

    tri_count: usize,
    /// 9 floats per triangle: `a.xyz b.xyz c.xyz`, world space. `f32`
    /// storage — see the module doc comment.
    pos: Vec<f32>,
    /// 3 floats per triangle: unit geometric normal. `f32` storage.
    nrm: Vec<f32>,
    surface: Vec<Surface>,
    mask: Vec<u16>,
    object: Vec<i32>,

    tri_index: Vec<u32>,
    /// 6 floats per node: `[minx,miny,minz,maxx,maxy,maxz]`. `f32` storage.
    node_bounds: Vec<f32>,
    /// 2 ints per node: `[leftFirst, count]`. `count > 0`: leaf, triangles at
    /// `triIndex[leftFirst..leftFirst+count)`. `count == 0`: interior,
    /// children at `leftFirst` and `leftFirst + 1`.
    node_meta: Vec<i32>,
    node_count: usize,
    max_depth: u32,

    /// Per-triangle centroid, 3 floats each. `f32` storage, retained across
    /// queries so `build_nodes` and `node_bounds_from_range` can both read
    /// it.
    cent: Vec<f32>,
    /// Per-triangle AABB, 6 floats each. `f32` storage, same retention
    /// reason as `cent`.
    taabb: Vec<f32>,

    dirty: bool,
    version: u64,

    aabb: Aabb,
}

impl Default for StaticWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticWorld {
    /// `bvh.js:44-94` (the constructor). Telemetry-only fields from the
    /// source (`buildMs`, `stats.{rayTests,nodeTests,triTests}`) are dropped:
    /// they are never read by any query below and `buildMs` specifically
    /// would need wall-clock time, which the port's determinism rules avoid
    /// wherever the value isn't load-bearing.
    pub fn new() -> Self {
        StaticWorld {
            objects: Vec::new(),
            free_ids: Vec::new(),
            tri_count: 0,
            pos: Vec::new(),
            nrm: Vec::new(),
            surface: Vec::new(),
            mask: Vec::new(),
            object: Vec::new(),
            tri_index: Vec::new(),
            node_bounds: Vec::new(),
            node_meta: Vec::new(),
            node_count: 0,
            max_depth: 0,
            cent: Vec::new(),
            taabb: Vec::new(),
            dirty: false,
            version: 0,
            aabb: Aabb::default(),
        }
    }

    /* ---------------------------------------------------------------- */
    /* Registration                                                      */
    /* ---------------------------------------------------------------- */

    /// Register raw world-space triangles. `bvh.js:127-139`
    /// (`addTriangles`). `positions` must hold at least `count * 9` floats
    /// (`a.xyz b.xyz c.xyz` per triangle). The source defaults
    /// `mask = LAYER.STATIC`, `name = 'raw'`; Rust has no default arguments,
    /// so both are required here.
    pub fn add_triangles(
        &mut self,
        positions: &[f64],
        count: usize,
        surface: Surface,
        mask: u16,
        name: &str,
    ) -> i32 {
        let id = self.free_ids.pop().unwrap_or(self.objects.len() as i32);
        let obj = StaticObject {
            id,
            name: name.to_string(),
            surfaces: vec![surface; count],
            mask,
            tris: positions[..count * 9].to_vec(),
            tri_count: count,
        };
        let idx = id as usize;
        if idx >= self.objects.len() {
            self.objects.resize_with(idx + 1, || None);
        }
        self.objects[idx] = Some(obj);
        self.dirty = true;
        id
    }

    /// `bvh.js:141-151` (`removeObject`). Returns `false` for an id that was
    /// never registered or already removed.
    pub fn remove_object(&mut self, id: i32) -> bool {
        let idx = id as usize;
        match self.objects.get_mut(idx) {
            Some(slot @ Some(_)) => {
                *slot = None;
                self.free_ids.push(id);
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    /* ---------------------------------------------------------------- */
    /* Build                                                             */
    /* ---------------------------------------------------------------- */

    /// `bvh.js:165-249` (`build`).
    pub fn build(&mut self) {
        let total: usize = self.objects.iter().flatten().map(|o| o.tri_count).sum();

        if total == 0 {
            self.tri_count = 0;
            self.node_count = 0;
            self.dirty = false;
            self.version += 1;
            return;
        }

        self.pos.clear();
        self.surface.clear();
        self.mask.clear();
        self.object.clear();
        for obj in self.objects.iter().flatten() {
            // The `f64` -> `f32` truncation the source's `pos.set(...)`
            // (into a `Float32Array`) performs implicitly — see the module
            // doc comment.
            self.pos.extend(obj.tris[..obj.tri_count * 9].iter().map(|&v| v as f32));
            for i in 0..obj.tri_count {
                self.surface.push(obj.surfaces[i]);
                self.mask.push(obj.mask);
                self.object.push(obj.id);
            }
        }
        self.tri_count = total;

        // Per-triangle normals, centroids, bounds.
        self.nrm = vec![0.0; total * 3];
        self.cent = vec![0.0; total * 3];
        self.taabb = vec![0.0; total * 6];
        self.tri_index = (0..total as u32).collect();

        let mut gmin = [f64::INFINITY; 3];
        let mut gmax = [f64::NEG_INFINITY; 3];
        for i in 0..total {
            let p = i * 9;
            // Widen the `f32`-stored vertices to `f64` — exactly what a JS
            // read out of `this.pos` (a `Float32Array`) does automatically.
            let (ax, ay, az) = (self.pos[p] as f64, self.pos[p + 1] as f64, self.pos[p + 2] as f64);
            let (bx, by, bz) = (self.pos[p + 3] as f64, self.pos[p + 4] as f64, self.pos[p + 5] as f64);
            let (cx, cy, cz) = (self.pos[p + 6] as f64, self.pos[p + 7] as f64, self.pos[p + 8] as f64);
            let (e1x, e1y, e1z) = (bx - ax, by - ay, bz - az);
            let (e2x, e2y, e2z) = (cx - ax, cy - ay, cz - az);
            let mut nx = e1y * e2z - e1z * e2y;
            let mut ny = e1z * e2x - e1x * e2z;
            let mut nz = e1x * e2y - e1y * e2x;
            // Source: `Math.hypot(nx,ny,nz)`. `hypot` and `sqrt(sum of
            // squares)` agree to within float rounding for these
            // in-range magnitudes; the normal is `f32`-truncated on write
            // regardless (see the module doc comment), which dwarfs the
            // `hypot`-vs-`sqrt` ULP difference.
            let l = (nx * nx + ny * ny + nz * nz).sqrt();
            if l > EPS {
                nx /= l;
                ny /= l;
                nz /= l;
            } else {
                nx = 0.0;
                ny = 1.0;
                nz = 0.0;
            }
            self.nrm[i * 3] = nx as f32;
            self.nrm[i * 3 + 1] = ny as f32;
            self.nrm[i * 3 + 2] = nz as f32;

            let mnx = ax.min(bx).min(cx);
            let mny = ay.min(by).min(cy);
            let mnz = az.min(bz).min(cz);
            let mxx = ax.max(bx).max(cx);
            let mxy = ay.max(by).max(cy);
            let mxz = az.max(bz).max(cz);
            let b = i * 6;
            self.taabb[b] = mnx as f32;
            self.taabb[b + 1] = mny as f32;
            self.taabb[b + 2] = mnz as f32;
            self.taabb[b + 3] = mxx as f32;
            self.taabb[b + 4] = mxy as f32;
            self.taabb[b + 5] = mxz as f32;
            self.cent[i * 3] = ((mnx + mxx) * 0.5) as f32;
            self.cent[i * 3 + 1] = ((mny + mxy) * 0.5) as f32;
            self.cent[i * 3 + 2] = ((mnz + mxz) * 0.5) as f32;

            // The world AABB accumulates from the `f64` locals directly,
            // exactly as the source does — not from a re-read of the
            // now-`f32`-truncated `taabb`. This is why `Aabb` stays `f64`.
            gmin[0] = gmin[0].min(mnx);
            gmin[1] = gmin[1].min(mny);
            gmin[2] = gmin[2].min(mnz);
            gmax[0] = gmax[0].max(mxx);
            gmax[1] = gmax[1].max(mxy);
            gmax[2] = gmax[2].max(mxz);
        }
        self.aabb = Aabb {
            minx: gmin[0],
            miny: gmin[1],
            minz: gmin[2],
            maxx: gmax[0],
            maxy: gmax[1],
            maxz: gmax[2],
        };

        self.build_nodes(total);
        self.dirty = false;
        self.version += 1;
    }

    /// `bvh.js:251-403` (`_buildNodes`). Binned-SAH split selection.
    fn build_nodes(&mut self, total: usize) {
        let max_nodes = 2 * total + 8;
        self.node_bounds = vec![0.0; max_nodes * 6];
        self.node_meta = vec![0; max_nodes * 2];

        self.node_count = 1;
        self.node_meta[0] = 0;
        self.node_meta[1] = total as i32;
        self.node_bounds_from_range(0, 0, total);

        // Explicit stack of (nodeIndex, start, count, depth), LIFO — matches
        // the source's `stack[sp++]`/`stack[--sp]` pairs exactly. The push
        // order below (left child, then right child) means the *right* child
        // is processed first on the next iteration, exactly as in the
        // source; this determines which node index a given split's children
        // land at, which the golden captures pin, so the order is not
        // incidental.
        let mut stack: Vec<[i64; 4]> = vec![[0, 0, total as i64, 0]];

        let mut max_depth: u32 = 0;

        while let Some([node, start, count, depth]) = stack.pop() {
            let (node, start, count) = (node as usize, start as usize, count as usize);
            let depth = depth as u32;
            if depth > max_depth {
                max_depth = depth;
            }
            if count <= LEAF_SIZE || depth > 60 {
                continue;
            }

            let nb = node * 6;
            // centroid bounds
            let mut cmin = [f64::INFINITY; 3];
            let mut cmax = [f64::NEG_INFINITY; 3];
            for i in start..start + count {
                let t = self.tri_index[i] as usize * 3;
                for a in 0..3 {
                    let v = self.cent[t + a] as f64;
                    cmin[a] = cmin[a].min(v);
                    cmax[a] = cmax[a].max(v);
                }
            }
            let extent = [cmax[0] - cmin[0], cmax[1] - cmin[1], cmax[2] - cmin[2]];
            let mut axis = 0usize;
            let mut ext = extent[0];
            let mut cminv = cmin[0];
            if extent[1] > ext {
                axis = 1;
                ext = extent[1];
                cminv = cmin[1];
            }
            if extent[2] > ext {
                axis = 2;
                ext = extent[2];
                cminv = cmin[2];
            }
            if ext < 1e-7 {
                continue; // degenerate cluster -> leaf
            }

            let scale = BINS as f64 / ext;
            let mut bin_count = [0i32; BINS];
            let mut bin_b = [0f64; BINS * 6];
            for b in 0..BINS {
                let o = b * 6;
                bin_b[o] = f64::INFINITY;
                bin_b[o + 1] = f64::INFINITY;
                bin_b[o + 2] = f64::INFINITY;
                bin_b[o + 3] = f64::NEG_INFINITY;
                bin_b[o + 4] = f64::NEG_INFINITY;
                bin_b[o + 5] = f64::NEG_INFINITY;
            }
            for i in start..start + count {
                let tri = self.tri_index[i] as usize;
                let mut b = ((self.cent[tri * 3 + axis] as f64 - cminv) * scale) as i64;
                if b < 0 {
                    b = 0;
                } else if b >= BINS as i64 {
                    b = BINS as i64 - 1;
                }
                let b = b as usize;
                bin_count[b] += 1;
                let o = b * 6;
                let tb = tri * 6;
                bin_b[o] = bin_b[o].min(self.taabb[tb] as f64);
                bin_b[o + 1] = bin_b[o + 1].min(self.taabb[tb + 1] as f64);
                bin_b[o + 2] = bin_b[o + 2].min(self.taabb[tb + 2] as f64);
                bin_b[o + 3] = bin_b[o + 3].max(self.taabb[tb + 3] as f64);
                bin_b[o + 4] = bin_b[o + 4].max(self.taabb[tb + 4] as f64);
                bin_b[o + 5] = bin_b[o + 5].max(self.taabb[tb + 5] as f64);
            }

            // sweep left
            let mut amin = [f64::INFINITY; 3];
            let mut amax = [f64::NEG_INFINITY; 3];
            let mut acc = 0i32;
            let mut left_area = [0f64; BINS];
            let mut left_cnt = [0i32; BINS];
            for b in 0..BINS - 1 {
                let o = b * 6;
                if bin_count[b] > 0 {
                    amin[0] = amin[0].min(bin_b[o]);
                    amin[1] = amin[1].min(bin_b[o + 1]);
                    amin[2] = amin[2].min(bin_b[o + 2]);
                    amax[0] = amax[0].max(bin_b[o + 3]);
                    amax[1] = amax[1].max(bin_b[o + 4]);
                    amax[2] = amax[2].max(bin_b[o + 5]);
                }
                acc += bin_count[b];
                left_cnt[b] = acc;
                left_area[b] = (acc > 0)
                    .then(|| surface_area(amin[0], amin[1], amin[2], amax[0], amax[1], amax[2]))
                    .unwrap_or(0.0);
            }

            // sweep right + pick
            amin = [f64::INFINITY; 3];
            amax = [f64::NEG_INFINITY; 3];
            let mut racc = 0i32;
            let mut best_cost = TRI_COST * count as f64; // cost of making this a leaf
            let mut best_split: i32 = -1;
            let parent_area = surface_area(
                self.node_bounds[nb] as f64,
                self.node_bounds[nb + 1] as f64,
                self.node_bounds[nb + 2] as f64,
                self.node_bounds[nb + 3] as f64,
                self.node_bounds[nb + 4] as f64,
                self.node_bounds[nb + 5] as f64,
            );
            let inv_parent = if parent_area > 0.0 { 1.0 / parent_area } else { 0.0 };
            for b in (1..BINS).rev() {
                let o = b * 6;
                if bin_count[b] > 0 {
                    amin[0] = amin[0].min(bin_b[o]);
                    amin[1] = amin[1].min(bin_b[o + 1]);
                    amin[2] = amin[2].min(bin_b[o + 2]);
                    amax[0] = amax[0].max(bin_b[o + 3]);
                    amax[1] = amax[1].max(bin_b[o + 4]);
                    amax[2] = amax[2].max(bin_b[o + 5]);
                }
                racc += bin_count[b];
                let lc = left_cnt[b - 1];
                if lc == 0 || racc == 0 {
                    continue;
                }
                let rarea = surface_area(amin[0], amin[1], amin[2], amax[0], amax[1], amax[2]);
                let cost =
                    TRAV_COST + TRI_COST * inv_parent * (left_area[b - 1] * lc as f64 + rarea * racc as f64);
                if cost < best_cost {
                    best_cost = cost;
                    best_split = b as i32;
                }
            }
            if best_split < 0 {
                continue; // leaf is cheaper
            }

            // partition in place
            let split_pos = cminv + ext * (best_split as f64 / BINS as f64);
            let mut i = start as i64;
            let mut j = (start + count) as i64 - 1;
            while i <= j {
                let tri = self.tri_index[i as usize];
                if (self.cent[tri as usize * 3 + axis] as f64) < split_pos {
                    i += 1;
                } else {
                    self.tri_index.swap(i as usize, j as usize);
                    j -= 1;
                }
            }
            let left_count = (i - start as i64) as usize;
            if left_count == 0 || left_count == count {
                continue;
            }

            let l = self.node_count;
            self.node_count += 2;
            self.node_meta[node * 2] = l as i32;
            self.node_meta[node * 2 + 1] = 0;
            self.node_meta[l * 2] = start as i32;
            self.node_meta[l * 2 + 1] = left_count as i32;
            self.node_meta[(l + 1) * 2] = i as i32;
            self.node_meta[(l + 1) * 2 + 1] = (count - left_count) as i32;
            self.node_bounds_from_range(l, start, left_count);
            self.node_bounds_from_range(l + 1, i as usize, count - left_count);

            stack.push([l as i64, start as i64, left_count as i64, (depth + 1) as i64]);
            stack.push([
                (l + 1) as i64,
                i,
                (count - left_count) as i64,
                (depth + 1) as i64,
            ]);
        }
        self.max_depth = max_depth;
    }

    /// `bvh.js:405-429` (`_nodeBoundsFromRange`).
    fn node_bounds_from_range(&mut self, node: usize, start: usize, count: usize) {
        let mut mn = [f64::INFINITY; 3];
        let mut mx = [f64::NEG_INFINITY; 3];
        for i in start..start + count {
            let b = self.tri_index[i] as usize * 6;
            mn[0] = mn[0].min(self.taabb[b] as f64);
            mn[1] = mn[1].min(self.taabb[b + 1] as f64);
            mn[2] = mn[2].min(self.taabb[b + 2] as f64);
            mx[0] = mx[0].max(self.taabb[b + 3] as f64);
            mx[1] = mx[1].max(self.taabb[b + 4] as f64);
            mx[2] = mx[2].max(self.taabb[b + 5] as f64);
        }
        // Float32 storage can round a bound inwards; pad by a hair so we
        // never reject a triangle that actually straddles the plane. `p`
        // itself is not exactly representable in either width, and the
        // result is `f32`-truncated on write (see the module doc comment),
        // so this is a genuine, not merely cosmetic, divergence point from a
        // pure-`f64` computation.
        let p = 1e-5;
        let o = node * 6;
        self.node_bounds[o] = (mn[0] - p) as f32;
        self.node_bounds[o + 1] = (mn[1] - p) as f32;
        self.node_bounds[o + 2] = (mn[2] - p) as f32;
        self.node_bounds[o + 3] = (mx[0] + p) as f32;
        self.node_bounds[o + 4] = (mx[1] + p) as f32;
        self.node_bounds[o + 5] = (mx[2] + p) as f32;
    }

    /* ---------------------------------------------------------------- */
    /* Queries                                                           */
    /* ---------------------------------------------------------------- */

    /// Closest-hit ray query. `bvh.js:440-518` (`raycast`). Both faces are
    /// tested — bullet penetration needs the backface exit hit. The source
    /// defaults `ignoreObject = -1`; pass `-1` explicitly here to mean "no
    /// object ignored".
    #[allow(clippy::too_many_arguments)]
    pub fn raycast(
        &self,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        max_dist: f64,
        mask: u16,
        ignore_object: i32,
    ) -> HitRecord {
        let mut out = HitRecord::default();
        if self.node_count == 0 || self.tri_count == 0 {
            return out;
        }
        let ix = 1.0 / if dx != 0.0 { dx } else { 1e-30 };
        let iy = 1.0 / if dy != 0.0 { dy } else { 1e-30 };
        let iz = 1.0 / if dz != 0.0 { dz } else { 1e-30 };

        let mut best = max_dist;
        let mut best_tri: i32 = -1;
        let mut best_front = true;

        if self.root_ray_aabb(ox, oy, oz, ix, iy, iz, best) == f64::INFINITY {
            return out;
        }

        let mut stack: Vec<(u32, f64)> = vec![(0, 0.0)];
        while let Some((mut node, t)) = stack.pop() {
            if t >= best {
                continue;
            }
            loop {
                let count = self.node_meta[node as usize * 2 + 1];
                if count > 0 {
                    let start = self.node_meta[node as usize * 2] as usize;
                    for i in start..start + count as usize {
                        let tri = self.tri_index[i];
                        if (self.mask[tri as usize] & mask) == 0 {
                            continue;
                        }
                        if ignore_object >= 0 && self.object[tri as usize] == ignore_object {
                            continue;
                        }
                        let hit = self.ray_triangle_at(tri, ox, oy, oz, dx, dy, dz);
                        if hit.t >= 0.0 && hit.t < best {
                            best = hit.t;
                            best_tri = tri as i32;
                            best_front = hit.front_face;
                        }
                    }
                    break;
                }
                let l = self.node_meta[node as usize * 2] as u32;
                let r = l + 1;
                let tl = self.node_ray_aabb(l, ox, oy, oz, ix, iy, iz, best);
                let tr = self.node_ray_aabb(r, ox, oy, oz, ix, iy, iz, best);
                if tl == f64::INFINITY && tr == f64::INFINITY {
                    break;
                }
                if tl <= tr {
                    if tr != f64::INFINITY {
                        stack.push((r, tr));
                    }
                    node = l;
                } else {
                    if tl != f64::INFINITY {
                        stack.push((l, tl));
                    }
                    node = r;
                }
            }
        }

        if best_tri < 0 {
            return out;
        }
        self.fill_hit(&mut out, best_tri as u32, best, ox, oy, oz, dx, dy, dz);
        out.front_face = best_front;
        // Face the normal against the incoming ray so callers can always use
        // it directly for reflection / decal orientation.
        if out.nx * dx + out.ny * dy + out.nz * dz > 0.0 {
            out.nx = -out.nx;
            out.ny = -out.ny;
            out.nz = -out.nz;
        }
        out
    }

    /// `math.js`'s `rayTriangle`, called against stored (`f32`-widened)
    /// triangle `tri`.
    fn ray_triangle_at(&self, tri: u32, ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64) -> crate::physics::math::RayTriangleHit {
        let p = tri as usize * 9;
        ray_triangle(
            ox,
            oy,
            oz,
            dx,
            dy,
            dz,
            self.pos[p] as f64,
            self.pos[p + 1] as f64,
            self.pos[p + 2] as f64,
            self.pos[p + 3] as f64,
            self.pos[p + 4] as f64,
            self.pos[p + 5] as f64,
            self.pos[p + 6] as f64,
            self.pos[p + 7] as f64,
            self.pos[p + 8] as f64,
        )
    }

    fn node_bounds_f64(&self, node: u32) -> [f64; 6] {
        let o = node as usize * 6;
        [
            self.node_bounds[o] as f64,
            self.node_bounds[o + 1] as f64,
            self.node_bounds[o + 2] as f64,
            self.node_bounds[o + 3] as f64,
            self.node_bounds[o + 4] as f64,
            self.node_bounds[o + 5] as f64,
        ]
    }

    #[allow(clippy::too_many_arguments)]
    fn root_ray_aabb(&self, ox: f64, oy: f64, oz: f64, ix: f64, iy: f64, iz: f64, tmax: f64) -> f64 {
        self.node_ray_aabb(0, ox, oy, oz, ix, iy, iz, tmax)
    }

    #[allow(clippy::too_many_arguments)]
    fn node_ray_aabb(&self, node: u32, ox: f64, oy: f64, oz: f64, ix: f64, iy: f64, iz: f64, tmax: f64) -> f64 {
        let b = self.node_bounds_f64(node);
        ray_aabb(ox, oy, oz, ix, iy, iz, b[0], b[1], b[2], b[3], b[4], b[5], tmax)
    }

    /// `bvh.js:520-533` (`_fillHit`).
    #[allow(clippy::too_many_arguments)]
    fn fill_hit(&self, out: &mut HitRecord, tri: u32, t: f64, ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64) {
        out.hit = true;
        out.t = t;
        out.px = ox + dx * t;
        out.py = oy + dy * t;
        out.pz = oz + dz * t;
        out.nx = self.nrm[tri as usize * 3] as f64;
        out.ny = self.nrm[tri as usize * 3 + 1] as f64;
        out.nz = self.nrm[tri as usize * 3 + 2] as f64;
        out.tri = tri as i32;
        out.surface = self.surface[tri as usize].index();
        out.object = self.object[tri as usize];
    }

    /// Any-hit shadow/visibility ray. Cheaper: no ordering, first hit wins.
    /// `bvh.js:536-579` (`raycastAny`).
    #[allow(clippy::too_many_arguments)]
    pub fn raycast_any(&self, ox: f64, oy: f64, oz: f64, dx: f64, dy: f64, dz: f64, max_dist: f64, mask: u16) -> bool {
        if self.node_count == 0 {
            return false;
        }
        let ix = 1.0 / if dx != 0.0 { dx } else { 1e-30 };
        let iy = 1.0 / if dy != 0.0 { dy } else { 1e-30 };
        let iz = 1.0 / if dz != 0.0 { dz } else { 1e-30 };
        if self.root_ray_aabb(ox, oy, oz, ix, iy, iz, max_dist) == f64::INFINITY {
            return false;
        }
        let mut stack: Vec<u32> = vec![0];
        while let Some(node) = stack.pop() {
            let count = self.node_meta[node as usize * 2 + 1];
            if count > 0 {
                let start = self.node_meta[node as usize * 2] as usize;
                for i in start..start + count as usize {
                    let tri = self.tri_index[i];
                    if (self.mask[tri as usize] & mask) == 0 {
                        continue;
                    }
                    let hit = self.ray_triangle_at(tri, ox, oy, oz, dx, dy, dz);
                    if hit.t >= 0.0 && hit.t < max_dist {
                        return true;
                    }
                }
                continue;
            }
            let l = self.node_meta[node as usize * 2] as u32;
            let r = l + 1;
            if self.node_ray_aabb(l, ox, oy, oz, ix, iy, iz, max_dist) != f64::INFINITY {
                stack.push(l);
            }
            if self.node_ray_aabb(r, ox, oy, oz, ix, iy, iz, max_dist) != f64::INFINITY {
                stack.push(r);
            }
        }
        false
    }

    /// Gather triangle indices whose AABB overlaps the query box. `bvh.js:
    /// 582-628` (`queryAabb`). The source exposes results through shared
    /// `candidates`/`candidateCount` getters (`bvh.js:630-635`) to dodge an
    /// allocation per query; this returns an owned `Vec` instead, for the
    /// same reason every other query in this port returns its result (see
    /// the module doc comment).
    #[allow(clippy::too_many_arguments)]
    pub fn query_aabb(&self, minx: f64, miny: f64, minz: f64, maxx: f64, maxy: f64, maxz: f64, mask: u16) -> Vec<u32> {
        let mut out = Vec::new();
        if self.node_count == 0 {
            return out;
        }
        let root = self.node_bounds_f64(0);
        if root[0] > maxx || root[3] < minx || root[1] > maxy || root[4] < miny || root[2] > maxz || root[5] < minz {
            return out;
        }
        let mut stack: Vec<u32> = vec![0];
        while let Some(node) = stack.pop() {
            let count = self.node_meta[node as usize * 2 + 1];
            if count > 0 {
                let start = self.node_meta[node as usize * 2] as usize;
                for i in start..start + count as usize {
                    let tri = self.tri_index[i];
                    if (self.mask[tri as usize] & mask) == 0 {
                        continue;
                    }
                    let b = tri as usize * 6;
                    let (bminx, bminy, bminz) = (self.taabb[b] as f64, self.taabb[b + 1] as f64, self.taabb[b + 2] as f64);
                    let (bmaxx, bmaxy, bmaxz) = (self.taabb[b + 3] as f64, self.taabb[b + 4] as f64, self.taabb[b + 5] as f64);
                    if bminx > maxx || bmaxx < minx {
                        continue;
                    }
                    if bminy > maxy || bmaxy < miny {
                        continue;
                    }
                    if bminz > maxz || bmaxz < minz {
                        continue;
                    }
                    out.push(tri);
                }
                continue;
            }
            let l = self.node_meta[node as usize * 2] as u32;
            let r = l + 1;
            let lb = self.node_bounds_f64(l);
            let rb = self.node_bounds_f64(r);
            let hit_l = !(lb[0] > maxx || lb[3] < minx || lb[1] > maxy || lb[4] < miny || lb[2] > maxz || lb[5] < minz);
            let hit_r = !(rb[0] > maxx || rb[3] < minx || rb[1] > maxy || rb[4] < miny || rb[2] > maxz || rb[5] < minz);
            if hit_l {
                stack.push(l);
            }
            if hit_r {
                stack.push(r);
            }
            // The source bounds this loop with `if (sp >= stack.length - 2)
            // break;` — a safety valve against overrunning its fixed-size
            // `_stackNode` buffer. This port's stack is a growable `Vec`, so
            // that overrun cannot happen and the valve has nothing to guard.
        }
        out
    }

    /// Swept capsule against the static world. `bvh.js:637-748`
    /// (`sweepCapsule`). The capsule translates linearly; per candidate
    /// triangle, conservative advancement runs on the exact segment/triangle
    /// distance function, which is convex under linear motion — so the
    /// result is a true time of impact with no tunnelling at any speed.
    #[allow(clippy::too_many_arguments)]
    pub fn sweep_capsule(
        &self,
        p0x: f64,
        p0y: f64,
        p0z: f64,
        p1x: f64,
        p1y: f64,
        p1z: f64,
        radius: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        max_dist: f64,
        mask: u16,
    ) -> HitRecord {
        let mut out = HitRecord::default();
        if self.node_count == 0 {
            return out;
        }
        let (ex, ey, ez) = (dx * max_dist, dy * max_dist, dz * max_dist);
        let r = radius + 0.002;
        let minx = p0x.min(p1x).min(p0x + ex).min(p1x + ex) - r;
        let miny = p0y.min(p1y).min(p0y + ey).min(p1y + ey) - r;
        let minz = p0z.min(p1z).min(p0z + ez).min(p1z + ez) - r;
        let maxx = p0x.max(p1x).max(p0x + ex).max(p1x + ex) + r;
        let maxy = p0y.max(p1y).max(p0y + ey).max(p1y + ey) + r;
        let maxz = p0z.max(p1z).max(p0z + ez).max(p1z + ez) + r;
        let cand = self.query_aabb(minx, miny, minz, maxx, maxy, maxz, mask);
        if cand.is_empty() {
            return out;
        }

        let mut best = max_dist;
        let mut best_tri: i32 = -1;
        let (mut bnx, mut bny, mut bnz) = (0.0, 1.0, 0.0);
        let (mut bpx, mut bpy, mut bpz) = (0.0, 0.0, 0.0);

        for &tri in &cand {
            let p = tri as usize * 9;
            let (ax, ay, az) = (self.pos[p] as f64, self.pos[p + 1] as f64, self.pos[p + 2] as f64);
            let (bx, by, bz) = (self.pos[p + 3] as f64, self.pos[p + 4] as f64, self.pos[p + 5] as f64);
            let (cx, cy, cz) = (self.pos[p + 6] as f64, self.pos[p + 7] as f64, self.pos[p + 8] as f64);

            // Cheap plane-slab prefilter. The min signed distance over the
            // capsule axis is linear in t, so the whole sweep can be
            // rejected with two dots.
            let (tnx, tny, tnz) = (
                self.nrm[tri as usize * 3] as f64,
                self.nrm[tri as usize * 3 + 1] as f64,
                self.nrm[tri as usize * 3 + 2] as f64,
            );
            let sd_a = (p0x - ax) * tnx + (p0y - ay) * tny + (p0z - az) * tnz;
            let sd_b = (p1x - ax) * tnx + (p1y - ay) * tny + (p1z - az) * tnz;
            let vd = (dx * tnx + dy * tny + dz * tnz) * best;
            let lo = sd_a.min(sd_b) + vd.min(0.0);
            let hi = sd_a.max(sd_b) + vd.max(0.0);
            if lo > radius || hi < -radius {
                continue;
            }

            let mut t = 0.0;
            let mut hit_t = -1.0;
            for _ in 0..CA_ITERS {
                let (ox, oy, oz) = (dx * t, dy * t, dz * t);
                let cl = seg_triangle_closest(
                    p0x + ox,
                    p0y + oy,
                    p0z + oz,
                    p1x + ox,
                    p1y + oy,
                    p1z + oz,
                    ax,
                    ay,
                    az,
                    bx,
                    by,
                    bz,
                    cx,
                    cy,
                    cz,
                );
                let dist = cl.d2.sqrt() - radius;
                // separating axis: capsule axis point -> triangle point
                let mut sx = cl.bx - cl.ax;
                let mut sy = cl.by - cl.ay;
                let mut sz = cl.bz - cl.az;
                let sl = (sx * sx + sy * sy + sz * sz).sqrt();
                if sl < 1e-12 {
                    hit_t = t;
                    break; // axis passes through the face
                }
                sx /= sl;
                sy /= sl;
                sz /= sl;
                let closing = dx * sx + dy * sy + dz * sz;
                if dist <= CA_TOL {
                    // Already touching. Only a *blocking* contact counts — a
                    // capsule resting on the floor must still be able to
                    // slide along it, or the controller stalls the instant
                    // it stands on anything.
                    if closing > 1e-6 {
                        hit_t = t;
                    }
                    break;
                }
                if closing <= 1e-7 {
                    break; // convex distance is non-decreasing -> miss
                }
                let step = dist / closing;
                t += if step > 1e-7 { step } else { 1e-7 };
                if t >= best {
                    break;
                }
            }
            if hit_t < 0.0 || hit_t >= best {
                continue;
            }

            // Recover the contact normal at the impact configuration.
            let (ox, oy, oz) = (dx * hit_t, dy * hit_t, dz * hit_t);
            let cl = seg_triangle_closest(
                p0x + ox,
                p0y + oy,
                p0z + oz,
                p1x + ox,
                p1y + oy,
                p1z + oz,
                ax,
                ay,
                az,
                bx,
                by,
                bz,
                cx,
                cy,
                cz,
            );
            let mut nx = cl.ax - cl.bx;
            let mut ny = cl.ay - cl.by;
            let mut nz = cl.az - cl.bz;
            let nl = (nx * nx + ny * ny + nz * nz).sqrt();
            if nl > 1e-7 {
                nx /= nl;
                ny /= nl;
                nz /= nl;
            } else {
                nx = tnx;
                ny = tny;
                nz = tnz;
            }
            // Never return a normal we are travelling away from.
            if nx * dx + ny * dy + nz * dz > 0.0 {
                if tnx * dx + tny * dy + tnz * dz < 0.0 {
                    nx = tnx;
                    ny = tny;
                    nz = tnz;
                } else {
                    nx = -tnx;
                    ny = -tny;
                    nz = -tnz;
                }
            }
            best = hit_t;
            best_tri = tri as i32;
            bnx = nx;
            bny = ny;
            bnz = nz;
            bpx = cl.bx;
            bpy = cl.by;
            bpz = cl.bz;
        }

        if best_tri < 0 {
            return out;
        }
        out.hit = true;
        out.t = best;
        out.px = bpx;
        out.py = bpy;
        out.pz = bpz;
        out.nx = bnx;
        out.ny = bny;
        out.nz = bnz;
        out.tri = best_tri;
        out.surface = self.surface[best_tri as usize].index();
        out.object = self.object[best_tri as usize];
        out.front_face = true;
        out
    }

    /// Collect penetration contacts for a capsule at rest. `bvh.js:755-807`
    /// (`overlapCapsule`). Normals point out of the surface, towards the
    /// capsule.
    #[allow(clippy::too_many_arguments)]
    pub fn overlap_capsule(
        &self,
        p0x: f64,
        p0y: f64,
        p0z: f64,
        p1x: f64,
        p1y: f64,
        p1z: f64,
        radius: f64,
        mask: u16,
        margin: f64,
    ) -> Contacts {
        let mut cts = Contacts::default();
        if self.node_count == 0 {
            return cts;
        }
        let r = radius + margin;
        let cand = self.query_aabb(
            p0x.min(p1x) - r,
            p0y.min(p1y) - r,
            p0z.min(p1z) - r,
            p0x.max(p1x) + r,
            p0y.max(p1y) + r,
            p0z.max(p1z) + r,
            mask,
        );
        if cand.is_empty() {
            return cts;
        }
        let r2 = r * r;
        for &tri in &cand {
            if cts.tri.len() >= CONTACTS_CAPACITY {
                break;
            }
            let p = tri as usize * 9;
            let cl = seg_triangle_closest(
                p0x,
                p0y,
                p0z,
                p1x,
                p1y,
                p1z,
                self.pos[p] as f64,
                self.pos[p + 1] as f64,
                self.pos[p + 2] as f64,
                self.pos[p + 3] as f64,
                self.pos[p + 4] as f64,
                self.pos[p + 5] as f64,
                self.pos[p + 6] as f64,
                self.pos[p + 7] as f64,
                self.pos[p + 8] as f64,
            );
            if cl.d2 >= r2 {
                continue;
            }
            let d = cl.d2.sqrt();
            let (nx, ny, nz);
            let (fnx, fny, fnz) = (
                self.nrm[tri as usize * 3] as f64,
                self.nrm[tri as usize * 3 + 1] as f64,
                self.nrm[tri as usize * 3 + 2] as f64,
            );
            if d > 1e-6 {
                let mut ux = (cl.ax - cl.bx) / d;
                let mut uy = (cl.ay - cl.by) / d;
                let mut uz = (cl.az - cl.bz) / d;
                // Deep contacts can pick a normal pointing into the solid;
                // fall back to the face normal when the closest-point
                // direction disagrees with it.
                let fdot = ux * fnx + uy * fny + uz * fnz;
                if fdot < 0.05 {
                    ux = fnx;
                    uy = fny;
                    uz = fnz;
                }
                (nx, ny, nz) = (ux, uy, uz);
            } else {
                (nx, ny, nz) = (fnx, fny, fnz);
            }
            cts.nx.push(nx as f32);
            cts.ny.push(ny as f32);
            cts.nz.push(nz as f32);
            cts.px.push(cl.bx as f32);
            cts.py.push(cl.by as f32);
            cts.pz.push(cl.bz as f32);
            cts.depth.push((r - d) as f32);
            cts.s.push(cl.s as f32);
            cts.tri.push(tri as i32);
        }
        cts
    }

    /// `bvh.js:809-811` (`surfaceOf`).
    pub fn surface_of(&self, tri: u32) -> Surface {
        self.surface[tri as usize]
    }

    /// `bvh.js:817-822` (`dispose`).
    pub fn dispose(&mut self) {
        self.objects.clear();
        self.free_ids.clear();
        self.pos.clear();
        self.node_count = 0;
        self.tri_count = 0;
    }

    /* ---------------------------------------------------------------- */
    /* Accessors — not in the source (JS reads the fields directly)      */
    /* ---------------------------------------------------------------- */

    pub fn tri_count(&self) -> usize {
        self.tri_count
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }

    pub fn aabb(&self) -> Aabb {
        self.aabb
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// The 6 `[minx,miny,minz,maxx,maxy,maxz]` floats of node `i`, widened
    /// from the `f32` storage — exactly what a JS read of `world.nodeBounds`
    /// returns (see the module doc comment).
    pub fn node_bounds(&self, i: usize) -> [f64; 6] {
        self.node_bounds_f64(i as u32)
    }

    /// The `[leftFirst, count]` pair of node `i`.
    pub fn node_meta(&self, i: usize) -> [i32; 2] {
        [self.node_meta[i * 2], self.node_meta[i * 2 + 1]]
    }

    /// The three world-space vertices of triangle `tri`, widened from the
    /// `f32` storage — the JS reads `world.pos` directly for this
    /// (`atlas.js:227-235`, the decal clipper). Additive: it exposes soup the
    /// BVH already owns, so `crate::fx::decals::DecalWorld` can be bound
    /// without a second copy of the triangles.
    pub fn triangle_of(&self, tri: u32) -> [[f64; 3]; 3] {
        let b = tri as usize * 9;
        let at = |i: usize| f64::from(self.pos[b + i]);
        [
            [at(0), at(1), at(2)],
            [at(3), at(4), at(5)],
            [at(6), at(7), at(8)],
        ]
    }

    /// The triangle normal at index `tri`, widened from `f32` storage.
    pub fn normal_of(&self, tri: u32) -> [f64; 3] {
        [
            self.nrm[tri as usize * 3] as f64,
            self.nrm[tri as usize * 3 + 1] as f64,
            self.nrm[tri as usize * 3 + 2] as f64,
        ]
    }
}

/// `bvh.js:825-829` (`surfaceArea`).
fn surface_area(minx: f64, miny: f64, minz: f64, maxx: f64, maxy: f64, maxz: f64) -> f64 {
    let dx = maxx - minx;
    let dy = maxy - miny;
    let dz = maxz - minz;
    if dx < 0.0 || dy < 0.0 || dz < 0.0 {
        return 0.0;
    }
    2.0 * (dx * dy + dy * dz + dz * dx)
}
