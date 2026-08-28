//! Ported from Claude-of-Duty `src/world/dressing.js:1403-1723` —
//! `dressBuildings` / `dressBuilding` / `alleyLines`: facade services and
//! roof clutter, driven by the anchors each building returned while it was
//! being generated.

use axiom_math::Mat4;

use crate::jsmath;
use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::buildings::BuildingInfo;
use crate::world::kit::{box_fine_kit, cloth_geometry, ll, patch_geometry, rubble_mound, ry_of, tube_y, world_of, ClothOpts, RubbleOpts};
use crate::world::palette::Surface;

use super::cable::{catenary_tube, CatenaryOpts};
use super::int_loop_continues;
use super::occupancy::{ground_y, is_open, jitter_rig};

const FABRIC3: [&str; 3] = ["fabric_red", "fabric_teal", "fabric_cream"];
const FABRIC4: [&str; 4] = ["fabric_red", "fabric_teal", "fabric_cream", "burlap"];
const BALCONY_JUNK: [&str; 7] = ["crate_b", "bucket", "planter", "box_card_b", "stool", "jerry_can", "tyre_small"];
const DOORSTEP_JUNK: [&str; 7] = ["bucket", "crate_b", "stool", "sandbag_a", "litter", "jerry_can", "planter"];
const ROOF_CRATES: [&str; 3] = ["crate_a", "crate_b", "crate_flat"];
const ROOF_MISC: [&str; 6] = ["stool", "chair", "tyre", "barrel_rust", "pallet", "gas_bottle"];
const ROOF_LITTER: [&str; 7] = ["brick_a", "brick_b", "rock_b", "litter", "cinder", "can", "plank_b"];

/// `dressBuildings(A, rng, infos)` (`dressing.js:1408-1413`).
pub fn dress_buildings(asm: &mut Assembler, rng: &mut Rng, infos: &[BuildingInfo]) {
    asm.jitter = Some(jitter_rig());
    for info in infos {
        dress_building(asm, rng, info);
    }
    alley_lines(asm, rng);
    asm.jitter = None;
}

/// `dressBuilding(A, rng, info)` (`dressing.js:1415-1681`).
fn dress_building(asm: &mut Assembler, rng: &mut Rng, info: &BuildingInfo) {
    let spec = &info.building;
    let top = info.roof_y;

    facade_services(asm, rng, info);
    balconies(asm, rng, info);
    signage(asm, rng, info);
    roof_clutter(asm, rng, info, spec.roof_props, top);
}

// ---- AC units, conduit and sat dishes hung off the open facades ----
/// `dressing.js:1420-1489`.
fn facade_services(asm: &mut Assembler, rng: &mut Rng, info: &BuildingInfo) {
    for wnd in &info.windows {
        let pm = wnd.pm;
        // Ground-floor windows are skipped BEFORE any draw.
        if wnd.floor == 0 {
            continue;
        }
        if rng.float() < 0.34 {
            // beside the window, bracketed off the wall
            let dx = (if rng.float() < 0.5 { -1.0f32 } else { 1.0 }) * (wnd.w / 2.0 + 0.55);
            let wp = world_of(&pm, wnd.x + dx, wnd.y - 0.35, -0.36);
            let grime = rng.range(0.8, 1.3);
            asm.put("ac_unit", wp.x, wp.y, wp.z, ry_of(&pm) + std::f32::consts::PI, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
            // condensate runs down the render below the unit: a narrow grime streak
            let run_h = wnd.y - 1.1;
            if run_h > 0.5 {
                let geo = box_fine_kit(asm);
                let m = ll(&pm, wnd.x + dx, wnd.y - 0.75 - run_h / 2.0, -0.004, 0.0, 0.16, run_h, 0.008, 0.0, 0.0);
                asm.add("plaster_sand", &geo, Some(&m), Some(AccumAddOpts { masks: Some([0.0, 1.0, 0.75]), paint: None }));
            }
        }
        if rng.float() < 0.16 {
            let side = if rng.float() < 0.5 { -1.0f32 } else { 1.0 };
            let wp = world_of(&pm, wnd.x + (wnd.w / 2.0 + 0.4) * side, wnd.y + 0.3, -0.07);
            asm.put("conduit_box", wp.x, wp.y, wp.z, ry_of(&pm) + std::f32::consts::PI, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
        }
        // washing line strung across a balcony window
        if rng.float() < 0.18 {
            let a = world_of(&pm, wnd.x - wnd.w / 2.0 - 0.1, wnd.y + 0.5, -0.12);
            let b = world_of(&pm, wnd.x + wnd.w / 2.0 + 0.1, wnd.y + 0.45, -0.12);
            let af = [f64::from(a.x), f64::from(a.y), f64::from(a.z)];
            let bf = [f64::from(b.x), f64::from(b.y), f64::from(b.z)];
            let line = catenary_tube(af, bf, 0.08, 0.008, CatenaryOpts { seg: 6, radial: 4, jitter: 0.0 });
            asm.add_once("metal_dark", &line, None, Some(AccumAddOpts { masks: Some([0.3, 0.6, 0.0]), paint: None }));
            for i in 0..2 {
                let t = 0.3 + f64::from(i) * 0.4;
                let cw = rng.range(0.3, 0.5);
                let ch = rng.range(0.4, 0.7);
                let wrinkle = rng.range(0.04, 0.065);
                let cloth = cloth_geometry(
                    cw as f32,
                    ch as f32,
                    ClothOpts { seg_x: 5, seg_y: 6, sag: 0.1, wrinkle: wrinkle as f32, twist: 0.1, fray: 0.012, ..ClothOpts::default() },
                    Some(rng),
                );
                let key = *rng.pick(&FABRIC3);
                let m = ll(
                    &Mat4::IDENTITY,
                    (af[0] + (bf[0] - af[0]) * t) as f32,
                    (af[1] - 0.35) as f32,
                    (af[2] + (bf[2] - af[2]) * t) as f32,
                    ry_of(&pm) + std::f32::consts::FRAC_PI_2,
                    1.0,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                );
                asm.add_once(key, &cloth, Some(&m), Some(AccumAddOpts { masks: Some([0.35, 0.6, 0.2]), paint: None }));
            }
        }
    }
}

// ---- balconies get lived in ----
/// `dressing.js:1492-1531`.
fn balconies(asm: &mut Assembler, rng: &mut Rng, info: &BuildingInfo) {
    for bal in &info.balconies {
        let pm = bal.pm;
        let n = rng.int(1, 4);
        for _ in 0..n {
            let lx = bal.x + rng.range(f64::from(-bal.w / 2.0 + 0.3), f64::from(bal.w / 2.0 - 0.3)) as f32;
            let lz = -rng.range(0.35, f64::from(bal.d - 0.3)) as f32;
            let wp = world_of(&pm, lx, bal.y + 0.13, lz);
            let id = *rng.pick(&BALCONY_JUNK);
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.85, 1.1);
            let grime = rng.range(1.0, 1.4);
            asm.put(id, wp.x, wp.y, wp.z, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        }
        // rug over the railing — instantly reads as inhabited
        if rng.float() < 0.55 {
            let cw = rng.range(0.8, 1.4);
            let ch = rng.range(0.7, 1.1);
            let wrinkle = rng.range(0.04, 0.07);
            let fray = rng.range(0.012, 0.03);
            let cloth = cloth_geometry(
                cw as f32,
                ch as f32,
                ClothOpts { seg_x: 7, seg_y: 7, sag: 0.09, wrinkle: wrinkle as f32, thickness: 0.0034, fray: fray as f32, ..ClothOpts::default() },
                Some(rng),
            );
            let off = rng.range(-0.3, 0.3) as f32;
            let wp = world_of(&pm, bal.x + off, bal.y + 0.95, -bal.d - 0.03);
            let key = *rng.pick(&FABRIC3);
            let m = ll(&Mat4::IDENTITY, wp.x, wp.y, wp.z, ry_of(&pm) + std::f32::consts::PI, 1.0, 1.0, 1.0, 0.0, 0.0);
            asm.add_once(key, &cloth, Some(&m), Some(AccumAddOpts { masks: Some([0.4, 0.55, 0.2]), paint: None }));
        }
    }
}

// ---- signage over shop and door openings ----
/// `dressing.js:1534-1573`.
fn signage(asm: &mut Assembler, rng: &mut Rng, info: &BuildingInfo) {
    for aw in &info.awnings {
        if rng.float() < 0.55 {
            let wp = world_of(&aw.pm, aw.x, aw.y + 1.0, -0.16);
            let grime = rng.range(0.8, 1.3);
            asm.put_s(
                "sign_board",
                wp.x,
                wp.y,
                wp.z,
                ry_of(&aw.pm) + std::f32::consts::PI,
                (1.3f32).min(aw.w / 1.6),
                1.0,
                1.0,
                Some([1.0, grime as f32, 1.0]),
                0.0,
                0.0,
            );
        }
    }
    for dr in &info.doors {
        if rng.float() < 0.5 {
            let off = rng.range(-0.2, 0.2) as f32;
            let wp = world_of(&dr.pm, dr.x + off, 2.55, -0.12);
            let s = rng.range(0.85, 1.15);
            asm.put("sign_hang", wp.x, wp.y, wp.z, ry_of(&dr.pm) + std::f32::consts::PI, s as f32, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
        }
        // step, mat, and the junk that lives beside a doorway
        let wp = world_of(&dr.pm, dr.x, 0.02, -0.55);
        let mut i = 0;
        while int_loop_continues(rng, i, 1, 4) {
            let ox = f64::from(wp.x) + rng.range(-1.3, 1.3);
            let oz = f64::from(wp.z) + rng.range(-1.0, 1.0);
            i += 1;
            if !is_open(ox, oz, 0.15) {
                continue;
            }
            let id = *rng.pick(&DOORSTEP_JUNK);
            let py = ground_y(ox, oz);
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.85, 1.1);
            let grime = rng.range(1.0, 1.4);
            asm.put(id, ox as f32, py as f32, oz as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        }
    }
}

// ---- roof clutter ----
/// `dressing.js:1576-1681`. Roofs are playable ground in this map (balconies
/// and parapets are the elevation layer), so they get real density, not a
/// token water tank.
fn roof_clutter(asm: &mut Assembler, rng: &mut Rng, info: &BuildingInfo, roof_props: u32, top: f32) {
    let rp = jsmath::round(f64::from(roof_props) * 2.4) as i64 + 2;
    // The ROOF plate, not the ground footprint. On a setback building the two
    // differ by a couple of metres, and using the footprint hangs water tanks,
    // aerials and crate stacks in mid-air over the terrace void.
    let rs = info.roof_spec;
    let (rsx, rsz, rsw, rsd) = (f64::from(rs.x), f64::from(rs.z), f64::from(rs.w), f64::from(rs.d));
    let rx0 = rsx - rsw / 2.0 + 1.0;
    let rx1 = rsx + rsw / 2.0 - 1.0;
    let rz0 = rsz - rsd / 2.0 + 1.0;
    let rz1 = rsz + rsd / 2.0 - 1.0;
    let roof_y = f64::from(top) + 0.02;

    for _ in 0..rp {
        let px = rng.range(rx0, rx1);
        let pz = rng.range(rz0, rz1);
        let pick = rng.float();
        if pick < 0.22 {
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.9, 1.15);
            let grime = rng.range(0.9, 1.3);
            asm.put("water_tank", px as f32, roof_y as f32, pz as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
            asm.collide_box(Surface::Metal, px as f32, (roof_y + 0.55) as f32, pz as f32, 1.2, 1.1, 1.2, 0.0);
        } else if pick < 0.45 {
            let pry = rng.float() * 6.28;
            let ps = rng.range(0.85, 1.15);
            let grime = rng.range(0.8, 1.3);
            asm.put("sat_dish", px as f32, roof_y as f32, pz as f32, pry as f32, ps as f32, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        } else if pick < 0.6 {
            let pry = rng.float() * 6.28;
            asm.put("roof_vent", px as f32, roof_y as f32, pz as f32, pry as f32, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
        } else if pick < 0.78 {
            let n = rng.int(2, 4);
            for k in 0..n {
                let id = *rng.pick(&ROOF_CRATES);
                let ox = px + rng.range(-0.15, 0.15);
                let oz = pz + rng.range(-0.15, 0.15);
                let pry = rng.float() * 6.28;
                let grime = rng.range(1.0, 1.4);
                asm.put(id, ox as f32, (roof_y + f64::from(k) * 0.53) as f32, oz as f32, pry as f32, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
            }
            asm.collide_box(Surface::Wood, px as f32, (roof_y + f64::from(n) * 0.26) as f32, pz as f32, 0.7, (f64::from(n) * 0.53) as f32, 0.7, 0.0);
        } else {
            let id = *rng.pick(&ROOF_MISC);
            let pry = rng.float() * 6.28;
            let grime = rng.range(1.1, 1.5);
            asm.put(id, px as f32, roof_y as f32, pz as f32, pry as f32, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        }
    }

    // dust and grit blown into the roof corners
    for _ in 0..4 {
        let radius = rng.range(0.6, 1.6);
        let g = patch_geometry(rng, radius, 9, 0.5, 0.0);
        let gx = rng.range(rx0, rx1);
        let gz = rng.range(rz0, rz1);
        let gry = rng.float() * 6.28;
        let m = ll(&Mat4::IDENTITY, gx as f32, (roof_y + 0.012) as f32, gz as f32, gry as f32, 1.0, 1.0, 0.7, 0.0, 0.0);
        asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.85, 0.5]), paint: None }));
    }

    let mut i = 0;
    while int_loop_continues(rng, i, 4, 10) {
        let px = rng.range(rx0 + 0.7, rx1 - 0.7);
        let pz = rng.range(rz0 + 0.7, rz1 - 0.7);
        let id = *rng.pick(&ROOF_LITTER);
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.6, 1.2);
        asm.put(id, px as f32, (roof_y + 0.02) as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
        i += 1;
    }

    // rooftop laundry line between the parapets, and rubble in a corner.
    // `&&` short-circuits: a narrow roof never draws the 0.4 roll.
    if rsw > 10.0 && rng.float() < 0.4 {
        let a = [rsx - rsw / 2.0 + 0.4, roof_y + 1.0, rng.range(rz0, rz1)];
        let b = [rsx + rsw / 2.0 - 0.4, roof_y + 0.96, rng.range(rz0, rz1)];
        let line = catenary_tube(a, b, 0.3, 0.01, CatenaryOpts { seg: 10, radial: 4, jitter: 0.0 });
        asm.add_once("metal_dark", &line, None, Some(AccumAddOpts { masks: Some([0.3, 0.6, 0.0]), paint: None }));
        for sx in [-1.0f64, 1.0] {
            let post = box_fine_kit(asm);
            let pz = a[2] + if sx > 0.0 { b[2] - a[2] } else { 0.0 };
            let m = ll(&Mat4::IDENTITY, (rsx + sx * (rsw / 2.0 - 0.4)) as f32, (roof_y + 0.9) as f32, pz as f32, 0.0, 0.06, 1.8, 0.06, 0.0, 0.0);
            asm.add("metal_rust", &post, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.1]), paint: None }));
        }
        let n = rng.int(2, 5);
        for i in 0..n {
            let t = (f64::from(i) + 0.5) / f64::from(n);
            let cw = rng.range(0.5, 0.8);
            let ch = rng.range(0.45, 0.8);
            let c_sag = rng.range(0.12, 0.22);
            let c_wrinkle = rng.range(0.045, 0.075);
            let c_twist = rng.range(0.08, 0.18);
            let c_fray = rng.range(0.01, 0.025);
            let cloth = cloth_geometry(
                cw as f32,
                ch as f32,
                ClothOpts {
                    seg_x: 7,
                    seg_y: 8,
                    sag: c_sag as f32,
                    wrinkle: c_wrinkle as f32,
                    twist: c_twist as f32,
                    fray: c_fray as f32,
                    ..ClothOpts::default()
                },
                Some(rng),
            );
            let key = *rng.pick(&FABRIC4);
            let grime = rng.range(0.4, 0.8);
            let m = ll(
                &Mat4::IDENTITY,
                (a[0] + (b[0] - a[0]) * t) as f32,
                (a[1] - 0.5 - 0.22 * (t * std::f64::consts::PI).sin()) as f32,
                (a[2] + (b[2] - a[2]) * t) as f32,
                0.0,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
            );
            asm.add_once(key, &cloth, Some(&m), Some(AccumAddOpts { masks: Some([0.3, grime as f32, 0.2]), paint: None }));
        }
    }
    if rng.float() < 0.6 {
        let mx = rng.range(rx0, rx1);
        let mz = rng.range(rz0, rz1);
        let radius = rng.range(0.7, 1.3);
        let count = rng.int(8, 16);
        rubble_mound(asm, rng, mx as f32, roof_y as f32, mz as f32, radius as f32, count as u32, RubbleOpts { key: "concrete_dark" });
    }

    // aerials: thin, tall, and they do a lot for a skyline
    let mut i = 0;
    while int_loop_continues(rng, i, 1, 3) {
        let px = rng.range(rx0, rx1);
        let pz = rng.range(rz0, rz1);
        let h = rng.range(1.4, 3.2);
        let pipe = asm.cache("aerial", || tube_y(0.018, 1.0, 5, 1.0, false, 1));
        let m = ll(&Mat4::IDENTITY, px as f32, roof_y as f32, pz as f32, 0.0, 1.0, h as f32, 1.0, 0.0, 0.0);
        asm.add("metal_rust", &pipe, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.1]), paint: None }));
        for k in 0..4 {
            let kry = rng.float() * 3.14;
            let ks = rng.range(0.25, 0.5);
            let m = ll(
                &Mat4::IDENTITY,
                px as f32,
                (roof_y + h * (0.5 + f64::from(k) * 0.11)) as f32,
                pz as f32,
                kry as f32,
                1.0,
                ks as f32,
                1.0,
                0.0,
                std::f32::consts::FRAC_PI_2,
            );
            asm.add("metal_rust", &pipe, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.1]), paint: None }));
        }
        i += 1;
    }
}

/// `alleyLines(A, rng, infos)` (`dressing.js:1684-1723`): cables and washing
/// lines strung across the alleys between buildings.
///
/// **`infos` is a dead parameter in the source** — `alleyLines` reads only
/// its own hard-coded `spans` table. Dropped from this signature rather than
/// carried as an unused argument; the fact is recorded here instead.
fn alley_lines(asm: &mut Assembler, rng: &mut Rng) {
    const SPANS: [[f64; 6]; 6] = [
        [-6.6, 5.0, 21.0, -6.6, 5.4, 24.0],
        [-6.6, 4.2, -9.0, -6.6, 4.6, -11.5],
        [7.0, 4.6, 2.5, 7.0, 4.2, 6.6],
        [7.0, 5.6, -16.0, 7.0, 5.2, -20.0],
        [-8.0, 6.4, 20.6, -8.0, 6.0, 23.8],
        [8.6, 6.2, 2.2, 8.6, 5.8, 7.2],
    ];
    for [x0, y0, z0, x1, y1, z1] in SPANS {
        let t = catenary_tube([x0, y0, z0], [x1, y1, z1], 0.5, 0.016, CatenaryOpts { seg: 10, radial: 4, jitter: 0.04 });
        asm.add_once("metal_dark", &t, None, Some(AccumAddOpts { masks: Some([0.4, 0.7, 0.2]), paint: None }));
        let n = rng.int(2, 4);
        for i in 0..n {
            let f = (f64::from(i) + 0.5) / f64::from(n);
            let cw = rng.range(0.45, 0.8);
            let ch = rng.range(0.5, 1.0);
            let c_sag = rng.range(0.12, 0.22);
            let c_wrinkle = rng.range(0.045, 0.075);
            let c_twist = rng.range(0.08, 0.18);
            let c_fray = rng.range(0.01, 0.025);
            let cloth = cloth_geometry(
                cw as f32,
                ch as f32,
                ClothOpts {
                    seg_x: 6,
                    seg_y: 8,
                    sag: c_sag as f32,
                    wrinkle: c_wrinkle as f32,
                    twist: c_twist as f32,
                    fray: c_fray as f32,
                    ..ClothOpts::default()
                },
                Some(rng),
            );
            let key = *rng.pick(&FABRIC4);
            let grime = rng.range(0.4, 0.8);
            let m = ll(
                &Mat4::IDENTITY,
                (x0 + (x1 - x0) * f) as f32,
                (y0 + (y1 - y0) * f - 0.6 - 0.4 * (f * std::f64::consts::PI).sin()) as f32,
                (z0 + (z1 - z0) * f) as f32,
                (-(z1 - z0)).atan2(x1 - x0) as f32,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
            );
            asm.add_once(key, &cloth, Some(&m), Some(AccumAddOpts { masks: Some([0.3, grime as f32, 0.2]), paint: None }));
        }
    }
}
