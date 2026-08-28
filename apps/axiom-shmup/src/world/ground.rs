//! Ported from Claude-of-Duty `src/world/ground.js:1-287` — the ground
//! plane, road, kerbs and everything wind piles against them.
//!
//! The road is a cambered, subdivided strip so it catches grazing sunlight;
//! the pavements are individual slabs with gaps, broken corners and sand
//! drifts, and the alleys are dirt. Collision is a handful of flat boxes
//! rather than the visual triangles, which keeps the BVH tiny.
//!
//! ## `Math.round`'s half-up quirk
//!
//! Every `Math.round` in this file (`Math.round(roadLen / 2)`,
//! `Math.round(len / 1.15)` in [`seam`]) rounds a value built from an
//! irrational division (`f64::sin`/`cos`/a non-terminating fraction) — see
//! `crate::world::noise`'s `round_half_up` doc for the JS-vs-Rust rounding
//! divergence at an exact `.5` boundary. None of these inputs can land
//! exactly on `.5` in practice, so plain [`f64::round`] (half-away-from-zero)
//! is used directly rather than threading a second copy of `round_half_up`
//! through this file.
//!
//! ## Surface vs. palette key
//!
//! `Assembler.box(surface, ...)` (`builder.js:275`) takes a bare bucket-key
//! *string* in the source — every real call site in `ground.js` happens to
//! pass a string that already spells a valid [`Surface`] name (`'dirt'`,
//! `'concrete'`) or the result of `A.surfaceOf(...)`. This port's
//! [`Assembler::collide_box`]/[`Assembler::collide_geo`] are typed on
//! [`Surface`] directly rather than an arbitrary string, so every such call
//! site below passes the `Surface` variant the JS string would have resolved
//! to — a stronger, equivalent translation, not a behavioural change.
//!
//! `CylinderGeometry` (the manhole ring) used to be ported here, as a
//! private copy, because it had exactly one caller in this whole port. It is
//! now promoted to `crate::world::kit::cylinder_geometry`: `kit.js`'s
//! `tubeY` is the second caller that copy's own doc anticipated ("if a
//! second caller arrives, promote it there").

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;
use crate::world::geo::WorldGeo;
use crate::world::kit::{chamfer_box, cylinder_geometry, patch_geometry, plane_geometry, trs};
use crate::world::layout::{road_y, ALLEYS, STREET};
use crate::world::noise::fbm3;
use crate::world::palette::Surface;

/// **How far a flat decal sits above the surface it is painted on**
/// (`ground.js:15-24`).
///
/// Two millimetres: enough to beat depth precision at this scale, little
/// enough that the decal reads as painted on rather than laid over. The old
/// values here were 42-50 mm, which is a hand's width of air under a dust
/// patch — invisible when the road was covered in rubble, and a floating disc
/// once it was not.
pub const DECAL_LIFT: f64 = 0.002;

/// `BOX`/`BOX_SOFT` (`kit.js:54,56`) — cached chamfered-box providers.
/// `kit.js`'s modular building kit is out of this port's scope (see
/// `docs/work-manifests/shmup-port/02-port-recipe.md`'s task), but
/// `buildGround` needs these two one-line cache wrappers around
/// [`chamfer_box`] (already ported, `crate::world::kit`), so they are
/// inlined here rather than pulled in through a whole unported module.
fn box_kit(asm: &mut Assembler) -> WorldGeo {
    asm.cache("box:0.012", || chamfer_box(1.0, 1.0, 1.0, 0.012))
}
fn box_soft_kit(asm: &mut Assembler) -> WorldGeo {
    asm.cache("box:0.03", || chamfer_box(1.0, 1.0, 1.0, 0.03))
}

/// `buildGround(A, rng)` (`ground.js:15-286`).
pub fn build_ground(asm: &mut Assembler, rng: &mut Rng) {
    let hw = STREET.half_width;
    let kb = STREET.kerb;
    let wh = STREET.walk_h;
    let z_min = STREET.z_min;
    let z_max = STREET.z_max;

    // ------------------------------------------------------------- terrain --
    let s = 168.0f32;
    let n = 42u32;
    let mut terrain = plane_geometry(s, s, n, n);
    terrain.rotate_x(-std::f32::consts::FRAC_PI_2);
    for i in 0..terrain.vert_count() {
        let x = terrain.pos[i * 3];
        let z = terrain.pos[i * 3 + 2];
        let in_street = f64::from(x).abs() < kb + 1.0 && f64::from(z) > z_min && f64::from(z) < z_max;
        let h = if in_street {
            0.0
        } else {
            (fbm3(f64::from(x) * 0.045, 7.3, f64::from(z) * 0.045, 3) - 0.5) * 1.1 + 0.02
        };
        terrain.pos[i * 3 + 1] = (h - 0.03) as f32;
    }
    terrain.compute_vertex_normals();
    terrain.paint_masks(|x, _y, z, _nx, _ny, _nz, out, _i| {
        out[1] = 0.25 + fbm3(f64::from(x) * 0.3, 1.1, f64::from(z) * 0.3, 2) as f32 * 0.4;
        out[0] = 0.2;
    });
    asm.add("sand", &terrain, None, None);
    asm.collide_geo(Surface::Sand, &terrain, None);

    // ---------------------------------------------------------------- road --
    let road_len = z_max - z_min;
    let road_height_segments = (road_len / 2.0).round().max(1.0) as u32;
    let mut road = plane_geometry((hw * 2.0) as f32, road_len as f32, 12, road_height_segments);
    road.rotate_x(-std::f32::consts::FRAC_PI_2);
    for i in 0..road.vert_count() {
        let x = f64::from(road.pos[i * 3]);
        let z = f64::from(road.pos[i * 3 + 2]);
        // `roadY(x)` (`ground.js:60`) — the same value the hand-written
        // formula produced, now from the one definition.
        let camber = road_y(x, 0.0);
        let wear = (fbm3(x * 0.55 + 3.0, 2.2, z * 0.35, 3) - 0.5) * 0.07;
        let rut = -((-((x.abs() - 1.6).powi(2)) / 0.5).exp()) * 0.022;
        road.pos[i * 3 + 1] = (camber + wear + rut) as f32;
    }
    road.compute_vertex_normals();
    road.paint_masks(|x, _y, z, _nx, _ny, _nz, out, _i| {
        let nn = fbm3(f64::from(x) * 0.7, 4.4, f64::from(z) * 0.7, 3) as f32;
        out[1] = 0.1 + (0.0f32.max((x.abs() - 3.4) / 1.5)) * 0.35 + nn * 0.18;
        out[0] = 0.2 + nn * 0.3;
    });
    road.translate(0.0, 0.0, ((z_min + z_max) / 2.0) as f32);
    asm.add("road_dust", &road, None, None);
    asm.collide_box(Surface::Dirt, 0.0, -0.2, ((z_min + z_max) / 2.0) as f32, (hw * 2.0) as f32, 0.42, road_len as f32, 0.0);

    // Old tarmac showing through the dust where wheels have polished it.
    for _ in 0..30 {
        let rut = rng.float() < 0.62;
        let x = if rut {
            let sign = if rng.float() < 0.5 { -1.0 } else { 1.0 };
            sign * rng.range(1.2, 2.1)
        } else {
            rng.range(-hw + 0.5, hw - 0.5)
        };
        let z = rng.range(z_min + 2.0, z_max - 2.0);
        // `roadY(x, DECAL_LIFT)` (`ground.js:84`), was `+ 0.042`.
        let camber = road_y(x, DECAL_LIFT);
        let radius = rng.range(0.45, 1.1);
        let g = patch_geometry(rng, radius, 11, 0.5, 0.0);
        let ry = rng.float() as f32 * 0.4;
        let sz = if rut { rng.range(2.0, 4.5) } else { rng.range(0.7, 1.4) };
        let m = trs(x as f32, camber as f32, z as f32, ry, 1.0, 1.0, sz as f32, 0.0, 0.0);
        asm.add_once("asphalt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.35, 0.25, 0.1]), paint: None }));
    }

    // ------------------------------------------------------- pavement slabs --
    for &side in &[-1.0f64, 1.0] {
        let mut z = z_min;
        while z < z_max {
            let seg_len = rng.range(3.2, 6.5);
            let gap = if rng.float() < 0.12 { rng.range(0.6, 1.6) } else { 0.06 };
            let mut mouth = false;
            for alley in ALLEYS {
                let in_x = if side > 0.0 { alley.x0 >= kb - 0.5 } else { alley.x1 <= -kb + 0.5 };
                if in_x && z + seg_len > alley.z0 - 0.2 && z < alley.z1 + 0.2 {
                    mouth = true;
                }
            }
            let cz = z + seg_len / 2.0;
            let cx = side * (kb + hw) / 2.0;
            let w_slab = kb - hw;
            if !mouth {
                let h = wh + rng.range(-0.012, 0.012);
                let box_soft = box_soft_kit(asm);
                let m1 = trs(cx as f32, (h / 2.0) as f32, cz as f32, 0.0, (w_slab - 0.05) as f32, h as f32, (seg_len - gap) as f32, 0.0, 0.0);
                asm.add("concrete", &box_soft, Some(&m1), Some(AccumAddOpts { masks: Some([0.6, 0.45, 0.2]), paint: None }));
                let m2 = trs((side * (hw + 0.11)) as f32, ((h + 0.022) / 2.0) as f32, cz as f32, 0.0, 0.22, (h + 0.022) as f32, (seg_len - gap) as f32, 0.0, 0.0);
                asm.add("concrete", &box_soft, Some(&m2), Some(AccumAddOpts { masks: Some([0.95, 0.35, 0.1]), paint: None }));
                asm.collide_box(Surface::Concrete, cx as f32, (h / 2.0) as f32, cz as f32, w_slab as f32, h as f32, (seg_len - gap * 0.5) as f32, 0.0);
                asm.collide_box(Surface::Concrete, (side * (hw + 0.11)) as f32, ((h + 0.022) / 2.0) as f32, cz as f32, 0.24, (h + 0.022) as f32, (seg_len - gap * 0.5) as f32, 0.0);
                if rng.float() < 0.5 {
                    let radius = rng.range(0.25, 0.7);
                    let g = patch_geometry(rng, radius, 9, 0.6, 0.0);
                    let x_off = rng.range(-0.5, 0.5);
                    let z_off = rng.range(-1.0, 1.0);
                    let ry = rng.float() as f32 * 6.28;
                    let m = trs((cx + x_off) as f32, (h + 0.006) as f32, (cz + z_off) as f32, ry, 1.0, 1.0, 1.0, 0.0, 0.0);
                    asm.add_once("concrete", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 1.0, 0.55]), paint: None }));
                }
            } else {
                let box_ = box_kit(asm);
                let m = trs(cx as f32, 0.035, cz as f32, 0.0, w_slab as f32, 0.07, seg_len as f32, 0.0, 0.0);
                asm.add("dirt", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.2, 0.7, 0.4]), paint: None }));
                asm.collide_box(Surface::Dirt, cx as f32, 0.03, cz as f32, w_slab as f32, 0.06, seg_len as f32, 0.0);
            }
            z += seg_len + gap;
        }
    }

    // ------------------------------------------------------------- alleys --
    for alley in ALLEYS {
        let w = alley.x1 - alley.x0;
        let d = alley.z1 - alley.z0;
        let box_ = box_kit(asm);
        let m = trs(((alley.x0 + alley.x1) / 2.0) as f32, 0.03, ((alley.z0 + alley.z1) / 2.0) as f32, 0.0, w as f32, 0.06, d as f32, 0.0, 0.0);
        asm.add(alley.surface, &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.2, 0.6, 0.35]), paint: None }));
        let surface = asm.surface_of(alley.surface);
        asm.collide_box(surface, ((alley.x0 + alley.x1) / 2.0) as f32, 0.02, ((alley.z0 + alley.z1) / 2.0) as f32, w as f32, 0.05, d as f32, 0.0);
    }

    // --------------------------------------------------------- material seams --
    let mut sr = Rng::new(0x5ea3_1d);
    seam(asm, &mut sr, -hw + 0.08, z_min + 2.0, -hw + 0.08, z_max - 2.0, "sand", "road_dust", 0.012);
    seam(asm, &mut sr, hw - 0.08, z_min + 2.0, hw - 0.08, z_max - 2.0, "sand", "road_dust", 0.012);
    seam(asm, &mut sr, -kb, z_min + 2.0, -kb, z_max - 2.0, "concrete", "sand", wh + 0.004);
    seam(asm, &mut sr, kb, z_min + 2.0, kb, z_max - 2.0, "concrete", "sand", wh + 0.004);
    for alley in ALLEYS {
        let ay = 0.062;
        seam(asm, &mut sr, alley.x0, alley.z0, alley.x1, alley.z0, alley.surface, "sand", ay);
        seam(asm, &mut sr, alley.x0, alley.z1, alley.x1, alley.z1, alley.surface, "sand", ay);
        seam(asm, &mut sr, alley.x0, alley.z0, alley.x0, alley.z1, alley.surface, "sand", ay);
        seam(asm, &mut sr, alley.x1, alley.z0, alley.x1, alley.z1, alley.surface, "sand", ay);
    }

    // ------------------------------------------- drifts, stains and covers --
    for _ in 0..130 {
        let side = if rng.float() < 0.5 { -1.0 } else { 1.0 };
        let against_wall = rng.float() < 0.55;
        let x = if against_wall {
            side * (STREET.kerb - rng.range(0.05, 0.9))
        } else {
            side * (hw + rng.range(-0.35, 0.5))
        };
        let z = rng.range(z_min + 2.0, z_max - 2.0);
        let y = if against_wall {
            wh + 0.012
        } else if x.abs() < hw {
            // `roadY(x, DECAL_LIFT)` (`ground.js:254`), was `+ 0.05`.
            road_y(x, DECAL_LIFT)
        } else {
            wh + 0.01
        };
        let radius = rng.range(0.35, 1.5);
        let g = patch_geometry(rng, radius, 9, 0.5, 0.0);
        let ry = rng.float() as f32 * 6.28;
        let sz = rng.range(0.5, 1.0);
        let m = trs(x as f32, y as f32, z as f32, ry, 1.0, 1.0, sz as f32, 0.0, 0.0);
        asm.add_once("sand", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.15, 0.5, 0.3]), paint: None }));
    }
    for _ in 0..26 {
        let radius = rng.range(0.5, 1.8);
        let g = patch_geometry(rng, radius, 10, 0.6, 0.0);
        let px = rng.range(-hw + 0.4, hw - 0.4);
        let z = rng.range(z_min + 3.0, z_max - 3.0);
        let ry = rng.float() as f32 * 6.28;
        let sz = rng.range(0.4, 0.9);
        // `roadY(px, DECAL_LIFT)` (`ground.js:267`), was `+ 0.048`.
        let y = road_y(px, DECAL_LIFT);
        let m = trs(px as f32, y as f32, z as f32, ry, 1.0, 1.0, sz as f32, 0.0, 0.0);
        asm.add_once("dirt", &g, Some(&m), Some(AccumAddOpts { masks: Some([0.1, 0.85, 0.5]), paint: None }));
    }

    // manholes and gully gratings.
    for _ in 0..7 {
        let z = rng.range(z_min + 6.0, z_max - 6.0);
        let x = rng.range(-2.5, 2.5);
        let ring = asm.cache("manhole", || {
            let mut g = cylinder_geometry(0.36, 0.36, 0.04, 18, 1, false);
            g.paint_masks(|_x, _y, _z, _nx, ny, _nz, out, _i| {
                out[0] = if ny > 0.5 { 0.95 } else { 0.4 };
                out[1] = 0.55;
            });
            g
        });
        let ry = rng.float() as f32 * 6.28;
        // Seated, not sitting on top (`ground.js:283-286`): the cylinder is
        // 4 cm thick and centred, so placing its CENTRE 1.2 cm below the
        // surface leaves 8 mm of rim proud and buries the rest. It used to be
        // centred 3.5 cm ABOVE the road — and on a DIFFERENT camber
        // coefficient (0.05 against the road's 0.055), so it also drifted off
        // the crown across the road's width.
        let m = trs(x as f32, road_y(x, -0.012) as f32, z as f32, ry, 1.0, 1.0, 1.0, 0.0, 0.0);
        asm.add("metal_dark", &ring, Some(&m), None);
    }
    for &side in &[-1.0f64, 1.0] {
        for _ in 0..5 {
            let z = rng.range(z_min + 8.0, z_max - 8.0);
            let box_ = box_kit(asm);
            let m = trs((side * (hw - 0.22)) as f32, (wh - 0.03) as f32, z as f32, 0.0, 0.42, 0.05, 0.62, 0.0, 0.0);
            asm.add("metal_dark", &box_, Some(&m), Some(AccumAddOpts { masks: Some([0.7, 0.8, 0.6]), paint: None }));
        }
    }
}

/// `seam(ax, az, bx, bz, keyA, keyB, y)` (`ground.js:158-209`): every place
/// two ground materials meet is otherwise a razor-straight polygon edge, so
/// each boundary gets a scatter of irregular patches of BOTH materials
/// straddling it, plus a line of loose stones. Uses its own fixed-seed
/// stream (`ground.js:157`) so the seam scatter never shifts the draw
/// sequence the rest of the level's placement depends on.
#[allow(clippy::too_many_arguments)]
fn seam(asm: &mut Assembler, sr: &mut Rng, ax: f64, az: f64, bx: f64, bz: f64, key_a: &str, key_b: &str, y: f64) {
    let len = (bx - ax).hypot(bz - az);
    let n = (len / 1.15).round().max(6.0) as u32;
    let tx = (bx - ax) / len;
    let tz = (bz - az) / len;
    let nxs = -tz;
    let nzs = tx;
    for i in 0..n {
        let t = ((f64::from(i) + sr.range(0.15, 0.85)) / f64::from(n)) * len;
        let px = ax + tx * t;
        let pz = az + tz * t;
        for (key, side) in [(key_a, -1.0f64), (key_b, 1.0f64)] {
            if sr.float() < 0.22 {
                continue;
            }
            let off = side * sr.range(-0.12, 0.62);
            let radius = sr.range(0.3, 0.62);
            let g = patch_geometry(sr, radius, 10, 0.6, 0.0);
            let y_jitter = sr.range(0.0, 0.004);
            let ry = sr.float() as f32 * 6.28;
            let sz = sr.range(0.55, 1.0);
            let m = trs((px + nxs * off) as f32, (y + 0.006 + y_jitter) as f32, (pz + nzs * off) as f32, ry, 1.0, 1.0, sz as f32, 0.0, 0.0);
            let mask_g = sr.range(0.3, 0.8) as f32;
            let mask_b = sr.range(0.2, 0.5) as f32;
            asm.add_once(key, &g, Some(&m), Some(AccumAddOpts { masks: Some([0.15, mask_g, mask_b]), paint: None }));
        }
        if asm.has("rock_b") {
            let k_count = sr.int(1, 3);
            for _ in 0..k_count {
                let off = sr.range(-0.55, 0.55);
                let id = if sr.float() < 0.68 { "rock_b" } else { "rock_a" };
                let x = px + nxs * off + sr.range(-0.2, 0.2);
                let z = pz + nzs * off + sr.range(-0.2, 0.2);
                let ry = sr.float() as f32 * 6.28;
                let s = sr.range(0.45, 1.0);
                let mask_g = sr.range(1.0, 1.5) as f32;
                asm.put(id, x as f32, (y + 0.01) as f32, z as f32, ry, s as f32, Some([1.0, mask_g, 1.0]), 0.0, 0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ground_produces_a_non_trivial_scene() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        build_ground(&mut asm, &mut rng);
        let result = asm.finalize();
        assert!(!result.statics.is_empty());
        assert!(!result.collision.is_empty());
        assert!(result.stats.static_tris > 0);
        assert!(result.stats.collide_tris > 0);
    }

    #[test]
    fn road_camber_peaks_at_the_crown_and_vanishes_at_the_kerb() {
        let hw = STREET.half_width;
        assert!((road_y(0.0, 0.0) - 0.055).abs() < 1e-12);
        assert!(road_y(hw, 0.0).abs() < 1e-9);
        assert!(road_y(0.0, 0.0) > road_y(hw / 2.0, 0.0));
    }

    /// The decal lift is 2 mm, not the 42-50 mm the pre-policy code used, and
    /// it is the SAME lift at every road-decal site (`ground.js:15-24`).
    #[test]
    fn a_road_decal_sits_two_millimetres_above_the_road_surface() {
        for x in [-4.0f64, -1.3, 0.0, 2.2, 4.4] {
            assert!((road_y(x, DECAL_LIFT) - road_y(x, 0.0) - 0.002).abs() < 1e-15);
        }
    }

    /// A manhole cover is SEATED: its centre is 1.2 cm below the road, so
    /// 8 mm of a 4 cm-thick ring stands proud (`ground.js:283-286`). The old
    /// placement centred it 3.5 cm above the road — a floating puck.
    #[test]
    fn a_manhole_cover_is_seated_below_the_road_not_floating_above_it() {
        let half_thickness = 0.04 / 2.0;
        for x in [-2.5f64, 0.0, 2.5] {
            let centre = road_y(x, -0.012);
            assert!(centre < road_y(x, 0.0), "the cover's centre is under the road");
            let proud = centre + half_thickness - road_y(x, 0.0);
            assert!((proud - 0.008).abs() < 1e-15, "8 mm of rim proud, got {proud}");
        }
    }

    #[test]
    fn seam_with_a_zero_probability_boundary_still_terminates() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut sr = Rng::new(0x5ea3_1d);
        seam(&mut asm, &mut sr, 0.0, 0.0, 1.0, 0.0, "sand", "dirt", 0.0);
        let result = asm.finalize();
        assert!(!result.statics.is_empty());
    }
}
