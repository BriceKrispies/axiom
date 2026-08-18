//! Ported from Claude-of-Duty `src/physics/math.js:1-400` — the whole file.
//!
//! The source's own header explains its shape: "Allocation-free geometric
//! kernel for the physics system. Every routine here takes scalar components
//! and writes into a caller-supplied 'out' record, so the hot paths (BVH
//! traversal, capsule sweeps, contact generation) never touch the allocator."
//! That out-parameter convention exists to dodge V8 GC pressure inside a
//! per-frame hot loop. Rust has no equivalent pressure for values this small —
//! [`Closest`] and [`HitRecord`] are a handful of `f64`s each, happily
//! returned by value on the stack — so every routine here returns its result
//! instead of writing through a mutable out-parameter. This is the one
//! systematic divergence from the source in this file; every site is a
//! mechanical translation otherwise (same operations, same order, same
//! constants).
//!
//! Conventions carried over unchanged from the source:
//! - Right-handed, Y-up, metres.
//! - A capsule is the Minkowski sum of a segment (p0..p1) and a sphere of
//!   radius r. p0/p1 are the *sphere centres*, not the tips.
//! - Triangle winding is CCW when seen from the front face; the geometric
//!   normal is `normalize(cross(b-a, c-a))`.
//!
//! All arithmetic is `f64` because a JavaScript number *is* an `f64`; this is
//! the geometric substrate the BVH golden captures are pinned against, so
//! narrowing to `f32` anywhere here would silently move every downstream
//! result.

/// `math.js:17`.
pub const EPS: f64 = 1e-9;

/// `math.js:19-21`.
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// A closest-feature record. Mirrors `makeClosest()` (`math.js:23-26`).
///
/// `s`/`t` are parameters along the query segment(s); `d2` is the squared
/// distance; `(ax,ay,az)`/`(bx,by,bz)` are the closest point pair.
///
/// **Source quirk carried forward deliberately:** in [`seg_triangle_closest`],
/// only the plane-straddle fast path ever assigns `t` (to `0.0`); the
/// endpoint-vs-face and edge-vs-edge branches never touch it
/// (`bvh.js` reuses one heap-allocated closest record across calls, so in the
/// source `out.t` is left holding whatever a *previous, unrelated* query wrote
/// into it). Nothing in `bvh.js` ever reads `Closest::t` back out of a record
/// produced by `seg_triangle_closest` — it is dead in the source. This port
/// returns a fresh `Closest` per call rather than mutating a shared one, so
/// there is no "previous query" to inherit from; `t` is simply left at its
/// `Default` (`0.0`) on every path that the source does not explicitly set it.
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

/// A raycast/sweep result record. Mirrors `makeHitRecord()` (`math.js:29-45`).
///
/// The source's record also carries a `body` field, always `null` from this
/// subsystem — it is written only by `rigidbody.js`, which is outside this
/// port slice (the static-world BVH has no rigid bodies). Omitted here; a
/// future rigid-body port can widen this type or wrap it when that arm lands.
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

/* ------------------------------------------------------------------ */
/* Ray primitives                                                      */
/* ------------------------------------------------------------------ */

/// The result of [`ray_triangle`]: the ray parameter `t` (`-1.0` on miss) and
/// which face was struck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayTriangleHit {
    pub t: f64,
    pub front_face: bool,
}

/// Möller–Trumbore. `math.js:56-80`. Does not cull backfaces — penetration
/// needs exit hits.
///
/// The source only writes `out.frontFace` "when out is supplied" (its
/// any-hit caller passes `null`, since it never reads the face). This port
/// always computes `front_face`; the branch it dodges was an allocation-cost
/// micro-optimisation in the source, not a behavioural difference, and
/// computing it here is a single extra comparison.
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
    let miss = RayTriangleHit {
        t: -1.0,
        front_face: true,
    };
    let e1x = bx - ax;
    let e1y = by - ay;
    let e1z = bz - az;
    let e2x = cx - ax;
    let e2y = cy - ay;
    let e2z = cz - az;
    let px = dy * e2z - dz * e2y;
    let py = dz * e2x - dx * e2z;
    let pz = dx * e2y - dy * e2x;
    let det = e1x * px + e1y * py + e1z * pz;
    if det > -1e-12 && det < 1e-12 {
        return miss; // parallel
    }
    let inv = 1.0 / det;
    let tx = ox - ax;
    let ty = oy - ay;
    let tz = oz - az;
    let u = (tx * px + ty * py + tz * pz) * inv;
    if !(-1e-6..=1.000001).contains(&u) {
        return miss;
    }
    let qx = ty * e1z - tz * e1y;
    let qy = tz * e1x - tx * e1z;
    let qz = tx * e1y - ty * e1x;
    let v = (dx * qx + dy * qy + dz * qz) * inv;
    if v < -1e-6 || u + v > 1.000001 {
        return miss;
    }
    let t = (e2x * qx + e2y * qy + e2z * qz) * inv;
    RayTriangleHit {
        t,
        front_face: det > 0.0,
    }
}

/// Slab test against an AABB using a precomputed reciprocal direction.
/// `math.js:87-110`. Returns the entry distance, or `f64::INFINITY` on miss.
/// Handles rays starting inside the box (returns `0.0`).
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
    let mut t0 = (minx - ox) * ix;
    let mut t1 = (maxx - ox) * ix;
    let mut lo = if t0 < t1 { t0 } else { t1 };
    let mut hi = if t0 < t1 { t1 } else { t0 };
    t0 = (miny - oy) * iy;
    t1 = (maxy - oy) * iy;
    let lo1 = if t0 < t1 { t0 } else { t1 };
    let hi1 = if t0 < t1 { t1 } else { t0 };
    if lo1 > lo {
        lo = lo1;
    }
    if hi1 < hi {
        hi = hi1;
    }
    t0 = (minz - oz) * iz;
    t1 = (maxz - oz) * iz;
    let lo2 = if t0 < t1 { t0 } else { t1 };
    let hi2 = if t0 < t1 { t1 } else { t0 };
    if lo2 > lo {
        lo = lo2;
    }
    if hi2 < hi {
        hi = hi2;
    }
    if hi < 0.0 || lo > hi || lo > tmax {
        return f64::INFINITY;
    }
    if lo < 0.0 {
        0.0
    } else {
        lo
    }
}

/* ------------------------------------------------------------------ */
/* Closest-feature queries                                             */
/* ------------------------------------------------------------------ */

/// Ericson, *Real-Time Collision Detection* §5.1.5. `math.js:117-169`. Returns
/// the closest point on triangle `abc` to point `p`; the source writes only
/// `out.b*` here, so this returns the point directly rather than a full
/// [`Closest`].
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
    let abx = bx - ax;
    let aby = by - ay;
    let abz = bz - az;
    let acx = cx - ax;
    let acy = cy - ay;
    let acz = cz - az;
    let apx = px - ax;
    let apy = py - ay;
    let apz = pz - az;
    let d1 = abx * apx + aby * apy + abz * apz;
    let d2 = acx * apx + acy * apy + acz * apz;
    if d1 <= 0.0 && d2 <= 0.0 {
        return (ax, ay, az);
    }
    let bpx = px - bx;
    let bpy = py - by;
    let bpz = pz - bz;
    let d3 = abx * bpx + aby * bpy + abz * bpz;
    let d4 = acx * bpx + acy * bpy + acz * bpz;
    if d3 >= 0.0 && d4 <= d3 {
        return (bx, by, bz);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (ax + abx * v, ay + aby * v, az + abz * v);
    }
    let cpx = px - cx;
    let cpy = py - cy;
    let cpz = pz - cz;
    let d5 = abx * cpx + aby * cpy + abz * cpz;
    let d6 = acx * cpx + acy * cpy + acz * cpz;
    if d6 >= 0.0 && d5 <= d6 {
        return (cx, cy, cz);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (ax + acx * w, ay + acy * w, az + acz * w);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / (d4 - d3 + (d5 - d6));
        return (bx + (cx - bx) * w, by + (cy - by) * w, bz + (cz - bz) * w);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    (
        ax + abx * v + acx * w,
        ay + aby * v + acy * w,
        az + abz * v + acz * w,
    )
}

/// Closest points between segments `p1q1` and `p2q2` (Ericson §5.1.9).
/// `math.js:175-219`. Returns a [`Closest`] with `a*` on segment 1, `b*` on
/// segment 2, `s`/`t` the two segment parameters, and `d2` the squared
/// distance.
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
    let dx1 = q1x - p1x;
    let dy1 = q1y - p1y;
    let dz1 = q1z - p1z;
    let dx2 = q2x - p2x;
    let dy2 = q2y - p2y;
    let dz2 = q2z - p2z;
    let rx = p1x - p2x;
    let ry = p1y - p2y;
    let rz = p1z - p2z;
    let a = dx1 * dx1 + dy1 * dy1 + dz1 * dz1;
    let e = dx2 * dx2 + dy2 * dy2 + dz2 * dz2;
    let f = dx2 * rx + dy2 * ry + dz2 * rz;
    let (mut s, mut t);
    if a <= EPS && e <= EPS {
        s = 0.0;
        t = 0.0;
    } else if a <= EPS {
        s = 0.0;
        t = clamp(f / e, 0.0, 1.0);
    } else {
        let c = dx1 * rx + dy1 * ry + dz1 * rz;
        if e <= EPS {
            t = 0.0;
            s = clamp(-c / a, 0.0, 1.0);
        } else {
            let b = dx1 * dx2 + dy1 * dy2 + dz1 * dz2;
            let denom = a * e - b * b;
            s = if denom != 0.0 {
                clamp((b * f - c * e) / denom, 0.0, 1.0)
            } else {
                0.0
            };
            t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = clamp(-c / a, 0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = clamp((b - c) / a, 0.0, 1.0);
            }
        }
    }
    let ax = p1x + dx1 * s;
    let ay = p1y + dy1 * s;
    let az = p1z + dz1 * s;
    let bx = p2x + dx2 * t;
    let by = p2y + dy2 * t;
    let bz = p2z + dz2 * t;
    let ex = ax - bx;
    let ey = ay - by;
    let ez = az - bz;
    Closest {
        d2: ex * ex + ey * ey + ez * ez,
        ax,
        ay,
        az,
        bx,
        by,
        bz,
        s,
        t,
    }
}

/// Squared distance between segment `p0p1` and triangle `abc`, plus the
/// closest point pair (`.a*` on the segment, `.b*` on the triangle).
/// `math.js:231-321`.
///
/// The single most important routine in the system: capsule sweeps, capsule
/// overlap, ragdoll bone collision and rigid-body probes all reduce to it.
/// Cost is ~5 sub-queries worst case, early-outs on intersection (the
/// plane-straddle fast path below).
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
    // Plane straddle test first: if the segment crosses the triangle interior
    // the distance is exactly zero and we can skip the five edge/vertex
    // sub-queries.
    let abx = bx - ax;
    let aby = by - ay;
    let abz = bz - az;
    let acx = cx - ax;
    let acy = cy - ay;
    let acz = cz - az;
    let nx = aby * acz - abz * acy;
    let ny = abz * acx - abx * acz;
    let nz = abx * acy - aby * acx;
    let d0 = nx * (p0x - ax) + ny * (p0y - ay) + nz * (p0z - az);
    let d1 = nx * (p1x - ax) + ny * (p1y - ay) + nz * (p1z - az);
    if (d0 > 0.0) != (d1 > 0.0) {
        let denom = d0 - d1;
        if denom != 0.0 {
            let u = d0 / denom;
            let ix = p0x + (p1x - p0x) * u;
            let iy = p0y + (p1y - p0y) * u;
            let iz = p0z + (p1z - p0z) * u;
            // barycentric inside test
            let vx = ix - ax;
            let vy = iy - ay;
            let vz = iz - az;
            let d00 = abx * abx + aby * aby + abz * abz;
            let d01 = abx * acx + aby * acy + abz * acz;
            let d11 = acx * acx + acy * acy + acz * acz;
            let d20 = vx * abx + vy * aby + vz * abz;
            let d21 = vx * acx + vy * acy + vz * acz;
            let den = d00 * d11 - d01 * d01;
            if den != 0.0 {
                let v = (d11 * d20 - d01 * d21) / den;
                let w = (d00 * d21 - d01 * d20) / den;
                if v >= 0.0 && w >= 0.0 && v + w <= 1.0 {
                    return Closest {
                        d2: 0.0,
                        ax: ix,
                        ay: iy,
                        az: iz,
                        bx: ix,
                        by: iy,
                        bz: iz,
                        s: u,
                        t: 0.0,
                    };
                }
            }
        }
    }

    let mut best = f64::INFINITY;
    let mut out = Closest::default();

    // segment endpoints vs triangle face
    let (tbx, tby, tbz) = closest_pt_point_triangle(p0x, p0y, p0z, ax, ay, az, bx, by, bz, cx, cy, cz);
    let mut ex = p0x - tbx;
    let mut ey = p0y - tby;
    let mut ez = p0z - tbz;
    let mut d = ex * ex + ey * ey + ez * ez;
    if d < best {
        best = d;
        out.ax = p0x;
        out.ay = p0y;
        out.az = p0z;
        out.bx = tbx;
        out.by = tby;
        out.bz = tbz;
        out.s = 0.0;
    }
    let (tbx, tby, tbz) = closest_pt_point_triangle(p1x, p1y, p1z, ax, ay, az, bx, by, bz, cx, cy, cz);
    ex = p1x - tbx;
    ey = p1y - tby;
    ez = p1z - tbz;
    d = ex * ex + ey * ey + ez * ez;
    if d < best {
        best = d;
        out.ax = p1x;
        out.ay = p1y;
        out.az = p1z;
        out.bx = tbx;
        out.by = tby;
        out.bz = tbz;
        out.s = 1.0;
    }

    // segment vs the three triangle edges
    let cl = closest_pt_seg_seg(p0x, p0y, p0z, p1x, p1y, p1z, ax, ay, az, bx, by, bz);
    d = cl.d2;
    if d < best {
        best = d;
        out.ax = cl.ax;
        out.ay = cl.ay;
        out.az = cl.az;
        out.bx = cl.bx;
        out.by = cl.by;
        out.bz = cl.bz;
        out.s = cl.s;
    }
    let cl = closest_pt_seg_seg(p0x, p0y, p0z, p1x, p1y, p1z, bx, by, bz, cx, cy, cz);
    d = cl.d2;
    if d < best {
        best = d;
        out.ax = cl.ax;
        out.ay = cl.ay;
        out.az = cl.az;
        out.bx = cl.bx;
        out.by = cl.by;
        out.bz = cl.bz;
        out.s = cl.s;
    }
    let cl = closest_pt_seg_seg(p0x, p0y, p0z, p1x, p1y, p1z, cx, cy, cz, ax, ay, az);
    d = cl.d2;
    if d < best {
        best = d;
        out.ax = cl.ax;
        out.ay = cl.ay;
        out.az = cl.az;
        out.bx = cl.bx;
        out.by = cl.by;
        out.bz = cl.bz;
        out.s = cl.s;
    }

    out.d2 = best;
    out
}

/* ------------------------------------------------------------------ */
/* Analytic sweeps used for dynamic (non-BVH) proxies                  */
/* ------------------------------------------------------------------ */

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
