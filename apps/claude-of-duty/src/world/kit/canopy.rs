//! Ported from Claude-of-Duty `src/world/kit.js:836-916` — `stripedCloth`
//! (a continuous catenary surface split into alternating colour strips) and
//! `awning` (a sloped canopy built from it, plus a steel frame).

use axiom_math::Mat4;

use crate::rng::Rng;
use crate::world::accum::AccumAddOpts;
use crate::world::assembler::Assembler;

use super::{box_thin_kit, cloth_geometry, ll, ClothOpts};

/// `stripedCloth(A, keys, m, w, h, opts = {})`'s `opts` (`kit.js:836-845`).
/// Rust has no default arguments; the source's defaults are named per
/// field. `bands`/`seg_x` are cross-field-defaulted in the source
/// (`bands ?? max(3, round(w/0.38))`, `segX ?? max(2, round(24/bands))`) —
/// resolve those formulas at the call site before constructing this.
pub struct StripedClothOpts {
    pub bands: u32,
    pub seg_x: u32,
    pub seg_y: u32,
    pub sag: f32,
    pub wrinkle: f32,
    pub bulge: f32,
    pub twist: f32,
    pub thickness: f32,
    pub hem: f32,
    pub fray: f32,
    /// `opts.skipBand ?? -1` (`kit.js:844`) — tears one strip out of an old
    /// tarp. `-1` (the default) never matches any real band index.
    pub skip_band: i32,
    pub masks: [f32; 3],
}

impl Default for StripedClothOpts {
    /// The subset of the source's defaults that don't depend on `w`
    /// (`bands`/`seg_x` still must be resolved by the caller — see the
    /// struct doc).
    fn default() -> Self {
        StripedClothOpts {
            bands: 3,
            seg_x: 8,
            seg_y: 6,
            sag: 0.14,
            wrinkle: 0.03,
            bulge: 0.04,
            twist: 0.0,
            thickness: 0.0024,
            hem: 1.0,
            fray: 0.0,
            skip_band: -1,
            masks: [0.3, 0.5, 0.15],
        }
    }
}

/// `stripedCloth`'s `bands`/`segX` default formulas (`kit.js:837,840`),
/// exposed so callers (including [`awning`]) can resolve them exactly as the
/// source does.
pub fn striped_cloth_default_bands(w: f32) -> u32 {
    ((w / 0.38).round() as i32).max(3) as u32
}

/// See [`striped_cloth_default_bands`].
pub fn striped_cloth_default_seg_x(bands: u32) -> u32 {
    ((24.0 / bands as f32).round() as i32).max(2) as u32
}

/// `stripedCloth(A, keys, m, w, h, opts = {})` (`kit.js:836-869`): splits one
/// continuous catenary cloth surface into `opts.bands` alternating-colour
/// strips sharing a single `seed` (drawn once, before the loop, exactly
/// once regardless of how many bands are skipped) — see the source's own
/// comment: a single flat colour is the fastest way to make fabric read as
/// a tarpaulin.
pub fn striped_cloth(asm: &mut Assembler, keys: &[&str], m: &Mat4, w: f32, h: f32, opts: StripedClothOpts, rng: Option<&mut Rng>) {
    let mut rng = rng;
    // `(rng ?? {float:()=>0.5}).float()*30` (`kit.js:839`): a real rng draw
    // when present, else the fixed fallback `0.5`.
    let seed = rng.as_mut().map_or(0.5, |r| r.float() as f32) * 30.0;
    for i in 0..opts.bands {
        if i as i32 == opts.skip_band {
            continue;
        }
        let u0 = i as f32 / opts.bands as f32;
        let u1 = (i + 1) as f32 / opts.bands as f32;
        let g = cloth_geometry(
            w,
            h,
            ClothOpts {
                seg_x: opts.seg_x,
                seg_y: opts.seg_y,
                sag: opts.sag,
                wrinkle: opts.wrinkle,
                bulge: opts.bulge,
                twist: opts.twist,
                thickness: opts.thickness,
                hem: opts.hem,
                fray: opts.fray,
                u_range: Some((u0, u1)),
                seed: Some(seed),
                bow: 1.0,
            },
            None,
        );
        // Per-band weathering: sewn-together strips never age at the same
        // rate. `rng ? rng.range(0.85, 1.18) : 1` (`kit.js:864`).
        let gv = rng.as_mut().map_or(1.0, |r| r.range(0.85, 1.18) as f32);
        let masks = [opts.masks[0], (opts.masks[1] * gv).min(1.0), opts.masks[2]];
        asm.add_once(keys[i as usize % keys.len()], &g, Some(m), Some(AccumAddOpts { masks: Some(masks), paint: None }));
    }
}

/// `awning(A, pm, x, y, w, rng, opts = {})`'s `opts` (`kit.js:873-878,906`).
/// Defaults: `depth=1.5`, `key="fabric_red"`, `slope=0.32`,
/// `keys=[key, key2]`, `key2="fabric_cream"`, `legs=false`.
pub struct AwningOpts<'a> {
    pub depth: f32,
    pub slope: f32,
    pub keys: [&'a str; 2],
    pub legs: bool,
}

/// `awning(...)`'s return shape (`kit.js:915`, `{x, y, w, d}`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AwningResult {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub d: f32,
}

/// `awning(A, pm, x, y, w, rng, opts = {})` (`kit.js:873-916`): a fabric
/// awning over a shopfront — sloped cloth (tipped to `opts.slope`), a
/// scalloped/frayed valance hanging off the front edge, and a steel frame.
pub fn awning(asm: &mut Assembler, pm: &Mat4, x: f32, y: f32, w: f32, rng: &mut Rng, opts: AwningOpts) -> AwningResult {
    let d = opts.depth;
    let slope = opts.slope;
    let slack = rng.range(0.85, 1.45) as f32;

    let bands = striped_cloth_default_bands(w);
    let seg_x = striped_cloth_default_seg_x(bands);

    let m1 = ll(pm, x, y - slope * 0.5, -d / 2.0, 0.0, 1.0, 1.0, 1.0, -std::f32::consts::FRAC_PI_2 + slope, 0.0);
    striped_cloth(
        asm,
        &opts.keys,
        &m1,
        w,
        d,
        StripedClothOpts { bands, seg_x, seg_y: 6, sag: 0.11 * slack, wrinkle: 0.026 * slack, bulge: 0.055 * slack, thickness: 0.0026, masks: [0.2, 0.45, 0.15], ..StripedClothOpts::default() },
        Some(rng),
    );
    let m2 = ll(pm, x, y - slope - 0.13, -d, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0);
    striped_cloth(
        asm,
        &opts.keys,
        &m2,
        w,
        0.26,
        StripedClothOpts { bands, seg_x, seg_y: 3, sag: 0.05 * slack, wrinkle: 0.026 * slack, bulge: 0.0, thickness: 0.0026, fray: 0.018, masks: [0.3, 0.5, 0.2], ..StripedClothOpts::default() },
        Some(rng),
    );

    let bar = box_thin_kit(asm);
    for sx in [-1.0f32, 1.0] {
        let m = ll(pm, x + sx * (w / 2.0 - 0.05), y - slope * 0.5, -d / 2.0, 0.0, 0.04, 0.04, d, -slope.atan2(d), 0.0);
        asm.add("metal_rust", &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.0]), paint: None }));
        if opts.legs {
            let m = ll(pm, x + sx * (w / 2.0 - 0.05), (y - slope) / 2.0, -d + 0.05, 0.0, 0.045, y - slope, 0.045, 0.0, 0.0);
            asm.add("metal_rust", &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.55, 0.0]), paint: None }));
        }
    }
    let m = ll(pm, x, y - slope, -d + 0.03, 0.0, w, 0.04, 0.04, 0.0, 0.0);
    asm.add("metal_rust", &bar, Some(&m), Some(AccumAddOpts { masks: Some([0.9, 0.5, 0.0]), paint: None }));

    AwningResult { x, y, w, d }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn striped_cloth_default_bands_and_seg_x_formulas() {
        assert_eq!(striped_cloth_default_bands(1.0), 3); // max(3, round(1/0.38)=3)
        assert_eq!(striped_cloth_default_bands(3.0), 8); // round(3/0.38)=8
        assert_eq!(striped_cloth_default_seg_x(3), 8); // round(24/3)=8
        assert_eq!(striped_cloth_default_seg_x(24), 2); // max(2, round(24/24)=1)
    }

    #[test]
    fn striped_cloth_emits_one_batch_per_non_skipped_band() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(2);
        striped_cloth(
            &mut asm,
            &["fabric_red", "fabric_cream"],
            &Mat4::IDENTITY,
            2.0,
            1.0,
            StripedClothOpts { bands: 4, seg_x: 4, skip_band: 1, ..StripedClothOpts::default() },
            Some(&mut rng),
        );
        let out = asm.finalize();
        // 4 bands, skipping index 1: 3 addOnce calls, alternating red/cream by i%2.
        assert_eq!(out.statics.len(), 2); // fabric_red, fabric_cream
        let total_tris: usize = out.statics.iter().map(|s| s.geo.tri_count()).sum();
        assert!(total_tris > 0);
    }

    #[test]
    fn striped_cloth_without_rng_is_deterministic_and_never_scales_masks_down() {
        let mut asm = Assembler::new(Rng::new(1));
        striped_cloth(&mut asm, &["fabric_red"], &Mat4::IDENTITY, 1.0, 1.0, StripedClothOpts { bands: 1, seg_x: 4, ..StripedClothOpts::default() }, None);
        let out = asm.finalize();
        // gv defaults to 1 without rng, so masks[1] stays exactly 0.5.
        let g = &out.statics[0].geo;
        assert!(g.color.chunks_exact(3).any(|c| (c[1] - 0.5).abs() < 1e-6));
    }

    #[test]
    fn awning_produces_cloth_frame_and_leg_batches_when_legs_enabled() {
        let mut asm = Assembler::new(Rng::new(1));
        let mut rng = Rng::new(3);
        let result = awning(&mut asm, &Mat4::IDENTITY, 0.0, 2.2, 2.0, &mut rng, AwningOpts { depth: 1.5, slope: 0.32, keys: ["fabric_red", "fabric_cream"], legs: true });
        assert_eq!(result, AwningResult { x: 0.0, y: 2.2, w: 2.0, d: 1.5 });
        let out = asm.finalize();
        let keys: Vec<&str> = out.statics.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"metal_rust"));
        assert!(keys.contains(&"fabric_red") || keys.contains(&"fabric_cream"));
    }
}
