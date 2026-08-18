//! Ported from Claude-of-Duty `src/weapons/parts.js:36-168` — small hardware
//! (pins, screws, QD sockets, sling loops, cartridges) and the Picatinny
//! rail mount.
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
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`match`
//! throughout, matching the JS ternary chains it replaces. Rust has no
//! default arguments, so every JS `= value` default is documented on the
//! function and callers pass it explicitly (same convention as the
//! `geometry::primitives` port).

use std::f32::consts::{FRAC_PI_2, TAU};

use crate::weapons::geometry::primitives::{box_geo, dome, lathe_z, picatinny, ring, rod_z, screw, PicatinnyOpts};
use crate::weapons::geometry::{Assembly, Geo, Xform};

/// Overall length of each muzzle device, so callers can lay out the barrel.
/// `MUZZLE_LEN` (`parts.js:36`).
pub struct MuzzleLen {
    pub brake: f32,
    pub a2: f32,
    pub comp: f32,
    pub trilug: f32,
}

pub const MUZZLE_LEN: MuzzleLen = MuzzleLen {
    brake: 0.062,
    a2: 0.0483,
    comp: 0.058,
    trilug: 0.042,
};

/* -------------------------------------------------------------------------- */
/*  small hardware                                                            */
/* -------------------------------------------------------------------------- */

/// Cross pin with a domed head (takedown pins, trigger/hammer pins). `r`
/// default `0.0022`, `len` default `0.02` (`addPin`, `parts.js:43-47`).
pub fn add_pin(asm: &mut Assembly, mat: &str, x: f32, y: f32, z: f32, r: f32, len: f32) {
    asm.add(
        rod_z(r, r, len, 12, 0.0004),
        mat,
        Some(Xform {
            x,
            y,
            z,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
    asm.add(
        dome(r * 1.25, 10, 0.5),
        mat,
        Some(Xform {
            x: x + len / 2.0,
            y,
            z,
            ry: -FRAC_PI_2,
            ..Default::default()
        }),
    );
    asm.add(
        dome(r * 1.25, 10, 0.5),
        mat,
        Some(Xform {
            x: x - len / 2.0,
            y,
            z,
            ry: FRAC_PI_2,
            ..Default::default()
        }),
    );
}

/// Axis a screw or QD socket mounts along — `axis = 'x' | 'y' | 'z'` in the
/// source (a bare string there; any value other than `'x'`/`'y'` falls
/// through to the identity-rotation branch, which in practice is always the
/// `'z'` call sites use). [`add_screw`] and [`add_qd_socket`] map the *same*
/// three variants to *different* rotations — verified against `parts.js:52`
/// and `parts.js:77` respectively; do not assume they share a mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountAxis {
    X,
    Y,
    Z,
}

/// Hex-socket screw, head facing `+axis`. `rHead` default `0.0022`, `axis`
/// default `'y'`, `len` default `0.008` (`addScrew`, `parts.js:50-55`).
pub fn add_screw(asm: &mut Assembly, mat: &str, x: f32, y: f32, z: f32, r_head: f32, axis: MountAxis, len: f32) {
    let g = screw(r_head, r_head * 0.55, r_head * 0.5, len, 10);
    let rot = match axis {
        MountAxis::Y => Xform {
            rx: FRAC_PI_2,
            ..Default::default()
        },
        MountAxis::X => Xform {
            ry: -FRAC_PI_2,
            ..Default::default()
        },
        MountAxis::Z => Xform::default(),
    };
    asm.add(g, mat, Some(Xform { x, y, z, ..rot }));
}

/// QD sling swivel socket: a countersunk cup with a steel insert. `axis`
/// default `'x'`, `r` default `0.0055` (`addQdSocket`, `parts.js:58-82`).
pub fn add_qd_socket(
    asm: &mut Assembly,
    mat_body: &str,
    mat_steel: &str,
    x: f32,
    y: f32,
    z: f32,
    axis: MountAxis,
    r: f32,
) {
    let cup = lathe_z(
        &[
            [0.0, r * 0.55],
            [0.0, r * 1.5],
            [0.0012, r * 1.62],
            [0.006, r * 1.62],
            [0.006, r * 0.9],
        ],
        14,
        0.0,
        TAU,
    );
    let inner = lathe_z(&[[0.004, 0.0], [0.004, r * 0.55], [0.0, r * 0.55]], 12, 0.0, TAU);
    let rot = match axis {
        MountAxis::X => Xform {
            ry: FRAC_PI_2,
            ..Default::default()
        },
        MountAxis::Y => Xform {
            rx: -FRAC_PI_2,
            ..Default::default()
        },
        MountAxis::Z => Xform::default(),
    };
    asm.add(cup, mat_body, Some(Xform { x, y, z, ..rot }));
    asm.add(inner, mat_steel, Some(Xform { x, y, z, ..rot }));
}

/// Fixed sling loop: a flat steel eye. `radius` default `0.008`, `rot`
/// default identity (`addSlingLoop`, `parts.js:85-89`).
pub fn add_sling_loop(asm: &mut Assembly, mat: &str, x: f32, y: f32, z: f32, radius: f32, rot: Xform) {
    let g = ring(radius, 0.0016, 14, 6, TAU);
    asm.add(g, mat, Some(Xform { x, y, z, ..rot }));
}

/// A live cartridge: brass case, shoulder, neck, copper FMJ tip. `caseLen`
/// default `0.0446`, `rimR` default `0.00495`, `bulletLen` default `0.019`
/// (`cartridge`, `parts.js:92-116`).
pub struct Cartridge {
    pub brass: Geo,
    pub bullet: Geo,
    pub length: f32,
}

pub fn cartridge(case_len: f32, rim_r: f32, bullet_len: f32) -> Cartridge {
    let neck_r = rim_r * 0.72;
    let brass = lathe_z(
        &[
            [0.0, 0.0],
            [0.0, rim_r],
            [0.0012, rim_r * 0.97],
            [case_len * 0.62, rim_r * 0.965],
            [case_len * 0.78, neck_r],
            [case_len, neck_r],
        ],
        16,
        0.0,
        TAU,
    );
    let bullet = lathe_z(
        &[
            [case_len - 0.004, neck_r * 0.98],
            [case_len + bullet_len * 0.45, neck_r * 0.98],
            [case_len + bullet_len * 0.8, neck_r * 0.62],
            [case_len + bullet_len, neck_r * 0.16],
            [case_len + bullet_len + 0.0004, 0.0],
        ],
        16,
        0.0,
        TAU,
    );
    Cartridge {
        brass,
        bullet,
        length: case_len + bullet_len,
    }
}

/// Fired case: same brass, no bullet, slightly belled mouth. `caseLen`
/// default `0.0446`, `rimR` default `0.00495` (`emptyCase`, `parts.js:119-134`).
pub fn empty_case(case_len: f32, rim_r: f32) -> Geo {
    let neck_r = rim_r * 0.72;
    lathe_z(
        &[
            [0.0, 0.0],
            [0.0, rim_r],
            [0.0012, rim_r * 0.97],
            [case_len * 0.62, rim_r * 0.965],
            [case_len * 0.78, neck_r],
            [case_len, neck_r * 1.02],
            [case_len, neck_r * 0.86],
            [case_len * 0.8, neck_r * 0.86],
        ],
        16,
        0.0,
        TAU,
    )
}

/* -------------------------------------------------------------------------- */
/*  rails                                                                     */
/* -------------------------------------------------------------------------- */

/// `opts` on `addRail(asm, mat, z0, z1, y, x = 0, opts = {})`
/// (`parts.js:141-168`). `base_h`/`top_h`/`waist` are read directly by
/// [`add_rail`]; every field passes through verbatim to
/// [`picatinny`][crate::weapons::geometry::primitives::picatinny]. Defaults
/// mirror `PicatinnyOpts::default()` plus `slot_floor: true`
/// (`opts.slotFloor !== false`, `parts.js:163`).
#[derive(Clone, Copy, Debug)]
pub struct RailOpts {
    pub base_h: f32,
    pub top_h: f32,
    pub waist: f32,
    pub width: f32,
    pub pitch: f32,
    pub slot: f32,
    pub crown_chamfer: f32,
    pub slot_floor: bool,
}

impl Default for RailOpts {
    fn default() -> Self {
        let p = PicatinnyOpts::default();
        RailOpts {
            base_h: p.base_h,
            top_h: p.top_h,
            waist: p.waist,
            width: p.width,
            pitch: p.pitch,
            slot: p.slot,
            crown_chamfer: p.crown_chamfer,
            slot_floor: true,
        }
    }
}

/// Picatinny run along Z, top face at `y`. `x` default `0` (`addRail`,
/// `parts.js:141-168`).
///
/// SLOT FLOORS (`parts.js:151-162`): a recoil slot is a 5.35 mm gap with a
/// 3.2 mm deep floor that in real light is always in shadow. Left in the
/// rail's own aluminium, the floor caught the sky at exactly the same rate
/// as the tooth tops, so a rail read as a ladder of flat near-white bars
/// instead of a row of cavities — the single loudest artefact on the whole
/// weapon. The strip is exactly the width of a tooth's foot, so it is
/// occluded by the teeth everywhere except inside the slots, where it
/// becomes the floor.
pub fn add_rail(asm: &mut Assembly, mat: &str, z0: f32, z1: f32, y: f32, x: f32, opts: RailOpts) {
    let len = (z1 - z0).abs();
    let base_h = opts.base_h;
    let top_h = opts.top_h;
    let waist = opts.waist;
    let cz = (z0 + z1) / 2.0;
    let yb = y - base_h - top_h;

    let g = picatinny(
        len,
        PicatinnyOpts {
            width: opts.width,
            waist: opts.waist,
            base_h: opts.base_h,
            top_h: opts.top_h,
            pitch: opts.pitch,
            slot: opts.slot,
            crown_chamfer: opts.crown_chamfer,
        },
    );
    asm.add(
        g,
        mat,
        Some(Xform {
            x,
            y: yb,
            z: cz,
            ..Default::default()
        }),
    );

    if opts.slot_floor {
        let floor = box_geo(waist * 0.99, 0.0014, len - 0.0004, 0.0002, 1);
        asm.add(
            floor,
            "cavity",
            Some(Xform {
                x,
                y: yb + base_h - 0.0003,
                z: cz,
                ..Default::default()
            }),
        );
    }
}
