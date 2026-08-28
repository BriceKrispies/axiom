//! Ported from Claude-of-Duty `src/physics/debug.js:1-342`.
//!
//! The collision debug view: the line soup the physics system draws when
//! `setDebugDraw(true)` is on. The source's own header:
//!
//! > One LineSegments with per-vertex colour, a fixed vertex budget and a
//! > moving draw range — no allocation, no per-frame material churn. Other
//! > subsystems flip it on with `ctx.get('physics').setDebugDraw(true)` while
//! > they are bringing geometry up, and off again for beauty shots.
//!
//! Colour key (`debug.js:11-17`): grey static collision triangles near the
//! camera; cyan BVH leaf bounds; green character capsules (red when not
//! grounded); orange rigid bodies (dim when asleep); magenta ragdoll bones;
//! yellow recent raycasts / sweeps; red contact normals.
//!
//! ## Where the port stops, and why
//!
//! `debug.js` is half vertex maths and half `THREE` plumbing. This module
//! ports the **placement and vertex maths as pure data** — the exact
//! `Float32Array` contents the source would hand a `BufferGeometry` — and
//! stops at the point where Three constructs a `Mesh`/`Material`/`Scene`
//! object. That is the same line [`crate::ai::grounding`] already drew for
//! `grounding.js`, and for the same reason: none of the engine's rendering
//! arm exists yet for this port to draw through.
//!
//! Ported (all of the arithmetic, none of it dropped):
//!
//! | source                     | here |
//! |----------------------------|------|
//! | `MAX_VERTS`, `COL`, `BOX_EDGES` | [`MAX_VERTS`], [`col`], `BOX_EDGES` |
//! | `constructor`'s buffers    | [`PhysicsDebugView::new`] |
//! | `setEnabled`               | [`PhysicsDebugView::set_enabled`] |
//! | `logRay`                   | [`PhysicsDebugView::log_ray`] |
//! | `begin`                    | [`PhysicsDebugView::begin`] |
//! | `line`/`triangle`/`box`/`obb`/`capsule` | the same five methods |
//! | `rebuild`                  | [`PhysicsDebugView::rebuild`] |
//!
//! **Not** ported, because each one *is* the Three object rather than the data
//! that feeds it — recorded here so nothing is lost, and so the future
//! rendering slice can reproduce the render state exactly:
//!
//! - `BufferGeometry` + two `BufferAttribute`s (`position`, `color`, both
//!   `DynamicDrawUsage`, both `needsUpdate = true` at the end of `rebuild`).
//!   [`PhysicsDebugView::positions`] / [`PhysicsDebugView::colors`] are those
//!   two attribute arrays; [`PhysicsDebugView::draw_count`] is the
//!   `setDrawRange(0, this._v)` count.
//! - `geometry.boundingSphere = Sphere(origin, 1e6)` — a stand-in for frustum
//!   culling that is switched off anyway.
//! - `LineBasicMaterial { vertexColors: true, transparent: true, opacity:
//!   0.85, depthWrite: false, toneMapped: false, fog: false }`.
//! - `LineSegments` with `name = 'physics:debug'`, `frustumCulled = false`,
//!   `renderOrder = 9000`, `visible = enabled`, `matrixAutoUpdate = false`,
//!   and `userData { owNoPrepass: true, owProbe: true, noCollision: true }`.
//! - `attach()` (`scene.add`) and `dispose()` (`parent.remove` + two
//!   `dispose()`s) — pure scene-graph lifecycle with no arithmetic in them.
//!
//! ## Storage width is part of the algorithm
//!
//! Every buffer in the source is a `Float32Array`: `positions`, `colors`,
//! `_corners` (the OBB corner scratch) and `rays` (the query ring, *including*
//! its per-entry TTL). JavaScript computes in `f64` and rounds on store, so
//! this port computes in `f64` and stores `f32` at exactly the same places —
//! including `rays[i + 6] -= dt`, which is a read-widen-subtract-round-store
//! and therefore accumulates f32 rounding frame over frame. Porting any of
//! these as `f64` would silently move every value.
//!
//! ## The two unported seams
//!
//! `rebuild` walks `phys.characters`, `phys.bodies.bodies`, `phys.ragdolls`
//! and `phys.colliders`. The first two are ported types
//! ([`Character`], [`RigidBody`]) and are named directly. The last two are
//! not: `physics/ragdoll.js` and the `Collider` registry in
//! `physics/index.js` are separate slices. Following the precedent set by
//! [`crate::fx::world::FxWorld`] and [`crate::ai::grounding::FootSource`],
//! each is named as a narrow trait listing exactly the fields `rebuild`
//! reads — [`RagdollBones`] and [`DebugCollider`] — so this module is
//! complete and testable today and the real types bind to it when they land.

use crate::physics::bvh::StaticWorld;
use crate::physics::character::Character;
use crate::physics::rigidbody::{RigidBody, Shape};

/// `debug.js:21`.
pub const MAX_VERTS: usize = 120_000;

/// `const COL` (`debug.js:23-34`).
///
/// Held as `f64` — the source's `COL` is an object of plain JavaScript arrays
/// (so, `f64`), and the narrowing to `f32` happens on the store into
/// `this.colors`, in [`PhysicsDebugView::line`]. Declaring these as `f32`
/// literals here would round the decimal once instead of twice, which is not
/// always the same value.
pub mod col {
    /// static collision triangles near the camera
    pub const TRI: [f64; 3] = [0.32, 0.34, 0.38];
    /// BVH leaf bounds
    pub const NODE: [f64; 3] = [0.10, 0.45, 0.55];
    /// character capsules, grounded
    pub const CHAR_GROUNDED: [f64; 3] = [0.25, 0.95, 0.35];
    /// character capsules, airborne
    pub const CHAR_AIR: [f64; 3] = [0.95, 0.35, 0.20];
    /// rigid bodies, awake
    pub const BODY: [f64; 3] = [1.0, 0.55, 0.12];
    /// rigid bodies, asleep
    pub const BODY_SLEEP: [f64; 3] = [0.35, 0.24, 0.10];
    /// ragdoll bones
    pub const RAGDOLL: [f64; 3] = [0.95, 0.25, 0.85];
    /// recent raycasts / sweeps
    pub const RAY: [f64; 3] = [0.98, 0.92, 0.25];
    /// contact normals
    pub const CONTACT: [f64; 3] = [1.0, 0.12, 0.12];
    /// dynamic collider proxies
    pub const PROXY: [f64; 3] = [0.35, 0.75, 1.0];
}

/// `const BOX_EDGES = Uint8Array.from([...])` (`debug.js:342`). Index pairs
/// into the eight corners [`PhysicsDebugView::obb`] writes.
const BOX_EDGES: [u8; 24] = [
    0, 1, 0, 2, 0, 4, 1, 3, 1, 5, 2, 3, 2, 6, 3, 7, 4, 5, 4, 6, 5, 7, 6, 7,
];

/// Capacity of the recent-query ring (`debug.js:80`, `new Float32Array(256 *
/// 7)` and the `% 256` in `logRay`).
const RAY_SLOTS: usize = 256;

/// `Math.hypot(x, y, z)` and JavaScript's `expr || 1`.
///
/// `hypot3` is **not** `(x*x + y*y + z*z).sqrt()`, and not the max-scaled form
/// either: V8 divides through by the largest magnitude *and* accumulates the
/// squares with Kahan compensation. That matters here because `capsule`'s axis
/// length divides the direction every ring vertex is built from, and `capsule`
/// leans on `|| 1` twice (`debug.js:171`, `:177`) to survive a degenerate axis
/// — where zero *and* NaN are both falsy.
///
/// Both live in [`crate::jsmath`] now. This module originally defined its own
/// correct transcription of each, as did `ai/nav.rs` and `ai/parts.rs`
/// independently — while `physics/rigidbody.rs` and `audio/spatial.rs` defined
/// incorrect ones. Consolidating them is what made that disagreement visible.
/// The alias keeps this module's call sites reading as the source does.
pub use crate::jsmath::{hypot3 as js_hypot3, or_one};

/// `THREE.Matrix4.compose(position, quaternion, scale)` (three r180,
/// `three.core.js:12302-12336`), returning the 16 elements in Three's
/// **column-major** `Matrix4.elements` order — the same order
/// [`crate::physics::math::ray_obb`] already takes, and the order
/// [`PhysicsDebugView::obb`] indexes with `e[0]/e[4]/e[8]/e[12]`.
///
/// `rebuild` calls this once, for a box rigid body (`debug.js:293`,
/// `_m.compose(b.position, b.quaternion, _one)`), so `scale` is always
/// `(1,1,1)` there; it is a parameter here because a collider's `p.matrix`
/// (the other `obb` caller) comes from an `Object3D.matrixWorld` that may
/// carry one.
pub fn compose(position: [f64; 3], quaternion: [f64; 4], scale: [f64; 3]) -> [f64; 16] {
    let (x, y, z, w) = (quaternion[0], quaternion[1], quaternion[2], quaternion[3]);
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, xy, xz) = (x * x2, x * y2, x * z2);
    let (yy, yz, zz) = (y * y2, y * z2, z * z2);
    let (wx, wy, wz) = (w * x2, w * y2, w * z2);
    let (sx, sy, sz) = (scale[0], scale[1], scale[2]);
    let mut te = [0.0_f64; 16];
    te[0] = (1.0 - (yy + zz)) * sx;
    te[1] = (xy + wz) * sx;
    te[2] = (xz - wy) * sx;
    te[3] = 0.0;

    te[4] = (xy - wz) * sy;
    te[5] = (1.0 - (xx + zz)) * sy;
    te[6] = (yz + wx) * sy;
    te[7] = 0.0;

    te[8] = (xz + wy) * sz;
    te[9] = (yz - wx) * sz;
    te[10] = (1.0 - (xx + yy)) * sz;
    te[11] = 0.0;

    te[12] = position[0];
    te[13] = position[1];
    te[14] = position[2];
    te[15] = 1.0;
    te
}

/// The four reads `rebuild` performs on a `phys.ragdolls[i]`
/// (`debug.js:298-307`), and nothing else.
///
/// `ragdoll.js` stores `px`/`py`/`pz` as `Float64Array` and `boneRadius` as a
/// `Float32Array`, so an implementer must widen the radius from its `f32`
/// storage rather than returning an unrounded authored value.
pub trait RagdollBones {
    /// `rd.boneCount`.
    fn bone_count(&self) -> usize;
    /// `rd.boneHead[i]` — the particle index at the head of bone `i`.
    fn bone_head(&self, i: usize) -> usize;
    /// `rd.boneTail[i]`.
    fn bone_tail(&self, i: usize) -> usize;
    /// `rd.boneRadius[i]`, widened from `f32` storage.
    fn bone_radius(&self, i: usize) -> f64;
    /// `[rd.px[p], rd.py[p], rd.pz[p]]`.
    fn particle(&self, p: usize) -> [f64; 3];
}

/// The six reads `rebuild` performs on a `phys.colliders[i]`
/// (`debug.js:309-316`), and nothing else. See `physics/index.js:111-166`
/// (`class Collider`) for the type that will implement it.
pub trait DebugCollider {
    /// `p.enabled`.
    fn enabled(&self) -> bool;
    /// `p.shape === 'box'`. Every other shape (`'capsule'`, `'sphere'`) takes
    /// the capsule arm, exactly as the source's `else` does.
    fn is_box(&self) -> bool;
    /// `p.matrix.elements` — Three's column-major order.
    fn matrix(&self) -> [f64; 16];
    /// `[p.hx, p.hy, p.hz]`.
    fn half_extents(&self) -> [f64; 3];
    /// `[p.ax, p.ay, p.az, p.bx, p.by, p.bz]`.
    fn segment(&self) -> [f64; 6];
    /// `p.radius`.
    fn radius(&self) -> f64;
}

/// The `phys` object [`PhysicsDebugView::rebuild`] reads, plus the `camera`
/// argument it takes. One borrowed view over the physics system's four actor
/// lists and its static world; see the module doc for why two of the four are
/// traits.
pub struct DebugScene<'a> {
    /// `phys.staticWorld`.
    pub static_world: &'a StaticWorld,
    /// `camera.position`. `None` reproduces the source's falsy-`camera` guard
    /// (`debug.js:226`), which skips the triangle pass entirely.
    pub camera: Option<[f64; 3]>,
    /// `phys.characters`.
    pub characters: &'a [Character],
    /// `phys.bodies.bodies` — i.e.
    /// [`crate::physics::rigidbody::RigidBodyWorld::bodies`].
    pub bodies: &'a [RigidBody],
    /// `phys.ragdolls`.
    pub ragdolls: &'a [&'a dyn RagdollBones],
    /// `phys.colliders`.
    pub colliders: &'a [&'a dyn DebugCollider],
}

/// `class PhysicsDebugView` (`debug.js:36-338`), minus its Three objects.
pub struct PhysicsDebugView {
    /// `this.enabled`.
    pub enabled: bool,
    /// `this.showTriangles`.
    pub show_triangles: bool,
    /// `this.showNodes`.
    pub show_nodes: bool,
    /// `this.showRays`.
    pub show_rays: bool,
    /// `this.radius` — metres of collision geometry drawn around the camera.
    pub radius: f64,

    /// `this.positions`, `MAX_VERTS * 3` `f32`s.
    positions: Vec<f32>,
    /// `this.colors`, `MAX_VERTS * 3` `f32`s.
    colors: Vec<f32>,
    /// `this._v` — the write head, in *vertices*.
    v: usize,
    /// `this._corners`, `new Float32Array(24)`. A field rather than a local
    /// because it is one in the source, and because its `f32` width rounds
    /// every OBB corner before it reaches `line`.
    corners: [f32; 24],
    /// `this.rays` — the ring of recent query lines,
    /// `[x0,y0,z0,x1,y1,z1,ttl] * 256`, all `f32`.
    rays: Vec<f32>,
    /// `this.rayHead`.
    ray_head: usize,
    /// `this.rayCount`. Written by `logRay` and never read by anything in
    /// `debug.js` (`rebuild` walks all 256 slots and tests the TTL instead) —
    /// dead in the source, carried here rather than dropped.
    pub ray_count: usize,
}

impl Default for PhysicsDebugView {
    fn default() -> Self {
        Self::new()
    }
}

impl PhysicsDebugView {
    /// `constructor(scene)` (`debug.js:37-83`), minus the Three objects. The
    /// `scene` argument exists in the source only so `attach()` can call
    /// `scene.add`, so it has no counterpart here.
    pub fn new() -> Self {
        PhysicsDebugView {
            enabled: false,
            show_triangles: true,
            show_nodes: false,
            show_rays: true,
            radius: 14.0,
            positions: vec![0.0; MAX_VERTS * 3],
            colors: vec![0.0; MAX_VERTS * 3],
            v: 0,
            corners: [0.0; 24],
            rays: vec![0.0; RAY_SLOTS * 7],
            ray_head: 0,
            ray_count: 0,
        }
    }

    /* ---------------------------------------------------------------- */
    /* The buffers a renderer reads                                      */
    /* ---------------------------------------------------------------- */

    /// The `position` attribute array, whole. Only the first
    /// `draw_count() * 3` entries are live; the rest is whatever a previous
    /// frame left, exactly as in the source (`begin()` moves the write head,
    /// it does not clear).
    pub fn positions(&self) -> &[f32] {
        &self.positions
    }

    /// The `color` attribute array, whole. Same liveness rule as
    /// [`positions`](Self::positions).
    pub fn colors(&self) -> &[f32] {
        &self.colors
    }

    /// `geometry.setDrawRange(0, this._v)` — the live vertex count.
    pub fn draw_count(&self) -> usize {
        self.v
    }

    /// The recent-query ring, `[x0,y0,z0,x1,y1,z1,ttl] * 256`.
    pub fn rays(&self) -> &[f32] {
        &self.rays
    }

    /// `this.rayHead`.
    pub fn ray_head(&self) -> usize {
        self.ray_head
    }

    /* ---------------------------------------------------------------- */
    /* Lifecycle                                                         */
    /* ---------------------------------------------------------------- */

    /// `setEnabled(on)` (`debug.js:89-93`). The source additionally sets
    /// `this.object.visible` and calls `attach()`; both are scene-graph
    /// plumbing (see the module doc) and have no counterpart here.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// `logRay(x0, y0, z0, x1, y1, z1, ttl = 1.5)` (`debug.js:96-105`).
    /// Records a query for visualisation; a no-op unless the view is enabled
    /// *and* showing rays. Rust has no default arguments, so `ttl` is
    /// explicit — the source's default is `1.5`.
    #[allow(clippy::too_many_arguments)]
    pub fn log_ray(&mut self, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, ttl: f64) {
        if !self.enabled || !self.show_rays {
            return;
        }
        let i = self.ray_head * 7;
        let r = &mut self.rays;
        r[i] = x0 as f32;
        r[i + 1] = y0 as f32;
        r[i + 2] = z0 as f32;
        r[i + 3] = x1 as f32;
        r[i + 4] = y1 as f32;
        r[i + 5] = z1 as f32;
        r[i + 6] = ttl as f32;
        self.ray_head = (self.ray_head + 1) % RAY_SLOTS;
        if self.ray_count < RAY_SLOTS {
            self.ray_count += 1;
        }
    }

    /// `begin()` (`debug.js:107-109`). Rewinds the write head; it does not
    /// clear the buffers.
    pub fn begin(&mut self) {
        self.v = 0;
    }

    /* ---------------------------------------------------------------- */
    /* Primitives                                                        */
    /* ---------------------------------------------------------------- */

    /// `line(x0, y0, z0, x1, y1, z1, c = COL.tri)` (`debug.js:111-121`).
    /// Silently drops the segment once the vertex budget is spent.
    #[allow(clippy::too_many_arguments)]
    pub fn line(&mut self, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, c: [f64; 3]) {
        if self.v + 2 > MAX_VERTS {
            return;
        }
        let mut o = self.v * 3;
        self.positions[o] = x0 as f32;
        self.positions[o + 1] = y0 as f32;
        self.positions[o + 2] = z0 as f32;
        self.colors[o] = c[0] as f32;
        self.colors[o + 1] = c[1] as f32;
        self.colors[o + 2] = c[2] as f32;
        o += 3;
        self.positions[o] = x1 as f32;
        self.positions[o + 1] = y1 as f32;
        self.positions[o + 2] = z1 as f32;
        self.colors[o] = c[0] as f32;
        self.colors[o + 1] = c[1] as f32;
        self.colors[o + 2] = c[2] as f32;
        self.v += 2;
    }

    /// `triangle(...)` (`debug.js:123-127`) — three lines, wound a→b→c→a.
    #[allow(clippy::too_many_arguments)]
    pub fn triangle(
        &mut self,
        ax: f64,
        ay: f64,
        az: f64,
        bx: f64,
        by: f64,
        bz: f64,
        cx: f64,
        cy: f64,
        cz: f64,
        c: [f64; 3],
    ) {
        self.line(ax, ay, az, bx, by, bz, c);
        self.line(bx, by, bz, cx, cy, cz, c);
        self.line(cx, cy, cz, ax, ay, az, c);
    }

    /// `box(minx, miny, minz, maxx, maxy, maxz, c)` (`debug.js:129-142`) — an
    /// axis-aligned wireframe box, twelve edges in the source's order (bottom
    /// ring, top ring, four uprights).
    ///
    /// Named `r#box` because `box` is a reserved word in Rust; the raw
    /// identifier keeps the source's name so the two files diff cleanly.
    #[allow(clippy::too_many_arguments)]
    pub fn r#box(
        &mut self,
        minx: f64,
        miny: f64,
        minz: f64,
        maxx: f64,
        maxy: f64,
        maxz: f64,
        c: [f64; 3],
    ) {
        self.line(minx, miny, minz, maxx, miny, minz, c);
        self.line(maxx, miny, minz, maxx, miny, maxz, c);
        self.line(maxx, miny, maxz, minx, miny, maxz, c);
        self.line(minx, miny, maxz, minx, miny, minz, c);
        self.line(minx, maxy, minz, maxx, maxy, minz, c);
        self.line(maxx, maxy, minz, maxx, maxy, maxz, c);
        self.line(maxx, maxy, maxz, minx, maxy, maxz, c);
        self.line(minx, maxy, maxz, minx, maxy, minz, c);
        self.line(minx, miny, minz, minx, maxy, minz, c);
        self.line(maxx, miny, minz, maxx, maxy, minz, c);
        self.line(maxx, miny, maxz, maxx, maxy, maxz, c);
        self.line(minx, miny, maxz, minx, maxy, maxz, c);
    }

    /// `obb(m, hx, hy, hz, c = COL.proxy)` (`debug.js:145-166`) — an oriented
    /// box from a `Matrix4` and half-extents.
    ///
    /// `m` is in Three's column-major `Matrix4.elements` order, which is why
    /// the point transform reads `e[0]/e[4]/e[8]/e[12]` for x and not
    /// `e[0]/e[1]/e[2]/e[3]`.
    ///
    /// The eight corners land in `this._corners`, a `Float32Array` — so each
    /// one is rounded to `f32` *before* `line` sees it, and `line` then
    /// stores that same value again. Keeping the scratch at `f32` is not
    /// cosmetic: an all-`f64` corner buffer would feed `line` a different
    /// number.
    pub fn obb(&mut self, m: &[f64; 16], hx: f64, hy: f64, hz: f64, c: [f64; 3]) {
        let e = m;
        let mut k = 0;
        for i in 0..2 {
            let x = if i != 0 { hx } else { -hx };
            for j in 0..2 {
                let y = if j != 0 { hy } else { -hy };
                for l in 0..2 {
                    let z = if l != 0 { hz } else { -hz };
                    self.corners[k] = (e[0] * x + e[4] * y + e[8] * z + e[12]) as f32;
                    k += 1;
                    self.corners[k] = (e[1] * x + e[5] * y + e[9] * z + e[13]) as f32;
                    k += 1;
                    self.corners[k] = (e[2] * x + e[6] * y + e[10] * z + e[14]) as f32;
                    k += 1;
                }
            }
        }
        // `_corners` is `Copy`; taking it by value here sidesteps the
        // simultaneous `&self.corners` / `&mut self` borrow that `line` would
        // otherwise need. The values are identical.
        let v = self.corners;
        let big_e = BOX_EDGES;
        let mut i = 0;
        while i < big_e.len() {
            let a = big_e[i] as usize * 3;
            let b = big_e[i + 1] as usize * 3;
            self.line(
                f64::from(v[a]),
                f64::from(v[a + 1]),
                f64::from(v[a + 2]),
                f64::from(v[b]),
                f64::from(v[b + 1]),
                f64::from(v[b + 2]),
                c,
            );
            i += 2;
        }
    }

    /// `capsule(ax, ay, az, bx, by, bz, r, c = COL.proxy, segments = 12)`
    /// (`debug.js:169-215`) — three great rings plus four connecting lines.
    ///
    /// Two traps live in these forty lines:
    ///
    /// - `Math.hypot(dx, dy, dz) || 1` twice, which is [`js_hypot3`] and
    ///   `or_one`, not `sqrt(x*x+y*y+z*z)` and not `.max(1.0)`.
    /// - the end-cap loop bounds, `i <= segments / 2` with `segments` a
    ///   JavaScript number. Every call site passes an even count (12, 10, 8),
    ///   but `segments / 2` is a *float* division in the source, so the bound
    ///   and the divisor are kept as `f64` here rather than as an integer
    ///   halving that would round an odd count the other way.
    #[allow(clippy::too_many_arguments)]
    pub fn capsule(
        &mut self,
        ax: f64,
        ay: f64,
        az: f64,
        bx: f64,
        by: f64,
        bz: f64,
        r: f64,
        c: [f64; 3],
        segments: usize,
    ) {
        let (mut dx, mut dy, mut dz) = (bx - ax, by - ay, bz - az);
        let l = or_one(js_hypot3(dx, dy, dz));
        dx /= l;
        dy /= l;
        dz /= l;
        // orthonormal basis
        let (mut ux, uy, mut uz) = (0.0_f64, 0.0_f64, 1.0_f64);
        if dz.abs() > 0.9 {
            ux = 1.0;
            uz = 0.0;
        }
        let (mut px, mut py, mut pz) = (
            uy * dz - uz * dy,
            uz * dx - ux * dz,
            ux * dy - uy * dx,
        );
        let pl = or_one(js_hypot3(px, py, pz));
        px /= pl;
        py /= pl;
        pz /= pl;
        let (qx, qy, qz) = (
            dy * pz - dz * py,
            dz * px - dx * pz,
            dx * py - dy * px,
        );

        // rings around each cap
        for e in 0..2 {
            let (cxp, cyp, czp) = if e == 0 { (ax, ay, az) } else { (bx, by, bz) };
            let (mut prx, mut pry, mut prz) = (0.0_f64, 0.0_f64, 0.0_f64);
            for i in 0..=segments {
                let t = (i as f64 / segments as f64) * std::f64::consts::PI * 2.0;
                let s = t.sin() * r;
                let co = t.cos() * r;
                let x = cxp + px * co + qx * s;
                let y = cyp + py * co + qy * s;
                let z = czp + pz * co + qz * s;
                if i > 0 {
                    self.line(prx, pry, prz, x, y, z, c);
                }
                prx = x;
                pry = y;
                prz = z;
            }
        }
        // side lines
        self.line(
            ax + px * r,
            ay + py * r,
            az + pz * r,
            bx + px * r,
            by + py * r,
            bz + pz * r,
            c,
        );
        self.line(
            ax - px * r,
            ay - py * r,
            az - pz * r,
            bx - px * r,
            by - py * r,
            bz - pz * r,
            c,
        );
        self.line(
            ax + qx * r,
            ay + qy * r,
            az + qz * r,
            bx + qx * r,
            by + qy * r,
            bz + qz * r,
            c,
        );
        self.line(
            ax - qx * r,
            ay - qy * r,
            az - qz * r,
            bx - qx * r,
            by - qy * r,
            bz - qz * r,
            c,
        );
        // end caps (half rings in the plane of d/p)
        let half = segments as f64 / 2.0;
        for e in 0..2 {
            let s = if e == 0 { -1.0 } else { 1.0 };
            let (cxp, cyp, czp) = if e == 0 { (ax, ay, az) } else { (bx, by, bz) };
            let (mut prx, mut pry, mut prz) = (0.0_f64, 0.0_f64, 0.0_f64);
            let mut i = 0usize;
            while (i as f64) <= half {
                let t = (i as f64 / half) * std::f64::consts::PI;
                let cc = t.cos() * r;
                let ss = t.sin() * r * s;
                let x = cxp + px * cc + dx * ss;
                let y = cyp + py * cc + dy * ss;
                let z = czp + pz * cc + dz * ss;
                if i > 0 {
                    self.line(prx, pry, prz, x, y, z, c);
                }
                prx = x;
                pry = y;
                prz = z;
                i += 1;
            }
        }
    }

    /* ---------------------------------------------------------------- */
    /* The per-frame rebuild                                             */
    /* ---------------------------------------------------------------- */

    /// `rebuild(phys, camera, dt)` (`debug.js:221-331`). Rebuilds the line
    /// buffer; called once per frame while enabled.
    ///
    /// The source finishes with `setDrawRange(0, this._v)` and two
    /// `needsUpdate = true` flags — the upload, not the maths. Here the draw
    /// range is [`draw_count`](Self::draw_count) and there is nothing to
    /// flag.
    pub fn rebuild(&mut self, scene: &DebugScene<'_>, dt: f64) {
        if !self.enabled {
            return;
        }
        self.begin();
        let w = scene.static_world;

        if self.show_triangles && w.tri_count() > 0 && scene.camera.is_some() {
            let c = scene.camera.expect("guarded above");
            let r = self.radius;
            // The source keeps the candidate list in a shared scratch array
            // and returns its length; the ported `query_aabb` returns the
            // list itself, so `n` is its length. Same triangles, same order.
            let cand = w.query_aabb(c[0] - r, c[1] - r, c[2] - r, c[0] + r, c[1] + r, c[2] + r, 0xffff);
            let n = cand.len();
            let limit = n.min(6000);
            for &tri in cand.iter().take(limit) {
                // `pos[cand[i] * 9 .. +9]`, read back out of the BVH's `f32`
                // triangle soup.
                let p = w.triangle_of(tri);
                self.triangle(
                    p[0][0], p[0][1], p[0][2], p[1][0], p[1][1], p[1][2], p[2][0], p[2][1],
                    p[2][2], col::TRI,
                );
            }
        }

        if self.show_nodes && w.node_count() > 0 {
            let limit = w.node_count().min(2000);
            for i in 0..limit {
                if w.node_meta(i)[1] == 0 {
                    continue; // interior
                }
                let nb = w.node_bounds(i);
                self.r#box(nb[0], nb[1], nb[2], nb[3], nb[4], nb[5], col::NODE);
            }
        }

        for ch in scene.characters {
            let c = if ch.grounded {
                col::CHAR_GROUNDED
            } else {
                col::CHAR_AIR
            };
            self.capsule(
                ch.position[0],
                ch.position[1] + ch.radius,
                ch.position[2],
                ch.position[0],
                ch.position[1] + ch.height - ch.radius,
                ch.position[2],
                ch.radius,
                c,
                12,
            );
            if ch.grounded {
                self.line(
                    ch.position[0],
                    ch.position[1],
                    ch.position[2],
                    ch.position[0] + ch.ground_normal[0] * 0.4,
                    ch.position[1] + ch.ground_normal[1] * 0.4,
                    ch.position[2] + ch.ground_normal[2] * 0.4,
                    col::CONTACT,
                );
            }
        }

        for b in scene.bodies {
            let c = if b.sleeping {
                col::BODY_SLEEP
            } else {
                col::BODY
            };
            match b.shape {
                Shape::Sphere => {
                    self.capsule(
                        b.position[0],
                        b.position[1] - 1e-4,
                        b.position[2],
                        b.position[0],
                        b.position[1] + 1e-4,
                        b.position[2],
                        b.radius,
                        c,
                        10,
                    );
                }
                Shape::Capsule => {
                    let q = b.quaternion;
                    // The body's local +Y axis in world space — the second
                    // column of the quaternion's rotation matrix, written out
                    // by hand exactly as the source does.
                    let ax = 2.0 * (q[0] * q[1] - q[3] * q[2]);
                    let ay = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
                    let az = 2.0 * (q[1] * q[2] + q[3] * q[0]);
                    let h = b.half_height;
                    self.capsule(
                        b.position[0] - ax * h,
                        b.position[1] - ay * h,
                        b.position[2] - az * h,
                        b.position[0] + ax * h,
                        b.position[1] + ay * h,
                        b.position[2] + az * h,
                        b.radius,
                        c,
                        10,
                    );
                }
                // The source's `else`: anything that is not a sphere or a
                // capsule draws as an oriented box.
                Shape::Box => {
                    let m = compose(b.position, b.quaternion, [1.0, 1.0, 1.0]);
                    self.obb(&m, b.hx, b.hy, b.hz, c);
                }
            }
        }

        for rd in scene.ragdolls {
            for i in 0..rd.bone_count() {
                let a = rd.bone_head(i);
                let t = rd.bone_tail(i);
                let pa = rd.particle(a);
                let pt = rd.particle(t);
                self.capsule(
                    pa[0],
                    pa[1],
                    pa[2],
                    pt[0],
                    pt[1],
                    pt[2],
                    rd.bone_radius(i),
                    col::RAGDOLL,
                    8,
                );
            }
        }

        for p in scene.colliders {
            if !p.enabled() {
                continue;
            }
            if p.is_box() {
                let h = p.half_extents();
                self.obb(&p.matrix(), h[0], h[1], h[2], col::PROXY);
            } else {
                let s = p.segment();
                self.capsule(s[0], s[1], s[2], s[3], s[4], s[5], p.radius(), col::PROXY, 8);
            }
        }

        if self.show_rays {
            for i in 0..RAY_SLOTS {
                let o = i * 7;
                if self.rays[o + 6] <= 0.0 {
                    continue;
                }
                // Read-widen-subtract-round-store: the TTL lives in a
                // `Float32Array`, so this rounds every frame.
                self.rays[o + 6] = (f64::from(self.rays[o + 6]) - dt) as f32;
                let (x0, y0, z0) = (
                    f64::from(self.rays[o]),
                    f64::from(self.rays[o + 1]),
                    f64::from(self.rays[o + 2]),
                );
                let (x1, y1, z1) = (
                    f64::from(self.rays[o + 3]),
                    f64::from(self.rays[o + 4]),
                    f64::from(self.rays[o + 5]),
                );
                self.line(x0, y0, z0, x1, y1, z1, col::RAY);
            }
        }
    }
}
