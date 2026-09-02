//! Ported from Claude-of-Duty `src/ai/parts.js:1-1073` — the body and clothing
//! part builders for the procedural soldier.
//!
//! Each function returns a mesh record in the actor's bind space (metres, feet
//! on `y = 0`, facing `+Z`, the character's right at `-X`).
//! [`crate::ai::soldier`] decides which parts a variant wears and hands them to
//! [`crate::ai::geo::CharacterBuilder`] along with the bones they bind to; this
//! module is only the geometry.
//!
//! This is the character-side analogue of [`crate::weapons::parts`], but it is
//! **not** built on the same kit. `weapons/geometry.js` is a Three.js
//! `BufferGeometry` kit (boxes, lathes, extrusions, `Assembly`, `f32`); the
//! soldier is built entirely from [`crate::ai::geo`]'s ring-lofting toolkit —
//! `f64` `{p, n, uv, i}` mesh records, one `loft()` under every primitive, and
//! UVs in metres of surface. The two share no primitive, exactly as the two
//! source files share none.
//!
//! ## Deliberate omissions from `parts.js`
//!
//! * `const V = (x, y, z) => [x, y, z]` (`parts.js:16`) is declared and never
//!   used. There is nothing to port.
//! * `revolve` and `vcount` appear in the import list (`parts.js:12-14`) and
//!   are never called. Both exist in [`crate::ai::geo`] anyway.
//! * `faceWrap`, `helmet` and `plateCarrier` each take a trailing `p = {}` —
//!   the variant record — and never read it. Those parameters are dropped
//!   here; an empty Rust options struct would be pure ceremony, and
//!   [`crate::ai::soldier`] already calls them without one. `jacketTorso`'s
//!   [`JacketOpts`] and `headMesh`'s [`HeadOpts`] *are* read, so they stay.
//!
//! ## Determinism
//!
//! `parts.js` draws no randomness of its own: every builder takes an already
//! constructed [`Noise`] and reads it as a pure function of position. The
//! single `rng` contact in the whole dependency closure is
//! [`Noise::new`], which shuffles a 256-entry permutation with 255
//! `rng.int(0, i)` draws — `soldier.js:183` builds it from `rng.fork()`. Part
//! build order therefore cannot perturb any value, which is what lets
//! `tests/ai_parts/capture.mjs` call the builders in any order.

use crate::ai::geo::{
    append_mesh, box_round, compute_normals, displace, ellipse_profile, ellipsoid, empty_mesh, loft, ribbon,
    super_ellipse, transform_mesh, tube, warp, BoxRoundOpts, EllipsoidOpts, LoftOpts, Mesh, Noise, RibbonOpts,
    Ring, TubeOpts, M4,
};
use crate::weapons::rig_math::{Q, V3};

/* ================================================================== */
/* Three.js rotations `ai/geo.rs` does not carry                      */
/* ================================================================== */

/// `Quaternion.setFromEuler(new Euler(x, y, z, 'YXZ'))` —
/// `three/src/math/Quaternion.js`'s `case 'YXZ'` branch (MIT, Three.js
/// authors), transcribed verbatim.
///
/// **Euler order is a convention, not a spelling.** `'YXZ'` is a *different
/// rotation* from `'XYZ'`, and from [`Q::from_euler_xyz`] and
/// `axiom_math::Quat::from_euler_xyz`. `parts.js:44` uses `'YXZ'` for every
/// `place()` in this file. Named in the port recipe's trap list.
///
/// `ai/weapon.rs` carries an identical private helper for `weapon.js:28`,
/// which is the same call in the same Euler order. Both are three lines of
/// transcription against the same Three source; folding them together would
/// mean choosing an owner between two sibling slices, which is
/// [`crate::ai::geo`]'s job to do — not something to decide from here.
fn quat_from_euler_yxz(x: f64, y: f64, z: f64) -> Q {
    let c1 = (x / 2.0).cos();
    let c2 = (y / 2.0).cos();
    let c3 = (z / 2.0).cos();
    let s1 = (x / 2.0).sin();
    let s2 = (y / 2.0).sin();
    let s3 = (z / 2.0).sin();
    Q::new(
        s1 * c2 * c3 + c1 * s2 * s3,
        c1 * s2 * c3 - s1 * c2 * s3,
        c1 * c2 * s3 - s1 * s2 * c3,
        c1 * c2 * c3 + s1 * s2 * s3,
    )
}

/// `Quaternion.setFromAxisAngle(axis, angle)`
/// (`three/src/math/Quaternion.js`, MIT). `boot` and `bootSole` rotate every
/// lofted section by `+PI/2` about X so their profiles stand up along the
/// foot (`parts.js:922`, `parts.js:960`).
fn quat_from_axis_angle(axis: V3, angle: f64) -> Q {
    let half_angle = angle / 2.0;
    let s = half_angle.sin();
    Q::new(axis.x * s, axis.y * s, axis.z * s, half_angle.cos())
}

/// `new THREE.Matrix4().makeBasis(xAxis, yAxis, zAxis).setPosition(pos)`
/// (`three/src/math/Matrix4.js`, MIT) — the glove's hand frame
/// (`parts.js:1010-1012`, `parts.js:1068-1069`). Column-major, matching
/// [`M4`]'s `elements` order: `makeBasis` writes each axis as a *column*, and
/// `setPosition` overwrites only `te[12..14]`.
fn basis_at(x_axis: V3, y_axis: V3, z_axis: V3, position: V3) -> M4 {
    M4 {
        e: [
            x_axis.x, x_axis.y, x_axis.z, 0.0, //
            y_axis.x, y_axis.y, y_axis.z, 0.0, //
            z_axis.x, z_axis.y, z_axis.z, 0.0, //
            position.x, position.y, position.z, 1.0,
        ],
    }
}

/* ================================================================== */
/* Shared transforms (parts.js:18-49)                                 */
/* ================================================================== */

/// `bendY(mesh, radius, centreZ)` (`parts.js:19-26`) — a cylindrical wrap
/// about the Y axis that bends flat slabs around the torso.
pub fn bend_y(mesh: &mut Mesh, radius: f64, centre_z: f64) {
    warp(
        mesh,
        |v, _i| {
            let r = radius + (v.z - centre_z);
            let a = v.x / radius;
            v.x = a.sin() * r;
            v.z = centre_z + a.cos() * r - radius;
        },
    );
}

/// `mirrorX(mesh)` (`parts.js:29-39`) — mirror across X (right <-> left) and
/// fix the winding. Returns a new mesh; the source `slice()`s all four
/// buffers.
pub fn mirror_x(mesh: &Mesh) -> Mesh {
    let mut out = mesh.clone();
    for i in (0..out.p.len()).step_by(3) {
        out.p[i] = -out.p[i];
    }
    for i in (0..out.n.len()).step_by(3) {
        out.n[i] = -out.n[i];
    }
    for t in (0..out.i.len()).step_by(3) {
        out.i.swap(t + 1, t + 2);
    }
    out
}

/// `place(mesh, x, y, z, rx, ry, rz, sx, sy, sz)` (`parts.js:41-49`) —
/// `computeNormals` then a composed TRS transform, rotation in Euler
/// **`'YXZ'`** (see [`quat_from_euler_yxz`]).
///
/// Every `parts.js` call site leaves the scale at 1 and most leave one or more
/// rotations at 0, but all nine parameters are passed explicitly at each site
/// so the port diffs against the source line for line.
#[allow(clippy::too_many_arguments)]
pub fn place(mesh: &mut Mesh, x: f64, y: f64, z: f64, rx: f64, ry: f64, rz: f64, sx: f64, sy: f64, sz: f64) {
    let m = M4::compose(
        V3::new(x, y, z),
        quat_from_euler_yxz(rx, ry, rz),
        V3::new(sx, sy, sz),
    );
    compute_normals(mesh);
    transform_mesh(mesh, &m);
}

/* ================================================================== */
/* Torso                                                              */
/* ================================================================== */

/// `jacketTorso`'s `p` bag (`parts.js:60-62`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JacketOpts {
    /// `p.flare ?? 1` — hem width.
    pub flare: f64,
    /// `p.bulk ?? 1` — chest and shoulder mass.
    pub bulk: f64,
}

impl Default for JacketOpts {
    fn default() -> Self {
        JacketOpts { flare: 1.0, bulk: 1.0 }
    }
}

/// `jacketTorso(nz, p)` (`parts.js:60-110`) — the jacket shell: lofted
/// horizontal sections from the hem to the neck with a real spinal curve, a
/// deeper chest than back, and layered fold noise. This is the silhouette
/// everything else hangs on.
pub fn jacket_torso(nz: &Noise, p: &JacketOpts) -> Mesh {
    let flare = p.flare;
    let bulk = p.bulk;
    // y, half-width, half-depth, z offset, corner exponent
    let s: [[f64; 5]; 13] = [
        [0.865, 0.150 * flare, 0.107 * flare, -0.004, 3.0],
        [0.925, 0.156, 0.110, -0.008, 3.0],
        [0.985, 0.152, 0.105, -0.012, 3.1],
        [1.055, 0.146, 0.100, -0.014, 3.2],
        [1.120, 0.150, 0.104, -0.010, 3.2],
        [1.185, 0.161, 0.112, -0.004, 3.1],
        [1.250, 0.172 * bulk, 0.113 * bulk, 0.002, 3.0],
        [1.310, 0.184 * bulk, 0.117 * bulk, 0.005, 2.9],
        [1.365, 0.195 * bulk, 0.118 * bulk, 0.004, 2.8],
        [1.418, 0.198, 0.111, -0.002, 2.7],
        [1.452, 0.152, 0.096, -0.008, 2.6],
        [1.482, 0.098, 0.080, -0.010, 2.4],
        [1.505, 0.070, 0.066, -0.010, 2.3],
    ];
    let seg = 26;
    let rings: Vec<Ring> = s
        .iter()
        .map(|&[y, hx, hz, zo, n]| Ring::at(super_ellipse(hx, hz, n, seg, 0.0), [0.0, y, zo]))
        .collect();
    let mut m = loft(&rings, LoftOpts { closed: true, cap_start: true, cap_end: false });
    compute_normals(&mut m);

    // chest deeper at the front than the back, shoulders squared off
    warp(
        &mut m,
        |v, _i| {
            let t = 0.0f64.max(1.0f64.min((v.y - 1.1) / 0.3));
            if v.z > 0.0 {
                v.z += 0.016 * t;
            } else {
                v.z -= 0.006 * t;
            }
            // trapezius slope
            if v.y > 1.40 {
                v.y -= 0.02 * 1.0f64.min(v.x.abs() / 0.18).powi(2);
            }
        },
    );
    compute_normals(&mut m);

    // cloth folds: horizontal creases at the waist, vertical pull from the plate
    displace(
        &mut m,
        |x, y, z, _, _, _, _| {
            let fold = nz.fbm3(x * 22.0, y * 15.0, z * 22.0, 3);
            let crease = (y * 38.0 + fold * 3.4).sin() * 0.5 + 0.5;
            let waist = (-((y - 1.06).powi(2)) / 0.006).exp();
            let gather = (-((y - 0.93).powi(2)) / 0.004).exp();
            fold * 0.0026
                + crease * (waist * 0.0022 + gather * 0.0018)
                + nz.fbm3(x * 46.0, y * 46.0, z * 46.0, 2) * 0.0007
        },
    );
    m
}

/// `pelvis(nz)` (`parts.js:113-126`) — pelvis / seat block so the hips read
/// solid between jacket hem and trousers.
pub fn pelvis(nz: &Noise) -> Mesh {
    let seg = 22;
    let rings: Vec<Ring> = [
        [0.845, 0.140, 0.100],
        [0.885, 0.148, 0.106],
        [0.935, 0.152, 0.108],
        [0.985, 0.150, 0.104],
        [1.030, 0.144, 0.098],
    ]
    .iter()
    .map(|&[y, hx, hz]| Ring::at(super_ellipse(hx, hz, 3.0, seg, 0.0), [0.0, y, -0.006]))
    .collect();
    let mut m = loft(&rings, LoftOpts { closed: true, cap_start: true, cap_end: true });
    compute_normals(&mut m);
    displace(&mut m, |x, y, z, _, _, _, _| nz.fbm3(x * 26.0, y * 20.0, z * 26.0, 3) * 0.004);
    m
}

/// `collar(nz)` (`parts.js:129-141`) — a short stand-up band around the neck.
pub fn collar(nz: &Noise) -> Mesh {
    let seg = 22;
    let rings: Vec<Ring> = [
        [1.435, 0.108, 0.092],
        [1.470, 0.090, 0.082],
        [1.500, 0.082, 0.076],
        [1.516, 0.086, 0.080],
    ]
    .iter()
    .map(|&[y, hx, hz]| Ring::at(super_ellipse(hx, hz, 2.6, seg, 0.0), [0.0, y, -0.006]))
    .collect();
    let mut m = loft(&rings, LoftOpts { closed: true, cap_start: false, cap_end: true });
    compute_normals(&mut m);
    displace(&mut m, |x, y, z, _, _, _, _| nz.fbm3(x * 40.0, y * 30.0, z * 40.0, 2) * 0.003);
    m
}

/* ================================================================== */
/* Limbs                                                              */
/* ================================================================== */

/// `limbTube`'s `opts` bag (`parts.js:166-204`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimbOpts {
    /// `opts.rings ?? 11`.
    pub rings: usize,
    /// `opts.seg ?? 14`.
    pub seg: usize,
    /// `opts.flat ?? 0.88` — the cross-section is that much shallower than
    /// it is wide.
    pub flat: f64,
    pub cap_start: bool,
    pub cap_end: bool,
    /// `opts.up ?? [0, 0, 1]` — the ring-frame up reference.
    pub up: [f64; 3],
    /// `opts.fold ?? 0.0016` — amplitude of the always-on fbm fold field.
    pub fold: f64,
    /// `opts.crease ?? 0` — `0` disables the whole cloth-crease pass
    /// (`if (crease > 0)`).
    pub crease: f64,
    /// `opts.bend ?? [0, 0, -1]` — the direction the joint folds toward in
    /// bind space: behind the knee / inside the elbow for a figure facing
    /// `+Z`.
    pub bend: [f64; 3],
}

impl Default for LimbOpts {
    fn default() -> Self {
        LimbOpts {
            rings: 11,
            seg: 14,
            flat: 0.88,
            cap_start: false,
            cap_end: false,
            up: [0.0, 0.0, 1.0],
            fold: 0.0016,
            crease: 0.0,
            bend: [0.0, 0.0, -1.0],
        }
    }
}

/// `limbTube(nz, a, b, c, radii, opts)` (`parts.js:166-234`) — sleeve /
/// trouser leg: a tube down a 3-point bone chain with an elliptical
/// cross-section that is wider than deep, plus fold noise at the joints.
///
/// **Cloth folds** (`opts.crease`) — isotropic fbm on a tube gives a lumpy
/// tube, not cloth. Real sleeves and trousers crease in bands that run
/// *around* the limb, they bunch where the limb bends, and they gather at the
/// cuff where the fabric is stopped by a hem. So the crease field is
/// parameterised by arc length `s` down the bone chain, not by world
/// position:
///
/// * transverse bands at 5-7 cm, ridged so each one is a sharp line with a
///   soft valley either side (that is what a pressed crease looks like in
///   light);
/// * a x2.4 gather inside the elbow / behind the knee (`s` near the joint, on
///   the bend side), which is the single most legible fold on a walking
///   figure;
/// * a x1.8 gather at the cuff, where the fabric stacks on the boot or glove.
pub fn limb_tube(nz: &Noise, a: [f64; 3], b: [f64; 3], c: [f64; 3], radii: &[f64], opts: &LimbOpts) -> Mesh {
    let n_rings = opts.rings;
    let segs = opts.seg;
    // sample the two-segment path with a smooth blend around the joint
    let a_v = V3::from_array(a);
    let b_v = V3::from_array(b);
    let c_v = V3::from_array(c);
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(n_rings);
    for i in 0..n_rings {
        let t = i as f64 / (n_rings - 1) as f64;
        // `lerpVectors(p, q, t)` and `lerp(q, t)` are the same expression in
        // Three (`p + (q - p) * t`), which is what `V3::lerp` transcribes.
        let mut tmp = if t <= 0.5 { a_v.lerp(b_v, t * 2.0) } else { b_v.lerp(c_v, (t - 0.5) * 2.0) };
        // round the corner slightly so the knee/elbow is not a crease
        if t > 0.34 && t < 0.66 {
            let k = 1.0 - (t - 0.5).abs() / 0.16;
            tmp = tmp.lerp(a_v.add(c_v).mul_scalar(0.5), 0.06 * k);
        }
        pts.push([tmp.x, tmp.y, tmp.z]);
    }
    let flat = opts.flat;
    let mut m = tube(
        &pts,
        |t, _i| {
            let r = radius_at(radii, t);
            ellipse_profile(r, r * flat, segs, 0.0)
        },
        &TubeOpts {
            up: opts.up,
            frames: None,
            loft: LoftOpts { closed: true, cap_start: opts.cap_start, cap_end: opts.cap_end },
        },
    );
    compute_normals(&mut m);
    let amp = opts.fold;
    let crease = opts.crease;
    if crease > 0.0 {
        // arc-length parameterisation of the two-segment chain
        let ab = b_v.subtract(a_v);
        let bc = c_v.subtract(b_v);
        let l_ab = ab.length();
        let l_bc = bc.length();
        // `divideScalar(s)` is Three's `multiplyScalar(1 / s)` — a reciprocal
        // multiply, not a division, and the two round differently.
        let u_ab = ab.mul_scalar(1.0 / 1e-5f64.max(l_ab));
        let u_bc = bc.mul_scalar(1.0 / 1e-5f64.max(l_bc));
        let total = l_ab + l_bc;
        let bend = V3::from_array(opts.bend).normalize_or_zero();
        displace(
            &mut m,
            |x, y, z, nx, ny, nzc, _i| {
                // distance along the chain, and how far out along the bend axis
                let q = V3::new(x, y, z);
                let t_ab = 0.0f64.max(l_ab.min(q.subtract(a_v).dot(u_ab)));
                let t_bc = 0.0f64.max(l_bc.min(q.subtract(b_v).dot(u_bc)));
                let s = if t_ab < l_ab - 1e-4 { t_ab } else { l_ab + t_bc };
                let u = s / total;
                // transverse crease bands: ridged, 5.5 cm, jittered so they are
                // not a corduroy ripple
                let jit = nz.fbm3(x * 6.0, y * 5.0, z * 6.0, 2) - 0.5;
                let band = ((s / 0.055 + jit * 0.9) * std::f64::consts::PI).sin().abs();
                let ridged = 1.0 - band.powf(0.65);
                // where the cloth actually bunches
                let joint = (-((u - 0.5).powi(2)) / 0.012).exp();
                let cuff = (-((u - 0.94).powi(2)) / 0.004).exp();
                let inner = 0.0f64.max(bend.x * nx + bend.y * ny + bend.z * nzc);
                let gather = 1.0 + joint * (0.6 + 1.8 * inner) + cuff * 0.8;
                // broad fold field on top, so the limb is never a clean cylinder
                let broad = nz.fbm3(x * 9.0, y * 7.0 + u * 3.1, z * 9.0, 3) - 0.5;
                crease * (ridged * gather * 0.9 + broad * 1.1)
            },
        );
        compute_normals(&mut m);
    }
    displace(
        &mut m,
        |x, y, z, _, _, _, _| {
            let f = nz.fbm3(x * 11.0, y * 9.0, z * 11.0, 3);
            let fine = nz.fbm3(x * 34.0, y * 30.0, z * 34.0, 2);
            f * amp + fine * amp * 0.3
        },
    );
    m
}

/// `radiusAt(radii, t)` (`parts.js:236-242`) — piecewise-linear radius lookup
/// down the chain.
fn radius_at(radii: &[f64], t: f64) -> f64 {
    let n = radii.len() - 1;
    let s = t * n as f64;
    let i = (s.floor() as i64).min(n as i64 - 1) as usize;
    let f = s - i as f64;
    radii[i] + (radii[i + 1] - radii[i]) * f
}

/// `shoulderCap(nz, shoulder, side)` (`parts.js:245-255`) — deltoid cap so
/// the shoulder is round rather than a tube end.
pub fn shoulder_cap(nz: &Noise, shoulder: [f64; 3], side: f64) -> Mesh {
    let mut m = ellipsoid(
        0.052,
        0.064,
        0.056,
        EllipsoidOpts { seg: 18, rows: 12, ..EllipsoidOpts::default() },
    );
    compute_normals(&mut m);
    warp(
        &mut m,
        |v, _i| {
            // `v.y *= 1.0` — a no-op the source keeps. Ported rather than
            // judged dead: the judgement that a line is dead can be wrong, and
            // preserving it costs nothing.
            v.y *= 1.0;
            if v.y < 0.0 {
                v.x *= 0.9;
            }
        },
    );
    place(
        &mut m,
        shoulder[0] + side * 0.012,
        shoulder[1] - 0.008,
        shoulder[2],
        0.0,
        0.0,
        -side * 0.12,
        1.0,
        1.0,
        1.0,
    );
    displace(&mut m, |x, y, z, _, _, _, _| nz.fbm3(x * 30.0, y * 30.0, z * 30.0, 3) * 0.004);
    m
}

/* ================================================================== */
/* Head                                                               */
/* ================================================================== */

/// `headMesh`'s `p` bag (`parts.js:262-263`). No `soldier.js` variant sets
/// `wide`, so `1` is the only value the game ever produces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadOpts {
    /// `p.wide ?? 1` — skull width multiplier.
    pub wide: f64,
}

impl Default for HeadOpts {
    fn default() -> Self {
        HeadOpts { wide: 1.0 }
    }
}

/// `headMesh(nz, base, p)` (`parts.js:262-314`) — skull + jaw, lofted from
/// anatomical sections. `base` is the Head bone position.
pub fn head_mesh(nz: &Noise, base: [f64; 3], p: &HeadOpts) -> Mesh {
    let w = p.wide;
    let s: [[f64; 5]; 11] = [
        [0.000, 0.038 * w, 0.050, 0.020, 2.6],
        [0.020, 0.056 * w, 0.068, 0.014, 2.6],
        [0.044, 0.068 * w, 0.076, 0.007, 2.5],
        [0.070, 0.077 * w, 0.083, 0.001, 2.4],
        [0.095, 0.084 * w, 0.088, -0.002, 2.4],
        [0.119, 0.086 * w, 0.090, -0.005, 2.4],
        [0.146, 0.083 * w, 0.089, -0.009, 2.4],
        [0.176, 0.076 * w, 0.082, -0.012, 2.4],
        [0.205, 0.062 * w, 0.066, -0.014, 2.4],
        [0.230, 0.038 * w, 0.041, -0.014, 2.4],
        [0.244, 0.012 * w, 0.013, -0.014, 2.4],
    ];
    let seg = 24;
    let rings: Vec<Ring> = s
        .iter()
        .map(|&[y, hx, hz, zo, n]| {
            Ring::at(super_ellipse(hx, hz, n, seg, 0.0), [base[0], base[1] + y, base[2] + zo])
        })
        .collect();
    let mut m = loft(&rings, LoftOpts { closed: true, cap_start: true, cap_end: false });
    compute_normals(&mut m);

    let (bx, by, bz) = (base[0], base[1], base[2]);
    // features, all in head-local coordinates
    warp(
        &mut m,
        |v, _i| {
            let x = v.x - bx;
            let y = v.y - by;
            let z = v.z - bz;
            let front = 0.0f64.max(z / 0.09);
            // brow ridge
            let brow = (-((y - 0.113).powi(2)) / 0.00016).exp() * front * (-(x * x) / 0.006).exp();
            // eye sockets
            let socket = (-((x.abs() - 0.033).powi(2)) / 0.00035).exp()
                * (-((y - 0.098).powi(2)) / 0.00022).exp()
                * front;
            // cheekbone
            let cheek = (-((x.abs() - 0.055).powi(2)) / 0.0009).exp()
                * (-((y - 0.070).powi(2)) / 0.0007).exp()
                * 0.0f64.max(z / 0.06);
            // temple flattening
            let temple =
                (-((y - 0.150).powi(2)) / 0.0016).exp() * (-((x.abs() - 0.082).powi(2)) / 0.0006).exp();
            // chin
            let chin = (-(y * y) / 0.00035).exp() * front;
            // occiput
            let occ = (-((y - 0.165).powi(2)) / 0.0018).exp() * 0.0f64.max(-z / 0.09);
            let scale = 1.0 + 0.05 * brow - 0.10 * socket + 0.05 * cheek - 0.06 * temple;
            v.x = bx + x * (1.0 - 0.05 * socket - 0.05 * temple);
            v.y = by + y;
            v.z = bz + z * scale + 0.006 * brow + 0.004 * chin + 0.008 * occ * -1.0;
        },
    );
    compute_normals(&mut m);
    displace(&mut m, |x, y, z, _, _, _, _| nz.fbm3(x * 70.0, y * 70.0, z * 70.0, 3) * 0.0012);
    m
}

/// `nose(nz, base)` (`parts.js:317-334`) — nose wedge + nostrils. `nz` is
/// accepted and never read by the source.
pub fn nose(_nz: &Noise, base: [f64; 3]) -> Mesh {
    let (bx, by, bz) = (base[0], base[1], base[2]);
    let s: [[f64; 4]; 6] = [
        [0.118, 0.075, 0.009, 0.010],
        [0.104, 0.084, 0.011, 0.016],
        [0.088, 0.093, 0.014, 0.020],
        [0.074, 0.100, 0.017, 0.021],
        [0.064, 0.100, 0.020, 0.018],
        [0.058, 0.092, 0.019, 0.012],
    ];
    let rings: Vec<Ring> = s
        .iter()
        .map(|&[y, z, hx, hz]| Ring::at(super_ellipse(hx, hz, 2.2, 12, 0.0), [bx, by + y, bz + z]))
        .collect();
    let mut m = loft(&rings, LoftOpts { closed: true, cap_start: false, cap_end: true });
    compute_normals(&mut m);
    m
}

/// `ear(nz, base, side)` (`parts.js:337-346`) — a folded flattened ellipsoid.
/// `nz` is accepted and never read by the source.
pub fn ear(_nz: &Noise, base: [f64; 3], side: f64) -> Mesh {
    let mut m = ellipsoid(0.010, 0.030, 0.020, EllipsoidOpts { seg: 12, rows: 9, ..EllipsoidOpts::default() });
    compute_normals(&mut m);
    warp(
        &mut m,
        |v, _i| {
            v.z += v.y * 0.25;
            v.x += v.y.abs() * 0.10;
        },
    );
    place(
        &mut m,
        base[0] + side * 0.083,
        base[1] + 0.098,
        base[2] - 0.008,
        0.1,
        side * 0.25,
        0.0,
        1.0,
        1.0,
        1.0,
    );
    m
}

/// `eyeball(base, side)` (`parts.js:349-354`) — a small dark glossy sphere
/// set into the socket.
pub fn eyeball(base: [f64; 3], side: f64) -> Mesh {
    let mut m = ellipsoid(
        0.0125,
        0.0125,
        0.0125,
        EllipsoidOpts { seg: 12, rows: 8, ..EllipsoidOpts::default() },
    );
    compute_normals(&mut m);
    place(
        &mut m,
        base[0] + side * 0.032,
        base[1] + 0.0975,
        base[2] + 0.0665,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        1.0,
    );
    m
}

/// `faceWrap(nz, base, p)` (`parts.js:366-437`) — balaclava / shemagh wrap
/// over the lower face and neck.
///
/// The wrap is not just a dome: the thing that makes a covered face read as a
/// FACE at 35 m is the hem seam along the eye line plus the bridge fold over
/// the nose. Both are built as geometry (a rolled hem ribbon and a
/// centre-front seam) so they survive to whatever mip the diffuse ends up at.
///
/// The source's trailing `p = {}` is never read; see the module doc.
pub fn face_wrap(nz: &Noise, base: [f64; 3]) -> Mesh {
    let (bx, by, bz) = (base[0], base[1], base[2]);
    let s: [[f64; 5]; 8] = [
        [-0.075, 0.062, 0.062, -0.010, 2.6],
        [-0.040, 0.070, 0.072, -0.006, 2.6],
        [-0.010, 0.080, 0.084, 0.004, 2.5],
        [0.014, 0.070, 0.082, 0.014, 2.5],
        [0.038, 0.078, 0.086, 0.008, 2.5],
        [0.060, 0.086, 0.092, 0.002, 2.4],
        [0.076, 0.090, 0.094, -0.002, 2.4],
        [0.086, 0.090, 0.093, -0.006, 2.4],
    ];
    let seg = 22;
    let rings: Vec<Ring> = s
        .iter()
        .map(|&[y, hx, hz, zo, n]| Ring::at(super_ellipse(hx, hz, n, seg, 0.0), [bx, by + y, bz + zo]))
        .collect();
    let mut m = loft(&rings, LoftOpts { closed: true, cap_start: false, cap_end: false });
    compute_normals(&mut m);
    // cut the front open above the eye line by pulling the top ring back
    displace(
        &mut m,
        |x, y, z, _, _, _, _| {
            let fold = nz.fbm3(x * 30.0, y * 24.0, z * 30.0, 3);
            let wrap = (y * 90.0 + fold * 4.0).sin() * 0.5 + 0.5;
            fold * 0.005 + wrap * 0.0035
        },
    );

    let mut out = empty_mesh();
    append_mesh(&mut out, &m);

    // --- rolled hem along the eye line -----------------------------------
    // A wrap's top edge is a doubled-over hem: 8 mm of roll that catches the
    // key light and draws the horizontal line under the eyes.
    let n_hem = 26usize;
    let mut hem: Vec<[f64; 3]> = Vec::with_capacity(n_hem + 1);
    for i in 0..=n_hem {
        let a = (i as f64 / n_hem as f64) * std::f64::consts::PI * 2.0;
        let sx = a.sin();
        let sz = a.cos();
        // the hem rides higher over the cheeks and dips at the bridge of the nose
        let y = 0.086 + 0.0f64.max(sz) * 0.006 - (-(sx * sx) / 0.06).exp() * 0.0f64.max(sz) * 0.010;
        hem.push([bx + sx * 0.092, by + y, bz + sz * 0.096 - 0.004]);
    }
    let mut roll = ribbon(
        &hem,
        0.015,
        0.008,
        &RibbonOpts { upright: true, seg: 6, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut roll);
    append_mesh(&mut out, &roll);

    // --- centre-front seam from the chin to the hem ------------------------
    let mut seam: Vec<[f64; 3]> = Vec::with_capacity(5);
    for i in 0..=4 {
        let t = f64::from(i) / 4.0;
        seam.push([bx, by + 0.082 - t * 0.086, bz + 0.088 - t * 0.020]);
    }
    let mut sm = ribbon(
        &seam,
        0.009,
        0.004,
        &RibbonOpts { upright: false, seg: 5, tube: TubeOpts { up: [1.0, 0.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut sm);
    append_mesh(&mut out, &sm);

    // --- bridge fold over the nose ----------------------------------------
    let mut bridge = ribbon(
        &[
            [bx - 0.042, by + 0.070, bz + 0.070],
            [bx, by + 0.078, bz + 0.092],
            [bx + 0.042, by + 0.070, bz + 0.070],
        ],
        0.013,
        0.005,
        &RibbonOpts { upright: false, seg: 6, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut bridge);
    append_mesh(&mut out, &bridge);

    compute_normals(&mut out);
    out
}

/// `sunglasses(base)`'s return value (`parts.js:467`).
#[derive(Debug, Clone, PartialEq)]
pub struct Sunglasses {
    pub lens: Mesh,
    pub frame: Mesh,
}

/// `sunglasses(base)` (`parts.js:445-468`) — wrap-around dark shooting
/// glasses for the un-helmeted fighter: a curved lens plus two thin temples.
/// This is the whole of variant #2's facing cue — a dark horizontal band at
/// the eye line, the one feature that survives to 35 m on a bare head.
pub fn sunglasses(base: [f64; 3]) -> Sunglasses {
    let (bx, by, bz) = (base[0], base[1], base[2]);
    let mut lens = box_round(
        0.072,
        0.0155,
        0.006,
        BoxRoundOpts { n: 3.0, seg: 18, rows: 5, round_y: 0.6, ..BoxRoundOpts::default() },
    );
    place(&mut lens, bx, by + 0.100, bz + 0.080, -0.06, 0.0, 0.0, 1.0, 1.0, 1.0);
    bend_y(&mut lens, 0.098, 0.0);
    compute_normals(&mut lens);
    let mut frame = empty_mesh();
    for side in [-1.0f64, 1.0] {
        let mut arm = ribbon(
            &[
                [bx + side * 0.070, by + 0.104, bz + 0.062],
                [bx + side * 0.083, by + 0.104, bz + 0.010],
                [bx + side * 0.080, by + 0.100, bz - 0.030],
            ],
            0.008,
            0.004,
            &RibbonOpts {
                upright: true,
                seg: 5,
                tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut arm);
        append_mesh(&mut frame, &arm);
    }
    compute_normals(&mut frame);
    Sunglasses { lens, frame }
}

/* ================================================================== */
/* Helmet                                                             */
/* ================================================================== */

/// `helmet(nz, base, p)` (`parts.js:478-527`) — high-cut ballistic helmet
/// with a scalloped ear cut and a brim lip. `base` is the Head bone position.
///
/// The source's trailing `p = {}` is never read; see the module doc.
pub fn helmet(nz: &Noise, base: [f64; 3]) -> Mesh {
    let mut out = empty_mesh();
    let (bx, by, bz) = (base[0], base[1], base[2]);
    let cy = by + 0.100; // shell centre (just above the brow)
    let (rx, ry, rz) = (0.121, 0.158, 0.135);

    // --- shell: revolved dome, bottom edge scalloped per angle
    let seg = 26;
    let rows = 12usize;
    let mut rings: Vec<Ring> = Vec::with_capacity(rows);
    for r in 0..rows {
        let t = r as f64 / (rows - 1) as f64;
        // t 0 = brim, 1 = crown
        let phi = (0.5 + 0.5 * t) * std::f64::consts::PI; // 90..180 deg
        let y = -phi.cos() * ry;
        let s = phi.sin();
        let pts = ellipse_profile(rx * 0.08f64.max(s), rz * 0.08f64.max(s), seg, 0.0);
        // The source also stores `t` on the ring (`parts.js:494`); `loft` never
        // reads it, so there is nothing for a Rust `Ring` to carry.
        rings.push(Ring::at(pts, [bx, cy + y, bz - 0.006]));
    }
    let mut shell = loft(&rings, LoftOpts { closed: true, cap_start: false, cap_end: false });
    compute_normals(&mut shell);
    // scallop: raise the rim over the ears, drop it at the front and back
    warp(
        &mut shell,
        |v, _i| {
            let dy = v.y - cy;
            if dy > 0.012 {
                return;
            }
            let ang = (v.x - bx).atan2(v.z - bz);
            let side = ang.sin().abs();
            let lift = side.powi(2) * 0.042 - 0.0f64.max(ang.cos()) * 0.010;
            let k = 1.0f64.min(0.0f64.max((0.012 - dy) / 0.06));
            v.y += lift * k;
        },
    );
    compute_normals(&mut shell);
    displace(&mut shell, |x, y, z, _, _, _, _| nz.fbm3(x * 40.0, y * 40.0, z * 40.0, 3) * 0.0016);
    append_mesh(&mut out, &shell);

    // --- brim lip: a thin band following the rim
    let n_lip = 30usize;
    let mut lip_pts: Vec<[f64; 3]> = Vec::with_capacity(n_lip + 1);
    for i in 0..=n_lip {
        let a = (i as f64 / n_lip as f64) * std::f64::consts::PI * 2.0;
        let sx = a.sin();
        let sz = a.cos();
        let side = sx.abs();
        let lift = side.powi(2) * 0.042 - 0.0f64.max(sz) * 0.010;
        lip_pts.push([bx + sx * rx * 0.955, cy + lift - 0.001, bz - 0.004 + sz * rz * 0.955]);
    }
    let mut lip = ribbon(
        &lip_pts,
        0.011,
        0.006,
        &RibbonOpts { upright: true, seg: 6, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut lip);
    append_mesh(&mut out, &lip);

    out
}

/// `helmetHardware(nz, base)` (`parts.js:530-567`) — side rails, NVG shroud
/// and rear counterweight pouch.
pub fn helmet_hardware(nz: &Noise, base: [f64; 3]) -> Mesh {
    let mut out = empty_mesh();
    let (bx, by, bz) = (base[0], base[1], base[2]);
    let cy = by + 0.100;

    // NVG shroud on the brow
    let mut shroud = box_round(
        0.030,
        0.012,
        0.022,
        BoxRoundOpts { n: 4.0, seg: 12, rows: 5, round_y: 0.5, ..BoxRoundOpts::default() },
    );
    place(&mut shroud, bx, cy + 0.062, bz + 0.120, -0.50, 0.0, 0.0, 1.0, 1.0, 1.0);
    append_mesh(&mut out, &shroud);
    let mut lug = box_round(
        0.009,
        0.016,
        0.007,
        BoxRoundOpts { n: 4.0, seg: 8, rows: 4, round_y: 0.4, ..BoxRoundOpts::default() },
    );
    place(&mut lug, bx, cy + 0.086, bz + 0.126, -0.50, 0.0, 0.0, 1.0, 1.0, 1.0);
    append_mesh(&mut out, &lug);

    // ARC rails: a slotted strip down each side
    for side in [-1.0f64, 1.0] {
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity(6);
        for i in 0..=5 {
            let t = f64::from(i) / 5.0;
            let a = (-0.55 + t * 1.1) * side;
            pts.push([
                bx + side * 0.114 * (a * 0.6).cos(),
                cy + 0.052 + (t * std::f64::consts::PI).sin() * 0.016,
                bz - 0.004 + a.sin() * 0.118,
            ]);
        }
        let mut rail = ribbon(
            &pts,
            0.016,
            0.009,
            &RibbonOpts {
                upright: true,
                seg: 6,
                tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut rail);
        append_mesh(&mut out, &rail);
    }

    // rear counterweight pouch
    let mut cw = box_round(
        0.058,
        0.034,
        0.026,
        BoxRoundOpts { n: 4.0, seg: 14, rows: 6, round_y: 0.5, ..BoxRoundOpts::default() },
    );
    place(&mut cw, bx, cy + 0.075, bz - 0.128, 0.28, 0.0, 0.0, 1.0, 1.0, 1.0);
    compute_normals(&mut cw);
    displace(&mut cw, |x, y, z, _, _, _, _| nz.fbm3(x * 40.0, y * 40.0, z * 40.0, 2) * 0.002);
    append_mesh(&mut out, &cw);
    out
}

/// `chinStrap(base)` (`parts.js:570-594`) — chin strap + nape pad.
pub fn chin_strap(base: [f64; 3]) -> Mesh {
    let mut out = empty_mesh();
    let (bx, by, bz) = (base[0], base[1], base[2]);
    let cy = by + 0.100;
    for side in [-1.0f64, 1.0] {
        let pts = [
            [bx + side * 0.104, cy + 0.004, bz + 0.036],
            [bx + side * 0.086, cy - 0.058, bz + 0.056],
            [bx + side * 0.048, cy - 0.104, bz + 0.062],
            [bx + side * 0.014, cy - 0.118, bz + 0.054],
        ];
        let mut s = ribbon(
            &pts,
            0.016,
            0.005,
            &RibbonOpts {
                upright: false,
                seg: 6,
                tube: TubeOpts { up: [0.0, 0.0, 1.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut s);
        append_mesh(&mut out, &s);
        let rear = [
            [bx + side * 0.106, cy + 0.000, bz - 0.024],
            [bx + side * 0.090, cy - 0.058, bz - 0.058],
            [bx + side * 0.040, cy - 0.078, bz - 0.082],
        ];
        let mut r = ribbon(
            &rear,
            0.014,
            0.005,
            &RibbonOpts {
                upright: false,
                seg: 6,
                tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut r);
        append_mesh(&mut out, &r);
    }
    out
}

/// `goggles(base, down)`'s return value (`parts.js:618`, `parts.js:641`).
///
/// Source quirk: the pushed-up variant returns `{ frame, strap }` with **no**
/// `down` key at all, which reads as falsy; only `gogglesDown` sets
/// `down: true`. Hence a plain `bool` rather than an `Option`.
#[derive(Debug, Clone, PartialEq)]
pub struct Goggles {
    pub frame: Mesh,
    pub strap: Mesh,
    pub down: bool,
}

/// `goggles(base, down)` (`parts.js:597-619`) — pushed up on the shell, or
/// pulled down over the eyes.
pub fn goggles(base: [f64; 3], down: bool) -> Goggles {
    if down {
        return goggles_down(base);
    }
    let mut frame = box_round(
        0.082,
        0.026,
        0.024,
        BoxRoundOpts { n: 3.2, seg: 20, rows: 6, round_y: 0.5, ..BoxRoundOpts::default() },
    );
    let (bx, by, bz) = (base[0], base[1], base[2]);
    place(&mut frame, bx, by + 0.176, bz + 0.098, -0.95, 0.0, 0.0, 1.0, 1.0, 1.0);
    bend_y(&mut frame, 0.15, 0.0);
    compute_normals(&mut frame);
    let mut strap = ribbon(
        &[
            [bx - 0.098, by + 0.176, bz + 0.078],
            [bx - 0.118, by + 0.198, bz - 0.020],
            [bx - 0.072, by + 0.226, bz - 0.116],
            [bx + 0.072, by + 0.226, bz - 0.116],
            [bx + 0.118, by + 0.198, bz - 0.020],
            [bx + 0.098, by + 0.176, bz + 0.078],
        ],
        0.024,
        0.007,
        &RibbonOpts { upright: true, seg: 6, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut strap);
    Goggles { frame, strap, down: false }
}

/// `gogglesDown(base)` (`parts.js:621-642`).
fn goggles_down(base: [f64; 3]) -> Goggles {
    let (bx, by, bz) = (base[0], base[1], base[2]);
    let mut frame = box_round(
        0.078,
        0.028,
        0.026,
        BoxRoundOpts { n: 3.2, seg: 20, rows: 6, round_y: 0.5, ..BoxRoundOpts::default() },
    );
    place(&mut frame, bx, by + 0.098, bz + 0.072, -0.10, 0.0, 0.0, 1.0, 1.0, 1.0);
    bend_y(&mut frame, 0.115, 0.0);
    compute_normals(&mut frame);
    let mut strap = ribbon(
        &[
            [bx - 0.084, by + 0.100, bz + 0.058],
            [bx - 0.106, by + 0.108, bz - 0.030],
            [bx - 0.062, by + 0.116, bz - 0.108],
            [bx + 0.062, by + 0.116, bz - 0.108],
            [bx + 0.106, by + 0.108, bz - 0.030],
            [bx + 0.084, by + 0.100, bz + 0.058],
        ],
        0.026,
        0.008,
        &RibbonOpts { upright: true, seg: 6, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut strap);
    Goggles { frame, strap, down: true }
}

/// `goggleLens(base, down)` (`parts.js:645-660`) — a curved slab of smoked
/// glass.
pub fn goggle_lens(base: [f64; 3], down: bool) -> Mesh {
    let (bx, by, bz) = (base[0], base[1], base[2]);
    if down {
        let mut lens = box_round(
            0.071,
            0.020,
            0.008,
            BoxRoundOpts { n: 3.0, seg: 18, rows: 5, round_y: 0.6, ..BoxRoundOpts::default() },
        );
        place(&mut lens, bx, by + 0.098, bz + 0.090, -0.10, 0.0, 0.0, 1.0, 1.0, 1.0);
        bend_y(&mut lens, 0.105, 0.0);
        compute_normals(&mut lens);
        return lens;
    }
    let mut lens = box_round(
        0.074,
        0.019,
        0.008,
        BoxRoundOpts { n: 3.0, seg: 18, rows: 5, round_y: 0.6, ..BoxRoundOpts::default() },
    );
    place(&mut lens, bx, by + 0.176, bz + 0.115, -0.95, 0.0, 0.0, 1.0, 1.0, 1.0);
    bend_y(&mut lens, 0.14, 0.0);
    compute_normals(&mut lens);
    lens
}

/// `headScarf(nz, base)` (`parts.js:667-708`) — wrapped head scarf for the
/// un-helmeted variant: a skull-hugging dome with a rolled brim and a tail
/// hanging off the back, so the silhouette reads as a fighter in a shemagh
/// rather than a bald mannequin.
pub fn head_scarf(nz: &Noise, base: [f64; 3]) -> Mesh {
    let mut out = empty_mesh();
    let (bx, by, bz) = (base[0], base[1], base[2]);
    // The skull crown sits at +0.244 in head-local space, so the dome has to
    // reach +0.250 or the bare scalp pokes through the top of the wrap.
    let mut dome = ellipsoid(
        0.102,
        0.146,
        0.112,
        EllipsoidOpts { seg: 22, rows: 12, v0: 0.34, v1: 1.0, ..EllipsoidOpts::default() },
    );
    compute_normals(&mut dome);
    place(&mut dome, bx, by + 0.104, bz - 0.008, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    displace(
        &mut dome,
        |x, y, z, _, _, _, _| {
            let f = nz.fbm3(x * 26.0, y * 22.0, z * 26.0, 3);
            f * 0.006 + (y * 70.0 + f * 4.0).sin() * 0.0022
        },
    );
    append_mesh(&mut out, &dome);
    // rolled brim
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(25);
    for i in 0..=24 {
        let a = (f64::from(i) / 24.0) * std::f64::consts::PI * 2.0;
        pts.push([
            bx + a.sin() * 0.099,
            by + 0.118 - 0.0f64.max(a.cos()) * 0.012,
            bz - 0.008 + a.cos() * 0.109,
        ]);
    }
    let mut brim = ribbon(
        &pts,
        0.030,
        0.016,
        &RibbonOpts { upright: true, seg: 7, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut brim);
    append_mesh(&mut out, &brim);
    // tail down the back
    let mut tail: Vec<[f64; 3]> = Vec::with_capacity(6);
    for i in 0..=5 {
        let t = f64::from(i) / 5.0;
        tail.push([bx + 0.028 * t, by + 0.115 - t * 0.20, bz - 0.085 - (t * 2.2).sin() * 0.03]);
    }
    let mut tl = tube(
        &tail,
        |t, _i| super_ellipse(0.052 - t * 0.012, 0.020 + t * 0.006, 3.0, 12, 0.0),
        &TubeOpts {
            up: [0.0, 0.0, 1.0],
            frames: None,
            loft: LoftOpts { closed: true, cap_start: false, cap_end: true },
        },
    );
    compute_normals(&mut tl);
    displace(&mut tl, |x, y, z, _, _, _, _| nz.fbm3(x * 30.0, y * 26.0, z * 30.0, 3) * 0.006);
    append_mesh(&mut out, &tl);
    out
}

/* ================================================================== */
/* Load-bearing gear                                                  */
/* ================================================================== */

/// `plate(hx, hy, hz, y, z, tilt, radius)` (`parts.js:715-728`) — one plate:
/// a curved slab with a soft edge.
fn plate(hx: f64, hy: f64, hz: f64, y: f64, z: f64, tilt: f64, radius: f64) -> Mesh {
    let mut m = box_round(
        hx,
        hy,
        hz,
        BoxRoundOpts { n: 3.6, seg: 22, rows: 11, round_y: 0.24, ..BoxRoundOpts::default() },
    );
    // taper: a real plate narrows toward the waist and wraps in at the bottom
    warp(
        &mut m,
        |v, _i| {
            let t = 0.0f64.max(-v.y / hy);
            v.x *= 1.0 - 0.20 * t * t;
            v.z *= 1.0 - 0.35 * t * t;
        },
    );
    compute_normals(&mut m);
    place(&mut m, 0.0, y, z, tilt, 0.0, 0.0, 1.0, 1.0, 1.0);
    bend_y(&mut m, radius, z);
    compute_normals(&mut m);
    m
}

/// `pouch`'s `o` bag (`parts.js:731-757`).
///
/// `lid_tilt` and `bend` are `f64`, not `Option<f64>`, because the source
/// tests them for *truthiness* (`o.lidTilt ? … : …`, `if (o.bend)`), under
/// which an absent key and a literal `0` behave identically. `soldier.js`
/// passes a literal `lidTilt: 0` for two of the three mag pouches, so both
/// arms are real call sites.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PouchOpts {
    pub hx: f64,
    pub hy: f64,
    pub hz: f64,
    pub lid_tilt: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub rx: f64,
    pub ry: f64,
    pub rz: f64,
    pub bend: f64,
}

impl Default for PouchOpts {
    fn default() -> Self {
        PouchOpts {
            hx: 0.038,
            hy: 0.055,
            hz: 0.030,
            lid_tilt: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
            bend: 0.0,
        }
    }
}

/// `pouch(nz, o)` (`parts.js:731-760`) — a rounded box with a lid, a pull tab
/// and compression stitching.
pub fn pouch(nz: &Noise, o: &PouchOpts) -> Mesh {
    let mut out = empty_mesh();
    let (hx, hy, hz) = (o.hx, o.hy, o.hz);
    let mut body = box_round(
        hx,
        hy,
        hz,
        BoxRoundOpts { n: 5.5, seg: 18, rows: 8, round_y: 0.18, ..BoxRoundOpts::default() },
    );
    compute_normals(&mut body);
    displace(&mut body, |x, y, z, _, _, _, _| nz.fbm3(x * 40.0, y * 40.0, z * 40.0, 3) * 0.0022);
    append_mesh(&mut out, &body);
    // lid
    let mut lid = box_round(
        hx * 1.03,
        0.010,
        hz * 0.98,
        BoxRoundOpts { n: 5.5, seg: 18, rows: 4, round_y: 0.5, ..BoxRoundOpts::default() },
    );
    place(
        &mut lid,
        0.0,
        hy - 0.004,
        (if o.lid_tilt == 0.0 { 0.0 } else { hz * 0.35 }) + hz * 0.10,
        o.lid_tilt - 0.18,
        0.0,
        0.0,
        1.0,
        1.0,
        1.0,
    );
    compute_normals(&mut lid);
    append_mesh(&mut out, &lid);
    // pull tab
    let mut tab = ribbon(
        &[
            [0.0, hy + 0.004, hz * 0.7],
            [0.0, hy - 0.010, hz * 1.16],
            [0.0, hy - 0.034, hz * 1.10],
        ],
        0.014,
        0.004,
        &RibbonOpts { upright: false, seg: 5, tube: TubeOpts { up: [1.0, 0.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut tab);
    append_mesh(&mut out, &tab);
    place(&mut out, o.x, o.y, o.z, o.rx, o.ry, o.rz, 1.0, 1.0, 1.0);
    if o.bend != 0.0 {
        bend_y(&mut out, o.bend, o.z);
    }
    compute_normals(&mut out);
    out
}

/// `plateCarrier(nz, p)` (`parts.js:763-798`) — front & back plates,
/// cummerbund, shoulder straps, buckles.
///
/// The source's trailing `p = {}` is never read; see the module doc.
pub fn plate_carrier(nz: &Noise) -> Mesh {
    let mut out = empty_mesh();
    let mut front = plate(0.152, 0.140, 0.030, 1.298, 0.126, -0.05, 0.20);
    displace(&mut front, |x, y, z, _, _, _, _| nz.fbm3(x * 34.0, y * 34.0, z * 34.0, 3) * 0.0026);
    append_mesh(&mut out, &front);
    let mut back = plate(0.154, 0.148, 0.026, 1.300, -0.116, 0.05, 0.21);
    displace(&mut back, |x, y, z, _, _, _, _| nz.fbm3(x * 34.0, y * 34.0, z * 34.0, 3) * 0.0026);
    append_mesh(&mut out, &back);

    // cummerbund wrapping the waist
    let n = 26usize;
    let mut cb: Vec<[f64; 3]> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = (i as f64 / n as f64) * std::f64::consts::PI * 2.0;
        cb.push([a.sin() * 0.168, 1.152 + (a * 2.0).cos() * 0.005, a.cos() * 0.121 - 0.004]);
    }
    let mut band = ribbon(
        &cb,
        0.100,
        0.022,
        &RibbonOpts { upright: true, seg: 8, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut band);
    displace(&mut band, |x, y, z, _, _, _, _| nz.fbm3(x * 34.0, y * 34.0, z * 34.0, 3) * 0.002);
    append_mesh(&mut out, &band);

    // shoulder straps
    for side in [-1.0f64, 1.0] {
        let pts = [
            [side * 0.082, 1.418, 0.144],
            [side * 0.100, 1.468, 0.040],
            [side * 0.104, 1.462, -0.036],
            [side * 0.092, 1.418, -0.120],
        ];
        let mut s = ribbon(
            &pts,
            0.076,
            0.030,
            &RibbonOpts {
                upright: false,
                seg: 8,
                tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut s);
        displace(&mut s, |x, y, z, _, _, _, _| nz.fbm3(x * 34.0, y * 34.0, z * 34.0, 3) * 0.002);
        append_mesh(&mut out, &s);
    }
    out
}

/// `carrierWebbing()` (`parts.js:801-831`) — drag handle, elastic retention,
/// admin panel loops.
pub fn carrier_webbing() -> Mesh {
    let mut out = empty_mesh();
    // PALS rows across the front plate
    for r in 0..2 {
        let y = 1.322 + f64::from(r) * 0.046;
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity(9);
        for i in 0..=8 {
            let t = f64::from(i) / 8.0;
            let x = (t - 0.5) * 0.150;
            pts.push([x, y, 0.150 - (x * x) / 0.20]);
        }
        let mut row = ribbon(
            &pts,
            0.013,
            0.0035,
            &RibbonOpts {
                upright: true,
                seg: 5,
                tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut row);
        append_mesh(&mut out, &row);
    }
    // drag handle on the back
    let mut drag = ribbon(
        &[
            [-0.052, 1.432, -0.132],
            [-0.022, 1.458, -0.152],
            [0.022, 1.458, -0.152],
            [0.052, 1.432, -0.132],
        ],
        0.028,
        0.010,
        &RibbonOpts { upright: true, seg: 6, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut drag);
    append_mesh(&mut out, &drag);
    out
}

/// `sling(gripPoint, stockPoint)` (`parts.js:834-848`) — a two-point sling
/// routed across the chest.
pub fn sling(grip_point: [f64; 3], stock_point: [f64; 3]) -> Mesh {
    let pts = [
        [stock_point[0], stock_point[1] + 0.02, stock_point[2]],
        [-0.130, 1.395, -0.010],
        [-0.120, 1.430, -0.090],
        [0.020, 1.430, -0.118],
        [0.120, 1.330, -0.070],
        [0.150, 1.250, 0.040],
        [0.110, 1.235, 0.135],
        [grip_point[0] + 0.02, grip_point[1] + 0.03, grip_point[2] + 0.02],
    ];
    let mut m = ribbon(
        &pts,
        0.032,
        0.009,
        &RibbonOpts { upright: false, seg: 6, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut m);
    m
}

/// `belt(nz)` (`parts.js:851-864`) — belt with a buckle and a holster.
pub fn belt(nz: &Noise) -> Mesh {
    let mut out = empty_mesh();
    let n = 24usize;
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = (i as f64 / n as f64) * std::f64::consts::PI * 2.0;
        pts.push([a.sin() * 0.158, 0.902, a.cos() * 0.113 - 0.008]);
    }
    let mut b = ribbon(
        &pts,
        0.056,
        0.018,
        &RibbonOpts { upright: true, seg: 7, tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() } },
    );
    compute_normals(&mut b);
    displace(&mut b, |x, y, z, _, _, _, _| nz.fbm3(x * 40.0, y * 40.0, z * 40.0, 2) * 0.0018);
    append_mesh(&mut out, &b);
    out
}

/// `hipPouch(nz, side)` (`parts.js:867-874`) — dump pouch / canteen hanging
/// off the belt at the back.
pub fn hip_pouch(nz: &Noise, side: f64) -> Mesh {
    pouch(
        nz,
        &PouchOpts {
            hx: 0.048,
            hy: 0.062,
            hz: 0.038,
            x: side * 0.142,
            y: 0.878,
            z: -0.070,
            rz: side * 0.12,
            ry: side * 0.5,
            ..PouchOpts::default()
        },
    )
}

/// `kneePad(nz, knee, side)` (`parts.js:877-898`) — a curved cap with two
/// elastic straps. `side` is accepted and never read by the source.
pub fn knee_pad(nz: &Noise, knee: [f64; 3], _side: f64) -> Mesh {
    let mut out = empty_mesh();
    let mut cap = box_round(
        0.064,
        0.080,
        0.026,
        BoxRoundOpts { n: 4.5, seg: 18, rows: 9, round_y: 0.42, ..BoxRoundOpts::default() },
    );
    place(&mut cap, 0.0, 0.0, 0.052, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    bend_y(&mut cap, 0.075, 0.052);
    compute_normals(&mut cap);
    displace(&mut cap, |x, y, z, _, _, _, _| nz.fbm3(x * 60.0, y * 60.0, z * 60.0, 3) * 0.0018);
    append_mesh(&mut out, &cap);
    for dy in [-0.056f64, 0.052] {
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity(15);
        for i in 0..=14 {
            let a = (f64::from(i) / 14.0) * std::f64::consts::PI * 2.0;
            pts.push([a.sin() * 0.066, dy, a.cos() * 0.058 + 0.006]);
        }
        let mut s = ribbon(
            &pts,
            0.016,
            0.006,
            &RibbonOpts {
                upright: true,
                seg: 6,
                tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut s);
        append_mesh(&mut out, &s);
    }
    place(&mut out, knee[0], knee[1] + 0.012, knee[2] + 0.004, 0.06, 0.0, 0.0, 1.0, 1.0, 1.0);
    compute_normals(&mut out);
    out
}

/* ================================================================== */
/* Boots, gloves                                                      */
/* ================================================================== */

/// `boot(nz, ankle, side)` (`parts.js:905-943`) — sole, upper, ankle cuff,
/// tongue and laces. `ankle` is the FootR/L bone. `side` is accepted and
/// never read by the source.
pub fn boot(nz: &Noise, ankle: [f64; 3], _side: f64) -> Mesh {
    let mut out = empty_mesh();
    let (ax, ay, az) = (ankle[0], ankle[1], ankle[2]);
    // upper: lofted sections front to back
    let s: [[f64; 4]; 7] = [
        [-0.078, 0.036, 0.030, 0.052],
        [-0.052, 0.044, 0.038, 0.062],
        [-0.016, 0.048, 0.044, 0.058],
        [0.030, 0.049, 0.046, 0.048],
        [0.076, 0.046, 0.042, 0.038],
        [0.112, 0.040, 0.034, 0.030],
        [0.134, 0.028, 0.022, 0.024],
    ];
    let seg = 18;
    let q = quat_from_axis_angle(V3::new(1.0, 0.0, 0.0), std::f64::consts::PI / 2.0);
    let rings: Vec<Ring> = s
        .iter()
        .map(|&[z, hx, hy, cy]| Ring {
            pts: super_ellipse(hx, hy, 2.8, seg, 0.0),
            o: Some([ax, ay - 0.088 + cy, az + z]),
            q: Some(q),
            ..Ring::default()
        })
        .collect();
    let mut upper = loft(&rings, LoftOpts { closed: true, cap_start: true, cap_end: true });
    compute_normals(&mut upper);
    displace(&mut upper, |x, y, z, _, _, _, _| nz.fbm3(x * 44.0, y * 44.0, z * 44.0, 3) * 0.0022);
    append_mesh(&mut out, &upper);

    // ankle cuff up the shin
    let mut cuff = tube(
        &[
            [ax, ay + 0.010, az - 0.004],
            [ax, ay + 0.070, az - 0.002],
            [ax, ay + 0.125, az + 0.002],
        ],
        |t, _i| ellipse_profile(0.056 - 0.004 * t, 0.050 - 0.002 * t, 16, 0.0),
        &TubeOpts::default(),
    );
    compute_normals(&mut cuff);
    displace(&mut cuff, |x, y, z, _, _, _, _| nz.fbm3(x * 44.0, y * 44.0, z * 44.0, 3) * 0.0025);
    append_mesh(&mut out, &cuff);
    out
}

/// `bootSole(ankle)` (`parts.js:946-970`) — boot sole + heel block, rubber.
pub fn boot_sole(ankle: [f64; 3]) -> Mesh {
    let s: [[f64; 3]; 7] = [
        [-0.082, 0.033, 0.018],
        [-0.055, 0.043, 0.020],
        [-0.020, 0.047, 0.014],
        [0.030, 0.049, 0.013],
        [0.080, 0.046, 0.013],
        [0.118, 0.038, 0.013],
        [0.140, 0.024, 0.012],
    ];
    let (ax, ay, az) = (ankle[0], ankle[1], ankle[2]);
    let q = quat_from_axis_angle(V3::new(1.0, 0.0, 0.0), std::f64::consts::PI / 2.0);
    let rings: Vec<Ring> = s
        .iter()
        .map(|&[z, hx, hy]| Ring {
            pts: super_ellipse(hx, hy, 3.6, 16, 0.0),
            o: Some([ax, ay - 0.088 + hy + 0.001, az + z]),
            q: Some(q),
            ..Ring::default()
        })
        .collect();
    let mut m = loft(&rings, LoftOpts { closed: true, cap_start: true, cap_end: true });
    compute_normals(&mut m);
    // heel block
    let mut heel = box_round(
        0.036,
        0.011,
        0.030,
        BoxRoundOpts { n: 4.0, seg: 12, rows: 4, round_y: 0.4, ..BoxRoundOpts::default() },
    );
    place(&mut heel, ax, ay - 0.082, az - 0.056, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    append_mesh(&mut m, &heel);
    compute_normals(&mut m);
    m
}

/// `bootLaces(ankle)` (`parts.js:973-995`) — cross-over ribbons up the boot
/// tongue.
pub fn boot_laces(ankle: [f64; 3]) -> Mesh {
    let mut out = empty_mesh();
    let (ax, ay, az) = (ankle[0], ankle[1], ankle[2]);
    for i in 0..5 {
        let t = f64::from(i) / 4.0;
        let z = az + 0.088 - t * 0.076;
        let y = ay - 0.028 + t * 0.070;
        let w = 0.030 - t * 0.004;
        let mut s = ribbon(
            &[
                [ax - w, y - 0.006, z + 0.006],
                [ax, y + 0.004, z],
                [ax + w, y - 0.006, z + 0.006],
            ],
            0.008,
            0.004,
            &RibbonOpts {
                upright: false,
                seg: 5,
                tube: TubeOpts { up: [0.0, 1.0, 0.0], ..TubeOpts::default() },
            },
        );
        compute_normals(&mut s);
        append_mesh(&mut out, &s);
    }
    out
}

/// `glove(nz, wrist, gripAxis, palmNormal, side)` (`parts.js:1001-1059`) — a
/// gloved hand curled around a grip. `wrist` is the hand bone position,
/// `grip_axis` the grip axis, `palm_normal` the direction out of the palm.
pub fn glove(nz: &Noise, wrist: [f64; 3], grip_axis: [f64; 3], palm_normal: [f64; 3], side: f64) -> Mesh {
    let mut out = empty_mesh();
    let w = V3::from_array(wrist);
    let a = V3::from_array(grip_axis).normalize_or_zero(); // along the grip
    let n = V3::from_array(palm_normal).normalize_or_zero(); // out of the palm
    let s = a.cross(n).normalize_or_zero(); // across the hand

    // palm block
    let mut palm = box_round(
        0.030,
        0.048,
        0.022,
        BoxRoundOpts { n: 3.2, seg: 16, rows: 7, round_y: 0.4, ..BoxRoundOpts::default() },
    );
    let pos = w.add_scaled(a, 0.030).add_scaled(n, -0.006);
    let m = basis_at(s, a, n, pos);
    compute_normals(&mut palm);
    transform_mesh(&mut palm, &m);
    append_mesh(&mut out, &palm);

    // finger mass: a tube curling around the grip axis
    for f in 0..4 {
        let t = f64::from(f) / 3.0;
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity(5);
        let start_y = 0.052 - t * 0.030;
        for i in 0..=4 {
            let u = f64::from(i) / 4.0;
            let ang = u * 2.2;
            let r = 0.030 - u * 0.004;
            let p = w
                .add_scaled(a, start_y - 0.004 + ang.sin() * r * 0.55)
                .add_scaled(n, -0.020 - (1.0 - ang.cos()) * r * 0.9)
                .add_scaled(s, side * (0.020 - t * 0.019));
            pts.push([p.x, p.y, p.z]);
        }
        let mut fin = tube(
            &pts,
            |u, _i| ellipse_profile(0.0115 - u * 0.002, 0.0105 - u * 0.002, 10, 0.0),
            &TubeOpts {
                up: [0.0, 0.0, 1.0],
                frames: None,
                loft: LoftOpts { closed: true, cap_start: true, cap_end: true },
            },
        );
        compute_normals(&mut fin);
        append_mesh(&mut out, &fin);
    }
    // thumb across the top
    let mut tp: Vec<[f64; 3]> = Vec::with_capacity(5);
    for i in 0..=4 {
        let u = f64::from(i) / 4.0;
        let p = w
            .add_scaled(a, 0.030 + u * 0.036)
            .add_scaled(n, 0.006 - u * 0.026)
            .add_scaled(s, side * (-0.026 - u * 0.004));
        tp.push([p.x, p.y, p.z]);
    }
    let mut thumb = tube(
        &tp,
        |u, _i| ellipse_profile(0.014 - u * 0.003, 0.013 - u * 0.003, 10, 0.0),
        &TubeOpts {
            up: [0.0, 0.0, 1.0],
            frames: None,
            loft: LoftOpts { closed: true, cap_start: true, cap_end: true },
        },
    );
    compute_normals(&mut thumb);
    append_mesh(&mut out, &thumb);

    compute_normals(&mut out);
    displace(&mut out, |x, y, z, _, _, _, _| nz.fbm3(x * 90.0, y * 90.0, z * 90.0, 3) * 0.0012);
    out
}

/// `knuckleGuard(wrist, gripAxis, palmNormal)` (`parts.js:1062-1073`) — the
/// knuckle guard on the back of the glove.
pub fn knuckle_guard(wrist: [f64; 3], grip_axis: [f64; 3], palm_normal: [f64; 3]) -> Mesh {
    let w = V3::from_array(wrist);
    let a = V3::from_array(grip_axis).normalize_or_zero();
    let n = V3::from_array(palm_normal).normalize_or_zero();
    let s = a.cross(n).normalize_or_zero();
    let mut g = box_round(
        0.026,
        0.024,
        0.007,
        BoxRoundOpts { n: 3.4, seg: 14, rows: 5, round_y: 0.5, ..BoxRoundOpts::default() },
    );
    let m = basis_at(s, a, n, w.add_scaled(a, 0.050).add_scaled(n, 0.020));
    compute_normals(&mut g);
    transform_mesh(&mut g, &m);
    g
}
