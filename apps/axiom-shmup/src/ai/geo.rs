//! Ported from Claude-of-Duty `src/ai/geo.js:1-754` — the whole file.
//!
//! AI — procedural geometry toolkit for the enemy characters.
//!
//! Everything a soldier is made of is lofted from rings of 2D profiles: limbs
//! are tapered tubes along the bone chain, the plate carrier and its pouches
//! are rounded boxes (superellipse rings under a rounding envelope), the
//! helmet and the optic are revolves, the slings and straps are extruded
//! ribbons. One core routine ([`loft`]) builds all of them, which keeps the
//! vertex layout, the UV convention and the normal handling identical
//! everywhere.
//!
//! UV CONVENTION — u,v are stored in **metres of surface** (u around the ring,
//! v along the path). [`CharacterBuilder::build`] divides by the material's
//! tile size when it writes the attribute, so the same physical texel density
//! holds on a boot, a sleeve and a magazine without any per-part tuning.
//!
//! Nothing here runs per frame; it is all boot-time work.
//!
//! # Naming — three unrelated "geo" types now exist in this app
//!
//! | type | source file | shape |
//! |---|---|---|
//! | [`crate::weapons::geometry::Geo`] | `weapons/geometry.js` | `f32` `pos`/`normal`/`uv`/`index` |
//! | [`crate::world::geo::WorldGeo`] | `world/util.js` | the above plus a `color` mask |
//! | [`Mesh`] (here) | `ai/geo.js` | **`f64`** `p`/`n`/`uv`/`i`, always indexed |
//!
//! They are deliberately separate, exactly as the three source files are.
//! [`Mesh`] is not a variant of the other two: the character pipeline is `f64`
//! end to end (plain JS arrays, never a `Float32Array`) right up to
//! [`CharacterBuilder::build`], which is the *one* place the source narrows to
//! `Float32Array` — and that narrowing is load-bearing (see
//! [`CharacterGeometry`]).
//!
//! [`Mesh`] and [`Noise`] keep the source's names rather than taking an `Ai`
//! prefix. Neither actually collides in Rust: there is no other `Mesh` in this
//! crate, and [`crate::fx::noise::Noise`] is reached by a different path.
//! (That one IS a different algorithm despite the shared name — 2-D, a
//! 16-entry angular gradient table, plus a Worley cell table, and its
//! constructor draws 512 extra `float`s. The source has two classes called
//! `Noise` too; this port keeps that, rather than inventing a distinction the
//! original does not make.)
//!
//! This is app code (`apps/`), outside the Branchless Law — the code below
//! uses plain `if`/`for` wherever that is the clearest way to say what the
//! source says.

use std::collections::HashMap;

use crate::ai::rig::Rig;
use crate::jsmath;

/// The `f64` THREE `Vector3`/`Quaternion` pair this module's rings, frames and
/// warps are expressed in, re-exported so a caller that builds a `Ring` or a
/// [`warp`] closure needs one `use` and not two. They are not defined here —
/// [`crate::weapons::rig_math`] owns them, transcribed from three@0.180.
pub use crate::weapons::rig_math::{Q, V3};

/* ------------------------------------------------------------------ */
/* Deterministic gradient noise                                        */
/* ------------------------------------------------------------------ */

/// `G3` (`geo.js:25-29`) — the 12 edge-midpoint gradients of a cube.
const G3: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

/// `class Noise` (`geo.js:31-99`) — classic Ken Perlin 3-D gradient noise over
/// an rng-shuffled 256-entry permutation table.
///
/// **Not [`crate::fx::noise::Noise`]**, which ports `fx/noise.js`: that one is
/// 2-D, uses a 16-entry angular gradient table and carries a Worley cell
/// table, and its constructor draws 512 extra `float`s from the rng. These are
/// two independent implementations that share only a name in the source.
pub struct Noise {
    /// 512 = the 256-entry permutation table, doubled so `p[i & 255]` never
    /// needs a second wrap (`geo.js:41-42`).
    perm: [u8; 512],
}

impl Noise {
    /// `constructor(rng)` (`geo.js:32-43`). Consumes exactly 255 draws of
    /// [`crate::rng::Rng::int`] — the Fisher-Yates loop runs `i = 255 .. 1`
    /// inclusive, never `i = 0`.
    pub fn new(rng: &mut crate::rng::Rng) -> Self {
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        for i in (1..256usize).rev() {
            let j = rng.int(0, i as i32) as usize;
            let t = p[i];
            p[i] = p[j];
            p[j] = t;
        }
        let mut perm = [0u8; 512];
        for (i, v) in perm.iter_mut().enumerate() {
            *v = p[i & 255];
        }
        Noise { perm }
    }

    /// The permutation table, for tests that want to pin the shuffle itself.
    pub fn perm(&self) -> &[u8; 512] {
        &self.perm
    }

    /// Perlin 3D, roughly `[-1, 1]` (`geo.js:46-75`).
    pub fn n3(&self, x: f64, y: f64, z: f64) -> f64 {
        let p = &self.perm;
        let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
        // JS `fx & 255` is `ToInt32(fx) & 255`. Every call site here feeds
        // metre-scale coordinates times at most ~70, so the ToInt32 wrap at
        // 2^31 is unreachable; masking the `i64` truncation reproduces the
        // two's-complement result for every value that can occur (`-3 & 255`
        // is `253` in both languages).
        let (bx, by, bz) = (
            ((fx as i64) & 255) as usize,
            ((fy as i64) & 255) as usize,
            ((fz as i64) & 255) as usize,
        );
        let x = x - fx;
        let y = y - fy;
        let z = z - fz;
        let u = x * x * x * (x * (x * 6.0 - 15.0) + 10.0);
        let v = y * y * y * (y * (y * 6.0 - 15.0) + 10.0);
        let w = z * z * z * (z * (z * 6.0 - 15.0) + 10.0);
        let a = usize::from(p[bx]) + by;
        let b = usize::from(p[bx + 1]) + by;
        let aa = usize::from(p[a]) + bz;
        let ab = usize::from(p[a + 1]) + bz;
        let ba = usize::from(p[b]) + bz;
        let bb = usize::from(p[b + 1]) + bz;
        let g = |h: u8, dx: f64, dy: f64, dz: f64| {
            let q = G3[usize::from(h) % 12];
            q[0] * dx + q[1] * dy + q[2] * dz
        };
        let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
        lerp(
            lerp(
                lerp(g(p[aa], x, y, z), g(p[ba], x - 1.0, y, z), u),
                lerp(g(p[ab], x, y - 1.0, z), g(p[bb], x - 1.0, y - 1.0, z), u),
                v,
            ),
            lerp(
                lerp(g(p[aa + 1], x, y, z - 1.0), g(p[ba + 1], x - 1.0, y, z - 1.0), u),
                lerp(g(p[ab + 1], x, y - 1.0, z - 1.0), g(p[bb + 1], x - 1.0, y - 1.0, z - 1.0), u),
                v,
            ),
            w,
        )
    }

    /// `fbm3(x, y, z, oct = 4, lac = 2.03, gain = 0.5)` (`geo.js:77-86`) with
    /// the two defaults baked in — every call site outside this file passes
    /// exactly four arguments (34 of them across `parts.js`, `soldier.js` and
    /// `weapon.js`; verified by grep), so `lac`/`gain` are never overridden.
    /// [`Noise::fbm3_with`] keeps the full signature available.
    pub fn fbm3(&self, x: f64, y: f64, z: f64, oct: i32) -> f64 {
        self.fbm3_with(x, y, z, oct, 2.03, 0.5)
    }

    /// `fbm3` with `lac`/`gain` supplied explicitly.
    pub fn fbm3_with(&self, x: f64, y: f64, z: f64, oct: i32, lac: f64, gain: f64) -> f64 {
        let mut a = 0.5;
        let mut f = 1.0;
        let mut s = 0.0;
        let mut norm = 0.0;
        for _ in 0..oct {
            s += a * self.n3(x * f, y * f, z * f);
            norm += a;
            a *= gain;
            f *= lac;
        }
        s / norm
    }

    /// Billowed / ridged variant — good for cloth folds and rock
    /// (`geo.js:89-98`). `lac` and `gain` are hard-coded in the source
    /// (`2.07` / `0.5`), unlike [`Noise::fbm3`]'s parameterised pair.
    pub fn ridge3(&self, x: f64, y: f64, z: f64, oct: i32) -> f64 {
        let mut a = 0.5;
        let mut f = 1.0;
        let mut s = 0.0;
        let mut norm = 0.0;
        for _ in 0..oct {
            s += a * (1.0 - self.n3(x * f, y * f, z * f).abs() * 2.0);
            norm += a;
            a *= 0.5;
            f *= 2.07;
        }
        s / norm
    }
}

/* ------------------------------------------------------------------ */
/* Mesh records                                                        */
/* ------------------------------------------------------------------ */

/// A mesh under construction: flat arrays, no GPU resources
/// (`emptyMesh()`, `geo.js:106-108`).
///
/// `f64` throughout, matching the source's plain JS arrays — see the module
/// doc for why this is *not* [`crate::weapons::geometry::Geo`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// `xyz` triples, one per vertex.
    pub p: Vec<f64>,
    /// `xyz` triples, one per vertex.
    pub n: Vec<f64>,
    /// `uv` pairs, one per vertex.
    pub uv: Vec<f64>,
    /// Triangle indices, three per triangle.
    pub i: Vec<u32>,
}

/// `emptyMesh()` (`geo.js:106-108`) — the free-function spelling, which is
/// how the source exports it and how every call site reads.
pub fn empty_mesh() -> Mesh {
    Mesh::default()
}

impl Mesh {
    /// [`empty_mesh`], as an associated function.
    pub fn new() -> Self {
        Mesh::default()
    }

    /// `vcount(m)` (`geo.js:110-112`).
    pub fn vcount(&self) -> usize {
        self.p.len() / 3
    }
}

// **Why `jsmath`, not three local helpers.** `Math.hypot`, `Math.sign` and
// `Math.round` all mean something different in Rust than in JavaScript, and
// all three are load-bearing here: `weldNormals`'s bucket key is a
// `Math.round` of a scaled position, `superEllipse` multiplies a `Math.sign`
// straight into a radius, and `Math.hypot` is the divisor in every normalize
// in this file. [`crate::jsmath`] carries one V8-transcribed copy of each,
// pinned against a golden captured from Node.
//
// `Math.hypot` in particular is not academic here. An earlier draft of this
// module used the plain `sqrt(a*a + b*b + c*c)`, and six of the built
// character's normal components came out at `-6.594e-17` where the original
// produced `-6.941e-17` — the z-normals of loft-seam vertices whose true
// value is zero, where `weldNormals` sums normals that cancel almost exactly
// and a 1-ULP difference in the divisor becomes a ~5% difference in the
// residue. Switching to the real algorithm took the whole port to **zero**
// deviation against the golden.

/* ---- profiles ---- */

/// Superellipse ring in the XZ plane (`geo.js:117-129`). `n` 2 = ellipse,
/// 6+ = rounded box.
pub fn super_ellipse(rx: f64, rz: f64, n: f64, seg: usize, rot: f64) -> Vec<[f64; 2]> {
    let e = 2.0 / n;
    (0..seg)
        .map(|i| {
            let t = (i as f64 / seg as f64) * std::f64::consts::PI * 2.0 + rot;
            let c = t.cos();
            let s = t.sin();
            [rx * jsmath::sign(c) * c.abs().powf(e), rz * jsmath::sign(s) * s.abs().powf(e)]
        })
        .collect()
}

/// `ellipseProfile(rx, rz, seg = 16, rot = 0)` (`geo.js:131-133`).
pub fn ellipse_profile(rx: f64, rz: f64, seg: usize, rot: f64) -> Vec<[f64; 2]> {
    super_ellipse(rx, rz, 2.0, seg, rot)
}

/* ---- the one true builder: loft a sequence of rings ---- */

/// One ring handed to [`loft`] — `{ pts, o, s, q, y }` (`geo.js:139`).
///
/// Every `Option` here is one of the source's `??` defaults, kept as an
/// `Option` rather than pre-defaulted so the call sites read the same way the
/// JS object literals do (`{ pts, o: points[i], q: frames[i] }` leaves `s`
/// and `y` absent).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ring {
    /// `[x, z]` pairs. Every ring in one loft must have the same count.
    pub pts: Vec<[f64; 2]>,
    /// `ring.o ?? [0, 0, 0]` — ring origin, applied **after** the rotation.
    pub o: Option<[f64; 3]>,
    /// `ring.s ?? [1, 1]` — per-ring `[sx, sz]` scale on the profile.
    pub s: Option<[f64; 2]>,
    /// `ring.q` — rotation applied to the profile before the origin offset.
    pub q: Option<Q>,
    /// `ring.y ?? 0` — the profile's local Y before rotation.
    pub y: Option<f64>,
}

impl Ring {
    /// A ring with only `pts` set, matching `{ pts }`.
    pub fn new(pts: Vec<[f64; 2]>) -> Self {
        Ring { pts, ..Ring::default() }
    }

    /// `{ pts, o }`.
    pub fn at(pts: Vec<[f64; 2]>, o: [f64; 3]) -> Self {
        Ring { pts, o: Some(o), ..Ring::default() }
    }
}

/// `loft` options (`geo.js:141-143`). `into` is not a field: it is the
/// [`loft_into`] entry point, so the borrow is explicit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoftOpts {
    /// `opts.closed !== false` — ring wraps around (default `true`).
    pub closed: bool,
    /// `opts.capStart` — fan cap on the first ring, wound flipped.
    pub cap_start: bool,
    /// `opts.capEnd` — fan cap on the last ring.
    pub cap_end: bool,
}

impl Default for LoftOpts {
    fn default() -> Self {
        LoftOpts { closed: true, cap_start: false, cap_end: false }
    }
}

/// `loft(rings, opts)` (`geo.js:145-244`) into a fresh mesh.
pub fn loft(rings: &[Ring], opts: LoftOpts) -> Mesh {
    let mut out = Mesh::new();
    loft_into(&mut out, rings, opts);
    out
}

/// `loft(rings, { ...opts, into })` — appends into an existing mesh, which is
/// what `opts.into ?? emptyMesh()` (`geo.js:146`) does. `base = vcount(out)`
/// is read before anything is pushed, so the emitted indices are relative to
/// whatever was already there.
pub fn loft_into(out: &mut Mesh, rings: &[Ring], opts: LoftOpts) {
    let closed = opts.closed;
    let k = rings[0].pts.len();
    let base = out.vcount() as u32;

    // ring arc lengths for u, path lengths for v
    let mut u_arr = vec![0.0f64; k + 1];

    let mut v_len = 0.0f64;
    let mut prev_centre: Option<[f64; 3]> = None;
    let mut centres: Vec<[f64; 3]> = Vec::with_capacity(rings.len());
    // `pos.push({ arr, v })` — the transformed ring points and the path
    // length at that ring.
    let mut pos: Vec<(Vec<f64>, f64)> = Vec::with_capacity(rings.len());

    for ring in rings {
        let o = ring.o.unwrap_or([0.0, 0.0, 0.0]);
        let s = ring.s.unwrap_or([1.0, 1.0]);
        let q = ring.q.unwrap_or(Q::IDENTITY);
        let mut arr = vec![0.0f64; k * 3];
        let (mut cx, mut cy, mut cz) = (0.0, 0.0, 0.0);
        for j in 0..k {
            let pt = ring.pts[j];
            let mut v = V3::new(pt[0] * s[0], ring.y.unwrap_or(0.0), pt[1] * s[1]);
            v = q.rotate(v);
            let v = V3::new(v.x + o[0], v.y + o[1], v.z + o[2]);
            arr[j * 3] = v.x;
            arr[j * 3 + 1] = v.y;
            arr[j * 3 + 2] = v.z;
            cx += v.x;
            cy += v.y;
            cz += v.z;
        }
        cx /= k as f64;
        cy /= k as f64;
        cz /= k as f64;
        centres.push([cx, cy, cz]);
        if let Some(pc) = prev_centre {
            v_len += jsmath::hypot3(cx - pc[0], cy - pc[1], cz - pc[2]);
        }
        prev_centre = Some([cx, cy, cz]);
        pos.push((arr, v_len));
    }

    // u from the arc length of the first ring (consistent across the tube)
    {
        let a = &pos[0].0;
        u_arr[0] = 0.0;
        for j in 1..=k {
            let j0 = ((j - 1) % k) * 3;
            let j1 = (j % k) * 3;
            u_arr[j] = u_arr[j - 1] + jsmath::hypot3(a[j1] - a[j0], a[j1 + 1] - a[j0 + 1], a[j1 + 2] - a[j0 + 2]);
        }
    }

    // duplicate seam column for correct UVs
    let cols = if closed { k + 1 } else { k };
    for (arr, ring_v) in &pos {
        for c in 0..cols {
            let j = (c % k) * 3;
            out.p.push(arr[j]);
            out.p.push(arr[j + 1]);
            out.p.push(arr[j + 2]);
            out.n.extend_from_slice(&[0.0, 0.0, 0.0]);
            out.uv.push(u_arr[c]);
            out.uv.push(*ring_v);
        }
    }

    for r in 0..pos.len().saturating_sub(1) {
        for c in 0..cols.saturating_sub(1) {
            let a = base + (r * cols + c) as u32;
            let b = a + 1;
            let d = base + ((r + 1) * cols + c) as u32;
            let e = d + 1;
            out.i.extend_from_slice(&[a, d, b, b, d, e]);
        }
    }

    // caps
    let cap = |out: &mut Mesh, ring_index: usize, flip: bool| {
        let (arr, ring_v) = &pos[ring_index];
        let c = centres[ring_index];
        let c_idx = out.vcount() as u32;
        out.p.extend_from_slice(&[c[0], c[1], c[2]]);
        out.n.extend_from_slice(&[0.0, 0.0, 0.0]);
        out.uv.extend_from_slice(&[u_arr[k] * 0.5, *ring_v]);
        let start = out.vcount() as u32;
        for j in 0..k {
            out.p.extend_from_slice(&[arr[j * 3], arr[j * 3 + 1], arr[j * 3 + 2]]);
            out.n.extend_from_slice(&[0.0, 0.0, 0.0]);
            let ang = (j as f64 / k as f64) * std::f64::consts::PI * 2.0;
            out.uv.push(u_arr[k] * 0.5 + ang.cos() * 0.02);
            out.uv.push(*ring_v + ang.sin() * 0.02);
        }
        for j in 0..k {
            let a = start + j as u32;
            let b = start + ((j + 1) % k) as u32;
            if flip {
                out.i.extend_from_slice(&[c_idx, b, a]);
            } else {
                out.i.extend_from_slice(&[c_idx, a, b]);
            }
        }
    };
    if opts.cap_start {
        cap(out, 0, true);
    }
    if opts.cap_end {
        cap(out, pos.len() - 1, false);
    }
}

/// `pathFrames(points, upRef = [0, 0, 1])` (`geo.js:247-268`) — the ring
/// frames for a path of points with an up reference.
pub fn path_frames(points: &[[f64; 3]], up_ref: [f64; 3]) -> Vec<Q> {
    let mut frames = Vec::with_capacity(points.len());
    for i in 0..points.len() {
        let a = points[i.saturating_sub(1)];
        let b = points[(i + 1).min(points.len() - 1)];
        let mut dir = V3::new(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
        if dir.length_squared() < 1e-12 {
            dir = V3::new(0.0, 1.0, 0.0);
        }
        let dir = dir.normalize_or_zero();
        let mut up = V3::from_array(up_ref);
        if up.dot(dir).abs() > 0.97 {
            up = V3::new(1.0, 0.0, 0.0);
        }
        let x = dir.cross(up).normalize_or_zero();
        let z = x.cross(dir).normalize_or_zero();
        // `m.makeBasis(x, dir, z)` then `setFromRotationMatrix(m)` — the
        // matrix is never materialised; `Q::from_basis` is that exact pair.
        frames.push(Q::from_basis(x, dir, z));
    }
    frames
}

/// [`tube`] options (`geo.js:274-282`).
#[derive(Debug, Clone, PartialEq)]
pub struct TubeOpts {
    /// `opts.up ?? [0, 0, 1]` — only read when `frames` is absent.
    pub up: [f64; 3],
    /// `opts.frames` — precomputed ring frames, bypassing [`path_frames`].
    pub frames: Option<Vec<Q>>,
    /// Forwarded to [`loft`].
    pub loft: LoftOpts,
}

impl Default for TubeOpts {
    fn default() -> Self {
        TubeOpts { up: [0.0, 0.0, 1.0], frames: None, loft: LoftOpts::default() }
    }
}

/// Tapered tube along a polyline (`geo.js:274-282`). `profile(t, i)` returns a
/// ring of `[x, z]` pairs (x = side, z = front/back in the frame built from
/// the path direction).
pub fn tube(points: &[[f64; 3]], profile: impl Fn(f64, usize) -> Vec<[f64; 2]>, opts: &TubeOpts) -> Mesh {
    let mut out = Mesh::new();
    tube_into(&mut out, points, profile, opts);
    out
}

/// [`tube`] appending into an existing mesh (`opts.into`).
pub fn tube_into(out: &mut Mesh, points: &[[f64; 3]], profile: impl Fn(f64, usize) -> Vec<[f64; 2]>, opts: &TubeOpts) {
    let frames = match &opts.frames {
        Some(f) => f.clone(),
        None => path_frames(points, opts.up),
    };
    let n = points.len();
    let rings: Vec<Ring> = (0..n)
        .map(|i| Ring {
            pts: profile(i as f64 / (n - 1) as f64, i),
            o: Some(points[i]),
            q: Some(frames[i]),
            ..Ring::default()
        })
        .collect();
    loft_into(out, &rings, opts.loft);
}

/// [`revolve`] options (`geo.js:285-293`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RevolveOpts {
    /// `opts.squash ? r * opts.squash : r` — a **truthy** test, so a squash of
    /// exactly `0` falls back to `r` rather than collapsing the ring. Modelled
    /// as `Option<f64>` plus that zero check, not `unwrap_or(1.0)`.
    pub squash: Option<f64>,
    /// Forwarded to [`loft`].
    pub loft: LoftOpts,
}

/// Revolve a 2D profile `[[r, y], ...]` about +Y (`geo.js:285-293`).
pub fn revolve(profile: &[[f64; 2]], seg: usize, opts: RevolveOpts) -> Mesh {
    let rings: Vec<Ring> = profile
        .iter()
        .map(|&[r, y]| {
            let rz = match opts.squash {
                Some(s) if s != 0.0 => r * s,
                _ => r,
            };
            Ring::at(ellipse_profile(r.max(1e-4), rz.max(1e-4), seg, 0.0), [0.0, y, 0.0])
        })
        .collect();
    loft(&rings, opts.loft)
}

/// [`box_round`] options (`geo.js:299-318`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxRoundOpts {
    /// `opts.n ?? 5` — corner sharpness in plan.
    pub n: f64,
    /// `opts.seg ?? 20`.
    pub seg: usize,
    /// `opts.rows ?? 9`.
    pub rows: usize,
    /// `opts.roundY ?? 0.28` — how much the top and bottom edges tuck in.
    pub round_y: f64,
    /// `opts.ny ?? 5` — the rounding envelope's exponent.
    pub ny: f64,
    /// Forwarded to [`loft`], except that `capStart`/`capEnd` are **forced
    /// false** by the source's `{ ...opts, capStart: false, capEnd: false }`
    /// (`geo.js:317`); only `closed` survives from here.
    pub loft: LoftOpts,
}

impl Default for BoxRoundOpts {
    fn default() -> Self {
        BoxRoundOpts { n: 5.0, seg: 20, rows: 9, round_y: 0.28, ny: 5.0, loft: LoftOpts::default() }
    }
}

/// Rounded box / superellipsoid slab (`geo.js:299-318`).
pub fn box_round(hx: f64, hy: f64, hz: f64, opts: BoxRoundOpts) -> Mesh {
    let rings: Vec<Ring> = (0..opts.rows)
        .map(|r| {
            let t = r as f64 / (opts.rows - 1) as f64;
            let y = (t * 2.0 - 1.0) * hy;
            // envelope: 1 in the middle, tucks in over `roundY` of the height
            let a = 1.0f64.min(y.abs() / hy);
            let k = 1.0f64.min(0.0f64.max((a - (1.0 - opts.round_y)) / opts.round_y));
            // clamp before the fractional power: 1 - k^ny can land at -1e-16
            let env = 0.0f64.max(1.0 - k.powf(opts.ny)).powf(1.0 / opts.ny);
            let e = 0.02f64.max(env);
            Ring::at(super_ellipse(hx * e, hz * e, opts.n, opts.seg, 0.0), [0.0, y, 0.0])
        })
        .collect();
    loft(&rings, LoftOpts { cap_start: false, cap_end: false, ..opts.loft })
}

/// [`ellipsoid`] options (`geo.js:321-335`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EllipsoidOpts {
    /// `opts.seg ?? 22`.
    pub seg: usize,
    /// `opts.rows ?? 14`.
    pub rows: usize,
    /// `opts.v0 ?? 0` — 0 = south pole.
    pub v0: f64,
    /// `opts.v1 ?? 1`.
    pub v1: f64,
    /// Forwarded to [`loft`].
    pub loft: LoftOpts,
}

impl Default for EllipsoidOpts {
    fn default() -> Self {
        EllipsoidOpts { seg: 22, rows: 14, v0: 0.0, v1: 1.0, loft: LoftOpts::default() }
    }
}

/// Ellipsoid with optional latitude clamp (helmet dome, head, shoulder)
/// (`geo.js:321-335`).
pub fn ellipsoid(rx: f64, ry: f64, rz: f64, opts: EllipsoidOpts) -> Mesh {
    let rings: Vec<Ring> = (0..opts.rows)
        .map(|r| {
            let t = opts.v0 + (opts.v1 - opts.v0) * (r as f64 / (opts.rows - 1) as f64);
            let phi = t * std::f64::consts::PI;
            let y = -phi.cos() * ry;
            let s = phi.sin();
            Ring::at(ellipse_profile((rx * s).max(1e-4), (rz * s).max(1e-4), opts.seg, 0.0), [0.0, y, 0.0])
        })
        .collect();
    loft(&rings, opts.loft)
}

/// [`ribbon`] options (`geo.js:346-352`).
#[derive(Debug, Clone, PartialEq)]
pub struct RibbonOpts {
    /// `opts.upright` — run the width along `up` (a belt or helmet rim)
    /// instead of across it (a shoulder strap). See the source's note: get
    /// this wrong and the strap becomes a horizontal flange sticking out of
    /// the character.
    pub upright: bool,
    /// `opts.seg ?? 8`.
    pub seg: usize,
    /// Forwarded to [`tube`]: `up`, `frames`, and the loft options — except
    /// that `capStart`/`capEnd` are **forced true** by `{ ...opts, capStart:
    /// true, capEnd: true }` (`geo.js:351`).
    pub tube: TubeOpts,
}

impl Default for RibbonOpts {
    fn default() -> Self {
        RibbonOpts { upright: false, seg: 8, tube: TubeOpts::default() }
    }
}

/// Flat ribbon (strap, sling, belt) extruded along a polyline
/// (`geo.js:346-352`).
pub fn ribbon(points: &[[f64; 3]], width: f64, thick: f64, opts: &RibbonOpts) -> Mesh {
    let half = width * 0.5;
    let ht = thick * 0.5;
    let pts = if opts.upright {
        super_ellipse(ht, half, 4.0, opts.seg, 0.0)
    } else {
        super_ellipse(half, ht, 4.0, opts.seg, 0.0)
    };
    let tube_opts = TubeOpts {
        up: opts.tube.up,
        frames: opts.tube.frames.clone(),
        loft: LoftOpts { cap_start: true, cap_end: true, ..opts.tube.loft },
    };
    tube(points, |_t, _i| pts.clone(), &tube_opts)
}

/* ------------------------------------------------------------------ */
/* Mesh ops                                                            */
/* ------------------------------------------------------------------ */

/// `computeNormals(m)` (`geo.js:358-379`) at the source's `from = 0` default —
/// which is what every call site in `parts.js`, `soldier.js` and `weapon.js`
/// uses. Rust has no default arguments, so the parameterised form is
/// [`compute_normals_from`].
pub fn compute_normals(m: &mut Mesh) {
    compute_normals_from(m, 0);
}

/// `computeNormals(m, from)` with an explicit `from` (`geo.js:358-379`).
///
/// **Two source shapes preserved verbatim.** First, a triangle is skipped only
/// when **all three** of its corners sit below `from` (`geo.js:363`) — a
/// triangle straddling the boundary still accumulates into the below-`from`
/// vertices, which were *not* zeroed and are *not* renormalized afterwards, so
/// their normals drift off unit length. Second, the final normalize divides by
/// `hypot(...) || 1`, so a zero-accumulation vertex stays zero instead of
/// becoming `NaN`.
pub fn compute_normals_from(m: &mut Mesh, from: usize) {
    let n_len = m.n.len();
    let start = (from * 3).min(n_len);
    for i in start..n_len {
        m.n[i] = 0.0;
    }
    let mut t = 0;
    while t + 2 < m.i.len() {
        let a = m.i[t] as usize * 3;
        let b = m.i[t + 1] as usize * 3;
        let c = m.i[t + 2] as usize * 3;
        t += 3;
        if a < from * 3 && b < from * 3 && c < from * 3 {
            continue;
        }
        let (ax, ay, az) = (m.p[a], m.p[a + 1], m.p[a + 2]);
        let (e1x, e1y, e1z) = (m.p[b] - ax, m.p[b + 1] - ay, m.p[b + 2] - az);
        let (e2x, e2y, e2z) = (m.p[c] - ax, m.p[c + 1] - ay, m.p[c + 2] - az);
        let nx = e1y * e2z - e1z * e2y;
        let ny = e1z * e2x - e1x * e2z;
        let nz = e1x * e2y - e1y * e2x;
        m.n[a] += nx;
        m.n[a + 1] += ny;
        m.n[a + 2] += nz;
        m.n[b] += nx;
        m.n[b + 1] += ny;
        m.n[b + 2] += nz;
        m.n[c] += nx;
        m.n[c + 1] += ny;
        m.n[c + 2] += nz;
    }
    let mut i = start;
    while i + 2 < n_len {
        let l = jsmath::hypot3(m.n[i], m.n[i + 1], m.n[i + 2]);
        let l = if l == 0.0 { 1.0 } else { l };
        m.n[i] /= l;
        m.n[i + 1] /= l;
        m.n[i + 2] /= l;
        i += 3;
    }
}

/// Weld coincident vertices so smooth normals cross the seams of a loft
/// (`weldNormals(m, eps = 1e-4)`, `geo.js:382-407`).
///
/// This averages **normals only** — no vertex is ever merged, so the vertex
/// count, the vertex order and the index buffer are untouched. (That is why
/// this port's golden test compares buffers element-wise rather than needing
/// `geometry_assert`'s weld-invariant triangle soup for the intermediate
/// meshes.)
///
/// The bucket key is the source's `${round(x*cell)},${round(y*cell)},...`
/// template string. `-0` and `0` stringify identically in JS, and [`js_round`]
/// plus an `i64` cast reproduces that collapse.
pub fn weld_normals(m: &mut Mesh, eps: f64) {
    let cell = 1.0 / eps;
    let mut map: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    let n = m.vcount();
    for i in 0..n {
        let k = (
            jsmath::round(m.p[i * 3] * cell) as i64,
            jsmath::round(m.p[i * 3 + 1] * cell) as i64,
            jsmath::round(m.p[i * 3 + 2] * cell) as i64,
        );
        map.entry(k).or_default().push(i);
    }
    // Iteration order over the buckets is irrelevant: the buckets partition
    // the vertices, and each one's sum runs in ascending vertex index (the
    // order they were pushed), exactly as the source's `Map` does.
    for list in map.values() {
        if list.len() < 2 {
            continue;
        }
        let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
        for &i in list {
            nx += m.n[i * 3];
            ny += m.n[i * 3 + 1];
            nz += m.n[i * 3 + 2];
        }
        let l = jsmath::hypot3(nx, ny, nz);
        let l = if l == 0.0 { 1.0 } else { l };
        nx /= l;
        ny /= l;
        nz /= l;
        for &i in list {
            m.n[i * 3] = nx;
            m.n[i * 3 + 1] = ny;
            m.n[i * 3 + 2] = nz;
        }
    }
}

/// Displace every vertex along its normal by `fn(x, y, z, nx, ny, nz, i)`
/// (`geo.js:410-423`), at the source's `from = 0` default — the only form any
/// call site uses. See [`displace_from`] for the parameterised one.
pub fn displace(m: &mut Mesh, f: impl Fn(f64, f64, f64, f64, f64, f64, usize) -> f64) {
    displace_from(m, f, 0);
}

/// [`displace`] with an explicit `from`.
///
/// **Source quirk preserved:** the guard is `if (!d) continue`, a *falsy*
/// test — so `d == 0.0` **and `d == NaN`** both skip the vertex. A Rust
/// `if d != 0.0` would push the vertex to `NaN` instead.
pub fn displace_from(m: &mut Mesh, f: impl Fn(f64, f64, f64, f64, f64, f64, usize) -> f64, from: usize) {
    let n = m.vcount();
    for i in from..n {
        let (x, y, z) = (m.p[i * 3], m.p[i * 3 + 1], m.p[i * 3 + 2]);
        let (nx, ny, nz) = (m.n[i * 3], m.n[i * 3 + 1], m.n[i * 3 + 2]);
        let d = f(x, y, z, nx, ny, nz, i);
        if d == 0.0 || d.is_nan() {
            continue;
        }
        m.p[i * 3] = x + nx * d;
        m.p[i * 3 + 1] = y + ny * d;
        m.p[i * 3 + 2] = z + nz * d;
    }
}

/// Free-form deform: `fn(v)` mutates the vector in place (`geo.js:426-436`),
/// at the source's `from = 0` default. See [`warp_from`] for the
/// parameterised form.
pub fn warp(m: &mut Mesh, f: impl Fn(&mut V3, usize)) {
    warp_from(m, f, 0);
}

/// [`warp`] with an explicit `from`.
pub fn warp_from(m: &mut Mesh, f: impl Fn(&mut V3, usize), from: usize) {
    let n = m.vcount();
    for i in from..n {
        let mut v = V3::new(m.p[i * 3], m.p[i * 3 + 1], m.p[i * 3 + 2]);
        f(&mut v, i);
        m.p[i * 3] = v.x;
        m.p[i * 3 + 1] = v.y;
        m.p[i * 3 + 2] = v.z;
    }
}

/// `THREE.Matrix4`, `f64`, in THREE's own **column-major** `elements` order
/// (`te[0..3]` is the first column). Only the three operations `geo.js` and
/// its callers reach for are here: [`M4::compose`] (how `parts.js`'s `place`
/// builds one), [`M4::transform_point`] (`Vector3.applyMatrix4`) and
/// [`M4::normal_matrix`] (`Matrix3.getNormalMatrix`).
///
/// This is *not* `axiom_math::Mat4` and not the cofactor shortcut
/// `weapons::geometry::geo`/`world::geo` use for the same job: those are `f32`
/// and algebraically-equivalent-but-differently-grouped. The character
/// pipeline is `f64`, and float arithmetic is not associative, so the
/// three@0.180 `Matrix3.invert()` + `transpose()` sequence is transcribed
/// literally below rather than folded into a cofactor matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct M4 {
    /// Column-major, exactly `Matrix4.elements`.
    pub e: [f64; 16],
}

impl M4 {
    /// `Matrix4.compose(position, quaternion, scale)`
    /// (`three/src/math/Matrix4.js`).
    pub fn compose(position: [f64; 3], q: Q, scale: [f64; 3]) -> M4 {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        let (sx, sy, sz) = (scale[0], scale[1], scale[2]);
        M4 {
            e: [
                (1.0 - (yy + zz)) * sx,
                (xy + wz) * sx,
                (xz - wy) * sx,
                0.0,
                (xy - wz) * sy,
                (1.0 - (xx + zz)) * sy,
                (yz + wx) * sy,
                0.0,
                (xz + wy) * sz,
                (yz - wx) * sz,
                (1.0 - (xx + yy)) * sz,
                0.0,
                position[0],
                position[1],
                position[2],
                1.0,
            ],
        }
    }

    /// `Vector3.applyMatrix4(m)` — including the perspective divide by
    /// `w = 1 / (e3*x + e7*y + e11*z + e15)`, which the source does
    /// unconditionally even for an affine matrix (where `w == 1`).
    pub fn transform_point(&self, v: V3) -> V3 {
        let e = &self.e;
        let (x, y, z) = (v.x, v.y, v.z);
        let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        V3::new(
            (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
            (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
            (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
        )
    }

    /// `Matrix3.getNormalMatrix(m4)` = `setFromMatrix4(m4).invert().transpose()`,
    /// transcribed step for step. Returns the `Matrix3` in THREE's
    /// column-major `elements` order.
    ///
    /// `Matrix3.invert()` on a singular matrix sets **all nine entries to
    /// zero** (it does not throw, and does not produce `Infinity`); that arm
    /// is kept.
    pub fn normal_matrix(&self) -> [f64; 9] {
        let me = &self.e;
        // `setFromMatrix4`: set(me[0], me[4], me[8], me[1], me[5], me[9],
        // me[2], me[6], me[10]) — `set` takes row-major arguments and stores
        // column-major, so te = [n11, n21, n31, n12, n22, n32, n13, n23, n33].
        let te = [me[0], me[1], me[2], me[4], me[5], me[6], me[8], me[9], me[10]];
        let (n11, n21, n31) = (te[0], te[1], te[2]);
        let (n12, n22, n32) = (te[3], te[4], te[5]);
        let (n13, n23, n33) = (te[6], te[7], te[8]);
        let t11 = n33 * n22 - n32 * n23;
        let t12 = n32 * n13 - n33 * n12;
        let t13 = n23 * n12 - n22 * n13;
        let det = n11 * t11 + n21 * t12 + n31 * t13;
        if det == 0.0 {
            return [0.0; 9];
        }
        let det_inv = 1.0 / det;
        let inv = [
            t11 * det_inv,
            (n31 * n23 - n33 * n21) * det_inv,
            (n32 * n21 - n31 * n22) * det_inv,
            t12 * det_inv,
            (n33 * n11 - n31 * n13) * det_inv,
            (n31 * n12 - n32 * n11) * det_inv,
            t13 * det_inv,
            (n21 * n13 - n23 * n11) * det_inv,
            (n22 * n11 - n21 * n12) * det_inv,
        ];
        // `Matrix3.transpose()`: swap te[1]<->te[3], te[2]<->te[6], te[5]<->te[7].
        [inv[0], inv[3], inv[6], inv[1], inv[4], inv[7], inv[2], inv[5], inv[8]]
    }
}

/// `Vector3.applyMatrix3(m)` with `m` in THREE's column-major order.
fn apply_matrix3(e: &[f64; 9], v: V3) -> V3 {
    let (x, y, z) = (v.x, v.y, v.z);
    V3::new(
        e[0] * x + e[3] * y + e[6] * z,
        e[1] * x + e[4] * y + e[7] * z,
        e[2] * x + e[5] * y + e[8] * z,
    )
}

/// `transformMesh(m, matrix)` (`geo.js:438-450`): every position through the
/// matrix, every normal through the normal matrix and re-normalized.
pub fn transform_mesh(m: &mut Mesh, matrix: &M4) {
    let nm = matrix.normal_matrix();
    let n = m.vcount();
    for i in 0..n {
        let v = matrix.transform_point(V3::new(m.p[i * 3], m.p[i * 3 + 1], m.p[i * 3 + 2]));
        m.p[i * 3] = v.x;
        m.p[i * 3 + 1] = v.y;
        m.p[i * 3 + 2] = v.z;
        let v = apply_matrix3(&nm, V3::new(m.n[i * 3], m.n[i * 3 + 1], m.n[i * 3 + 2])).normalize_or_zero();
        m.n[i * 3] = v.x;
        m.n[i * 3 + 1] = v.y;
        m.n[i * 3 + 2] = v.z;
    }
}

/// `appendMesh(dst, src)` (`geo.js:452-459`).
pub fn append_mesh(dst: &mut Mesh, src: &Mesh) {
    let base = dst.vcount() as u32;
    dst.p.extend_from_slice(&src.p);
    dst.n.extend_from_slice(&src.n);
    dst.uv.extend_from_slice(&src.uv);
    dst.i.extend(src.i.iter().map(|&i| i + base));
}

/* ------------------------------------------------------------------ */
/* Character builder — parts, materials, skin weights, baked AO        */
/* ------------------------------------------------------------------ */

/// The builder's `materials` table — `{ name: { tile } }` (`geo.js:475`),
/// flattened to the `(name, tile)` pairs the one real caller authors it as
/// (`soldier.js:19-33`, nine entries, every one of them with a `tile`).
///
/// The source reads it as `this.materials[p.material]?.tile ?? 0.4`, two
/// fallbacks deep: material absent, or material present with no `tile`. Both
/// land on the same `0.4`, and the second is unreachable — every entry in the
/// real table has a tile — so a flat slice loses nothing a caller can observe,
/// and costs no allocation for what the source writes as a module constant.
pub type MaterialTiles<'a> = &'a [(&'a str, f64)];

/// An AO proxy capsule — `{ a, b, r, k }` (`geo.js:477`, `occlude()`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Occluder {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub r: f64,
    /// `k = 1` by default — the proxy's strength.
    pub k: f64,
}

/// The per-part options object `CharacterBuilder.add(mesh, o)` takes
/// (`geo.js:486-488`).
#[derive(Debug, Clone, PartialEq)]
pub struct PartOptions {
    /// `o.name` — reported back in [`CharacterGeometry::parts`].
    pub name: String,
    /// `o.material` — the group key; also the `materials` map lookup.
    pub material: String,
    /// `o.bone` — a single bone name for a rigid part, overriding `bones`.
    pub bone: Option<String>,
    /// `o.bones ?? ['Hips']` — candidate bone names for smooth binding.
    pub bones: Option<Vec<String>>,
    /// `o.bias` — per-candidate weight multiplier, `bias[c] ?? 1`.
    pub bias: Option<Vec<f64>>,
    /// `o.power ?? 3.2` — the inverse-distance falloff exponent.
    pub power: Option<f64>,
    /// `o.colour ?? [1, 1, 1]`.
    pub colour: Option<[f64; 3]>,
    /// `o.wear ?? 0`.
    pub wear: Option<f64>,
    /// `o.grime ?? 0.5`.
    pub grime: Option<f64>,
    /// `o.dirt ?? 0.5`.
    pub dirt: Option<f64>,
    /// `o.dust ?? 0.22`.
    pub dust: Option<f64>,
    /// `o.tile` — overrides the material's tile size.
    pub tile: Option<f64>,
    /// `o.uvOffset?.[0] ?? 0` / `[1] ?? 0`.
    pub uv_offset: Option<[f64; 2]>,
    /// `o.weld !== false` — **default `true`**; see [`PartOptions::default`].
    pub weld: bool,
}

impl Default for PartOptions {
    /// Every field absent, and `weld = true` — the source's test is
    /// `o.weld !== false`, so an unset `weld` welds.
    fn default() -> Self {
        PartOptions {
            name: String::new(),
            material: String::new(),
            bone: None,
            bones: None,
            bias: None,
            power: None,
            colour: None,
            wear: None,
            grime: None,
            dirt: None,
            dust: None,
            tile: None,
            uv_offset: None,
            weld: true,
        }
    }
}

/// A `geometry.addGroup(start, count, materialIndex)` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub start: usize,
    pub count: usize,
    /// `matNames.indexOf(g.mat)` — `-1` when the material is not in the list,
    /// reachable only from the empty-builder case (see [`CharacterBuilder::build`]).
    pub material_index: i32,
}

/// A per-part vertex range, "so the albedo audit in `selftest.mjs` can report
/// the effective value of every single piece of kit" (`geo.js:581-583`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartRange {
    pub name: String,
    pub material: String,
    pub start: usize,
    pub count: usize,
}

/// `geometry.setIndex(new BufferAttribute(idx, 1))` — the source picks the
/// element width from the vertex total (`geo.js:520`), and the width is part
/// of the contract for whatever uploads this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexBuffer {
    /// `new Uint16Array(iTotal)` — chosen when `vTotal <= 65535`.
    U16(Vec<u16>),
    /// `new Uint32Array(iTotal)` — chosen when `vTotal > 65535`.
    U32(Vec<u32>),
}

impl IndexBuffer {
    /// The indices widened to `u32`, for callers that do not care which
    /// element width the source picked.
    pub fn to_u32(&self) -> Vec<u32> {
        match self {
            IndexBuffer::U16(v) => v.iter().map(|&i| u32::from(i)).collect(),
            IndexBuffer::U32(v) => v.clone(),
        }
    }
}

/// `geometry.boundingSphere` after `computeBoundingSphere()` **and** the
/// `radius *= 1.45` on `geo.js:573` ("animated poses reach outside the
/// bind-pose bounds").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingSphere {
    pub center: [f64; 3],
    pub radius: f64,
}

/// `geometry.boundingBox` after `computeBoundingBox()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// What `CharacterBuilder.build()` returns (`geo.js:576-584`), with the
/// `THREE.BufferGeometry` flattened to its attribute buffers.
///
/// **Storage width is part of the algorithm here.** The source allocates
/// `Float32Array`s for `position`/`normal`/`uv`/`color`/`skinWeight` and a
/// `Uint16Array` for `skinIndex` (`geo.js:514-520`), and then *reads back out
/// of them*: `_bind` and `_shade` both take their `x, y, z, nx, ny, nz`
/// from the already-narrowed `pos`/`nrm`, and so does
/// `computeBoundingSphere`. Every one of those is therefore computed from
/// `f32`-rounded inputs in `f64` and stored back as `f32`. Porting these as
/// `f64` would diverge by ~1e-8 in the vertex colours and the bounds. (Port
/// recipe: "Storage width is part of the algorithm".)
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterGeometry {
    pub position: Vec<f32>,
    pub normal: Vec<f32>,
    pub uv: Vec<f32>,
    pub color: Vec<f32>,
    pub skin_index: Vec<u16>,
    pub skin_weight: Vec<f32>,
    pub index: IndexBuffer,
    pub groups: Vec<Group>,
    pub material_names: Vec<String>,
    pub vertices: usize,
    pub triangles: usize,
    pub parts: Vec<PartRange>,
    pub bounding_sphere: BoundingSphere,
    pub bounding_box: BoundingBox,
}

/// One `{ mesh, ...o }` entry in `this.parts` (`geo.js:490`).
struct Part {
    mesh: Mesh,
    o: PartOptions,
    /// `p._vo` — this part's first vertex in the merged buffers.
    vo: usize,
    /// `p._vn` — this part's vertex count.
    vn: usize,
}

/// `class CharacterBuilder` (`geo.js:470-733`).
///
/// Collects parts into one interleaved buffer set with one geometry group per
/// material, computes skin weights against the rig's bind-pose segments and
/// bakes ambient occlusion, grime and edge wear into the vertex colours.
pub struct CharacterBuilder<'a> {
    rig: &'a Rig,
    noise: &'a Noise,
    materials: MaterialTiles<'a>,
    parts: Vec<Part>,
    occluders: Vec<Occluder>,
}

impl<'a> CharacterBuilder<'a> {
    /// `constructor(rig, { noise, materials })` (`geo.js:471-478`).
    ///
    /// Takes [`crate::ai::rig::Rig`] concretely rather than through a trait.
    /// There is exactly one skeleton in this engine and exactly one caller
    /// (`ai::soldier`); a type parameter with a single instantiation would be
    /// ceremony, and the two methods used here (`index`, `distance_to_bone`)
    /// are already the whole of what the source's `CharacterBuilder` asks of
    /// its rig.
    pub fn new(rig: &'a Rig, noise: &'a Noise, materials: MaterialTiles<'a>) -> Self {
        CharacterBuilder { rig, noise, materials, parts: Vec::new(), occluders: Vec::new() }
    }

    /// `add(mesh, o)` (`geo.js:489-492`). Takes the mesh by value because the
    /// source stores the same object it just mutated in place; the normals
    /// are computed (and, unless `weld == false`, welded) here, not at build
    /// time.
    pub fn add(&mut self, mut mesh: Mesh, o: PartOptions) -> &mut Self {
        compute_normals(&mut mesh);
        if o.weld {
            weld_normals(&mut mesh, 1e-4);
        }
        self.parts.push(Part { mesh, o, vo: 0, vn: 0 });
        self
    }

    /// `occlude(a, b, r, k = 1)` (`geo.js:495-498`) — register an occlusion
    /// proxy capsule used when baking vertex AO.
    pub fn occlude(&mut self, a: [f64; 3], b: [f64; 3], r: f64, k: f64) -> &mut Self {
        self.occluders.push(Occluder { a, b, r, k });
        self
    }

    /// `build()` (`geo.js:500-585`).
    ///
    /// # Panics
    /// If a part names a bone the rig does not know (the source throws from
    /// `rig.index`).
    pub fn build(&mut self) -> CharacterGeometry {
        // `const rig = this.rig;` (`geo.js:501`) is dead in the source —
        // `build()` never reads it; `_bind` re-reads `this.rig` itself. Kept
        // as this comment rather than a binding Rust would warn on. (Port
        // recipe: "dead computation in the source is still part of the
        // source".)
        let mut mat_names: Vec<String> = Vec::new();
        for p in &self.parts {
            if !mat_names.contains(&p.o.material) {
                mat_names.push(p.o.material.clone());
            }
        }
        // sort parts by material so each group is contiguous
        let mut order: Vec<usize> = Vec::with_capacity(self.parts.len());
        for m in &mat_names {
            for (i, p) in self.parts.iter().enumerate() {
                if &p.o.material == m {
                    order.push(i);
                }
            }
        }

        let mut v_total = 0usize;
        let mut i_total = 0usize;
        for &pi in &order {
            v_total += self.parts[pi].mesh.vcount();
            i_total += self.parts[pi].mesh.i.len();
        }

        let mut pos = vec![0.0f32; v_total * 3];
        let mut nrm = vec![0.0f32; v_total * 3];
        let mut uv = vec![0.0f32; v_total * 2];
        let mut col = vec![0.0f32; v_total * 3];
        let mut skin_index = vec![0u16; v_total * 4];
        let mut skin_weight = vec![0.0f32; v_total * 4];
        let use_u32 = v_total > 65535;
        let mut idx = vec![0u32; i_total];

        let mut groups: Vec<Group> = Vec::new();
        let mut vo = 0usize;
        let mut io = 0usize;
        let mut cur_mat: Option<String> = None;
        let mut group_start = 0usize;

        for &pi in &order {
            if Some(&self.parts[pi].o.material) != cur_mat.as_ref() {
                if let Some(mat) = &cur_mat {
                    groups.push(Group {
                        start: group_start,
                        count: io - group_start,
                        material_index: index_of(&mat_names, mat),
                    });
                }
                cur_mat = Some(self.parts[pi].o.material.clone());
                group_start = io;
            }
            let p = &self.parts[pi];
            let m = &p.mesh;
            let n = m.vcount();
            let tile = p
                .o
                .tile
                .or_else(|| {
                    self.materials.iter().find(|(name, _)| *name == p.o.material).map(|(_, t)| *t)
                })
                .unwrap_or(0.4);
            let inv = 1.0 / tile;
            let uv_off = p.o.uv_offset.unwrap_or([0.0, 0.0]);
            for i in 0..n {
                pos[(vo + i) * 3] = m.p[i * 3] as f32;
                pos[(vo + i) * 3 + 1] = m.p[i * 3 + 1] as f32;
                pos[(vo + i) * 3 + 2] = m.p[i * 3 + 2] as f32;
                nrm[(vo + i) * 3] = m.n[i * 3] as f32;
                nrm[(vo + i) * 3 + 1] = m.n[i * 3 + 1] as f32;
                nrm[(vo + i) * 3 + 2] = m.n[i * 3 + 2] as f32;
                uv[(vo + i) * 2] = (m.uv[i * 2] * inv + uv_off[0]) as f32;
                uv[(vo + i) * 2 + 1] = (m.uv[i * 2 + 1] * inv + uv_off[1]) as f32;
            }
            for i in 0..m.i.len() {
                idx[io + i] = m.i[i] + vo as u32;
            }
            let i_len = m.i.len();
            self.parts[pi].vo = vo;
            self.parts[pi].vn = n;
            vo += n;
            io += i_len;
        }
        groups.push(Group {
            start: group_start,
            count: io - group_start,
            // `matNames.indexOf(curMat)` with `curMat === null` (no parts at
            // all) yields -1; `index_of` reproduces that.
            material_index: cur_mat.as_ref().map_or(-1, |m| index_of(&mat_names, m)),
        });

        // ---- skin weights -------------------------------------------------
        for &pi in &order {
            self.bind(pi, &pos, &mut skin_index, &mut skin_weight);
        }

        // ---- vertex colour: AO + grime + wear ------------------------------
        self.shade(&order, &pos, &nrm, &mut col);

        let bounding_box = compute_bounding_box(&pos);
        let mut bounding_sphere = compute_bounding_sphere(&pos, bounding_box);
        // animated poses reach outside the bind-pose bounds
        bounding_sphere.radius *= 1.45;

        CharacterGeometry {
            position: pos,
            normal: nrm,
            uv,
            color: col,
            skin_index,
            skin_weight,
            index: if use_u32 {
                IndexBuffer::U32(idx)
            } else {
                IndexBuffer::U16(idx.iter().map(|&i| i as u16).collect())
            },
            groups,
            material_names: mat_names,
            vertices: v_total,
            triangles: i_total / 3,
            parts: order
                .iter()
                .map(|&pi| {
                    let p = &self.parts[pi];
                    PartRange {
                        name: p.o.name.clone(),
                        material: p.o.material.clone(),
                        start: p.vo,
                        count: p.vn,
                    }
                })
                .collect(),
            bounding_sphere,
            bounding_box,
        }
    }

    /// `_bind(part, pos, skinIndex, skinWeight)` (`geo.js:587-638`).
    fn bind(&self, pi: usize, pos: &[f32], skin_index: &mut [u16], skin_weight: &mut [f32]) {
        let part = &self.parts[pi];
        let n = part.vn;
        let vo = part.vo;
        // `if (part.bone)` is a truthy test, so an EMPTY bone name would fall
        // through to the smooth-binding path in the source where `Some("")`
        // takes this branch here. No call site passes one (a bone name is
        // always a real `rig.js` label), and `rig.index("")` would throw
        // anyway, so this is a difference in which error you get, not in any
        // reachable behaviour — noted rather than emulated.
        if let Some(bone) = &part.o.bone {
            let bi = self.rig.index(bone) as u16;
            for i in 0..n {
                skin_index[(vo + i) * 4] = bi;
                skin_weight[(vo + i) * 4] = 1.0;
            }
            return;
        }
        let default_bones = vec!["Hips".to_string()];
        let names = part.o.bones.as_ref().unwrap_or(&default_bones);
        let cands: Vec<usize> = names.iter().map(|nm| self.rig.index(nm)).collect();
        let bias = part.o.bias.as_ref();
        let power = part.o.power.unwrap_or(3.2);
        let mut w_buf = vec![0.0f64; cands.len()];
        for i in 0..n {
            let x = f64::from(pos[(vo + i) * 3]);
            let y = f64::from(pos[(vo + i) * 3 + 1]);
            let z = f64::from(pos[(vo + i) * 3 + 2]);
            // `sum` is accumulated by the source and never read — dead, and
            // kept as this note rather than an unused Rust binding.
            for c in 0..cands.len() {
                let d = self.rig.distance_to_bone(cands[c], x, y, z);
                let mut w = 1.0 / (d.powf(power) + 1e-6);
                if let Some(b) = bias {
                    w *= b.get(c).copied().unwrap_or(1.0);
                }
                w_buf[c] = w;
            }
            // keep the four strongest
            let (mut i0, mut i1, mut i2, mut i3) = (-1i32, -1i32, -1i32, -1i32);
            let (mut w0, mut w1, mut w2, mut w3) = (-1.0f64, -1.0f64, -1.0f64, -1.0f64);
            for (c, &w) in w_buf.iter().enumerate() {
                let c = c as i32;
                if w > w0 {
                    i3 = i2;
                    w3 = w2;
                    i2 = i1;
                    w2 = w1;
                    i1 = i0;
                    w1 = w0;
                    i0 = c;
                    w0 = w;
                } else if w > w1 {
                    i3 = i2;
                    w3 = w2;
                    i2 = i1;
                    w2 = w1;
                    i1 = c;
                    w1 = w;
                } else if w > w2 {
                    i3 = i2;
                    w3 = w2;
                    i2 = c;
                    w2 = w;
                } else if w > w3 {
                    i3 = c;
                    w3 = w;
                }
            }
            let picks = [i0, i1, i2, i3];
            let ws = [w0, w1, w2, w3];
            let mut tot = 0.0f64;
            for s in 0..4 {
                if picks[s] >= 0 && ws[s] > 0.0 {
                    tot += ws[s];
                }
            }
            if tot <= 0.0 {
                skin_index[(vo + i) * 4] = cands[0] as u16;
                skin_weight[(vo + i) * 4] = 1.0;
                continue;
            }
            for s in 0..4 {
                let c = picks[s];
                if c < 0 || ws[s] <= 0.0 {
                    continue;
                }
                skin_index[(vo + i) * 4 + s] = cands[c as usize] as u16;
                skin_weight[(vo + i) * 4 + s] = (ws[s] / tot) as f32;
            }
        }
    }

    /// `_shade(order, pos, nrm, col)` (`geo.js:646-732`).
    ///
    /// Vertex colour = baked capsule AO x crevice grime x edge wear x
    /// per-part tint. This is what puts dark under the plate carrier and the
    /// helmet brim, grime at the hems and boots, and rub-through on knees and
    /// elbows — the things that stop a procedural character reading as
    /// plastic.
    fn shade(&self, order: &[usize], pos: &[f32], nrm: &[f32], col: &mut [f32]) {
        let occ = &self.occluders;
        let nz = self.noise;
        let ground_dirt = |y: f64| 0.0f64.max(1.0 - 0.0f64.max(y - 0.02) / 0.55);
        for &pi in order {
            let part = &self.parts[pi];
            let n = part.vn;
            let vo = part.vo;
            let tint = part.o.colour.unwrap_or([1.0, 1.0, 1.0]);
            let wear_amt = part.o.wear.unwrap_or(0.0);
            let grime_amt = part.o.grime.unwrap_or(0.5);
            let dirt_amt = part.o.dirt.unwrap_or(0.5);
            let dust_amt = part.o.dust.unwrap_or(0.22);
            for i in 0..n {
                let vi = vo + i;
                let x = f64::from(pos[vi * 3]);
                let y = f64::from(pos[vi * 3 + 1]);
                let z = f64::from(pos[vi * 3 + 2]);
                let nx = f64::from(nrm[vi * 3]);
                let ny = f64::from(nrm[vi * 3 + 1]);
                let nz3 = f64::from(nrm[vi * 3 + 2]);

                // --- capsule AO: sample each proxy, attenuate by how deeply
                //     the vertex sits inside it and how much it faces away
                let mut ao = 1.0f64;
                for c in occ {
                    let (d, closest) = seg_dist(x, y, z, c.a, c.b);
                    let t = d - c.r;
                    if t > 0.09 {
                        continue;
                    }
                    // facing term: surfaces pointing into the occluder darken most
                    let (cx, cy, cz) = (closest[0], closest[1], closest[2]);
                    let (mut dx, mut dy, mut dz) = (cx - x, cy - y, cz - z);
                    let dl = jsmath::hypot3(dx, dy, dz);
                    let dl = if dl == 0.0 { 1.0 } else { dl };
                    dx /= dl;
                    dy /= dl;
                    dz /= dl;
                    let face = 0.0f64.max(nx * dx + ny * dy + nz3 * dz);
                    let w = (1.0 - 1.0f64.min(0.0f64.max(t) / 0.09)) * face * c.k;
                    // 0.42, not 0.55: the vertex AO is multiplied into the
                    // albedo, so a heavy term here drags the whole uniform out
                    // of the 0.16-0.32 window the material set is calibrated
                    // to. Occlusion belongs in the light, not in the diffuse
                    // colour — the cavity *grime* below is what should carry
                    // the visible darkening.
                    ao *= 1.0 - 0.42 * w;
                }

                // --- crevice grime follows AO, tinted brown-black
                let grime = (1.0 - ao) * grime_amt;
                // --- ground dirt at the hems / boots
                let dirt = ground_dirt(y) * dirt_amt * (0.55 + 0.45 * nz.fbm3(x * 26.0, y * 26.0, z * 26.0, 2));
                // --- settled dust: up-facing surfaces collect a pale film
                let dust = 0.0f64.max(ny).powf(2.2)
                    * (0.35 + 0.65 * nz.fbm3(x * 13.0 + 4.0, y * 13.0, z * 13.0 + 9.0, 2))
                    * dust_amt;
                // --- edge wear: outward, upward-ish facets on high parts rub pale
                let mut wear = 0.0f64;
                if wear_amt > 0.0 {
                    // `Math.max(0, Math.abs(nz3))` is just `abs` — the source's
                    // redundant clamp, kept.
                    let upness = 0.0f64.max(ny) * 0.55 + 0.0f64.max(nz3.abs()) * 0.45;
                    let nzv = nz.fbm3(x * 34.0 + 11.0, y * 34.0, z * 34.0 - 7.0, 3);
                    wear = wear_amt * upness * 0.0f64.max(nzv * 0.5 + 0.42).powf(1.6);
                }
                // --- broad value noise so no two square centimetres match
                let mottle = 1.0 + 0.055 * nz.fbm3(x * 9.0, y * 9.0, z * 9.0, 3);

                let mut r = tint[0] * ao * mottle;
                let mut g = tint[1] * ao * mottle;
                let mut b = tint[2] * ao * mottle;
                // grime: pull toward a dark warm neutral
                r = r * (1.0 - grime) + 0.055 * grime;
                g = g * (1.0 - grime) + 0.045 * grime;
                b = b * (1.0 - grime) + 0.036 * grime;
                // ground dirt: pull toward pale sand
                r = r * (1.0 - dirt * 0.42) + 0.34 * dirt * 0.42;
                g = g * (1.0 - dirt * 0.42) + 0.30 * dirt * 0.42;
                b = b * (1.0 - dirt * 0.42) + 0.24 * dirt * 0.42;
                // settled dust on up-facing surfaces: a thin pale film, not a repaint
                r = r * (1.0 - dust * 0.30) + 0.40 * dust * 0.30;
                g = g * (1.0 - dust * 0.30) + 0.36 * dust * 0.30;
                b = b * (1.0 - dust * 0.30) + 0.30 * dust * 0.30;
                // wear: rub toward a scuffed mid grey. 0.33, not 0.5 — abraded
                // nylon goes dull, and a pale target turns every knee pad into
                // a highlight
                r = r * (1.0 - wear) + 0.33 * wear;
                g = g * (1.0 - wear) + 0.325 * wear;
                b = b * (1.0 - wear) + 0.31 * wear;

                col[vi * 3] = clamp01(r) as f32;
                col[vi * 3 + 1] = clamp01(g) as f32;
                col[vi * 3 + 2] = clamp01(b) as f32;
            }
        }
    }
}

/// `Array.prototype.indexOf` — `-1` when absent.
fn index_of(names: &[String], name: &str) -> i32 {
    names.iter().position(|n| n == name).map_or(-1, |i| i as i32)
}

/// `clamp01(v)` (`geo.js:735-737`) — written as the source's nested ternary,
/// not `f64::clamp`, because the two differ on `NaN` (the source returns
/// `NaN`; `clamp` panics only on a NaN *bound*, but `f64::clamp` of a NaN
/// value returns NaN too — the shapes agree here, and the ternary is kept for
/// diffability).
fn clamp01(v: f64) -> f64 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// `segDist(px, py, pz, a, b)` (`geo.js:741-752`) — closest point on a
/// segment and the distance to it.
///
/// The source stashes the closest point in three module-level globals
/// (`closestX/Y/Z`) for the caller to read straight afterwards; this returns
/// it as the second tuple element instead, which is the same information
/// without the shared mutable state (Axiom's determinism rules) and without
/// changing a single arithmetic step.
pub fn seg_dist(px: f64, py: f64, pz: f64, a: [f64; 3], b: [f64; 3]) -> (f64, [f64; 3]) {
    let (ax, ay, az) = (a[0], a[1], a[2]);
    let (bx, by, bz) = (b[0], b[1], b[2]);
    let (dx, dy, dz) = (bx - ax, by - ay, bz - az);
    let l2 = dx * dx + dy * dy + dz * dz;
    let mut t = if l2 > 1e-12 {
        ((px - ax) * dx + (py - ay) * dy + (pz - az) * dz) / l2
    } else {
        0.0
    };
    t = if t < 0.0 {
        0.0
    } else if t > 1.0 {
        1.0
    } else {
        t
    };
    let closest = [ax + dx * t, ay + dy * t, az + dz * t];
    (jsmath::hypot3(px - closest[0], py - closest[1], pz - closest[2]), closest)
}

/// `Box3.setFromBufferAttribute(position)` over an `f32` position buffer, then
/// `BufferGeometry.computeBoundingBox()`'s store. An empty buffer leaves the
/// THREE defaults (`+Infinity` min, `-Infinity` max), which is what
/// `Box3.makeEmpty()` sets and what the min/max loop produces on zero
/// iterations — so the two agree and no special case is needed.
fn compute_bounding_box(pos: &[f32]) -> BoundingBox {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for v in pos.chunks_exact(3) {
        for c in 0..3 {
            let x = f64::from(v[c]);
            if x < min[c] {
                min[c] = x;
            }
            if x > max[c] {
                max[c] = x;
            }
        }
    }
    BoundingBox { min, max }
}

/// `BufferGeometry.computeBoundingSphere()`
/// (`three/src/core/BufferGeometry.js`): the centre is the **bounding box
/// centre**, not the centroid, and the radius is the largest distance from it
/// to any vertex — "try to find a boundingSphere with a radius smaller than
/// the boundingSphere of the boundingBox".
fn compute_bounding_sphere(pos: &[f32], bbox: BoundingBox) -> BoundingSphere {
    // `Box3.getCenter`: an empty box (max < min on any axis) centres at the
    // origin instead of averaging two infinities into NaN.
    let empty = (0..3).any(|c| bbox.max[c] < bbox.min[c]);
    let center = if empty {
        [0.0, 0.0, 0.0]
    } else {
        [
            (bbox.min[0] + bbox.max[0]) * 0.5,
            (bbox.min[1] + bbox.max[1]) * 0.5,
            (bbox.min[2] + bbox.max[2]) * 0.5,
        ]
    };
    let mut max_radius_sq = 0.0f64;
    for v in pos.chunks_exact(3) {
        let dx = f64::from(v[0]) - center[0];
        let dy = f64::from(v[1]) - center[1];
        let dz = f64::from(v[2]) - center[2];
        let d = dx * dx + dy * dy + dz * dz;
        if d > max_radius_sq {
            max_radius_sq = d;
        }
    }
    BoundingSphere { center, radius: max_radius_sq.sqrt() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mesh_has_no_vertices() {
        assert_eq!(Mesh::new().vcount(), 0);
    }
}
