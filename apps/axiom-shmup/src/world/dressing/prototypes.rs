//! Ported from Claude-of-Duty `src/world/dressing.js:151-303` —
//! `registerDressingProps`: the eight instanced prototypes only the dressing
//! pass uses (the burnt-out wreck, a flat tyre still on its hub, a fan of
//! blown glass, a cinder block, a produce tray and its heap, a wall conduit
//! box, and the plastic stool that is outside every shop).
//!
//! Registration order is the source's, and every builder call draws against
//! the shared `rng` stream — reordering, adding or removing one shifts every
//! subsequent draw in the whole level build (`registerDressingProps` runs
//! straight after `registerProps`, `world/index.js:111-112`).

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::weapons::geometry::primitives::ring;
use crate::world::assembler::{Assembler, ProtoSpec};
use crate::world::geo::WorldGeo;
use crate::world::kit::{chamfer_box, merge_simple, rock_geometry};
use crate::world::props::{burnt_car, RegisteredProto};

/// `registerDressingProps`'s per-prototype `opts` (`dressing.js:153`), with
/// the same defaults [`ProtoSpec`] documents.
#[derive(Debug, Clone, Copy)]
struct Opts {
    chunk: bool,
    max_dist: f32,
    cast_shadow: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { chunk: true, max_dist: 0.0, cast_shadow: true }
    }
}

/// `const P = (id, key, geo, opts = {}) => A.proto(id, { geo, key, ...opts });`
/// (`dressing.js:153`), plus a [`RegisteredProto`] summary for the golden
/// test — the same deliberate testability addition
/// `crate::world::props::register_props` already makes and documents.
fn p(a: &mut Assembler, out: &mut Vec<RegisteredProto>, id: &str, key: &str, geo: WorldGeo, o: Opts) {
    out.push(RegisteredProto {
        id: id.to_string(),
        key: key.to_string(),
        geo: geo.clone(),
        tilt: 0.0,
        sink: 0.0,
        skirt: 0.0,
        max_dist: o.max_dist,
        chunk: o.chunk,
        cast_shadow: o.cast_shadow,
        receive_shadow: true,
    });
    a.proto(
        id,
        ProtoSpec {
            geo,
            key: key.to_string(),
            tilt: 0.0,
            sink: 0.0,
            skirt: 0.0,
            cast_shadow: o.cast_shadow,
            receive_shadow: true,
            chunk: o.chunk,
            max_dist: o.max_dist,
            no_prepass: false,
        },
    );
}

/// `new THREE.Matrix4().makeRotationY(theta).setPosition(x, y, z)`
/// (`Matrix4.js`): a Y rotation with a translation in the fourth column.
fn rotation_y_at(theta: f64, x: f64, y: f64, z: f64) -> Mat4 {
    let (s, c) = theta.sin_cos();
    Mat4::from_cols_array([
        c as f32, 0.0, -s as f32, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        s as f32, 0.0, c as f32, 0.0, //
        x as f32, y as f32, z as f32, 1.0, //
    ])
}

/// `new THREE.Matrix4().makeRotationZ(theta).setPosition(x, y, z)`.
fn rotation_z_at(theta: f64, x: f64, y: f64, z: f64) -> Mat4 {
    let (s, c) = theta.sin_cos();
    Mat4::from_cols_array([
        c as f32, s as f32, 0.0, 0.0, //
        -s as f32, c as f32, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        x as f32, y as f32, z as f32, 1.0, //
    ])
}

/// A wheel still on the hub of a wreck — flat tyre, exposed rim
/// (`dressing.js:158-173`). Draws nothing from `rng`.
fn wheel_flat() -> WorldGeo {
    // `new THREE.TorusGeometry(0.24, 0.11, 10, 16)` — Three's parameter
    // order is `(radius, tube, radialSegments, tubularSegments, arc)`, and
    // `weapons::geometry::primitives::ring(radius, thickness, seg, rings,
    // arc)` names the tubular count `seg` and the radial count `rings`, so
    // the two swap here. `ring`'s trailing `normalize_attributes()` is inert
    // for a torus (both `uv` and `normal` are already populated).
    let g = ring(0.24, 0.11, 16, 10, std::f32::consts::TAU);
    let mut g = WorldGeo { pos: g.pos, normal: g.normal, uv: g.uv, color: Vec::new(), index: g.index };
    g.rotate_y(std::f32::consts::FRAC_PI_2);
    for p in g.pos.chunks_exact_mut(3) {
        p[1] *= 0.82;
    }
    g.compute_vertex_normals();
    g.fill_masks(0.3, 0.6, 0.2);
    g
}

/// Broken glass fan under a blown-out window (`dressing.js:176-198`).
fn glass_shards(rng: &mut Rng) -> WorldGeo {
    let mut list = Vec::new();
    for _ in 0..9 {
        let s = 0.03 + rng.float() * 0.06;
        let sz = s * rng.range(0.5, 1.6);
        let mut g = chamfer_box(s as f32, 0.004, sz as f32, 0.001);
        let a = rng.float() * 6.28;
        let tx = rng.range(-0.5, 0.5);
        let tz = rng.range(-0.4, 0.4);
        g.apply(&rotation_y_at(a, tx, 0.003, tz));
        g.fill_masks(0.6, 0.2, 0.0);
        list.push(g);
    }
    merge_simple(&list)
}

/// Cinder blocks — the universal Middle-Eastern building unit
/// (`dressing.js:201-213`). Draws nothing from `rng`.
fn cinder() -> WorldGeo {
    let mut g = chamfer_box(0.44, 0.21, 0.21, 0.012);
    g.paint_masks(|_x, _y, _z, _nx, ny, _nz, out, _i| {
        out[0] = 0.7;
        out[1] = 0.3 + (0.0f32).max(-ny) * 0.5;
        out[2] = (0.0f32).max(-ny) * 0.4;
    });
    g.translate(0.0, 0.105, 0.0);
    g
}

/// A stack of flat bread crates / produce trays for the stalls
/// (`dressing.js:216-239`). Draws nothing from `rng`.
fn tray() -> WorldGeo {
    let mut list: Vec<WorldGeo> = Vec::new();
    let mut add = |sx: f32, sy: f32, sz: f32, x: f32, y: f32, z: f32| {
        let mut g = chamfer_box(sx, sy, sz, 0.005);
        g.translate(x, y, z);
        list.push(g);
    };
    add(0.6, 0.02, 0.42, 0.0, 0.01, 0.0);
    for s in [-1.0f32, 1.0] {
        add(0.6, 0.09, 0.02, 0.0, 0.055, s * 0.2);
        add(0.02, 0.09, 0.42, s * 0.29, 0.055, 0.0);
    }
    let mut g = merge_simple(&list);
    // Only channels 0 and 1 are written: channel 2 keeps `chamferBox`'s own
    // per-vertex AO, exactly as the source's `paintMasks` callback does.
    g.paint_masks(|_x, _y, _z, _nx, _ny, _nz, out, _i| {
        out[0] = 0.8;
        out[1] = 0.35;
    });
    g
}

/// Produce heap: a lumpy mound that sits in a tray (`dressing.js:242-256`).
fn produce(rng: &mut Rng) -> WorldGeo {
    let mut list = Vec::new();
    for _ in 0..7 {
        // Argument order: the size draw happens BEFORE `rockGeometry`'s own
        // single `seed` draw.
        let size = rng.range(0.055, 0.1);
        let mut g = rock_geometry(rng, size as f32, 0, 0.8);
        let tx = rng.range(-0.22, 0.22);
        let ty = 0.035 + rng.range(0.0, 0.04);
        let tz = rng.range(-0.14, 0.14);
        g.translate(tx as f32, ty as f32, tz as f32);
        list.push(g);
    }
    let mut g = merge_simple(&list);
    g.fill_masks(0.15, 0.2, 0.1);
    g
}

/// Wall conduit box — small, but it is what makes a facade look serviced
/// (`dressing.js:259-275`). Draws nothing from `rng`.
fn conduit_box() -> WorldGeo {
    let mut list = Vec::new();
    let b = chamfer_box(0.2, 0.26, 0.11, 0.008);
    list.push(b);
    let mut lid = chamfer_box(0.17, 0.22, 0.02, 0.004);
    lid.translate(0.0, 0.0, 0.065);
    list.push(lid);
    let mut g = merge_simple(&list);
    g.paint_masks(|_x, _y, _z, _nx, _ny, _nz, out, _i| {
        out[0] = 0.85;
        out[1] = 0.45;
    });
    g
}

/// Cheap plastic chair — one is on every roof and outside every shop
/// (`dressing.js:278-300`). Draws nothing from `rng`.
fn stool() -> WorldGeo {
    let mut list = Vec::new();
    let mut top = chamfer_box(0.34, 0.04, 0.34, 0.01);
    top.translate(0.0, 0.42, 0.0);
    list.push(top);
    for sx in [-1.0f64, 1.0] {
        for sz in [-1.0f64, 1.0] {
            let mut leg = chamfer_box(0.035, 0.42, 0.035, 0.005);
            leg.apply(&rotation_z_at(sx * 0.06, sx * 0.13, 0.21, sz * 0.13));
            list.push(leg);
        }
    }
    let mut g = merge_simple(&list);
    g.paint_masks(|_x, _y, _z, _nx, ny, _nz, out, _i| {
        out[0] = 0.8;
        out[1] = 0.3 + (0.0f32).max(-ny) * 0.4;
    });
    g
}

/// `registerDressingProps(A, rng)` (`dressing.js:151-303`). Returns one
/// [`RegisteredProto`] per registered prototype — see that struct's own doc
/// for why the source's bare `return A;` grows a return value here.
pub fn register_dressing_props(a: &mut Assembler, rng: &mut Rng) -> Vec<RegisteredProto> {
    let mut out = Vec::new();

    p(a, &mut out, "wreck", "metal_dark", burnt_car(rng), Opts { chunk: false, ..Opts::default() });
    p(a, &mut out, "wheel_flat", "rubber", wheel_flat(), Opts::default());
    p(a, &mut out, "glass_shards", "glass", glass_shards(rng), Opts { max_dist: 40.0, cast_shadow: false, ..Opts::default() });
    p(a, &mut out, "cinder", "concrete_prop", cinder(), Opts::default());
    p(a, &mut out, "tray", "wood_prop", tray(), Opts::default());
    p(a, &mut out, "produce", "burlap", produce(rng), Opts { max_dist: 60.0, ..Opts::default() });
    p(a, &mut out, "conduit_box", "metal_dark", conduit_box(), Opts { max_dist: 55.0, ..Opts::default() });
    p(a, &mut out, "stool", "wood_prop", stool(), Opts::default());

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dressing_prototype_is_registered_once_in_source_order() {
        let mut a = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(20260821);
        let out = register_dressing_props(&mut a, &mut rng);
        let ids: Vec<&str> = out.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["wreck", "wheel_flat", "glass_shards", "cinder", "tray", "produce", "conduit_box", "stool"]);
        for id in ids {
            assert!(a.has(id), "missing prototype {id}");
        }
    }

    #[test]
    fn wheel_flat_is_a_squashed_torus_with_a_uniform_mask() {
        let g = wheel_flat();
        // TorusGeometry(0.24, 0.11, 10, 16): (10+1) * (16+1) vertices,
        // 10 * 16 quads.
        assert_eq!(g.vert_count(), 11 * 17);
        assert_eq!(g.tri_count(), 10 * 16 * 2);
        assert!(g.color.chunks_exact(3).all(|c| c == [0.3, 0.6, 0.2]));
        // `rotateY(PI/2)` leaves Y alone (it swaps X and Z), so the Y extent
        // is the full outer radius `0.24 + 0.11`, squashed by 0.82.
        let hi = g.pos.iter().skip(1).step_by(3).copied().fold(f32::NEG_INFINITY, f32::max);
        assert!((hi - 0.35 * 0.82).abs() < 1e-5, "{hi}");
    }

    #[test]
    fn register_dressing_props_is_deterministic_for_a_fixed_seed() {
        let mut a1 = Assembler::new(Rng::new(1));
        let mut r1 = Rng::new(42);
        register_dressing_props(&mut a1, &mut r1);
        let mut a2 = Assembler::new(Rng::new(1));
        let mut r2 = Rng::new(42);
        register_dressing_props(&mut a2, &mut r2);
        assert_eq!(r1.state(), r2.state());
    }
}
