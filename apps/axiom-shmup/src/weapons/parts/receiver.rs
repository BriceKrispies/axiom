//! Ported from Claude-of-Duty `src/weapons/parts.js` — the receiver group:
//! `addHandguard` (`:391-514`), `addUpperReceiver` (`:525-656`),
//! `addBoltCarrier` (`:662-687`), `addLowerReceiver` (`:693-792`).
//!
//! See `parts.js:19-31`: every dimension here is a real, published firearm
//! measurement, not an eyeballed guess — an AR-15 upper receiver really is
//! 198 mm long with a 21.2 mm rail and a 66 mm optic height over bore, and
//! no amount of texture detail rescues a receiver that is 30% too fat.
//! Weapon-local space is `+X` right, `+Y` up, `-Z` toward the muzzle, origin
//! at the shooting hand's anchor (the web of the thumb, top-rear of the
//! pistol grip) — the convention `geometry.js:28-30` documents and the
//! `geometry` module (`03-weapon-geometry-api.md`) carries forward.
//!
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`for`
//! throughout, matching the JS this replaces. Rust has no default arguments,
//! so every JS `?? value` default is documented on the option struct and
//! callers pass it explicitly (same convention as `parts::barrel`,
//! `parts::hardware`, `parts::magazine`).

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use axiom_math::{Mat4, Quat, Vec3};

use crate::weapons::geometry::primitives::{box_geo, knurl_band, lathe_z, mlok_slot, rod_z, round_rect, extrude, ExtrudeOpts};
use crate::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use crate::weapons::parts::hardware::{add_pin, add_rail, RailOpts};

/// `BufferGeometry.translate(x, y, z)`, via the normal-matrix-correct
/// [`Geo::apply`] — see `geometry/primitives/xform.rs`'s doc for why this
/// reuses `apply` rather than hand-rolling a second transform path.
fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `BufferGeometry.rotateY(angle)`.
fn rotate_y(g: &mut Geo, angle: f32) {
    let q = Quat::from_axis_angle(Vec3::UNIT_Y, angle).expect("Vec3::UNIT_Y is nonzero");
    g.apply(&Mat4::from_quaternion(q));
}

/// `BufferGeometry.rotateZ(angle)`.
fn rotate_z(g: &mut Geo, angle: f32) {
    let q = Quat::from_axis_angle(Vec3::UNIT_Z, angle).expect("Vec3::UNIT_Z is nonzero");
    g.apply(&Mat4::from_quaternion(q));
}

/* -------------------------------------------------------------------------- */
/*  handguard                                                                 */
/* -------------------------------------------------------------------------- */

/// `o` on `addHandguard(asm, matAlu, o)` (`parts.js:391-407,446-447,470,486`).
/// `mat_panel` mirrors `o.matPanel ?? matAlu` — `None` reproduces that
/// fallback. `z0`/`z1` have no JS default (`o.z0`/`o.z1` are read bare);
/// `Default` sets them to `0.0` only so the rest of the struct can use
/// struct-update syntax — every real caller sets both explicitly.
#[derive(Clone, Copy, Debug)]
pub struct HandguardOpts<'a> {
    pub mat_panel: Option<&'a str>,
    pub y: f32,
    /// Receiver end (rear, larger z).
    pub z0: f32,
    /// Muzzle end.
    pub z1: f32,
    pub r: f32,
    pub sides: u32,
    pub slat_w: f32,
    pub slat_t: f32,
    /// `o.topFrom`/`o.topTo` (`parts.js:446-447`): let a caller ask for a
    /// bare polymer top over the section the support hand actually grips,
    /// so the fingers can close over the handguard instead of through a
    /// rail. Every real call site sets both together or neither.
    pub top_from: Option<f32>,
    pub top_to: Option<f32>,
    pub braces: u32,
    pub slots: u32,
}

impl Default for HandguardOpts<'_> {
    fn default() -> Self {
        HandguardOpts {
            mat_panel: None,
            y: 0.0,
            z0: 0.0,
            z1: 0.0,
            r: 0.0235,
            sides: 8,
            slat_w: 0.0135,
            slat_t: 0.0032,
            top_from: None,
            top_to: None,
            braces: 3,
            slots: 3,
        }
    }
}

/// Free-float handguard built from longitudinal slats with real gaps, so the
/// barrel and gas block are visible through it and the silhouette breaks up
/// (`addHandguard`, `parts.js:391-514`).
///
/// MATERIAL SPLIT: the barrel nut, the ring braces and the end cap are
/// machined aluminium (they carry the barrel); the slats and their M-LOK
/// slots are a moulded polymer panel set. That is a real product
/// configuration, and it is also the only place on the gun where the two
/// dielectric classes sit directly against each other over a large area —
/// which is what makes the class break legible at hipfire framing instead
/// of theoretical.
pub fn add_handguard(asm: &mut Assembly, mat_alu: &str, o: HandguardOpts) {
    let mat_panel = o.mat_panel.unwrap_or(mat_alu);
    let yb = o.y;
    let z0 = o.z0;
    let z1 = o.z1;
    let len = z0 - z1;
    let r_out = o.r;
    let sides = o.sides;
    let slat_w = o.slat_w;
    let slat_t = o.slat_t;
    let cz = (z0 + z1) / 2.0;

    // barrel nut / rear collar
    let collar = lathe_z(
        &[
            [0.0, r_out * 0.72],
            [0.0, r_out + 0.0018],
            [0.0025, r_out + 0.0026],
            [0.014, r_out + 0.0026],
            [0.0165, r_out + 0.0012],
            [0.0165, r_out * 0.72],
        ],
        18,
        0.0,
        TAU,
    );
    asm.add(
        collar,
        mat_alu,
        Some(Xform {
            y: yb,
            z: z0 - 0.0165,
            ..Default::default()
        }),
    );
    let nut_knurl = knurl_band(r_out + 0.0028, 0.011, 34, 0.00035, 3);
    asm.add(
        nut_knurl,
        mat_alu,
        Some(Xform {
            y: yb,
            z: z0 - 0.0085,
            ..Default::default()
        }),
    );

    let slat = box_geo(slat_w, slat_t, len - 0.019, 0.0006, 1);
    // mlokSlot is authored as a flat plate in XY extruded along +Z, so its
    // pocket recesses along -Z and its long axis is X. Assembly composes
    // its Euler in 'XYZ' order, which APPLIES rz first, so a single add()
    // cannot both roll the plate onto the barrel axis and spin it round to
    // the slat's clock position. Bake the axis roll into the geometry once:
    // normal -> +X (radial), long axis -> Z (along the barrel). Then
    // `rz: a` alone puts it on any slat.
    //
    // Left unrolled the plates lay in the XY plane and read as loose
    // diamond tabs standing off the handguard, which is exactly what they
    // were doing.
    let mut slot_geo = mlok_slot(0.026, 0.0072, 0.0018);
    rotate_y(&mut slot_geo, FRAC_PI_2);
    // The top slat is normally the rail's job. `topFrom`/`topTo` let a
    // caller ask for a bare polymer top over the section the support hand
    // actually grips, so the fingers can close over the handguard instead
    // of through a rail.
    for i in 0..sides {
        let a = (i as f32 / sides as f32) * TAU + PI / sides as f32;
        let is_top = (a.sin() - 1.0).abs() < 0.35;
        let y = a.sin() * (r_out - slat_t * 0.5);
        let x = a.cos() * (r_out - slat_t * 0.5);
        if is_top {
            let Some((top_from, top_to)) = o.top_from.zip(o.top_to) else {
                continue;
            };
            let t_len = (top_from - top_to).abs();
            let top = box_geo(slat_w, slat_t, t_len, 0.0006, 1);
            asm.add(
                top,
                mat_panel,
                Some(Xform {
                    x,
                    y: yb + y,
                    z: (top_from + top_to) / 2.0,
                    rz: a - FRAC_PI_2,
                    ..Default::default()
                }),
            );
            continue;
        }
        asm.add(
            slat.clone(),
            mat_panel,
            Some(Xform {
                x,
                y: yb + y,
                z: cz - 0.0095,
                rz: a - FRAC_PI_2,
                ..Default::default()
            }),
        );
        // M-LOK slots on the 3/6/9-o'clock slats only, like the real thing
        let cardinal = a.cos().abs() > 0.85 || a.sin() < -0.85;
        if cardinal {
            for s in 0..o.slots {
                let sz = cz + len * 0.5 - 0.045 - s as f32 * 0.038;
                if sz < z1 + 0.02 {
                    break;
                }
                asm.add(
                    slot_geo.clone(),
                    mat_panel,
                    Some(Xform {
                        x: x * 1.005,
                        y: yb + y * 1.005,
                        z: sz,
                        rz: a,
                        ..Default::default()
                    }),
                );
                // The pocket floor: a dark recess so the slot is a hole in
                // the panel and not a raised lozenge that catches the same
                // light as the panel face.
                let pocket = box_geo(0.0012, 0.0052, 0.0232, 0.0002, 1);
                asm.add(
                    pocket,
                    "cavity",
                    Some(Xform {
                        x: x * 0.955,
                        y: yb + y * 0.955,
                        z: sz,
                        rz: a,
                        ..Default::default()
                    }),
                );
            }
        }
    }

    // ring braces tie the slats together
    let brace_count = o.braces;
    let brace_denom = brace_count.saturating_sub(1).max(1);
    for i in 0..brace_count {
        let z = z0 - 0.03 - (i as f32 / brace_denom as f32) * (len - 0.07);
        let brace = lathe_z(
            &[
                [0.0, r_out - slat_t],
                [0.0, r_out + 0.0006],
                [0.0035, r_out + 0.0006],
                [0.0035, r_out - slat_t],
            ],
            (sides * 2).max(10),
            0.0,
            TAU,
        );
        asm.add(brace, mat_alu, Some(Xform { y: yb, z, ..Default::default() }));
    }

    // anti-rotation index tabs at the front, and a chamfered end cap ring
    let cap = lathe_z(
        &[
            [0.0, r_out - slat_t - 0.0008],
            [0.0, r_out - 0.0002],
            [0.0022, r_out - 0.0012],
            [0.0022, r_out - slat_t - 0.0008],
        ],
        (sides * 2).max(10),
        0.0,
        TAU,
    );
    asm.add(
        cap,
        mat_alu,
        Some(Xform {
            y: yb,
            z: z1 + 0.001,
            ..Default::default()
        }),
    );
}

/* -------------------------------------------------------------------------- */
/*  receiver                                                                  */
/* -------------------------------------------------------------------------- */

/// `o` on `addUpperReceiver(asm, mat, matSteel, matCavity, o)`
/// (`parts.js:525-529`). `r` default `0.0192`. `zRear`/`zFront`/`bore`/
/// `portZ`/`railTop` have no JS default; `Default` zeroes them purely so
/// struct-update syntax works — every real caller sets all five.
#[derive(Clone, Copy, Debug)]
pub struct UpperReceiverOpts {
    pub z_rear: f32,
    pub z_front: f32,
    pub bore: f32,
    pub r: f32,
    pub port_z: f32,
    pub rail_top: f32,
}

impl Default for UpperReceiverOpts {
    fn default() -> Self {
        UpperReceiverOpts {
            z_rear: 0.0,
            z_front: 0.0,
            bore: 0.0,
            r: 0.0192,
            port_z: 0.0,
            rail_top: 0.0,
        }
    }
}

/// `addUpperReceiver`'s return: `{ railTop }` (`parts.js:655`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpperReceiverResult {
    pub rail_top: f32,
}

/// AR-pattern upper receiver: a flat-top tube with the rail on the crest,
/// the forward assist and brass deflector at the rear right, a recessed
/// ejection port, and the charging-handle channel (`addUpperReceiver`,
/// `parts.js:525-656`).
pub fn add_upper_receiver(
    asm: &mut Assembly,
    mat: &str,
    mat_steel: &str,
    mat_cavity: &str,
    o: UpperReceiverOpts,
) -> UpperReceiverResult {
    let z_rear = o.z_rear;
    let z_front = o.z_front;
    let bore = o.bore;
    let r = o.r;
    let len = z_rear - z_front;
    let cz = (z_rear + z_front) / 2.0;

    // Main tube, flattened on top where the rail sits.
    //
    // Both ends are CLOSED (radius 0). An annular end face leaves a 19 mm
    // hole straight down the receiver, and in ADS the eye is 0.2 m behind
    // it looking right in: you see the bolt carrier and the chambered round
    // floating in a black pipe. Nothing inside the receiver is ever meant
    // to be visible except through the ejection-port cavity.
    let body = lathe_z(
        &[
            [0.0, 0.0],
            [0.0, r * 0.98],
            [0.0022, r],
            [len * 0.52, r],
            [len * 0.54, r * 0.985],
            [len - 0.004, r * 0.985],
            [len, r * 0.93],
            [len, 0.0],
        ],
        22,
        0.0,
        TAU,
    );
    asm.add(
        body,
        mat,
        Some(Xform {
            y: bore,
            z: z_rear,
            ry: PI,
            ..Default::default()
        }),
    );

    // Flat top deck the rail is machined onto.
    let deck = box_geo(0.0235, 0.008, len - 0.002, 0.0008, 1);
    asm.add(
        deck,
        mat,
        Some(Xform {
            y: bore + r - 0.0025,
            z: cz,
            ..Default::default()
        }),
    );

    // Charging-handle raceway hump at the rear.
    let hump = box_geo(0.0245, 0.011, 0.05, 0.0012, 2);
    asm.add(
        hump,
        mat,
        Some(Xform {
            y: bore + r - 0.0075,
            z: z_rear - 0.024,
            ..Default::default()
        }),
    );

    // Forward assist boss (rear right) — a real stepped cylinder with a pad.
    let fa = lathe_z(
        &[
            [0.0, 0.0],
            [0.0, 0.0055],
            [0.0015, 0.0062],
            [0.006, 0.0062],
            [0.007, 0.0048],
            [0.019, 0.0048],
            [0.019, 0.0],
        ],
        14,
        0.0,
        TAU,
    );
    asm.add(
        fa,
        mat,
        Some(Xform {
            x: 0.0115,
            y: bore - 0.004,
            z: z_rear - 0.006,
            rx: 0.35,
            ..Default::default()
        }),
    );
    let fa_pad = box_geo(0.0085, 0.0085, 0.0035, 0.0008, 2);
    asm.add(
        fa_pad,
        mat_steel,
        Some(Xform {
            x: 0.0132,
            y: bore - 0.0025,
            z: z_rear + 0.0025,
            rx: 0.35,
            ..Default::default()
        }),
    );

    // Brass deflector: the little wedge behind the port.
    let defl = extrude(
        &[[0.0, 0.0], [0.013, 0.004], [0.013, 0.019], [0.0, 0.017]],
        0.016,
        ExtrudeOpts {
            bevel: 0.0009,
            ..Default::default()
        },
    );
    asm.add(
        defl,
        mat,
        Some(Xform {
            x: r - 0.001,
            y: bore - 0.006,
            z: z_rear - 0.045,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );

    // Ejection port: a recessed cavity with a hinged dust cover just below.
    let port_w = 0.032;
    let port_h = 0.019;
    let cav = box_geo(port_h, 0.012, port_w, 0.0008, 1);
    asm.add(
        cav,
        mat_cavity,
        Some(Xform {
            x: r - 0.0075,
            y: bore + 0.001,
            z: o.port_z,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    // port lip
    let lip = extrude(
        &round_rect(f64::from(port_w) + 0.005, f64::from(port_h) + 0.005, 0.0022, 3),
        0.0022,
        ExtrudeOpts {
            bevel: 0.0006,
            ..Default::default()
        },
    );
    let lip_inner = extrude(
        &round_rect(f64::from(port_w), f64::from(port_h), 0.0018, 3),
        0.003,
        ExtrudeOpts {
            bevel: 0.0005,
            ..Default::default()
        },
    );
    asm.add(
        lip,
        mat,
        Some(Xform {
            x: r - 0.0022,
            y: bore + 0.001,
            z: o.port_z,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    asm.add(
        lip_inner,
        mat_cavity,
        Some(Xform {
            x: r - 0.0042,
            y: bore + 0.001,
            z: o.port_z,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );

    // DUST COVER, hung open.
    //
    // The port on its own is a dark rectangle and reads as a decal. What
    // makes it read as a mechanism is the cover: a stamped panel with a
    // RAISED LIP around three edges (that lip is the stiffening flange, and
    // it is the only part of the cover that ever catches a highlight),
    // sprung open on a hinge rod below the port so it hangs down and
    // rearward off the receiver flank. Two separate masses — the rod and
    // the flanged panel — where there used to be none.
    let hinge_y = bore - 0.0092;
    let hinge_x = r - 0.0035;
    let rod = rod_z(0.0016, 0.0016, port_w + 0.014, 10, 0.0003);
    asm.add(
        rod,
        mat_steel,
        Some(Xform {
            x: hinge_x,
            y: hinge_y,
            z: o.port_z,
            ..Default::default()
        }),
    );
    // The panel swings open about the rod: 1.35 rad puts it hanging
    // down-outboard, clear of the magwell, which is where a sprung cover
    // actually sits.
    let cover_open = 1.35;
    let mut cover_parts: Vec<Geo> = Vec::new();
    let panel = box_geo(port_h + 0.004, 0.0014, port_w + 0.006, 0.0005, 1);
    cover_parts.push(panel);
    // Stiffening flange: proud 1.2 mm on the two long edges and the free
    // edge.
    for sz in [-1.0f32, 1.0f32] {
        let mut f = box_geo(port_h + 0.004, 0.0032, 0.0016, 0.0004, 1);
        translate(&mut f, 0.0, 0.0009, sz * (port_w * 0.5 + 0.0022));
        cover_parts.push(f);
    }
    let mut free_edge = box_geo(0.0018, 0.0034, port_w + 0.006, 0.0004, 1);
    translate(&mut free_edge, (port_h + 0.004) * 0.5 - 0.0009, 0.001, 0.0);
    cover_parts.push(free_edge);
    let mut cover = merge_all(cover_parts).expect("the dust cover always builds the panel plus two flanges plus the free edge");
    // Author it lying in the XZ plane hinged along -X, then swing it open.
    translate(&mut cover, (port_h + 0.004) * 0.5, 0.0, 0.0);
    rotate_z(&mut cover, -cover_open);
    asm.add(
        cover,
        mat,
        Some(Xform {
            x: hinge_x,
            y: hinge_y,
            z: o.port_z,
            ..Default::default()
        }),
    );

    // Rail on the crest.
    add_rail(asm, mat, z_front + 0.002, z_rear - 0.002, o.rail_top, 0.0, RailOpts::default());

    // Receiver pins.
    add_pin(asm, mat_steel, 0.0, bore - r + 0.004, z_front + 0.014, 0.0024, r * 2.0 - 0.004);

    UpperReceiverResult { rail_top: o.rail_top }
}

/// `o` on `addBoltCarrier(asm, matSteel, o)` (`parts.js:662-665`). `r`
/// default `0.0155`, `len` default `0.09`. `z` has no JS default (`o.z` is
/// read bare); `Default` zeroes it purely so struct-update syntax works —
/// every real caller sets it.
#[derive(Clone, Copy, Debug)]
pub struct BoltCarrierOpts {
    pub y: f32,
    pub r: f32,
    pub len: f32,
    pub z: f32,
}

impl Default for BoltCarrierOpts {
    fn default() -> Self {
        BoltCarrierOpts {
            y: 0.0,
            r: 0.0155,
            len: 0.09,
            z: 0.0,
        }
    }
}

/// Bolt carrier group seen through the ejection port, and the case in the
/// chamber. Returned as its own assembly because it cycles (`addBoltCarrier`,
/// `parts.js:662-687`).
pub fn add_bolt_carrier(asm: &mut Assembly, mat_steel: &str, o: BoltCarrierOpts) {
    let y = o.y;
    let r = o.r;
    let len = o.len;
    let body = lathe_z(
        &[
            [0.0, r * 0.6],
            [0.0, r],
            [0.002, r + 0.0004],
            [len * 0.45, r + 0.0004],
            [len * 0.47, r],
            [len, r],
            [len, r * 0.5],
        ],
        18,
        0.0,
        TAU,
    );
    asm.add(
        body,
        mat_steel,
        Some(Xform {
            y,
            z: o.z,
            ry: PI,
            ..Default::default()
        }),
    );
    // cam pin track + gas key
    let key = box_geo(0.011, 0.0075, 0.016, 0.0006, 1);
    asm.add(
        key,
        mat_steel,
        Some(Xform {
            y: y + r + 0.0026,
            z: o.z + len * 0.25,
            ..Default::default()
        }),
    );
    let lug = box_geo(0.006, 0.005, 0.03, 0.0005, 1);
    asm.add(
        lug,
        mat_steel,
        Some(Xform {
            x: r * 0.78,
            y: y + r * 0.42,
            z: o.z + len * 0.1,
            rz: 0.5,
            ..Default::default()
        }),
    );
}

/// `o` on `addLowerReceiver(asm, mat, matSteel, o)` (`parts.js:693-703`).
/// `w` default `0.0245`, `magW` default `0.0295`, `magD` default `0.0685`,
/// `magTilt` default `0.09`. `magTop`/`magBottom` default to `bore - 0.014`
/// / `bore - 0.062` — since a Rust `Default` cannot see `bore`, callers pass
/// `None` to reproduce that fallback. `bore`/`zRear`/`zFront`/`magZ`/
/// `triggerZ`/`gripAngle` have no JS default; `Default` zeroes them purely
/// so struct-update syntax works — every real caller sets all six.
#[derive(Clone, Copy, Debug)]
pub struct LowerReceiverOpts {
    pub bore: f32,
    pub z_rear: f32,
    pub z_front: f32,
    pub w: f32,
    pub mag_w: f32,
    pub mag_d: f32,
    pub mag_top: Option<f32>,
    pub mag_bottom: Option<f32>,
    pub mag_z: f32,
    pub mag_tilt: f32,
    pub trigger_z: f32,
    pub grip_angle: f32,
}

impl Default for LowerReceiverOpts {
    fn default() -> Self {
        LowerReceiverOpts {
            bore: 0.0,
            z_rear: 0.0,
            z_front: 0.0,
            w: 0.0245,
            mag_w: 0.0295,
            mag_d: 0.0685,
            mag_top: None,
            mag_bottom: None,
            mag_z: 0.0,
            mag_tilt: 0.09,
            trigger_z: 0.0,
            grip_angle: 0.0,
        }
    }
}

/// `addLowerReceiver`'s return: `{ magTop, magBottom, magZ, magTilt, wellH,
/// magW, magD }` (`parts.js:791`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LowerReceiverResult {
    pub mag_top: f32,
    pub mag_bottom: f32,
    pub mag_z: f32,
    pub mag_tilt: f32,
    pub well_h: f32,
    pub mag_w: f32,
    pub mag_d: f32,
}

/// AR lower receiver: magwell, trigger guard, grip boss, selector, mag
/// release, bolt catch, takedown pins (`addLowerReceiver`, `parts.js:693-792`).
///
/// `matSteel` (`parts.js:693`) is a preserved source quirk: the body never
/// references it — every real geometry call in this range (`bodyG`, `well`,
/// `liner`, `mouth`, `tower`, `guard`, `bossG`) uses `mat` or the literal
/// `'cavity'` bucket. Per the port recipe's rule 7 ("port the behaviour and
/// pin it with a test naming it as a source quirk"), the parameter is kept
/// for call-order fidelity as `_mat_steel: &str`, not silently dropped.
pub fn add_lower_receiver(asm: &mut Assembly, mat: &str, _mat_steel: &str, o: LowerReceiverOpts) -> LowerReceiverResult {
    let bore = o.bore;
    let z_rear = o.z_rear;
    let z_front = o.z_front;
    let w = o.w;
    let mag_w = o.mag_w;
    let mag_d = o.mag_d;
    let mag_top = o.mag_top.unwrap_or(bore - 0.014);
    let mag_bottom = o.mag_bottom.unwrap_or(bore - 0.062);
    let mag_z = o.mag_z;
    let mag_tilt = o.mag_tilt;

    // Receiver body — the flat-sided box under the upper.
    let body_h = 0.026;
    let body_g = box_geo(w, body_h, z_rear - z_front, 0.0016, 2);
    asm.add(
        body_g,
        mat,
        Some(Xform {
            y: bore - 0.014,
            z: (z_rear + z_front) / 2.0,
            ..Default::default()
        }),
    );

    // Magwell: a genuinely hollow tube (so the well is a hole when the
    // magazine drops out during a reload), tilted forward like the real
    // one.
    let well_h = mag_top - mag_bottom;
    let well = extrude(
        &round_rect(f64::from(mag_w), f64::from(mag_d), 0.0075, 5),
        well_h,
        ExtrudeOpts {
            bevel: 0.0012,
            holes: vec![round_rect(f64::from(mag_w) - 0.005, f64::from(mag_d) - 0.005, 0.006, 5)],
            ..Default::default()
        },
    );
    asm.add(
        well,
        mat,
        Some(Xform {
            y: (mag_top + mag_bottom) / 2.0,
            z: mag_z,
            rx: FRAC_PI_2 + mag_tilt,
            ..Default::default()
        }),
    );
    let liner = extrude(
        &round_rect(f64::from(mag_w) - 0.0052, f64::from(mag_d) - 0.0052, 0.006, 5),
        well_h - 0.004,
        ExtrudeOpts {
            bevel: 0.0006,
            holes: vec![round_rect(f64::from(mag_w) - 0.0082, f64::from(mag_d) - 0.0082, 0.005, 5)],
            ..Default::default()
        },
    );
    asm.add(
        liner,
        "cavity",
        Some(Xform {
            y: (mag_top + mag_bottom) / 2.0,
            z: mag_z,
            rx: FRAC_PI_2 + mag_tilt,
            ..Default::default()
        }),
    );
    let mouth = extrude(
        &round_rect(f64::from(mag_w) + 0.004, f64::from(mag_d) + 0.005, 0.008, 5),
        0.006,
        ExtrudeOpts {
            bevel: 0.0012,
            holes: vec![round_rect(f64::from(mag_w) - 0.003, f64::from(mag_d) - 0.003, 0.006, 5)],
            ..Default::default()
        },
    );
    asm.add(
        mouth,
        mat,
        Some(Xform {
            y: mag_bottom + 0.002,
            z: mag_z + mag_tilt.sin() * well_h * 0.5,
            rx: FRAC_PI_2 + mag_tilt,
            ..Default::default()
        }),
    );

    // Rear takedown lug + buffer tower.
    let tower = box_geo(w - 0.001, 0.03, 0.026, 0.0014, 2);
    asm.add(
        tower,
        mat,
        Some(Xform {
            y: bore - 0.0155,
            z: z_rear - 0.012,
            ..Default::default()
        }),
    );

    // Trigger guard: a bevelled loop under the receiver.
    //
    // The outline is authored in the weapon's SIDE plane — the first
    // coordinate is fore/aft, the second is up/down — and then rotated so
    // the extrusion runs across the receiver. Extruding the outline
    // straight out of the XY plane would stand the loop up across the gun
    // like a trigger-shaped cattle guard, which is invisible from the side
    // and wrong from every other angle. +X in the outline is the muzzle
    // side, so it maps to -Z below.
    let guard_outer: [[f64; 2]; 7] = [
        [-0.028, 0.0],
        [0.03, 0.0],
        [0.032, -0.006],
        [0.028, -0.0225],
        [0.018, -0.0275],
        [-0.02, -0.0275],
        [-0.028, -0.021],
    ];
    let guard_inner: [[f64; 2]; 7] = [
        [-0.0225, -0.003],
        [0.0245, -0.003],
        [0.0255, -0.008],
        [0.022, -0.0205],
        [0.015, -0.0235],
        [-0.0165, -0.0235],
        [-0.0225, -0.019],
    ];
    let mut guard = extrude(
        &guard_outer,
        0.0172,
        ExtrudeOpts {
            bevel: 0.0011,
            bevel_segments: 2,
            holes: vec![guard_inner.to_vec()],
            ..Default::default()
        },
    );
    // outline-X -> -Z (forward), extrusion -> across
    rotate_y(&mut guard, FRAC_PI_2);
    asm.add(
        guard,
        mat,
        Some(Xform {
            y: bore - 0.026,
            z: o.trigger_z,
            ..Default::default()
        }),
    );

    // Grip boss + screw.
    let boss_g = box_geo(0.028, 0.012, 0.03, 0.0012, 2);
    asm.add(
        boss_g,
        mat,
        Some(Xform {
            y: bore - 0.0255,
            z: z_rear - 0.028,
            rx: -o.grip_angle * 0.5,
            ..Default::default()
        }),
    );

    // Selector lever: a real paddle with a detent boss, both sides.
    LowerReceiverResult {
        mag_top,
        mag_bottom,
        mag_z,
        mag_tilt,
        well_h,
        mag_w,
        mag_d,
    }
}
