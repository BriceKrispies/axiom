//! Ported from Claude-of-Duty `src/world/dressing.js:1861-2205` —
//! `gateAperture`, `merlonRun` and `buildGate`: the street terminator at the
//! south end of the vista.
//!
//! Four masses at four heights, stepped in Z as well as Y, with a pointed
//! archway through the middle, an upper loggia of dark recessed openings, a
//! rampart walkway on corbels with a shadowed underside, sandbag
//! emplacements on top, and a sliver of sky over the arch that shows `BS3`
//! receding behind it.

use axiom_math::Mat4;

use crate::jsmath;
use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::kit::{box_kit, box_soft_kit, ll, rubble_mound, spall_patch, RubbleOpts};
use crate::world::layout::GATE;
use crate::world::palette::Surface;

use super::int_loop_continues;
use super::occupancy::ground_y;
use super::sandbags::sandbag_wall;

const CHECKPOINT_LITTER: [&str; 8] = ["brick_a", "brick_b", "rock_b", "litter", "cinder", "can", "weeds", "plank_b"];

// ============================================================== aperture ==
/// `gateAperture(A, rng, x, y, z, w, h, t, opts = {})`'s `opts`
/// (`dressing.js:1869`). Defaults: `recess = 0.5`, `sill = true`.
///
/// **No call site in the source ever passes `sill: false`** — the flag is
/// carried anyway rather than hard-coding the always-taken arm, because the
/// source's `opts.sill !== false` guard is real code and a future caller
/// would silently lose it.
#[derive(Debug, Clone, Copy)]
pub struct ApertureOpts {
    pub recess: f64,
    pub sill: bool,
}

impl Default for ApertureOpts {
    fn default() -> Self {
        ApertureOpts { recess: 0.5, sill: true }
    }
}

/// `gateAperture(A, rng, x, y, z, w, h, t, opts = {})`
/// (`dressing.js:1869-1935`): a deep opening in the terminator mass — a
/// recessed panel with a genuinely dark back plane and a lintel over it.
#[allow(clippy::too_many_arguments)]
fn gate_aperture(asm: &mut Assembler, rng: &mut Rng, x: f64, y: f64, z: f64, w: f64, h: f64, t: f64, opts: ApertureOpts) {
    // The street runs down -Z and every hero camera looks along it, so +Z is
    // the face that matters.
    let zf = z + t / 2.0;
    let rec = opts.recess;

    // the void: dark, set well back, so the reveal shadows across it
    let bx = box_kit(asm);
    let m = ll(&Mat4::IDENTITY, x as f32, y as f32, (zf - rec - 0.06) as f32, 0.0, w as f32, h as f32, 0.12, 0.0, 0.0);
    asm.add("window_void", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.15, 0.95, 1.0]), paint: None }));

    // reveal: four returns boxing the void in, in shadow all afternoon
    let m = ll(&Mat4::IDENTITY, x as f32, (y + h / 2.0 + 0.07) as f32, (zf - rec / 2.0) as f32, 0.0, (w + 0.3) as f32, 0.14, rec as f32, 0.0, 0.0);
    asm.add("concrete_dark", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.3, 0.85, 0.9]), paint: None }));
    let m = ll(&Mat4::IDENTITY, x as f32, (y - h / 2.0 - 0.07) as f32, (zf - rec / 2.0) as f32, 0.0, (w + 0.3) as f32, 0.14, rec as f32, 0.0, 0.0);
    asm.add("concrete_dark", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.55, 0.75, 0.7]), paint: None }));
    for s in [-1.0f64, 1.0] {
        let m = ll(&Mat4::IDENTITY, (x + s * (w / 2.0 + 0.07)) as f32, y as f32, (zf - rec / 2.0) as f32, 0.0, 0.14, h as f32, rec as f32, 0.0, 0.0);
        asm.add("concrete_dark", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.3, 0.8, 0.85]), paint: None }));
    }

    // stone lintel / arch head standing proud of the wall face
    let soft = box_soft_kit(asm);
    let m = ll(&Mat4::IDENTITY, x as f32, (y + h / 2.0 + 0.16) as f32, (zf + 0.09) as f32, 0.0, (w + 0.5) as f32, 0.2, 0.34, 0.0, 0.0);
    asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.5, 0.25]), paint: None }));
    if opts.sill {
        let m = ll(&Mat4::IDENTITY, x as f32, (y - h / 2.0 - 0.1) as f32, (zf + 0.12) as f32, 0.0, (w + 0.44) as f32, 0.11, 0.42, 0.0, 0.0);
        asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.55, 0.45, 0.3]), paint: None }));
    }

    // a shutter or a rag hanging in some of them: nothing is uniform
    if rng.float() < 0.4 {
        let ox = rng.range(-0.1, 0.1);
        let sw = w * rng.range(0.5, 0.9);
        let sh = h * rng.range(0.4, 0.8);
        let m = ll(&Mat4::IDENTITY, (x + ox) as f32, (y - h * 0.1) as f32, (zf - 0.14) as f32, 0.0, sw as f32, sh as f32, 0.03, 0.0, 0.0);
        asm.add("metal_rust", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.6, 0.2]), paint: None }));
    }
}

// ============================================================== merlon run ==
/// `merlonRun(A, rng, x0, x1, z, t, yTop, opts = {})`'s `opts`
/// (`dressing.js:1948-1950`). Defaults: `key="plaster_sand"`, `depth=0.45`,
/// `set=0.06`.
#[derive(Debug, Clone, Copy)]
pub struct MerlonOpts<'a> {
    pub key: &'a str,
    pub depth: f64,
    pub set: f64,
}

impl Default for MerlonOpts<'_> {
    fn default() -> Self {
        MerlonOpts { key: "plaster_sand", depth: 0.45, set: 0.06 }
    }
}

/// `merlonRun(A, rng, x0, x1, z, t, yTop, opts = {})`
/// (`dressing.js:1947-1987`): an irregular crenellated run.
///
/// A merlon run at a perfectly regular pitch, all one height, all one value,
/// is the single loudest "untextured blockout" tell there is.
#[allow(clippy::too_many_arguments)]
fn merlon_run(asm: &mut Assembler, rng: &mut Rng, x0: f64, x1: f64, z: f64, t: f64, y_top: f64, opts: MerlonOpts) {
    let key = opts.key;
    let dt = t * opts.depth;
    let zc = z + t / 2.0 - dt / 2.0 - opts.set; // set back from the +Z face

    // coping course the merlons stand on, proud of the wall on both faces
    let soft = box_soft_kit(asm);
    let m = ll(&Mat4::IDENTITY, ((x0 + x1) / 2.0) as f32, (y_top + 0.07) as f32, z as f32, 0.0, (x1 - x0) as f32, 0.14, (t + 0.3) as f32, 0.0, 0.0);
    asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.85, 0.4, 0.15]), paint: None }));

    let bx = box_kit(asm);
    let mut x = x0 + rng.range(0.05, 0.35);
    while x < x1 - 0.4 {
        let w = rng.range(0.62, 1.35).min(x1 - 0.1 - x);
        if w < 0.3 {
            break;
        }
        let broken = rng.float() < 0.22;
        let h = if broken { rng.range(0.16, 0.42) } else { rng.range(0.62, 1.15) };
        let cx = x + w / 2.0;
        let lean = rng.range(-0.035, 0.035);
        let m = ll(&Mat4::IDENTITY, cx as f32, (y_top + 0.14 + h / 2.0) as f32, zc as f32, 0.0, w as f32, h as f32, dt as f32, 0.0, lean as f32);
        asm.add(key, &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.55, 0.45, 0.2]), paint: None }));
        asm.collide_box(Surface::Concrete, cx as f32, (y_top + 0.14 + h / 2.0) as f32, zc as f32, w as f32, h as f32, dt as f32, 0.0);
        // a cap stone on some, and spalled render showing the clay block
        // beneath. `!broken && …` short-circuits: a broken merlon never draws
        // the cap roll.
        if !broken && rng.float() < 0.55 {
            let m = ll(&Mat4::IDENTITY, cx as f32, (y_top + 0.16 + h) as f32, zc as f32, 0.0, (w + 0.1) as f32, 0.07, (dt + 0.1) as f32, 0.0, 0.0);
            asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.35, 0.1]), paint: None }));
        }
        if rng.float() < 0.45 {
            let sw = w * rng.range(0.3, 0.62);
            let sh = h * rng.range(0.25, 0.55);
            let g = spall_patch(rng, sw as f32, sh as f32, 0.02);
            let ox = rng.range(-w * 0.2, w * 0.2);
            let oy = h * rng.range(0.3, 0.7);
            let m = ll(&Mat4::IDENTITY, (cx + ox) as f32, (y_top + 0.14 + oy) as f32, (zc + dt / 2.0 - 0.013) as f32, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
            asm.add_once("brick_fine", &g, Some(&m), None);
        }
        x += w + rng.range(0.34, 0.95);
    }
}

// ================================================================== gate ==
/// One block of the mass: body, plinth, cornice, spalled render, walkway
/// (`dressing.js:1999-2058`, the `block` closure).
///
/// The source's `block` returns `{ cx, w }` and **no caller ever reads it**.
/// Dropped from this signature; the fact is recorded here rather than
/// carrying a dead return value.
#[allow(clippy::too_many_arguments)]
fn gate_block(asm: &mut Assembler, rng: &mut Rng, x0: f64, x1: f64, h: f64, tt: f64, zc: f64, key: &str) {
    let cx = (x0 + x1) / 2.0;
    let w = x1 - x0;

    let bx = box_kit(asm);
    let m = ll(&Mat4::IDENTITY, cx as f32, (h / 2.0) as f32, zc as f32, 0.0, w as f32, h as f32, tt as f32, 0.0, 0.0);
    asm.add(key, &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.45, 0.6, 0.35]), paint: None }));
    asm.collide_box(Surface::Concrete, cx as f32, (h / 2.0) as f32, zc as f32, w as f32, h as f32, tt as f32, 0.0);

    // plinth: catches the ground grime band and the sand drift at the base
    let soft = box_soft_kit(asm);
    let m = ll(&Mat4::IDENTITY, cx as f32, 0.4, zc as f32, 0.0, (w + 0.24) as f32, 0.8, (tt + 0.26) as f32, 0.0, 0.0);
    asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.85, 0.55]), paint: None }));

    // Pilasters standing 0.3 m proud at each end of the block.
    for s in [-1.0f64, 1.0] {
        let m = ll(
            &Mat4::IDENTITY,
            (cx + s * (w / 2.0 - 0.3)) as f32,
            (h * 0.5) as f32,
            (zc + tt / 2.0 + 0.15) as f32,
            0.0,
            0.6,
            (h - 0.2) as f32,
            0.34,
            0.0,
            0.0,
        );
        asm.add(key, &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.6, 0.5, 0.25]), paint: None }));
    }

    // cornice, well proud of the face
    let m = ll(&Mat4::IDENTITY, cx as f32, (h - 0.22) as f32, (zc + 0.2) as f32, 0.0, (w + 0.5) as f32, 0.3, (tt + 0.66) as f32, 0.0, 0.0);
    asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.45, 0.2]), paint: None }));

    // corbels under it, so the overhang reads as carried rather than floating
    let nb = (jsmath::round(w / 1.15) as i64).max(2);
    for i in 0..nb {
        let bxp = x0 + 0.35 + (i as f64 / (1.max(nb - 1)) as f64) * (w - 0.7);
        let m = ll(&Mat4::IDENTITY, bxp as f32, (h - 0.62) as f32, (zc + tt / 2.0 + 0.22) as f32, 0.0, 0.22, 0.44, 0.46, 0.0, 0.0);
        asm.add("concrete", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.55, 0.35]), paint: None }));
    }

    // A string course at mid height.
    let m = ll(&Mat4::IDENTITY, cx as f32, (h * 0.46) as f32, (zc + tt / 2.0 + 0.11) as f32, 0.0, (w + 0.18) as f32, 0.16, 0.3, 0.0, 0.0);
    asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.5, 0.25]), paint: None }));

    // Spalled render over the visible face.
    let sp = jsmath::round(w * h * 0.05) as i64;
    for _ in 0..sp {
        let sw = rng.range(0.24, 0.7);
        let sh = rng.range(0.22, 0.6);
        let g = spall_patch(rng, sw as f32, sh as f32, 0.022);
        let px = rng.range(x0 + 0.5, x1 - 0.5);
        let py = rng.range(0.9, h - 0.7);
        let m = ll(&Mat4::IDENTITY, px as f32, py as f32, (zc + tt / 2.0 - 0.014) as f32, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once("brick_fine", &g, Some(&m), None);
    }
}

/// `buildGate(A, rng)` (`dressing.js:1996-2205`).
pub fn build_gate(asm: &mut Assembler, rng: &mut Rng) {
    let g = &GATE;
    let (z, span, height, outer_w, body_h) = (g.z, g.span, g.height, g.outer_w, g.body_h);
    let (x_l0, x_l1, h_l) = (g.x_l0, g.x_l1, g.h_l);
    let (x_r0, x_r1, h_r, east_proud) = (g.x_r0, g.x_r1, g.h_r, g.east_proud);
    let (x_t0, x_t1, h_t, tower_proud) = (g.x_t0, g.x_t1, g.h_t, g.tower_proud);
    let t = g.depth;

    // ------------------------------------------------------- four masses --
    // west gatehouse block: lowest, with an upper loggia of three dark openings
    gate_block(asm, rng, x_l0, x_l1, h_l, t, z, "plaster_sand");
    for i in 0..3 {
        gate_aperture(asm, rng, x_l0 + 1.0 + f64::from(i) * ((x_l1 - x_l0 - 2.0) / 2.0), h_l * 0.66, z, 0.9, 1.5, t, ApertureOpts::default());
    }
    gate_aperture(asm, rng, (x_l0 + x_l1) / 2.0, h_l * 0.3, z, 1.1, 1.3, t, ApertureOpts::default());
    merlon_run(asm, rng, x_l0, x_l1, z, t, h_l, MerlonOpts::default());

    // east block: nearly two metres taller and half a metre proud
    let zr = z + east_proud / 2.0;
    let tr = t + east_proud;
    gate_block(asm, rng, x_r0, x_r1, h_r, tr, zr, "plaster_blue");
    gate_aperture(asm, rng, (x_r0 + x_r1) / 2.0, h_r * 0.62, zr, 1.0, 1.6, tr, ApertureOpts::default());
    gate_aperture(asm, rng, (x_r0 + x_r1) / 2.0, h_r * 0.34, zr, 0.85, 1.2, tr, ApertureOpts::default());
    merlon_run(asm, rng, x_r0, x_r1, zr, tr, h_r, MerlonOpts { key: "plaster_blue", ..MerlonOpts::default() });

    // the tower: tallest, and standing proud toward the camera
    let zt = z + tower_proud / 2.0;
    gate_block(asm, rng, x_t0, x_t1, h_t, t + tower_proud, zt, "plaster_cream");
    for i in 0..3 {
        gate_aperture(
            asm,
            rng,
            (x_t0 + x_t1) / 2.0 + (f64::from(i) - 1.0) * 1.05,
            h_t * 0.55 + if i == 1 { 0.25 } else { 0.0 },
            zt,
            0.5,
            if i == 1 { 1.5 } else { 1.1 },
            t + tower_proud,
            ApertureOpts::default(),
        );
    }
    gate_aperture(asm, rng, (x_t0 + x_t1) / 2.0, h_t * 0.8, zt, 1.5, 1.0, t + tower_proud, ApertureOpts { recess: 0.75, sill: true });
    merlon_run(asm, rng, x_t0, x_t1, z + tower_proud / 2.0, t + tower_proud, h_t, MerlonOpts { key: "plaster_cream", ..MerlonOpts::default() });
    // a bent aerial on the tower: breaks the hard corner against the sky
    let bx = box_kit(asm);
    let m = ll(&Mat4::IDENTITY, (x_t1 - 0.5) as f32, (h_t + 1.9) as f32, zt as f32, 0.0, 0.06, 3.4, 0.06, 0.04, 0.07);
    asm.add("metal_rust", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.95, 0.5, 0.0]), paint: None }));
    asm.put("sat_dish", (x_t0 + 0.9) as f32, (h_t + 0.3) as f32, (zt + 0.4) as f32, 0.7, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);

    // sandbag emplacements on the ramparts, and a crate of ammunition
    sandbag_wall(asm, rng, x_l0 + 1.9, z - 0.15, 0.0, 2.4, 3, Some(h_l + 0.16));
    sandbag_wall(asm, rng, x_r0 + 1.7, zr - 0.15, 0.0, 1.9, 3, Some(h_r + 0.16));
    sandbag_wall(asm, rng, (x_t0 + x_t1) / 2.0, zt - 0.25, 0.0, 2.2, 4, Some(h_t + 0.16));
    asm.skirts = false;
    asm.put("crate_c", (x_l1 - 1.2) as f32, (h_l + 0.16) as f32, (z - 0.6) as f32, 0.4, 1.0, Some([1.0, 1.3, 1.0]), 0.0, 0.0);
    asm.put("barrel_rust", (x_r0 + 0.6) as f32, (h_r + 0.16) as f32, (zr - 0.5) as f32, 0.2, 1.0, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
    asm.skirts = true;

    // The spandrel over the arch, built as a wall panel with a pointed hole.
    let span_h = body_h - height;
    let bx = box_kit(asm);
    let m = ll(&Mat4::IDENTITY, 0.0, (height + span_h / 2.0) as f32, z as f32, 0.0, (span + 0.4) as f32, span_h as f32, t as f32, 0.0, 0.0);
    asm.add("plaster_sand", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.45, 0.6, 0.35]), paint: None }));
    asm.collide_box(Surface::Concrete, 0.0, (height + span_h / 2.0) as f32, z as f32, (span + 0.4) as f32, span_h as f32, t as f32, 0.0);

    // Arch voussoirs: individual stones around a pointed profile.
    let seg = 15;
    for i in 0..=seg {
        let a = (f64::from(i) / f64::from(seg)) * std::f64::consts::PI;
        let r = span / 2.0;
        let px = -a.cos() * r;
        let py = height - r + a.sin() * r * 1.18;
        if py < height - r - 0.01 {
            continue;
        }
        let ang = a - std::f64::consts::FRAC_PI_2;
        let m = ll(&Mat4::IDENTITY, px as f32, py as f32, z as f32, 0.0, 0.62, 0.42, (t + 0.14) as f32, 0.0, (-ang) as f32);
        asm.add("concrete", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.45, 0.25]), paint: None }));
    }
    // spring-line blocks and the walls beside the opening
    for sx in [-1.0f64, 1.0] {
        let m = ll(
            &Mat4::IDENTITY,
            (sx * (span / 2.0 + 0.1)) as f32,
            (height - span / 2.0 - 0.2) as f32,
            z as f32,
            0.0,
            0.6,
            0.4,
            (t + 0.2) as f32,
            0.0,
            0.0,
        );
        asm.add("concrete", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.5, 0.3]), paint: None }));
        asm.collide_box(
            Surface::Concrete,
            (sx * (span / 2.0 + 0.28)) as f32,
            ((height - span / 2.0) / 2.0) as f32,
            z as f32,
            0.56,
            (height - span / 2.0) as f32,
            (t + 0.2) as f32,
            0.0,
        );
    }

    // The rampart walkway over the arch.
    let wz = z + t / 2.0 + 0.38;
    let m = ll(&Mat4::IDENTITY, 0.0, (body_h + 0.11) as f32, wz as f32, 0.0, (span + 1.4) as f32, 0.22, 0.82, 0.0, 0.0);
    asm.add("roof_screed", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.55, 0.35, 0.15]), paint: None }));
    asm.collide_box(Surface::Concrete, 0.0, (body_h + 0.11) as f32, wz as f32, (span + 1.4) as f32, 0.22, 0.82, 0.0);
    for i in 0..6 {
        let bxp = -(span + 0.6) / 2.0 + (f64::from(i) / 5.0) * (span + 0.6);
        let m = ll(&Mat4::IDENTITY, bxp as f32, (body_h - 0.24) as f32, (wz - 0.06) as f32, 0.0, 0.2, 0.46, 0.66, 0.0, 0.0);
        asm.add("concrete", &bx, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.6, 0.4]), paint: None }));
    }
    // a low, irregular parapet along the walkway's outer edge, sandbags behind
    merlon_run(asm, rng, -span / 2.0 - 0.6, span / 2.0 + 0.6, z + 0.76, t, body_h + 0.22, MerlonOpts { depth: 0.34, set: 0.02, ..MerlonOpts::default() });
    sandbag_wall(asm, rng, -0.9, z + 0.15, 0.0, 2.0, 3, Some(body_h + 0.34));

    // guard hut and checkpoint clutter under the arch.
    // `const hutX = -span / 2 - 1.2;` (`dressing.js:2152`) is computed and
    // never read anywhere in the source — a dead binding, preserved here as
    // a note rather than as an unused Rust `let`.
    asm.put("block_big", 0.0, 0.0, (z + 3.2) as f32, 0.1, 1.0, Some([1.0, 1.2, 1.0]), 0.0, 0.0);
    asm.collide_box(Surface::Concrete, 0.0, 0.48, (z + 3.2) as f32, 1.3, 0.96, 0.9, 0.0);
    for [bxp, bz, br] in [[-2.2f64, z + 2.6, 0.1], [2.4, z + 2.2, 1.6], [-1.4, z - 2.4, 1.5], [2.0, z - 2.8, 0.2]] {
        let grime = rng.range(0.9, 1.3);
        asm.put("jersey", bxp as f32, 0.0, bz as f32, br as f32, 1.0, Some([1.0, grime as f32, 1.0]), 0.0, 0.0);
        asm.collide_box(Surface::Concrete, bxp as f32, 0.46, bz as f32, 0.62, 0.92, 1.9, br as f32);
    }
    sandbag_wall(asm, rng, -1.9, z + 4.6, 0.1, 2.4, 4, None);
    sandbag_wall(asm, rng, 2.1, z - 4.4, 0.0, 2.0, 3, None);
    for _ in 0..24 {
        let px = rng.range(-outer_w / 2.0, outer_w / 2.0);
        let pz = z + rng.range(-5.0, 5.0);
        if px.abs() > span / 2.0 && (pz - z).abs() < t / 2.0 + 0.3 {
            continue;
        }
        let id = *rng.pick(&CHECKPOINT_LITTER);
        let py = ground_y(px, pz) + 0.02;
        let pry = rng.float() * 6.28;
        let ps = rng.range(0.6, 1.2);
        asm.put(id, px as f32, py as f32, pz as f32, pry as f32, ps as f32, Some([1.0, 1.4, 1.0]), 0.0, 0.0);
    }
    // spalled corners and a bullet-scarred face
    rubble_mound(asm, rng, (-span / 2.0 - 1.0) as f32, 0.0, (z + 1.4) as f32, 1.2, 16, RubbleOpts { key: "concrete" });
    rubble_mound(asm, rng, (span / 2.0 + 1.4) as f32, 0.0, (z - 1.6) as f32, 1.0, 12, RubbleOpts { key: "concrete" });

    // Bullet scarring, clustered. Kept off the tower, whose face stands 0.9 m
    // proud — a pock on the main plane there would float inside the masonry.
    if asm.has("pock") {
        for _ in 0..12 {
            let cx = rng.range(x_l0 + 0.5, x_r1 - 0.5);
            let cy = rng.range(0.6, 6.0);
            if cx.abs() < span / 2.0 && cy < height {
                continue;
            }
            let mut j = 0;
            while int_loop_continues(rng, j, 3, 8) {
                let px = cx + rng.gauss() * 0.4;
                let py = cy + rng.gauss() * 0.3;
                j += 1;
                if px.abs() < span / 2.0 && py < height {
                    continue;
                }
                if px < x_l0 + 0.1 || px > x_r1 - 0.1 || py < 0.2 {
                    continue;
                }
                if py > (if px < x_l1 { h_l } else { h_r }) - 0.4 {
                    continue;
                }
                let s = rng.range(0.55, 1.4);
                let sz = rng.range(0.5, 1.2);
                let grime = rng.range(0.7, 1.3);
                asm.put_s(
                    "pock",
                    px as f32,
                    py as f32,
                    (z + t / 2.0 + 0.0015) as f32,
                    0.0,
                    s as f32,
                    s as f32,
                    sz as f32,
                    Some([1.0, grime as f32, 1.0]),
                    0.0,
                    0.0,
                );
            }
        }
        // and a burst across the tower's own proud face
        for _ in 0..4 {
            let cx = rng.range(x_t0 + 0.4, x_t1 - 0.4);
            let cy = rng.range(0.8, h_t - 1.0);
            let mut j = 0;
            while int_loop_continues(rng, j, 3, 7) {
                let px = cx + rng.gauss() * 0.35;
                let py = cy + rng.gauss() * 0.28;
                j += 1;
                if px < x_t0 + 0.1 || px > x_t1 - 0.1 || py < 0.3 {
                    continue;
                }
                let s = rng.range(0.5, 1.3);
                let sz = rng.range(0.5, 1.1);
                let grime = rng.range(0.7, 1.3);
                asm.put_s(
                    "pock",
                    px as f32,
                    py as f32,
                    (z + t / 2.0 + tower_proud + 0.0015) as f32,
                    0.0,
                    s as f32,
                    s as f32,
                    sz as f32,
                    Some([1.0, grime as f32, 1.0]),
                    0.0,
                    0.0,
                );
            }
        }
    }
}
