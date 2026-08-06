//! The tarmac's aggregate grain, as a tiling albedo texture.
//!
//! Every material in this app was, until now, a single flat colour: the road
//! surface renders the *same* RGB at eight metres and at sixty, and the largest
//! object in any frame — the tarmac fills roughly half of it — has no surface at
//! all. Real asphalt is a bound aggregate, and what makes it read as asphalt
//! rather than as grey paper is a fine, low-amplitude mottle. This module
//! produces exactly that.
//!
//! ## The amplitude is measured — and the reference it was measured against has
//! ## changed
//!
//! The numbers below used to be derived from a **night** reference, where an
//! unpainted patch of near road measured a standard deviation of 10–15% of its
//! own displayed value around a mean of ~16 levels. Against that, this texture
//! was authored at **5.92%** — deliberately short of a figure that was partly the
//! frame's lighting falloff rather than its surface.
//!
//! The app now converges on the **era-C daylight** reference, and that reference
//! measures a completely different surface. Sampling its cleanest unpainted
//! asphalt — the patches with the lowest total variance, so no lane paint, no
//! palm shadow, no kerb — gives `1.28/81.2` in full sun and `0.76/33.6` in shade:
//! a standard deviation of **1.6–2.3%** of the road's own displayed value. A 5×5
//! high-pass over the same patches isolates the texture from the lighting ramp
//! and lands at 1.0–1.8%. The two agree, which is the point: on this reference
//! the asphalt is *nearly smooth*, carrying a faint broad mottle and a scatter of
//! isolated grit, and nothing like a carpet.
//!
//! Relative amplitude is the right unit here and it is **exposure-invariant**:
//! the grain is a multiplied albedo, so re-exposing the frame scales the road and
//! its grain together and this ratio does not move. The 5.92% authored for the
//! night frame is therefore not a number a lighting change can absolve — it is
//! ~3× the reference's, on the largest surface in the shot, and normalising the
//! two frames for brightness shows exactly what that buys: the reference's near
//! road is a smooth plane, and this one is television static. That is the defect
//! [`MIN_MULTIPLIER`] now closes, at **1.87%** — mid-band of the reference's own
//! measurement.
//!
//! What was *never* settled is where in the spectrum that 5.9% sits — and getting
//! it wrong is what the near road actually looked like. The split used to be
//! pinned by a sampler that no longer exists. This module was authored against a
//! `Repeat` + **`Nearest`, no-mipmap** material sampler, under which a minified
//! road takes one arbitrary texel per pixel and a per-texel hash aliases into a
//! crawling carpet of sparkle. The defence was **low-frequency dominance**: two
//! thirds of the amplitude pushed into the smoothly-interpolated `LATTICE`-cell
//! field, so neighbours are nearly equal and whichever texel a minified sample
//! lands on, it lands near the local mean.
//!
//! The engine has since grown real mip chains and per-material anisotropic
//! filtering, and [`super::palette`] opts the tarmac into
//! `TextureSampling::Anisotropic`: minification is now trilinear across a mip
//! chain with 16× anisotropy along the view axis
//! (`axiom-gpu-backend/src/texture_sampling.rs`). A minified sample is an
//! *average* now, not a lottery, so the fine octave can no longer sparkle — but
//! the premium the road had been paying for that insurance kept coming due, as a
//! visible artifact. A `LATTICE` cell is 4.7 cm, and with two thirds of the
//! amplitude living in it the near tarmac rendered as a soft cellular quilt of
//! centimetre blobs: embossed leather, or orange peel. The reference's asphalt at
//! the same depth is a fine micro-speckle with nothing resolvable at that scale
//! at all.
//!
//! So the split inverts. Most of the amplitude now lives in the per-texel hash —
//! aggregate, at the size of a chipping — and the smooth octave stays on only as
//! the low-amplitude patchiness of the mix. That inversion was a pure move along
//! the frequency axis — the displayed variation was held at 5.92% across it while
//! the share carried at cell scale fell from **66% to 31%** — and it survives the
//! amplitude retune above untouched: [`SMOOTH_SHARE`] and [`CONTRAST`] do not
//! move, so the cell-scale share stays at 31%. Only the *strength* changes.
//! [`tests::most_of_the_grain_lives_at_texel_scale_not_at_cell_scale`] is the
//! assertion that keeps it there, and it is the one this module was missing —
//! every existing test measured the grain's *strength*, and the defect was
//! entirely in its *scale*.
//!
//! ## The grain has an *axis*, because the filter that erases it has one
//!
//! Both octaves above are **isotropic** — they carry the same detail across the
//! road as along it — and that is the assumption this module was built on and
//! never re-examined when it opted the tarmac into anisotropic sampling. A 16×
//! anisotropic sampler at this camera's grazing angle is not a blur: it is a
//! *directional* low-pass. It picks its mip from the **across**-road footprint
//! (centimetres) and then averages up to sixteen taps **along** the road (tens of
//! centimetres to metres). Every octave whose detail lives on the along-road axis
//! is therefore averaged toward its own mean before it ever reaches a pixel, and
//! an isotropic field is *entirely* made of such detail.
//!
//! That is not a theory, it is the measured champion frame. The authored variation
//! is 1.87% of the tarmac's displayed value; the frame's near road measures
//! **0.80%**, its mid road **1.4%**, and the reference's cleanest asphalt at the
//! same depth measures **2.4%**. Simulating the sampler's own footprint reproduces
//! it exactly — an isotropic 1.87% survives at 0.49% near and decays to 0.22% by
//! the middle distance. The road the module set out to stop being a flat fill was
//! being flat-filled by the filter, and getting flatter with depth, which is the
//! one direction real asphalt never goes.
//!
//! Spending more amplitude does not fix this; it buys a stronger signal into the
//! same shredder, at the exposure cost [`MIN_MULTIPLIER`] documents below. The fix
//! is to put the amplitude where the filter cannot reach: a **cross-road** octave
//! ([`CROSS_SHARE`]), a field that varies with the lateral coordinate only and is
//! constant along the course. Averaging sixteen taps along the road leaves it
//! **untouched** — it survives at 1.79% at nine centimetres of footprint and
//! 1.73% at seventy-five, where the isotropic octaves have long since vanished.
//!
//! And it is the correct surface, not a trick played on a sampler. `road_mesh`
//! maps `u` to the lateral offset and `v` to course distance, so a cross-road
//! band is a band that runs *down* the road — which is what a road is actually
//! made of: paver-lane seams, tyre-polished wheel tracks, the wear stripe between
//! them. Those are the features a driver's eye sees at a grazing angle, they are
//! why the reference's tarmac still reads as a surface at the vanishing point, and
//! at [`CROSS_BANDS`] they sit at a plausible ~19 cm.
//!
//! ## The grain darkens the tarmac, and how much is now a *cost*, not a bonus
//!
//! A multiplied albedo can only ever darken (a texel's ceiling is `1.0`), so the
//! band's width is also an unavoidable exposure cut: the mean multiplier is
//! `1 - (1 - MIN_MULTIPLIER) / 2` give or take the contrast.
//!
//! Under the night reference that was a free win — the render's tarmac sat
//! brighter than the reference's, so the wide band's `0.81` mean pulled it the
//! way it needed to go anyway. Era C inverts that too. The daylight reference's
//! sunlit asphalt displays at ~81 levels and this render's sits near 13, so every
//! stop the texture takes out is now a stop working against the frame. Narrowing
//! the band to `0.86` lifts the mean multiplier to **0.93** — a ~15% brightening
//! of the largest surface in the shot, in the direction era C points, obtained
//! for free as the same edit that kills the static.
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

/// The texture's edge length in texels. One tile covers [`TILE_METRES`] square
/// of road (see below), so at 128 texels a texel is **~1.2 cm** — the size of a
/// real chipping, which is the scale the word "aggregate" actually means.
///
/// It used to be 32, and that was the frame's most conspicuous surface defect.
/// A 32-texel tile puts a texel at ~4.7 cm and a `LATTICE` cell at ~19 cm, and
/// 19 cm is not a grain size — it is a *paving stone*. The smooth octave carries
/// [`SMOOTH_SHARE`] of the amplitude, so that decimetre field is what the eye
/// actually tracks, and the near road rendered as a regular quilt of rounded
/// diamonds: embossed leather, or cobbles, not tarmac. The reference's asphalt
/// at the same depth is a fine near-uniform micro-grain with no structure
/// resolvable above a centimetre or two.
///
/// The fix is a pure change of *scale*, not of amplitude: `RES` and [`LATTICE`]
/// are raised **together**, by the same factor, so `RES / LATTICE` — the texels
/// per lattice cell, and therefore the interpolation slope between neighbours —
/// is unchanged at 4. The displayed variation survives that change untouched at
/// ~6% of the tarmac's value. What changes is only how much road one cycle of the
/// pattern covers — which fixed the *period* of the quilt while leaving its
/// amplitude exactly where it was; see [`SMOOTH_SHARE`] for the half of the
/// defect a pure change of scale could never reach.
/// 128 × 128 × RGBA is 64 KiB — a rounding
/// error against a frame, and the one surface in the game that is never far away.
pub const RES: u32 = 128;

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
///
/// **This is locked to [`RES`] at 4 texels per cell**, and that ratio — not
/// either number alone — is what the alias budget is bought with. Raising
/// `LATTICE` on its own would steepen the interpolation between neighbouring
/// texels and hand the road straight back to the sparkle the smooth octave
/// exists to prevent; raising `RES` on its own would leave the decimetre quilt
/// exactly where it was and merely dither it. They move together, and
/// [`tests::the_grain_sits_at_the_physical_scale_of_aggregate`] pins the metres
/// the result covers rather than the texels, which is the unit the defect was
/// ever visible in.
const LATTICE: u32 = 32;

/// The darkest linear multiplier a texel may apply to the tarmac's base colour —
/// and therefore, on its own, **the grain's strength**.
///
/// [`SMOOTH_SHARE`] and [`CONTRAST`] decide how the amplitude is *distributed*
/// across the spectrum; this decides how much of it there is. It is the one knob
/// the reference change reaches, because the reference's asphalt did not change
/// frequency between eras — it changed how much surface it shows.
///
/// `0.62` was measured against the night reference, whose asphalt varied by
/// 10–15% of its own displayed value; it produced 5.92%. The era-C daylight
/// reference measures **1.6–2.3%** on the same statistic (see the module docs for
/// the patches), and `0.86` produces **1.87%** — mid-band. That is a 3.2× cut,
/// and the frame it fixes is unambiguous: normalised for exposure, the champion's
/// near road was a dense per-pixel speckle where the reference's was a smooth
/// plane with a few specks of grit on it.
///
/// The floor this may not cross is a flat fill. At `0.86` a tile still spans 17
/// distinct sRGB byte levels and neighbouring texels still differ, so the road
/// still renders differently at eight metres and at sixty — which was, and
/// remains, the entire reason this module exists.
const MIN_MULTIPLIER: f32 = 0.86;

/// Share of the amplitude carried by the smooth octave. The remainder is
/// per-texel hash.
///
/// **This is the constant that decides whether the road reads as aggregate or as
/// leather**, and the module docs above explain why it used to sit at `0.75`: a
/// mip-less nearest sampler made the fine octave sparkle, so the amplitude was
/// parked in the smooth one where a minified sample could not miss. The tarmac is
/// sampled trilinear + 16× anisotropic now, so that debt is settled and the
/// smooth field's only remaining job is the patchiness of the mix — which is a
/// *quiet* thing in real asphalt, not the thing you see first.
///
/// At `0.25` the per-texel hash still carries the grain and the smooth field
/// still carries the patchiness, which is the right way round: a `LATTICE` cell
/// is 4.7 cm, far too coarse to be a chipping, so every unit of amplitude spent
/// there is spent on a feature asphalt does not have. It gives up a little to
/// [`CROSS_SHARE`], which is the only octave the sampler delivers at depth.
const SMOOTH_SHARE: f32 = 0.25;

/// Share of the amplitude carried by the **cross-road** octave — the one the
/// anisotropic sampler cannot average away.
///
/// This is the majority share, and the module docs above carry the argument: the
/// tarmac is sampled with 16× anisotropy at a grazing angle, which averages up to
/// sixteen taps *along* the road and leaves the *across*-road axis at full
/// resolution. Both other octaves are isotropic, so all of their detail lies on
/// the axis that gets averaged; measured against the champion frame they arrive at
/// 0.49% near and 0.22% at the middle distance, from 1.87% authored. This octave
/// arrives at 1.79% and 1.73% — the difference between a road that has a surface
/// all the way to the vanishing point and one that washes to flat grey a short way
/// past the car.
///
/// It is a majority because it has to be, not because bands are the look: the two
/// isotropic octaves keep the near road from reading as pure corduroy (up close,
/// where the footprint is a texel or two, they arrive nearly intact and the road
/// is speckled aggregate), and this one keeps the *mid and far* road from reading
/// as nothing at all. Push it much past here and the near tarmac loses its
/// stones; pull it back and the far tarmac loses its surface.
const CROSS_SHARE: f32 = 0.55;

/// The cross-road octave's band count across one tile, so a band is
/// `TILE_METRES / CROSS_BANDS` = **18.8 cm** wide.
///
/// Sized as the physical thing it stands for, exactly like [`LATTICE`]. A wheel
/// track polished into asphalt, the seam between two paver lanes, and the wear
/// stripe beside them are all decimetre-scale features — nothing on a road runs
/// longitudinally at a centimetre. It is also the scale that has to *survive*: at
/// this frame's near road a pixel spans roughly 1.6 cm, so an 18.8 cm band is a
/// dozen pixels across and plainly resolvable, where the 1.2 cm texels of the fine
/// octave are already at or below Nyquist before the sampler touches them.
const CROSS_BANDS: u32 = 8;

/// Contrast applied about the field's midpoint before it is mapped to a
/// multiplier. Two independent `0..=1` sources summed give a triangular
/// distribution — most texels pile up in the middle, so the *range* looks right
/// while the actual variation reads as almost nothing. Expanding about the
/// midpoint spends the authored range instead of wasting it.
///
/// Lowered from `1.5` in lock-step with [`SMOOTH_SHARE`], and by arithmetic
/// rather than by eye: the per-texel hash is a full-width uniform where the
/// interpolated smooth field is not, so moving amplitude into it *raises* the
/// total variation on its own. `1.2` is the gain that gave back exactly what the
/// re-weighting added, so that re-weighting was a pure move along the frequency
/// axis with the strength held fixed.
///
/// It stayed at `1.2` through the era-C amplitude retune, and that was
/// deliberate: the strength belongs to [`MIN_MULTIPLIER`] alone. Spending this
/// gain on the cut instead would have narrowed the *distribution* rather than the
/// band, piling texels back into the middle and trading a surface for a flat fill
/// with outliers — which is the one failure mode this constant was introduced to
/// prevent.
///
/// It moves to `1.6` with [`CROSS_SHARE`] by that same arithmetic, in the
/// opposite direction. The rule is the one stated above: a full-width uniform
/// (the per-texel hash) contributes more variation per unit of share than an
/// interpolated field does, so *taking* share out of the hash and giving it to a
/// second interpolated octave lowers the total on its own. `1.6` gives back
/// exactly what the re-weighting removed — the authored variation lands at 1.95%
/// against 1.87%, inside a band that has not moved — so introducing the
/// cross-road octave is again a pure move along the frequency axis with the
/// strength held fixed. [`MIN_MULTIPLIER`] is untouched, so the exposure is too.
const CONTRAST: f32 = 1.6;

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
///
/// Three octaves, and the third is the one that reaches the screen at depth: the
/// smooth field is the mix's patchiness, the fine hash is its aggregate, and the
/// cross-road field is the longitudinal structure that the anisotropic sampler
/// cannot average away (see the module docs).
fn grain(x: u32, y: u32) -> f32 {
    let smooth = smooth_octave(x, y);
    let cross = cross_octave(x);
    let fine = hash_unit(x, y, 0x9E37_79B9);
    let mixed = smooth * SMOOTH_SHARE
        + cross * CROSS_SHARE
        + fine * (1.0 - SMOOTH_SHARE - CROSS_SHARE);
    ((mixed - 0.5) * CONTRAST + 0.5).clamp(0.0, 1.0)
}

/// Value noise on a `CROSS_BANDS` toroidal ring across the road, smoothstep-
/// interpolated — the wheel tracks, paver seams and wear stripes that run *down*
/// a road rather than across it.
///
/// It depends on `x` alone, and that is the entire point: `road_mesh` maps `u` to
/// the lateral offset, so this field is constant along the course, and the
/// sixteen along-road taps an anisotropic sampler takes at a grazing angle all
/// return the same value. It is the only octave here whose amplitude reaches a
/// distant pixel intact.
///
/// Toroidal for the same reason [`smooth_octave`] is: band `CROSS_BANDS` *is*
/// band `0`, so the field is continuous across the tile wrap and the repeat
/// leaves no stripe down the road.
fn cross_octave(x: u32) -> f32 {
    let per_band = (RES / CROSS_BANDS) as f32;
    let fx = x as f32 / per_band;
    let bx = fx.floor();
    let t = smoothstep(fx - bx);
    let band = |o: u32| hash_unit((bx as u32 + o) % CROSS_BANDS, 0, 0xC2B2_AE35);
    lerp(band(0), band(1), t)
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

    /// **As strong as the reference's asphalt, and no stronger.** The whole point
    /// of the texture is that the tarmac stops rendering one identical value
    /// everywhere, and the unit that decides whether a human sees that is the
    /// *displayed* spread as a fraction of the displayed value — not the authored
    /// linear range, which sRGB encoding compresses by more than half.
    ///
    /// That fraction is the right unit for a second reason: the grain is a
    /// multiplied albedo, so it is **exposure-invariant**. Re-lighting the frame
    /// scales the road and its grain together and this number does not move —
    /// which is why it can be compared against a reference shot under completely
    /// different light, and why no lighting change can ever excuse a bad value
    /// here.
    ///
    /// The band is the era-C daylight reference's own measurement: its cleanest
    /// unpainted asphalt reads `1.28/81.2` in sun and `0.76/33.6` in shade —
    /// **1.6–2.3%**. Widened by a whisker at each end for the byte quantisation.
    /// Above it the road renders as television static laid over tarmac (the
    /// defect the night reference's 10–15% left behind, at 5.92%); below it the
    /// grain has quietly become a flat fill again. Either is a regression, and
    /// an innocent-looking tweak to `CONTRAST` or `MIN_MULTIPLIER` reaches both.
    #[test]
    fn the_grain_is_as_strong_as_the_reference_asphalt_and_no_stronger() {
        let levels = tarmac_levels();
        let mean = levels.iter().sum::<f32>() / levels.len() as f32;
        let sd = (levels.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / levels.len() as f32)
            .sqrt();
        let relative = sd / mean;
        assert!(
            (0.012..0.030).contains(&relative),
            "displayed variation is {:.2}% of the tarmac's value; the era-C \
             reference measures 1.6-2.3%, static measures 6% and a flat fill \
             measures 0%",
            relative * 100.0
        );
    }

    /// **Quiet enough not to crawl.** The tarmac is sampled trilinear across a
    /// real mip chain with 16× anisotropy (`super::super::palette` opts it into
    /// `TextureSampling::Anisotropic`), so a minified sample is an average of the
    /// texels it covers rather than an arbitrary one of them, and the fine octave
    /// cannot alias into sparkle however sharp it is.
    ///
    /// The step is therefore no longer an *alias* budget — it is a **magnified**
    /// one. Up close a texel covers more than a pixel, and this is the hardest
    /// edge the near road can show between two neighbouring chippings.
    ///
    /// The bound tightens with the era-C amplitude cut, and it has to: at the
    /// night reference's strength this ran to ~16.4 levels under a ceiling of
    /// 18, which is a bound that could no longer fail. The 3.2× cut takes the
    /// worst step to ~5.5, and `8.0` restores the same ratio of headroom — so
    /// this stays what it was written to be, a live guard against the grain
    /// becoming noise laid over asphalt rather than the asphalt itself.
    #[test]
    fn adjacent_texels_stay_inside_the_magnified_step_budget() {
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
            worst <= 8.0,
            "adjacent texels differ by {worst:.1} display levels; past ~8 the \
             near road reads as noise laid over asphalt rather than as asphalt"
        );
    }

    /// **The grain is at the scale of a chipping, not of a paving stone.**
    ///
    /// This is the assertion the module was missing, and the one the visible
    /// defect lived under. Every other test here measures how *strong* the grain
    /// is; none of them could tell a fine aggregate speckle from a soft cellular
    /// quilt of 4.7 cm blobs, because the two have the identical mean, the
    /// identical standard deviation and the identical multiplier band. They
    /// differ only in *where in the spectrum* the amplitude sits — and that is
    /// the whole difference between tarmac and orange peel.
    ///
    /// Box-averaging each `LATTICE` cell (`RES / LATTICE` = 4 texels square)
    /// strips the per-texel hash and leaves exactly the low-frequency field the
    /// eye tracks as blobs.
    ///
    /// **What is measured of that field is its wobble *along the road*, and the
    /// direction is the whole assertion.** This test used to take the cell field's
    /// standard deviation flat — every cell against the global mean — and that
    /// statistic cannot tell a blob from a stripe. [`CROSS_SHARE`] is a
    /// cell-scale field by that measure and scores 91%, while being the exact
    /// opposite of orange peel: it is dead straight down the course, so it has no
    /// blobs in it at all. Left as it was, this test would have forced the
    /// amplitude back onto the one axis the anisotropic sampler averages away —
    /// it would have been an assertion *for* the defect, which is the worst thing
    /// a test can quietly become.
    ///
    /// Embossed leather is cell-scale structure that varies in **both** axes; a
    /// road surface is allowed — required — to be structured across its width.
    /// So the metric is the RMS, over the cell columns, of each column's
    /// variation down the course, as a share of the whole texture's. It measured
    /// **66%** at the old `SMOOTH_SHARE = 0.75`, 31% at `0.30`, and **23%** now.
    /// The bound has not moved, because the claim has not: the smooth octave is
    /// the mix's patchiness, a supporting term, and the moment it carries most of
    /// the amplitude the road stops being made of stones.
    #[test]
    fn most_of_the_grain_lives_at_texel_scale_not_at_cell_scale() {
        let owned = tarmac_levels();
        let levels: &[f32] = &owned;
        let per_cell = (RES / LATTICE) as usize;
        let cells_across = RES as usize / per_cell;
        let sd = |v: &[f32]| {
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
        };
        let cell = |cx: usize, cy: usize| {
            let sum: f32 = (0..per_cell)
                .flat_map(|j| {
                    (0..per_cell)
                        .map(move |i| ((cy * per_cell + j) * RES as usize) + cx * per_cell + i)
                })
                .map(|idx| levels[idx])
                .sum();
            sum / (per_cell * per_cell) as f32
        };
        // Each column of cells is one lateral position; its spread down the
        // course is that column's contribution to the quilt.
        let along: Vec<f32> = (0..cells_across)
            .map(|cx| {
                let column: Vec<f32> = (0..cells_across).map(|cy| cell(cx, cy)).collect();
                sd(&column)
            })
            .collect();
        let rms = (along.iter().map(|s| s * s).sum::<f32>() / along.len() as f32).sqrt();
        let share = rms / sd(levels);
        assert!(
            share < 0.45,
            "{:.0}% of the grain's amplitude is {:.0} cm blobs wobbling down the \
             course; past a minority share the near road renders as embossed \
             leather rather than as aggregate",
            share * 100.0,
            TILE_METRES / LATTICE as f32 * 100.0
        );
    }

    /// **The grain survives the sampler it is actually read through.**
    ///
    /// This is the assertion the module was missing, and the champion frame's
    /// road lived in the gap. Every other test here measures the texture as
    /// authored — its strength, its spectrum, its scale — and a *buffer* cannot
    /// see the thing that was wrong: [`super::palette`] opts the tarmac into 16×
    /// anisotropic sampling, and at this camera's grazing angle that averages up
    /// to sixteen taps **along** the road before a pixel is written. An isotropic
    /// field is entirely made of detail on that axis, so 1.87% of authored
    /// variation arrived as 0.49% near and 0.22% at the middle distance — a road
    /// getting *flatter* with depth, which is the one thing asphalt never does.
    ///
    /// Averaging whole runs of texels down the `v` axis is exactly that filter,
    /// and the bound is the reference's own asphalt: its cleanest unpainted
    /// patches measure 1.6–2.3% of their displayed value. The footprints span a
    /// near-road pixel to a far one; the point of the lower bound is that the
    /// number must still be there at 75 cm, not just at 9.
    #[test]
    fn the_grain_survives_the_anisotropic_filter_it_is_sampled_with() {
        let owned = tarmac_levels();
        let levels: &[f32] = &owned;
        for taps in [8_u32, 16, 32, 64] {
            let filtered: Vec<f32> = (0..RES / taps)
                .flat_map(|band| {
                    (0..RES).map(move |x| {
                        (0..taps)
                            .map(|j| levels[((band * taps + j) * RES + x) as usize])
                            .sum::<f32>()
                            / taps as f32
                    })
                })
                .collect();
            let mean = filtered.iter().sum::<f32>() / filtered.len() as f32;
            let sd = (filtered.iter().map(|l| (l - mean).powi(2)).sum::<f32>()
                / filtered.len() as f32)
                .sqrt();
            let relative = sd / mean;
            assert!(
                (0.012..0.030).contains(&relative),
                "after averaging {taps} texels ({:.0} cm) along the road the \
                 tarmac varies by {:.2}% of its own value; the reference's \
                 asphalt measures 1.6-2.3% at every depth, and an isotropic \
                 grain measures 0.2-0.5% here",
                taps as f32 * TILE_METRES / RES as f32 * 100.0,
                relative * 100.0
            );
        }
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

        // The cross-road octave wraps too, and it is the one that would show:
        // it carries the majority of the amplitude and runs the full length of
        // the course, so a discontinuity here is not a blemish, it is a stripe
        // drawn down the road every 1.5 m for nine kilometres.
        let cross_seam = (cross_octave(RES - 1) - cross_octave(0)).abs();
        assert!(
            cross_seam <= 1.0 / (RES / CROSS_BANDS) as f32,
            "the cross-road bands do not wrap: {cross_seam}"
        );
    }

    /// **The grain is the size of gravel, in metres.**
    ///
    /// Every other test here speaks in texels, and a texel is not a unit anyone
    /// can see — the tile's metre coverage is what decides whether the result
    /// reads as aggregate or as paving. The defect this pins was invisible to
    /// all of them: at `RES = 32` the amplitude, the alias step and the seam
    /// were all in budget while the road rendered a periodic quilt of 19 cm
    /// blobs, because the *scale* was never asserted.
    ///
    /// The bounds are the physical thing they describe. Road-surface chippings
    /// run roughly 5–14 mm, so a texel above ~1.5 cm cannot resolve one. The
    /// smooth octave is the mix's patchiness rather than its stones, so it is
    /// allowed to be coarser — but past ~6 cm it stops being a surface and
    /// starts being masonry, which is exactly the failure being locked out.
    #[test]
    fn the_grain_sits_at_the_physical_scale_of_aggregate() {
        let texel_metres = TILE_METRES / RES as f32;
        let cell_metres = TILE_METRES / LATTICE as f32;
        assert!(
            texel_metres <= 0.015,
            "a {:.1} cm texel cannot resolve a chipping",
            texel_metres * 100.0
        );
        assert!(
            cell_metres <= 0.06,
            "the smooth octave repeats every {:.0} cm; past ~6 cm the road reads \
             as paving stones rather than tarmac",
            cell_metres * 100.0
        );
        // And the ratio the alias budget is actually bought with, stated once:
        // four texels per cell, whatever the two constants are set to.
        assert_eq!(RES / LATTICE, 4, "the smooth octave's slope has moved");
        assert_eq!(RES % LATTICE, 0, "a cell that is not a whole number of texels");

        // The cross-road bands are decimetre features, and both bounds are
        // physical. Below ~8 cm nothing on a road runs longitudinally at that
        // width and the near tarmac reads as corduroy; above ~40 cm a band is
        // wider than a wheel track and the road reads as painted lanes. It must
        // also stay well clear of the near road's ~1.6 cm pixel, which is the
        // reason this octave exists at all.
        let band_metres = TILE_METRES / CROSS_BANDS as f32;
        assert!(
            (0.08..=0.40).contains(&band_metres),
            "a {:.0} cm longitudinal band is not a wheel track or a paver seam",
            band_metres * 100.0
        );
        assert_eq!(RES % CROSS_BANDS, 0, "a band that is not a whole number of texels");
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
