//! The roadside's ground cover, as a tiling albedo texture.
//!
//! ## The verge is the frame's last true flat fill
//!
//! [`super::asphalt_texture`] exists because the tarmac was one flat colour, and
//! it settled that: the road now carries aggregate, wheel tracks and paver seams
//! all the way to the vanishing point. Nothing did the same for the surface
//! *beside* it, and that surface is the second-largest ground plane in any frame
//! — the verge quad runs from the shoulder edge out past the barrier by
//! [`super::road_mesh::VERGE_REACH`], forty-odd metres of ground either side of
//! the road, from the bumper to the horizon.
//!
//! It is not "nearly flat" the way the pre-texture tarmac was. Measured on the
//! champion frame it is **exactly** flat: sampling 12 × 12 patches of the verge
//! band on both shoulders returns a standard deviation of `0.00` — every pixel
//! byte-identical at `(75, 93, 87)` on the left and `(47, 52, 64)` on the right,
//! across thousands of pixels. That is not a shading gradient with quiet texture
//! on it; it is a single RGB triple painted over a two-hundred-metre strip of
//! ground. The reference's roadside never does this anywhere: its cleanest,
//! lowest-variance roadside patches still measure 0.9–1.9 levels of variation,
//! and what the eye actually reads there is coarser still — dry sand breaking
//! through low cover, scrub clumps, bare earth between them.
//!
//! ## What this authors, and why it is a *hue* field rather than a grey one
//!
//! A grey multiplier can only make the verge a darker or lighter green, and a
//! roadside that varies only in value reads as one material under uneven light,
//! which is not what is missing. What is missing is that the roadside is **two**
//! materials interleaved: living cover, and the dry earth it does not quite
//! reach. The reference shows exactly that mix on both sides of the road.
//!
//! So the field runs between two authored linear colours — [`COVER`] and
//! [`DUST`] — and the texture carries the per-channel multiplier that takes the
//! material's [`BASE`] to one or the other. Green where the cover wins, warm and
//! a little darker where the earth shows through.
//!
//! ## Exposure is held, and the arithmetic is not left to the eye
//!
//! A custom albedo multiplies the material colour (`base = albedo * colour`), and
//! a texel's ceiling is `1.0`, so a texture can only ever darken. [`BASE`] is
//! therefore the channel-wise maximum of the two targets — the smallest colour
//! from which both are reachable — and it is *brighter* than the flat verge it
//! replaces. That lift is not a grade: it is what keeps the textured mean where
//! the flat fill was. [`tests::the_verge_keeps_the_luminance_the_flat_fill_had`]
//! pins the mean of the actual authored buffer against the old value, so a future
//! edit to either target colour cannot quietly re-expose the roadside.
//!
//! The software arm ignores custom textures, so it sees [`BASE`] alone: a ~7%
//! brighter, very slightly warmer verge, still plainly the same green strip. That
//! is the sanctioned shape of a capability-gated change — the richness lands on
//! the GPU arm and the other one degrades to the base colour.
//!
//! ## Scale, and the UV mapping this depends on
//!
//! [`super::road_mesh::verge_uvs`] maps this texture in **metres of world**, for
//! the reason the paving does: a verge quad spans roughly 47 m laterally by one
//! 2 m sample step, so the builder's default `0..1`-per-quad UVs stretched a
//! single tile across it at 23:1 *and* repeated that same smeared copy in
//! lock-step every two metres down the course. Whatever is authored here, that
//! mapping renders as transverse banding to the horizon.
//!
//! At [`TILE_METRES`] the tile is square and the clump field sits at the size of
//! the thing it stands for: a [`CLUMPS`] cell is 37.5 cm, which is a bush, a
//! tuft, a patch of bare ground — decimetres, an order coarser than the tarmac's
//! centimetre aggregate, because roadside cover *is* an order coarser than road
//! aggregate.
//!
//! Sampled [`axiom::prelude::TextureSampling::Crisp`], not anisotropic. Crisp
//! still minifies linearly across a real mip chain (only magnification and
//! anisotropy differ), so the far verge averages cleanly to its mean rather than
//! sparkling; the tarmac's anisotropy is bought for a surface whose *lateral*
//! detail — lane-width wheel tracks — has to survive at the vanishing point, and
//! the verge has no such feature to protect.
//!
//! ## Why it tiles without a seam
//!
//! `Repeat` addressing puts texel column `RES-1` next to column `0`, so both
//! octaves are sampled toroidally — cell indices wrap with `%` — and the
//! interpolated field is continuous across the wrap. A discontinuity here would
//! not be a blemish: it would be a line drawn down nine kilometres of roadside.

/// The texture's edge length in texels. At [`TILE_METRES`] a texel is 4.7 cm —
/// comfortably finer than the [`CLUMPS`] field that carries the read, so a clump
/// has a resolved edge rather than a staircase. 64 × 64 × RGBA is 16 KiB.
pub const RES: u32 = 64;

/// How much ground, in metres, one tile covers — in both axes.
///
/// Coarser than the tarmac's 1.5 m by design. Road aggregate is centimetres and
/// roadside cover is decimetres, so a verge mapped at the paving's scale would
/// render as a fine dither rather than as clumps, and the whole point of the
/// field is the clumps.
pub const TILE_METRES: f32 = 3.0;

/// The clump field's cell count across the tile — a cell is
/// `TILE_METRES / CLUMPS` = **37.5 cm**, which is a bush or a bare patch, not a
/// blade and not a field.
const CLUMPS: u32 = 8;

/// The living-cover end of the field, in linear RGB — the green the verge is
/// today (`[0.115, 0.145, 0.105]`, held exactly, so the cover reads as the same
/// material this replaces rather than as a new one).
const COVER: [f32; 3] = [0.115, 0.145, 0.105];

/// The bare-earth end, in linear RGB. Warm and a little darker than the cover:
/// dry roadside soil is a red-shifted near-neutral, and the reference's roadside
/// shows exactly this between its scrub. Deliberately not sand-bright — the
/// verge's *exposure* belongs to the lighting and grade, and this module's job is
/// to give it a surface without moving it.
const DUST: [f32; 3] = [0.155, 0.128, 0.076];

/// The material base colour this texture multiplies — the verge material's
/// authored colour, and what the software arm shows on its own.
///
/// It is not a fourth authored colour: it is the **channel-wise maximum** of
/// [`COVER`] and [`DUST`], the darkest colour from which a `<= 1.0` multiplier
/// can still reach both, since a custom albedo can only darken. Anything dimmer
/// would clip the warm end of the field back to the cover's green and the whole
/// hue split would collapse into a value ramp.
/// [`tests::base_is_the_channel_wise_maximum_of_the_two_targets`] recomputes it
/// from the two targets, so an edit to either that leaves this behind fails
/// rather than silently flattening the field.
pub const BASE: [f32; 3] = [0.155, 0.145, 0.105];

/// The tiling ground-cover albedo, as `RES * RES` RGBA8 texels ready for
/// `RunningApp::add_texture_data`.
pub fn verge_albedo() -> Vec<u8> {
    (0..RES * RES)
        .flat_map(|i| {
            let (x, y) = (i % RES, i / RES);
            let t = mix(x, y);
            let texel = |c: usize| {
                byte_for_multiplier(lerp(COVER[c], DUST[c], t) / BASE[c])
            };
            [texel(0), texel(1), texel(2), 255]
        })
        .collect()
}

/// Where a texel sits between cover (`0`) and bare earth (`1`).
///
/// Two octaves. The clump field is the read — 37.5 cm patches of cover and gap,
/// smoothstep-interpolated so a clump has an edge rather than a staircase.
/// The fine hash is the litter on top of it: at [`FINE_SHARE`] it is a quiet
/// term, enough that two neighbouring texels inside one clump are never the same
/// colour, nowhere near enough to turn the verge into noise.
fn mix(x: u32, y: u32) -> f32 {
    (clump_octave(x, y) * (1.0 - FINE_SHARE) + hash_unit(x, y, 0x7F4A_7C15) * FINE_SHARE)
        .clamp(0.0, 1.0)
}

/// Share of the field carried by the per-texel hash rather than by the clumps.
const FINE_SHARE: f32 = 0.22;

/// Value noise on a `CLUMPS x CLUMPS` toroidal grid, smoothstep-interpolated.
///
/// Toroidal because the texture repeats: cell `CLUMPS` *is* cell `0`, so the
/// field is continuous across the tile boundary and the repeat leaves no seam
/// down the roadside.
fn clump_octave(x: u32, y: u32) -> f32 {
    let per_cell = RES as f32 / CLUMPS as f32;
    let (fx, fy) = (x as f32 / per_cell, y as f32 / per_cell);
    let (cx, cy) = (fx.floor(), fy.floor());
    let (tx, ty) = (smoothstep(fx - cx), smoothstep(fy - cy));
    let corner = |ox: u32, oy: u32| {
        hash_unit(
            (cx as u32 + ox) % CLUMPS,
            (cy as u32 + oy) % CLUMPS,
            0x68E3_1DA4,
        )
    };
    let top = lerp(corner(0, 0), corner(1, 0), tx);
    let bottom = lerp(corner(0, 1), corner(1, 1), tx);
    lerp(top, bottom, ty)
}

/// The cubic ease `3t² − 2t³`, so a clump's boundary is a soft edge rather than
/// the diamond lattice a linear interpolation of value noise draws.
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A deterministic `0..=1` hash of a texel/cell coordinate. Integer-only, so the
/// texture is byte-identical on every platform and every run.
fn hash_unit(x: u32, y: u32, salt: u32) -> f32 {
    let mut h = x.wrapping_mul(0x27D4_EB2D) ^ y.wrapping_mul(0x1656_67B1) ^ salt;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x2974_5C69);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}

/// The sRGB byte that decodes to `multiplier` in linear light.
///
/// The backend uploads a custom albedo as `Rgba8UnormSrgb` and the shader sees
/// the *decoded* value, so the transfer function is inverted here rather than
/// guessed at — authoring the byte directly would land the hue split in the
/// wrong place, by more than half near white where the curve is steepest.
fn byte_for_multiplier(multiplier: f32) -> u8 {
    let m = multiplier.clamp(0.0, 1.0);
    let encoded = if m <= 0.003_130_8 {
        m * 12.92
    } else {
        1.055 * m.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(byte: u8) -> f32 {
        let e = byte as f32 / 255.0;
        if e <= 0.040_45 {
            e / 12.92
        } else {
            ((e + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: [f32; 3]) -> f32 {
        c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
    }

    /// Every texel's rendered linear colour — `BASE * decoded(texel)`, which is
    /// exactly what the shader computes.
    fn rendered() -> Vec<[f32; 3]> {
        verge_albedo()
            .chunks(4)
            .map(|t| {
                [
                    BASE[0] * decoded(t[0]),
                    BASE[1] * decoded(t[1]),
                    BASE[2] * decoded(t[2]),
                ]
            })
            .collect()
    }

    #[test]
    fn the_albedo_is_exactly_the_pixel_buffer_add_texture_data_accepts() {
        let pixels = verge_albedo();
        assert_eq!(pixels.len(), (RES * RES * 4) as usize);
        // Opaque throughout: the shader's alpha mask cuts at 0.5 and a roadside
        // with holes in it shows the sky through the ground.
        assert!(pixels.chunks(4).all(|t| t[3] == 255));
    }

    /// A channel's relative standard deviation across the whole texture.
    fn channel_variation(px: &[[f32; 3]], c: usize) -> f32 {
        let mean = px.iter().map(|p| p[c]).sum::<f32>() / px.len() as f32;
        let sd = (px.iter().map(|p| (p[c] - mean).powi(2)).sum::<f32>() / px.len() as f32).sqrt();
        sd / mean
    }

    /// **The flat fill is gone.** The defect this module exists for is measured,
    /// not described: the champion frame's verge returns a standard deviation of
    /// exactly `0.00` over 12 × 12 patches, thousands of byte-identical pixels.
    /// The floor here is that the verge varies at all, and the ceiling is that it
    /// does not vary so hard the roadside becomes noise.
    ///
    /// Measured **per channel**, and that is deliberate: this field is a hue
    /// split authored at near-constant luminance (see
    /// [`tests::the_verge_keeps_the_luminance_the_flat_fill_had`]), so a
    /// luminance statistic would report ~2% and conclude the texture had barely
    /// done anything — while the red and blue channels, which are what the eye
    /// reads as "two materials", travel three times as far.
    #[test]
    fn the_verge_has_a_surface_and_it_is_not_static() {
        let px = rendered();
        let worst = (0..3)
            .map(|c| channel_variation(&px, c))
            .fold(0.0f32, f32::max);
        assert!(
            (0.02..0.18).contains(&worst),
            "the verge's widest channel varies by {:.1}% of its own value; 0% is \
             the flat fill this module replaces and past ~18% the roadside reads \
             as noise",
            worst * 100.0
        );
    }

    /// **The variation is a hue split, not a value ramp.** A grey mottle would
    /// pass the test above while leaving the verge one material under uneven
    /// light, which is not the thing the reference's roadside has. The claim is
    /// that the red/green ratio genuinely travels across the texture: cover is
    /// green-dominant, bare earth is red-dominant, and both are present.
    #[test]
    fn the_roadside_is_two_materials_interleaved_not_one_shaded_twice() {
        let px = rendered();
        let ratio: Vec<f32> = px.iter().map(|c| c[0] / c[1]).collect();
        let lo = ratio.iter().copied().fold(f32::MAX, f32::min);
        let hi = ratio.iter().copied().fold(f32::MIN, f32::max);
        assert!(lo < 0.85, "nothing on the verge reads as living cover: {lo:.2}");
        assert!(hi > 1.15, "nothing on the verge reads as bare earth: {hi:.2}");
        // And both targets are actually reachable — a BASE that clipped one end
        // would silently collapse the split.
        assert!(
            COVER.iter().zip(BASE).all(|(c, b)| *c <= b + 1.0e-6),
            "the cover colour is brighter than BASE and cannot be reached"
        );
        assert!(
            DUST.iter().zip(BASE).all(|(d, b)| *d <= b + 1.0e-6),
            "the dust colour is brighter than BASE and cannot be reached"
        );
    }

    /// **Adding a surface is not a grade.** The verge's flat fill was
    /// `[0.115, 0.145, 0.105]`; the textured mean has to land on the same
    /// brightness, or this module has quietly re-exposed the second-largest
    /// ground plane in the frame while claiming to add detail to it. Held within
    /// 8% — a fraction of a display level at this value.
    #[test]
    fn the_verge_keeps_the_luminance_the_flat_fill_had() {
        let was = luminance([0.115, 0.145, 0.105]);
        let px = rendered();
        let now = px.iter().map(|c| luminance(*c)).sum::<f32>() / px.len() as f32;
        assert!(
            (now / was - 1.0).abs() < 0.08,
            "the textured verge sits at {now:.4} against the flat fill's {was:.4}"
        );
    }

    /// **Quiet enough not to crawl, up close.** `Crisp` minifies linearly across
    /// a real mip chain, so the far verge cannot alias; this is the *magnified*
    /// budget — the hardest edge two neighbouring texels may show under the front
    /// wheels, as a share of the verge's own value.
    #[test]
    fn adjacent_texels_stay_inside_the_magnified_step_budget() {
        let px = rendered();
        let worst = (0..3)
            .map(|c| {
                let chan: Vec<f32> = px.iter().map(|p| p[c]).collect();
                let mean = chan.iter().sum::<f32>() / chan.len() as f32;
                let at = |x: u32, y: u32| chan[(y * RES + x) as usize];
                (0..RES)
                    .flat_map(|y| {
                        (0..RES).map(move |x| {
                            // Both axes, wrapping — `Repeat` makes the last
                            // column a neighbour of the first.
                            (at(x, y) - at((x + 1) % RES, y))
                                .abs()
                                .max((at(x, y) - at(x, (y + 1) % RES)).abs())
                        })
                    })
                    .fold(0.0f32, f32::max)
                    / mean
            })
            .fold(0.0f32, f32::max);
        assert!(
            worst < 0.30,
            "adjacent texels differ by {:.0}% of the verge's value; past ~30% the \
             near roadside reads as noise laid over ground rather than as ground",
            worst * 100.0
        );
    }

    /// The seam test. `Repeat` puts column `RES-1` beside column `0`, and a
    /// discontinuity in the clump field is a line drawn down the whole roadside.
    #[test]
    fn the_clump_field_wraps_without_a_seam() {
        let one_texel = CLUMPS as f32 / RES as f32;
        let worst_x = (0..RES)
            .map(|y| (clump_octave(RES - 1, y) - clump_octave(0, y)).abs())
            .fold(0.0f32, f32::max);
        let worst_y = (0..RES)
            .map(|x| (clump_octave(x, RES - 1) - clump_octave(x, 0)).abs())
            .fold(0.0f32, f32::max);
        assert!(worst_x <= one_texel, "vertical seam down the verge: {worst_x}");
        assert!(worst_y <= one_texel, "horizontal seam across the verge: {worst_y}");
    }

    /// **The clumps are the size of the thing they stand for.** Every other test
    /// here measures strength; none can tell half-metre scrub from a metre-scale
    /// camouflage blotch or a centimetre dither, and that scale is the whole
    /// difference between a roadside and a pattern.
    #[test]
    fn the_clumps_sit_at_the_physical_scale_of_ground_cover() {
        let cell = TILE_METRES / CLUMPS as f32;
        assert!(
            (0.25..=1.0).contains(&cell),
            "a {cell:.2} m clump is not a bush or a bare patch"
        );
        let texel = TILE_METRES / RES as f32;
        assert!(texel <= 0.08, "a {texel:.3} m texel cannot resolve a clump edge");
        assert_eq!(RES % CLUMPS, 0, "a clump that is not a whole number of texels");
    }

    /// [`BASE`] is a *consequence* of the two targets, not a third authored
    /// colour. Recomputed here so that editing [`COVER`] or [`DUST`] without
    /// following through fails loudly instead of clipping the field flat.
    #[test]
    fn base_is_the_channel_wise_maximum_of_the_two_targets() {
        let expected = [
            COVER[0].max(DUST[0]),
            COVER[1].max(DUST[1]),
            COVER[2].max(DUST[2]),
        ];
        assert_eq!(BASE, expected, "BASE has drifted from the colours it bounds");
    }

    #[test]
    fn the_texture_is_deterministic() {
        assert_eq!(verge_albedo(), verge_albedo());
    }

    #[test]
    fn bytes_are_the_srgb_encoding_of_the_multiplier_they_stand_for() {
        assert_eq!(byte_for_multiplier(1.0), 255);
        assert_eq!(byte_for_multiplier(0.0), 0);
        assert!((decoded(byte_for_multiplier(0.74)) - 0.74).abs() < 0.01);
        assert_eq!(byte_for_multiplier(0.001), 3);
        assert_eq!(byte_for_multiplier(4.0), 255);
        assert_eq!(byte_for_multiplier(-1.0), 0);
    }
}
