//! Ported from Claude-of-Duty `src/world/dressing.js:1165-1276` —
//! `overheadLines` (the cable spans and the laundry lines with their hanging
//! cloth) and `facadeHangings` (the rugs hung off the facades).

use axiom_math::Mat4;

use crate::jsmath;
use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{box_fine_kit, cloth_geometry, ll, ClothOpts};
use crate::world::layout::SET_PIECES;

use super::cable::{catenary_tube, CatenaryOpts};

const LAUNDRY_KEYS: [&str; 4] = ["fabric_red", "fabric_teal", "fabric_cream", "burlap"];
const RUG_KEYS: [&str; 3] = ["fabric_red", "fabric_teal", "fabric_cream"];
const SMALL_RUG_KEYS: [&str; 2] = ["fabric_red", "fabric_cream"];

/// `const SAG = 0.42;` (`dressing.js:1191`).
const SAG: f64 = 0.42;

/// `insulator(x, y, z)` (`dressing.js:1167-1171`).
fn insulator(asm: &mut Assembler, x: f64, y: f64, z: f64) {
    let geo = box_fine_kit(asm);
    let m = ll(&Mat4::IDENTITY, x as f32, y as f32, z as f32, 0.0, 0.1, 0.16, 0.1, 0.0, 0.0);
    asm.add("concrete_dark", &geo, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.5, 0.2]), paint: None }));
}

/// `overheadLines(A, rng)` (`dressing.js:1166-1233`).
pub fn overhead_lines(asm: &mut Assembler, rng: &mut Rng) {
    for [x0, y0, z0, x1, y1, z1, sag] in SET_PIECES.cables.iter().copied() {
        let t = catenary_tube([x0, y0, z0], [x1, y1, z1], sag, 0.022, CatenaryOpts { seg: 14, radial: 4, jitter: 0.05 });
        asm.add_once("metal_dark", &t, None, Some(AccumAddOpts { masks: Some([0.4, 0.7, 0.2]), paint: None }));
        // a second, thinner line running with it — never one lonely wire
        let t2 = catenary_tube([x0, y0 - 0.22, z0 + 0.18], [x1, y1 - 0.18, z1 + 0.2], sag * 1.12, 0.014, CatenaryOpts { seg: 14, radial: 4, jitter: 0.06 });
        asm.add_once("metal_dark", &t2, None, Some(AccumAddOpts { masks: Some([0.4, 0.7, 0.2]), paint: None }));
        insulator(asm, x0, y0 + 0.06, z0);
        insulator(asm, x1, y1 + 0.06, z1);
    }

    for [x0, y0, z0, x1, y1, z1] in SET_PIECES.laundry.iter().copied() {
        let line = catenary_tube([x0, y0, z0], [x1, y1, z1], SAG, 0.012, CatenaryOpts { seg: 12, radial: 4, jitter: 0.0 });
        asm.add_once("metal_dark", &line, None, Some(AccumAddOpts { masks: Some([0.3, 0.6, 0.2]), paint: None }));
        let dx = x1 - x0;
        let dz = z1 - z0;
        // `Math.hypot(dx, dz)`. NOT `(dx*dx + dz*dz).sqrt()`, and NOT
        // `f64::hypot` either — that is a different (correctly-rounded)
        // algorithm that disagrees with V8's in the last bits. See
        // `crate::jsmath::hypot`.
        let len = jsmath::hypot2(dx, dz);
        let ry = (-dz).atan2(dx);
        let n = (jsmath::round(len / 1.7) as i64).max(2) as i32;
        let k = 1.5f64.cosh() - 1.0;
        for i in 0..n {
            let t = (f64::from(i) + 0.5) / f64::from(n);
            if rng.float() < 0.12 {
                continue;
            }
            // hang from the line where the line actually is: same catenary as
            // the tube
            let droop = (1.5f64.cosh() - ((t - 0.5) * 3.0).cosh()) / k;
            let px = x0 + dx * t;
            let pz = z0 + dz * t;
            let py = y0 + (y1 - y0) * t - SAG * droop - 0.03;
            let w = rng.range(0.72, 1.15);
            let h = rng.range(0.85, 1.45);
            let c_sag = rng.range(0.18, 0.3);
            let c_wrinkle = rng.range(0.05, 0.085);
            let c_twist = rng.range(0.1, 0.2);
            let c_thickness = rng.range(0.0016, 0.003);
            let c_fray = rng.range(0.01, 0.03);
            let cloth = cloth_geometry(
                w as f32,
                h as f32,
                ClothOpts {
                    seg_x: 9,
                    seg_y: 10,
                    sag: c_sag as f32,
                    wrinkle: c_wrinkle as f32,
                    twist: c_twist as f32,
                    bulge: 0.06,
                    thickness: c_thickness as f32,
                    fray: c_fray as f32,
                    ..ClothOpts::default()
                },
                Some(rng),
            );
            let key = *rng.pick(&LAUNDRY_KEYS);
            let grime = rng.range(0.4, 0.8);
            let m = ll(&Mat4::IDENTITY, px as f32, (py - h / 2.0 + 0.02) as f32, pz as f32, ry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
            asm.add_once(key, &cloth, Some(&m), Some(AccumAddOpts { masks: Some([0.3, grime as f32, 0.2]), paint: None }));
        }
    }
}

/// `facadeHangings(A, rng)` (`dressing.js:1236-1276`).
///
/// A rug on a facade is the biggest single piece of cloth in the frame, so it
/// is also the one that most obviously reads as a sheet of glass if it has no
/// thickness, no hem and no slack. Heavy gauge, deep folds, frayed bottom.
pub fn facade_hangings(asm: &mut Assembler, rng: &mut Rng) {
    for [x, y, z, ry, w, h] in SET_PIECES.hangings.iter().copied() {
        let c_sag = rng.range(0.09, 0.15);
        let c_wrinkle = rng.range(0.04, 0.07);
        let c_bulge = rng.range(0.05, 0.11);
        let c_twist = rng.range(0.03, 0.1);
        let c_thickness = rng.range(0.0026, 0.004);
        let c_fray = rng.range(0.015, 0.035);
        let cloth = cloth_geometry(
            w as f32,
            h as f32,
            ClothOpts {
                seg_x: 10,
                seg_y: 10,
                sag: c_sag as f32,
                wrinkle: c_wrinkle as f32,
                bulge: c_bulge as f32,
                twist: c_twist as f32,
                thickness: c_thickness as f32,
                fray: c_fray as f32,
                // belly out into the street, not through the facade
                bow: -1.0,
                ..ClothOpts::default()
            },
            Some(rng),
        );
        let key = *rng.pick(&RUG_KEYS);
        let grime = rng.range(0.42, 0.72);
        let m = ll(&Mat4::IDENTITY, x as f32, y as f32, z as f32, ry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once(key, &cloth, Some(&m), Some(AccumAddOpts { masks: Some([0.35, grime as f32, 0.2]), paint: None }));
        // the rail it hangs from
        let rail = box_fine_kit(asm);
        let rm = ll(&Mat4::IDENTITY, x as f32, (y + h / 2.0 + 0.06) as f32, z as f32, ry as f32, (w + 0.2) as f32, 0.035, 0.035, 0.0, 0.0);
        asm.add("metal_rust", &rail, Some(&rm), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.1]), paint: None }));
        // a second, smaller rug beside it, half-rolled
        if rng.float() < 0.6 {
            let c2 = cloth_geometry(
                (w * 0.55) as f32,
                (h * 0.7) as f32,
                ClothOpts { seg_x: 7, seg_y: 8, sag: 0.12, wrinkle: 0.06, thickness: 0.0032, fray: 0.025, bow: -1.0, ..ClothOpts::default() },
                Some(rng),
            );
            let key2 = *rng.pick(&SMALL_RUG_KEYS);
            let m2 = ll(
                &Mat4::IDENTITY,
                (x - ry.sin() * (w * 0.75)) as f32,
                (y - 0.25) as f32,
                (z - ry.cos() * (w * 0.75)) as f32,
                ry as f32,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
            );
            asm.add_once(key2, &c2, Some(&m2), Some(AccumAddOpts { masks: Some([0.4, 0.6, 0.25]), paint: None }));
        }
    }
}
