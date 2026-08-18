//! Ported from Claude-of-Duty `src/world/util.js` (the geometry toolkit) and
//! `src/world/kit.js` (the modular building kit built on top of it) — see
//! `docs/work-manifests/claude-of-duty-port/02-port-recipe.md`'s task for
//! the split. Everything from `util.js` not already covered by
//! [`crate::world::masks`] (the mask convention) or [`crate::world::noise`]
//! (the noise basis these builders paint with) lives directly in this file:
//! `trs` (`util.js:86-92`), `chamferBox` (`util.js:267-361`), `weatherProp`
//! (`util.js:246-260`), `patchGeometry` (`util.js:642-669`), `wallPanel`
//! (below), plus the `THREE.PlaneGeometry`/`quad` pair (`util.js:379-384`,
//! `three/src/geometries/PlaneGeometry.js`, MIT licensed, Three.js authors)
//! that `buildGround` needs for the terrain and road strips.
//!
//! Everything from `kit.js` — the actual modular building elements
//! (`facadeWall`, `windowUnit`, `doorUnit`, `shopfront`, `balcony`,
//! `parapet`, `stairRun`, `stripedCloth`, `awning`, `drainpipe`,
//! `pockGeometry`, `spallPatch`, `rubbleMound`) — lives in this directory's
//! submodules, one per element family, and is re-exported flat here so
//! `crate::world::kit::facade_wall` etc. reads exactly like the source's
//! single-file `kit.js` namespace: [`facade`], [`window`], [`door`],
//! [`shopfront`], [`balcony`], [`parapet`], [`stairs`], [`canopy`],
//! [`pipework`], [`damage`], and the shared sub-primitives
//! (`solidSlabs`/`clothGeometry`/`tubeY`/`polyPrism`/`rockGeometry`/
//! `mergeSimple`) in [`primitives`].
//!
//! `plainBox` (`util.js:369-376`) is **not** duplicated here: `chamfer=0.0`
//! already takes `weapons::geometry::primitives::box_geo`'s unchamfered
//! `THREE.BoxGeometry(1,1,1)` branch (`box_geo`'s own doc: "Falls back to a
//! plain (unchamfered) indexed box when the clamped radius is negligible"),
//! so [`plain_box`] just calls that public API and adds the zeroed `color`
//! column the source's `plainBox` appends — reusing the already-ported,
//! already-tested box builder instead of a second hand-written copy of
//! `BoxGeometry`'s six-face construction.
//!
//! `wallPanel` and `Accum` are ported in [`wall_panel`] (this module) and
//! [`crate::world::accum`] respectively.
//!
//! ## `L`/`LL` collapse to one function
//!
//! `kit.js:33-52` defines two composers, `L` (allocates a scratch `Euler`)
//! and `LL` (reuses module-level scratch objects to dodge that allocation) —
//! both build the *exact same* `pm * TRS(x,y,z,ry,sx,sy,sz,rx,rz)` matrix in
//! the panel-space `'YXZ'` Euler order. That split exists only to dodge a
//! JS garbage-collector cost; a Rust `Mat4` is returned by value with no such
//! concern, so [`ll`] is the one function every element submodule composes
//! through, standing in for both `L` and `LL`.

use axiom_math::{Mat4, Quat, Vec3};

use crate::rng::Rng;
use crate::weapons::geometry::primitives::{box_geo, extrude, ExtrudeOpts};
use crate::world::noise::fbm3;

use super::geo::WorldGeo;

mod balcony;
mod canopy;
mod damage;
mod door;
mod facade;
mod parapet;
mod pipework;
mod primitives;
mod shopfront;
mod stairs;
mod window;

pub use balcony::{balcony, BalconyOpts, BalconyRailing, BalconyResult};
pub use canopy::{awning, striped_cloth, striped_cloth_default_bands, striped_cloth_default_seg_x, AwningOpts, AwningResult, StripedClothOpts};
pub use damage::{pock_geometry, rubble_mound, spall_patch, RubbleOpts};
pub use door::{door_unit, DoorOpts};
pub use facade::{facade_wall, FacadeSpec};
pub use parapet::{parapet, ParapetOpts};
pub use pipework::{drainpipe, DrainpipeOpts};
pub use primitives::{
    cloth_geometry, cylinder_geometry, merge_simple, poly_prism, rock_geometry, solid_slabs, tube_y, ClothOpts, SolidSlab,
};
pub use shopfront::{shopfront, ShopfrontOpts};
pub use stairs::{stair_run, StairOpts, StairRailing, StairResult};
pub use window::{window_state, window_unit, WindowOpts, WindowState};

/// `LL(pm, x, y, z, ry=0, sx=1, sy=1, sz=1, rx=0, rz=0)` (`kit.js:41-52`,
/// see this module's doc for why `L` collapses into the same function):
/// compose a local `'YXZ'`-order transform onto the panel matrix `pm`. Every
/// element builder in this directory composes its parts through this.
#[allow(clippy::too_many_arguments)]
pub fn ll(pm: &Mat4, x: f32, y: f32, z: f32, ry: f32, sx: f32, sy: f32, sz: f32, rx: f32, rz: f32) -> Mat4 {
    pm.multiply(trs(x, y, z, ry, sx, sy, sz, rx, rz))
}

/// `worldOf(pm, x, y, z)` (`kit.js:1099-1105`): transform a panel-space point
/// to level space. The source writes into a shared scratch triple to dodge
/// an allocation; a Rust `Vec3` is returned by value instead (see this
/// module doc's `L`/`LL` note for the same JS-GC-avoidance pattern).
pub fn world_of(pm: &Mat4, x: f32, y: f32, z: f32) -> Vec3 {
    pm.transform_point(Vec3::new(x, y, z))
}

/// `ryOf(pm)` (`kit.js:1108-1111`): extract the Y rotation baked into a panel
/// matrix, reading the same two column-major elements (`e[8]`, `e[10]`) the
/// source reads from `pm.elements`.
pub fn ry_of(pm: &Mat4) -> f32 {
    let c = pm.as_cols_array();
    c[8].atan2(c[10])
}

// ------------------------------------------------------ cached box/pane kit --
// `BOX`/`BOX_FINE`/`BOX_SOFT`/`BOX_THIN`/`PANE` (`kit.js:54-60`): each is a
// one-line `(A) => A.cache(key, factory)` arrow in the source. Ported as
// plain functions of the [`Assembler`] rather than closures captured once,
// since every call site already has `asm` in scope; every element submodule
// in this directory reaches for these instead of re-deriving the same cache
// key.
use crate::world::assembler::Assembler;

/// `BOX` (`kit.js:54`): a 44-triangle chamfered box, `0.012` bevel.
pub fn box_kit(asm: &mut Assembler) -> WorldGeo {
    asm.cache("box:0.012", || chamfer_box(1.0, 1.0, 1.0, 0.012))
}

/// `BOX_FINE` (`kit.js:55`): a finer `0.004` bevel, for small props.
pub fn box_fine_kit(asm: &mut Assembler) -> WorldGeo {
    asm.cache("box:0.004", || chamfer_box(1.0, 1.0, 1.0, 0.004))
}

/// `BOX_SOFT` (`kit.js:56`): a softer `0.03` bevel, for weathered masonry.
pub fn box_soft_kit(asm: &mut Assembler) -> WorldGeo {
    asm.cache("box:0.03", || chamfer_box(1.0, 1.0, 1.0, 0.03))
}

/// `BOX_THIN` (`kit.js:57-58`): a 12-tri unchamfered box for thin repeated
/// members (window frame rails, shutter slats, grille bars).
pub fn box_thin_kit(asm: &mut Assembler) -> WorldGeo {
    asm.cache("box:plain", || plain_box())
}

/// `PANE` (`kit.js:59-60`): a single quad, for window glass and thin panels.
pub fn pane_kit(asm: &mut Assembler) -> WorldGeo {
    asm.cache("pane", || quad(1.0, 1.0))
}

/// `slab(A, key, pm, x, y, z, sx, sy, sz, opts = null, ry = 0)` (`kit.js:63-65`):
/// merge a unit chamfer box scaled to a slab.
#[allow(clippy::too_many_arguments)]
pub fn slab(
    asm: &mut Assembler,
    key: &str,
    pm: &Mat4,
    x: f32,
    y: f32,
    z: f32,
    sx: f32,
    sy: f32,
    sz: f32,
    opts: Option<crate::world::accum::AccumAddOpts>,
    ry: f32,
) {
    let geo = box_kit(asm);
    let m = ll(pm, x, y, z, ry, sx, sy, sz, 0.0, 0.0);
    asm.add(key, &geo, Some(&m), opts);
}

// ------------------------------------------------------------------ trs --
/// `trs(out, x, y, z, ry=0, sx=1, sy=sx, sz=sx, rx=0, rz=0)` (`util.js:86-92`):
/// compose a translate * rotate * scale matrix without any shared mutable
/// scratch (the source reuses `_e`/`_q`/`_p`/`_s` module-level objects to
/// dodge a per-call allocation; nothing in Rust needs that dodge).
///
/// The rotation is Euler order **YXZ** (`new THREE.Euler(0,0,0,'YXZ')`,
/// `util.js:81`) — **not** the weapon geometry kit's `Assembly::add`, which
/// uses `'XYZ'`. The two really are different rotations for the same
/// `(rx,ry,rz)` triple; [`euler_yxz_quat`] composes `qy * qx * qz`, verified
/// against a real `three@0.180` `new THREE.Euler(0.3,-0.5,0.7,'YXZ')` /
/// `Quaternion.setFromEuler`, captured under Node:
/// `(x,y,z,w) = (0.052132410889547995, -0.2794438940784743,
/// 0.36323736972823584, 0.8872721876797527)` — see
/// `tests/world_port.rs::trs_matches_the_javascript`.
#[allow(clippy::too_many_arguments)]
pub fn trs(x: f32, y: f32, z: f32, ry: f32, sx: f32, sy: f32, sz: f32, rx: f32, rz: f32) -> Mat4 {
    let translate = Mat4::translation(Vec3::new(x, y, z));
    let rotate = Mat4::from_quaternion(euler_yxz_quat(rx, ry, rz));
    let scale = Mat4::scale(Vec3::new(sx, sy, sz));
    translate.multiply(rotate).multiply(scale)
}

/// Compose a unit rotation for Euler angles `(x, y, z)` in `THREE.Euler`'s
/// `'YXZ'` order. See [`trs`]'s doc for the golden capture that pins the
/// `qy * qx * qz` composition order (as opposed to `weapons::geometry::
/// assembly`'s `'XYZ'` order, `qx * qy * qz`).
fn euler_yxz_quat(x: f32, y: f32, z: f32) -> Quat {
    let (hx, hy, hz) = (x * 0.5, y * 0.5, z * 0.5);
    let qx = Quat::new(hx.sin(), 0.0, 0.0, hx.cos());
    let qy = Quat::new(0.0, hy.sin(), 0.0, hy.cos());
    let qz = Quat::new(0.0, 0.0, hz.sin(), hz.cos());
    qy.multiply(qx).multiply(qz)
}

// ----------------------------------------------------------- plane/quad --
/// `new THREE.PlaneGeometry(width, height, widthSegments, heightSegments)`
/// (`three/src/geometries/PlaneGeometry.js`, MIT licensed, Three.js
/// authors): a flat indexed grid in the XY plane (normal `+Z`), width along
/// X, height along Y, centred at the origin. `buildGround` always follows
/// this with a `rotateX(-PI/2)` ([`WorldGeo::rotate_x`]) to lay it flat.
pub fn plane_geometry(width: f32, height: f32, width_segments: u32, height_segments: u32) -> WorldGeo {
    let width_half = f64::from(width) / 2.0;
    let height_half = f64::from(height) / 2.0;
    let grid_x = width_segments;
    let grid_y = height_segments;
    let grid_x1 = grid_x + 1;
    let grid_y1 = grid_y + 1;
    let segment_width = f64::from(width) / f64::from(grid_x);
    let segment_height = f64::from(height) / f64::from(grid_y);

    let mut pos = Vec::new();
    let mut normal = Vec::new();
    let mut uv = Vec::new();
    for iy in 0..grid_y1 {
        let y = f64::from(iy) * segment_height - height_half;
        for ix in 0..grid_x1 {
            let x = f64::from(ix) * segment_width - width_half;
            pos.push(x as f32);
            pos.push(-y as f32);
            pos.push(0.0);
            normal.push(0.0);
            normal.push(0.0);
            normal.push(1.0);
            uv.push((f64::from(ix) / f64::from(grid_x)) as f32);
            uv.push((1.0 - f64::from(iy) / f64::from(grid_y)) as f32);
        }
    }

    let mut index = Vec::new();
    for iy in 0..grid_y {
        for ix in 0..grid_x {
            let a = ix + grid_x1 * iy;
            let b = ix + grid_x1 * (iy + 1);
            let c = (ix + 1) + grid_x1 * (iy + 1);
            let d = (ix + 1) + grid_x1 * iy;
            index.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }

    WorldGeo {
        pos,
        normal,
        uv,
        color: Vec::new(),
        index,
    }
}

/// `quad(w = 1, h = 1)` (`util.js:379-384`): a single-quad plane with a
/// zeroed `color` column already attached.
pub fn quad(w: f32, h: f32) -> WorldGeo {
    let mut g = plane_geometry(w, h, 1, 1);
    g.color = vec![0.0; g.vert_count() * 3];
    g
}

/// `plainBox()` (`util.js:369-376`) — see the module doc for why this reuses
/// `box_geo` rather than reimplementing `BoxGeometry`.
pub fn plain_box() -> WorldGeo {
    let g = box_geo(1.0, 1.0, 1.0, 0.0, 1);
    let color = vec![0.0; g.vert_count() * 3];
    WorldGeo {
        pos: g.pos,
        normal: g.normal,
        uv: g.uv,
        color,
        index: g.index,
    }
}

// ------------------------------------------------------------- chamfer --
/// `chamferBox(sx, sy, sz, bevel = 0.012)` (`util.js:267-361`): a hard box
/// with a real bevel on every edge and corner, 44 triangles total (6 faces x
/// 2 + 12 edges x 2 + 8 corners x 1) — see the module doc's box-count
/// arithmetic. Non-indexed, matching the source (`addPoly` never calls
/// `setIndex`).
///
/// Every coordinate here is exact `+ - *` arithmetic on the box's own
/// half-extents and clamped bevel radius — no `sin`/`cos`/`sqrt` touches a
/// *position*. `atan2` only orders a face's four corners before fanning them
/// into triangles (`util.js:330-334`); its inputs are always exactly `±1.0`
/// (the corner sign table), so the four resulting angles are `±π/4`/`±3π/4`
/// — far enough apart that libm ULP noise can never reorder them, which is
/// why this port pins position/normal/uv/color at exact equality rather
/// than a tolerance (see `tests/world_port.rs`).
pub fn chamfer_box(sx: f32, sy: f32, sz: f32, bevel: f32) -> WorldGeo {
    let (sx, sy, sz, bevel) = (f64::from(sx), f64::from(sy), f64::from(sz), f64::from(bevel));
    let h = [sx * 0.5, sy * 0.5, sz * 0.5];
    let b = (bevel.min(sx.min(sy).min(sz) * 0.4)).max(0.0005);

    let signs: Vec<[f64; 3]> = (0..8)
        .map(|i: i32| {
            [
                if i & 1 != 0 { 1.0 } else { -1.0 },
                if i & 2 != 0 { 1.0 } else { -1.0 },
                if i & 4 != 0 { 1.0 } else { -1.0 },
            ]
        })
        .collect();

    let vert = |ci: usize, axis: usize| -> [f64; 3] {
        let s = signs[ci];
        let mut p = [0.0; 3];
        for a in 0..3 {
            p[a] = s[a] * if a == axis { h[a] } else { h[a] - b };
        }
        p
    };

    let mut pos = Vec::new();
    let mut normal = Vec::new();
    let mut uv = Vec::new();
    let mut color = Vec::new();

    let mut add_poly = |pts: Vec<[f64; 3]>, wear: f64, grime: f64| {
        add_chamfer_poly(pts, wear, grime, &mut pos, &mut normal, &mut uv, &mut color);
    };

    // 6 faces.
    for axis in 0..3 {
        let a1 = (axis + 1) % 3;
        let a2 = (axis + 2) % 3;
        for &sa in &[-1.0f64, 1.0] {
            let mut corners: Vec<usize> = (0..8).filter(|&ci| signs[ci][axis] == sa).collect();
            corners.sort_by(|&p, &q| {
                let ap = signs[p][a2].atan2(signs[p][a1]);
                let aq = signs[q][a2].atan2(signs[q][a1]);
                ap.partial_cmp(&aq).expect("atan2 of finite ±1 inputs is never NaN")
            });
            add_poly(corners.into_iter().map(|ci| vert(ci, axis)).collect(), 0.06, 0.0);
        }
    }
    // 12 edge strips.
    for a in 0..3 {
        for bx in (a + 1)..3 {
            for &sa in &[-1.0f64, 1.0] {
                for &sb in &[-1.0f64, 1.0] {
                    let cs: Vec<usize> = (0..8).filter(|&ci| signs[ci][a] == sa && signs[ci][bx] == sb).collect();
                    add_poly(vec![vert(cs[0], a), vert(cs[0], bx), vert(cs[1], bx), vert(cs[1], a)], 1.0, 0.0);
                }
            }
        }
    }
    // 8 corner triangles.
    for ci in 0..8 {
        add_poly(vec![vert(ci, 0), vert(ci, 1), vert(ci, 2)], 1.0, 0.0);
    }

    WorldGeo {
        pos,
        normal,
        uv,
        color,
        index: Vec::new(),
    }
}

/// `addPoly(pts, wear, grime)` (`util.js:285-320`): orient the polygon
/// outward from the box centre, fan-triangulate it, and paint flat wear/grime
/// masks per triangle.
fn add_chamfer_poly(
    mut pts: Vec<[f64; 3]>,
    wear: f64,
    grime: f64,
    pos: &mut Vec<f32>,
    normal: &mut Vec<f32>,
    uv: &mut Vec<f32>,
    color: &mut Vec<f32>,
) {
    let n0 = cross3(sub3(pts[1], pts[0]), sub3(pts[2], pts[0]));
    let centroid = average3(&pts);
    if dot3(n0, centroid) < 0.0 {
        pts.reverse();
    }
    let n = normalize3(cross3(sub3(pts[1], pts[0]), sub3(pts[2], pts[0])));

    for t in 1..pts.len() - 1 {
        for p in [pts[0], pts[t], pts[t + 1]] {
            pos.push(p[0] as f32);
            pos.push(p[1] as f32);
            pos.push(p[2] as f32);
            normal.push(n[0] as f32);
            normal.push(n[1] as f32);
            normal.push(n[2] as f32);
            let ax = if n[0].abs() > n[1].abs() {
                if n[0].abs() > n[2].abs() {
                    0
                } else {
                    2
                }
            } else if n[1].abs() > n[2].abs() {
                1
            } else {
                2
            };
            uv.push((if ax == 0 { p[2] } else { p[0] }) as f32);
            uv.push((if ax == 1 { p[2] } else { p[1] }) as f32);
            let gr = if n[1] < -0.5 { grime + 0.35 } else { grime };
            color.push(wear as f32);
            color.push(gr.min(1.0) as f32);
            color.push(if n[1] < -0.4 { 0.35 } else { 0.0 });
        }
    }
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn average3(pts: &[[f64; 3]]) -> [f64; 3] {
    let n = pts.len() as f64;
    let mut c = [0.0; 3];
    for p in pts {
        c[0] += p[0];
        c[1] += p[1];
        c[2] += p[2];
    }
    [c[0] / n, c[1] / n, c[2] / n]
}
fn normalize3(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

// --------------------------------------------------------- weather prop --
/// `weatherProp(geo, opts = {})` (`util.js:246-260`): analytic wear on
/// convex/upward-facing surfaces, grime on undersides and toward the base,
/// AO toward the base — applied to nearly every prop so nothing reads as a
/// clean extruded box. Defaults match the source: `base=0.0`, `wear=0.85`,
/// `grime=0.5`, `down=0.6`, `height=1.0`; Rust has no default arguments, so
/// callers pass these explicitly.
#[allow(clippy::too_many_arguments)]
pub fn weather_prop(geo: &mut WorldGeo, base: f32, wear: f32, grime: f32, down: f32, height: f32) {
    let lo = geo.pos.iter().skip(1).step_by(3).copied().fold(f32::INFINITY, f32::min);
    let hi = geo.pos.iter().skip(1).step_by(3).copied().fold(f32::NEG_INFINITY, f32::max);
    let h = (height * (hi - lo)).max(1e-3);

    geo.paint_masks(|x, y, z, _nx, ny, nz, out, _i| {
        let up = ny.max(0.0);
        let dn = (-ny).max(0.0);
        let t = 1.0 - ((y - lo) / h).min(1.0);
        let n = fbm3(f64::from(x) * 3.1, f64::from(y) * 3.3, f64::from(z) * 3.1, 2) as f32;
        out[0] = (out[0] * wear + up * 0.18 * wear * n).min(1.0);
        out[1] = (out[1] + grime * (dn * down + t * t * base) * (0.55 + 0.9 * n)).min(1.0);
        out[2] = (out[2] + dn * 0.35 + t * t * base * 0.7).min(1.0);
        let _ = nz;
    });
}

// -------------------------------------------------------- patch geometry --
/// `patchGeometry(rng, radius, opts = {})` (`util.js:642-669`): a flat
/// irregular fan patch on the XZ plane (`y = -sag`), radius wobbled per lobe
/// by `rng`. Defaults: `lobes=9`, `wobble=0.45`, `sag=0.0`. Indexed
/// (triangle fan from the centre vertex), no `color` attribute — matching
/// the source, which never sets one.
pub fn patch_geometry(rng: &mut Rng, radius: f64, lobes: u32, wobble: f64, sag: f64) -> WorldGeo {
    let mut pos = vec![0.0f32, 0.0, 0.0];
    let mut normal = vec![0.0f32, 1.0, 0.0];
    let mut uv = vec![0.0f32, 0.0];

    let rs: Vec<f64> = (0..lobes).map(|_| radius * (1.0 - wobble + rng.float() * wobble * 2.0)).collect();
    for i in 0..lobes {
        let t = (f64::from(i) / f64::from(lobes)) * std::f64::consts::PI * 2.0;
        let r = rs[i as usize];
        pos.push((t.cos() * r) as f32);
        pos.push(-sag as f32);
        pos.push((t.sin() * r) as f32);
        normal.push(0.0);
        normal.push(1.0);
        normal.push(0.0);
        uv.push(t.cos() as f32);
        uv.push(t.sin() as f32);
    }

    let mut index = Vec::new();
    for i in 0..lobes {
        index.push(0);
        index.push(1 + i);
        index.push(1 + (i + 1) % lobes);
    }

    WorldGeo {
        pos,
        normal,
        uv,
        color: Vec::new(),
        index,
    }
}

// ---------------------------------------------------------------- wallPanel --
/// One `wallPanel` opening (`util.js`'s hole spec object,
/// `{x,y,w,h,arch?,sill?,ragged?}` — `sill` is read by callers, not by
/// `holePath`/`wallPanel` themselves, so it is not modelled here). `x`/`y` is
/// the hole centre in panel space; `arch` (0..1, arch radius as a fraction of
/// `w/2`) and `ragged` (a jitter radius) are mutually exclusive shape modes,
/// checked in that order — matching `holePath`'s `if (o.ragged) ... if
/// (o.arch > 0) ... else` (`util.js:398-438`): a hole with both set uses
/// `ragged`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WallHole {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub arch: f32,
    pub ragged: f32,
}

/// `wallPanel`'s `top`/`jag` options are coupled in the source: `jag` only
/// ever applies inside the `top !== 'ragged'` branch (`util.js:477-485`).
/// Modelled as one enum so an invalid `Ragged` + `jag` combination cannot be
/// constructed.
#[derive(Debug, Clone, Copy)]
pub enum WallTop {
    /// `top: 'flat'` (the default) with the source's `jag` amplitude
    /// (`util.js:452,478-484`; `0.0` reproduces the default `jag = 0`).
    Flat { jag: f32 },
    /// `top: 'ragged'` with `raggedAmp` (`util.js:452,458-475`; default `0.5`).
    Ragged { amp: f32 },
}

/// `wallPanel`'s `opts` (`util.js:451-452`). `bevel` defaults `0.02`;
/// `curve_segments` is the source's `opts.curveSegments ?? 6` — the sampling
/// resolution for an arched hole's two quadratic Bezier segments (see
/// [`wall_panel`]'s doc). Rust has no default arguments, so callers pass
/// these explicitly.
#[derive(Debug, Clone, Copy)]
pub struct WallPanelOpts {
    pub bevel: f32,
    pub top: WallTop,
    pub curve_segments: u32,
}

/// `holePath(o, rng)` (`util.js:392-439`), returning the OPEN point ring
/// (matching [`crate::weapons::geometry::primitives::extrude`]'s own contract
/// — see its module doc's "This port skips porting Shape/Path/CurvePath"
/// section: the caller supplies the closed-loop point list *before* the
/// auto-closing duplicate `Path.closePath()` would add, and `extrude` adds it
/// back). The `arch` branch's two `quadraticCurveTo` calls are the one place
/// in this whole port that samples a real Bezier curve (every other contour
/// in `util.js`/`kit.js` is straight `moveTo`/`lineTo` segments) — sampled
/// via [`quadratic_bezier_point`] at `curve_segments` divisions, matching
/// `Curve.getPoints(divisions)` / `QuadraticBezierCurve.getPoint(t)`
/// (`three/src/extras/core/Curve.js`, `.../curves/QuadraticBezierCurve.js`,
/// MIT licensed, Three.js authors) exactly: `t = d / divisions` for
/// `d = 1..=divisions` (the source's `d = 0` sample is always a duplicate of
/// the previous curve's last point and gets deduplicated away by
/// `CurvePath.getPoints`, so it is never emitted here either).
fn hole_path(o: &WallHole, mut rng: Option<&mut Rng>, curve_segments: u32) -> Vec<[f64; 2]> {
    let x0 = o.x - o.w / 2.0;
    let x1 = o.x + o.w / 2.0;
    let y0 = o.y - o.h / 2.0;
    let y1 = o.y + o.h / 2.0;

    if o.ragged > 0.0 {
        let n = 18u32;
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        let rx = (x1 - x0) / 2.0;
        let ry = (y1 - y0) / 2.0;
        let mut pts = Vec::with_capacity(n as usize);
        for i in 0..n {
            let t = (i as f32 / n as f32) * std::f32::consts::TAU;
            let c = t.cos();
            let s = t.sin();
            let k = 1.0 / c.abs().max(s.abs()).powf(0.85);
            let j = 1.0
                + rng
                    .as_mut()
                    .map_or(0.0, |r| r.range(f64::from(-o.ragged), f64::from(o.ragged)) as f32);
            pts.push([f64::from(cx + c * k * rx * j), f64::from(cy + s * k * ry * j)]);
        }
        return pts;
    }

    if o.arch > 0.0 {
        let r = (o.w / 2.0) * o.arch;
        let y_a = y1 - r;
        let mut pts = vec![[f64::from(x0), f64::from(y0)], [f64::from(x1), f64::from(y0)], [f64::from(x1), f64::from(y_a)]];
        let p0 = (f64::from(x1), f64::from(y_a));
        let c1 = (f64::from(x1), f64::from(y1));
        let e1 = (f64::from(o.x), f64::from(y1));
        for d in 1..=curve_segments {
            pts.push(quadratic_bezier_point(f64::from(d) / f64::from(curve_segments), p0, c1, e1));
        }
        let c2 = (f64::from(x0), f64::from(y1));
        let e2 = (f64::from(x0), f64::from(y_a));
        for d in 1..=curve_segments {
            pts.push(quadratic_bezier_point(f64::from(d) / f64::from(curve_segments), e1, c2, e2));
        }
        pts.push([f64::from(x0), f64::from(y0)]);
        return pts;
    }

    vec![
        [f64::from(x0), f64::from(y0)],
        [f64::from(x1), f64::from(y0)],
        [f64::from(x1), f64::from(y1)],
        [f64::from(x0), f64::from(y1)],
    ]
}

/// `QuadraticBezier(t, p0, p1, p2)` (`three/src/extras/core/Interpolations.js`,
/// MIT licensed, Three.js authors): `(1-t)^2 P0 + 2(1-t)t P1 + t^2 P2`,
/// applied per component.
fn quadratic_bezier_point(t: f64, p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) -> [f64; 2] {
    let k = 1.0 - t;
    [
        k * k * p0.0 + 2.0 * k * t * p1.0 + t * t * p2.0,
        k * k * p0.1 + 2.0 * k * t * p1.1 + t * t * p2.1,
    ]
}

/// `wallPanel(w, h, t, holes = [], opts = {})` (`util.js:451-515`): a wall
/// slab of real thickness `t` with real holes, extruded with a bevel so
/// every opening has depth and a chamfered reveal.
///
/// **Reuses `weapons::geometry::primitives::extrude`** for the actual
/// extrusion (bevel, hole triangulation via earcut, side-wall/lid faces) —
/// the same bevelled-extrude-with-holes engine, already ported and verified
/// against the JavaScript's own `THREE.ExtrudeGeometry` bevel path — rather
/// than a second ~400-line hand copy. Two consequences of that reuse, both
/// documented here rather than hidden:
///
/// 1. **Z convention.** `weapons::geometry::primitives::extrude` recentres
///    its output around `z=0` (translate by `-depth/2 + bevel`, matching
///    `geometry.js`'s own `extrude()` wrapper, `geometry.js:178`). `wallPanel`
///    wants panel space's own convention instead — "z from 0 at the OUTER
///    face to +t at the inner face" (`kit.js`'s panel-space doc) — which is
///    `extrude`'s raw (pre-translate) output shifted by just `+bevel`
///    (`util.js:500`, `if (bevel > 0) geo.translate(0, 0, bevel);`). Algebra:
///    `extrude`'s applied offset is `-t/2 + bevel`; the wanted offset is
///    `bevel`; the difference is `+t/2`, applied here as one corrective
///    [`WorldGeo::translate`] call after `extrude` returns.
/// 2. **Vertex welding.** `extrude` always welds coincident vertices at
///    `1e-6` (mirroring `geometry.js`'s `extrude()`, which calls
///    `mergeVertices` — `geometry.js:182`). The *raw* `wallPanel`
///    (`util.js:490-501`) builds a `THREE.ExtrudeGeometry` directly and never
///    welds it. Welding never changes **triangle count** (it only merges
///    duplicate vertices sharing the exact same position+normal+uv, which is
///    what this port's tests pin — see `tests/world_port.rs`) but can shift
///    exact vertex *count* and, at a welded seam, the per-vertex averaged
///    normal. A hand-written unwelded second extrude engine would remove
///    this divergence at the cost of duplicating `extrude_shape`'s ~400
///    lines (which is `pub(super)`-private to the weapons kit and cannot be
///    called directly); reuse was judged the better trade for a wall panel,
///    where the difference is invisible at render distance and the shape
///    (hole positions, bevel profile, triangle topology) is otherwise exact.
pub fn wall_panel(w: f32, h: f32, t: f32, holes: &[WallHole], opts: WallPanelOpts, mut rng: Option<&mut Rng>) -> WorldGeo {
    let x0 = -w / 2.0;
    let x1 = w / 2.0;
    let mut contour: Vec<[f64; 2]> = vec![[f64::from(x0), 0.0], [f64::from(x1), 0.0]];

    match opts.top {
        WallTop::Ragged { amp } => {
            let steps = (w / 0.55).round().max(4.0) as u32;
            let mut pts: Vec<(f32, f32)> = Vec::with_capacity(steps as usize + 1);
            for i in 0..=steps {
                let x = x1 - (i as f32 / steps as f32) * w;
                let f = i as f32 / steps as f32;
                let drop = amp * h * (0.25 + 0.75 * fbm3(f64::from(x) * 0.6 + 11.0, 3.1, 2.7, 3) as f32) * (0.35 + f);
                pts.push((x, (h - drop).max(0.4)));
            }
            contour.push([f64::from(x1), f64::from(pts[0].1)]);
            for i in 0..pts.len() {
                let (x, y) = pts[i];
                let nx = if i < pts.len() - 1 { pts[i + 1].0 } else { x0 };
                contour.push([f64::from(x), f64::from(y)]);
                let jitter = rng.as_mut().map_or(0.0, |r| r.range(-0.12, 0.12) as f32);
                contour.push([f64::from(nx), f64::from(y + jitter)]);
            }
            contour.push([f64::from(x0), f64::from(pts[pts.len() - 1].1)]);
        }
        WallTop::Flat { jag } => {
            contour.push([f64::from(x1), f64::from(h)]);
            if jag > 0.0 {
                let steps = (w / 1.2).round().max(3.0) as u32;
                for i in (1..steps).rev() {
                    let x = x0 + (i as f32 / steps as f32) * w;
                    let y = h + (fbm3(f64::from(x) * 1.7, 5.5, 1.3, 2) as f32 - 0.5) * jag;
                    contour.push([f64::from(x), f64::from(y)]);
                }
            }
            contour.push([f64::from(x0), f64::from(h)]);
        }
    }
    contour.push([f64::from(x0), 0.0]);

    let mut hole_rings: Vec<Vec<[f64; 2]>> = Vec::with_capacity(holes.len());
    for o in holes {
        let reborrowed = rng.as_mut().map(|r| &mut **r);
        hole_rings.push(hole_path(o, reborrowed, opts.curve_segments));
    }

    let extrude_opts = ExtrudeOpts {
        bevel: opts.bevel,
        bevel_segments: 1,
        curve_segments: opts.curve_segments,
        steps: 1,
        holes: hole_rings,
    };
    let raw = extrude(&contour, t, extrude_opts);
    let mut g = WorldGeo {
        pos: raw.pos,
        normal: raw.normal,
        uv: raw.uv,
        color: Vec::new(),
        index: raw.index,
    };
    g.translate(0.0, 0.0, t / 2.0);

    // paintMasks(geo, ...) (`util.js:506-513`).
    g.paint_masks(|x, y, z, _nx, ny, nz, out, _i| {
        let face = nz.abs();
        let reveal = 1.0 - face;
        let n = fbm3(f64::from(x) * 2.3, f64::from(y) * 2.1, f64::from(z) * 2.7, 2) as f32;
        out[0] = (reveal * 0.55 * (0.4 + n) + ny.max(0.0) * 0.3).min(1.0);
        out[1] = (reveal * 0.42 * (0.5 + n) + (-ny).max(0.0) * 0.55).min(1.0);
        out[2] = (reveal * 0.4 + (-ny).max(0.0) * 0.4).min(1.0);
    });
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_panel_no_holes_is_a_closed_flat_slab() {
        let g = wall_panel(2.0, 3.0, 0.3, &[], WallPanelOpts { bevel: 0.02, top: WallTop::Flat { jag: 0.0 }, curve_segments: 6 }, None);
        assert!(g.vert_count() > 0);
        assert!(g.tri_count() > 0);
        // z should span roughly [0, t] per the panel-space convention.
        let z_min = g.pos.iter().skip(2).step_by(3).copied().fold(f32::INFINITY, f32::min);
        let z_max = g.pos.iter().skip(2).step_by(3).copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(z_min > -0.05 && z_min < 0.05, "z_min={z_min}");
        assert!(z_max > 0.25 && z_max < 0.35, "z_max={z_max}");
    }

    #[test]
    fn wall_panel_with_a_rect_hole_has_fewer_triangles_than_a_bigger_slab_alone() {
        let solid = wall_panel(2.0, 3.0, 0.3, &[], WallPanelOpts { bevel: 0.02, top: WallTop::Flat { jag: 0.0 }, curve_segments: 6 }, None);
        let hole = WallHole { x: 0.0, y: 1.5, w: 0.6, h: 0.8, arch: 0.0, ragged: 0.0 };
        let with_hole = wall_panel(2.0, 3.0, 0.3, &[hole], WallPanelOpts { bevel: 0.02, top: WallTop::Flat { jag: 0.0 }, curve_segments: 6 }, None);
        // A hole adds side-wall/reveal triangles beyond the flat panel's own.
        assert!(with_hole.tri_count() > solid.tri_count());
    }

    #[test]
    fn wall_panel_arch_hole_samples_curve_segments_worth_of_extra_geometry() {
        let hole = WallHole { x: 0.0, y: 1.0, w: 0.8, h: 1.6, arch: 0.6, ragged: 0.0 };
        let coarse = wall_panel(2.0, 3.0, 0.3, &[hole], WallPanelOpts { bevel: 0.02, top: WallTop::Flat { jag: 0.0 }, curve_segments: 4 }, None);
        let fine = wall_panel(2.0, 3.0, 0.3, &[hole], WallPanelOpts { bevel: 0.02, top: WallTop::Flat { jag: 0.0 }, curve_segments: 12 }, None);
        assert!(fine.tri_count() > coarse.tri_count());
    }

    #[test]
    fn wall_panel_ragged_top_uses_an_rng_stream_deterministically() {
        let mut rng_a = Rng::new(42);
        let mut rng_b = Rng::new(42);
        let a = wall_panel(2.0, 3.0, 0.3, &[], WallPanelOpts { bevel: 0.02, top: WallTop::Ragged { amp: 0.5 }, curve_segments: 6 }, Some(&mut rng_a));
        let b = wall_panel(2.0, 3.0, 0.3, &[], WallPanelOpts { bevel: 0.02, top: WallTop::Ragged { amp: 0.5 }, curve_segments: 6 }, Some(&mut rng_b));
        assert_eq!(a.pos, b.pos);
        assert!(a.tri_count() > 0);
    }

    #[test]
    fn quadratic_bezier_endpoints_match_control_points() {
        let p0 = (0.0, 0.0);
        let p1 = (1.0, 2.0);
        let p2 = (2.0, 0.0);
        assert_eq!(quadratic_bezier_point(0.0, p0, p1, p2), [0.0, 0.0]);
        assert_eq!(quadratic_bezier_point(1.0, p0, p1, p2), [2.0, 0.0]);
        let mid = quadratic_bezier_point(0.5, p0, p1, p2);
        assert!((mid[0] - 1.0).abs() < 1e-12);
        assert!((mid[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn chamfer_box_produces_44_triangles_non_indexed() {
        let g = chamfer_box(1.0, 1.0, 1.0, 0.012);
        assert!(g.index.is_empty());
        assert_eq!(g.tri_count(), 44);
        assert_eq!(g.vert_count(), 132);
    }

    #[test]
    fn chamfer_box_face_vertex_sits_at_the_full_half_extent() {
        // Every "face" quad vertex has its dominant axis at the FULL h[axis]
        // (not h[axis]-b) — that's the whole point of a chamfer.
        let g = chamfer_box(2.0, 2.0, 2.0, 0.1);
        let has_full_extent_x = g.pos.chunks_exact(3).any(|p| (p[0].abs() - 1.0).abs() < 1e-6);
        assert!(has_full_extent_x);
    }

    #[test]
    fn plane_geometry_matches_three_planegeometry_shape() {
        let g = plane_geometry(2.0, 4.0, 2, 1);
        // (widthSegments+1) * (heightSegments+1) = 3 * 2 = 6 vertices.
        assert_eq!(g.vert_count(), 6);
        // widthSegments * heightSegments * 2 triangles = 4.
        assert_eq!(g.tri_count(), 4);
        // First vertex: ix=0, iy=0 -> x=-1, y=-(-2)=2, z=0.
        assert_eq!(&g.pos[0..3], &[-1.0, 2.0, 0.0]);
        assert_eq!(&g.normal[0..3], &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn quad_has_a_zeroed_color_column() {
        let g = quad(1.0, 1.0);
        assert_eq!(g.vert_count(), 4);
        assert_eq!(g.color, vec![0.0; 12]);
    }

    #[test]
    fn plain_box_reuses_box_geo_and_zeroes_color() {
        let g = plain_box();
        assert_eq!(g.tri_count(), 12);
        assert_eq!(g.color.len(), g.pos.len());
        assert!(g.color.iter().all(|&c| c == 0.0));
    }

    #[test]
    fn weather_prop_wears_the_top_and_grimes_the_bottom() {
        let mut g = plain_box();
        weather_prop(&mut g, 0.0, 0.85, 0.5, 0.6, 1.0);
        // Find a bottom-facing vertex (normal.y < -0.5) and a top-facing one.
        let bottom_grime: Vec<f32> = g
            .normal
            .chunks_exact(3)
            .zip(g.color.chunks_exact(3))
            .filter(|(n, _)| n[1] < -0.5)
            .map(|(_, c)| c[1])
            .collect();
        let top_grime: Vec<f32> = g
            .normal
            .chunks_exact(3)
            .zip(g.color.chunks_exact(3))
            .filter(|(n, _)| n[1] > 0.5)
            .map(|(_, c)| c[1])
            .collect();
        assert!(!bottom_grime.is_empty() && !top_grime.is_empty());
        let avg = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!(avg(&bottom_grime) > avg(&top_grime));
    }

    #[test]
    fn patch_geometry_lobe_count_drives_vertex_and_triangle_count() {
        let mut rng = Rng::new(1);
        let g = patch_geometry(&mut rng, 1.0, 9, 0.45, 0.0);
        assert_eq!(g.vert_count(), 10); // centre + 9 lobes
        assert_eq!(g.tri_count(), 9);
        assert!(g.color.is_empty());
    }

    #[test]
    fn patch_geometry_sag_offsets_the_rim_but_never_the_centre_vertex() {
        // The centre vertex (index 0) is always pinned at y=0
        // (`pos.push(0, 0, 0)`, `util.js:648`) — only the rim ring drops by
        // `-sag`.
        let mut rng = Rng::new(1);
        let g = patch_geometry(&mut rng, 1.0, 6, 0.0, 0.25);
        assert_eq!(g.pos[1], 0.0);
        for chunk in g.pos.chunks_exact(3).skip(1) {
            assert_eq!(chunk[1], -0.25);
        }
    }

    #[test]
    fn trs_yxz_matches_the_captured_three_js_quaternion() {
        // new THREE.Euler(0.3, -0.5, 0.7, 'YXZ') -> Quaternion.setFromEuler,
        // captured under Node with three@0.180 (see this module's `trs` doc).
        let q = euler_yxz_quat(0.3, -0.5, 0.7);
        let want = (0.052_132_410_889_547_995, -0.279_443_894_078_474_3, 0.363_237_369_728_235_84, 0.887_272_187_679_752_7);
        assert!((f64::from(q.x) - want.0).abs() < 1e-6);
        assert!((f64::from(q.y) - want.1).abs() < 1e-6);
        assert!((f64::from(q.z) - want.2).abs() < 1e-6);
        assert!((f64::from(q.w) - want.3).abs() < 1e-6);
    }

    #[test]
    fn trs_identity_is_the_identity_matrix() {
        let m = trs(0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
        assert_eq!(m.as_cols_array(), Mat4::IDENTITY.as_cols_array());
    }

    #[test]
    fn trs_translates() {
        let m = trs(1.0, 2.0, 3.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
        let c = m.as_cols_array();
        assert_eq!([c[12], c[13], c[14]], [1.0, 2.0, 3.0]);
    }
}
