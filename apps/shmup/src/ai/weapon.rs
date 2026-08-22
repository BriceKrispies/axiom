//! Ported from Claude-of-Duty `src/ai/weapon.js:1-291`.
//!
//! AI — the enemy's weapon, modelled in the weapon's own frame and then baked
//! into the character mesh rigidly bound to the firing hand. That keeps the
//! whole soldier, rifle included, at one draw call per material.
//!
//! Weapon frame: origin at the pistol grip (the firing wrist), +Z down the
//! bore, +Y up, +X the shooter's left. Bore line sits 0.095 m above the origin
//! ([`BORE_Y`]).
//!
//! ## What this file is, and what it is not
//!
//! Despite living next to [`super::agent`], `weapon.js` carries **no firing
//! logic**: no muzzle-flash / tracer / shell-eject events, no ballistics, no
//! ammunition, no `EventBus` contact. It is a pure, deterministic geometry
//! builder that returns four material meshes plus six named anchor points in
//! the actor's bind space. `agent.js`'s "muzzle-flash/tracer/shell events
//! fired through `weapon.js`'s facade" (quoted in [`super::agent`]'s module
//! doc) actually means `_fireRound` reading the anchors below — `W.muzzle`,
//! `W.ejection` — and handing them to `fx/`. Those events belong to
//! `ai/index.js` and `agent.js`'s `_shoot`/`_fireRound`, both of which are
//! still deferred; nothing in `crate::fx` or `crate::weapons` is consumed
//! here, because the source consumes nothing from them here either.
//!
//! The player's *viewmodel* weapon is a completely separate model family
//! (`src/weapons/`, ported in [`crate::weapons`]). This one is the enemy's,
//! authored to a different budget and in a different frame; they share no
//! code in the source and share none here.
//!
//! ## Seam — the `ai/geo` and `ai/rig` API this module assumes
//!
//! `weapon.js` is a leaf consumer of `ai/geo.js` (the procedural geometry
//! toolkit) and two constants from `ai/rig.js`. Both are separate slices. The
//! names imported below are the direct Rust translations of the JavaScript
//! ones; every assumption is listed in
//! `docs/work-manifests/shmup-port/notes/ai-weapon.md`. If a name lands
//! differently, the whole seam is the single `use super::geo::{…}` block at
//! the top of this file.
//!
//! ## Determinism
//!
//! One `rng` draw, at `weapon.js:263`: `rng.range(-0.10, -0.03)`, the cant
//! angle. Nothing else in the builder is random — the surface variation all
//! comes from the caller's shared [`Noise`] field. When `rng` is absent the
//! source substitutes the literal `-0.06`; [`build_weapon`] takes
//! `Option<&mut Rng>` and preserves that branch exactly.
//!
//! This is app code (`apps/`), outside the Branchless Law — the ports below
//! use plain `if`/`for` where that is what the source says.

use crate::rng::Rng;

use super::geo::{
    append_mesh, box_round, compute_normals, displace, ellipse_profile, empty_mesh, loft, ribbon,
    super_ellipse, transform_mesh, tube, warp, BoxRoundOpts, LoftOpts, Mesh, Noise, Q, Ring,
    RibbonOpts, TubeOpts, V3, M4,
};
use super::rig::{BORE_DIR, GRIP_R};

/// `BORE_Y`. `weapon.js:17`.
pub const BORE_Y: f64 = 0.095;

/// `weapon.js:55-58`'s `style`. The source takes a string and derives one
/// boolean from it — `const long = style === 'ak'` (`weapon.js:67`) — so any
/// value that is not exactly `'ak'` is a carbine. Two variants say the same
/// thing without an unrepresentable third state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeaponStyle {
    /// `'carbine'` — flat-top 5.56 carbine, short optic, collapsible stock.
    #[default]
    Carbine,
    /// `'ak'` — long-stroke rifle, iron sights, side-folding wire stock.
    Ak,
}

impl WeaponStyle {
    /// `soldier.js`'s `VARIANTS[].weapon` is the source's own string
    /// (`'carbine'` / `'ak'`), and `weapon.js:67` keys off it with
    /// `style === 'ak'`. Anything that is not `'ak'` is the carbine, exactly
    /// as that comparison behaves — an unknown string is not an error in the
    /// source and is not one here.
    pub fn from_name(name: &str) -> Self {
        match name {
            "ak" => WeaponStyle::Ak,
            _ => WeaponStyle::Carbine,
        }
    }

    /// `const long = style === 'ak'`. `weapon.js:67`.
    fn long(self) -> bool {
        self == WeaponStyle::Ak
    }
}

/// The return value of [`build_weapon`] — `weapon.js:277-290`. Every mesh is
/// already transformed into the actor's bind space, and every anchor is a
/// bind-space point.
#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    /// `steel`.
    pub steel: Mesh,
    /// `polymer` (the source's local is `poly`).
    pub polymer: Mesh,
    /// `rubber`.
    pub rubber: Mesh,
    /// `glass`. Empty for [`WeaponStyle::Ak`], which has iron sights and no
    /// optic — `soldier.js:718` guards on `W.glass.p.length` for exactly that
    /// reason.
    pub glass: Mesh,
    /// The bake matrix, `weapon.js:266-267`.
    pub matrix: M4,
    /// `toBind(0, BORE_Y, barrelEnd + 0.012)`.
    pub muzzle: [f64; 3],
    /// `toBind(0, BORE_Y, 0)`.
    pub bore_origin: [f64; 3],
    /// `toBind(-0.024, BORE_Y + 0.012, 0.012)`.
    pub ejection: [f64; 3],
    /// `toBind(0, BORE_Y, -0.10)`.
    pub stock_top: [f64; 3],
    /// `toBind(0, BORE_Y - 0.028, long ? 0.22 : 0.205)`.
    pub foregrip: [f64; 3],
    /// `toBind(0, BORE_Y - 0.25, 0.03)`.
    pub mag_bottom: [f64; 3],
}

/* ------------------------------------------------------------------ */
/* Local helpers — `weapon.js:19-50`                                   */
/* ------------------------------------------------------------------ */

/// `box()`'s `opts` bag, `weapon.js:20-29`. The defaults are the source's
/// `??` fallbacks: `n = 3.6`, `seg = 16`, `rows = 7`, `roundY = 0.3`,
/// `rx/ry/rz = 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BoxOpts {
    n: f64,
    seg: usize,
    rows: usize,
    round_y: f64,
    rx: f64,
    ry: f64,
    rz: f64,
}

impl BoxOpts {
    const DEFAULT: BoxOpts = BoxOpts {
        n: 3.6,
        seg: 16,
        rows: 7,
        round_y: 0.3,
        rx: 0.0,
        ry: 0.0,
        rz: 0.0,
    };
}

/// `Quaternion.setFromEuler(new Euler(x, y, z, 'YXZ'))` —
/// `three/src/math/Quaternion.js`'s `case 'YXZ'` branch (MIT, Three.js
/// authors).
///
/// **Euler order is a convention, not a spelling.** `'YXZ'` is *not*
/// `'XYZ'`: the `z` and `w` terms carry the opposite sign. Nothing in
/// `weapon.js` ever passes a non-zero `rx`/`ry`/`rz` — every `box()` call
/// site (`weapon.js:71,73,135,139,143,147,152,153,156,160,216,217,221,229,230,
/// 236,237,240,255,256`) leaves them at the `?? 0` default, so this always
/// returns identity in practice. It is ported anyway: "dead computation in
/// the source is still part of the source", and the judgement that it is dead
/// can be wrong.
fn quat_from_euler_yxz(x: f64, y: f64, z: f64) -> Q {
    let (c1, c2, c3) = ((x * 0.5).cos(), (y * 0.5).cos(), (z * 0.5).cos());
    let (s1, s2, s3) = ((x * 0.5).sin(), (y * 0.5).sin(), (z * 0.5).sin());
    Q {
        x: s1 * c2 * c3 + c1 * s2 * s3,
        y: c1 * s2 * c3 - s1 * c2 * s3,
        z: c1 * c2 * s3 - s1 * s2 * c3,
        w: c1 * c2 * c3 + s1 * s2 * s3,
    }
}

/// `Quaternion.setFromAxisAngle(axis, angle)` — `Quaternion.js`. The axis is
/// assumed normalized, exactly as Three assumes.
fn quat_from_axis_angle(axis: [f64; 3], angle: f64) -> Q {
    let half_angle = angle / 2.0;
    let s = half_angle.sin();
    Q {
        x: axis[0] * s,
        y: axis[1] * s,
        z: axis[2] * s,
        w: half_angle.cos(),
    }
}

/// `Vector3.applyQuaternion(q)` — `three/src/math/Vector3.js:467-486`,
/// transcribed with the source's exact left-to-right grouping (float addition
/// is not associative, so `vx + qw*tx + qy*tz - qz*ty` may **not** be folded
/// into `vx + qw*tx + (qy*tz - qz*ty)`).
fn apply_quat(v: [f64; 3], q: Q) -> [f64; 3] {
    let (vx, vy, vz) = (v[0], v[1], v[2]);
    let (qx, qy, qz, qw) = (q.x, q.y, q.z, q.w);
    // t = 2 * cross( q.xyz, v );
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    // v + q.w * t + cross( q.xyz, t );
    [
        vx + qw * tx + qy * tz - qz * ty,
        vy + qw * ty + qz * tx - qx * tz,
        vz + qw * tz + qx * ty - qy * tx,
    ]
}

/// `Vector3.crossVectors(a, b)` — and `a.cross(b)`, which is the same thing
/// with `a` as `this`.
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `Vector3.normalize()`: `divideScalar(length() || 1)` — the `|| 1` keeps a
/// zero vector at zero instead of producing `NaN`.
fn normalize(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let d = if len == 0.0 { 1.0 } else { len };
    [v[0] / d, v[1] / d, v[2] / d]
}

/// `Matrix4.compose(position, quaternion, scale)` — `Matrix4.js:1001-1035`.
/// Column-major `elements`, Three's layout: `e[col * 4 + row]`.
fn mat4_compose(position: [f64; 3], q: Q, scale: [f64; 3]) -> M4 {
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

/// `Matrix4.makeBasis(xAxis, yAxis, zAxis)` — `Matrix4.js:253-264`. The three
/// axes become the matrix **columns**; writing them as rows would flip every
/// off-diagonal sign.
fn mat4_make_basis(x: [f64; 3], y: [f64; 3], z: [f64; 3]) -> M4 {
    M4 {
        e: [
            x[0], x[1], x[2], 0.0, //
            y[0], y[1], y[2], 0.0, //
            z[0], z[1], z[2], 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    }
}

/// `Matrix4.setPosition(x, y, z)` — `Matrix4.js:688-706`.
fn mat4_set_position(m: &mut M4, x: f64, y: f64, z: f64) {
    m.e[12] = x;
    m.e[13] = y;
    m.e[14] = z;
}

/// `Vector3.applyMatrix4(m)` — `three/src/math/Vector3.js`. Full affine with
/// the perspective divide; for the matrices here `w` is exactly `1`, and
/// multiplying by `1.0` is exact, so the divide is a no-op in value but is
/// kept because the source keeps it.
fn apply_matrix4(v: [f64; 3], m: &M4) -> [f64; 3] {
    let (x, y, z) = (v[0], v[1], v[2]);
    let e = &m.e;
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    [
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    ]
}

/// `box(hx, hy, hz, x, y, z, opts)` — `weapon.js:20-33`. A rounded box
/// positioned in weapon space.
///
/// Named `box_at` because `box` is a reserved word in Rust.
fn box_at(hx: f64, hy: f64, hz: f64, x: f64, y: f64, z: f64, opts: BoxOpts) -> Mesh {
    let mut m = box_round(
        hx,
        hy,
        hz,
        BoxRoundOpts {
            n: opts.n,
            seg: opts.seg,
            rows: opts.rows,
            loft: LoftOpts::default(),
            round_y: opts.round_y,
            // `ny` is never passed by `box()`, so it stays at `boxRound`'s own
            // `?? 5` default (`geo.js:304`).
            ny: 5.0,
        },
    );
    let q = quat_from_euler_yxz(opts.rx, opts.ry, opts.rz);
    compute_normals(&mut m);
    transform_mesh(&mut m, &mat4_compose([x, y, z], q, [1.0, 1.0, 1.0]));
    m
}

/// `cyl(r0, r1, z0, z1, x, y, seg, cap)` — `weapon.js:36-50`. A cylinder
/// along +Z in weapon space, lofted over five path points.
fn cyl(r0: f64, r1: f64, z0: f64, z1: f64, x: f64, y: f64, seg: usize, cap: bool) -> Mesh {
    let n = 5;
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(n);
    for i in 0..n {
        // `z0 + ((z1 - z0) * i) / (n - 1)` — the multiply happens before the
        // divide in the source; do not reorder it.
        pts.push([x, y, z0 + ((z1 - z0) * i as f64) / (n - 1) as f64]);
    }
    let mut m = tube(
        &pts,
        |t, _i| {
            let r = r0 + (r1 - r0) * t;
            ellipse_profile(r, r, seg, 0.0)
        },
        &TubeOpts {
            up: [0.0, 1.0, 0.0],
            loft: LoftOpts { closed: true, cap_start: cap, cap_end: cap },
            ..Default::default()
        },
    );
    compute_normals(&mut m);
    m
}

/* ------------------------------------------------------------------ */

/// The bake basis, `weapon.js:258-267`.
///
/// Factored out of [`build_weapon`] only because it is the one part of the
/// builder that does not touch `ai/geo` at all: it is pure `rig` constants
/// plus the single `rng` draw, so it can be pinned against the golden without
/// the geometry toolkit. `build_weapon` calls it at exactly the point the
/// source does — *after* all the geometry, which is also the only point at
/// which the stream is touched.
///
/// The cant is `rng.range(-0.10, -0.03)`, or the literal `-0.06` when no
/// `rng` is supplied (`weapon.js:263`); `soldier.js:704` always supplies one,
/// `selftest.mjs:73` supplies `rng.fork()`.
pub fn bind_matrix(rng: Option<&mut Rng>) -> M4 {
    let z = normalize(*BORE_DIR);
    let x = normalize(cross([0.0, 1.0, 0.0], z));
    let y = normalize(cross(z, x));
    // a few degrees of cant so nothing is perfectly upright
    let cant = quat_from_axis_angle(z, match rng {
        Some(r) => r.range(-0.10, -0.03),
        None => -0.06,
    });
    // Note the source normalizes `x` and `y` *before* the cant and does not
    // renormalize afterwards.
    let x = apply_quat(x, cant);
    let y = apply_quat(y, cant);
    let mut m = mat4_make_basis(x, y, z);
    mat4_set_position(&mut m, GRIP_R[0], GRIP_R[1], GRIP_R[2]);
    m
}

/// `buildWeapon(nz, style, rng)` — `weapon.js:61-291`.
///
/// Builds a weapon and bakes it into the actor's bind space. `nz` is the
/// character builder's shared [`Noise`] field (`selftest.mjs:20`,
/// `soldier.js:704`), used only for surface displacement.
pub fn build_weapon(nz: &Noise, style: WeaponStyle, rng: Option<&mut Rng>) -> Weapon {
    let mut steel = empty_mesh();
    let mut poly = empty_mesh();
    let mut rubber = empty_mesh();
    let mut glass = empty_mesh();

    let long = style.long();
    let barrel_end = if long { 0.50 } else { 0.435 };

    /* ---- lower receiver + magwell + grip (polymer) ---- */
    append_mesh(
        &mut poly,
        &box_at(
            0.019,
            0.031,
            0.055,
            0.0,
            BORE_Y - 0.036,
            -0.012,
            BoxOpts { n: 4.2, round_y: 0.22, ..BoxOpts::DEFAULT },
        ),
    );
    // magwell
    append_mesh(
        &mut poly,
        &box_at(
            0.0165,
            0.030,
            0.028,
            0.0,
            BORE_Y - 0.052,
            0.004,
            BoxOpts { n: 4.5, round_y: 0.18, ..BoxOpts::DEFAULT },
        ),
    );
    // trigger guard
    {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for i in 0..=8 {
            let t = i as f64 / 8.0;
            let a = std::f64::consts::PI * t;
            pts.push([
                0.0,
                BORE_Y - 0.068 - a.sin() * 0.020,
                -0.028 + a.cos() * -0.024 + 0.024,
            ]);
        }
        let mut g = ribbon(
            &pts,
            0.014,
            0.006,
            &RibbonOpts {
                seg: 5,
                upright: false,
                tube: TubeOpts { up: [1.0, 0.0, 0.0], ..Default::default() },
            },
        );
        compute_normals(&mut g);
        append_mesh(&mut poly, &g);
    }
    // pistol grip: tapered, raked back 22 degrees
    {
        let rake = -0.38;
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for i in 0..6 {
            let t = i as f64 / 5.0;
            pts.push([
                0.0,
                BORE_Y - 0.052 - t * 0.105,
                // `-0.028 - Math.sin(rake) * -t * 0.105 * 0.55 - t * 0.030`:
                // left-to-right, exactly as written. Do not tidy the double
                // negation into an addition.
                -0.028 - f64::sin(rake) * -t * 0.105 * 0.55 - t * 0.030,
            ]);
        }
        let mut g = tube(
            &pts,
            |t, _i| super_ellipse(0.0165 - t * 0.002, 0.020 - t * 0.004, 3.4, 14, 0.0),
            &TubeOpts {
                up: [0.0, 0.0, 1.0],
                loft: LoftOpts { closed: true, cap_start: true, cap_end: true },
                ..Default::default()
            },
        );
        compute_normals(&mut g);
        // finger grooves
        displace(&mut g, |x, y, z, _nx, _ny, _nzz, _i| {
            f64::sin((y - BORE_Y) * 150.0) * 0.0012
                + nz.fbm3(x * 90.0, y * 90.0, z * 90.0, 2) * 0.0012
        });
        append_mesh(&mut poly, &g);
    }

    /* ---- magazine ---- */
    {
        let mut rings: Vec<Ring> = Vec::new();
        let rows = 9;
        for i in 0..rows {
            let t = i as f64 / (rows - 1) as f64;
            let y = BORE_Y - 0.070 - t * (if long { 0.20 } else { 0.175 });
            // STANAG / AK curve: the magazine sweeps forward as it drops
            let z = 0.004 + t * t * (if long { 0.062 } else { 0.030 });
            rings.push(Ring {
                pts: super_ellipse(0.0135, 0.0225 - t * 0.001, 4.4, 14, 0.0),
                o: Some([0.0, y, z]),
                q: Some(quat_from_axis_angle(
                    [1.0, 0.0, 0.0],
                    t * (if long { 0.5 } else { 0.28 }),
                )),
                s: Some([1.0, 1.0]),
                y: Some(0.0),
            });
        }
        let mut mag = loft(
            &rings,
            LoftOpts { closed: true, cap_start: true, cap_end: true },
        );
        compute_normals(&mut mag);
        displace(&mut mag, |x, y, z, nx, _ny, _nzz, _i| {
            // moulded ribs down the sides
            let rib = f64::sin((y - BORE_Y) * 210.0) * 0.5 + 0.5;
            if nx.abs() > 0.6 {
                rib * 0.0012
            } else {
                nz.fbm3(x * 80.0, y * 80.0, z * 80.0, 2) * 0.0008
            }
        });
        append_mesh(&mut poly, &mag);
    }

    /* ---- upper receiver (steel) ---- */
    append_mesh(
        &mut steel,
        &box_at(
            0.0175,
            0.0225,
            0.062,
            0.0,
            BORE_Y + 0.014,
            0.005,
            BoxOpts { n: 4.4, round_y: 0.22, ..BoxOpts::DEFAULT },
        ),
    );
    // top rail with slots
    {
        let rail_z0 = -0.055;
        let rail_z1 = if long { 0.06 } else { 0.145 };
        append_mesh(
            &mut steel,
            &box_at(
                0.0115,
                0.005,
                (rail_z1 - rail_z0) * 0.5,
                0.0,
                BORE_Y + 0.040,
                (rail_z0 + rail_z1) * 0.5,
                BoxOpts { n: 6.0, round_y: 0.1, ..BoxOpts::DEFAULT },
            ),
        );
        let n = ((rail_z1 - rail_z0) / 0.0102).floor() as usize;
        for i in 0..n {
            let z = rail_z0 + 0.004 + i as f64 * 0.0102;
            append_mesh(
                &mut steel,
                &box_at(
                    0.0125,
                    0.0022,
                    0.0026,
                    0.0,
                    BORE_Y + 0.0435,
                    z,
                    BoxOpts { n: 8.0, rows: 4, round_y: 0.1, ..BoxOpts::DEFAULT },
                ),
            );
        }
    }
    // ejection port cover on the shooter's right (-X)
    append_mesh(
        &mut steel,
        &box_at(
            0.0035,
            0.011,
            0.026,
            -0.018,
            BORE_Y + 0.012,
            0.012,
            BoxOpts { n: 5.0, round_y: 0.2, ..BoxOpts::DEFAULT },
        ),
    );
    // forward assist
    // `weapon.js:149` omits `cap`, so it takes `cyl`'s `cap = true` default.
    append_mesh(
        &mut steel,
        &cyl(0.007, 0.007, -0.042, -0.026, -0.014, BORE_Y + 0.028, 10, true),
    );
    // charging handle
    if !long {
        append_mesh(
            &mut steel,
            &box_at(
                0.026,
                0.005,
                0.010,
                0.0,
                BORE_Y + 0.036,
                -0.070,
                BoxOpts { n: 5.0, round_y: 0.3, ..BoxOpts::DEFAULT },
            ),
        );
        append_mesh(
            &mut steel,
            &box_at(
                0.010,
                0.010,
                0.004,
                -0.026,
                BORE_Y + 0.036,
                -0.070,
                BoxOpts { n: 5.0, round_y: 0.3, ..BoxOpts::DEFAULT },
            ),
        );
    } else {
        // reciprocating handle on the right
        append_mesh(
            &mut steel,
            &box_at(
                0.010,
                0.008,
                0.020,
                -0.024,
                BORE_Y + 0.026,
                -0.020,
                BoxOpts { n: 5.0, round_y: 0.3, ..BoxOpts::DEFAULT },
            ),
        );
    }
    // barrel + gas block + muzzle device
    append_mesh(
        &mut steel,
        &cyl(0.0105, 0.0098, 0.055, barrel_end - 0.045, 0.0, BORE_Y, 12, false),
    );
    append_mesh(
        &mut steel,
        &box_at(
            0.0115,
            0.014,
            0.014,
            0.0,
            BORE_Y + 0.008,
            if long { 0.33 } else { 0.29 },
            BoxOpts { n: 5.0, round_y: 0.25, ..BoxOpts::DEFAULT },
        ),
    );
    {
        let mut mz = cyl(0.0145, 0.0135, barrel_end - 0.045, barrel_end, 0.0, BORE_Y, 14, true);
        // port slots
        displace(&mut mz, |x, y, z, _nx, _ny, _nzz, _i| {
            let a = f64::atan2(x, y - BORE_Y);
            let slot = if f64::abs(f64::sin(a * 3.0)) > 0.92 && z > barrel_end - 0.035 {
                -0.0035
            } else {
                0.0
            };
            slot
        });
        append_mesh(&mut steel, &mz);
    }

    /* ---- handguard ---- */
    {
        let z0 = 0.055;
        let z1 = if long { 0.34 } else { 0.30 };
        let rows = 7;
        let mut rings: Vec<Ring> = Vec::new();
        for i in 0..rows {
            let t = i as f64 / (rows - 1) as f64;
            let r = if long { 0.0255 - t * 0.002 } else { 0.0245 - t * 0.0025 };
            rings.push(Ring {
                pts: super_ellipse(
                    r,
                    r * (if long { 1.02 } else { 0.98 }),
                    if long { 3.0 } else { 4.0 },
                    18,
                    0.0,
                ),
                o: Some([0.0, BORE_Y - 0.002, z0 + (z1 - z0) * t]),
                q: Some(quat_from_axis_angle(
                    [1.0, 0.0, 0.0],
                    std::f64::consts::PI / 2.0,
                )),
                s: Some([1.0, 1.0]),
                y: Some(0.0),
            });
        }
        let mut hg = loft(
            &rings,
            LoftOpts { closed: true, cap_start: false, cap_end: true },
        );
        compute_normals(&mut hg);
        displace(&mut hg, |x, y, z, nx, ny, _nzz, _i| {
            if long {
                // ribbed polymer handguard
                let rib = f64::sin(z * 260.0) * 0.5 + 0.5;
                return rib * 0.0016 + nz.fbm3(x * 70.0, y * 70.0, z * 70.0, 2) * 0.001;
            }
            // M-LOK slots on the sides and bottom
            let side = nx.abs() > 0.55;
            let down = ny < -0.55;
            // JS `%` on floats is a truncated remainder, which is exactly
            // Rust's `%` on `f64` — the sign follows the dividend in both.
            let slot = if ((z * 1000.0) % 42.0) < 22.0 && (side || down) {
                -0.0035
            } else {
                0.0
            };
            slot + nz.fbm3(x * 70.0, y * 70.0, z * 70.0, 2) * 0.0008
        });
        append_mesh(if long { &mut poly } else { &mut steel }, &hg);
    }

    /* ---- stock ---- */
    if long {
        // side-folding wire stock: two rails and a pad
        for s in [-1.0f64, 1.0] {
            let pts = [
                [s * 0.012, BORE_Y - 0.004, -0.055],
                [s * 0.016, BORE_Y - 0.016, -0.135],
                [s * 0.018, BORE_Y - 0.022, -0.235],
            ];
            let mut r = ribbon(
                &pts,
                0.010,
                0.008,
                &RibbonOpts {
                    seg: 6,
                    upright: false,
                    tube: TubeOpts { up: [0.0, 1.0, 0.0], ..Default::default() },
                },
            );
            compute_normals(&mut r);
            append_mesh(&mut steel, &r);
        }
        append_mesh(
            &mut rubber,
            &box_at(
                0.020,
                0.036,
                0.008,
                0.0,
                BORE_Y - 0.026,
                -0.245,
                BoxOpts { n: 4.0, round_y: 0.4, ..BoxOpts::DEFAULT },
            ),
        );
        append_mesh(
            &mut poly,
            &box_at(
                0.020,
                0.026,
                0.030,
                0.0,
                BORE_Y - 0.018,
                -0.085,
                BoxOpts { n: 4.0, round_y: 0.3, ..BoxOpts::DEFAULT },
            ),
        );
    } else {
        append_mesh(
            &mut steel,
            &cyl(0.0155, 0.0155, -0.075, -0.225, 0.0, BORE_Y + 0.002, 12, false),
        );
        // collapsible stock body
        let mut body = box_at(
            0.0215,
            0.030,
            0.058,
            0.0,
            BORE_Y - 0.004,
            -0.175,
            BoxOpts { n: 3.8, round_y: 0.28, ..BoxOpts::DEFAULT },
        );
        warp(&mut body, |v: &mut V3, _i| {
            // cheek weld slope
            if v.y > BORE_Y + 0.008 {
                v.y -= (v.z + 0.175) * -0.12;
            }
        });
        compute_normals(&mut body);
        displace(&mut body, |x, y, z, _nx, _ny, _nzz, _i| {
            nz.fbm3(x * 80.0, y * 80.0, z * 80.0, 2) * 0.0012
        });
        append_mesh(&mut poly, &body);
        append_mesh(
            &mut poly,
            &box_at(
                0.0075,
                0.026,
                0.048,
                0.0,
                BORE_Y - 0.034,
                -0.165,
                BoxOpts { n: 4.0, round_y: 0.3, ..BoxOpts::DEFAULT },
            ),
        );
        append_mesh(
            &mut rubber,
            &box_at(
                0.0215,
                0.033,
                0.007,
                0.0,
                BORE_Y - 0.004,
                -0.230,
                BoxOpts { n: 4.0, round_y: 0.35, ..BoxOpts::DEFAULT },
            ),
        );
    }

    /* ---- sights / optic ---- */
    if long {
        // rear leaf sight + front post
        append_mesh(
            &mut steel,
            &box_at(
                0.010,
                0.010,
                0.006,
                0.0,
                BORE_Y + 0.026,
                -0.040,
                BoxOpts { n: 5.0, round_y: 0.3, ..BoxOpts::DEFAULT },
            ),
        );
        append_mesh(
            &mut steel,
            &box_at(
                0.008,
                0.016,
                0.005,
                0.0,
                BORE_Y + 0.030,
                // `long ? 0.33 : 0.29` inside the `if (long)` arm
                // (`weapon.js:237`) — always `0.33`. Ported as written rather
                // than folded: dead computation in the source is still part
                // of the source.
                if long { 0.33 } else { 0.29 },
                BoxOpts { n: 5.0, round_y: 0.3, ..BoxOpts::DEFAULT },
            ),
        );
    } else {
        // short tube optic on a riser
        append_mesh(
            &mut steel,
            &box_at(
                0.016,
                0.016,
                0.028,
                0.0,
                BORE_Y + 0.056,
                0.010,
                BoxOpts { n: 4.4, round_y: 0.25, ..BoxOpts::DEFAULT },
            ),
        );
        let tube_z0 = -0.018;
        let tube_z1 = 0.058;
        append_mesh(
            &mut steel,
            &cyl(0.0205, 0.0205, tube_z0, tube_z1, 0.0, BORE_Y + 0.085, 16, false),
        );
        append_mesh(
            &mut steel,
            &cyl(
                0.0225,
                0.0225,
                tube_z0 - 0.006,
                tube_z0 + 0.004,
                0.0,
                BORE_Y + 0.085,
                16,
                true,
            ),
        );
        append_mesh(
            &mut steel,
            &cyl(
                0.0225,
                0.0225,
                tube_z1 - 0.004,
                tube_z1 + 0.006,
                0.0,
                BORE_Y + 0.085,
                16,
                true,
            ),
        );
        // lenses
        let l0 = cyl(
            0.0185,
            0.0185,
            tube_z0 - 0.0015,
            tube_z0 + 0.0015,
            0.0,
            BORE_Y + 0.085,
            16,
            true,
        );
        let l1 = cyl(
            0.0185,
            0.0185,
            tube_z1 - 0.0015,
            tube_z1 + 0.0015,
            0.0,
            BORE_Y + 0.085,
            16,
            true,
        );
        append_mesh(&mut glass, &l0);
        append_mesh(&mut glass, &l1);
        // adjustment turret
        append_mesh(
            &mut steel,
            &cyl(0.009, 0.009, 0.012, 0.030, -0.020, BORE_Y + 0.085, 10, true),
        );
    }

    /* ---- sling loops ---- */
    append_mesh(
        &mut steel,
        &box_at(
            0.005,
            0.010,
            0.006,
            -0.016,
            BORE_Y - 0.020,
            if long { -0.06 } else { -0.072 },
            BoxOpts { n: 5.0, round_y: 0.3, ..BoxOpts::DEFAULT },
        ),
    );
    append_mesh(
        &mut steel,
        &box_at(
            0.005,
            0.008,
            0.006,
            -0.020,
            BORE_Y - 0.012,
            if long { 0.30 } else { 0.26 },
            BoxOpts { n: 5.0, round_y: 0.3, ..BoxOpts::DEFAULT },
        ),
    );

    /* ---- bake into the actor's bind space ---- */
    let m = bind_matrix(rng);

    for mesh in [&mut steel, &mut poly, &mut rubber, &mut glass] {
        compute_normals(mesh);
        transform_mesh(mesh, &m);
    }

    // `weapon.js:274-275`. Bound to locals before the struct literal so the
    // closure's borrow of `m` is over before `m` is moved into `matrix`.
    let to_bind = |px: f64, py: f64, pz: f64| apply_matrix4([px, py, pz], &m);
    let muzzle = to_bind(0.0, BORE_Y, barrel_end + 0.012);
    let bore_origin = to_bind(0.0, BORE_Y, 0.0);
    let ejection = to_bind(-0.024, BORE_Y + 0.012, 0.012);
    let stock_top = to_bind(0.0, BORE_Y, -0.10);
    let foregrip = to_bind(0.0, BORE_Y - 0.028, if long { 0.22 } else { 0.205 });
    let mag_bottom = to_bind(0.0, BORE_Y - 0.25, 0.03);

    Weapon {
        muzzle,
        bore_origin,
        ejection,
        stock_top,
        foregrip,
        mag_bottom,
        steel,
        polymer: poly,
        rubber,
        glass,
        matrix: m,
    }
}
