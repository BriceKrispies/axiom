//! The tarmac's aggregate grain, as a tiling albedo texture.
//!
//! Every material in this app was, until now, a single flat colour: the road
//! surface renders the *same* RGB at eight metres and at sixty, and the largest
//! object in any frame — the tarmac fills roughly half of it — has no surface at
//! all. Real asphalt is a bound aggregate, and what makes it read as asphalt
//! rather than as grey paper is a fine, low-amplitude mottle: in the night
//! reference this app is converging on, an unpainted patch of near road measures
//! a standard deviation of ~2.4 sRGB levels around a mean of ~16 — about 15% of
//! its own value, all of it in the darks. This module produces exactly that.
//!
//! ## The amplitude is measured, not chosen
//!
//! The backend's material sampler is `Repeat` + **`Nearest`, with no mipmaps**
//! (`scene_renderer.rs`). A road runs to the vanishing point, so its texels
//! minify without bound, and a high-contrast texture under a mip-less nearest
//! sampler does not fade into an average — it aliases into a crawling carpet of
//! sparkle, which is a far worse artifact than the flat grey it replaced. The
//! grain therefore has to be strong enough to read and weak enough not to crawl,
//! and both ends of that are pinned by measurement rather than by taste:
//!
//! * **Strong enough.** Unpainted asphalt in the reference measures a standard
//!   deviation of 10–15% of its own displayed value (three patches at different
//!   depths: `0.80/7.6`, `3.91/31.7`, `2.35/15.8`) — though some of that is the
//!   frame's lighting falloff across the patch, not texture. This texture lands
//!   at **~6%** of the tarmac's displayed value: unmistakably a surface, and
//!   deliberately short of a number that is partly lighting.
//! * **Quiet enough.** The worst *adjacent-texel* step is **~10 sRGB levels** on
//!   the tarmac. That is the whole error budget of a minified sample that lands
//!   on the wrong texel — a fine dither in the darkest region of the frame, not
//!   a sparkle. [`tests::adjacent_texels_stay_inside_the_alias_budget`] holds it.
//!
//! The second bound is bought by **low-frequency dominance**: [`SMOOTH_SHARE`] of
//! the amplitude lives in a smoothly-interpolated `LATTICE`-cell field, so
//! neighbouring texels are nearly equal and a minified sample lands near the
//! local mean whichever texel it hits. Only the remainder is per-texel hash — the
//! part that survives to give the near field its bite.
//!
//! ## The grain darkens the tarmac, and that is the honest direction
//!
//! A multiplied albedo can only ever darken (a texel's ceiling is `1.0`), so a
//! band wide enough to read pulls the mean down — here to `0.81`, about a fifth.
//! That is not a side effect to apologise for: asphalt *is* dark, and the live
//! render's tarmac currently sits far brighter than the reference's. A texture
//! wide enough to be a material and centred on `1.0` does not exist on a
//! multiply-only path; this is the trade, taken deliberately and in the
//! direction the reference points.
//!
//! ## Why it tiles without a seam
//!
//! `Repeat` addressing means texel column `RES-1` is adjacent to column `0` on
//! screen. The lattice is therefore sampled **toroidally** — cell indices wrap
//! with `% LATTICE` — so the interpolated field is continuous across the wrap and
//! no repeat boundary is visible as a line down the road.
//!
//! ## Why the pixels are authored as sRGB bytes
//!
//! The GPU backend uploads a custom albedo as `Rgba8UnormSrgb` and the shader
//! computes `base = albedo * colour`, so a byte here is decoded to linear before
//! it multiplies [`super::palette`]'s authored tarmac colour. The byte range is
//! therefore derived from the linear multipliers it must produce, not picked by
//! eye — see [`byte_for_multiplier`].

/// The texture's edge length in texels. Deliberately small: one tile covers
/// [`TILE_METRES`] square of road (see below), so at 32 texels a texel is ~5 cm
/// and a lattice cell ~19 cm — decimetre-scale grain, coarse enough to survive
/// nearest-sampling in the near field, fine enough to read as aggregate.
pub const RES: u32 = 32;

/// How much road, in metres, one tile of this texture covers — **in both axes**.
///
/// This is the number that decides whether the grain reads as asphalt, and it is
/// a property of the texture rather than of any one quad, which is why it lives
/// here and `road_mesh` reads it. It is also the constant this module's own scale
/// claims were silently missing: the paving quads used to be UV-mapped `0..1`
/// per quad, so a single tile was stretched across an 18 m × 2 m panel — 0.56 m
/// texels smeared 9:1, `LATTICE` cells over two metres wide, and the identical
/// pattern repeating in lock-step every 2 m down the road. What was authored as
/// aggregate rendered as camouflage. At 1.5 m the tile is square, the grain sits
/// at the scale the amplitude below was measured for, and the repeat period is
/// short enough that the toroidal lattice hides it completely.
pub const TILE_METRES: f32 = 1.5;

/// The smooth octave's cell count across the texture. `RES / LATTICE` texels per
/// cell, so the low-frequency field changes slowly and minifies gracefully.
const LATTICE: u32 = 8;

/// The darkest linear multiplier a texel may apply to the tarmac's base colour.
const MIN_MULTIPLIER: f32 = 0.62;

/// Share of the amplitude carried by the smooth octave. The remainder is
/// per-texel hash — see the module docs on why the split is weighted this way.
const SMOOTH_SHARE: f32 = 0.75;

/// Contrast applied about the field's midpoint before it is mapped to a
/// multiplier. Two independent `0..=1` sources summed give a triangular
/// distribution — most texels pile up in the middle, so the *range* looks right
/// while the actual variation reads as almost nothing (measured: 4.1% of the
/// tarmac's value at gain `1.0`, against the reference's 10–15%). Expanding
/// about the midpoint spends the authored range instead of wasting it, and
/// `1.5` is the largest gain that still keeps the adjacent-texel step inside
/// the alias budget above.
const CONTRAST: f32 = 1.5;

/// The tiling asphalt albedo, as `RES * RES` RGBA8 texels ready for
/// `RunningApp::add_texture_data`.
pub fn asphalt_albedo() -> Vec<u8> {
    (0..RES * RES)
        .flat_map(|i| {
            let (x, y) = (i % RES, i / RES);
            let value = grain(x, y);
            let byte = byte_for_multiplier(MIN_MULTIPLIER + (1.0 - MIN_MULTIPLIER) * value);
            // Neutral grey: the tarmac's hue is the material's, this only shades it.
            [byte, byte, byte, 255]
        })
        .collect()
}

/// The combined grain field at a texel, in `0..=1`.
fn grain(x: u32, y: u32) -> f32 {
    let smooth = smooth_octave(x, y);
    let fine = hash_unit(x, y, 0x9E37_79B9);
    let mixed = smooth * SMOOTH_SHARE + fine * (1.0 - SMOOTH_SHARE);
    ((mixed - 0.5) * CONTRAST + 0.5).clamp(0.0, 1.0)
}

/// Value noise on a `LATTICE x LATTICE` toroidal grid, smoothstep-interpolated.
///
/// Toroidal because the texture repeats: cell `LATTICE` *is* cell `0`, so the
/// field is continuous across the tile boundary and the repeat leaves no seam.
fn smooth_octave(x: u32, y: u32) -> f32 {
    let per_cell = (RES / LATTICE) as f32;
    let (fx, fy) = (x as f32 / per_cell, y as f32 / per_cell);
    let (cx, cy) = (fx.floor(), fy.floor());
    let (tx, ty) = (smoothstep(fx - cx), smoothstep(fy - cy));
    let corner = |ox: u32, oy: u32| {
        hash_unit(
            (cx as u32 + ox) % LATTICE,
            (cy as u32 + oy) % LATTICE,
            0x85EB_CA6B,
        )
    };
    let top = lerp(corner(0, 0), corner(1, 0), tx);
    let bottom = lerp(corner(0, 1), corner(1, 1), tx);
    lerp(top, bottom, ty)
}

/// The cubic ease `3t² − 2t³`, so the lattice's cell joins have no visible
/// creases (a linear interpolation of value noise shows its grid as a diamond
/// pattern, which on a road reads as a repeating defect).
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A deterministic `0..=1` hash of a texel/cell coordinate. Integer-only, so the
/// texture is byte-identical on every platform and every run — a race that
/// replays identically deserves a road that does too.
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
/// The backend uploads this texture as `Rgba8UnormSrgb`, so the shader sees the
/// *decoded* value. Authoring the byte directly would make the grain roughly
/// twice as strong as intended near white, where the sRGB curve is steepest, so
/// the transfer function is inverted here rather than guessed at.
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

    /// The tarmac's linear base colour (`super::super::palette::road_materials`),
    /// green channel. The grain only ever multiplies this, so every claim about
    /// how the grain *looks* has to be evaluated after it.
    const TARMAC: f32 = 0.088;

    /// A linear value as the 0..255 display level it lands on, so the tests can
    /// speak in the same units the reference was measured in.
    fn displayed(linear: f32) -> f32 {
        255.0 * (1.055 * linear.powf(1.0 / 2.4) - 0.055)
    }

    /// Every texel's displayed tarmac level, row-major.
    fn tarmac_levels() -> Vec<f32> {
        asphalt_albedo()
            .chunks(4)
            .map(|t| displayed(TARMAC * decoded(t[0])))
            .collect()
    }

    #[test]
    fn the_albedo_is_exactly_the_pixel_buffer_add_texture_data_accepts() {
        let pixels = asphalt_albedo();
        assert_eq!(pixels.len(), (RES * RES * 4) as usize);
        // Opaque throughout: the shader's alpha-mask capability cuts at 0.5, and a
        // road with holes in it is not a road.
        assert!(pixels.chunks(4).all(|t| t[3] == 255));
        // Neutral: the tarmac's hue belongs to the material, not to the grain.
        assert!(pixels.chunks(4).all(|t| t[0] == t[1] && t[1] == t[2]));
    }

    #[test]
    fn every_texel_stays_inside_the_authored_multiplier_band() {
        let multipliers: Vec<f32> = asphalt_albedo().chunks(4).map(|t| decoded(t[0])).collect();
        let lo = multipliers.iter().copied().fold(f32::MAX, f32::min);
        let hi = multipliers.iter().copied().fold(f32::MIN, f32::max);
        assert!(lo >= MIN_MULTIPLIER - 0.01, "grain darkens past its bound: {lo}");
        assert!(hi <= 1.0, "a multiplier cannot brighten past the base colour: {hi}");
    }

    /// **Strong enough to be a material.** The whole point of the texture is
    /// that the tarmac stops rendering one identical value everywhere, and the
    /// unit that decides whether a human sees that is the *displayed* spread as
    /// a fraction of the displayed value — not the authored linear range, which
    /// sRGB encoding compresses by more than half.
    ///
    /// The reference's own unpainted asphalt measures 10–15% (part of which is
    /// its lighting falloff, not its surface). Falling under 4% here means the
    /// grain has quietly become a flat fill again — which is exactly what an
    /// innocent-looking tweak to `CONTRAST` or `MIN_MULTIPLIER` would do.
    #[test]
    fn the_grain_varies_enough_to_read_as_a_surface() {
        let levels = tarmac_levels();
        let mean = levels.iter().sum::<f32>() / levels.len() as f32;
        let sd = (levels.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / levels.len() as f32)
            .sqrt();
        let relative = sd / mean;
        assert!(
            (0.04..0.10).contains(&relative),
            "displayed variation is {:.1}% of the tarmac's value; the reference \
             measures 10-15% (lighting included) and a flat fill measures 0%",
            relative * 100.0
        );
    }

    /// **Quiet enough not to crawl.** The material sampler is `Repeat` +
    /// `Nearest` with *no mipmaps*, so a minified road samples one arbitrary
    /// texel per pixel and the error of that choice is bounded by how far
    /// adjacent texels sit apart. Measured in display levels, that error budget
    /// is what separates asphalt grain from a sparkling carpet, and it is bought
    /// by `SMOOTH_SHARE` — most of the amplitude is in a field whose neighbours
    /// are nearly equal.
    #[test]
    fn adjacent_texels_stay_inside_the_alias_budget() {
        let levels = tarmac_levels();
        let at = |x: u32, y: u32| levels[(y * RES + x) as usize];
        let worst = (0..RES)
            .flat_map(|y| {
                (0..RES).map(move |x| {
                    // Both axes, wrapping — `Repeat` makes the last column a
                    // neighbour of the first.
                    (at(x, y) - at((x + 1) % RES, y))
                        .abs()
                        .max((at(x, y) - at(x, (y + 1) % RES)).abs())
                })
            })
            .fold(0.0f32, f32::max);
        assert!(
            worst <= 12.0,
            "adjacent texels differ by {worst:.1} display levels; past ~12 a \
             mip-less nearest sampler turns the distant road into sparkle"
        );
    }

    /// The seam test. `Repeat` addressing puts column `RES-1` next to column `0`,
    /// so the *smooth* octave — the part the eye tracks — must be continuous
    /// across the wrap or the tile boundary draws a line down the road every
    /// two metres.
    #[test]
    fn the_smooth_octave_wraps_without_a_seam() {
        let worst_x = (0..RES)
            .map(|y| (smooth_octave(RES - 1, y) - smooth_octave(0, y)).abs())
            .fold(0.0f32, f32::max);
        let worst_y = (0..RES)
            .map(|x| (smooth_octave(x, RES - 1) - smooth_octave(x, 0)).abs())
            .fold(0.0f32, f32::max);
        // One texel of the lattice's own slope is continuity; a discontinuity
        // would jump by a whole cell's amplitude.
        let one_texel = 1.0 / (RES / LATTICE) as f32;
        assert!(worst_x <= one_texel, "vertical seam across the tile: {worst_x}");
        assert!(worst_y <= one_texel, "horizontal seam across the tile: {worst_y}");
    }

    #[test]
    fn the_texture_is_deterministic() {
        assert_eq!(asphalt_albedo(), asphalt_albedo());
    }

    /// The sRGB inversion, pinned at both ends and in the middle. A regression
    /// here silently changes the grain's strength without changing any authored
    /// constant.
    #[test]
    fn bytes_are_the_srgb_encoding_of_the_multiplier_they_stand_for() {
        assert_eq!(byte_for_multiplier(1.0), 255);
        assert_eq!(byte_for_multiplier(0.0), 0);
        assert!((decoded(byte_for_multiplier(0.78)) - 0.78).abs() < 0.01);
        assert!((decoded(byte_for_multiplier(0.5)) - 0.5).abs() < 0.01);
        // The linear toe, and the clamp on out-of-range input.
        assert_eq!(byte_for_multiplier(0.001), 3);
        assert_eq!(byte_for_multiplier(4.0), 255);
        assert_eq!(byte_for_multiplier(-1.0), 0);
    }
}
