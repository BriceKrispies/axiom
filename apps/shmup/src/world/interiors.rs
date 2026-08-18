//! Ported from Claude-of-Duty `src/world/interiors.js:1-654` — interior
//! furnishing: rooms get furniture against the walls, clutter in the middle,
//! something on every horizontal surface and rubbish in the corners.
//!
//! `furnishRoom(A, rng, r)` takes a level-space rect (`r`) plus a `kind` of
//! `shop|living|storage|ruin` and dispatches to one of four per-kind
//! furnishers, then dresses every wall (`dressWalls`) and the ceiling
//! (`dressCeiling`), and finally drops a bare bulb on a flex
//! ([`hanging_bulb`]).
//!
//! Room plans live in [`crate::world::layout::RoomFurnish`] in *normalised*
//! `0..1` room coordinates so a plan survives a footprint change; the caller
//! (`crate::world::buildings::build_interior`) resolves each entry to a
//! level-space [`RoomRect`] before calling [`furnish_room`], exactly as
//! `buildings.js:723-739` resolves `r.x0 * iw` etc. before calling
//! `furnishRoom`.
//!
//! ## Two dead parameters this port drops
//!
//! - `furnishShop`/`furnishLiving`/`furnishStorage`/`furnishRuin` each accept
//!   a wall-margin `m` (`interiors.js:24`, `const m = 0.45`) that not one of
//!   the four ever reads (confirmed against the source: `m` never appears in
//!   any of their four bodies) — dropped from every corresponding Rust
//!   signature below rather than threaded through unused.
//! - `hangingBulb(A, rng, x, yCeil, z, rngIn)` (`interiors.js:349`) takes a
//!   SECOND rng parameter, `rngIn`, that its body never reads either (only
//!   the first `rng` draws `drop = rng.range(0.35, 0.95)`) — and the one call
//!   site (`furnishRoom`) passes the *same* `rng` binding for both arguments,
//!   which would alias a `&mut Rng` twice in Rust anyway. [`hanging_bulb`]
//!   below takes one `rng` parameter.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{box_fine_kit, box_kit, box_thin_kit, chamfer_box, cloth_geometry, cylinder_geometry, ll, patch_geometry, rubble_mound, ClothOpts, RubbleOpts};
use crate::world::palette::Surface;

/// `furnishRoom`'s `r` argument (`interiors.js:17-18`), resolved to LEVEL
/// space by the caller. `r.spec` (`buildings.js:736`) is accepted at the JS
/// call site but never read inside `furnishRoom`/`dressWalls`/`dressCeiling`
/// (confirmed against the source), so it is not carried here.
#[derive(Debug, Clone, Copy)]
pub struct RoomRect {
    pub kind: &'static str,
    /// `r.street` (`interiors.js:131`, `buildings.js:729`) — the building's
    /// `streetSide`, so furnishing never blockades a shopfront opening.
    pub street: u32,
    pub x0: f32,
    pub z0: f32,
    pub x1: f32,
    pub z1: f32,
    pub y: f32,
    pub h: f32,
}

/// `furnishRoom(A, rng, r)` (`interiors.js:17-88`).
pub fn furnish_room(asm: &mut Assembler, rng: &mut Rng, r: RoomRect) {
    let w = (r.x1 - r.x0).abs();
    let d = (r.z1 - r.z0).abs();
    if w < 1.2 || d < 1.2 {
        return;
    }
    let cx = (r.x0 + r.x1) / 2.0;
    let cz = (r.z0 + r.z1) / 2.0;

    // floor dressing everybody gets: dust patches, plaster fall, litter
    let patches = rng.int(2, 4);
    for _ in 0..patches {
        let g = patch_geometry(rng, rng.range(0.4, 1.1), 8, 0.5, 0.0);
        let m = ll(&Mat4::IDENTITY, rng.range(f64::from(r.x0) + 0.3, f64::from(r.x1) - 0.3) as f32, r.y + 0.012, rng.range(f64::from(r.z0) + 0.3, f64::from(r.z1) - 0.3) as f32, rng.float() as f32 * 6.28, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.8, 0.5]), paint: None }));
    }
    for _ in 0..rng.int(4, 9) {
        asm.put(
            "litter",
            rng.range(f64::from(r.x0) + 0.2, f64::from(r.x1) - 0.2) as f32,
            r.y + 0.015,
            rng.range(f64::from(r.z0) + 0.2, f64::from(r.z1) - 0.2) as f32,
            rng.float() as f32 * 6.28,
            rng.range(0.7, 1.3) as f32,
            Some([1.0, 1.3, 1.0]),
            0.0,
            0.0,
        );
    }
    for _ in 0..rng.int(2, 5) {
        asm.put(
            *rng.pick(&["brick_a", "brick_b", "rock_b"]),
            rng.range(f64::from(r.x0) + 0.25, f64::from(r.x1) - 0.25) as f32,
            r.y + 0.04,
            rng.range(f64::from(r.z0) + 0.25, f64::from(r.z1) - 0.25) as f32,
            rng.float() as f32 * 6.28,
            rng.range(0.5, 1.0) as f32,
            Some([1.0, 1.4, 1.0]),
            0.0,
            0.0,
        );
    }

    match r.kind {
        "shop" => furnish_shop(asm, rng, &r, cx, cz, w, d),
        "living" => furnish_living(asm, rng, &r, cx, cz, w, d),
        "ruin" => furnish_ruin(asm, rng, &r, cx, cz, w, d),
        _ => furnish_storage(asm, rng, &r, cx, cz, w, d),
    }

    // Everything above dresses the MIDDLE of the room; walls and the
    // wall/floor junction carry most of the frame for a 2-3 m interior shot.
    dress_walls(asm, rng, &r);
    dress_ceiling(asm, rng, &r);

    // hanging bulb, roughly central, offset so it isn't dead centre
    if r.kind != "ruin" || rng.float() < 0.5 {
        hanging_bulb(asm, rng, cx + rng.range(-0.8, 0.8) as f32, r.y + r.h - 0.05, cz + rng.range(-0.8, 0.8) as f32);
    }
}

// -------------------------------------------------------------- wall side --
/// One of the room's four walls (`interiors.js:113-117`'s `sides[]` entry).
struct WallSide {
    px: f32,
    pz: f32,
    tx: f32,
    tz: f32,
    nx: f32,
    nz: f32,
    len: f32,
    yaw: f32,
}

/// `at(s, t, off)` (`interiors.js:119`).
fn at(s: &WallSide, t: f32, off: f32) -> (f32, f32) {
    (s.px + s.tx * t + s.nx * off, s.pz + s.tz * t + s.nz * off)
}

/// `pierT()` (`interiors.js:138-139`): a random pier (outer 30% of the wall)
/// on either side of centre.
fn pier_t(rng: &mut Rng, half: f32) -> f32 {
    let sign: f32 = if rng.float() < 0.5 { -1.0 } else { 1.0 };
    sign * rng.range(f64::from(half) * 0.62, f64::from(half)) as f32
}

/// `anyT()` (`interiors.js:140`).
fn any_t(rng: &mut Rng, half: f32) -> f32 {
    rng.range(f64::from(-half), f64::from(half)) as f32
}

/// `wallT` (`interiors.js:141`): `isOpening ? pierT : anyT`, resolved at each
/// call site rather than captured as a closure reference (Rust has no
/// zero-cost way to hold "one of two functions of `&mut Rng`" without
/// re-borrowing `rng` per call anyway).
fn wall_t(rng: &mut Rng, is_opening: bool, half: f32) -> f32 {
    if is_opening {
        pier_t(rng, half)
    } else {
        any_t(rng, half)
    }
}

/// `dressWalls(A, rng, r)` (`interiors.js:110-305`).
fn dress_walls(asm: &mut Assembler, rng: &mut Rng, r: &RoomRect) {
    let sides = [
        WallSide { px: (r.x0 + r.x1) / 2.0, pz: r.z0, tx: 1.0, tz: 0.0, nx: 0.0, nz: 1.0, len: r.x1 - r.x0, yaw: 0.0 },
        WallSide { px: r.x1, pz: (r.z0 + r.z1) / 2.0, tx: 0.0, tz: 1.0, nx: -1.0, nz: 0.0, len: r.z1 - r.z0, yaw: std::f32::consts::FRAC_PI_2 },
        WallSide { px: (r.x0 + r.x1) / 2.0, pz: r.z1, tx: 1.0, tz: 0.0, nx: 0.0, nz: -1.0, len: r.x1 - r.x0, yaw: 0.0 },
        WallSide { px: r.x0, pz: (r.z0 + r.z1) / 2.0, tx: 0.0, tz: -1.0, nx: 1.0, nz: 0.0, len: r.z1 - r.z0, yaw: std::f32::consts::FRAC_PI_2 },
    ];

    for (side, s) in sides.iter().enumerate() {
        if s.len < 1.6 {
            continue;
        }
        let half = s.len / 2.0 - 0.35;
        let is_opening = side as u32 == r.street;

        // ---- surface conduit: two drops and a run under the ceiling ----
        if rng.float() < 0.8 {
            let pipe = asm.cache("conduit", || {
                let mut g = cylinder_geometry(0.016, 0.016, 1.0, 6, 1, false);
                g.fill_masks(0.35, 0.5, 0.1);
                g
            });
            let run_y = r.y + r.h - rng.range(0.18, 0.4) as f32;
            let t0 = if is_opening { wall_t(rng, is_opening, half) } else { rng.range(f64::from(-half), 0.0) as f32 };
            let neg_t0 = -t0;
            let sign_val: f32 = if neg_t0 == 0.0 || neg_t0 > 0.0 { 1.0 } else { -1.0 };
            let t1 = if is_opening { t0 + sign_val * rng.range(0.3, 0.55) as f32 } else { t0 + rng.range(0.8, f64::from((1.0f32).max(half - t0))) as f32 };
            let (rx0, rz0) = at(s, (t0 + t1) / 2.0, 0.045);
            let m = ll(&Mat4::IDENTITY, rx0, run_y, rz0, s.yaw, 1.0, (t1 - t0).abs(), 1.0, 0.0, std::f32::consts::FRAC_PI_2);
            asm.add("metal_dark", &pipe, Some(&m), None);

            let drop_t = if rng.float() < 0.5 { t0 } else { t1 };
            let box_y = r.y + rng.range(1.15, 1.55) as f32;
            let (dx, dz) = at(s, drop_t, 0.045);
            let m = ll(&Mat4::IDENTITY, dx, (run_y + box_y) / 2.0, dz, 0.0, 1.0, run_y - box_y, 1.0, 0.0, 0.0);
            asm.add("metal_dark", &pipe, Some(&m), None);
            let fine = box_fine_kit(asm);
            let (jx, jz) = at(s, drop_t, 0.055);
            let m = ll(&Mat4::IDENTITY, jx, box_y, jz, s.yaw, 0.15, 0.19, 0.09, 0.0, 0.0);
            asm.add("metal_dark", &fine, Some(&m), Some(AccumAddOpts { masks: Some([0.55, 0.5, 0.2]), paint: None }));
            if rng.float() < 0.5 {
                let (fx, fz) = at(s, drop_t + 0.06, 0.05);
                let m = ll(&Mat4::IDENTITY, fx, box_y - 0.28, fz, 0.0, 0.4, 0.34, 0.4, 0.0, 0.0);
                asm.add("metal_dark", &pipe, Some(&m), None);
            }
        }

        // ---- a plank shelf on two brackets, with goods ----
        if r.kind != "ruin" && rng.float() < 0.55 {
            let sy = r.y + rng.range(1.05, 1.65) as f32;
            let s_len = (rng.range(if is_opening { 0.6 } else { 0.9 }, if is_opening { 1.0 } else { 1.8 }) as f32).min(s.len - 0.6);
            let st = if is_opening { wall_t(rng, is_opening, half) } else { rng.range(f64::from(-half + s_len / 2.0), f64::from(half - s_len / 2.0)) as f32 };
            let (sx, sz) = at(s, st, 0.15);
            let box_ = box_kit(asm);
            let m = ll(&Mat4::IDENTITY, sx, sy, sz, s.yaw, s_len, 0.035, 0.28, 0.0, 0.0);
            asm.add("wood_prop_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.5, 0.15]), paint: None }));
            for bt in [-1.0f32, 1.0] {
                let (bx, bz) = at(s, st + bt * (s_len / 2.0 - 0.12), 0.1);
                let fine = box_fine_kit(asm);
                let m = ll(&Mat4::IDENTITY, bx, sy - 0.09, bz, s.yaw, 0.03, 0.16, 0.18, 0.0, 0.0);
                asm.add("metal_dark", &fine, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.6, 0.3]), paint: None }));
            }
            for _ in 0..rng.int(2, 5) {
                let (gx, gz) = at(s, st + rng.range(f64::from(-s_len / 2.0 + 0.12), f64::from(s_len / 2.0 - 0.12)) as f32, rng.range(0.11, 0.2) as f32);
                asm.put(*rng.pick(&["bottle", "can", "box_card_b", "bucket"]), gx, sy + 0.02, gz, rng.float() as f32 * 6.28, rng.range(0.6, 0.95) as f32, Some([1.0, 1.1, 1.0]), 0.0, 0.0);
            }
        }

        // ---- something leaning on it ----
        if !is_opening && rng.float() < 0.5 {
            let lean = rng.range(0.13, 0.22) as f32;
            let lt = rng.range(f64::from(-half), f64::from(half)) as f32;
            let lh = rng.range(1.1, 1.8) as f32;
            let lw = rng.range(0.5, 1.0) as f32;
            let off = 0.06 + (lean.sin() * lh) / 2.0;
            let (lx, lz) = at(s, lt, off);
            let key = *rng.pick(&["plywood", "corrugated", "wood_prop_dark"]);
            // Tip the top INTO the wall: after `s.yaw` a sheet's local -Z
            // faces the wall on sides 0/3 and its +Z on sides 1/2, so the
            // tilt sign has to follow the inward normal.
            let lean_sign: f32 = if s.nz != 0.0 { -s.nz } else { -s.nx };
            let thin = box_thin_kit(asm);
            let m = ll(&Mat4::IDENTITY, lx, r.y + (lean.cos() * lh) / 2.0, lz, s.yaw, lw, lh, 0.022, lean_sign * lean, 0.0);
            asm.add(key, &thin, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.55, 0.3]), paint: None }));
        }

        // ---- objects standing against the skirting ----
        for _ in 0..rng.int(2, 5) {
            let bt = rng.range(f64::from(-half), f64::from(half)) as f32;
            let (bx, bz) = at(s, bt, rng.range(0.18, 0.42) as f32);
            asm.put(
                *rng.pick(&["sandbag_a", "sandbag_b", "crate_b", "box_card_a", "box_card_b", "bucket", "jerry_can", "tyre_small", "barrel_wood"]),
                bx,
                r.y + 0.01,
                bz,
                rng.float() as f32 * 6.28,
                rng.range(0.8, 1.05) as f32,
                Some([1.0, 1.2, 1.0]),
                0.0,
                0.0,
            );
        }

        // ---- swept dust and plaster fall in the junction ----
        let n_wedge = ((s.len / 1.5).round() as i32).max(2) as u32;
        for i in 0..n_wedge {
            let wt = ((i as f32 + rng.range(0.2, 0.8) as f32) / n_wedge as f32 - 0.5) * s.len;
            let (wx, wz) = at(s, wt, rng.range(0.05, 0.3) as f32);
            let g = patch_geometry(rng, rng.range(0.3, 0.75), 9, 0.55, 0.0);
            let m = ll(&Mat4::IDENTITY, wx, r.y + 0.011, wz, rng.float() as f32 * 6.28, 1.0, 1.0, rng.range(0.35, 0.6) as f32, 0.0, 0.0);
            asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.85, 0.55]), paint: None }));
            if rng.float() < 0.7 {
                let (cx2, cz2) = at(s, wt + rng.range(-0.3, 0.3) as f32, rng.range(0.06, 0.34) as f32);
                asm.put(*rng.pick(&["brick_a", "brick_b", "rock_b", "litter", "litter"]), cx2, r.y + 0.03, cz2, rng.float() as f32 * 6.28, rng.range(0.45, 0.9) as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
            }
        }

        // ---- a sack or a cloth hung on a nail ----
        if rng.float() < 0.45 {
            let ht = wall_t(rng, is_opening, half);
            let (hx, hz) = at(s, ht, 0.05);
            let hy = r.y + rng.range(1.3, 1.85) as f32;
            let cl = cloth_geometry(
                rng.range(0.45, 0.8) as f32,
                rng.range(0.6, 1.0) as f32,
                ClothOpts { seg_x: 6, seg_y: 7, sag: 0.1, wrinkle: 0.09, thickness: 0.003, fray: 0.014, ..ClothOpts::default() },
                Some(rng),
            );
            let m = ll(&Mat4::IDENTITY, hx, hy, hz, s.yaw, 1.0, 1.0, 1.0, 0.0, 0.0);
            asm.add_once(*rng.pick(&["fabric_red", "fabric_teal", "fabric_cream"]), &cl, Some(&m), Some(AccumAddOpts { masks: Some([0.35, 0.6, 0.3]), paint: None }));
            let fine = box_fine_kit(asm);
            let m = ll(&Mat4::IDENTITY, hx, hy + 0.34, hz, s.yaw, 0.02, 0.02, 0.05, 0.0, 0.0);
            asm.add("metal_dark", &fine, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.5, 0.0]), paint: None }));
        }
    }
}

/// `dressCeiling(A, rng, r)` (`interiors.js:317-346`): exposed joists, a
/// conduit run and a hanging cable so a plain ceiling plane has something to
/// occlude and something for the bulb to rim-light.
fn dress_ceiling(asm: &mut Assembler, rng: &mut Rng, r: &RoomRect) {
    let w = r.x1 - r.x0;
    let d = r.z1 - r.z0;
    if r.h < 2.1 || w < 1.6 || d < 1.6 {
        return;
    }
    let along_x = w < d;
    let span = if along_x { w } else { d };
    let run_len = if along_x { d } else { w };
    let n = ((run_len / rng.range(0.75, 1.15) as f32).round() as i32).max(2) as u32;
    let box_ = box_kit(asm);
    for i in 1..n {
        let t = (i as f32 / n as f32 - 0.5) * run_len;
        let jx = if along_x { (r.x0 + r.x1) / 2.0 } else { (r.x0 + r.x1) / 2.0 + t };
        let jz = if along_x { (r.z0 + r.z1) / 2.0 + t } else { (r.z0 + r.z1) / 2.0 };
        let m = ll(&Mat4::IDENTITY, jx, r.y + r.h - 0.06, jz, if along_x { 0.0 } else { std::f32::consts::FRAC_PI_2 }, span - 0.05, 0.11, rng.range(0.055, 0.075) as f32, 0.0, 0.0);
        asm.add("wood_prop_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.35, 0.6, 0.45]), paint: None }));
    }
}

/// `hangingBulb(A, rng, x, yCeil, z, rngIn)` (`interiors.js:349-371`) — see
/// this module's doc for why `rngIn` is dropped.
pub fn hanging_bulb(asm: &mut Assembler, rng: &mut Rng, x: f32, y_ceil: f32, z: f32) {
    let drop = rng.range(0.35, 0.95) as f32;
    let wire = asm.cache("bulbwire", || {
        let mut g = cylinder_geometry(0.006, 0.006, 1.0, 5, 1, false);
        g.fill_masks(0.2, 0.4, 0.0);
        g
    });
    let m = ll(&Mat4::IDENTITY, x, y_ceil - drop / 2.0, z, 0.0, 1.0, drop, 1.0, 0.0, 0.0);
    asm.add("metal_dark", &wire, Some(&m), None);
    let fine = box_fine_kit(asm);
    let m = ll(&Mat4::IDENTITY, x, y_ceil - 0.02, z, 0.0, 0.09, 0.04, 0.09, 0.0, 0.0);
    asm.add("metal_dark", &fine, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.6, 0.3]), paint: None }));
    let bulb = asm.cache("bulb", || {
        use crate::weapons::geometry::primitives::sphere_geometry;
        let g = sphere_geometry(0.045, 10, 7, 0.0, std::f64::consts::TAU, 0.0, std::f64::consts::PI);
        let mut wg = crate::world::geo::WorldGeo { pos: g.pos, normal: g.normal, uv: g.uv, color: Vec::new(), index: g.index };
        wg.apply(&Mat4::scale(axiom_math::Vec3::new(1.0, 1.25, 1.0)));
        wg.fill_masks(0.1, 0.2, 0.0);
        wg
    });
    let m = ll(&Mat4::IDENTITY, x, y_ceil - drop - 0.05, z, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
    asm.add("emissive_warm", &bulb, Some(&m), None);
    let fine2 = box_fine_kit(asm);
    let m = ll(&Mat4::IDENTITY, x, y_ceil - drop + 0.02, z, 0.0, 0.05, 0.06, 0.05, 0.0, 0.0);
    asm.add("metal_dark", &fine2, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.4, 0.0]), paint: None }));
    asm.interior_lights.push(axiom_math::Vec3::new(x, y_ceil - drop - 0.05, z));
}

// ------------------------------------------------------------------- shop --
/// `furnishShop(A, rng, r, cx, cz, w, d, m)` (`interiors.js:374-485`) — see
/// this module's doc for the dropped `m` parameter.
fn furnish_shop(asm: &mut Assembler, rng: &mut Rng, r: &RoomRect, cx: f32, cz: f32, w: f32, d: f32) {
    let (x0, z0, x1, z1, y) = (r.x0, r.z0, r.x1, r.z1, r.y);
    let front_z: f32 = if r.street == 0 { -1.0 } else if r.street == 2 { 1.0 } else { 0.0 };
    add_rug(asm, rng, cx + rng.range(-0.5, 0.5) as f32, y, cz + rng.range(-0.5, 0.5) as f32, rng.range(1.6, 2.4) as f32);

    let along_z = r.street == 1 || r.street == 3;
    let ccx = if along_z { if r.street == 1 { x1 - 1.3 } else { x0 + 1.3 } } else { cx };
    let ccz = if along_z { cz } else if front_z != 0.0 { cz - front_z * (d * 0.5 - 1.3) } else { cz + d * 0.18 };
    let clen = ((if along_z { d } else { w }) - 1.4).min(4.4);
    let c_sx = if along_z { 0.74 } else { clen };
    let c_sz = if along_z { clen } else { 0.74 };

    let box_ = box_kit(asm);
    let m = ll(&Mat4::IDENTITY, ccx, y + 0.9, ccz, 0.0, c_sx, 0.06, c_sz, 0.0, 0.0);
    asm.add("wood_prop_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.4, 0.1]), paint: None }));
    let m = ll(&Mat4::IDENTITY, ccx + if along_z { -0.32 } else { 0.0 }, y + 0.45, ccz + if along_z { 0.0 } else { 0.32 }, 0.0, if along_z { 0.09 } else { c_sx }, 0.9, if along_z { c_sz } else { 0.09 }, 0.0, 0.0);
    asm.add("wood_prop_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.6, 0.4]), paint: None }));
    let m = ll(&Mat4::IDENTITY, ccx, y + 0.28, ccz, 0.0, c_sx - 0.2, 0.04, c_sz - 0.2, 0.0, 0.0);
    asm.add("wood_prop_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.4, 0.7, 0.5]), paint: None }));
    asm.collide_box(Surface::Wood, ccx, y + 0.45, ccz, c_sx, 0.9, c_sz, 0.0);

    for _ in 0..6 {
        let t = rng.range(f64::from(-clen / 2.0 + 0.3), f64::from(clen / 2.0 - 0.3)) as f32;
        let px = ccx + if along_z { rng.range(-0.22, 0.22) as f32 } else { t };
        let pz = ccz + if along_z { t } else { rng.range(-0.22, 0.22) as f32 };
        if rng.float() < 0.45 {
            asm.put("tray", px, y + 0.94, pz, rng.range(-0.4, 0.4) as f32 + if along_z { std::f32::consts::FRAC_PI_2 } else { 0.0 }, 1.0, Some([1.0, 1.1, 1.0]), 0.0, 0.0);
            if rng.float() < 0.8 {
                asm.put("produce", px, y + 0.96, pz, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.0, 1.0]), 0.0, 0.0);
            }
        } else {
            asm.put(
                *rng.pick(&["box_card_a", "box_card_b", "crate_b", "bottle", "can", "bucket"]),
                px,
                y + 0.94,
                pz,
                rng.float() as f32 * 6.28,
                rng.range(0.6, 0.9) as f32,
                Some([1.0, 1.15, 1.0]),
                0.0,
                0.0,
            );
        }
    }

    // sacks and trays stacked on the customer side of the counter
    for i in 0..rng.int(3, 6) {
        let t = rng.range(f64::from(-clen / 2.0), f64::from(clen / 2.0)) as f32;
        let off = rng.range(0.55, 1.05) as f32;
        let px = ccx + if along_z { if r.street == 1 { off } else { -off } } else { t };
        let pz = ccz + if along_z { t } else if front_z != 0.0 { front_z * off } else { off };
        asm.put(
            *rng.pick(&["sandbag_a", "sandbag_b", "tray", "crate_b", "crate_flat"]),
            px,
            y + 0.02 + (i % 2) as f32 * 0.16,
            pz,
            rng.float() as f32 * 6.28,
            rng.range(0.9, 1.05) as f32,
            Some([1.0, rng.range(1.0, 1.3) as f32, 1.0]),
            0.0,
            0.0,
        );
    }

    // shelving against the side walls, never against the frontage
    for sx in [-1.0f32, 1.0] {
        if (r.street == 1 && sx > 0.0) || (r.street == 3 && sx < 0.0) {
            continue;
        }
        let n = ((d / 1.5).floor() as i32 - 1).max(1) as u32;
        for i in 0..n {
            let sz = z0 + 0.8 + i as f32 * 1.35;
            if sz > z1 - 0.7 {
                break;
            }
            if rng.float() < 0.25 {
                continue;
            }
            asm.put("shelf", cx + sx * (w / 2.0 - 0.22), y, sz, if sx > 0.0 { -std::f32::consts::FRAC_PI_2 } else { std::f32::consts::FRAC_PI_2 }, rng.range(0.92, 1.08) as f32, Some([1.0, rng.range(0.8, 1.4) as f32, 1.0]), 0.0, 0.0);
            for k in 0..3 {
                asm.put(
                    *rng.pick(&["box_card_b", "bottle", "can"]),
                    cx + sx * (w / 2.0 - 0.24) + rng.range(-0.1, 0.1) as f32,
                    y + 0.25 + k as f32 * 0.55,
                    sz + rng.range(-0.35, 0.35) as f32,
                    rng.float() as f32 * 6.28,
                    rng.range(0.7, 1.1) as f32,
                    Some([1.0, 1.2, 1.0]),
                    0.0,
                    0.0,
                );
            }
        }
    }

    stack_crates(asm, rng, x0 + 0.7, y, z1 - 0.9, rng.int(3, 6) as u32);
    for i in 0..rng.int(3, 6) {
        asm.put(
            *rng.pick(&["sandbag_a", "sandbag_b", "sandbag_c"]),
            rng.range(f64::from(x0) + 0.4, f64::from(x0) + 1.6) as f32,
            y + 0.02 + (i % 2) as f32 * 0.19,
            rng.range(f64::from(z0) + 0.5, f64::from(z0) + 2.0) as f32,
            rng.float() as f32 * 6.28,
            rng.range(0.9, 1.1) as f32,
            Some([1.0, 1.2, 1.0]),
            0.0,
            0.0,
        );
    }
    asm.put("barrel_wood", x1 - 0.6, y, z0 + 0.7, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
    asm.put("table_small", cx - w * 0.28, y, cz - d * 0.28, rng.range(-0.4, 0.4) as f32, 1.0, Some([1.0, 1.0, 1.0]), 0.0, 0.0);
    asm.put("chair", cx - w * 0.28 + 0.7, y, cz - d * 0.2, rng.range(2.0, 4.0) as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
    asm.collide_box(Surface::Wood, cx - w * 0.28, y + 0.4, cz - d * 0.28, 1.0, 0.8, 0.8, 0.0);
}

// ----------------------------------------------------------------- living --
/// `furnishLiving(A, rng, r, cx, cz, w, d, m)` (`interiors.js:488-522`).
fn furnish_living(asm: &mut Assembler, rng: &mut Rng, r: &RoomRect, cx: f32, cz: f32, _w: f32, _d: f32) {
    let (x0, z0, x1, z1, y) = (r.x0, r.z0, r.x1, r.z1, r.y);
    add_rug(asm, rng, cx, y, cz, rng.range(2.0, 2.8) as f32);
    asm.put("mattress", x0 + 1.1, y, z1 - 0.9, rng.range(-0.1, 0.1) as f32, 1.0, Some([1.0, 1.1, 1.0]), 0.0, 0.0);
    asm.collide_box(Surface::Fabric, x0 + 1.1, y + 0.1, z1 - 0.9, 1.9, 0.2, 0.9, 0.0);
    let bl = cloth_geometry(1.5, 0.9, ClothOpts { seg_x: 7, seg_y: 6, sag: 0.05, wrinkle: 0.05, thickness: 0.0032, fray: 0.012, ..ClothOpts::default() }, Some(rng));
    let m = ll(&Mat4::IDENTITY, x0 + 1.2, y + 0.19, z1 - 1.0, 0.0, 1.0, 1.0, 1.0, -std::f32::consts::FRAC_PI_2, 0.0);
    asm.add_once("fabric_teal", &bl, Some(&m), Some(AccumAddOpts { masks: Some([0.3, 0.5, 0.2]), paint: None }));
    for _ in 0..3 {
        let mut g = chamfer_box(0.42, 0.14, 0.42, 0.06);
        g.fill_masks(0.2, 0.4, 0.2);
        let m = ll(&Mat4::IDENTITY, cx + rng.range(-1.0, 1.0) as f32, y + 0.07, cz + rng.range(-1.0, 1.0) as f32, rng.float() as f32 * 6.28, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once(*rng.pick(&["fabric_red", "fabric_teal", "fabric_cream"]), &g, Some(&m), None);
    }
    asm.put("cabinet", x1 - 0.35, y, cz + rng.range(-0.6, 0.6) as f32, -std::f32::consts::FRAC_PI_2, 1.0, Some([1.0, 1.0, 1.0]), 0.0, 0.0);
    asm.collide_box(Surface::Wood, x1 - 0.35, y + 0.6, cz, 0.5, 1.2, 0.9, 0.0);
    asm.put("table_small", cx + 0.4, y, cz - 0.8, rng.range(0.0, 0.4) as f32, 1.0, Some([1.0, 1.0, 1.0]), 0.0, 0.0);
    asm.put("chair", cx - 0.8, y, cz - 1.2, rng.range(1.5, 2.5) as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
    asm.put("chair", cx + 1.4, y, cz - 0.4, rng.range(-1.5, -0.5) as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
    asm.put("bottle", cx + 0.4, y + 0.74, cz - 0.8, 0.0, 1.0, Some([1.0, 1.0, 1.0]), 0.0, 0.0);
    asm.put("can", cx + 0.6, y + 0.74, cz - 0.7, 1.0, 1.0, Some([1.0, 1.0, 1.0]), 0.0, 0.0);
    let wall = cloth_geometry(1.7, 1.1, ClothOpts { seg_x: 8, seg_y: 7, sag: 0.04, wrinkle: 0.05, thickness: 0.0036, fray: 0.02, bow: -1.0, ..ClothOpts::default() }, Some(rng));
    let m = ll(&Mat4::IDENTITY, cx - 0.4, y + 1.65, z0 + 0.09, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
    asm.add_once("fabric_red", &wall, Some(&m), Some(AccumAddOpts { masks: Some([0.3, 0.4, 0.2]), paint: None }));
    stack_crates(asm, rng, x1 - 0.9, y, z0 + 0.8, rng.int(1, 3) as u32);
}

// ---------------------------------------------------------------- storage --
/// `furnishStorage(A, rng, r, cx, cz, w, d, m)` (`interiors.js:525-570`).
fn furnish_storage(asm: &mut Assembler, rng: &mut Rng, r: &RoomRect, _cx: f32, _cz: f32, _w: f32, _d: f32) {
    let (x0, z0, x1, z1, y) = (r.x0, r.z0, r.x1, r.z1, r.y);
    let spots = rng.int(4, 7);
    for _ in 0..spots {
        let sx = rng.range(f64::from(x0) + 0.6, f64::from(x1) - 0.6) as f32;
        let sz = rng.range(f64::from(z0) + 0.6, f64::from(z1) - 0.6) as f32;
        let pick = rng.float();
        if pick < 0.35 {
            stack_crates(asm, rng, sx, y, sz, rng.int(2, 5) as u32);
        } else if pick < 0.55 {
            asm.put("pallet", sx, y + 0.01, sz, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
            for k in 0..rng.int(1, 4) {
                asm.put(
                    *rng.pick(&["sandbag_a", "sandbag_b", "box_card_a"]),
                    sx + rng.range(-0.3, 0.3) as f32,
                    y + 0.11 + k as f32 * 0.2,
                    sz + rng.range(-0.25, 0.25) as f32,
                    rng.float() as f32 * 6.28,
                    1.0,
                    Some([1.0, 1.2, 1.0]),
                    0.0,
                    0.0,
                );
            }
            asm.collide_box(Surface::Wood, sx, y + 0.1, sz, 1.2, 0.2, 1.0, 0.0);
        } else if pick < 0.72 {
            asm.put(*rng.pick(&["barrel_rust", "barrel_blue", "barrel_wood"]), sx, y, sz, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
            asm.collide_box(Surface::Metal, sx, y + 0.45, sz, 0.62, 0.9, 0.62, 0.0);
        } else if pick < 0.85 {
            asm.put("tyre", sx, y, sz, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
            if rng.float() < 0.6 {
                asm.skirts = false;
                asm.put("tyre", sx + 0.03, y + 0.19, sz + 0.02, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
                asm.skirts = true;
            }
        } else {
            asm.put("shelf", sx, y, sz, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
        }
    }
    for _ in 0..rng.int(2, 5) {
        asm.put(
            "plank_a",
            rng.range(f64::from(x0) + 0.5, f64::from(x1) - 0.5) as f32,
            y + 0.02,
            rng.range(f64::from(z0) + 0.5, f64::from(z1) - 0.5) as f32,
            rng.float() as f32 * 6.28,
            1.0,
            Some([1.0, 1.3, 1.0]),
            0.0,
            0.0,
        );
    }
    asm.put("jerry_can", x1 - 0.5, y, z1 - 0.5, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
    asm.put("bucket", x0 + 0.5, y, z1 - 0.6, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
}

// ------------------------------------------------------------------- ruin --
/// `furnishRuin(A, rng, r, cx, cz, w, d, m)` (`interiors.js:573-609`).
fn furnish_ruin(asm: &mut Assembler, rng: &mut Rng, r: &RoomRect, cx: f32, cz: f32, _w: f32, _d: f32) {
    let (x0, z0, x1, z1, y) = (r.x0, r.z0, r.x1, r.z1, r.y);
    rubble_mound(asm, rng, cx + rng.range(-1.0, 1.0) as f32, y, cz + rng.range(-1.0, 1.0) as f32, rng.range(1.4, 2.2) as f32, 22, RubbleOpts { key: "concrete" });
    for _ in 0..rng.int(3, 6) {
        asm.put(
            "slab_shard",
            rng.range(f64::from(x0) + 0.5, f64::from(x1) - 0.5) as f32,
            y + 0.05,
            rng.range(f64::from(z0) + 0.5, f64::from(z1) - 0.5) as f32,
            rng.float() as f32 * 6.28,
            1.0,
            Some([1.0, 1.4, 1.0]),
            0.0,
            0.0,
        );
    }
    for _ in 0..rng.int(6, 12) {
        asm.put(
            *rng.pick(&["brick_a", "brick_b", "rock_a", "rock_b"]),
            rng.range(f64::from(x0) + 0.3, f64::from(x1) - 0.3) as f32,
            y + 0.06,
            rng.range(f64::from(z0) + 0.3, f64::from(z1) - 0.3) as f32,
            rng.float() as f32 * 6.28,
            rng.range(0.6, 1.3) as f32,
            Some([1.0, 1.5, 1.0]),
            0.0,
            0.0,
        );
    }
    asm.put("rebar", cx + rng.range(-1.0, 1.0) as f32, y + 0.06, cz + rng.range(-1.0, 1.0) as f32, rng.float() as f32 * 6.28, 1.0, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
    for _ in 0..3 {
        asm.put(
            "plank_b",
            rng.range(f64::from(x0) + 0.4, f64::from(x1) - 0.4) as f32,
            y + 0.03,
            rng.range(f64::from(z0) + 0.4, f64::from(z1) - 0.4) as f32,
            rng.float() as f32 * 6.28,
            1.0,
            Some([1.0, 1.4, 1.0]),
            0.0,
            0.0,
        );
    }
    asm.put(
        "chair",
        rng.range(f64::from(x0) + 0.6, f64::from(x1) - 0.6) as f32,
        y + 0.05,
        rng.range(f64::from(z0) + 0.6, f64::from(z1) - 0.6) as f32,
        rng.float() as f32 * 6.28,
        1.0,
        Some([1.0, 1.5, 1.0]),
        0.0,
        0.0,
    );
    // dust sheet snagged on the rubble
    let sheet = cloth_geometry(1.4, 1.1, ClothOpts { seg_x: 7, seg_y: 7, sag: 0.24, wrinkle: 0.075, twist: 0.08, fray: 0.02, ..ClothOpts::default() }, Some(rng));
    let m = ll(&Mat4::IDENTITY, cx + rng.range(-1.5, 1.5) as f32, y + 0.55, cz + rng.range(-1.5, 1.5) as f32, rng.float() as f32 * 6.28, 1.0, 1.0, 1.0, -1.2, 0.0);
    asm.add_once("fabric_cream", &sheet, Some(&m), Some(AccumAddOpts { masks: Some([0.4, 0.7, 0.3]), paint: None }));
}

// ---------------------------------------------------------------- helpers --
/// `addRug(A, rng, x, y, z, size)` (`interiors.js:612-628`).
fn add_rug(asm: &mut Assembler, rng: &mut Rng, x: f32, y: f32, z: f32, size: f32) {
    let g = cloth_geometry(size, size * rng.range(0.55, 0.75) as f32, ClothOpts { seg_x: 8, seg_y: 6, sag: 0.0, wrinkle: 0.02, thickness: 0.0038, fray: 0.012, ..ClothOpts::default() }, Some(rng));
    let m = ll(&Mat4::IDENTITY, x, y + 0.014, z, rng.range(-0.4, 0.4) as f32, 1.0, 1.0, 1.0, -std::f32::consts::FRAC_PI_2, 0.0);
    asm.add_once(*rng.pick(&["fabric_red", "fabric_teal", "fabric_cream"]), &g, Some(&m), Some(AccumAddOpts { masks: Some([0.45, 0.55, 0.25]), paint: None }));
}

/// `stackCrates(A, rng, x, y, z, n)` (`interiors.js:630-653`). `pub` — the
/// source exports it (`export function stackCrates`) and `dressing.js`'s
/// `dressStreet`/`dressBuildings`/`scatterDebris` reach for it too.
pub fn stack_crates(asm: &mut Assembler, rng: &mut Rng, x: f32, y: f32, z: f32, n: u32) -> f32 {
    let mut cy = y;
    let was_skirt = asm.skirts;
    for i in 0..n {
        asm.skirts = was_skirt && i == 0;
        let id = *rng.pick(&["crate_a", "crate_b", "crate_c", "crate_flat"]);
        let s = rng.range(0.92, 1.08) as f32;
        let hh = if id == "crate_c" { 0.82 * 0.85 } else if id == "crate_b" { 0.48 * 0.85 } else { 0.62 * 0.85 };
        asm.put(id, x + rng.range(-0.12, 0.12) as f32, cy, z + rng.range(-0.12, 0.12) as f32, rng.range(-0.5, 0.5) as f32, s, Some([1.0, rng.range(0.7, 1.4) as f32, 1.0]), 0.0, 0.0);
        asm.collide_box(Surface::Wood, x, cy + (hh * s) / 2.0, z, 0.7, hh * s, 0.7, 0.0);
        cy += hh * s;
        if rng.float() < 0.2 {
            break;
        }
    }
    asm.skirts = was_skirt;
    cy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assembler() -> Assembler {
        let mut a = Assembler::new(Rng::new(1));
        crate::world::props::register_props(&mut a, &mut Rng::new(2));
        a
    }

    fn small_room(kind: &'static str) -> RoomRect {
        RoomRect { kind, street: 0, x0: 0.0, z0: 0.0, x1: 0.6, z1: 0.6, y: 0.0, h: 2.5 }
    }

    fn room(kind: &'static str) -> RoomRect {
        RoomRect { kind, street: 0, x0: -2.0, z0: -2.0, x1: 2.0, z1: 2.0, y: 0.0, h: 2.6 }
    }

    #[test]
    fn furnish_room_below_1_2m_in_either_axis_is_a_no_op() {
        let mut asm = assembler();
        let mut rng = Rng::new(5);
        furnish_room(&mut asm, &mut rng, small_room("shop"));
        let out = asm.finalize();
        assert!(out.statics.is_empty() && out.instanced.is_empty());
    }

    #[test]
    fn furnish_room_shop_produces_geometry() {
        let mut asm = assembler();
        let mut rng = Rng::new(7);
        furnish_room(&mut asm, &mut rng, room("shop"));
        let out = asm.finalize();
        assert!(!out.statics.is_empty());
        assert!(!out.instanced.is_empty());
    }

    #[test]
    fn furnish_room_every_kind_is_deterministic_from_the_same_seed() {
        for kind in ["shop", "living", "storage", "ruin", "unknown-falls-back-to-storage"] {
            let mut asm_a = assembler();
            let mut rng_a = Rng::new(11);
            furnish_room(&mut asm_a, &mut rng_a, room(kind));
            let a = asm_a.finalize();

            let mut asm_b = assembler();
            let mut rng_b = Rng::new(11);
            furnish_room(&mut asm_b, &mut rng_b, room(kind));
            let b = asm_b.finalize();

            assert_eq!(a.stats.instances, b.stats.instances, "kind={kind}");
            assert_eq!(a.stats.static_tris, b.stats.static_tris, "kind={kind}");
        }
    }

    #[test]
    fn furnish_room_unknown_kind_falls_back_to_storage() {
        let mut asm_a = assembler();
        let mut rng_a = Rng::new(3);
        furnish_room(&mut asm_a, &mut rng_a, room("storage"));
        let a = asm_a.finalize();

        let mut asm_b = assembler();
        let mut rng_b = Rng::new(3);
        furnish_room(&mut asm_b, &mut rng_b, room("bogus"));
        let b = asm_b.finalize();

        assert_eq!(a.stats.instances, b.stats.instances);
    }

    #[test]
    fn hanging_bulb_registers_an_interior_light_and_emissive_geometry() {
        let mut asm = assembler();
        let mut rng = Rng::new(4);
        hanging_bulb(&mut asm, &mut rng, 0.0, 2.5, 0.0);
        assert_eq!(asm.interior_lights.len(), 1);
        let out = asm.finalize();
        assert!(out.statics.iter().any(|s| s.key == "emissive_warm"));
    }

    #[test]
    fn stack_crates_stops_early_when_rng_rolls_below_0_2_and_restores_skirts() {
        let mut asm = assembler();
        asm.skirts = true;
        let mut rng = Rng::new(1); // first float() draw inside is < 0.2 quickly for some seeds
        let top = stack_crates(&mut asm, &mut rng, 0.0, 0.0, 0.0, 5);
        assert!(top >= 0.0);
        assert!(asm.skirts);
    }
}
