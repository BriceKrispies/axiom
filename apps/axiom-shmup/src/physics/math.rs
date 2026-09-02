//! The collision world's geometric kernel — **the engine's**, in the call
//! shape the source uses.
//!
//! The algorithms used to live here, ported from Claude-of-Duty
//! `src/physics/math.js:1-400`. Four of them are now `axiom_math`'s
//! double-precision geometry family, promoted under the Branchless and
//! Coverage Laws:
//!
//! | this module                  | `axiom_math`                              |
//! |------------------------------|-------------------------------------------|
//! | [`ray_triangle`]             | [`DTriangle::ray_hit`]                    |
//! | [`ray_aabb`]                 | [`DAabb::ray_entry`]                      |
//! | [`closest_pt_point_triangle`]| [`DTriangle::closest_point`]              |
//! | [`closest_pt_seg_seg`]       | [`DSegment::closest_points_to`]           |
//! | [`seg_triangle_closest`]     | [`DSegment::closest_to_triangle`]         |
//!
//! ## Why they are engine, not game
//!
//! Not one of them knows what a weapon or a soldier is. Möller-Trumbore, the
//! slab test and Ericson's closest-feature solves are the leaf primitives of
//! *any* triangle-soup collision world; this game merely happened to be the
//! first thing in the repo that needed them. `axiom-physics` already offered
//! `raycast`, `overlap_capsule` and `capsule_cast` against spheres, boxes,
//! capsules, planes and heightfields — everything except a **mesh**, which is
//! the one shape a level is actually made of.
//!
//! ## The precision, and why it did not narrow
//!
//! `f64` throughout, because this is the substrate the BVH golden captures are
//! pinned against and because a broad-phase over a city-scale world is one of
//! the domains whose internal precision is load-bearing —
//! `axiom_math::Scalar` sets out the rule. At a kilometre from the origin an
//! `f32` box has about `1e-4 m` of resolution, which is the scale at which a
//! capsule resting on a floor jitters between touching and not.
//!
//! ## One deliberate behaviour change
//!
//! [`ray_aabb`] no longer misses a box that a ray grazes exactly along one of
//! its faces. The source computes `(face - origin) * (1 / direction)`, which is
//! `0 * inf` = `NaN` for an axis-parallel ray starting on that face, and a
//! comparison-based slab test then loses every comparison the `NaN` takes part
//! in and reports a miss. `DAabb::ray_entry` reads the `NaN` for what it means
//! — the origin is *on* that face, so the axis constrains nothing — and hits.
//!
//! This is a fix, not a divergence to be reverted: a grounded character's
//! downward probe slides along the floor every frame, which is exactly the
//! shape of the case. It is called out here because it is the one place the
//! promotion does not reproduce the source bit-for-bit.
//!
//! ## What stays
//!
//! [`ray_sphere`], [`ray_capsule`] and [`ray_obb`] — the analytic sweeps
//! `physics::system` uses for its *dynamic* proxies, which have no BVH behind
//! them. They are just as engine-shaped as the four above and they land with
//! the rigid-body arm of the promotion; there is no consumer for them in the
//! engine yet, and a layer export with no caller is the ceremonial surface the
//! Layer Law bans.
//!
//! Conventions, carried over unchanged from the source:
//! - Right-handed, Y-up, metres.
//! - A capsule is the Minkowski sum of a segment (p0..p1) and a sphere of
//!   radius r. p0/p1 are the *sphere centres*, not the tips.
//! - Triangle winding is CCW seen from the front face; the geometric normal is
//!   `normalize(cross(b-a, c-a))`.

use axiom_math::{DAabb, DClosestPair, DSegment, DTriangle, DVec3};

/// `math.js:17`.
pub const EPS: f64 = 1e-9;

/// `math.js:19-21`, now [`axiom_math::clamp`].
///
/// **This was `v.max(lo).min(hi)`, which is not what `math.js:20` says.** The
/// source is `v < lo ? lo : v > hi ? hi : v`, a comparison chain that passes NaN
/// through; `f64::max` is documented to *ignore* NaN and return the other
/// operand, so this pinned a NaN to `lo`. Every other faithful copy in this app
/// used the chain. Same class of defect as the `round_half_up` one, and found
/// the same way — by diffing the copies against each other.
pub use axiom_math::clamp;

/// A closest-feature record. Mirrors `makeClosest()` (`math.js:23-26`).
///
/// `s`/`t` are parameters along the query segment(s); `d2` is the squared
/// distance; `(ax,ay,az)`/`(bx,by,bz)` are the closest point pair.
///
/// **Source quirk carried forward deliberately:** in [`seg_triangle_closest`],
/// only the plane-straddle fast path ever assigns `t`. Nothing in `bvh.js` ever
/// reads `Closest::t` back out of a record produced by that function — it is
/// dead in the source — so it is left at `0.0` on every other path, which is
/// exactly what `DClosestPair::second_parameter` documents for a shape with no
/// parameter to report.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Closest {
    pub d2: f64,
    pub ax: f64,
    pub ay: f64,
    pub az: f64,
    pub bx: f64,
    pub by: f64,
    pub bz: f64,
    pub s: f64,
    pub t: f64,
}

impl Closest {
    /// Flatten the engine's pair into the source's scalar record.
    fn from_pair(pair: DClosestPair) -> Closest {
        Closest {
            d2: pair.distance_squared,
            ax: pair.on_first.x,
            ay: pair.on_first.y,
            az: pair.on_first.z,
            bx: pair.on_second.x,
            by: pair.on_second.y,
            bz: pair.on_second.z,
            s: pair.first_parameter,
            t: pair.second_parameter,
        }
    }
}

/// A raycast/sweep result record. Mirrors `makeHitRecord()` (`math.js:29-45`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRecord {
    pub hit: bool,
    pub t: f64,
    pub px: f64,
    pub py: f64,
    pub pz: f64,
    pub nx: f64,
    pub ny: f64,
    pub nz: f64,
    /// Triangle index in the owning `StaticWorld`'s flattened soup, or `-1`.
    pub tri: i32,
    /// Surface index of the hit triangle (`0` = the first `SURFACE_NAMES`
    /// entry, `concrete`, matching the source's default).
    pub surface: u8,
    /// Owning object id, or `-1`.
    pub object: i32,
    pub front_face: bool,
}

impl Default for HitRecord {
    fn default() -> Self {
        HitRecord {
            hit: false,
            t: 0.0,
            px: 0.0,
            py: 0.0,
            pz: 0.0,
            nx: 0.0,
            ny: 1.0,
            nz: 0.0,
            tri: -1,
            surface: 0,
            object: -1,
            front_face: true,
        }
    }
}

/// The result of [`ray_triangle`]: the ray parameter `t` (`-1.0` on miss) and
/// which face was struck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayTriangleHit {
    pub t: f64,
    pub front_face: bool,
}

/// Moller-Trumbore, via [`DTriangle::ray_hit`]. Does not cull backfaces —
/// penetration needs exit hits.
#[allow(clippy::too_many_arguments)]
pub fn ray_triangle(
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    ax: f64,
    ay: f64,
    az: f64,
    bx: f64,
    by: f64,
    bz: f64,
    cx: f64,
    cy: f64,
    cz: f64,
) -> RayTriangleHit {
    let triangle = DTriangle::new(
        DVec3::new(ax, ay, az),
        DVec3::new(bx, by, bz),
        DVec3::new(cx, cy, cz),
    );
    triangle
        .ray_hit(DVec3::new(ox, oy, oz), DVec3::new(dx, dy, dz))
        .map_or(
            RayTriangleHit {
                t: -1.0,
                front_face: true,
            },
            |hit| RayTriangleHit {
                t: hit.distance,
                front_face: hit.front_face,
            },
        )
}

/// Slab test against an AABB, via [`DAabb::ray_entry`]. Returns the entry
/// distance, or `f64::INFINITY` on miss. A ray starting inside returns `0.0`.
#[allow(clippy::too_many_arguments)]
pub fn ray_aabb(
    ox: f64,
    oy: f64,
    oz: f64,
    ix: f64,
    iy: f64,
    iz: f64,
    minx: f64,
    miny: f64,
    minz: f64,
    maxx: f64,
    maxy: f64,
    maxz: f64,
    tmax: f64,
) -> f64 {
    DAabb::new(
        DVec3::new(minx, miny, minz),
        DVec3::new(maxx, maxy, maxz),
    )
    .ray_entry(
        DVec3::new(ox, oy, oz),
        DVec3::new(ix, iy, iz),
        tmax,
    )
    .unwrap_or(f64::INFINITY)
}

/// Closest point on triangle `abc` to point `p`, via
/// [`DTriangle::closest_point`].
#[allow(clippy::too_many_arguments)]
pub fn closest_pt_point_triangle(
    px: f64,
    py: f64,
    pz: f64,
    ax: f64,
    ay: f64,
    az: f64,
    bx: f64,
    by: f64,
    bz: f64,
    cx: f64,
    cy: f64,
    cz: f64,
) -> (f64, f64, f64) {
    let p = DTriangle::new(
        DVec3::new(ax, ay, az),
        DVec3::new(bx, by, bz),
        DVec3::new(cx, cy, cz),
    )
    .closest_point(DVec3::new(px, py, pz));
    (p.x, p.y, p.z)
}

/// Closest points between segments `p1q1` and `p2q2`, via
/// [`DSegment::closest_points_to`].
#[allow(clippy::too_many_arguments)]
pub fn closest_pt_seg_seg(
    p1x: f64,
    p1y: f64,
    p1z: f64,
    q1x: f64,
    q1y: f64,
    q1z: f64,
    p2x: f64,
    p2y: f64,
    p2z: f64,
    q2x: f64,
    q2y: f64,
    q2z: f64,
) -> Closest {
    let first = DSegment::new(DVec3::new(p1x, p1y, p1z), DVec3::new(q1x, q1y, q1z));
    let second = DSegment::new(DVec3::new(p2x, p2y, p2z), DVec3::new(q2x, q2y, q2z));
    Closest::from_pair(first.closest_points_to(second))
}

/// Squared distance between segment `p0p1` and triangle `abc`, plus the closest
/// point pair, via [`DSegment::closest_to_triangle`].
///
/// The single most important routine in the system: capsule sweeps, capsule
/// overlap, ragdoll bone collision and rigid-body probes all reduce to it.
#[allow(clippy::too_many_arguments)]
pub fn seg_triangle_closest(
    p0x: f64,
    p0y: f64,
    p0z: f64,
    p1x: f64,
    p1y: f64,
    p1z: f64,
    ax: f64,
    ay: f64,
    az: f64,
    bx: f64,
    by: f64,
    bz: f64,
    cx: f64,
    cy: f64,
    cz: f64,
) -> Closest {
    let segment = DSegment::new(DVec3::new(p0x, p0y, p0z), DVec3::new(p1x, p1y, p1z));
    let triangle = DTriangle::new(
        DVec3::new(ax, ay, az),
        DVec3::new(bx, by, bz),
        DVec3::new(cx, cy, cz),
    );
    Closest::from_pair(segment.closest_to_triangle(triangle))
}

/// Ray vs sphere. `math.js:328-340`. Returns the entry distance or `-1.0`.
#[allow(clippy::too_many_arguments)]
pub fn ray_sphere(
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    cx: f64,
    cy: f64,
    cz: f64,
    r: f64,
    max_dist: f64,
) -> f64 {
    let mx = ox - cx;
    let my = oy - cy;
    let mz = oz - cz;
    let b = mx * dx + my * dy + mz * dz;
    let c = mx * mx + my * my + mz * mz - r * r;
    if c > 0.0 && b > 0.0 {
        return -1.0;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return -1.0;
    }
    let sq = disc.sqrt();
    let mut t = -b - sq;
    if t < 0.0 {
        t = -b + sq; // origin inside
    }
    if t < 0.0 || t > max_dist {
        return -1.0;
    }
    t
}

/// Ray vs capsule (segment `a..b`, radius `r`). `math.js:346-383`. Solved as
/// ray-vs-infinite-cylinder clipped by the two end spheres. Returns the
/// distance or `-1.0`.
#[allow(clippy::too_many_arguments)]
pub fn ray_capsule(
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    ax: f64,
    ay: f64,
    az: f64,
    bx: f64,
    by: f64,
    bz: f64,
    r: f64,
    max_dist: f64,
) -> f64 {
    let abx = bx - ax;
    let aby = by - ay;
    let abz = bz - az;
    let aox = ox - ax;
    let aoy = oy - ay;
    let aoz = oz - az;
    let abd = abx * dx + aby * dy + abz * dz;
    let abo = abx * aox + aby * aoy + abz * aoz;
    let abab = abx * abx + aby * aby + abz * abz;
    if abab < EPS {
        return ray_sphere(ox, oy, oz, dx, dy, dz, ax, ay, az, r, max_dist);
    }
    let m = abd / abab;
    let n = abo / abab;
    let qx = dx - abx * m;
    let qy = dy - aby * m;
    let qz = dz - abz * m;
    let sx = aox - abx * n;
    let sy = aoy - aby * n;
    let sz = aoz - abz * n;
    let aa = qx * qx + qy * qy + qz * qz;
    let bb = 2.0 * (qx * sx + qy * sy + qz * sz);
    let cc = sx * sx + sy * sy + sz * sz - r * r;
    let mut best = -1.0;
    if aa > EPS {
        let disc = bb * bb - 4.0 * aa * cc;
        if disc >= 0.0 {
            let sq = disc.sqrt();
            let mut t = (-bb - sq) / (2.0 * aa);
            if t < 0.0 {
                t = (-bb + sq) / (2.0 * aa);
            }
            if (0.0..=max_dist).contains(&t) {
                let k = n + t * m;
                if (0.0..=1.0).contains(&k) {
                    best = t;
                }
            }
        }
    } else if cc <= 0.0 {
        best = 0.0; // ray parallel to axis and already inside the cylinder
    }
    let t1 = ray_sphere(ox, oy, oz, dx, dy, dz, ax, ay, az, r, max_dist);
    if t1 >= 0.0 && (best < 0.0 || t1 < best) {
        best = t1;
    }
    let t2 = ray_sphere(ox, oy, oz, dx, dy, dz, bx, by, bz, r, max_dist);
    if t2 >= 0.0 && (best < 0.0 || t2 < best) {
        best = t2;
    }
    best
}

/// Ray vs oriented box. `math.js:386-400`. `inv` is the world->local matrix
/// elements, in `THREE.Matrix4.elements` (column-major) order.
#[allow(clippy::too_many_arguments)]
pub fn ray_obb(
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    inv: &[f64; 16],
    hx: f64,
    hy: f64,
    hz: f64,
    max_dist: f64,
) -> f64 {
    let lx = inv[0] * ox + inv[4] * oy + inv[8] * oz + inv[12];
    let ly = inv[1] * ox + inv[5] * oy + inv[9] * oz + inv[13];
    let lz = inv[2] * ox + inv[6] * oy + inv[10] * oz + inv[14];
    let ldx = inv[0] * dx + inv[4] * dy + inv[8] * dz;
    let ldy = inv[1] * dx + inv[5] * dy + inv[9] * dz;
    let ldz = inv[2] * dx + inv[6] * dy + inv[10] * dz;
    let inv_x = 1.0 / if ldx != 0.0 { ldx } else { 1e-30 };
    let inv_y = 1.0 / if ldy != 0.0 { ldy } else { 1e-30 };
    let inv_z = 1.0 / if ldz != 0.0 { ldz } else { 1e-30 };
    let t = ray_aabb(lx, ly, lz, inv_x, inv_y, inv_z, -hx, -hy, -hz, hx, hy, hz, max_dist);
    if t == f64::INFINITY {
        -1.0
    } else {
        t
    }
}
