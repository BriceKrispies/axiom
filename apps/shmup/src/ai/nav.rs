//! Ported from Claude-of-Duty `src/ai/nav.js:1-511` — the whole file.
//!
//! NAVIGATION is a dense walkability grid sampled straight out of the physics
//! BVH at boot: one downward ray per cell finds the floor, one upward ray
//! checks standing clearance, and the floor normal gives the slope. A* runs
//! over the 8-connected grid with a binary heap and slope/step penalties,
//! then a greedy string pull turns the staircase of cell centres into a path
//! that hugs corners.
//!
//! COVER is derived from the same grid: every walkable cell next to a
//! blocker becomes a [`CoverPoint`] with a facing direction and a height
//! class, scored at runtime against the live threat direction, the agent's
//! distance, and what the rest of the squad has claimed.
//!
//! ## The physics seam
//!
//! The source reads `phys.raycast`/`phys.raycastAny` directly off the shared
//! `PhysicsSystem`. This port names that surface as [`WorldProbe`] — the
//! narrow trait the recipe calls for when a subsystem needs something an
//! already-ported layer provides in a different shape.
//! [`crate::physics::probe::PhysicsWorld`] implements it (see that module);
//! the mask constants themselves (`phys.MASK.WORLD`/`phys.MASK.SIGHT` in the
//! source) are read directly from [`crate::physics::surfaces::mask`], exactly
//! as the source reads them off its own `phys.MASK` object.
//!
//! ## Typed-array storage, replicated
//!
//! The source stores `floor` and `gScore` as `Float32Array`, and the A* open
//! heap's priority key as a `Float32Array` too (`Heap.key`). Every other field
//! (`cell`, `radius`, cover point coordinates, ...) is a plain JS number
//! (`f64`). Following the discipline established in
//! `apps/shmup/src/physics/bvh.rs`'s module doc comment — compute in
//! `f64`, store `f32` at exactly the points the source's typed arrays would
//! truncate — [`NavGrid::floor`], [`NavGrid::g_score`] and [`Heap`]'s key
//! array are all `f32` storage here. This is not cosmetic: A*'s tie-breaking
//! (`g >= this.gScore[ni]`) and the heap's pop order both compare against
//! `f32`-truncated values in the source, so an all-`f64` port would silently
//! explore ties in a different order and could return a different (still
//! valid, but not byte-identical) path.
//!
//! ## `Math.hypot` is not `sqrt(x*x + y*y)`
//!
//! Five expressions here are `Math.hypot` in the source (`lineOfWalk`'s
//! segment length, `pick`'s threat distance, travel distance and squad
//! spacing, and `physics.lineOfSight`'s ray length). Rust's `f64::hypot` is a
//! *different* algorithm with the same intent, and `(x*x + y*y).sqrt()` is a
//! third; all three agree to within a ULP or so, and none of them agree
//! bit-for-bit with V8. Those last bits reach real decisions here:
//! `ceil(dist / (cell * 0.65))` picks a step count, `d_t` is compared against
//! hard 2.5/40 gates and then *divides* into the protection dot product, and
//! `score > best_score` decides between cover points that are frequently tied
//! by symmetry. So every `Math.hypot` site goes through
//! [`crate::jsmath::hypot2`]/[`crate::jsmath::hypot3`], the crate's single
//! V8 transcription — not a private copy, for the reason that module's doc
//! comment sets out at length.
//!
//! ## Return values, not out-parameters
//!
//! As in `physics::math`/`physics::bvh`, [`NavGrid::find_path`] returns its
//! waypoints instead of writing into a caller-supplied, reused `THREE.Vector3`
//! array (`out` in the source) — Rust has no GC-pressure reason to avoid the
//! allocation. The source's private `this._raw` (the A* parent-chain walk,
//! before string pulling) is still a genuine instance field the source reuses
//! across calls for the same reason; this port keeps it as
//! [`NavGrid::last_raw_path`] rather than dropping it, because
//! `tests/ai_nav_port.rs` pins the raw path independently of the pulled one.

use crate::jsmath::{hypot2, hypot3};
use crate::physics::bvh::Aabb;
use crate::physics::surfaces::mask;

const SQRT2: f64 = std::f64::consts::SQRT_2;

/// `phys.raycast`/`phys.raycastAny`, the two calls `nav.js` makes on the
/// shared physics system. `crate::physics::probe::PhysicsWorld` implements
/// this.
pub trait WorldProbe {
    fn raycast(&self, origin: [f64; 3], dir: [f64; 3], max_dist: f64, mask: u16) -> Option<RayHit>;
    fn raycast_any(&self, origin: [f64; 3], dir: [f64; 3], max_dist: f64, mask: u16) -> bool;
}

/// The two fields `nav.js` reads off a hit record: `point.y` and `normal.y`
/// (the only components the ray-sampling ever touches — every ray here is
/// axis-aligned).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub distance: f64,
}

/* ------------------------------------------------------------------ */
/* Binary heap for A*                                                  */
/* ------------------------------------------------------------------ */

/// `class Heap`. `nav.js:30-77`. `idx` mirrors `Int32Array`, `key` mirrors
/// `Float32Array` — see the module doc comment for why the key precision
/// matters.
#[derive(Debug, Clone)]
struct Heap {
    idx: Vec<i32>,
    key: Vec<f32>,
    n: usize,
}

impl Heap {
    fn new(cap: usize) -> Self {
        Heap {
            idx: vec![0; cap],
            key: vec![0.0; cap],
            n: 0,
        }
    }

    fn clear(&mut self) {
        self.n = 0;
    }

    /// `push(i, k)`. `nav.js:41-54`. Silently drops the push once the heap is
    /// full, exactly as the source does (`if (this.n >= this.idx.length)
    /// return;`).
    fn push(&mut self, i: i32, k: f32) {
        if self.n >= self.idx.len() {
            return;
        }
        let mut c = self.n;
        self.n += 1;
        self.idx[c] = i;
        self.key[c] = k;
        while c > 0 {
            let p = (c - 1) >> 1;
            if self.key[p] <= self.key[c] {
                break;
            }
            self.idx.swap(p, c);
            self.key.swap(p, c);
            c = p;
        }
    }

    /// `pop()`. `nav.js:56-76`.
    fn pop(&mut self) -> i32 {
        let top = self.idx[0];
        self.n -= 1;
        if self.n > 0 {
            self.idx[0] = self.idx[self.n];
            self.key[0] = self.key[self.n];
            let mut c = 0usize;
            loop {
                let l = c * 2 + 1;
                let r = l + 1;
                let mut m = c;
                if l < self.n && self.key[l] < self.key[m] {
                    m = l;
                }
                if r < self.n && self.key[r] < self.key[m] {
                    m = r;
                }
                if m == c {
                    break;
                }
                self.idx.swap(m, c);
                self.key.swap(m, c);
                c = m;
            }
        }
        top
    }
}

/* ------------------------------------------------------------------ */
/* Nav grid                                                            */
/* ------------------------------------------------------------------ */

/// `opts` defaults from `new NavGrid(physics, opts)`. `nav.js:86-91`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavGridOpts {
    pub cell: f64,
    pub radius: f64,
    pub height: f64,
    pub crouch_height: f64,
    pub max_step: f64,
    pub max_slope_deg: f64,
}

impl Default for NavGridOpts {
    fn default() -> Self {
        NavGridOpts {
            cell: 0.8,
            radius: 0.36,
            height: 1.78,
            crouch_height: 1.15,
            max_step: 0.45,
            max_slope_deg: 46.0,
        }
    }
}

/// Extra tuning `findPath` accepts (`opts.maxNodes`). `nav.js:244`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FindPathOpts {
    pub max_nodes: u32,
}

impl Default for FindPathOpts {
    fn default() -> Self {
        FindPathOpts { max_nodes: 6000 }
    }
}

/// `class NavGrid`. `nav.js:83-360`.
pub struct NavGrid {
    pub cell: f64,
    pub radius: f64,
    pub height: f64,
    pub crouch_height: f64,
    pub max_step: f64,
    /// `cos((opts.maxSlopeDeg ?? 46) * PI / 180)`. `nav.js:91`.
    pub max_slope: f64,

    pub min_x: f64,
    pub min_z: f64,
    pub nx: usize,
    pub nz: usize,
    pub top_y: f64,

    /// 0 = blocked, 1 = walkable standing, 2 = walkable crouched only.
    flags: Vec<u8>,
    /// `f32` storage — see the module doc comment.
    floor: Vec<f32>,
    /// How enclosed a cell is: 0 open, up to 4 hemmed in.
    enclosure: Vec<u8>,

    /// `f32` storage — see the module doc comment.
    g_score: Vec<f32>,
    came: Vec<i32>,
    visit_stamp: Vec<i32>,
    stamp: i32,
    open: Heap,

    /// `this._raw` — the A* parent-chain walk from the most recent
    /// [`NavGrid::find_path`], before string pulling. `nav.js:294-301`.
    last_raw: Vec<usize>,

    pub walkable_count: usize,
}

impl NavGrid {
    /// `new NavGrid(physics, opts)`. `nav.js:84-121`. `physics` is not stored
    /// here (only [`NavGrid::build`] needs it, and it is passed there
    /// directly rather than held — this port has no shared-mutable-physics
    /// story yet for a struct field to usefully hold onto).
    pub fn new(bounds: Aabb, opts: NavGridOpts) -> Self {
        let cell = opts.cell;
        let min_x = (bounds.minx / cell).floor() * cell;
        let min_z = (bounds.minz / cell).floor() * cell;
        let nx = 1.0f64.max(((bounds.maxx - min_x) / cell).ceil()) as usize;
        let nz = 1.0f64.max(((bounds.maxz - min_z) / cell).ceil()) as usize;
        let top_y = bounds.maxy + 4.0;

        let n = nx * nz;
        NavGrid {
            cell,
            radius: opts.radius,
            height: opts.height,
            crouch_height: opts.crouch_height,
            max_step: opts.max_step,
            max_slope: (opts.max_slope_deg * std::f64::consts::PI / 180.0).cos(),
            min_x,
            min_z,
            nx,
            nz,
            top_y,
            flags: vec![0; n],
            floor: vec![f32::NEG_INFINITY; n],
            enclosure: vec![0; n],
            g_score: vec![0.0; n],
            came: vec![0; n],
            visit_stamp: vec![0; n],
            stamp: 0,
            open: Heap::new(n.min(1 << 16)),
            last_raw: Vec::new(),
            walkable_count: 0,
        }
    }

    pub fn index(&self, ix: i64, iz: i64) -> usize {
        (iz * self.nx as i64 + ix) as usize
    }

    pub fn cell_x(&self, x: f64) -> i64 {
        ((x - self.min_x) / self.cell).round() as i64
    }

    pub fn cell_z(&self, z: f64) -> i64 {
        ((z - self.min_z) / self.cell).round() as i64
    }

    pub fn world_x(&self, ix: i64) -> f64 {
        self.min_x + ix as f64 * self.cell
    }

    pub fn world_z(&self, iz: i64) -> f64 {
        self.min_z + iz as f64 * self.cell
    }

    pub fn inside(&self, ix: i64, iz: i64) -> bool {
        ix >= 0 && iz >= 0 && (ix as usize) < self.nx && (iz as usize) < self.nz
    }

    /// Sample the physics world. ~2 rays per cell. `build()`. `nav.js:148-186`.
    pub fn build(&mut self, phys: &impl WorldProbe) -> &mut Self {
        let m = mask::WORLD;
        let r = self.radius;
        let mut walk = 0usize;
        for iz in 0..self.nz as i64 {
            for ix in 0..self.nx as i64 {
                let i = self.index(ix, iz);
                let x = self.world_x(ix);
                let z = self.world_z(iz);
                let Some(down) = phys.raycast([x, self.top_y, z], [0.0, -1.0, 0.0], self.top_y + 30.0, m)
                else {
                    continue;
                };
                self.floor[i] = down.point[1] as f32;
                if down.normal[1] < self.max_slope {
                    continue;
                }
                let fy = down.point[1];
                // standing clearance straight up
                let up = phys.raycast([x, fy + 0.25, z], [0.0, 1.0, 0.0], self.height - 0.2, m);
                match up {
                    None => self.flags[i] = 1,
                    Some(hit) if hit.distance > self.crouch_height - 0.25 => self.flags[i] = 2,
                    Some(_) => continue,
                }
                // shoulder clearance: four short lateral probes at chest height
                let mut blocked = 0u32;
                for d in 0..4 {
                    let dx = if d == 0 { 1.0 } else if d == 1 { -1.0 } else { 0.0 };
                    let dz = if d == 2 { 1.0 } else if d == 3 { -1.0 } else { 0.0 };
                    if phys.raycast_any([x, fy + 0.95, z], [dx, 0.0, dz], r + 0.06, m) {
                        blocked += 1;
                    }
                }
                if blocked >= 3 {
                    self.flags[i] = 0;
                    continue;
                }
                self.enclosure[i] = blocked as u8;
                walk += 1;
            }
        }
        self.walkable_count = walk;
        self
    }

    pub fn walkable(&self, ix: i64, iz: i64, crouch: bool) -> bool {
        if !self.inside(ix, iz) {
            return false;
        }
        let f = self.flags[self.index(ix, iz)];
        if crouch {
            f != 0
        } else {
            f == 1
        }
    }

    pub fn floor_at(&self, ix: i64, iz: i64) -> f64 {
        f64::from(self.floor[self.index(ix, iz)])
    }

    /// Raw `f32` floor storage, index-aligned — exposed for the golden
    /// captures and for [`super::grounding`]'s foot-contact math.
    pub fn floor(&self) -> &[f32] {
        &self.floor
    }

    pub fn flags(&self) -> &[u8] {
        &self.flags
    }

    pub fn enclosure(&self) -> &[u8] {
        &self.enclosure
    }

    /// The `f32`-precision A* accumulated cost, index-aligned. Exposed for
    /// the golden captures; see the module doc comment for why this is `f32`.
    pub fn g_score(&self) -> &[f32] {
        &self.g_score
    }

    /// `nearest(x, z, y, maxRings, yTol)`. `nav.js:203-227`. Nearest walkable
    /// cell to a world point, searched in expanding rings.
    pub fn nearest(&self, x: f64, z: f64, y: Option<f64>, max_rings: i64, y_tol: f64) -> i64 {
        let cx = self.cell_x(x);
        let cz = self.cell_z(z);
        let ok_y = |i: usize, floor: &[f32]| {
            y.is_none_or(|y| (f64::from(floor[i]) - y).abs() <= y_tol)
        };
        if self.walkable(cx, cz, true) && ok_y(self.index(cx, cz), &self.floor) {
            return self.index(cx, cz) as i64;
        }
        for ring in 1..=max_rings {
            let mut best = -1i64;
            let mut best_d = f64::INFINITY;
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dz.abs()) != ring {
                        continue;
                    }
                    let ix = cx + dx;
                    let iz = cz + dz;
                    if !self.walkable(ix, iz, true) {
                        continue;
                    }
                    let i = self.index(ix, iz);
                    if !ok_y(i, &self.floor) {
                        continue;
                    }
                    let mut d = (dx * dx + dz * dz) as f64;
                    if let Some(y) = y {
                        let fy = f64::from(self.floor[i]);
                        if fy.is_finite() {
                            d += (fy - y).powi(2) * 4.0;
                        }
                    }
                    if d < best_d {
                        best_d = d;
                        best = i as i64;
                    }
                }
            }
            if best >= 0 {
                return best;
            }
        }
        -1
    }

    /// `findPath(from, to, out, opts)`. `nav.js:233-303`. Returns the
    /// string-pulled world-space waypoints. The pre-pull A* node chain is
    /// left in [`NavGrid::last_raw_path`], mirroring the source's `this._raw`.
    pub fn find_path(&mut self, from: [f64; 3], to: [f64; 3], opts: FindPathOpts) -> Vec<[f64; 3]> {
        let start = self.nearest(from[0], from[2], Some(from[1]), 8, f64::INFINITY);
        let goal = self.nearest(to[0], to[2], Some(to[1]), 8, f64::INFINITY);
        self.last_raw.clear();
        if start < 0 || goal < 0 {
            return Vec::new();
        }
        if start == goal {
            return vec![to];
        }
        let nx = self.nx as i64;
        let gx = goal % nx;
        let gz = goal / nx;
        let cell = self.cell;
        let max_nodes = opts.max_nodes;

        self.stamp += 1;
        let stamp = self.stamp;
        self.open.clear();
        self.g_score[start as usize] = 0.0;
        self.came[start as usize] = -1;
        self.visit_stamp[start as usize] = stamp;
        self.open.push(start as i32, 0.0);

        let mut expanded = 0u32;
        let mut found = false;
        while self.open.n > 0 && expanded < max_nodes {
            let cur = self.open.pop();
            if cur as i64 == goal {
                found = true;
                break;
            }
            expanded += 1;
            let cxi = cur as i64 % nx;
            let czi = cur as i64 / nx;
            let cg = f64::from(self.g_score[cur as usize]);
            let cy = f64::from(self.floor[cur as usize]);
            for d in 0..8usize {
                let dx = DX[d];
                let dz = DZ[d];
                let ix = cxi + dx;
                let iz = czi + dz;
                if !self.walkable(ix, iz, true) {
                    continue;
                }
                if dx != 0 && dz != 0 {
                    // no corner cutting
                    if !self.walkable(cxi + dx, czi, true) || !self.walkable(cxi, czi + dz, true) {
                        continue;
                    }
                }
                let ni = self.index(ix, iz);
                let dy = f64::from(self.floor[ni]) - cy;
                if dy.abs() > self.max_step {
                    continue;
                }
                // `(dx && dz ? SQRT2 : 1) * cell`. Parenthesised: an
                // `if`-expression immediately followed by a binary operator is
                // a Rust parse hazard worth removing outright.
                let mut cost = (if dx != 0 && dz != 0 { SQRT2 } else { 1.0 }) * cell;
                cost += dy.abs() * 2.2; // prefer flat ground
                if self.flags[ni] == 2 {
                    cost += cell * 1.6; // crouch-only squeeze
                }
                cost += f64::from(self.enclosure[ni]) * cell * 0.25; // avoid scraping walls
                let g = cg + cost;
                if self.visit_stamp[ni] == stamp && g >= f64::from(self.g_score[ni]) {
                    continue;
                }
                self.visit_stamp[ni] = stamp;
                self.g_score[ni] = g as f32;
                self.came[ni] = cur;
                let hx = (ix - gx).abs() as f64;
                let hz = (iz - gz).abs() as f64;
                let h = (hx.max(hz) + (SQRT2 - 1.0) * hx.min(hz)) * cell;
                self.open.push(ni as i32, (g + h * 1.06) as f32);
            }
        }
        if !found {
            return Vec::new();
        }

        // walk the parents back, then string-pull
        let mut raw = Vec::new();
        let mut n = goal;
        while n >= 0 {
            raw.push(n as usize);
            n = self.came[n as usize] as i64;
        }
        raw.reverse();
        self.last_raw = raw.clone();
        self.string_pull(&raw, from, to)
    }

    /// The pre-string-pull A* node-index chain from the most recent
    /// [`NavGrid::find_path`] call. `this._raw`, `nav.js:294`.
    pub fn last_raw_path(&self) -> &[usize] {
        &self.last_raw
    }

    /// `_stringPull(raw, from, to, out)`. `nav.js:314-341`. Greedy string
    /// pull: keep the furthest waypoint still reachable in a straight
    /// walkable line from the anchor.
    fn string_pull(&self, raw: &[usize], from: [f64; 3], to: [f64; 3]) -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        let mut anchor = from;
        let mut i = 0usize;
        let nx = self.nx as i64;
        while i < raw.len().saturating_sub(1) {
            let mut best = i + 1;
            for j in (i + 1..raw.len()).rev() {
                let c = raw[j] as i64;
                let pos = [self.world_x(c % nx), f64::from(self.floor[raw[j]]), self.world_z(c / nx)];
                if self.line_of_walk(anchor, pos) {
                    best = j;
                    break;
                }
            }
            let c = raw[best] as i64;
            let pos = [self.world_x(c % nx), f64::from(self.floor[raw[best]]), self.world_z(c / nx)];
            out.push(pos);
            anchor = pos;
            i = best;
            if out.len() >= 32 {
                break;
            }
        }
        // finish on the exact goal if we can see it
        if self.line_of_walk(anchor, to) && out.len() < 32 {
            out.push(to);
        } else if out.is_empty() {
            out.push(to);
        }
        out
    }

    /// `lineOfWalk(a, b)`. `nav.js:344-359`. Is the straight segment walkable
    /// end to end?
    pub fn line_of_walk(&self, a: [f64; 3], b: [f64; 3]) -> bool {
        let dx = b[0] - a[0];
        let dz = b[2] - a[2];
        let dist = hypot2(dx, dz);
        let steps = 1u32.max((dist / (self.cell * 0.65)).ceil() as u32);
        let mut prev_y = a[1];
        for s in 1..=steps {
            let t = f64::from(s) / f64::from(steps);
            let x = a[0] + dx * t;
            let z = a[2] + dz * t;
            let ix = self.cell_x(x);
            let iz = self.cell_z(z);
            if !self.walkable(ix, iz, true) {
                return false;
            }
            let y = f64::from(self.floor[self.index(ix, iz)]);
            if (y - prev_y).abs() > self.max_step {
                return false;
            }
            prev_y = y;
        }
        true
    }
}

/// `nav.js:362-363`.
const DX: [i64; 8] = [1, -1, 0, 0, 1, 1, -1, -1];
const DZ: [i64; 8] = [0, 0, 1, -1, 1, -1, 1, -1];

/* ------------------------------------------------------------------ */
/* Cover                                                               */
/* ------------------------------------------------------------------ */

/// A cover point: a spot to stand plus the direction the protection comes
/// from. `high` means the blocker stops a standing shot; otherwise it is
/// crouch cover. Mirrors the plain object literal `nav.js:414-421` pushes.
///
/// **Source quirk carried forward deliberately:** `score` is initialised to
/// `0` and never written back — `CoverMap::pick`'s scoring loop keeps its
/// running score in a local, never `p.score = score`. Nothing in the source
/// ever reads a cover point's `score` field after construction; it is dead.
/// This port keeps the field (and its permanent `0.0`) rather than dropping
/// it, per the recipe's "dead computation in the source is still part of the
/// source."
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Direction the cover faces (toward the blocker).
    pub dx: f64,
    pub dz: f64,
    pub high: bool,
    pub dist: f64,
    pub claimed: i32,
    /// Always `0.0` — see the struct doc comment.
    pub score: f64,
}

/// A squad member's id/position/liveness, the only fields
/// [`CoverMap::pick`]'s bunching penalty reads off `squad`. `nav.js:465-470`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SquadMemberPos {
    pub id: i32,
    pub alive: bool,
    pub x: f64,
    pub z: f64,
}

/// `pick()`'s options (`opts`, `nav.js:436-443`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickOpts<'a> {
    pub min_range: f64,
    pub max_range: f64,
    pub id: i32,
    pub squad: Option<&'a [SquadMemberPos]>,
    pub max_travel: f64,
    pub y_ref: Option<f64>,
    pub y_tol: f64,
}

impl Default for PickOpts<'_> {
    fn default() -> Self {
        PickOpts {
            min_range: 6.0,
            max_range: 26.0,
            id: -1,
            squad: None,
            max_travel: 22.0,
            y_ref: None,
            y_tol: f64::INFINITY,
        }
    }
}

/// `build()`'s options (`opts`, `nav.js:390-391`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverBuildOpts {
    /// Sample every Nth cell.
    pub step: i64,
    pub reach: f64,
}

impl Default for CoverBuildOpts {
    fn default() -> Self {
        CoverBuildOpts { step: 1, reach: 1.25 }
    }
}

/// `class CoverMap`. `nav.js:374-510`.
pub struct CoverMap {
    pub points: Vec<CoverPoint>,
}

impl CoverMap {
    pub fn new() -> Self {
        CoverMap { points: Vec::new() }
    }

    /// `build(opts)`. `nav.js:385-428`.
    pub fn build(&mut self, grid: &NavGrid, phys: &impl WorldProbe, opts: CoverBuildOpts) -> &mut Self {
        let m = mask::WORLD;
        let step = opts.step;
        let reach = opts.reach;
        self.points.clear();
        let mut iz = 1i64;
        while iz < grid.nz as i64 - 1 {
            let mut ix = 1i64;
            while ix < grid.nx as i64 - 1 {
                if !grid.walkable(ix, iz, true) {
                    ix += step;
                    continue;
                }
                let i = grid.index(ix, iz);
                if grid.enclosure[i] == 0 {
                    // still allow cover next to a blocked cell (thin props, sandbags)
                    let mut adj = false;
                    for d in 0..4 {
                        if !grid.walkable(ix + DX[d], iz + DZ[d], true) {
                            adj = true;
                            break;
                        }
                    }
                    if !adj {
                        ix += step;
                        continue;
                    }
                }
                let x = grid.world_x(ix);
                let z = grid.world_z(iz);
                let y = f64::from(grid.floor[i]);
                // find the strongest blocking direction at chest and knee height
                for d in 0..8usize {
                    let s = if d < 4 { 1.0 } else { SQRT2 };
                    let dx = DX[d] as f64 / s;
                    let dz = DZ[d] as f64 / s;
                    let Some(low) = phys.raycast([x, y + 0.55, z], [dx, 0.0, dz], reach, m) else {
                        continue;
                    };
                    let high = phys.raycast_any([x, y + 1.32, z], [dx, 0.0, dz], reach, m);
                    self.points.push(CoverPoint {
                        x,
                        y,
                        z,
                        dx,
                        dz,
                        high,
                        dist: low.distance,
                        claimed: -1,
                        score: 0.0,
                    });
                    break;
                }
                ix += step;
            }
            iz += step;
        }
        self
    }

    /// `pick(pos, threat, opts)`. `nav.js:436-482`. Best cover for an agent at
    /// `pos` against a threat at `threat`.
    pub fn pick(&mut self, pos: [f64; 3], threat: [f64; 3], opts: PickOpts) -> Option<usize> {
        let claim_id = opts.id;
        let (tx, tz) = (threat[0], threat[2]);
        let mut best: Option<usize> = None;
        let mut best_score = f64::NEG_INFINITY;
        for (i, p) in self.points.iter().enumerate() {
            if p.claimed >= 0 && p.claimed != claim_id {
                continue;
            }
            let to_threat_x = tx - p.x;
            let to_threat_z = tz - p.z;
            let d_t = hypot2(to_threat_x, to_threat_z);
            if d_t < 2.5 || d_t > 40.0 {
                continue;
            }
            let travel = hypot2(p.x - pos[0], p.z - pos[2]);
            if travel > opts.max_travel {
                continue;
            }
            if let Some(y_ref) = opts.y_ref {
                if (p.y - y_ref).abs() > opts.y_tol {
                    continue;
                }
            }
            // protection: the blocker must be on the threat side
            let prot = (to_threat_x / d_t) * p.dx + (to_threat_z / d_t) * p.dz;
            if prot < 0.25 {
                continue;
            }
            let mut score = prot * 5.0 + if p.high { 2.2 } else { 1.0 };
            // range preference
            if d_t < opts.min_range {
                score -= (opts.min_range - d_t) * 0.55;
            } else if d_t > opts.max_range {
                score -= (d_t - opts.max_range) * 0.28;
            }
            score -= travel * 0.16;
            // do not bunch up
            if let Some(squad) = opts.squad {
                for other in squad {
                    if other.id == claim_id || !other.alive {
                        continue;
                    }
                    let d = hypot2(other.x - p.x, other.z - p.z);
                    if d < 3.2 {
                        score -= (3.2 - d) * 1.4;
                    }
                }
            }
            if score > best_score {
                best_score = score;
                best = Some(i);
            }
        }
        if let Some(i) = best {
            if claim_id >= 0 {
                for p in &mut self.points {
                    if p.claimed == claim_id {
                        p.claimed = -1;
                    }
                }
                self.points[i].claimed = claim_id;
            }
        }
        best
    }

    /// `release(claimId)`. `nav.js:484-486`.
    pub fn release(&mut self, claim_id: i32) {
        for p in &mut self.points {
            if p.claimed == claim_id {
                p.claimed = -1;
            }
        }
    }

    /// `peekOffset(cover, threat, eyeH, out)`. `nav.js:492-509`. Where to lean
    /// out from a cover point to shoot: try both sides and pick the one with
    /// line of sight from the eye to the threat. Returns `(side, position)`;
    /// `side` is `1`/`-1`/`0` matching the source's return value.
    pub fn peek_offset(&self, cover: &CoverPoint, threat: [f64; 3], eye_h: f64, phys: &dyn WorldProbe) -> (i32, [f64; 3]) {
        // lateral axis = perpendicular to the cover facing
        let lx = -cover.dz;
        let lz = cover.dx;
        for s in [1.0f64, -1.0, 0.0] {
            let px = cover.x + lx * 0.62 * s;
            let pz = cover.z + lz * 0.62 * s;
            let from = [px, cover.y + eye_h, pz];
            if line_of_sight(phys, from, threat, mask::SIGHT) {
                return (s as i32, [px, cover.y, pz]);
            }
        }
        (0, [cover.x, cover.y, cover.z])
    }
}

impl Default for CoverMap {
    fn default() -> Self {
        Self::new()
    }
}

/// `phys.lineOfSight(from, to, mask)`. `src/physics/index.js:616-623` — true
/// when nothing blocks the straight line between two points. Reimplemented
/// here directly against [`WorldProbe::raycast_any`] rather than widening the
/// trait. Used by [`CoverMap::peek_offset`] and, since perception is the same
/// primitive, by [`super::agent::Agent::sense`].
pub fn line_of_sight(phys: &dyn WorldProbe, from: [f64; 3], to: [f64; 3], mask: u16) -> bool {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    // `Math.hypot(dx, dy, dz)` in the source, not a root of the sum of
    // squares — see the module doc comment.
    let d = hypot3(dx, dy, dz);
    if d < 1e-6 {
        return true;
    }
    !phys.raycast_any(from, [dx / d, dy / d, dz / d], d - 1e-3, mask)
}
