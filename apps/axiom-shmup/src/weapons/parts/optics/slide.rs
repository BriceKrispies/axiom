//! Ported from Claude-of-Duty `src/weapons/parts.js:1971-2072` —
//! `buildSlide`, a pistol slide: a machined block with front and rear
//! grasping serrations, a lightening cut, the ejection port, a chamber hood,
//! sight dovetails and a breech face. Built in slide space with the origin
//! at the bore axis, so the rig can cycle it straight back along `+Z`.
//!
//! Weapon-local space is `+X` right, `+Y` up, `-Z` toward the muzzle, origin
//! at the shooting hand's anchor — the convention `geometry.js:28-30`
//! documents and the `geometry` module (`03-weapon-geometry-api.md`) carries
//! forward.
//!
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`for`
//! throughout. Rust has no default arguments, so every JS `?? value` default
//! is documented on [`SlideOpts`] and callers pass it explicitly.

use std::f32::consts::{FRAC_PI_2, PI};

use crate::weapons::geometry::primitives::{box_geo, dome, extrude, round_rect, ExtrudeOpts};
use crate::weapons::geometry::{Assembly, Xform};

/// `o` on `buildSlide(asm, o)` (`parts.js:1971-1976`).
#[derive(Clone, Copy, Debug)]
pub struct SlideOpts {
    pub w: f32,
    pub h: f32,
    pub len: f32,
    pub mat: &'static str,
    pub z_rear: f32,
}

impl Default for SlideOpts {
    fn default() -> Self {
        SlideOpts { w: 0.0262, h: 0.0248, len: 0.183, mat: "steel", z_rear: 0.052 }
    }
}

/// `buildSlide`'s return (`parts.js:2071`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideResult {
    pub z_rear: f32,
    pub z_front: f32,
    pub w: f32,
    pub h: f32,
    pub len: f32,
    pub sight_y: f32,
}

/// Pistol slide: chamfered main body with a top rib, a bevelled nose taper,
/// front/rear grasping serrations, lightening cuts on both flanks, an
/// ejection port with a real cavity and lip, a breech face, and front/rear
/// sights with witness dots (`buildSlide`, `parts.js:1971-2072`).
pub fn build_slide(asm: &mut Assembly, o: SlideOpts) -> SlideResult {
    let SlideOpts { w, h, len, mat, z_rear } = o;
    let z_front = z_rear - len;
    let cz = (z_rear + z_front) / 2.0;
    // `const bore = 0;` in the source (`parts.js:1978`) — a named zero
    // offset kept for call-order/read fidelity rather than inlined as a
    // bare `0.0` at every use site.
    let bore: f32 = 0.0;

    // Main body: chamfered block with a top rib.
    let body_g = box_geo(w, h, len, 0.0016, 2);
    asm.add(body_g, mat, Some(Xform { y: bore + 0.0015, z: cz, ..Default::default() }));
    let rib = box_geo(w - 0.008, 0.004, len - 0.02, 0.0012, 2);
    asm.add(rib, mat, Some(Xform { y: bore + h * 0.5 + 0.0025, z: cz - 0.004, ..Default::default() }));

    // front taper / nose bevel
    let nose = extrude(
        &[
            [-f64::from(w) * 0.5, -f64::from(h) * 0.5],
            [f64::from(w) * 0.5, -f64::from(h) * 0.5],
            [f64::from(w) * 0.5, f64::from(h) * 0.34],
            [f64::from(w) * 0.36, f64::from(h) * 0.5],
            [-f64::from(w) * 0.36, f64::from(h) * 0.5],
            [-f64::from(w) * 0.5, f64::from(h) * 0.34],
        ],
        0.016,
        ExtrudeOpts { bevel: 0.0012, ..Default::default() },
    );
    asm.add(nose, mat, Some(Xform { y: bore + 0.0015, z: z_front + 0.008, ..Default::default() }));

    // Grasping serrations, front and rear.
    [(z_rear - 0.006, 7u32), (z_front + 0.03, 5u32)].into_iter().for_each(|(z0, count)| {
        (0..count).for_each(|i| {
            let z = z0 - i as f32 * 0.0052;
            let g = box_geo(w + 0.0006, h * 0.62, 0.0026, 0.0006, 1);
            asm.add(g, mat, Some(Xform { y: bore + 0.0015, z, ..Default::default() }));
        });
    });

    // Lightening cuts on the flanks.
    [-1.0f32, 1.0].into_iter().for_each(|sx| {
        let cut = extrude(
            &round_rect(0.042, f64::from(h) * 0.4, 0.004, 3),
            0.0016,
            ExtrudeOpts { bevel: 0.0005, ..Default::default() },
        );
        asm.add(
            cut,
            mat,
            Some(Xform { x: sx * (w * 0.5 - 0.0004), y: bore + 0.001, z: cz - 0.012, ry: FRAC_PI_2, ..Default::default() }),
        );
    });

    // Ejection port with a real cavity and a chamber hood.
    let port_w: f32 = 0.036;
    let port_h: f32 = 0.0135;
    let cav = box_geo(0.01, port_h, port_w, 0.0008, 1);
    asm.add(
        cav,
        "cavity",
        Some(Xform { x: w * 0.5 - 0.006, y: bore + 0.004, z: z_rear - 0.05, ry: FRAC_PI_2, ..Default::default() }),
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
    asm.add(
        lip,
        mat,
        Some(Xform { x: w * 0.5 - 0.0009, y: bore + 0.004, z: z_rear - 0.05, ry: FRAC_PI_2, ..Default::default() }),
    );

    // Breech face + extractor.
    let breech = box_geo(w - 0.006, h - 0.008, 0.004, 0.0008, 1);
    asm.add(breech, "steel_bright", Some(Xform { y: bore + 0.001, z: z_rear - 0.032, ..Default::default() }));

    // Sights: front post with a dot, rear notch with two.
    let rear = extrude(
        &[
            [-0.009, 0.0],
            [0.009, 0.0],
            [0.009, 0.0055],
            [0.0022, 0.0055],
            [0.0022, 0.0022],
            [-0.0022, 0.0022],
            [-0.0022, 0.0055],
            [-0.009, 0.0055],
        ],
        0.0055,
        ExtrudeOpts { bevel: 0.0004, ..Default::default() },
    );
    asm.add(rear, "steel_bright", Some(Xform { y: bore + h * 0.5 + 0.0045, z: z_rear - 0.012, ..Default::default() }));
    [-1.0f32, 1.0].into_iter().for_each(|sx| {
        let dot = dome(0.0011, 8, 0.5);
        asm.add(
            dot,
            "steel_bright",
            Some(Xform { x: sx * 0.0055, y: bore + h * 0.5 + 0.0075, z: z_rear - 0.0148, ry: PI, ..Default::default() }),
        );
    });
    let front = box_geo(0.0035, 0.0062, 0.0042, 0.0004, 1);
    asm.add(front, "steel_bright", Some(Xform { y: bore + h * 0.5 + 0.0055, z: z_front + 0.014, ..Default::default() }));
    let fdot = dome(0.0013, 8, 0.5);
    asm.add(fdot, "steel_bright", Some(Xform { y: bore + h * 0.5 + 0.0058, z: z_front + 0.0118, ry: PI, ..Default::default() }));

    SlideResult { z_rear, z_front, w, h, len, sight_y: bore + h * 0.5 + 0.0065 }
}
