//! Ported from Claude-of-Duty `src/weapons/models/pistol.js` (~230 lines) —
//! `buildPistol()`, a striker-fired polymer-framed 9 mm sidearm.
//!
//! A pistol is where proportion errors are most obvious, so the numbers are
//! the real ones: 183 mm slide, 26 mm across, bore 36 mm over the web of the
//! hand, 18-degree grip rake, 22 mm of slide travel (`pistol.js:14-20`).
//!
//! This is app code (`apps/`), outside the Branchless Law and the Coverage
//! Law — plain `if`/`for` throughout, matching the source's own control flow.

use std::f32::consts::{FRAC_PI_2, PI};

use axiom_math::{Mat4, Vec3};

use crate::weapons::geometry::primitives::{blob, box_geo, extrude, lathe_z, tube_z, ExtrudeOpts};
use crate::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use crate::weapons::parts::controls::{add_pistol_grip, trigger_part, PistolGripOpts};
use crate::weapons::parts::hardware::{add_rail, RailOpts};
use crate::weapons::parts::magazine::{build_magazine, MagazineDims, MagazineOpts};
use crate::weapons::parts::optics::{build_mini_reflex, build_slide, MiniReflexOpts, MiniReflexResult, SlideOpts, SlideResult};

use super::{GripTarget, PosRot, ShellDims};

/// `BufferGeometry.translate(x, y, z)`, via the normal-matrix-correct
/// [`Geo::apply`] — see `geometry/primitives/xform.rs`'s doc for why this
/// reuses `apply` rather than hand-rolling a second transform path.
fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `moving: { magazine, trigger, slide: slideAsm }` (`pistol.js:257`).
pub struct PistolMoving {
    pub magazine: Assembly,
    pub trigger: Assembly,
    pub slide: Assembly,
}

/// `nodes` (`pistol.js:258-286`). Unlike the rifle/smg, there is no
/// `chargeRest`/`boltRest`/`selectorPivot` — a striker-fired pistol has no
/// charging handle or selector, and the reciprocating part is the slide
/// itself (`slideRest`/`slideTravel`/`slideGeom`).
pub struct PistolNodes {
    pub muzzle: [f32; 3],
    pub chamber: [f32; 3],
    pub eject: [f32; 3],
    pub eject_dir: [f32; 3],
    pub sight: [f32; 3],
    pub sight_axis: [f32; 3],
    pub iron_sight: [f32; 3],
    pub grip_r: GripTarget,
    /// Support hand cups the firing hand rather than the frame
    /// (`pistol.js:272-273`).
    pub grip_l: GripTarget,
    pub mag_seat: PosRot,
    pub mag_drop: [f32; 3],
    pub slide_rest: PosRot,
    pub slide_travel: [f32; 3],
    pub trigger_pivot: PosRot,
    pub trigger_pull: f32,
    pub optic_glass: MiniReflexResult,
    pub slide_geom: SlideResult,
}

/// `buildPistol()`'s full return value (`pistol.js:252-289`).
pub struct PistolModel {
    pub id: &'static str,
    pub label: &'static str,
    pub fx_class: &'static str,
    pub body: Assembly,
    pub moving: PistolMoving,
    pub nodes: PistolNodes,
    pub shell: ShellDims,
    pub mag_size: MagazineDims,
}

/// The sidearm — a striker-fired polymer-framed 9 mm, slide-mounted mini
/// reflex (`buildPistol`, `pistol.js:21-290`).
pub fn build_pistol() -> PistolModel {
    let bore: f32 = 0.036;
    let slide_h: f32 = 0.0248;
    let slide_w: f32 = 0.0262;
    let slide_len: f32 = 0.183;
    let z_slide_rear: f32 = 0.052;
    let z_slide_front: f32 = z_slide_rear - slide_len;
    let grip_angle: f32 = 0.32;

    let mut body = Assembly::new("pistol-frame");

    // ---- frame -------------------------------------------------------------
    // Dust cover / frame rails under the slide.
    let dust = extrude(
        &[
            [f64::from(-slide_w) * 0.5 + 0.001, 0.0],
            [f64::from(slide_w) * 0.5 - 0.001, 0.0],
            [f64::from(slide_w) * 0.5 - 0.001, -0.0125],
            [f64::from(slide_w) * 0.5 - 0.004, -0.016],
            [f64::from(-slide_w) * 0.5 + 0.004, -0.016],
            [f64::from(-slide_w) * 0.5 + 0.001, -0.0125],
        ],
        0.108,
        ExtrudeOpts {
            bevel: 0.001,
            ..Default::default()
        },
    );
    body.add(
        dust,
        "polymer",
        Some(Xform {
            y: bore - 0.0075,
            z: -0.062,
            ..Default::default()
        }),
    );

    // Frame body around the trigger and the magwell.
    let frame_core = blob(slide_w - 0.001, 0.05, 0.062, 0.004, 3);
    body.add(
        frame_core,
        "polymer",
        Some(Xform {
            y: bore - 0.032,
            z: 0.012,
            ..Default::default()
        }),
    );

    // Beavertail / tang.
    let tang = extrude(
        &[[-0.008, 0.0], [0.03, -0.004], [0.032, -0.012], [-0.008, -0.014]],
        slide_w - 0.003,
        ExtrudeOpts {
            bevel: 0.0012,
            ..Default::default()
        },
    );
    body.add(
        tang,
        "polymer",
        Some(Xform {
            y: bore - 0.014,
            z: 0.034,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );

    // Accessory rail under the dust cover.
    add_rail(
        &mut body,
        "polymer",
        -0.112,
        -0.058,
        bore - 0.0175,
        0.0,
        RailOpts {
            width: 0.0175,
            waist: 0.013,
            base_h: 0.0026,
            top_h: 0.0024,
            pitch: 0.0092,
            slot: 0.0046,
            ..RailOpts::default()
        },
    );

    // Trigger guard: undercut, with a slight index ledge.
    let guard_outer: [[f64; 2]; 7] = [
        [-0.024, 0.0],
        [0.026, 0.0],
        [0.028, -0.007],
        [0.024, -0.022],
        [0.013, -0.027],
        [-0.016, -0.027],
        [-0.024, -0.021],
    ];
    let guard_inner: [[f64; 2]; 7] = [
        [-0.019, -0.003],
        [0.021, -0.003],
        [0.0225, -0.009],
        [0.0185, -0.0205],
        [0.01, -0.0235],
        [-0.013, -0.0235],
        [-0.019, -0.0185],
    ];
    let guard = extrude(
        &guard_outer,
        slide_w - 0.004,
        ExtrudeOpts {
            bevel: 0.001,
            holes: vec![guard_inner.to_vec()],
            ..Default::default()
        },
    );
    body.add(
        guard,
        "polymer",
        Some(Xform {
            y: bore - 0.0245,
            z: -0.03,
            ..Default::default()
        }),
    );

    // ---- grip -----------------------------------------------------------------
    add_pistol_grip(
        &mut body,
        "polymer",
        "rubber",
        PistolGripOpts {
            y: bore - 0.014,
            z: 0.016,
            angle: grip_angle,
            len: 0.113,
            w: 0.0305,
        },
    );
    // Stippling: a field of tiny raised pyramids on both side panels.
    let mut stipple: Vec<Geo> = Vec::new();
    for r in 0..9u32 {
        for c_idx in 0..5u32 {
            let mut g = box_geo(0.0024, 0.0024, 0.0009, 0.0003, 1);
            translate(&mut g, -0.005 + c_idx as f32 * 0.0026 + (r % 2) as f32 * 0.0013, -0.012 - r as f32 * 0.0072, 0.0);
            stipple.push(g);
        }
    }
    let stipple_g = merge_all(stipple).expect("the stippling loop always builds 9 * 5 pyramids");
    for sx in [-1.0f32, 1.0f32] {
        body.add(
            stipple_g.clone(),
            "polymer",
            Some(Xform {
                x: sx * 0.0152,
                y: bore - 0.016,
                z: 0.017,
                ry: sx * PI * 0.5,
                rx: 0.0,
                rz: if sx > 0.0 { -grip_angle } else { grip_angle },
                ..Default::default()
            }),
        );
    }

    // Magazine release, slide stop lever, takedown lever.
    let rel_button = lathe_z(
        &[[0.0, 0.0], [0.0, 0.0042], [0.0015, 0.0048], [0.0038, 0.0048], [0.0038, 0.0]],
        12,
        0.0,
        std::f32::consts::TAU,
    );
    body.add(
        rel_button,
        "polymer",
        Some(Xform {
            x: 0.0138,
            y: bore - 0.032,
            z: -0.014,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    let stop_lever = extrude(
        &[[-0.014, -0.0028], [0.012, -0.0035], [0.014, 0.0028], [-0.014, 0.0035]],
        0.0032,
        ExtrudeOpts {
            bevel: 0.0005,
            ..Default::default()
        },
    );
    body.add(
        stop_lever.clone(),
        "steel",
        Some(Xform {
            x: -0.0132,
            y: bore - 0.0135,
            z: -0.022,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    body.add(
        stop_lever,
        "steel",
        Some(Xform {
            x: 0.0132,
            y: bore - 0.0135,
            z: -0.022,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    let takedown = lathe_z(&[[0.0, 0.0], [0.0, 0.0035], [0.0022, 0.004], [0.0022, 0.0]], 12, 0.0, std::f32::consts::TAU);
    body.add(
        takedown,
        "steel",
        Some(Xform {
            x: -0.0138,
            y: bore - 0.0175,
            z: -0.046,
            ry: -FRAC_PI_2,
            ..Default::default()
        }),
    );

    // ---- barrel, exposed at the muzzle, plus the recoil spring ----------------
    let barrel = lathe_z(
        &[[0.0, 0.0], [0.0, 0.0082], [0.0016, 0.0088], [0.006, 0.0088], [0.0072, 0.0078], [0.0072, 0.0048]],
        18,
        0.0,
        std::f32::consts::TAU,
    );
    body.add(
        barrel,
        "steel_bright",
        Some(Xform {
            y: bore,
            z: z_slide_front + 0.0012,
            ry: PI,
            ..Default::default()
        }),
    );
    let bore_hole = tube_z(0.0048, 0.0034, 0.03, 12, 0.0002);
    body.add(
        bore_hole,
        "cavity",
        Some(Xform {
            y: bore,
            z: z_slide_front + 0.012,
            ..Default::default()
        }),
    );
    let spring = lathe_z(&[[0.0, 0.0032], [0.0, 0.0048], [0.004, 0.0048], [0.004, 0.0032]], 12, 0.0, std::f32::consts::TAU);
    body.add(
        spring,
        "steel_bright",
        Some(Xform {
            y: bore - 0.0125,
            z: z_slide_front + 0.0025,
            ..Default::default()
        }),
    );

    // ---- moving parts -----------------------------------------------------------
    let mut slide_asm = Assembly::new("pistol-slide");
    let slide = build_slide(
        &mut slide_asm,
        SlideOpts {
            w: slide_w,
            h: slide_h,
            len: slide_len,
            // Nitrided, not bare steel: a slide is one big flat facing the sky.
            mat: "steel_black",
            z_rear: z_slide_rear,
        },
    );
    // Slide-mounted mini reflex, in a milled pocket behind the rear sight.
    let reflex = build_mini_reflex(
        &mut slide_asm,
        MiniReflexOpts {
            w: 0.0246,
            h: 0.021,
            len: 0.0455,
            y: slide_h * 0.5 + 0.0018,
            z: z_slide_rear - 0.038,
            mat_body: "alu_fine",
            tilt: 0.16,
        },
    );
    let optic_y = bore + slide_h * 0.5 + 0.0018 + 0.021 * 0.56;
    let optic_z = z_slide_rear - 0.038 + 0.0455 * 0.14;

    let mut magazine = Assembly::new("pistol-mag");
    let mag = build_magazine(
        &mut magazine,
        (),
        MagazineOpts {
            w: 0.0212,
            d: 0.0295,
            len: 0.108,
            curve: 0.004,
            segs: 5,
            witness: 3,
            case_len: 0.0192,
            rim_r: 0.00478,
            bullet_len: 0.0132,
            poly: "polymer",
        },
    );

    let mut trigger = Assembly::new("pistol-trigger");
    let trg = trigger_part("polymer");
    trigger.add(trg.geo, "polymer", Some(Xform::default()));
    // The trigger safety blade down the middle of the face.
    let blade = extrude(
        &[[-0.0022, 0.003], [0.0022, 0.003], [0.0022, -0.016], [-0.0022, -0.017]],
        0.0028,
        ExtrudeOpts {
            bevel: 0.0004,
            ..Default::default()
        },
    );
    trigger.add(
        blade,
        "steel",
        Some(Xform {
            x: 0.0,
            y: -0.001,
            z: 0.0022,
            ..Default::default()
        }),
    );

    PistolModel {
        id: "pistol",
        label: "P-19",
        fx_class: "pistol",
        body,
        moving: PistolMoving {
            magazine,
            trigger,
            slide: slide_asm,
        },
        nodes: PistolNodes {
            muzzle: [0.0, bore, z_slide_front - 0.004],
            chamber: [0.0, bore, z_slide_rear - 0.05],
            eject: [slide_w * 0.5 + 0.004, bore + 0.005, z_slide_rear - 0.05],
            eject_dir: [0.82, 0.52, 0.24],
            sight: [0.0, optic_y, optic_z],
            sight_axis: [0.0, 0.0, -1.0],
            iron_sight: [0.0, bore + slide_h * 0.5 + 0.0065, z_slide_rear - 0.012],
            // Wrist targets (see `models::rifle` for the derivation).
            grip_r: GripTarget {
                pos: [0.028, 0.003, 0.07],
                finger: [0.0, -0.315, -0.949],
                back: [0.98, 0.0, -0.2],
            },
            grip_l: GripTarget {
                pos: [-0.03, -0.012, 0.076],
                finger: [0.34, -0.28, -0.9],
                back: [0.15, 0.93, -0.33],
            },
            mag_seat: PosRot {
                pos: [0.0, bore - 0.03, 0.019],
                rot: [-grip_angle, 0.0, 0.0],
            },
            mag_drop: [0.0, -0.42, 0.05],
            slide_rest: PosRot {
                pos: [0.0, bore, 0.0],
                rot: [0.0, 0.0, 0.0],
            },
            slide_travel: [0.0, 0.0, 0.0225],
            trigger_pivot: PosRot {
                pos: [0.0, bore - 0.0135, -0.0165],
                rot: [0.0, 0.0, 0.0],
            },
            trigger_pull: -0.3,
            optic_glass: reflex,
            slide_geom: slide,
        },
        shell: ShellDims {
            case_len: 0.0192,
            rim_r: 0.00478,
        },
        mag_size: mag,
    }
}
