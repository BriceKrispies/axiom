//! Ported from Claude-of-Duty `src/world/dressing.js:206-708` —
//! `streetFloor`, the bottom third of every wide shot.
//!
//! A street is not a plane with a few crates on it: it is a floor with mass —
//! sand and swept rubble banked against every wall base, masonry spilling off
//! the kerb, polished ruts down the driving line, and enough at eye level in
//! the 10-30 m band to give the alley depth. The berms do double duty: they
//! bury the hard geometric line where wall meets ground, which otherwise
//! reads as a Z-fighting seam in every establishing shot.

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{box_thin_kit, cloth_geometry, ll, patch_geometry, rubble_mound, ClothOpts, RubbleOpts};
use crate::world::layout::{BUILDINGS, STREET};
use crate::world::palette::Surface;

use super::berm::{drift_berm, DRIFT_BERM_DEFAULT_NZ};
use super::int_loop_continues;
use super::occupancy::{cam_clear, ground_skirt, ground_y, is_open, js_sign, SkirtOpts};
use super::tyres::tyre_stack;

/// The masonry vocabulary that sits IN a drift, half buried
/// (`dressing.js:462`).
const IN_DRIFT: [&str; 8] = ["brick_a", "brick_b", "cinder", "rock_a", "rock_b", "slab_shard", "litter", "can"];

/// The kerb-spill vocabulary (`dressing.js:539`).
const SPILL: [&str; 7] = ["slab_shard", "brick_a", "brick_b", "cinder", "rock_a", "rebar", "plank_b"];

/// Debris round the stalled saloon (`dressing.js:589`).
const CAR_DEBRIS: [&str; 6] = ["glass_shards", "brick_b", "rock_b", "litter", "can", "slab_shard"];

/// `DRUM_MIX` (`dressing.js:600-604`).
const DRUM_MIX: [[&str; 3]; 3] = [
    ["barrel_rust", "barrel_rust", "barrel_blue"],
    ["barrel_blue", "barrel_rust", "barrel_wood"],
    ["barrel_rust", "barrel_wood", "barrel_rust"],
];

/// The tarp keys thrown over a drum cluster (`dressing.js:663`).
const TARP: [&str; 3] = ["fabric_teal", "fabric_cream", "burlap"];

/// Eight proper spill mounds where a parapet or a balcony came down
/// (`dressing.js:552-561`).
const SPILL_MOUNDS: [[f64; 2]; 8] = [
    [-5.4, 16.5],
    [5.5, 11.0],
    [-5.6, 2.0],
    [5.6, -4.0],
    [-5.5, -13.5],
    [5.4, -19.0],
    [-5.3, -25.5],
    [5.5, -33.0],
];

/// `streetFloor(A, rng)` (`dressing.js:216-708`).
pub fn street_floor(asm: &mut Assembler, rng: &mut Rng) {
    let hw = STREET.half_width;
    let kb = STREET.kerb;
    let wh = STREET.walk_h;
    let z_min = STREET.z_min;
    let z_max = STREET.z_max;

    wall_to_ground_junction(asm, rng, kb, wh, z_min, z_max);
    drift_berms(asm, rng, kb, wh, z_min, z_max);
    kerb_line(asm, rng, hw, z_min, z_max);
    tyre_tracks(asm, rng, hw, z_min, z_max);
    masonry_spill(asm, rng, kb, z_min, z_max);
    silhouette_breakers(asm, rng);
}

// ---- 0. the wall-to-ground junction ----
/// `dressing.js:227-268`. A facade that meets the pavement on a ruled line is
/// the tell that says "two boxes intersecting". Drawn on the outer face of
/// the building's PLINTH, in the plinth's own material with the grime mask
/// pinned high.
fn wall_to_ground_junction(asm: &mut Assembler, rng: &mut Rng, kb: f64, wh: f64, z_min: f64, z_max: f64) {
    for side in [-1.0f64, 1.0] {
        let mut z = z_min;
        while z < z_max {
            let seg = rng.range(0.5, 1.1);
            let cz = z + seg / 2.0;
            let host = BUILDINGS.iter().find(|b| {
                // the facade that faces the street sits at |x| = kerb
                (b.x.abs() - b.w / 2.0 - kb).abs() <= 0.3
                    && js_sign(b.x) == side
                    && cz > b.z - b.d / 2.0 + 0.05
                    && cz < b.z + b.d / 2.0 - 0.05
            });
            if host.is_some() {
                let h = rng.range(0.15, 0.25);
                // the plinth stands 7 cm proud of the facade: stain ITS face,
                // not the render 7 cm behind it
                let px = side * (kb + 0.056);
                let geo = box_thin_kit(asm);
                let m = ll(&Mat4::IDENTITY, px as f32, (wh + h / 2.0 - 0.025) as f32, cz as f32, 0.0, 0.034, h as f32, (seg * 0.99) as f32, 0.0, 0.0);
                // `host.plinthKey ?? 'concrete'`: no `BUILDINGS` entry in the
                // source ever declares `plinthKey`, so this always resolves
                // to `'concrete'` — and `crate::world::layout::Building` has
                // no such field for the same reason.
                asm.add("concrete", &geo, Some(&m), Some(AccumAddOpts { masks: Some([0.0, 1.0, 0.85]), paint: None }));
                // and a low fillet of swept grit in the corner itself
                if rng.float() < 0.75 {
                    let bw = rng.range(0.16, 0.34);
                    let bh = rng.range(0.04, 0.09);
                    let g = drift_berm(rng, seg * 0.95, bw, bh, 3);
                    let ry = if side > 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
                    let m = ll(&Mat4::IDENTITY, (side * (kb - 0.04)) as f32, (wh - 0.012) as f32, cz as f32, ry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
                    asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.95, 0.7]), paint: None }));
                }
            }
            z += seg;
        }
    }
}

// ---- 1. drift berms banked against the building line, both sides ----
/// `dressing.js:271-317`.
fn drift_berms(asm: &mut Assembler, rng: &mut Rng, kb: f64, wh: f64, z_min: f64, z_max: f64) {
    for side in [-1.0f64, 1.0] {
        let mut z = z_min + 1.0;
        while z < z_max - 2.0 {
            let len = rng.range(2.2, 6.5);
            let cz = z + len / 2.0;
            let x = side * (kb - 0.06);
            // Alley mouths and doorways stay clear: a berm across a door
            // reads as a bug. `&&` short-circuits — a closed spot never draws
            // the 0.96 roll.
            if is_open(x - side * 0.5, cz, 0.05) && rng.float() < 0.96 {
                let h = rng.range(0.14, 0.42);
                let w = rng.range(0.6, 1.5);
                let g = drift_berm(rng, len, w, h, DRIFT_BERM_DEFAULT_NZ);
                // ry = -PI/2 for the +X side puts the tall edge against the wall
                let key = if rng.float() < 0.72 { "sand" } else { "road_dust" };
                let ry = if side > 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
                let m = ll(&Mat4::IDENTITY, x as f32, (wh - 0.02) as f32, cz as f32, ry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
                asm.add_once(key, &g, Some(&m), Some(AccumAddOpts { masks: Some([0.15, 0.55, 0.45]), paint: None }));
                // masonry and litter sitting IN the drift, half buried
                let mut i = 0;
                while int_loop_continues(rng, i, 2, 6) {
                    let px = x - side * rng.range(0.05, w * 0.8);
                    let pz = cz + rng.range(-len / 2.0 + 0.2, len / 2.0 - 0.2);
                    let id = *rng.pick(&IN_DRIFT);
                    let py = wh + h * rng.range(0.1, 0.55);
                    let pry = rng.float() * 6.28;
                    let ps = rng.range(0.6, 1.15);
                    let grime = rng.range(1.1, 1.5);
                    let prx = rng.range(-0.25, 0.25);
                    let prz = rng.range(-0.25, 0.25);
                    asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), prx as f32, prz as f32);
                    i += 1;
                }
            }
            z += len + rng.range(0.1, 0.9);
        }
    }
}

// ---- 2. the kerb line: sand spilling off the pavement into the gutter ----
/// `dressing.js:320-329`.
fn kerb_line(asm: &mut Assembler, rng: &mut Rng, hw: f64, z_min: f64, z_max: f64) {
    for _ in 0..70 {
        let side = if rng.float() < 0.5 { -1.0f64 } else { 1.0 };
        let cz = rng.range(z_min + 2.0, z_max - 2.0);
        let len = rng.range(1.2, 3.4);
        if !is_open(side * (hw + 0.4), cz, 0.05) {
            continue;
        }
        let bw = rng.range(0.35, 0.8);
        let bh = rng.range(0.05, 0.14);
        let g = drift_berm(rng, len, bw, bh, 3);
        let ry = if side > 0.0 { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 };
        let m = ll(&Mat4::IDENTITY, (side * (hw + 0.12)) as f32, 0.02, cz as f32, ry as f32, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once("sand", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.15, 0.5, 0.3]), paint: None }));
    }
}

// ---- 3. tyre tracks polished into the dust along the driving line ----
/// `dressing.js:332-395`. Two ruts, laid as long overlapping strips so the
/// line wanders instead of ruling a straight edge down the middle of the
/// frame.
fn tyre_tracks(asm: &mut Assembler, rng: &mut Rng, hw: f64, z_min: f64, z_max: f64) {
    for side in [-1.0f64, 1.0] {
        let mut z = z_min + 2.0;
        while z < z_max - 3.0 {
            let len = rng.range(5.0, 13.0);
            let x = side * rng.range(1.25, 1.95);
            let camber = (1.0 - (x / hw).powi(2)) * 0.055 + 0.038;
            let g = patch_geometry(rng, 0.34, 13, 0.28, 0.0);
            let ry = rng.range(-0.03, 0.03);
            let m = ll(&Mat4::IDENTITY, x as f32, camber as f32, (z + len / 2.0) as f32, ry as f32, 1.0, 1.0, (len / 0.68) as f32, 0.0, 0.0);
            asm.add_once("road_rut", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.55, 0.5, 0.15]), paint: None }));
            // a lighter, wider halo of disturbed dust either side of the
            // polished strip
            if rng.float() < 0.7 {
                let hg = patch_geometry(rng, 0.62, 11, 0.4, 0.0);
                let hry = rng.range(-0.04, 0.04);
                let m = ll(&Mat4::IDENTITY, x as f32, (camber - 0.004) as f32, (z + len / 2.0) as f32, hry as f32, 1.0, 1.0, (len / 1.24) as f32, 0.0, 0.0);
                asm.add_once("road_dust", &hg, Some(&m), Some(AccumAddOpts { masks: Some([0.45, 0.15, 0.08]), paint: None }));
            }
            // the fine dust ridge thrown up between the wheels
            if rng.float() < 0.6 {
                let dg = drift_berm(rng, len * 0.8, 0.3, 0.035, 3);
                let m = ll(
                    &Mat4::IDENTITY,
                    (x - side * 0.42) as f32,
                    (camber + 0.004) as f32,
                    (z + len / 2.0) as f32,
                    std::f32::consts::FRAC_PI_2,
                    1.0,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                );
                asm.add_once("road_dust", &dg, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.4, 0.2]), paint: None }));
            }
            z += len + rng.range(0.5, 4.0);
        }
    }
    // a couple of turning scuffs where vehicles have swung across the road
    for _ in 0..8 {
        let z = rng.range(z_min + 5.0, z_max - 5.0);
        let radius = rng.range(0.5, 1.1);
        let g = patch_geometry(rng, radius, 12, 0.5, 0.0);
        let x = rng.range(-hw + 0.6, hw - 0.6);
        let ry = rng.float() * 3.14;
        let sz = rng.range(1.4, 2.6);
        let m = ll(&Mat4::IDENTITY, x as f32, ((1.0 - (x / hw).powi(2)) * 0.055 + 0.04) as f32, z as f32, ry as f32, 1.0, 1.0, sz as f32, 0.0, 0.0);
        asm.add_once("asphalt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.45, 0.4, 0.15]), paint: None }));
    }
}

// ---- 4. masonry spill: chunks that fell off the buildings onto the kerb ----
/// `dressing.js:398-424` (loose chunks) and `:425-450` (eight spill mounds).
fn masonry_spill(asm: &mut Assembler, rng: &mut Rng, kb: f64, z_min: f64, z_max: f64) {
    for _ in 0..120 {
        let side = if rng.float() < 0.5 { -1.0f64 } else { 1.0 };
        let z = rng.range(z_min + 1.0, z_max - 1.0);
        let x = side * (kb - rng.gauss().abs() * 1.5 - 0.1);
        if !is_open(x, z, 0.05) {
            continue;
        }
        let y = ground_y(x, z);
        let id = *rng.pick(&SPILL);
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.7, 1.35);
        let grime = rng.range(1.0, 1.5);
        let prx = rng.range(-0.3, 0.3);
        let prz = rng.range(-0.3, 0.3);
        asm.put(id, x as f32, (y + 0.02) as f32, z as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), prx as f32, prz as f32);
    }
    for [x, z] in SPILL_MOUNDS {
        // `||` short-circuits: an off-map spot never runs `camClear`.
        if !is_open(x, z, 0.1) || !cam_clear(x, z, 1.8) {
            continue;
        }
        let radius = rng.range(1.1, 1.9);
        let count = rng.int(18, 30);
        rubble_mound(asm, rng, x as f32, ground_y(x, z) as f32, z as f32, radius as f32, count as u32, RubbleOpts { key: "concrete_prop" });
    }
}

// ---- 5. silhouette breakers at eye level in the 10-30 m mid-ground ----
/// `dressing.js:453-708`. A stalled saloon, three drum clusters, a tyre pile
/// and a pallet stack: mass between the camera and the terminator, so the
/// alley has depth cues rather than an empty floor and a wall at the end.
fn silhouette_breakers(asm: &mut Assembler, rng: &mut Rng) {
    stalled_saloon(asm, rng);
    drum_clusters(asm, rng);
    tyre_and_pallet_stacks(asm, rng);
}

/// `dressing.js:459-497`.
fn stalled_saloon(asm: &mut Assembler, rng: &mut Rng) {
    let car = [-3.35f64, -6.2, 0.28];
    if !cam_clear(car[0], car[1], 2.6) {
        return;
    }
    let y = ground_y(car[0], car[1]);
    asm.put("wreck", car[0] as f32, (y + 0.02) as f32, car[1] as f32, car[2] as f32, 1.0, Some([1.0, 0.85, 1.0]), 0.0, 0.0);
    asm.collide_box(Surface::Metal, car[0] as f32, (y + 0.75) as f32, car[1] as f32, 1.85, 1.5, 4.4, car[2] as f32);
    // it has been sitting long enough to gather its own drift and shed a wheel
    let dg = drift_berm(rng, 4.2, 0.7, 0.13, 3);
    let m = ll(
        &Mat4::IDENTITY,
        (car[0] - 1.0) as f32,
        (y + 0.005) as f32,
        car[1] as f32,
        (car[2] + std::f64::consts::FRAC_PI_2) as f32,
        1.0,
        1.0,
        1.0,
        0.0,
        0.0,
    );
    asm.add_once("sand", &dg, Some(&m), Some(AccumAddOpts { masks: Some([0.15, 0.6, 0.5]), paint: None }));
    asm.skirts = false;
    asm.put("tyre", (car[0] + 1.5) as f32, (y + 0.1) as f32, (car[1] - 1.8) as f32, 1.1, 1.0, Some([1.0, 1.4, 1.0]), 1.5, 0.2);
    asm.skirts = true;
    for _ in 0..12 {
        let px = car[0] + rng.range(-2.2, 2.2);
        let pz = car[1] + rng.range(-3.0, 3.0);
        if !is_open(px, pz, 0.1) {
            continue;
        }
        let id = *rng.pick(&CAR_DEBRIS);
        let py = ground_y(px, pz) + 0.015;
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.6, 1.2);
        asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
    }
}

/// `dressing.js:499-671`. Same module in three places, so each one gets its
/// own barrel mix, ring radius, damage level and a different piece of
/// dressing on top — otherwise the eye recognises the arrangement.
fn drum_clusters(asm: &mut Assembler, rng: &mut Rng) {
    let mut cluster: usize = 0;
    for [dx, dz, n] in [[-5.1f64, -2.0, 5.0], [4.9, -11.5, 4.0], [4.75, 6.2, 3.0]] {
        let n = n as i32;
        let mix = DRUM_MIX[cluster % DRUM_MIX.len()];
        let spread = [0.62f64, 0.8, 0.5][cluster % 3];
        let lying_p = [0.28f64, 0.1, 0.45][cluster % 3];
        // Drawn BEFORE the `camClear` bail and before the counter bumps, so a
        // skipped cluster still consumes one draw.
        let phase = rng.float() * 6.28;
        cluster += 1;
        if !cam_clear(dx, dz, 1.4) {
            continue;
        }
        let mut tallest: Option<[f64; 3]> = None;
        for i in 0..n {
            let a = phase + (f64::from(i) / f64::from(n)) * 6.28 + rng.range(-0.5, 0.5);
            let r = if i == 0 { 0.0 } else { rng.range(spread * 0.85, spread * 1.4) };
            let px = dx + a.cos() * r;
            let pz = dz + a.sin() * r;
            if !is_open(px, pz, 0.2) {
                continue;
            }
            // `i > 0 && rng.float() < lyingP` short-circuits: the centre
            // barrel never draws the roll.
            let lying = i > 0 && rng.float() < lying_p;
            let y = ground_y(px, pz);
            let id = *rng.pick(&mix);
            let pry = rng.float() * 6.28;
            let grime = rng.range(1.1, 1.5);
            let (prx, prz) = if lying { (std::f64::consts::FRAC_PI_2, 0.0) } else { (0.0, rng.range(-0.03, 0.03)) };
            asm.put(
                id,
                px as f32,
                (y + if lying { 0.3 } else { 0.0 }) as f32,
                pz as f32,
                pry as f32,
                1.0,
                Some([1.0, grime as f32, 1.0]),
                prx as f32,
                prz as f32,
            );
            asm.collide_box(
                Surface::Metal,
                px as f32,
                (y + if lying { 0.3 } else { 0.45 }) as f32,
                pz as f32,
                0.64,
                if lying { 0.6 } else { 0.9 },
                0.64,
                0.0,
            );
            if !lying {
                let pebbles = rng.int(2, 5);
                ground_skirt(asm, rng, px, y, pz, 0.36, SkirtOpts { pebbles: Some(pebbles), ..SkirtOpts::default() });
                if tallest.is_none() {
                    tallest = Some([px, y, pz]);
                }
            }
        }
        // a plank ramp and litter round the cluster: nothing stands alone
        let plx = dx + rng.range(-1.2, 1.2);
        let ply = ground_y(dx, dz) + 0.03;
        let plz = dz + rng.range(-1.2, 1.2);
        let plry = rng.float() * 6.28;
        asm.put("plank_a", plx as f32, ply as f32, plz as f32, plry as f32, 1.2, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
        // and on some of them, a tarp thrown over the drums.
        // `tallest && rng.float() < 0.55` short-circuits: no standing barrel,
        // no roll.
        if let Some(t) = tallest {
            if rng.float() < 0.55 {
                let cw = rng.range(1.0, 1.5);
                let ch = rng.range(0.9, 1.3);
                let cloth = cloth_geometry(
                    cw as f32,
                    ch as f32,
                    ClothOpts { seg_x: 8, seg_y: 8, sag: 0.22, wrinkle: 0.055, twist: 0.1, thickness: 0.003, fray: 0.02, ..ClothOpts::default() },
                    Some(rng),
                );
                let key = *rng.pick(&TARP);
                let cry = rng.float() * 6.28;
                let grime = rng.range(0.55, 0.9);
                let m = ll(&Mat4::IDENTITY, t[0] as f32, (t[1] + 0.86) as f32, t[2] as f32, cry as f32, 1.0, 1.0, 1.0, -1.35, 0.0);
                asm.add_once(key, &cloth, Some(&m), Some(AccumAddOpts { masks: Some([0.35, grime as f32, 0.25]), paint: None }));
            }
        }
    }
}

/// `dressing.js:673-707`. A tyre pile and a pallet stack, on the pavement so
/// they never block the road.
fn tyre_and_pallet_stacks(asm: &mut Assembler, rng: &mut Rng) {
    for (px, pz, kind) in [(-5.5f64, 6.2f64, "tyres"), (5.55, -1.2, "pallets"), (5.45, -26.5, "tyres")] {
        if !cam_clear(px, pz, 1.2) {
            continue;
        }
        let y = ground_y(px, pz);
        if kind == "tyres" {
            let n = rng.int(5, 8);
            tyre_stack(asm, rng, px, y, pz, n);
            ground_skirt(asm, rng, px, y, pz, 0.45, SkirtOpts::default());
            asm.collide_box(Surface::Rubber, px as f32, (y + (f64::from(n) * 0.172) / 2.0) as f32, pz as f32, 0.7, (f64::from(n) * 0.172) as f32, 0.7, 0.0);
        } else {
            let n = rng.int(4, 7);
            for i in 0..n {
                let ox = px + rng.range(-0.07, 0.07);
                let oz = pz + rng.range(-0.07, 0.07);
                let ry = rng.range(-0.12, 0.12);
                let grime = rng.range(1.0, 1.4);
                asm.put("pallet", ox as f32, (y + f64::from(i) * 0.135) as f32, oz as f32, ry as f32, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
            }
            asm.collide_box(Surface::Wood, px as f32, (y + (f64::from(n) * 0.135) / 2.0) as f32, pz as f32, 1.2, (f64::from(n) * 0.135) as f32, 0.9, 0.0);
            let cry = rng.float() * 6.28;
            asm.put("crate_b", (px + 0.75) as f32, y as f32, (pz + 0.5) as f32, cry as f32, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
            let pebbles = rng.int(3, 6);
            ground_skirt(asm, rng, px, y, pz, 0.72, SkirtOpts { pebbles: Some(pebbles), ..SkirtOpts::default() });
        }
    }
}
