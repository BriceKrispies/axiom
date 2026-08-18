//! Ported from Claude-of-Duty `src/weapons/parts.js` — controls and
//! furniture: `selectorPart` (`:795-828`), `triggerPart` (`:838-866`),
//! `addPistolGrip` (`:876-956`), `addCarbineStock` (`:962-1071`),
//! `chargingHandlePart` (`:1781-1854`), `addForeGrip` (`:1857-1879`).
//!
//! See `parts.js:19-31`: every dimension is a real, published firearm
//! measurement, not an eyeballed guess. Weapon-local space is `+X` right,
//! `+Y` up, `-Z` toward the muzzle, origin at the shooting hand's anchor —
//! the convention `geometry.js:28-30` documents and the `geometry` module
//! (`03-weapon-geometry-api.md`) carries forward.
//!
//! This is app code (`apps/`), outside the Branchless Law and the Coverage
//! Law — plain `if`/`for` throughout, matching the source's own control
//! flow, per the port recipe. Rust has no default arguments, so every JS
//! `?? value`/`= value` default is documented on the option struct/function
//! and callers pass it explicitly (same convention as `parts::barrel`,
//! `parts::hardware`, `parts::magazine`).

use axiom_math::{Mat4, Vec3};

use crate::weapons::geometry::primitives::{blob, box_geo, extrude, lathe_z, rod_z, tube_z, ExtrudeOpts};
use crate::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use crate::weapons::parts::hardware::{add_qd_socket, add_screw, add_sling_loop, MountAxis};

/* -------------------------------------------------------------------------- */
/*  local transform helpers                                                   */
/* -------------------------------------------------------------------------- */

/// `BufferGeometry.translate(x, y, z)`, via the normal-matrix-correct
/// [`Geo::apply`] — see `geometry/primitives/xform.rs`'s doc for why this
/// reuses `apply` rather than hand-rolling a second transform path.
fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `BufferGeometry.rotateY(angle)`. `angle` is `f64` and the rotation is
/// built directly from `f64`-computed `sin`/`cos` (matching
/// `THREE.Matrix4.makeRotationY`, which takes a full-precision `f64` angle
/// throughout); only the resulting matrix elements are rounded to `f32`.
/// This does **not** go through [`axiom_math::Quat::from_axis_angle`], which
/// only accepts `f32` and would force the angle to truncate *before* the
/// trigonometry — the same precision trap `parts::magazine`'s `rotate_y`
/// documents and was pinned by a real weld tie-break mismatch there.
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

/// `BufferGeometry.rotateZ(angle)`. See [`rotate_y`] for why this computes
/// `sin`/`cos` in `f64` and builds the matrix directly rather than rounding
/// the angle down to `f32` first.
fn rotate_z(g: &mut Geo, angle: f64) {
    let (s, c) = (angle.sin() as f32, angle.cos() as f32);
    let m = Mat4::from_cols_array([
        c, s, 0.0, 0.0, //
        -s, c, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
    ]);
    g.apply(&m);
}

/* -------------------------------------------------------------------------- */
/*  selector / trigger                                                        */
/* -------------------------------------------------------------------------- */

/// `selectorPart`'s return: `{ geo, mat }` (`parts.js:827`).
pub struct SelectorPart {
    pub geo: Geo,
    pub mat: String,
}

/// Ambidextrous safety selector — the paddle rotates around the X axis.
/// `r` default `0.006` (`selectorPart`, `parts.js:795-828`).
///
/// `mat_steel` is a preserved source quirk: `selectorPart(matAlu, matSteel, r)`
/// (`parts.js:795`) declares a second material parameter its body never
/// reads — the return is always `{ geo, mat: matAlu }`. Kept for call-order
/// fidelity (per the port recipe's rule 7), not silently dropped, the same
/// way `parts::magazine::build_magazine` keeps its dead `mats` parameter.
pub fn selector_part(mat_alu: &str, _mat_steel: &str, r: f32) -> SelectorPart {
    let mut parts: Vec<Geo> = Vec::new();

    let mut shaft = rod_z(r * 0.62, r * 0.62, 0.03, 12, 0.0004);
    rotate_y(&mut shaft, std::f64::consts::FRAC_PI_2);
    parts.push(shaft);

    let mut boss = lathe_z(
        &[[0.0, 0.0], [0.0, r], [0.0012, r * 1.1], [0.005, r * 1.1], [0.005, 0.0]],
        12,
        0.0,
        std::f32::consts::TAU,
    );
    rotate_y(&mut boss, -std::f64::consts::FRAC_PI_2);
    translate(&mut boss, 0.0135, 0.0, 0.0);
    parts.push(boss);

    let mut paddle = extrude(
        &[
            [0.0, -0.0035],
            [0.021, -0.006],
            [0.024, 0.0],
            [0.02, 0.005],
            [0.0, 0.0045],
        ],
        0.0042,
        ExtrudeOpts {
            bevel: 0.0008,
            ..Default::default()
        },
    );
    rotate_y(&mut paddle, std::f64::consts::FRAC_PI_2);
    translate(&mut paddle, 0.0185, 0.0, 0.0);
    parts.push(paddle);

    SelectorPart {
        geo: merge_all(parts).expect("selectorPart always pushes shaft/boss/paddle"),
        mat: mat_alu.to_string(),
    }
}

/// `triggerPart`'s return: `{ geo, mat }` (`parts.js:865`).
pub struct TriggerPart {
    pub geo: Geo,
    pub mat: String,
}

/// Curved trigger blade with a serrated face; pivots about its pin.
///
/// The outline is a SIDE view: `+X` is rearward (the face the finger
/// presses), `-Y` is down. The whole blade is rotated at the end so that
/// outline-X becomes `+Z` and the 7 mm extrusion becomes the blade's width
/// across the receiver — without that the blade is a plate standing across
/// the trigger guard (`triggerPart`, `parts.js:838-866`).
pub fn trigger_part(mat_steel: &str) -> TriggerPart {
    let blade = extrude(
        &[
            [-0.0045, 0.0045],
            [0.0048, 0.0045],
            [0.0056, -0.008],
            [0.0044, -0.0158],
            [0.0016, -0.0202],
            [-0.0032, -0.0192],
            [-0.0055, -0.011],
            [-0.006, -0.002],
        ],
        0.0072,
        ExtrudeOpts {
            bevel: 0.0007,
            bevel_segments: 2,
            ..Default::default()
        },
    );
    let mut parts: Vec<Geo> = vec![blade];
    // Serrations across the face the finger pad sits on.
    for i in 0..6u32 {
        let mut g = box_geo(0.0015, 0.0011, 0.0066, 0.0003, 1);
        // Spin in place first, THEN place: rotating after the translate would
        // swing the serration around the blade's pivot instead of tilting it.
        rotate_z(&mut g, -0.2 - f64::from(i) * 0.05);
        translate(&mut g, 0.0049 - i as f32 * 0.0004, -0.0045 - i as f32 * 0.0026, 0.0);
        parts.push(g);
    }
    let mut geo = merge_all(parts).expect("triggerPart always pushes the blade plus 6 serrations");
    rotate_y(&mut geo, -std::f64::consts::FRAC_PI_2); // outline-X -> +Z (rearward), extrusion -> across
    TriggerPart {
        geo,
        mat: mat_steel.to_string(),
    }
}

/* -------------------------------------------------------------------------- */
/*  grip / stock                                                              */
/* -------------------------------------------------------------------------- */

/// `o` on `addPistolGrip(asm, matPoly, matRubber, o)` (`parts.js:876-956`).
/// Defaults match the source: `len = 0.108`, `w = 0.031`, `angle = 0.38`
/// (positive rake tilts the BOTTOM rearward), `y`/`z` default `0`.
#[derive(Clone, Copy, Debug)]
pub struct PistolGripOpts {
    pub len: f32,
    pub w: f32,
    pub angle: f32,
    pub y: f32,
    pub z: f32,
}

impl Default for PistolGripOpts {
    fn default() -> Self {
        PistolGripOpts {
            len: 0.108,
            w: 0.031,
            angle: 0.38,
            y: 0.0,
            z: 0.0,
        }
    }
}

/// Pistol grip with a palm swell, finger grooves, a beavertail and moulded
/// texture panels. Built along its own axis then rotated by `angle`
/// (`addPistolGrip`, `parts.js:876-956`).
pub fn add_pistol_grip(asm: &mut Assembly, mat_poly: &str, mat_rubber: &str, o: PistolGripOpts) {
    let PistolGripOpts { len, w, angle, y: oy, z: oz } = o;

    // Side profile in (z, y), authored as one closed outline and extruded
    // across the grip's width. A single solid cannot develop the seams a
    // lofted stack of slices does, and the outline is where the shape
    // actually lives: a swept front strap with finger relief, a straight
    // back strap, a beavertail.
    let zf: f64 = -0.0155; // front strap
    let zb: f64 = 0.0155; // back strap
    let len64 = f64::from(len);
    let profile: Vec<[f64; 2]> = vec![
        [zb + 0.004, 0.008],
        [zf - 0.002, 0.007],
        [zf - 0.0035, -0.006],
        [zf - 0.0015, -0.02],
        [zf - 0.003, -0.034],
        [zf - 0.0005, -0.05],
        [zf - 0.002, -0.064],
        [zf + 0.001, -0.08],
        [zf + 0.0035, -len64 + 0.004],
        [zf + 0.008, -len64],
        [zb - 0.006, -len64],
        [zb - 0.001, -len64 + 0.006],
        [zb + 0.001, -0.06],
        [zb + 0.0025, -0.03],
        [zb + 0.006, -0.012],
    ];
    let mut core = extrude(
        &profile,
        w,
        ExtrudeOpts {
            bevel: 0.0035,
            bevel_segments: 3,
            curve_segments: 4,
            ..Default::default()
        },
    );
    rotate_y(&mut core, std::f64::consts::FRAC_PI_2);
    asm.add(
        core,
        mat_poly,
        Some(Xform {
            y: oy,
            z: oz,
            rx: -angle,
            ..Default::default()
        }),
    );

    // Palm swell on both flanks so the grip is not a slab.
    let swell = blob(0.008, len * 0.62, 0.03, 0.006, 3);
    for sx in [-1.0f32, 1.0f32] {
        asm.add(
            swell.clone(),
            mat_poly,
            Some(Xform {
                x: sx * (w * 0.5 - 0.0015),
                y: oy - len * 0.42,
                z: oz + 0.0035,
                rx: -angle,
                ..Default::default()
            }),
        );
    }

    // Beavertail behind the trigger, blending into the receiver.
    let beaver = blob(w * 0.96, 0.02, 0.024, 0.006, 3);
    asm.add(
        beaver,
        mat_poly,
        Some(Xform {
            y: oy + 0.005,
            z: oz + 0.012,
            rx: -angle * 0.6,
            ..Default::default()
        }),
    );

    // Rubberised over-mould: side panels plus the front-strap finger swells.
    let panel = blob(w * 1.03, len * 0.58, 0.019, 0.005, 3);
    asm.add(
        panel,
        mat_rubber,
        Some(Xform {
            y: oy - len * 0.44,
            z: oz + 0.0025,
            rx: -angle,
            ..Default::default()
        }),
    );
    // Finger swells on the front strap: shallow cross-wise ridges, not rings.
    for i in 0..4u32 {
        let t = 0.15 + i as f32 * 0.2;
        let ridge = blob(w * 0.9, 0.011, 0.007, 0.003, 3);
        let yy = oy - t * len;
        let zz = oz + zf as f32 + 0.001 + (t * std::f32::consts::PI).sin() * 0.001;
        // Rotate into the raked frame by hand so the ridge hugs the strap.
        let cs = (-angle).cos();
        let sn = (-angle).sin();
        asm.add(
            ridge,
            mat_rubber,
            Some(Xform {
                y: oy + (yy - oy) * cs - (zz - oz) * sn,
                z: oz + (yy - oy) * sn + (zz - oz) * cs,
                rx: -angle,
                ..Default::default()
            }),
        );
    }

    // Grip cap with its screw.
    let cap_y = oy - angle.cos() * len;
    let cap_z = oz + angle.sin() * len;
    let cap = blob(w * 0.92, 0.007, 0.031, 0.0025, 2);
    asm.add(
        cap,
        mat_poly,
        Some(Xform {
            y: cap_y + 0.001,
            z: cap_z,
            rx: -angle,
            ..Default::default()
        }),
    );
    add_screw(asm, mat_rubber, 0.0, cap_y - 0.0015, cap_z, 0.0026, MountAxis::Y, 0.006);
}

/// `o` on `addCarbineStock(asm, matAlu, matPoly, matRubber, o)`
/// (`parts.js:962-1071`). `bore`/`zRear`/`zFront` have no JS default (bare
/// reads); `y` defaults to `None`, reproducing `o.y ?? bore - 0.012`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CarbineStockOpts {
    pub bore: f32,
    pub z_rear: f32,
    pub z_front: f32,
    pub y: Option<f32>,
}

/// Collapsible carbine stock on a mil-spec receiver extension: 6 detent
/// positions, cheek weld, sling loop, adjustment lever and a rubber butt
/// pad (`addCarbineStock`, `parts.js:962-1071`).
pub fn add_carbine_stock(asm: &mut Assembly, mat_alu: &str, mat_poly: &str, mat_rubber: &str, o: CarbineStockOpts) {
    let CarbineStockOpts { bore, z_rear, z_front, y } = o;
    let y_axis = y.unwrap_or(bore - 0.012);
    let tube_r = 0.0146;
    let len = z_rear - z_front;

    // receiver extension
    let ext = tube_z(tube_r, tube_r - 0.0022, len - 0.004, 18, 0.0004);
    asm.add(
        ext,
        mat_alu,
        Some(Xform {
            y: y_axis,
            z: (z_rear + z_front) / 2.0,
            ..Default::default()
        }),
    );
    // castle nut + end plate
    let nut = lathe_z(
        &[
            [0.0, tube_r],
            [0.0, tube_r + 0.0034],
            [0.0016, tube_r + 0.0038],
            [0.0085, tube_r + 0.0038],
            [0.01, tube_r + 0.003],
            [0.01, tube_r],
        ],
        18,
        0.0,
        std::f32::consts::TAU,
    );
    asm.add(
        nut,
        mat_alu,
        Some(Xform {
            y: y_axis,
            z: z_front,
            ..Default::default()
        }),
    );
    for i in 0..6u32 {
        let a = (f64::from(i) / 6.0) * std::f64::consts::TAU;
        let mut notch = box_geo(0.0022, 0.0034, 0.006, 0.0004, 1);
        translate(&mut notch, 0.0, tube_r + 0.0032, 0.0);
        rotate_z(&mut notch, a);
        translate(&mut notch, 0.0, y_axis, z_front + 0.005);
        asm.add(notch, mat_alu, Some(Xform::default()));
    }
    // detent notches along the bottom of the tube
    for i in 0..6u32 {
        let z = z_front + 0.026 + i as f32 * 0.018;
        if z > z_rear - 0.02 {
            break;
        }
        let n = box_geo(0.0075, 0.0032, 0.0075, 0.0006, 1);
        asm.add(
            n,
            mat_alu,
            Some(Xform {
                y: y_axis - tube_r + 0.0008,
                z,
                ..Default::default()
            }),
        );
    }

    // Stock body: a side profile extruded across the width, so the comb
    // slopes and the toe kicks down the way a collapsible carbine stock
    // actually does.
    let body_len = 0.104;
    let bz = z_rear - body_len / 2.0;
    let comb_y = y_axis + 0.026;
    let toe_y = y_axis - 0.042;
    let outline: Vec<[f64; 2]> = vec![
        [f64::from(-body_len * 0.5), f64::from(y_axis + 0.004)],
        [f64::from(-body_len * 0.5 + 0.012), f64::from(y_axis + 0.017)],
        [f64::from(-body_len * 0.5 + 0.03), f64::from(comb_y - 0.002)],
        [f64::from(body_len * 0.5 - 0.012), f64::from(comb_y)],
        [f64::from(body_len * 0.5), f64::from(comb_y - 0.006)],
        [f64::from(body_len * 0.5), f64::from(toe_y + 0.008)],
        [f64::from(body_len * 0.5 - 0.008), f64::from(toe_y)],
        [f64::from(-body_len * 0.5 + 0.028), f64::from(toe_y + 0.006)],
        [f64::from(-body_len * 0.5 + 0.008), f64::from(y_axis - 0.02)],
        [f64::from(-body_len * 0.5), f64::from(y_axis - 0.009)],
    ];
    let mut shell_parts: Vec<Geo> = Vec::new();
    let mut shell = extrude(
        &outline,
        0.043,
        ExtrudeOpts {
            bevel: 0.0035,
            bevel_segments: 2,
            ..Default::default()
        },
    );
    rotate_y(&mut shell, std::f64::consts::FRAC_PI_2);
    shell_parts.push(shell);
    // Cheek weld ridge along the comb.
    let mut cheek = blob(0.047, 0.012, body_len * 0.66, 0.005, 3);
    translate(&mut cheek, 0.0, comb_y - 0.002, -0.006);
    shell_parts.push(cheek);
    // Lightening scallops on both flanks.
    for sx in [-1.0f32, 1.0f32] {
        let mut sc = blob(0.005, 0.024, 0.052, 0.005, 3);
        translate(&mut sc, sx * 0.0205, y_axis - 0.012, 0.004);
        shell_parts.push(sc);
    }
    let body = merge_all(shell_parts).expect("addCarbineStock always builds the shell + cheek weld");
    asm.add(
        body,
        mat_poly,
        Some(Xform {
            z: bz,
            ..Default::default()
        }),
    );

    // adjustment lever under the stock
    let lever = extrude(
        &[
            [-0.014, 0.0],
            [0.016, 0.0],
            [0.018, -0.007],
            [0.012, -0.011],
            [-0.012, -0.011],
            [-0.016, -0.005],
        ],
        0.014,
        ExtrudeOpts {
            bevel: 0.0008,
            ..Default::default()
        },
    );
    asm.add(
        lever,
        mat_poly,
        Some(Xform {
            y: y_axis - 0.036,
            z: bz + 0.012,
            ..Default::default()
        }),
    );

    // Butt pad — rubber, with real grooves, following the comb-to-toe rake.
    let pad = blob(0.045, 0.072, 0.013, 0.0045, 3);
    asm.add(
        pad,
        mat_rubber,
        Some(Xform {
            y: y_axis - 0.008,
            z: z_rear - 0.004,
            rx: 0.06,
            ..Default::default()
        }),
    );
    for i in 0..5u32 {
        let g = box_geo(0.043, 0.0035, 0.005, 0.0012, 2);
        asm.add(
            g,
            mat_rubber,
            Some(Xform {
                y: y_axis + 0.02 - i as f32 * 0.0125,
                z: z_rear + 0.0026,
                rx: 0.06,
                ..Default::default()
            }),
        );
    }

    // sling loop + QD socket
    add_sling_loop(
        asm,
        mat_alu,
        0.0225,
        y_axis - 0.026,
        bz - 0.03,
        0.0075,
        Xform {
            ry: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        },
    );
    add_qd_socket(asm, mat_poly, mat_alu, -0.0215, y_axis - 0.014, bz - 0.026, MountAxis::X, 0.005);
}

/* -------------------------------------------------------------------------- */
/*  charging handle / foregrip                                                */
/* -------------------------------------------------------------------------- */

/// AR charging handle: latch, T-bar, ridged wings. Moves as one part
/// (`chargingHandlePart`, `parts.js:1781-1854`).
pub fn charging_handle_part() -> Geo {
    let mut parts: Vec<Geo> = Vec::new();

    let mut bar = box_geo(0.028, 0.0055, 0.052, 0.0008, 1);
    translate(&mut bar, 0.0, 0.0, 0.012);
    parts.push(bar);

    let mut shaft_g = rod_z(0.0055, 0.0055, 0.07, 12, 0.0005);
    translate(&mut shaft_g, 0.0, -0.0022, -0.02);
    parts.push(shaft_g);

    // T-handle wings with grip ridges
    let wing = extrude(
        &[
            [0.0, -0.005],
            [0.02, -0.0075],
            [0.024, -0.002],
            [0.024, 0.004],
            [0.0, 0.004],
        ],
        0.0055,
        ExtrudeOpts {
            bevel: 0.0007,
            ..Default::default()
        },
    );
    let mut w_r = wing.clone();
    rotate_y(&mut w_r, std::f64::consts::FRAC_PI_2);
    rotate_z(&mut w_r, 0.0); // no-op in source too (`wR.rotateZ(0)`), kept for call-order fidelity
    translate(&mut w_r, 0.012, 0.0, 0.034);
    parts.push(w_r);
    let mut w_l = wing;
    rotate_y(&mut w_l, -std::f64::consts::FRAC_PI_2);
    translate(&mut w_l, -0.012, 0.0, 0.034);
    parts.push(w_l);
    for i in 0..3u32 {
        for sx in [-1.0f32, 1.0f32] {
            let mut r = box_geo(0.0022, 0.0075, 0.0016, 0.0003, 1);
            translate(&mut r, sx * (0.017 + i as f32 * 0.003), 0.0, 0.031 + i as f32 * 0.0022);
            parts.push(r);
        }
    }

    /*
     * THE LATCH. A charging handle without one is a T-shaped tab and reads
     * as a moulded lug; the latch is what says "this part is a mechanism
     * that has to be released before it moves". It is a separate hooked
     * lever on the LEFT wing — the side that faces the camera in the
     * hipfire pose — pivoting on a visible roll pin, with the hook standing
     * proud of the wing so it breaks the silhouette rather than being a
     * groove in it.
     */
    let mut latch_body = extrude(
        &[
            [0.0, -0.0032],
            [0.0165, -0.0042],
            [0.0205, -0.0018],
            [0.0205, 0.0026],
            [0.0155, 0.0042],
            [0.0, 0.0034],
        ],
        0.0042,
        ExtrudeOpts {
            bevel: 0.0006,
            ..Default::default()
        },
    );
    rotate_y(&mut latch_body, -std::f64::consts::FRAC_PI_2);
    translate(&mut latch_body, -0.0125, 0.0012, 0.0335);
    parts.push(latch_body);
    // The hook that engages the receiver shelf: proud 1.6 mm, pointing forward.
    let mut hook = box_geo(0.0038, 0.0052, 0.0032, 0.0005, 1);
    translate(&mut hook, -0.0295, 0.0006, 0.0292);
    parts.push(hook);
    // Pivot pin through the wing, and the finger pad on the lever's tail.
    let mut pin = rod_z(0.0011, 0.0011, 0.0072, 8, 0.0002);
    rotate_y(&mut pin, std::f64::consts::FRAC_PI_2);
    translate(&mut pin, -0.0135, 0.0012, 0.0356);
    parts.push(pin);
    let mut pad = box_geo(0.0028, 0.0062, 0.0075, 0.0004, 1);
    translate(&mut pad, -0.0316, 0.0014, 0.0345);
    parts.push(pad);

    merge_all(parts).expect("chargingHandlePart always pushes at least the bar and shaft")
}

/// `o` on `addForeGrip(asm, matPoly, matRubber, o)` (`parts.js:1857-1879`).
/// `len` default `0.062`, `angle` default `0.25`; `y`/`z` have no JS default
/// (bare reads).
#[derive(Clone, Copy, Debug)]
pub struct ForeGripOpts {
    pub len: f32,
    pub y: f32,
    pub z: f32,
    pub angle: f32,
}

impl Default for ForeGripOpts {
    fn default() -> Self {
        ForeGripOpts {
            len: 0.062,
            y: 0.0,
            z: 0.0,
            angle: 0.25,
        }
    }
}

/// Vertical / angled foregrip for the SMG (`addForeGrip`, `parts.js:1857-1879`).
pub fn add_fore_grip(asm: &mut Assembly, mat_poly: &str, mat_rubber: &str, o: ForeGripOpts) {
    let ForeGripOpts { len, y, z, angle } = o;

    let mut parts: Vec<Geo> = Vec::new();
    for i in 0..5u32 {
        let t = i as f32 / 4.0;
        let mut g = blob(0.026 - t * 0.003, len / 5.0 + 0.003, 0.03 - t * 0.004, 0.005, 3);
        translate(&mut g, 0.0, -t * len, t * 0.008);
        parts.push(g);
    }
    let core = merge_all(parts).expect("addForeGrip always builds 5 blob slices");
    asm.add(
        core,
        mat_poly,
        Some(Xform {
            y,
            z,
            rx: angle,
            ..Default::default()
        }),
    );

    let mut grip_parts: Vec<Geo> = Vec::new();
    for i in 0..4u32 {
        let t = 0.15 + i as f32 * 0.23;
        let mut gr = box_geo(0.024, 0.006, 0.0055, 0.002, 2);
        translate(&mut gr, 0.0, -t * len, -0.013);
        grip_parts.push(gr);
    }
    let grips = merge_all(grip_parts).expect("addForeGrip always builds 4 grip ridges");
    asm.add(
        grips,
        mat_rubber,
        Some(Xform {
            y,
            z,
            rx: angle,
            ..Default::default()
        }),
    );
}
