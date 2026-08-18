//! Ported from Claude-of-Duty `src/weapons/parts.js:1886-1971` —
//! `buildMiniReflex`, a micro/mini reflex sight (base plate, tapered side
//! walls, hood + emitter housing, canted glass pane in a bevelled frame).
//!
//! Weapon-local space is `+X` right, `+Y` up, `-Z` toward the muzzle, origin
//! at the shooting hand's anchor — the convention `geometry.js:28-30`
//! documents and the `geometry` module (`03-weapon-geometry-api.md`) carries
//! forward.
//!
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`for`
//! throughout. Rust has no default arguments, so every JS `?? value` default
//! is documented on [`MiniReflexOpts`] and callers pass it explicitly.

use std::f32::consts::{FRAC_PI_2, TAU};

use crate::weapons::geometry::primitives::{blob, box_geo, extrude, lathe_z, round_rect, ExtrudeOpts};
use crate::weapons::geometry::{Assembly, Xform};
use crate::weapons::parts::hardware::{add_screw, MountAxis};

/// `o` on `buildMiniReflex(asm, o)` (`parts.js:1886-1892`). `tilt` is
/// `o.tilt ?? 0.16` — the rear-canted window, like the real thing.
#[derive(Clone, Copy, Debug)]
pub struct MiniReflexOpts {
    pub w: f32,
    pub h: f32,
    pub len: f32,
    pub y: f32,
    pub z: f32,
    pub mat_body: &'static str,
    pub tilt: f32,
}

impl Default for MiniReflexOpts {
    fn default() -> Self {
        MiniReflexOpts {
            w: 0.0246,
            h: 0.021,
            len: 0.0455,
            y: 0.0,
            z: 0.0,
            mat_body: "alu",
            tilt: 0.16,
        }
    }
}

/// `buildMiniReflex`'s return (`parts.js:1963-1970`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniReflexResult {
    pub center: [f32; 3],
    pub lens_z: f32,
    pub aperture_r: f32,
    pub window_w: f32,
    pub window_h: f32,
    pub tilt: f32,
}

/// Mini reflex / micro red dot: base plate, two tapered side walls joined by
/// a hood, an emitter housing with an LED, battery/adjustment screws, and a
/// canted glass pane in a bevelled frame (`buildMiniReflex`,
/// `parts.js:1886-1971`).
pub fn build_mini_reflex(asm: &mut Assembly, o: MiniReflexOpts) -> MiniReflexResult {
    let MiniReflexOpts {
        w,
        h,
        len,
        y,
        z,
        mat_body,
        tilt: glass_tilt,
    } = o;

    // Base plate.
    let base = extrude(
        &round_rect(f64::from(w), f64::from(len), 0.003, 3),
        0.0042,
        ExtrudeOpts { bevel: 0.0007, ..Default::default() },
    );
    asm.add(base, mat_body, Some(Xform { y: y + 0.002, z, rx: FRAC_PI_2, ..Default::default() }));

    // Two side walls that taper toward the front, joined by the hood.
    let wall = extrude(
        &[
            [-f64::from(len) * 0.5, 0.0],
            [f64::from(len) * 0.42, 0.0],
            [f64::from(len) * 0.46, f64::from(h) * 0.52],
            [f64::from(len) * 0.3, f64::from(h) * 0.86],
            [-f64::from(len) * 0.42, f64::from(h)],
            [-f64::from(len) * 0.5, f64::from(h) * 0.92],
        ],
        0.0036,
        ExtrudeOpts { bevel: 0.0007, ..Default::default() },
    );
    [-1.0f32, 1.0].into_iter().for_each(|sx| {
        asm.add(
            wall.clone(),
            mat_body,
            Some(Xform { x: sx * (w * 0.5 - 0.0018), y: y + 0.004, z, ry: FRAC_PI_2, ..Default::default() }),
        );
    });

    // Hood over the front, and the emitter housing at the front floor.
    let hood = box_geo(w, 0.0035, 0.011, 0.0008, 1);
    asm.add(hood, mat_body, Some(Xform { y: y + h * 0.98, z: z - len * 0.36, ..Default::default() }));
    let emitter = blob(w - 0.007, 0.0075, 0.012, 0.0016, 2);
    asm.add(emitter, mat_body, Some(Xform { y: y + 0.0075, z: z - len * 0.3, ..Default::default() }));
    let led = lathe_z(&[[0.0, 0.0], [0.0, 0.0016], [0.0012, 0.0018], [0.0012, 0.0]], 10, 0.0, TAU);
    asm.add(led, "steel_bright", Some(Xform { y: y + 0.0105, z: z - len * 0.28, rx: -0.5, ..Default::default() }));

    // Battery tray + adjustment screws.
    add_screw(asm, "steel", 0.0, y + 0.004, z + len * 0.4, 0.0026, MountAxis::Y, 0.008);
    add_screw(asm, "steel", w * 0.5 - 0.002, y + h * 0.5, z + len * 0.28, 0.0022, MountAxis::X, 0.006);
    add_screw(asm, "steel", 0.0, y + h * 0.86, z + len * 0.1, 0.0022, MountAxis::Y, 0.006);

    // The window: a real pane, canted back, in a bevelled frame.
    let glass_w = w - 0.007;
    let glass_h = h * 0.72;
    let pane = extrude(
        &round_rect(f64::from(glass_w), f64::from(glass_h), 0.0015, 3),
        0.0012,
        ExtrudeOpts { bevel: 0.0003, ..Default::default() },
    );
    asm.add(pane, "glass", Some(Xform { y: y + h * 0.56, z: z + len * 0.14, rx: glass_tilt, ..Default::default() }));
    let frame = extrude(
        &round_rect(f64::from(glass_w) + 0.0028, f64::from(glass_h) + 0.0028, 0.0018, 3),
        0.0022,
        ExtrudeOpts {
            bevel: 0.0005,
            holes: vec![round_rect(f64::from(glass_w) - 0.0002, f64::from(glass_h) - 0.0002, 0.0014, 3)],
            ..Default::default()
        },
    );
    asm.add(frame, mat_body, Some(Xform { y: y + h * 0.56, z: z + len * 0.14, rx: glass_tilt, ..Default::default() }));

    MiniReflexResult {
        center: [0.0, y + h * 0.56, z + len * 0.14],
        lens_z: z + len * 0.14,
        aperture_r: glass_w.min(glass_h) * 0.46,
        window_w: glass_w * 0.46,
        window_h: glass_h * 0.46,
        tilt: glass_tilt,
    }
}
