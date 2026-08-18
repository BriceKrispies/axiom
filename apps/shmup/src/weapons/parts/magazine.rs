//! Ported from Claude-of-Duty `src/weapons/parts.js`: `buildMagazine`
//! (`:1082-1202`), `addRollmark` (`:1646-1675`), `addFrontSight`
//! (`:1678-1717`), `addRearSight` (`:1720-1778`).
//!
//! Every builder here bolts a real mechanical assembly onto an [`Assembly`]
//! at a given offset, authored from published dimensions — see
//! `docs/work-manifests/shmup-port/03-weapon-geometry-api.md` for
//! the fixed Rust primitive API these are written against.
//!
//! This is app code (`apps/`), outside the Branchless Law and the Coverage
//! Law — plain `if`/`for` throughout, matching the source's own control
//! flow, per the port recipe.
//!
//! ## The `mats` parameter is dead code, and that is a preserved source quirk
//!
//! `buildMagazine(asm, mats, o)` (`parts.js:1082`) declares a `mats`
//! parameter that its body never references — every real call site
//! (`models/rifle.js:269`, `models/pistol.js:221`, `models/smg.js:240`)
//! passes `null`. Per the port recipe's rule 7 ("port the behaviour and pin
//! it with a test naming it as a source quirk"), the parameter is kept for
//! call-order fidelity as `_mats: ()`, not silently dropped.
//!
//! `buildMagazine` also calls `cartridge()` (`parts.js:92-116`), which lives
//! in the "small hardware" section of `parts.js` and is ported at
//! [`crate::weapons::parts::hardware::cartridge`] — used here, not
//! duplicated.

use axiom_math::{Mat4, Vec3};

use crate::weapons::geometry::primitives::{box_geo, extrude, knurl_band, lathe_z, rod_z, round_rect, ring, ExtrudeOpts};
use crate::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use crate::weapons::parts::hardware::cartridge;

/* -------------------------------------------------------------------------- */
/*  local transform helpers                                                   */
/* -------------------------------------------------------------------------- */

/// `BufferGeometry.translate(x, y, z)`, via the normal-matrix-correct
/// [`Geo::apply`] — see `geometry/primitives/xform.rs`'s doc for why this
/// reuses `apply` rather than hand-rolling a second transform path.
fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `BufferGeometry.rotateX(angle)`. `angle` is `f64` and the rotation is
/// built directly from `f64`-computed `sin`/`cos` (matching
/// `THREE.Matrix4.makeRotationX`, which takes a full-precision `f64` angle
/// throughout); only the resulting matrix elements are rounded to `f32`.
///
/// This does **not** go through [`axiom_math::Quat::from_axis_angle`], which only
/// accepts `f32` and would force the angle to truncate *before* the
/// trigonometry — a strictly worse rounding order than rounding the trig
/// result, and the cause of a real second-weld tie-break mismatch pinned by
/// `tests/weapons_parts_magazine_port.rs`. There is precedent for building
/// the matrix by hand instead of reusing `axiom_math`: see
/// `geometry/assembly.rs`'s `euler_xyz_quat` doc comment.
fn rotate_x(g: &mut Geo, angle: f64) {
    let (s, c) = (angle.sin() as f32, angle.cos() as f32);
    let m = Mat4::from_cols_array([
        1.0, 0.0, 0.0, 0.0, //
        0.0, c, s, 0.0, //
        0.0, -s, c, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ]);
    g.apply(&m);
}

/// `BufferGeometry.rotateY(angle)`. See [`rotate_x`] for why this computes
/// `sin`/`cos` in `f64` and builds the matrix directly rather than rounding
/// the angle down to `f32` first.
fn rotate_y(g: &mut Geo, angle: f64) {
    let (s, c) = (angle.sin() as f32, angle.cos() as f32);
    let m = Mat4::from_cols_array([
        c, 0.0, -s, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        s, 0.0, c, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ]);
    g.apply(&m);
}

/// `BufferGeometry.scale(x, y, z)`.
fn scale(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::scale(Vec3::new(x, y, z)));
}

/* -------------------------------------------------------------------------- */
/*  magazine                                                                  */
/* -------------------------------------------------------------------------- */

/// `o` on `buildMagazine(asm, mats, o)` (`parts.js:1082-1088,1173,1192-1194`).
/// Defaults match the source exactly.
#[derive(Clone, Copy, Debug)]
pub struct MagazineOpts {
    pub w: f32,
    pub d: f32,
    pub len: f32,
    /// Sagitta of the feed curve in METRES over the magazine's length
    /// (`parts.js:1086`).
    pub curve: f32,
    pub segs: u32,
    pub poly: &'static str,
    /// Witness-hole count (`o.witness ?? 4`, `parts.js:1173`).
    pub witness: u32,
    /// Cartridge dimensions fed to [`cartridge`] (`parts.js:1192-1194`).
    pub case_len: f32,
    pub rim_r: f32,
    pub bullet_len: f32,
}

impl Default for MagazineOpts {
    fn default() -> Self {
        MagazineOpts {
            w: 0.0255,
            d: 0.0655,
            len: 0.215,
            curve: 0.028,
            segs: 8,
            poly: "polymer",
            witness: 4,
            case_len: 0.0446,
            rim_r: 0.00495,
            bullet_len: 0.019,
        }
    }
}

/// `{ len, w, d }` (`parts.js:1201`), `buildMagazine`'s return value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagazineDims {
    pub len: f32,
    pub w: f32,
    pub d: f32,
}

/// The arc-position of one magazine slice: `y` runs down, `z` bows forward
/// (`-Z`), and `tilt` is the local tangent so a stack of extruded slices
/// reads as one continuous curved body. `at(t)` (`parts.js:1093-1097`).
///
/// Computed in `f64` (per `03-weapon-geometry-api.md`'s corrections): `tilt`
/// is an `atan2`, and JS numbers are `f64` throughout — every caller below
/// casts the result to `f32` exactly once, at the point it feeds a transform.
struct MagPoint {
    y: f64,
    z: f64,
    tilt: f64,
}

fn at(t: f64, len: f64, curve: f64) -> MagPoint {
    MagPoint {
        y: -t * len,
        z: -curve * t * t,
        tilt: (2.0 * curve * t).atan2(len),
    }
}

/// `buildMagazine(asm, mats, o)` (`parts.js:1082-1202`): the curved-body
/// magazine — extruded rounded-rect slices bent along an arc, moulded grip
/// ribs, feed lips, a rear catch notch, a floor plate + finger ledge, a
/// rubber base pad, witness holes, and the top round visible under the feed
/// lips.
pub fn build_magazine(asm: &mut Assembly, _mats: (), o: MagazineOpts) -> MagazineDims {
    let MagazineOpts {
        w,
        d,
        len,
        curve,
        segs,
        poly,
        witness,
        case_len,
        rim_r,
        bullet_len,
    } = o;
    let len64 = f64::from(len);
    let curve64 = f64::from(curve);

    let mut body_parts: Vec<Geo> = Vec::new();
    let mut rib_parts: Vec<Geo> = Vec::new();
    let step = len / segs as f32;

    for i in 0..segs {
        let t = (f64::from(i) + 0.5) / f64::from(segs);
        let p = at(t, len64, curve64);
        let taper = (1.0 - t * 0.04) as f32;

        let pts = round_rect(f64::from(w * taper), f64::from(d * taper), 0.0055, 5);
        let mut seg = extrude(&pts, step * 1.06, ExtrudeOpts { bevel: 0.0008, ..Default::default() });
        rotate_x(&mut seg, std::f64::consts::FRAC_PI_2 + p.tilt);
        translate(&mut seg, 0.0, p.y as f32, p.z as f32);
        body_parts.push(seg);

        // Moulded grip ribs down the flanks.
        if i > 0 && i < segs - 1 {
            for sx in [-1.0f32, 1.0f32] {
                let mut rib = box_geo(0.0018, step * 0.62, d * 0.66, 0.0005, 1);
                rotate_x(&mut rib, p.tilt);
                translate(&mut rib, sx * (w * taper * 0.5), p.y as f32, p.z as f32);
                rib_parts.push(rib);
            }
        }
    }

    // Feed lips: two rails either side of the mouth, plus the rear catch notch.
    let lip_pts: [[f64; 2]; 4] = [[-0.0032, 0.0], [0.0032, 0.0], [0.0026, 0.009], [-0.0026, 0.009]];
    let mut lip = extrude(&lip_pts, d * 0.9, ExtrudeOpts { bevel: 0.0005, ..Default::default() });
    rotate_y(&mut lip, std::f64::consts::FRAC_PI_2);
    for sx in [-1.0f32, 1.0f32] {
        let mut g = lip.clone();
        translate(&mut g, sx * (w * 0.5 - 0.0032), -0.0015, 0.0);
        body_parts.push(g);
    }
    let mut notch = box_geo(0.008, 0.0075, 0.0055, 0.0009, 1);
    translate(&mut notch, 0.0, -0.03, d * 0.5 + 0.0015);
    body_parts.push(notch);

    // Floor plate + finger ledge, on the arc's tangent.
    let end = at(1.0, len64, curve64);
    let plate_pts = round_rect(f64::from(w) + 0.0026, f64::from(d) * 0.97, 0.004, 4);
    let mut plate = extrude(&plate_pts, 0.01, ExtrudeOpts { bevel: 0.001, ..Default::default() });
    rotate_x(&mut plate, std::f64::consts::FRAC_PI_2 + end.tilt);
    translate(&mut plate, 0.0, (end.y - 0.0035) as f32, end.z as f32);
    body_parts.push(plate);

    let mut ledge = box_geo(w + 0.0034, 0.007, 0.013, 0.0016, 2);
    rotate_x(&mut ledge, end.tilt);
    translate(&mut ledge, 0.0, (end.y - 0.007) as f32, (end.z - f64::from(d) * 0.4) as f32);
    body_parts.push(ledge);

    // Base pad, a slightly different polymer batch.
    let pad_pts = round_rect(f64::from(w) + 0.003, f64::from(d) * 0.9, 0.004, 4);
    let mut pad = extrude(&pad_pts, 0.005, ExtrudeOpts { bevel: 0.0009, ..Default::default() });
    rotate_x(&mut pad, std::f64::consts::FRAC_PI_2 + end.tilt);
    translate(&mut pad, 0.0, (end.y - 0.0105) as f32, end.z as f32);

    let body = merge_all(body_parts).expect("buildMagazine always builds at least the floor plate + feed lips");
    asm.add(body, poly, Some(Xform::default()));
    if let Some(ribs) = merge_all(rib_parts) {
        asm.add(ribs, poly, Some(Xform::default()));
    }
    asm.add(pad, "rubber", Some(Xform::default()));

    // Witness holes: recessed dark slots down both sides.
    let denom = f64::from(witness.saturating_sub(1).max(1));
    for i in 0..witness {
        let t = 0.26 + (f64::from(i) / denom) * 0.56;
        let p = at(t, len64, curve64);
        for sx in [-1.0f32, 1.0f32] {
            let h_pts = round_rect(0.0085, 0.0044, 0.0018, 3);
            let mut h = extrude(&h_pts, 0.004, ExtrudeOpts { bevel: 0.0004, ..Default::default() });
            rotate_y(&mut h, std::f64::consts::FRAC_PI_2);
            rotate_x(&mut h, p.tilt);
            translate(&mut h, sx * (w * 0.5 - 0.0006), p.y as f32, p.z as f32);
            asm.add(h, "cavity", Some(Xform::default()));
        }
    }

    // The top round under the feed lips — the detail everyone notices.
    let c = cartridge(case_len, rim_r, bullet_len);
    let cz = (d * 0.5 - 0.0025).min(case_len + bullet_len - d * 0.5 + 0.0015);
    let round_xform = Xform {
        y: -0.0085,
        z: cz,
        ry: std::f32::consts::PI,
        ..Xform::default()
    };
    asm.add(c.brass, "brass", Some(round_xform));
    asm.add(c.bullet, "copper", Some(round_xform));

    MagazineDims { len, w, d }
}

/* -------------------------------------------------------------------------- */
/*  optics + sights                                                           */
/* -------------------------------------------------------------------------- */

/// `o` on `addRollmark(asm, mat, o)` (`parts.js:1646-1652`). Defaults match
/// the source; `pattern` defaults to the fixed digit sequence carrying the
/// mark's stroke rhythm, so the mark is byte-identical every boot.
#[derive(Clone, Debug)]
pub struct RollmarkOpts {
    pub h: f32,
    pub stroke: f32,
    pub depth: f32,
    pub pitch: f32,
    pub pattern: Vec<u8>,
    /// `o.count ?? pat.length` (`parts.js:1652`); `None` uses `pattern.len()`.
    pub count: Option<usize>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
    /// `if (o.sx) g.scale(o.sx, 1, 1)` (`parts.js:1672`) is a **truthy**
    /// check, not `??` — only a present, nonzero value triggers the mirror
    /// scale. [`add_rollmark`] reproduces that by filtering `0.0` out.
    pub sx: Option<f32>,
}

impl Default for RollmarkOpts {
    fn default() -> Self {
        RollmarkOpts {
            h: 0.0036,
            stroke: 0.0006,
            depth: 0.0008,
            pitch: 0.0017,
            pattern: vec![3, 2, 3, 3, 1, 0, 2, 3, 2, 3, 0, 3, 1, 2, 3, 2, 0, 3, 3, 2],
            count: None,
            x: None,
            y: None,
            z: None,
            sx: None,
        }
    }
}

/// `addRollmark(asm, mat, o)` (`parts.js:1646-1675`): an engraved rollmark /
/// calibre stamp, modelled as real recessed strokes in the part's own local
/// space rather than a projected decal — see the source doc comment for why
/// (a viewmodel translates and rotates every frame; anything sampled in
/// world space would swim across the receiver).
pub fn add_rollmark(asm: &mut Assembly, mat: &str, o: RollmarkOpts) {
    let n = o.count.unwrap_or(o.pattern.len());
    let mut parts: Vec<Geo> = Vec::new();

    for i in 0..n {
        let p = o.pattern[i % o.pattern.len()];
        if p == 0 {
            continue;
        }
        let bh = o.h * (0.52 + f32::from(p) * 0.16);
        let mut b = box_geo(o.depth, bh, o.stroke, 0.000_08, 1);
        translate(&mut b, 0.0, (o.h - bh) * 0.5, -(i as f32) * o.pitch);
        parts.push(b);
        // a crossbar, so a run of strokes reads as letters and not as a comb
        if p == 3 {
            let mut c = box_geo(o.depth, o.stroke * 0.85, o.pitch * 0.72, 0.000_08, 1);
            translate(&mut c, 0.0, (o.h - bh) * 0.5 + bh * 0.16, -(i as f32) * o.pitch - o.pitch * 0.3);
            parts.push(c);
        }
    }

    let mut line = box_geo(o.depth, o.stroke * 0.9, (n as f32 - 1.0) * o.pitch, 0.000_08, 1);
    translate(&mut line, 0.0, -o.h * 0.55, -(n as f32 - 1.0) * o.pitch * 0.5);
    parts.push(line);

    let mut g = merge_all(parts).expect("addRollmark always builds at least the underline stroke");
    if let Some(sx) = o.sx.filter(|&s| s != 0.0) {
        scale(&mut g, sx, 1.0, 1.0);
    }
    asm.add(
        g,
        mat,
        Some(Xform {
            x: o.x.unwrap_or(0.0),
            y: o.y.unwrap_or(0.0),
            z: o.z.unwrap_or(0.0),
            ..Xform::default()
        }),
    );
}

/// `addFrontSight(asm, matSteel, matAlu, x, railTop, z, up = true)`
/// (`parts.js:1678-1717`): folding front sight post, protective ears, hinge,
/// detent.
pub fn add_front_sight(asm: &mut Assembly, mat_steel: &str, mat_alu: &str, x: f32, rail_top: f32, z: f32, up: bool) {
    let base_g = box_geo(0.024, 0.008, 0.019, 0.0008, 1);
    asm.add(
        base_g,
        mat_alu,
        Some(Xform {
            x,
            y: rail_top + 0.004,
            z,
            ..Xform::default()
        }),
    );

    let hinge = rod_z(0.0026, 0.0026, 0.026, 10, 0.0003);
    asm.add(
        hinge,
        mat_steel,
        Some(Xform {
            x,
            y: rail_top + 0.008,
            z: z + 0.006,
            ry: std::f32::consts::FRAC_PI_2,
            ..Xform::default()
        }),
    );

    let h = if up { 0.03 } else { 0.006 };
    let tilt = if up { 0.0 } else { -1.35 };
    let ear_pts: [[f64; 2]; 5] = [
        [-0.0022, 0.0],
        [0.0022, 0.0],
        [0.0022, f64::from(h)],
        [0.0, f64::from(h) + 0.002],
        [-0.0022, f64::from(h)],
    ];
    let ear_l = extrude(&ear_pts, 0.0075, ExtrudeOpts { bevel: 0.0005, ..Default::default() });

    let mut ears: Vec<Geo> = Vec::new();
    for sx in [-1.0f32, 1.0f32] {
        let mut g = ear_l.clone();
        translate(&mut g, sx * 0.0088, 0.0, 0.0);
        ears.push(g);
    }
    // the post itself
    let mut post = rod_z(0.0011, 0.0009, h * 0.72, 8, 0.0002);
    rotate_x(&mut post, std::f64::consts::FRAC_PI_2);
    translate(&mut post, 0.0, h * 0.36 + 0.002, 0.0);
    ears.push(post);
    let mut cross = box_geo(0.019, 0.0022, 0.0055, 0.0004, 1);
    translate(&mut cross, 0.0, h - 0.0012, 0.0);
    ears.push(cross);

    let g = merge_all(ears).expect("addFrontSight always builds two ears, a post, and a crossbar");
    asm.add(
        g,
        mat_steel,
        Some(Xform {
            x,
            y: rail_top + 0.008,
            z,
            rx: tilt,
            ..Xform::default()
        }),
    );
}

/// `addRearSight(asm, matSteel, matAlu, x, railTop, z, up = true)`
/// (`parts.js:1720-1778`): folding rear sight aperture wheel, windage drum,
/// protective wings.
pub fn add_rear_sight(asm: &mut Assembly, mat_steel: &str, mat_alu: &str, x: f32, rail_top: f32, z: f32, up: bool) {
    let base_g = box_geo(0.024, 0.0085, 0.022, 0.0008, 1);
    asm.add(
        base_g,
        mat_alu,
        Some(Xform {
            x,
            y: rail_top + 0.0042,
            z,
            ..Xform::default()
        }),
    );

    let h = if up { 0.027 } else { 0.005 };
    let tilt = if up { 0.0 } else { 1.35 };
    let mut parts: Vec<Geo> = Vec::new();

    let leaf_pts: [[f64; 2]; 6] = [
        [-0.011, 0.0],
        [0.011, 0.0],
        [0.011, f64::from(h) * 0.55],
        [0.006, f64::from(h)],
        [-0.006, f64::from(h)],
        [-0.011, f64::from(h) * 0.55],
    ];
    let leaf = extrude(&leaf_pts, 0.006, ExtrudeOpts { bevel: 0.0006, ..Default::default() });
    parts.push(leaf);

    // aperture ring
    let mut ap = ring(0.0032, 0.0011, 14, 6, std::f32::consts::TAU);
    translate(&mut ap, 0.0, h * 0.66, 0.0);
    parts.push(ap);

    // Windage drum -- KNURLED (see `parts.js:1744-1757` for why a smooth
    // lathe reads as a mirror-bright bead in hipfire).
    let drum_pts: [[f32; 2]; 5] = [[0.0, 0.0], [0.0, 0.0048], [0.0035, 0.0052], [0.008, 0.0052], [0.008, 0.0]];
    let drum = lathe_z(&drum_pts, 20, 0.0, std::f32::consts::TAU);
    let mut drum_knurl = knurl_band(0.0053, 0.0042, 22, 0.000_28, 3);
    translate(&mut drum_knurl, 0.0, 0.0, 0.0055);
    let mut drum_g = merge_all(vec![drum, drum_knurl]).expect("addRearSight always builds the drum plus its knurl band");
    rotate_y(&mut drum_g, std::f64::consts::FRAC_PI_2);
    translate(&mut drum_g, 0.012, h * 0.3, 0.0);
    parts.push(drum_g);

    let g = merge_all(parts).expect("addRearSight always builds the leaf, aperture ring, and windage drum");
    asm.add(
        g,
        mat_steel,
        Some(Xform {
            x,
            y: rail_top + 0.0085,
            z,
            rx: tilt,
            ..Xform::default()
        }),
    );
}
