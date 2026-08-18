//! Ported from Claude-of-Duty `src/world/kit.js:112-453` — the per-window
//! state distribution (`windowState`) and the window unit itself
//! (`windowUnit`, `sashLeaf`, `shutterLeaf`).

use axiom_math::{Mat4, Vec3};

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::geo::WorldGeo;

use super::{box_thin_kit, box_kit, box_soft_kit, cloth_geometry, ll, merge_simple, pane_kit, plain_box, trs, ClothOpts, WallHole};

/// The per-window states (`kit.js:122-133`'s seven return strings). A facade
/// where every opening is the same glazed panel is, per the source's own
/// comment, "the single loudest tell of procedural architecture" — real
/// streets have open windows, boarded windows, shuttered windows and the odd
/// lit room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Boarded,
    Open,
    Shuttered,
    Ajar,
    Curtain,
    Lit,
    Glazed,
}

/// `windowState(rng, floor = 1, damage = 0.2, opts = {})` (`kit.js:122-134`).
/// `opts.allowLit` (default `true` via `!== false`) is `allow_lit` here.
/// Ground-floor openings (`floor <= 0`) are shopfronts and barred windows,
/// never a `Shuttered`/`Ajar` casement — those two arms are gated on
/// `upper = floor > 0` exactly as the source gates them.
pub fn window_state(rng: &mut Rng, floor: i32, damage: f64, allow_lit: bool) -> WindowState {
    let r = rng.float();
    let upper = floor > 0;
    if r < 0.07 + damage * 0.25 {
        return WindowState::Boarded;
    }
    if r < 0.2 + damage * 0.5 {
        return WindowState::Open;
    }
    if upper && r < 0.42 {
        return WindowState::Shuttered;
    }
    if upper && r < 0.52 {
        return WindowState::Ajar;
    }
    if r < 0.6 {
        return WindowState::Curtain;
    }
    if allow_lit && r < 0.66 {
        return WindowState::Lit;
    }
    WindowState::Glazed
}

/// `windowUnit(A, pm, o, rng, opts = {})`'s `opts` (`kit.js:148-159`). Rust
/// has no default arguments and no cross-field computed defaults (`state`
/// and `broken` are mutually derived in the source — see each field's doc);
/// callers resolve the source's defaulting formula themselves and pass the
/// resolved value. Defaults named here: `t=0.34`, `frame_key="wood_dark"`,
/// `depth=t*0.62`, `back=true`, `back_set=0.19`, `no_glass=false`,
/// `sill=true`, `lintel=true`, `grille=false`, `shutters=false`,
/// `shutter_key="metal_blue"`, `curtain=false`, `curtain_key="fabric_cream"`.
pub struct WindowOpts<'a> {
    pub t: f32,
    pub frame_key: &'a str,
    pub depth: f32,
    /// The source's `opts.state ?? (opts.broken ? 'open' : 'glazed')`
    /// (`kit.js:158`) — resolve this before calling.
    pub state: WindowState,
    /// The source's `opts.broken ?? state === 'open'` (`kit.js:159`).
    pub broken: bool,
    pub back: bool,
    pub back_set: f32,
    pub no_glass: bool,
    pub sill: bool,
    pub lintel: bool,
    pub grille: bool,
    pub shutters: bool,
    pub shutter_key: &'a str,
    pub curtain: bool,
    pub curtain_key: &'a str,
}

/// `windowUnit(A, pm, o, rng, opts = {})` (`kit.js:148-396`): a recessed
/// frame, mullions, glass (or blown out), stone sill, lintel, optional
/// shutters/grille/curtain — and a dark interior backing plane set back
/// behind the glass, which is what separates a window from a rectangle of
/// paint (see the source's own doc comment, `kit.js:136-147`).
pub fn window_unit<'a>(asm: &mut Assembler, pm: &Mat4, o: &'a WallHole, rng: &mut Rng, opts: WindowOpts) -> &'a WallHole {
    let (w, h, x, y) = (o.w, o.h, o.x, o.y);
    let fw = 0.055;
    let fd = 0.075;
    let state = opts.state;
    let broken = opts.broken;
    let boarded = state == WindowState::Boarded;
    let open = state == WindowState::Open || state == WindowState::Ajar;
    let lit = state == WindowState::Lit;
    let box_ = box_thin_kit(asm);

    // ---- the dark room behind the opening -------------------------------
    if opts.back {
        let bd = opts.depth + if open { 0.26 } else { opts.back_set };
        let pane = pane_kit(asm);
        let m = ll(pm, x, y, bd, 0.0, w + 0.14, h + 0.14, 1.0, 0.0, 0.0);
        asm.add(
            if lit { "window_glow" } else { "window_void" },
            &pane,
            Some(&m),
            Some(AccumAddOpts { masks: Some(if lit { [0.2, 0.4, 0.1] } else { [0.15, 0.9, 0.95] }), paint: None }),
        );
        if !boarded {
            let m1 = ll(pm, x - w / 2.0 - 0.03, y, bd - 0.09, 0.0, 0.05, h + 0.1, 0.2, 0.0, 0.0);
            asm.add("window_void", &box_, Some(&m1), Some(AccumAddOpts { masks: Some([0.1, 0.95, 1.0]), paint: None }));
            let m2 = ll(pm, x, y + h / 2.0 + 0.03, bd - 0.09, 0.0, w + 0.1, 0.05, 0.2, 0.0, 0.0);
            asm.add("window_void", &box_, Some(&m2), Some(AccumAddOpts { masks: Some([0.1, 0.95, 1.0]), paint: None }));
        }
    }

    // ---- frame: four members inside the reveal --------------------------
    let m = ll(pm, x, y + h / 2.0 - fw / 2.0, opts.depth, 0.0, w - 0.02, fw, fd, 0.0, 0.0);
    asm.add(opts.frame_key, &box_, Some(&m), None);
    let m = ll(pm, x, y - h / 2.0 + fw / 2.0, opts.depth, 0.0, w - 0.02, fw, fd, 0.0, 0.0);
    asm.add(opts.frame_key, &box_, Some(&m), None);
    let m = ll(pm, x - w / 2.0 + fw / 2.0, y, opts.depth, 0.0, fw, h - 0.02, fd, 0.0, 0.0);
    asm.add(opts.frame_key, &box_, Some(&m), None);
    let m = ll(pm, x + w / 2.0 - fw / 2.0, y, opts.depth, 0.0, fw, h - 0.02, fd, 0.0, 0.0);
    asm.add(opts.frame_key, &box_, Some(&m), None);

    // ---- mullion + transom, or swung-in casement leaves ------------------
    let mut open_l = false;
    let mut open_r = false;
    if !open {
        let m = ll(pm, x, y, opts.depth, 0.0, 0.045, h - 0.1, fd * 0.85, 0.0, 0.0);
        asm.add(opts.frame_key, &box_, Some(&m), None);
        let m = ll(pm, x, y + h * 0.16, opts.depth, 0.0, w - 0.1, 0.04, fd * 0.85, 0.0, 0.0);
        asm.add(opts.frame_key, &box_, Some(&m), None);
    } else {
        open_l = true;
        // `state === 'open' || rng.float() < 0.4` (`kit.js:203`) — a real
        // short-circuit: `rng.float()` is drawn only when `state != Open`.
        open_r = state == WindowState::Open || rng.float() < 0.4;
        let sw = w / 2.0 - 0.03;
        let sash = asm.cache(&format!("sash:{sw:.2}:{h:.2}"), || sash_leaf(sw, h - 0.06));
        let ry1 = rng.range(-1.35, -0.75) as f32;
        let m = ll(pm, x - w / 2.0 + 0.04, y, opts.depth + 0.02, ry1, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add(opts.frame_key, &sash, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.45, 0.2]), paint: None }));
        if open_r {
            let ry2 = rng.range(0.75, 1.35) as f32;
            let m = ll(pm, x + w / 2.0 - 0.04, y, opts.depth + 0.02, ry2, 1.0, 1.0, 1.0, 0.0, 0.0);
            asm.add(opts.frame_key, &sash, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.45, 0.2]), paint: None }));
        } else {
            let m = ll(pm, x, y, opts.depth, 0.0, 0.045, h - 0.1, fd * 0.85, 0.0, 0.0);
            asm.add(opts.frame_key, &box_, Some(&m), None);
        }
    }

    // ---- plywood board nailed over the opening ---------------------------
    if boarded {
        let n = rng.int(3, 5);
        for i in 0..n {
            let bh = (h + 0.05) / n as f32;
            let jx = rng.range(-0.04, 0.04) as f32;
            let jz = rng.range(-0.02, 0.02) as f32;
            let m = ll(
                pm,
                x + jx,
                y - (h + 0.05) / 2.0 + (i as f32 + 0.5) * bh,
                opts.depth - 0.05,
                0.0,
                w - 0.02,
                bh - 0.012,
                0.026,
                0.0,
                jz,
            );
            asm.add("plywood", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.75, 0.55, 0.25]), paint: None }));
        }
        if rng.float() < 0.5 {
            let m = ll(pm, x, y + h / 2.0 - 0.1, opts.depth - 0.08, 0.0, w * 0.5, 0.03, 0.02, 0.0, 0.0);
            asm.add("metal_rust", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.6, 0.0]), paint: None }));
        }
    }

    // ---- glass: four panes, some missing ---------------------------------
    if !opts.no_glass && !boarded {
        let panes = [
            (x - w / 4.0, y + h * 0.33, w / 2.0 - 0.09, h * 0.3),
            (x + w / 4.0, y + h * 0.33, w / 2.0 - 0.09, h * 0.3),
            (x - w / 4.0, y - h * 0.17, w / 2.0 - 0.09, h * 0.6),
            (x + w / 4.0, y - h * 0.17, w / 2.0 - 0.09, h * 0.6),
        ];
        let pane = pane_kit(asm);
        for (px, py, pw, ph) in panes {
            let skip_leaf = if px < x { open_l } else { open_r };
            if skip_leaf {
                continue;
            }
            if broken && rng.float() < 0.55 {
                continue;
            }
            let m = ll(pm, px, py, opts.depth, 0.0, pw, ph, 1.0, 0.0, 0.0);
            asm.add("window_glass", &pane, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.3, 0.0]), paint: None }));
        }
        if broken {
            for _ in 0..3 {
                let sw = rng.range(0.08, 0.26) as f32;
                let jx = rng.range(f64::from(-w / 2.0 + 0.1), f64::from(w / 2.0 - 0.1)) as f32;
                let ph_factor = rng.range(0.4, 0.9) as f32;
                let sz_factor = rng.range(0.6, 1.4) as f32;
                let rz = rng.range(-0.5, 0.5) as f32;
                let m = ll(pm, x + jx, y + h / 2.0 - sw * ph_factor, opts.depth, 0.0, sw, sw * sz_factor, 0.01, 0.0, rz);
                asm.add("glass", &box_, Some(&m), None);
            }
        }
    }

    // ---- stone sill, protruding and dripping dirt ------------------------
    if opts.sill {
        let soft = box_soft_kit(asm);
        let m = ll(pm, x, y - h / 2.0 - 0.045, -0.045, 0.0, w + 0.26, 0.09, opts.t * 0.55, 0.0, 0.0);
        asm.add("concrete", &soft, Some(&m), Some(AccumAddOpts { masks: Some([0.5, 0.35, 0.2]), paint: None }));
    }
    // ---- lintel -----------------------------------------------------------
    if opts.lintel {
        let hard = box_kit(asm);
        let m = ll(pm, x, y + h / 2.0 + 0.055, 0.02, 0.0, w + 0.18, 0.11, opts.t * 0.42, 0.0, 0.0);
        asm.add("concrete", &hard, Some(&m), Some(AccumAddOpts { masks: Some([0.35, 0.5, 0.3]), paint: None }));
    }

    // ---- metal grille on some ground-floor windows -----------------------
    if opts.grille {
        let bar = box_thin_kit(asm);
        let n = ((w / 0.16).round() as i32).max(3);
        for i in 0..n {
            let gx = x - w / 2.0 + 0.08 + (i as f32 / (n - 1) as f32) * (w - 0.16);
            let m = ll(pm, gx, y, 0.055, 0.0, 0.022, h - 0.06, 0.022, 0.0, 0.0);
            asm.add("metal_rust", &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.5, 0.0]), paint: None }));
        }
        for i in 0..2 {
            let m = ll(pm, x, y - h / 4.0 + (i as f32 * h) / 2.0, 0.055, 0.0, w - 0.05, 0.022, 0.022, 0.0, 0.0);
            asm.add("metal_rust", &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.8, 0.5, 0.0]), paint: None }));
        }
    }

    // ---- shutters: one closed, one hanging open at an angle ---------------
    if opts.shutters {
        let sw = w / 2.0 - 0.01;
        let louvre = asm.cache(&format!("shutter:{sw:.2}:{h:.2}"), || shutter_leaf(sw, h - 0.03));
        let shut = state == WindowState::Shuttered;
        // `shut ? false : rng.float() < 0.45` (`kit.js:343-344`): a real
        // short-circuit per leaf.
        let swung_l = !shut && rng.float() < 0.45;
        let swung_r = !shut && rng.float() < 0.45;
        let ry_l = if swung_l { rng.range(0.9, 1.5) as f32 } else { 0.0 };
        let m = ll(
            pm,
            x - w / 2.0 + if swung_l { 0.02 } else { sw / 2.0 },
            y,
            -0.03,
            ry_l,
            1.0,
            1.0,
            1.0,
            0.0,
            0.0,
        );
        asm.add(opts.shutter_key, &louvre, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.4, 0.0]), paint: None }));
        let ry_r = if swung_r { -(rng.range(0.9, 1.5) as f32) } else { 0.0 };
        let m = ll(
            pm,
            x + w / 2.0 - if swung_r { 0.02 } else { sw / 2.0 },
            y,
            -0.03,
            ry_r,
            1.0,
            1.0,
            1.0,
            0.0,
            0.0,
        );
        asm.add(opts.shutter_key, &louvre, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.4, 0.0]), paint: None }));
    }

    // ---- interior curtain / cloth, visible from the street ----------------
    if opts.curtain {
        let c = cloth_geometry(
            w * 0.92,
            h * 0.95,
            ClothOpts { seg_x: 7, seg_y: 7, sag: 0.05, wrinkle: 0.055, twist: 0.05, fray: 0.012, ..ClothOpts::default() },
            Some(rng),
        );
        let m = ll(pm, x + w * 0.03, y, opts.depth + 0.09, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add_once(opts.curtain_key, &c, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.35, 0.1]), paint: None }));
    }

    o
}

/// `sashLeaf(w, h)` (`kit.js:404-422`): a glazed casement leaf, hinged at its
/// LEFT edge (origin on the hinge) so it can be swung by a single Y
/// rotation.
fn sash_leaf(w: f32, h: f32) -> WorldGeo {
    let mut parts = Vec::new();
    push_box(&mut parts, 0.05, h, 0.032, 0.025, 0.0, 0.0, 0.0);
    push_box(&mut parts, 0.05, h, 0.032, w - 0.025, 0.0, 0.0, 0.0);
    push_box(&mut parts, w, 0.05, 0.032, w / 2.0, h / 2.0 - 0.025, 0.0, 0.0);
    push_box(&mut parts, w, 0.05, 0.032, w / 2.0, -h / 2.0 + 0.025, 0.0, 0.0);
    push_box(&mut parts, w - 0.08, 0.038, 0.026, w / 2.0, h * 0.14, 0.0, 0.0);
    // The pane, inset into the rebate — a thin sheet, not a solid slab.
    push_box(&mut parts, w - 0.09, h - 0.09, 0.008, w / 2.0, 0.0, -0.006, 0.0);
    merge_simple(&parts)
}

/// `shutterLeaf(w, h)` (`kit.js:425-453`): a louvred shutter leaf, origin at
/// leaf centre.
fn shutter_leaf(w: f32, h: f32) -> WorldGeo {
    let mut parts = Vec::new();
    push_box(&mut parts, 0.05, h, 0.035, -w / 2.0 + 0.025, 0.0, 0.0, 0.0);
    push_box(&mut parts, 0.05, h, 0.035, w / 2.0 - 0.025, 0.0, 0.0, 0.0);
    push_box(&mut parts, w, 0.05, 0.035, 0.0, h / 2.0 - 0.025, 0.0, 0.0);
    push_box(&mut parts, w, 0.05, 0.035, 0.0, -h / 2.0 + 0.025, 0.0, 0.0);
    push_box(&mut parts, w, 0.05, 0.035, 0.0, 0.0, 0.0, 0.0);
    let slats = (((h - 0.16) / 0.115).floor() as i32).max(4);
    for i in 0..slats {
        let y = -h / 2.0 + 0.09 + (i as f32 / (slats - 1) as f32) * (h - 0.18);
        if y.abs() < 0.04 {
            continue;
        }
        push_box(&mut parts, w - 0.08, 0.06, 0.014, 0.0, y, 0.006, -0.5);
    }
    merge_simple(&parts)
}

/// The shared `push(sx, sy, sz, x, y, z, rx = 0)` closure both `sashLeaf`
/// (`kit.js:406-411`, always `rx = 0`) and `shutterLeaf` (`kit.js:427-438`,
/// `rx` for the tilted slats) build: a `plainBox()` scaled to `(sx, sy, sz)`,
/// then rotated about X by `rx` and translated to `(x, y, z)` — the rotation
/// is single-axis, so the Euler order that would otherwise distinguish
/// `kit.js`'s own `'YXZ'`-order scratch objects from any other order is
/// irrelevant here (only one angle is ever nonzero).
#[allow(clippy::too_many_arguments)]
fn push_box(parts: &mut Vec<WorldGeo>, sx: f32, sy: f32, sz: f32, x: f32, y: f32, z: f32, rx: f32) {
    let mut g = plain_box();
    g.apply(&Mat4::scale(Vec3::new(sx, sy, sz)));
    g.apply(&trs(x, y, z, 0.0, 1.0, 1.0, 1.0, rx, 0.0));
    parts.push(g);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_state_low_roll_is_always_boarded_regardless_of_floor() {
        // r < 0.07 + damage*0.25 unconditionally returns Boarded first,
        // ahead of every other arm.
        let seed = (0..200u32)
            .find(|&seed| Rng::new(seed).float() < 0.07)
            .expect("expected at least one seed under 200 to roll < 0.07");
        assert_eq!(window_state(&mut Rng::new(seed), 1, 0.0, true), WindowState::Boarded);
        assert_eq!(window_state(&mut Rng::new(seed), 0, 0.0, true), WindowState::Boarded);
    }

    #[test]
    fn window_state_ground_floor_never_returns_shuttered_or_ajar() {
        for seed in 0..500u32 {
            let mut rng = Rng::new(seed);
            let state = window_state(&mut rng, 0, 0.2, true);
            assert_ne!(state, WindowState::Shuttered);
            assert_ne!(state, WindowState::Ajar);
        }
    }

    #[test]
    fn window_state_allow_lit_false_never_returns_lit() {
        for seed in 0..500u32 {
            let mut rng = Rng::new(seed);
            let state = window_state(&mut rng, 1, 0.2, false);
            assert_ne!(state, WindowState::Lit);
        }
    }

    #[test]
    fn window_unit_glazed_produces_frame_sill_lintel_and_glass_batches() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let o = WallHole { x: 0.0, y: 1.5, w: 1.0, h: 1.4, arch: 0.0, ragged: 0.0 };
        window_unit(
            &mut asm,
            &Mat4::IDENTITY,
            &o,
            &mut rng,
            WindowOpts {
                t: 0.34,
                frame_key: "wood_dark",
                depth: 0.34 * 0.62,
                state: WindowState::Glazed,
                broken: false,
                back: true,
                back_set: 0.19,
                no_glass: false,
                sill: true,
                lintel: true,
                grille: false,
                shutters: false,
                shutter_key: "metal_blue",
                curtain: false,
                curtain_key: "fabric_cream",
            },
        );
        let result = asm.finalize();
        let keys: Vec<&str> = result.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"window_void"));
        assert!(keys.contains(&"wood_dark"));
        assert!(keys.contains(&"concrete"));
        assert!(keys.contains(&"window_glass"));
    }

    #[test]
    fn window_unit_boarded_never_emits_glass_or_void_batches() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        let o = WallHole { x: 0.0, y: 1.5, w: 1.0, h: 1.4, arch: 0.0, ragged: 0.0 };
        window_unit(
            &mut asm,
            &Mat4::IDENTITY,
            &o,
            &mut rng,
            WindowOpts {
                t: 0.34,
                frame_key: "wood_dark",
                depth: 0.34 * 0.62,
                state: WindowState::Boarded,
                broken: false,
                back: true,
                back_set: 0.19,
                no_glass: false,
                sill: true,
                lintel: true,
                grille: false,
                shutters: false,
                shutter_key: "metal_blue",
                curtain: false,
                curtain_key: "fabric_cream",
            },
        );
        let result = asm.finalize();
        let keys: Vec<&str> = result.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(!keys.contains(&"window_glass"));
        assert!(keys.contains(&"plywood"));
    }

    #[test]
    fn sash_leaf_and_shutter_leaf_produce_non_empty_indexed_geometry() {
        let sash = sash_leaf(0.4, 1.2);
        assert!(sash.vert_count() > 0);
        assert!(sash.tri_count() > 0);
        let shutter = shutter_leaf(0.4, 1.2);
        assert!(shutter.vert_count() > 0);
        assert!(shutter.tri_count() > 0);
    }
}
