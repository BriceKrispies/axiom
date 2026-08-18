//! Ported from Claude-of-Duty `src/weapons/geometry.js:213-357` (`screw`,
//! `knurlBand`, `serrations`, `picatinny`, `mlokSlot`) — the primitives that
//! compose several sub-pieces through `merge_all`, rather than building one
//! geometry directly.

use super::lathe::lathe_z;
use super::octahedron::octahedron_detail0;
use super::rounded_box::box_geo;
use super::extrude::{extrude, round_rect, ExtrudeOpts};
use super::xform;
use super::super::{merge_all, Geo};

/// The axis `serrations` cuts across — `axis = 'x' | 'y'` in the source
/// (`geometry.js:268`), a string enum here since Rust has no string-literal
/// union type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// `opts` on `picatinny(len, opts = {})` (`geometry.js:308-318`). Defaults
/// match the source: MIL-STD-1913 dimensions in metres.
#[derive(Clone, Copy, Debug)]
pub struct PicatinnyOpts {
    pub width: f32,
    pub waist: f32,
    pub base_h: f32,
    pub top_h: f32,
    pub pitch: f32,
    pub slot: f32,
    pub crown_chamfer: f32,
}

impl Default for PicatinnyOpts {
    fn default() -> Self {
        PicatinnyOpts {
            width: 0.0212,
            waist: 0.0157,
            base_h: 0.0042,
            top_h: 0.0032,
            pitch: 0.01055,
            slot: 0.00535,
            crown_chamfer: 0.0015,
        }
    }
}

/// Hex-socket cap screw, axis `+Z`, head at `z = 0` facing `-Z`. `seg`
/// default `12` (`geometry.js:218`). The socket is a genuine counterbore
/// (annular head + a plug at the bottom) so it reads as a hole instead of a
/// painted dot.
pub fn screw(r_head: f32, r_shank: f32, head_h: f32, shank_l: f32, seg: u32) -> Geo {
    let r_socket = r_head * 0.52;
    let mut parts = Vec::new();

    parts.push(lathe_z(
        &[
            [0.0, r_socket],
            [0.0, r_head - 0.0002],
            [0.0002, r_head],
            [head_h, r_head],
            [head_h, r_shank],
            [head_h + shank_l, r_shank],
            [head_h + shank_l, 0.0],
        ],
        seg,
        0.0,
        std::f32::consts::TAU,
    ));

    // Counterbore wall + floor.
    parts.push(lathe_z(
        &[[head_h * 0.62, 0.0], [head_h * 0.62, r_socket], [0.0, r_socket]],
        6,
        0.0,
        std::f32::consts::TAU,
    ));

    merge_all(parts).expect("screw always builds exactly two parts")
}

/// Knurling/checkering: a band of tiny pyramids around a cylinder. `count`
/// default `28`, `depth` default `0.0004`, `rows` default `3`
/// (`geometry.js:249`).
pub fn knurl_band(radius: f32, len: f32, count: u32, depth: f32, rows: u32) -> Geo {
    let mut cell = octahedron_detail0(f64::from(depth) * 2.2);
    xform::scale(&mut cell, 1.0, 1.0, 0.55);

    let mut parts = Vec::new();
    (0..rows).for_each(|r| {
        let z = -f64::from(len) / 2.0 + ((f64::from(r) + 0.5) / f64::from(rows)) * f64::from(len);
        (0..count).for_each(|i| {
            let a = (f64::from(i) / f64::from(count)) * std::f64::consts::TAU
                + f64::from(r % 2) * (std::f64::consts::PI / f64::from(count));
            let mut g = cell.clone();
            xform::rotate_z(&mut g, a as f32);
            xform::translate(&mut g, (a.cos() * f64::from(radius)) as f32, (a.sin() * f64::from(radius)) as f32, z as f32);
            g.normalize_attributes();
            parts.push(g);
        });
    });

    merge_all(parts).expect("knurl_band always builds rows * count > 0 parts")
}

/// Fine longitudinal serrations (slide grip, handguard panels, mag ribs).
/// `depth` default `0.0006` (`geometry.js:268`).
pub fn serrations(w: f32, h: f32, len: f32, count: u32, depth: f32, axis: Axis) -> Geo {
    let step = match axis {
        Axis::X => w,
        Axis::Y => h,
    } / count as f32;
    let (rib_w, rib_h) = match axis {
        Axis::X => (step * 0.55, h),
        Axis::Y => (w, step * 0.55),
    };
    let rib = box_geo(rib_w, rib_h, len, depth * 0.9, 1);

    let mut parts = Vec::new();
    (0..count).for_each(|i| {
        let t = -0.5 + (i as f32 + 0.5) / count as f32;
        let mut g = rib.clone();
        match axis {
            Axis::X => xform::translate(&mut g, t * w, 0.0, 0.0),
            Axis::Y => xform::translate(&mut g, 0.0, t * h, 0.0),
        }
        parts.push(g);
    });

    let mut merged = merge_all(parts).expect("serrations always builds count > 0 parts");
    // `merged.translate(0, 0, 0)` (`geometry.js:281`) — a literal no-op
    // translation, kept for fidelity: `Geo::apply` still re-normalizes every
    // normal even for an identity transform (see `xform`'s doc), matching
    // the source's `applyMatrix4` doing the same.
    xform::translate(&mut merged, 0.0, 0.0, 0.0);
    merged
}

/// MIL-STD-1913 Picatinny rail running along Z. See `geometry.js:285-307`
/// for the real-world dimensions and the reasoning behind the crown chamfer.
pub fn picatinny(len: f32, opts: PicatinnyOpts) -> Geo {
    let PicatinnyOpts {
        width,
        waist,
        base_h,
        top_h,
        pitch,
        slot,
        crown_chamfer: ch,
    } = opts;
    // Fixed local constant in the source (`geometry.js:316`), not exposed
    // through `opts`.
    let chamfer = 0.000_35;

    let teeth = 1u32.max(((len + slot) / pitch).floor() as u32);
    let tooth_len = pitch - slot;
    let mut parts = Vec::new();

    let mut base = box_geo(width, base_h, len, chamfer, 1);
    xform::translate(&mut base, 0.0, base_h / 2.0, 0.0);
    parts.push(base);

    let profile = [
        [-waist / 2.0, 0.0],
        [-width / 2.0, top_h - ch],
        [-width / 2.0 + ch, top_h],
        [width / 2.0 - ch, top_h],
        [width / 2.0, top_h - ch],
        [waist / 2.0, 0.0],
    ];
    let tooth = extrude(
        &profile,
        tooth_len,
        ExtrudeOpts {
            bevel: 0.00025,
            bevel_segments: 1,
            ..Default::default()
        },
    );

    for i in 0..teeth {
        let z = len / 2.0 - tooth_len / 2.0 - i as f32 * pitch;
        if z - tooth_len / 2.0 < -len / 2.0 {
            break;
        }
        let mut g = tooth.clone();
        xform::translate(&mut g, 0.0, base_h, z);
        parts.push(g);
    }

    merge_all(parts).expect("picatinny always builds the base plus >= 1 tooth")
}

/// M-LOK style slot: a recessed pocket with a raised lip, for handguard
/// slats. `len` default `0.032`, `wide` default `0.0075`, `depth` default
/// `0.0022` (`geometry.js:350`).
pub fn mlok_slot(len: f32, wide: f32, depth: f32) -> Geo {
    let outer = extrude(
        &round_rect(len, wide + 0.0028, 0.0014, 3),
        0.0016,
        ExtrudeOpts {
            bevel: 0.0004,
            ..Default::default()
        },
    );
    let mut inner = extrude(
        &round_rect(len - 0.0016, wide, 0.0012, 3),
        depth,
        ExtrudeOpts {
            bevel: 0.0003,
            ..Default::default()
        },
    );
    xform::translate(&mut inner, 0.0, 0.0, -depth * 0.35);

    merge_all(vec![outer, inner]).expect("mlok_slot always builds the outer plus the inner pocket")
}
