//! Ported from Claude-of-Duty `src/weapons/models/smg.js` (~330 lines) —
//! `buildSmg()`, an MPX/MP5-flavoured 9 mm roller.
//!
//! A submachine gun is *smaller* than a carbine in every dimension, and that
//! is the whole point of building it separately: 9 mm ammunition means a
//! 26 mm receiver and a 190 mm magazine, and the silhouette has to read that
//! way (`smg.js:24-32`).
//!
//! This is app code (`apps/`), outside the Branchless Law and the Coverage
//! Law — plain `if`/`for` throughout, matching the source's own control flow.

use axiom_math::{Mat4, Vec3};

use crate::weapons::geometry::primitives::{box_geo, dome, extrude, lathe_z, rod_z, round_rect, tube_z, ExtrudeOpts};
use crate::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use crate::weapons::parts::barrel::{add_barrel, add_muzzle_device, BarrelOpts, MuzzleKind};
use crate::weapons::parts::controls::{add_fore_grip, add_pistol_grip, selector_part, trigger_part, ForeGripOpts, PistolGripOpts};
use crate::weapons::parts::hardware::{add_pin, add_qd_socket, add_rail, add_sling_loop, cartridge, MountAxis, RailOpts};
use crate::weapons::parts::magazine::{add_front_sight, add_rear_sight, build_magazine, MagazineDims, MagazineOpts};
use crate::weapons::parts::optics::{build_optic, OpticOpts, OpticResult};
use crate::weapons::parts::receiver::add_handguard;
use crate::weapons::parts::receiver::HandguardOpts;

use super::{GripTarget, PosRot, ShellDims};

/// `BufferGeometry.translate(x, y, z)`, via the normal-matrix-correct
/// [`Geo::apply`] — see `geometry/primitives/xform.rs`'s doc for why this
/// reuses `apply` rather than hand-rolling a second transform path.
fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `BufferGeometry.rotateY(angle)`. `angle` is `f64` and the rotation is
/// built directly from `f64`-computed `sin`/`cos` (matching
/// `THREE.Matrix4.makeRotationY`, which takes a full-precision `f64` angle
/// throughout); only the resulting matrix elements are rounded to `f32`. This
/// does **not** go through [`axiom_math::Quat::from_axis_angle`], which only
/// accepts `f32` and would force the angle to truncate *before* the
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

/// `moving: { magazine, charging, bolt, trigger, selector }` (`smg.js:319`).
pub struct SmgMoving {
    pub magazine: Assembly,
    pub charging: Assembly,
    pub bolt: Assembly,
    pub trigger: Assembly,
    pub selector: Assembly,
}

/// `nodes` (`smg.js:320-352`). Unlike the rifle's [`super::rifle::RifleNodes`],
/// there is no `handguard` node here: `smg.js` never adds one — the SMG's
/// support-hand target (`gripL`) is on the vertical foregrip, not solved
/// against the handguard cylinder.
pub struct SmgNodes {
    pub muzzle: [f32; 3],
    pub chamber: [f32; 3],
    pub eject: [f32; 3],
    pub eject_dir: [f32; 3],
    pub sight: [f32; 3],
    pub sight_axis: [f32; 3],
    pub iron_sight: [f32; 3],
    pub grip_r: GripTarget,
    /// Support hand on the vertical foregrip: metacarpals run forward
    /// around the front of the post, palm facing inboard (`smg.js:335-336`).
    pub grip_l: GripTarget,
    pub mag_seat: PosRot,
    pub mag_drop: [f32; 3],
    pub charge_rest: PosRot,
    pub charge_pull: [f32; 3],
    pub bolt_rest: PosRot,
    pub bolt_travel: [f32; 3],
    pub trigger_pivot: PosRot,
    pub trigger_pull: f32,
    pub selector_pivot: PosRot,
    pub optic_glass: OpticResult,
}

/// `buildSmg()`'s full return value (`smg.js:314-356`).
pub struct SmgModel {
    pub id: &'static str,
    pub label: &'static str,
    pub fx_class: &'static str,
    pub body: Assembly,
    pub moving: SmgMoving,
    pub nodes: SmgNodes,
    pub shell: ShellDims,
    pub mag_size: MagazineDims,
}

/// The submachine gun — an MPX/MP5-flavoured 9 mm roller: slim tubular
/// receiver, side-mounted non-reciprocating charging handle in a cocking
/// tube, short M-LOK handguard with a vertical foregrip, tri-lug flash
/// hider, folding skeleton stock and a low-mounted compact red dot
/// (`buildSmg`, `smg.js:33-356`).
pub fn build_smg() -> SmgModel {
    let bore: f32 = 0.068;
    let r_rec: f32 = 0.0158;
    let rail_top: f32 = bore + 0.0245;
    let z_rec_rear: f32 = 0.062;
    let z_rec_front: f32 = -0.112;
    let port_z: f32 = -0.042;
    let mag_z: f32 = -0.052;
    let mag_tilt: f32 = 0.05;
    let hg_z0: f32 = -0.114;
    let hg_z1: f32 = -0.268;
    let hg_r: f32 = 0.019;
    let z_barrel_end: f32 = -0.3;
    let optic_y: f32 = bore + 0.055;
    let optic_z: f32 = -0.008;

    let mut body = Assembly::new("smg-body");

    // ---- receiver: a slim tube with a machined flat top and a mag housing ----
    let rec = lathe_z(
        &[
            [0.0, r_rec * 0.55],
            [0.0, r_rec * 0.99],
            [0.002, r_rec],
            [z_rec_rear - z_rec_front - 0.004, r_rec],
            [z_rec_rear - z_rec_front - 0.002, r_rec * 0.96],
            [z_rec_rear - z_rec_front, r_rec * 0.6],
        ],
        22,
        0.0,
        std::f32::consts::TAU,
    );
    body.add(
        rec,
        "alu",
        Some(Xform {
            y: bore,
            z: z_rec_rear,
            ry: std::f32::consts::PI,
            ..Default::default()
        }),
    );
    let deck = box_geo(0.0225, 0.009, z_rec_rear - z_rec_front - 0.004, 0.0009, 1);
    body.add(
        deck,
        "alu",
        Some(Xform {
            y: bore + r_rec - 0.003,
            z: (z_rec_rear + z_rec_front) / 2.0,
            ..Default::default()
        }),
    );
    add_rail(&mut body, "alu", z_rec_front + 0.004, z_rec_rear - 0.004, rail_top, 0.0, RailOpts::default());

    // Cocking tube above the barrel with the charging handle slot.
    let cock_tube = tube_z(0.0072, 0.0052, 0.14, 14, 0.0004);
    body.add(
        cock_tube,
        "alu",
        Some(Xform {
            x: -r_rec + 0.0028,
            y: bore + r_rec - 0.007,
            z: -0.06,
            ..Default::default()
        }),
    );

    // Ejection port, right side.
    let port_w: f32 = 0.03;
    let port_h: f32 = 0.017;
    let cav = box_geo(0.01, port_h, port_w, 0.0008, 1);
    body.add(
        cav,
        "cavity",
        Some(Xform {
            x: r_rec - 0.006,
            y: bore + 0.002,
            z: port_z,
            ry: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        }),
    );
    let lip = extrude(
        &round_rect(f64::from(port_w) + 0.004, f64::from(port_h) + 0.004, 0.002, 3),
        0.002,
        ExtrudeOpts {
            bevel: 0.0005,
            holes: vec![round_rect(f64::from(port_w), f64::from(port_h), 0.0016, 3)],
            ..Default::default()
        },
    );
    body.add(
        lip,
        "alu",
        Some(Xform {
            x: r_rec - 0.0012,
            y: bore + 0.002,
            z: port_z,
            ry: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        }),
    );
    let carrier = lathe_z(
        &[[0.0, r_rec * 0.5], [0.0, r_rec * 0.82], [0.07, r_rec * 0.82], [0.07, r_rec * 0.5]],
        16,
        0.0,
        std::f32::consts::TAU,
    );
    body.add(
        carrier,
        "steel_bright",
        Some(Xform {
            y: bore,
            z: port_z - 0.02,
            ..Default::default()
        }),
    );

    // ---- lower: magwell housing, trigger group, grip ----------------------
    let mag_w: f32 = 0.0242;
    let mag_d: f32 = 0.0345;
    let lower_body = box_geo(0.0245, 0.028, 0.13, 0.0016, 2);
    body.add(
        lower_body,
        "polymer",
        Some(Xform {
            y: bore - 0.0195,
            z: -0.02,
            ..Default::default()
        }),
    );

    let well_h: f32 = 0.036;
    let well = extrude(
        &round_rect(f64::from(mag_w) + 0.003, f64::from(mag_d) + 0.003, 0.005, 4),
        well_h,
        ExtrudeOpts {
            bevel: 0.0011,
            holes: vec![round_rect(f64::from(mag_w) - 0.002, f64::from(mag_d) - 0.002, 0.004, 4)],
            ..Default::default()
        },
    );
    body.add(
        well,
        "polymer",
        Some(Xform {
            y: bore - 0.038,
            z: mag_z,
            rx: std::f32::consts::FRAC_PI_2 + mag_tilt,
            ..Default::default()
        }),
    );
    let liner = extrude(
        &round_rect(f64::from(mag_w) - 0.0022, f64::from(mag_d) - 0.0022, 0.004, 4),
        well_h - 0.004,
        ExtrudeOpts {
            bevel: 0.0005,
            holes: vec![round_rect(f64::from(mag_w) - 0.005, f64::from(mag_d) - 0.005, 0.003, 4)],
            ..Default::default()
        },
    );
    body.add(
        liner,
        "cavity",
        Some(Xform {
            y: bore - 0.038,
            z: mag_z,
            rx: std::f32::consts::FRAC_PI_2 + mag_tilt,
            ..Default::default()
        }),
    );
    let flare = extrude(
        &round_rect(f64::from(mag_w) + 0.007, f64::from(mag_d) + 0.008, 0.006, 4),
        0.007,
        ExtrudeOpts {
            bevel: 0.0012,
            holes: vec![round_rect(f64::from(mag_w) + 0.001, f64::from(mag_d) + 0.001, 0.004, 4)],
            ..Default::default()
        },
    );
    body.add(
        flare,
        "polymer",
        Some(Xform {
            y: bore - 0.055,
            z: mag_z + 0.0016,
            rx: std::f32::consts::FRAC_PI_2 + mag_tilt,
            ..Default::default()
        }),
    );

    // Trigger guard.
    let guard_outer: [[f64; 2]; 7] = [
        [-0.026, 0.0],
        [0.028, 0.0],
        [0.03, -0.006],
        [0.026, -0.021],
        [0.016, -0.026],
        [-0.018, -0.026],
        [-0.026, -0.02],
    ];
    let guard_inner: [[f64; 2]; 7] = [
        [-0.021, -0.003],
        [0.0225, -0.003],
        [0.0235, -0.008],
        [0.02, -0.0195],
        [0.013, -0.0225],
        [-0.015, -0.0225],
        [-0.0205, -0.018],
    ];
    let guard = extrude(
        &guard_outer,
        0.0155,
        ExtrudeOpts {
            bevel: 0.0009,
            holes: vec![guard_inner.to_vec()],
            ..Default::default()
        },
    );
    body.add(
        guard,
        "polymer",
        Some(Xform {
            y: bore - 0.03,
            z: -0.008,
            ..Default::default()
        }),
    );

    // Ambi mag release paddles + bolt catch.
    for sx in [-1.0f32, 1.0f32] {
        let paddle = extrude(
            &[[-0.008, -0.004], [0.009, -0.005], [0.01, 0.004], [-0.008, 0.005]],
            0.004,
            ExtrudeOpts {
                bevel: 0.0006,
                ..Default::default()
            },
        );
        body.add(
            paddle,
            "alu",
            Some(Xform {
                x: sx * 0.0132,
                y: bore - 0.026,
                z: -0.03,
                ry: std::f32::consts::FRAC_PI_2,
                ..Default::default()
            }),
        );
    }

    add_pistol_grip(
        &mut body,
        "polymer",
        "rubber",
        PistolGripOpts {
            y: 0.033,
            z: 0.018,
            angle: 0.36,
            len: 0.102,
            w: 0.03,
        },
    );

    // ---- barrel + handguard -------------------------------------------------
    add_barrel(
        &mut body,
        "steel",
        "cavity",
        BarrelOpts {
            y: bore,
            z_breech: -0.09,
            z_muzzle: z_barrel_end,
            r_chamber: 0.0092,
            r_barrel: 0.0062,
            r_gas: 0.0072,
            gas_at: Some(-0.2),
            knurl: false,
            ..BarrelOpts::default()
        },
    );
    let muzzle = add_muzzle_device(&mut body, "steel_soot", "cavity", MuzzleKind::Trilug, z_barrel_end, 0.0062, bore);
    add_handguard(
        &mut body,
        "alu",
        HandguardOpts {
            y: bore,
            z0: hg_z0,
            z1: hg_z1,
            r: hg_r,
            sides: 8,
            slat_w: 0.0132,
            slat_t: 0.0032,
            slots: 3,
            braces: 2,
            ..Default::default()
        },
    );
    add_rail(&mut body, "alu", hg_z1 + 0.004, hg_z0 - 0.002, rail_top, 0.0, RailOpts::default());
    add_fore_grip(
        &mut body,
        "polymer",
        "rubber",
        ForeGripOpts {
            y: bore - hg_r - 0.004,
            z: -0.208,
            angle: 0.2,
            len: 0.058,
        },
    );
    add_qd_socket(&mut body, "alu", "steel", -hg_r + 0.001, bore - 0.006, hg_z0 - 0.022, MountAxis::X, 0.0045);

    // ---- folding skeleton stock ----------------------------------------------
    let hinge_block = box_geo(0.026, 0.03, 0.024, 0.003, 3);
    body.add(
        hinge_block,
        "alu",
        Some(Xform {
            y: bore - 0.008,
            z: z_rec_rear + 0.008,
            ..Default::default()
        }),
    );
    add_pin(&mut body, "steel", 0.0, bore - 0.008, z_rec_rear + 0.014, 0.003, 0.028);
    // two struts and a butt plate
    for sx in [-1.0f32, 1.0f32] {
        let strut = box_geo(0.0075, 0.011, 0.145, 0.0018, 2);
        body.add(
            strut,
            "alu",
            Some(Xform {
                x: sx * 0.0125,
                y: bore - 0.014,
                z: z_rec_rear + 0.085,
                rx: -0.045,
                ..Default::default()
            }),
        );
    }
    let crossbar = box_geo(0.032, 0.009, 0.0095, 0.0016, 2);
    body.add(
        crossbar,
        "alu",
        Some(Xform {
            y: bore - 0.019,
            z: z_rec_rear + 0.12,
            ..Default::default()
        }),
    );
    let butt_plate = extrude(
        &round_rect(0.042, 0.058, 0.006, 4),
        0.009,
        ExtrudeOpts {
            bevel: 0.0012,
            ..Default::default()
        },
    );
    body.add(
        butt_plate,
        "polymer",
        Some(Xform {
            y: bore - 0.026,
            z: z_rec_rear + 0.155,
            rx: 0.06,
            ..Default::default()
        }),
    );
    let pad = crate::weapons::geometry::primitives::blob(0.04, 0.05, 0.0085, 0.0035, 3);
    body.add(
        pad,
        "rubber",
        Some(Xform {
            y: bore - 0.026,
            z: z_rec_rear + 0.162,
            rx: 0.06,
            ..Default::default()
        }),
    );
    let cheek = crate::weapons::geometry::primitives::blob(0.019, 0.013, 0.09, 0.005, 3);
    body.add(
        cheek,
        "polymer",
        Some(Xform {
            y: bore + 0.012,
            z: z_rec_rear + 0.08,
            rx: -0.05,
            ..Default::default()
        }),
    );
    add_sling_loop(
        &mut body,
        "steel",
        0.0165,
        bore - 0.022,
        z_rec_rear + 0.026,
        0.007,
        Xform {
            ry: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        },
    );

    // ---- sights ---------------------------------------------------------------
    let optic = build_optic(
        &mut body,
        OpticOpts {
            r_tube: 0.0138,
            // Same aperture-budget argument as the rifle (see `build_optic`):
            // a shorter tube is what makes the sight picture fill the
            // housing in ADS.
            len: 0.044,
            hood: 0.006,
            y: optic_y,
            z: optic_z,
            rail_top,
            mat_body: "alu_fine",
            mat_steel: "steel",
        },
    );
    add_front_sight(&mut body, "polymer", "alu", 0.0, rail_top, -0.248, false);
    // Same ADS composition fix as the rifle (see there): a folded BUIS at the
    // back of the receiver sits inside the eye's near field and fills the
    // bottom of the sight frame with a pale slab.
    add_rear_sight(&mut body, "polymer", "alu", 0.0, rail_top, -0.09, false);

    // ---- moving parts -----------------------------------------------------------
    let mut magazine = Assembly::new("smg-mag");
    let mag = build_magazine(
        &mut magazine,
        (),
        MagazineOpts {
            w: 0.0235,
            d: 0.0335,
            len: 0.192,
            curve: 0.026,
            segs: 7,
            witness: 5,
            case_len: 0.0192,
            rim_r: 0.00478,
            bullet_len: 0.0132,
            poly: "polymer",
        },
    );

    // Non-reciprocating charging handle: a paddle in the cocking tube.
    let mut charging = Assembly::new("smg-charging");
    let mut ch_parts: Vec<Geo> = Vec::new();
    let ch_shaft = rod_z(0.0048, 0.0048, 0.12, 12, 0.0004);
    ch_parts.push(ch_shaft);
    let mut ch_paddle = extrude(
        &[[0.0, -0.0075], [0.017, -0.009], [0.019, 0.0], [0.017, 0.008], [0.0, 0.007]],
        0.0055,
        ExtrudeOpts {
            bevel: 0.0008,
            ..Default::default()
        },
    );
    rotate_y(&mut ch_paddle, -std::f64::consts::FRAC_PI_2);
    translate(&mut ch_paddle, -0.0075, 0.0, -0.05);
    ch_parts.push(ch_paddle);
    let mut ch_knob = dome(0.0055, 12, 0.6);
    rotate_y(&mut ch_knob, -std::f64::consts::FRAC_PI_2);
    translate(&mut ch_knob, -0.024, 0.0, -0.05);
    ch_parts.push(ch_knob);
    let ch_g = merge_all(ch_parts).expect("the smg charging handle always builds the shaft, paddle, and knob");
    charging.add(ch_g, "steel_bright", Some(Xform::default()));

    let mut bolt = Assembly::new("smg-bolt");
    let bolt_body = lathe_z(
        &[[0.0, r_rec * 0.45], [0.0, r_rec * 0.8], [0.078, r_rec * 0.8], [0.078, r_rec * 0.45]],
        16,
        0.0,
        std::f32::consts::TAU,
    );
    bolt.add(
        bolt_body,
        "steel_bright",
        Some(Xform {
            z: -0.078,
            ..Default::default()
        }),
    );
    let bface = box_geo(0.014, 0.014, 0.003, 0.0006, 1);
    bolt.add(
        bface,
        "steel",
        Some(Xform {
            z: -0.0005,
            ..Default::default()
        }),
    );
    let chamber_round = cartridge(0.0192, 0.00478, 0.0132);
    // Along the bore, not across it (see the rifle for the same trap).
    bolt.add(
        chamber_round.brass,
        "brass",
        Some(Xform {
            z: -0.0215,
            ry: std::f32::consts::PI,
            ..Default::default()
        }),
    );

    let mut trigger = Assembly::new("smg-trigger");
    let trg = trigger_part("steel_bright");
    trigger.add(trg.geo, "steel_bright", Some(Xform::default()));

    let mut selector = Assembly::new("smg-selector");
    let sel = selector_part("alu", "steel", 0.006);
    selector.add(sel.geo, "alu", Some(Xform::default()));
    let sel_r = selector_part("alu", "steel", 0.006);
    selector.add(
        sel_r.geo,
        "alu",
        Some(Xform {
            sx: -1.0,
            ..Default::default()
        }),
    );

    SmgModel {
        id: "smg",
        label: "MPX-9",
        fx_class: "smg",
        body,
        moving: SmgMoving {
            magazine,
            charging,
            bolt,
            trigger,
            selector,
        },
        nodes: SmgNodes {
            muzzle: [0.0, bore, muzzle.crown_z],
            chamber: [0.0, bore, port_z],
            eject: [r_rec + 0.006, bore + 0.002, port_z],
            eject_dir: [0.9, 0.4, 0.18],
            sight: [0.0, optic_y, optic.lens_z],
            sight_axis: [0.0, 0.0, -1.0],
            iron_sight: [0.0, rail_top + 0.024, 0.042],
            // Wrist targets, derived the same way as the rifle's (see
            // `models::rifle`): knuckle/grip contact point minus the palm
            // offset along the hand axis.
            grip_r: GripTarget {
                pos: [0.024, 0.028, 0.064],
                finger: [-0.05, -0.4, -0.915],
                back: [0.97, -0.05, -0.22],
            },
            grip_l: GripTarget {
                pos: [-0.056, 0.015, -0.153],
                finger: [0.45, 0.05, -0.89],
                back: [-0.88, -0.05, -0.45],
            },
            mag_seat: PosRot {
                pos: [0.0, bore - 0.02, mag_z],
                rot: [mag_tilt, 0.0, 0.0],
            },
            mag_drop: [0.0, -0.4, 0.02],
            charge_rest: PosRot {
                pos: [-r_rec + 0.0028, bore + r_rec - 0.007, -0.06],
                rot: [0.0, 0.0, 0.0],
            },
            charge_pull: [0.0, 0.0, 0.062],
            bolt_rest: PosRot {
                pos: [0.0, bore, port_z + 0.032],
                rot: [0.0, 0.0, 0.0],
            },
            bolt_travel: [0.0, 0.0, 0.05],
            trigger_pivot: PosRot {
                pos: [0.0, bore - 0.026, -0.001],
                rot: [0.0, 0.0, 0.0],
            },
            trigger_pull: -0.36,
            selector_pivot: PosRot {
                pos: [0.0, bore - 0.019, 0.022],
                rot: [0.0, 0.0, 0.0],
            },
            optic_glass: optic,
        },
        shell: ShellDims {
            case_len: 0.0192,
            rim_r: 0.00478,
        },
        mag_size: mag,
    }
}
