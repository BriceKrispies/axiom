//! **The 33³ display grading LUT** — the colourist chain that runs *after* AgX,
//! transcribed from the reference's JavaScript.
//!
//! Ported from Claude-of-Duty `src/render/lut.js` (the whole file: the preset,
//! `shoulderParams`, `applyGrade` and `createGradeLut`) plus its one consumer,
//! the `sampleLut` helper at `src/render/composite.js:32-36` and the blend at
//! `composite.js:144-145`.
//!
//! # Where it sits, and it is not where the file's name suggests
//!
//! This is **not** a scene-referred grade, **not** part of the bloom chain, and
//! **not** a look-up the tone map consults. It is the last thing that happens to
//! a pixel before grain and dither. `composite.js:131-145`, verbatim in order:
//!
//! ```text
//!   hdr           linear scene radiance x exposure, + bloom, x cos^4 vignette
//!   col   = owAgX( hdr, slope, power, sat )        <- crate::agx
//!   col   = clamp( col, 0.0, 1.0 )
//!   disp  = owLinearToSrgb( col )                  <- crate::surface_encode
//!   graded = sampleLut( disp )                     <- THIS MODULE
//!   disp  = mix( disp, graded, lutStrength )
//!   ... grain, ordered dither, write
//! ```
//!
//! The source's own comment on the line above `sampleLut` is the whole argument
//! and is worth keeping intact:
//!
//! > Everything below this line is display-referred (code values, 0..1 sRGB).
//! > The grade LUT and the grain are authored in that space: the LUT's
//! > toe/shadowTint are additive *code value* offsets, so feeding it linear
//! > light turned a 0.008 toe into a hard linear floor and painted the whole
//! > frame's shadows blue-grey. **Encode first, grade second.**
//!
//! That is a defect the reference already shipped and fixed, and it is exactly
//! the mistake a port makes when it files a thing called "grading LUT" next to
//! the tone map. Every constant in [`SHIPPED_PRESET`] is calibrated to where
//! **AgX** puts things: [`GradePreset::pivot`] is `0.50` *because* "AgX puts 18%
//! scene grey near 0.50 display", and [`GradePreset::saturation`] is `1.20`
//! *because* "AgX's inset/outset pair is a desaturating transform by
//! construction". Applied before AgX, or to linear light, every one of them is
//! calibrated against nothing.
//!
//! # Where it goes in *this* crate: inside `graded()`, first
//!
//! [`crate::post_chain`]'s HDR composite already runs the first four lines:
//!
//! ```text
//!   let tone = mix(rolled, mapped, AXIOM_TONE_STRENGTH);   // mapped = axiom_agx(...)
//!   let out  = graded(tone);                               // graded() = srgb_encode -> ... -> srgb_decode
//! ```
//!
//! and `graded()` opens with `let d = srgb_encode(linear);` — **that `d` is the
//! source's `disp`.** So the LUT belongs immediately after that encode and
//! **before** the [`axiom_host::FramePostProcess`] grade terms (`f`, `e`, `k`,
//! `s`), i.e. the first statement of the display-referred window:
//!
//! ```text
//!   fn graded(linear: vec3<f32>) -> vec3<f32> {
//!       let d = srgb_encode(linear);
//!       let d = axiom_lut_apply(d, AXIOM_LUT_SIZE, AXIOM_LUT_STRENGTH);   // <- here
//!       let f = max((d - vec3<f32>(params.grade.w)) / ...
//! ```
//!
//! **Before, not after**, and the ordering is the correctness. Three reasons,
//! in descending force:
//!
//! 1. **The source feeds the LUT raw AgX output and nothing else.** There is no
//!    stage between `owLinearToSrgb` and `sampleLut`. Any term inserted ahead of
//!    it hands the table code values it was never authored against.
//! 2. **`FramePostProcess` has no counterpart in the source chain.** It is
//!    Axiom's *app-authored* whole-frame grade — the thing an app dials per
//!    scene. The LUT is the engine's film print. A print goes on first and the
//!    colourist works on top of it, not the reverse.
//! 3. **The LUT is the only stage of the two that is absolutely calibrated.**
//!    `FramePostProcess` packs the identity when unauthored, so putting it
//!    second costs nothing in the default case and preserves the source exactly;
//!    putting the LUT second would make the frame depend on whether an app had
//!    authored a grade, which the reference's picture does not.
//!
//! It is emphatically **not** in the bloom chain: bloom is added to the scene in
//! *linear light* at `composite.js:126`, eleven lines and one tone map earlier.
//!
//! # The half-texel inset is the thing to get right
//!
//! `composite.js:32-36`, to the character:
//!
//! ```text
//!   vec3 uvw = clamp( c, 0.0, 1.0 ) * ( ( n - 1.0 ) / n ) + ( 0.5 / n );
//! ```
//!
//! A 33³ texture's texel *centres* live at `(i + 0.5) / 33` for `i` in `0..=32`.
//! Input `0.0` must land on centre 0 and input `1.0` on centre 32, so the map is
//! a scale by `32/33` and a shift by `0.5/33`. [`inset_uvw`] is that, with the
//! source's grouping — `(n - 1.0) / n` is a division of two runtime floats, not
//! the folded constant `0.969696…`.
//!
//! Sampling without the inset (`uvw = c`) is the classic LUT port defect and it
//! **looks plausible**: the identity still comes out roughly identity, the grade
//! still reads as the right grade. What actually happens is that the whole table
//! is compressed by one texel — input `0` lands half a texel *below* centre 0
//! and clamps, input `1` lands half a texel *above* centre 32 and clamps — so
//! every interior sample is off by half a texel and both ends are flat. On a
//! smooth grade that is a fraction of a code value in the mid-tones and a
//! visible crush at the extremes, which is precisely why it survives review.
//!
//! # The format is a real 3D texture, not a strip and not a tile grid
//!
//! `createGradeLut` builds a `THREE.Data3DTexture( data, 33, 33, 33 )` — RGBA8,
//! `LinearFilter` both ways, `ClampToEdgeWrapping` on all three axes,
//! `unpackAlignment = 1`. The brief's warning about strip/tile layouts does not
//! bite here because there is no image: the LUT is *computed*, so it never
//! passes through a 2D encoding at all.
//!
//! The write order **is** the layout, though, and it is `z` outer, `y`, `x`
//! inner (`lut.js:153-163`), four bytes per entry. So R varies fastest, along
//! `x`; G along `y`; B along `z`; and the sample coordinate is `(r, g, b)`.
//! [`texel_index`] is that address, and [`GRADE_LUT_BYTES_PER_ROW`] is the `33 *
//! 4 = 132` a `write_texture` must be told.
//!
//! # The table is built in `f64`, then quantised once
//!
//! `applyGrade` is JavaScript: every `Math.pow`, every multiply, every add is
//! **`f64`**, and the only narrowing in the whole file is
//! `Math.round( … * 255 )` into a `Uint8Array`. Storage width is part of the
//! algorithm, so [`grade`] and [`ShoulderParams`] are `f64` throughout and
//! [`quantize`] is the single narrowing. Computing the table in `f32` moves
//! entries by a code value here and there — small, permanent, and invisible.
//!
//! The *sampling* is the other side of that line and is `f32`: it happens on the
//! GPU, on bytes.
//!
//! # Where this is bound
//!
//! `post_chain`'s **HDR composite arm**, and nowhere else: the table is uploaded
//! as a 33^3 `Rgba8Unorm` volume at chain build (it is a table, not a target, so
//! it costs no pass), and `axiom_lut_apply` runs on `srgb_encode(tone)` before
//! the frame's grade terms. `tests::the_lut_reaches_only_the_hdr_composite_and_in_the_right_order`
//! pins that ordering.

// CPU<->GPU parity on a real adapter. Behind `offscreen` because it needs one;
// the table generation and the grade above are pure and are covered natively.
#[cfg(all(test, feature = "offscreen"))]
mod parity;

/// The LUT's edge length: `const SIZE = 33` (`lut.js:16`).
///
/// 33 and not 32 because a `2^k + 1` grid puts a sample exactly on both ends and
/// exactly on the centre, which is what makes an identity LUT exactly identity
/// at those points. It is also why the inset's `(n - 1) / n` is `32/33` and not
/// a power of two.
pub(crate) const SIZE: usize = 33;

/// Bytes in the finished table: `33 * 33 * 33 * 4` (`lut.js:151`).
pub(crate) const GRADE_LUT_BYTES: usize = SIZE * SIZE * SIZE * 4;

/// `bytes_per_row` for a `write_texture` of the table: `33 * 4`.
///
/// Not a multiple of 256. `Queue::write_texture` imposes no row alignment (that
/// is a `copy_buffer_to_texture` rule), which is what lets the table upload as
/// one call; a caller that routes it through a staging *buffer* instead must pad
/// each row to 256 itself.
pub(crate) const GRADE_LUT_BYTES_PER_ROW: u32 = SIZE as u32 * 4;

/// Rec.709 luminance weights: `const LUM = [0.2126, 0.7152, 0.0722]`
/// (`lut.js:25`). They sum to exactly `1`, which is what makes the saturation
/// stage luminance-preserving.
const LUM: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// The composite's shipped LUT strength, `settings.lutStrength = 1.0`
/// (`index.js:384`, applied at `index.js:848`) — the LUT is applied in full.
pub(crate) const LUT_STRENGTH: f32 = 1.0;

/// `createComposite`'s constructor default, `uGrade.y = 0.85`
/// (`composite.js:333`), which `index.js:848` overwrites on the first frame.
///
/// Kept because it differs from [`LUT_STRENGTH`], and a port that carries only
/// one of the two numbers has silently picked which frames are right.
pub(crate) const CONSTRUCTOR_LUT_STRENGTH: f32 = 0.85;

/// One entry of `GRADE_PRESETS` (`lut.js:27-85`), in the source's field order.
///
/// Every field is `f64` because the whole grade is evaluated in JavaScript
/// arithmetic before anything is narrowed. The comments the source attaches to
/// these numbers are measurements, not opinions, and they are reproduced on the
/// fields because they are the only record of *why* each is where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GradePreset {
    /// ASC-CDL slope, per channel.
    pub(crate) slope: [f64; 3],
    /// ASC-CDL offset, per channel.
    pub(crate) offset: [f64; 3],
    /// ASC-CDL power, per channel.
    pub(crate) power: [f64; 3],
    /// Additive tint applied where luminance is low.
    pub(crate) shadow_tint: [f64; 3],
    /// Additive tint applied where luminance is high.
    pub(crate) highlight_tint: [f64; 3],
    /// **Display-space** saturation. Well over unity on purpose: "AgX's
    /// inset/outset pair is a *desaturating* transform by construction and the
    /// shoulder takes another chunk out of anything bright: measured on the
    /// 16:30 frame the zenith came out of the tone map at B-R = +15 code values
    /// for a sky whose scene radiance is 3:1 blue over red."
    pub(crate) saturation: f64,
    /// Global contrast, as a power about [`Self::pivot`].
    pub(crate) contrast: f64,
    /// The **code value** that must not move. "AgX puts 18% scene grey near 0.50
    /// display, so that is where the pivot belongs" — at 0.42 the contrast was
    /// really an exposure lift and 18% scene grey landed on code 153 instead of
    /// ~120.
    pub(crate) pivot: f64,
    /// How much chroma the shoulder takes. A tenth: 0.28 "turned the sunset into
    /// a cream void and the noon zenith into grey."
    pub(crate) highlight_desat: f64,
    /// Toe lift, in code values / 255. `0.008` is two codes — "visible as 'not a
    /// hole', invisible as haze."
    pub(crate) toe: f64,
    /// Shoulder knee, in post-contrast units.
    pub(crate) shoulder: f64,
    /// Shoulder softness, in the same units as the knee. `None` selects the
    /// source's `??` fallback, `0.55 * (1 - k)` (`lut.js:93`).
    pub(crate) shoulder_soft: Option<f64>,
}

/// `GRADE_PRESETS.default` (`lut.js:30-84`) — the only preset the source
/// defines and the only one `index.js:214` asks for.
pub(crate) const SHIPPED_PRESET: GradePreset = GradePreset {
    slope: [1.0, 0.995, 0.985],
    offset: [-0.004, -0.002, 0.004],
    power: [1.0, 1.005, 1.02],
    shadow_tint: [-0.001, 0.006, 0.022],
    highlight_tint: [0.030, 0.014, -0.006],
    saturation: 1.20,
    contrast: 1.28,
    pivot: 0.50,
    highlight_desat: 0.10,
    toe: 0.008,
    shoulder: 0.60,
    shoulder_soft: Some(1.20),
};

/// The three shoulder constants derived from a preset (`lut.js:91-98`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShoulderParams {
    /// The knee, clamped into `[0.05, 0.98]`.
    pub(crate) knee: f64,
    /// The softness, floored at `1e-3`.
    pub(crate) softness: f64,
    /// The normaliser that makes the curve reach **exactly** `1.0` at input
    /// `1.0`.
    pub(crate) norm: f64,
}

/// `shoulderParams( g )` (`lut.js:91-98`).
///
/// `norm` is what the source's long comment is about: without dividing the
/// exponential through by its own value at the largest in-gamut output, "input
/// 1.0 landed on 0.66 + 0.34 * (1 - e^-1) = 0.875 -> 223 code values, and the
/// sky, the sunlit stucco and every specular highlight piled up inside 220..232."
///
/// `cMax` is `pivot * pow( 1 / pivot, contrast )` — the largest post-contrast
/// value an in-gamut input can produce. The `1 / pivot` is a **division**, kept
/// as one.
pub(crate) fn shoulder_params(preset: GradePreset) -> ShoulderParams {
    let knee = f64::min(0.98, f64::max(0.05, preset.shoulder));
    // `??` is lazy and `unwrap_or` is not, but the fallback is pure arithmetic
    // with no side effect, so eager evaluation is the same program.
    let softness = f64::max(
        1e-3,
        preset.shoulder_soft.unwrap_or(0.55 * (1.0 - knee)),
    );
    let c_max = preset.pivot * f64::powf(1.0 / preset.pivot, preset.contrast);
    let norm = 1.0 - f64::exp(-f64::max(c_max - knee, 1e-3) / softness);
    ShoulderParams {
        knee,
        softness,
        norm,
    }
}

/// The filmic S-curve, one channel (`lut.js:132-143`).
///
/// Contrast as a power about the pivot — so the pivot itself never moves — then
/// a normalised exponential shoulder above the knee, then the toe lift.
///
/// The `t <= 0` guard and the `c > sh.k` guard are both selects here rather than
/// branches, and both alternatives are finite for every input the table
/// generator produces: `powf(0.0, contrast)` is `0.0`, and `exp` of a positive
/// argument below the knee is large but bounded.
pub(crate) fn scurve(x: f64, preset: GradePreset, shoulder: ShoulderParams) -> f64 {
    let t = f64::max(0.0, x);
    let powered = preset.pivot * f64::powf(t / preset.pivot, preset.contrast);
    let contrasted = [powered, 0.0][usize::from(t <= 0.0)];
    let rolled = shoulder.knee
        + (1.0 - shoulder.knee)
            * ((1.0 - f64::exp(-(contrasted - shoulder.knee) / shoulder.softness))
                / shoulder.norm);
    let c = [contrasted, rolled][usize::from(contrasted > shoulder.knee)];
    preset.toe + (1.0 - preset.toe) * f64::min(1.0, f64::max(0.0, c))
}

/// `applyGrade( rgb, g, sh )` (`lut.js:100-145`) — **the semantic definition**
/// of one LUT entry, in `f64`.
///
/// Five stages, in the source's order, and the order is not interchangeable:
///
/// 1. **ASC-CDL** slope / offset / power, per channel, with the `max(0, …)`
///    floor before the power so a negative offset cannot produce a NaN.
/// 2. **Split toning** by luminance — cool shadows, warm highlights. The
///    luminance `l` is computed **once, before any channel is written**, so the
///    three additions cannot contaminate one another.
/// 3. **Saturation** about a second luminance `l2`, also computed before any
///    write.
/// 4. **Highlight desaturation**, `highlightDesat * l2³`, toward `l2` — and it
///    reuses the *pre-saturation* `l2` against the *post-saturation* channels.
///    That is deliberate and correct rather than a stale variable: [`LUM`] sums
///    to one, so saturation about `l2` leaves the luminance at `l2`, and
///    recomputing it would be the same number by a longer route.
/// 5. **The S-curve**, per channel.
pub(crate) fn grade(rgb: [f64; 3], preset: GradePreset, shoulder: ShoulderParams) -> [f64; 3] {
    // 1. ASC CDL: slope / offset / power.
    let cdl = [0_usize, 1, 2].map(|i| {
        f64::powf(
            f64::max(0.0, rgb[i] * preset.slope[i] + preset.offset[i]),
            preset.power[i],
        )
    });

    // 2. Split toning by luminance.
    let l = cdl[0] * LUM[0] + cdl[1] * LUM[1] + cdl[2] * LUM[2];
    let shadow_w = f64::powf(1.0 - f64::min(1.0, l), 2.2);
    let high_w = f64::powf(f64::min(1.0, l), 2.0);
    let toned =
        [0_usize, 1, 2].map(|i| cdl[i] + (preset.shadow_tint[i] * shadow_w + preset.highlight_tint[i] * high_w));

    // 3. Saturation about luminance.
    let l2 = toned[0] * LUM[0] + toned[1] * LUM[1] + toned[2] * LUM[2];
    let saturated = [0_usize, 1, 2].map(|i| l2 + (toned[i] - l2) * preset.saturation);

    // 4. Highlights lose saturation the way film does.
    let hd = preset.highlight_desat * f64::powf(f64::min(1.0, f64::max(0.0, l2)), 3.0);
    let desaturated = [0_usize, 1, 2].map(|i| saturated[i] + (l2 - saturated[i]) * hd);

    // 5. The filmic S-curve.
    [0_usize, 1, 2].map(|i| scurve(desaturated[i], preset, shoulder))
}

/// `Math.round( Math.min( 1, Math.max( 0, v ) ) * 255 )` (`lut.js:158-160`) —
/// the single narrowing in the whole file.
///
/// `Math.round` breaks ties toward `+Infinity` and Rust's `f64::round` breaks
/// them away from zero. Those differ **only for a negative argument**, and the
/// clamp above makes one impossible, so the two are the same function here. The
/// distinction is recorded because it is a named trap and because the clamp is
/// what disarms it.
pub(crate) fn quantize(value: f64) -> u8 {
    (f64::min(1.0, f64::max(0.0, value)) * 255.0).round() as u8
}

/// The byte offset of entry `(x, y, z)`'s red channel (`lut.js:151-163`).
///
/// `z` outer, `y`, `x` inner, four bytes per entry. R runs along `x`, G along
/// `y`, B along `z`, which is what makes `(r, g, b)` the sample coordinate.
pub(crate) fn texel_index(x: usize, y: usize, z: usize) -> usize {
    ((z * SIZE + y) * SIZE + x) * 4
}

/// `createGradeLut( preset )` (`lut.js:147-177`) — the finished RGBA8 table,
/// [`GRADE_LUT_BYTES`] long.
///
/// The lattice input is `[x / (n - 1), y / (n - 1), z / (n - 1)]`, so it spans
/// `0.0..=1.0` inclusive at both ends. Alpha is a constant `255`, as the source
/// writes it — the sampler never reads it, but the texture is `RGBAFormat` and
/// the byte is part of the layout.
pub(crate) fn grade_lut(preset: GradePreset) -> Vec<u8> {
    let shoulder = shoulder_params(preset);
    let last = (SIZE - 1) as f64;
    (0..SIZE)
        .flat_map(|z| (0..SIZE).flat_map(move |y| (0..SIZE).map(move |x| (x, y, z))))
        .flat_map(|(x, y, z)| {
            let out = grade(
                [x as f64 / last, y as f64 / last, z as f64 / last],
                preset,
                shoulder,
            );
            [quantize(out[0]), quantize(out[1]), quantize(out[2]), 255]
        })
        .collect()
}

/// `sampleLut( c )`'s coordinate (`composite.js:33-34`) — the half-texel inset.
///
/// `clamp( c, 0, 1 ) * ( ( n - 1 ) / n ) + ( 0.5 / n )`, with the source's
/// grouping and both divisions kept as divisions. `n` is a float — the shader
/// receives it in `uGrade.w` — so this is not a compile-time fold.
///
/// See the module docs for what a port that omits this looks like, and why it
/// looks fine.
pub(crate) fn inset_uvw(c: [f32; 3], n: f32) -> [f32; 3] {
    c.map(|lane| f32::min(f32::max(lane, 0.0), 1.0) * ((n - 1.0) / n) + (0.5 / n))
}

/// One `Rgba8Unorm` texel of the table as the sampler sees it: `byte / 255`.
fn texel(table: &[u8], x: usize, y: usize, z: usize) -> [f32; 3] {
    let at = texel_index(x, y, z);
    [0_usize, 1, 2].map(|lane| f32::from(table[at + lane]) / 255.0)
}

/// The hardware's trilinear fetch of the table, modelled on the CPU — the
/// semantic definition the WGSL's `textureSampleLevel` is compared against.
///
/// A `LinearFilter` / `ClampToEdgeWrapping` 3D fetch is: scale the normalised
/// coordinate into texel space and shift by half a texel, split into an integer
/// corner and a fraction, clamp both corners to the edge, and blend the eight
/// with the fraction. The clamp is what `ClampToEdgeWrapping` means, and with
/// [`inset_uvw`] in front it can only ever bite at the two extreme inputs — at
/// which the fraction is zero, so the clamped corner carries no weight.
///
/// The blend order — `x` innermost, then `y`, then `z` — mirrors the texel
/// layout rather than the other way round; float addition is not associative, so
/// a different nesting is a different (if barely) number.
pub(crate) fn trilinear(table: &[u8], uvw: [f32; 3]) -> [f32; 3] {
    let n = SIZE as f32;
    let p = uvw.map(|lane| lane * n - 0.5);
    let base = p.map(f32::floor);
    let frac = [0_usize, 1, 2].map(|i| p[i] - base[i]);
    let corner = |step: usize, axis: usize| {
        let raw = base[axis] + step as f32;
        f32::min(f32::max(raw, 0.0), n - 1.0) as usize
    };
    let at = |dx: usize, dy: usize, dz: usize| {
        texel(table, corner(dx, 0), corner(dy, 1), corner(dz, 2))
    };
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [0_usize, 1, 2].map(|i| a[i] * (1.0 - t) + b[i] * t);
    let along_x = |dy: usize, dz: usize| lerp(at(0, dy, dz), at(1, dy, dz), frac[0]);
    let along_y = |dz: usize| lerp(along_x(0, dz), along_x(1, dz), frac[1]);
    lerp(along_y(0), along_y(1), frac[2])
}

/// `sampleLut( disp )` followed by `mix( disp, graded, lutStrength )`
/// (`composite.js:144-145`) — the whole display-referred grade as one function.
///
/// `disp` is the sRGB-encoded AgX output. The result is what the grain stage
/// receives.
pub(crate) fn apply(table: &[u8], disp: [f32; 3], strength: f32) -> [f32; 3] {
    let graded = trilinear(table, inset_uvw(disp, SIZE as f32));
    // GLSL `mix( x, y, a )` is `x * ( 1 - a ) + y * a`, written out.
    [0_usize, 1, 2].map(|i| disp[i] * (1.0 - strength) + graded[i] * strength)
}

/// The LUT fetch as WGSL: the inset, the sample, and the strength blend.
///
/// Binding-free arithmetic plus **one** texture and sampler pair, so it
/// concatenates in front of whichever pass needs it, exactly as
/// [`crate::agx::AGX_WGSL`] does.
///
/// # What the composite must supply, and where
///
/// The consumer is [`crate::post_chain`]'s `graded()`, and the insertion point
/// is its **first** statement — see the module docs for why before and not
/// after. Concretely, three edits the orchestrator owns:
///
/// 1. Concatenate this text into the composite source **on the HDR arm only**,
///    alongside [`crate::agx::AGX_WGSL`] — the LUT is calibrated against AgX and
///    means nothing in front of the LDR rolloff.
/// 2. Add `const AXIOM_LUT_SIZE: f32 = 33.0;` and
///    `const AXIOM_LUT_STRENGTH: f32 = 1.0;` to `tone_constants`, from [`SIZE`]
///    and [`LUT_STRENGTH`] rather than retyped.
/// 3. Insert `let d = axiom_lut_apply(d, AXIOM_LUT_SIZE, AXIOM_LUT_STRENGTH);`
///    immediately after `let d = srgb_encode(linear);` in `graded()`.
///
/// # What the frame graph must supply
///
/// One `Rgba8Unorm` **3D** texture, `33 x 33 x 33`, uploaded once at startup
/// from [`grade_lut`] with `bytes_per_row` [`GRADE_LUT_BYTES_PER_ROW`] and
/// `rows_per_image` [`SIZE`]; a `Linear`/`Linear` sampler with
/// `ClampToEdge` on all three axes. `33` is inside
/// `Limits::downlevel_webgl2_defaults().max_texture_dimension_3d` (256), so the
/// browser arm can hold it — but a 3D texture is the one resource in this slice
/// whose WebGL2 support is worth confirming on the real device before the arm is
/// switched on, rather than assuming.
///
/// The `@group(1)` index below is a **placeholder**: this text has no composite
/// to belong to yet, and the composite's own bindings live in group 0. Whoever
/// splices it renumbers the group to whatever is free there — the group index is
/// the one thing in this string that is not transcribed from the source and
/// carries no meaning of its own.
pub(crate) const GRADE_LUT_WGSL: &str = r#"
// The display grading LUT, from Claude-of-Duty `src/render/lut.js` and the
// `sampleLut` helper at `src/render/composite.js:32-36`.
//
// DISPLAY-REFERRED. The input is sRGB-encoded AgX output (`composite.js:142`),
// not linear light and not scene radiance. See `lut.rs`.

// Group 2, because `post_chain` already spends 0 on the source/params and 1 on
// the bloom. This chunk is spliced into that composite and nowhere else, so the
// number lives here rather than being parameterised.
@group(2) @binding(0) var axiom_lut_texture: texture_3d<f32>;
@group(2) @binding(1) var axiom_lut_sampler: sampler;

// The half-texel inset (`composite.js:33-34`). Both divisions are kept as
// divisions and the grouping is the source's; `n` is a float, so none of this
// folds at compile time.
//
// Without the `+ 0.5 / n` term every interior sample lands half a texel off and
// both ends clamp flat. It still looks like the right grade. See `lut.rs`.
fn axiom_lut_uvw(c: vec3<f32>, n: f32) -> vec3<f32> {
    let clamped = vec3<f32>(
        min(max(c.x, 0.0), 1.0),
        min(max(c.y, 0.0), 1.0),
        min(max(c.z, 0.0), 1.0),
    );
    return clamped * ((n - 1.0) / n) + (0.5 / n);
}

// `sampleLut( c )` (`composite.js:32-36`).
fn axiom_lut_sample(c: vec3<f32>, n: f32) -> vec3<f32> {
    return textureSampleLevel(axiom_lut_texture, axiom_lut_sampler, axiom_lut_uvw(c, n), 0.0).rgb;
}

// `sampleLut` + the strength blend (`composite.js:144-145`). GLSL
// `mix( x, y, a )` is `x * ( 1 - a ) + y * a`, written out.
fn axiom_lut_apply(disp: vec3<f32>, n: f32, strength: f32) -> vec3<f32> {
    let graded = axiom_lut_sample(disp, n);
    return disp * (1.0 - strength) + graded * strength;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The table's shape and layout: 33³ RGBA8, `x` fastest, `z` slowest,
    /// alpha a constant 255.
    #[test]
    fn the_table_is_a_thirty_three_cubed_rgba8_lattice_with_x_fastest() {
        let table = grade_lut(SHIPPED_PRESET);
        assert_eq!(table.len(), GRADE_LUT_BYTES);
        assert_eq!(GRADE_LUT_BYTES, 143_748);
        assert_eq!(GRADE_LUT_BYTES_PER_ROW, 132);

        // The addressing is `((z * n + y) * n + x) * 4`.
        assert_eq!(texel_index(0, 0, 0), 0);
        assert_eq!(texel_index(1, 0, 0), 4, "x is the fastest axis");
        assert_eq!(texel_index(0, 1, 0), SIZE * 4, "then y");
        assert_eq!(texel_index(0, 0, 1), SIZE * SIZE * 4, "then z");
        assert_eq!(texel_index(32, 32, 32), GRADE_LUT_BYTES - 4);

        // Alpha is 255 everywhere.
        let opaque = table.chunks_exact(4).all(|entry| entry[3] == 255);
        assert!(opaque, "every entry's alpha must be 255");

        // R runs along x, G along y, B along z — which is what makes (r, g, b)
        // the sample coordinate rather than some permutation of it.
        let red_low = texel(&table, 0, 16, 16)[0];
        let red_high = texel(&table, 32, 16, 16)[0];
        assert!(red_high > red_low, "red must rise along x: {red_low} -> {red_high}");
        let green_low = texel(&table, 16, 0, 16)[1];
        let green_high = texel(&table, 16, 32, 16)[1];
        assert!(green_high > green_low, "green must rise along y: {green_low} -> {green_high}");
        let blue_low = texel(&table, 16, 16, 0)[2];
        let blue_high = texel(&table, 16, 16, 32)[2];
        assert!(blue_high > blue_low, "blue must rise along z: {blue_low} -> {blue_high}");
    }

    /// **The half-texel inset.** Input 0 lands exactly on texel centre 0, input
    /// 1 exactly on texel centre 32, and the map is `32/33` and `0.5/33`.
    #[test]
    fn the_inset_maps_the_unit_interval_onto_the_texel_centres() {
        let n = SIZE as f32;
        let low = inset_uvw([0.0, 0.0, 0.0], n);
        let high = inset_uvw([1.0, 1.0, 1.0], n);
        // Centre of texel 0 is 0.5/33; centre of texel 32 is 32.5/33.
        assert_eq!(low, [0.5 / 33.0; 3]);
        let expected_high = 32.5_f32 / 33.0;
        let high_delta = (high[0] - expected_high).abs();
        assert!(
            high_delta < 1e-7,
            "input 1.0 must land on texel centre 32 ({expected_high}), got {} (delta {high_delta})",
            high[0]
        );

        // Out-of-range input clamps before the inset, not after.
        assert_eq!(inset_uvw([-1.0, 2.0, 0.0], n), [low[0], high[1], low[2]]);

        // In texel space those are exactly texel 0 and texel 32, which is the
        // property the whole inset exists to produce.
        let texel_low = low[0] * n - 0.5;
        let texel_high = high[0] * n - 0.5;
        assert!(texel_low.abs() < 1e-5, "input 0 must sit on texel 0, got {texel_low}");
        assert!(
            (texel_high - 32.0).abs() < 1e-4,
            "input 1 must sit on texel 32, got {texel_high}"
        );
    }

    /// The inset is not optional, and this is what it costs to omit it: without
    /// it, both ends of the range are fetched from outside the table and go flat.
    /// The test pins the *magnitude* so the defect cannot be dismissed as noise.
    ///
    /// **Measured at black, not at mid-grey.** The inset is a contraction about
    /// the centre — `c * ((n-1)/n) + 0.5/n` — so `c = 0.5` is its **fixed
    /// point** and the one input where omitting it changes nothing at all.
    /// Measuring there reads zero and concludes the inset is free.
    #[test]
    fn omitting_the_inset_moves_every_sample_by_half_a_texel() {
        let table = grade_lut(SHIPPED_PRESET);
        let n = SIZE as f32;
        // The defect: `uvw = c`, which a plausible port writes.
        let naive = [0.0_f32, 0.0, 0.0];
        let correct = inset_uvw([0.0, 0.0, 0.0], n);
        let offset_texels = (correct[0] - naive[0]) * n;
        assert!(
            (offset_texels - 0.5).abs() < 1e-4,
            "the omission is worth half a texel, measured {offset_texels}"
        );
        // The offset is not a constant half texel — it is exactly `0.5 - c`
        // texels, half at black, zero at mid-grey, minus half at white. Stating
        // the identity rather than one sample is what keeps the fixed point from
        // being rediscovered as a surprise.
        (0..=10).for_each(|i| {
            let c = i as f32 / 10.0;
            let offset = (inset_uvw([c; 3], n)[0] - c) * n;
            assert!(
                (offset - (0.5 - c)).abs() < 1e-4,
                "at c = {c} the offset must be {} texels, measured {offset}",
                0.5 - c
            );
        });
        // And it is a real difference in the fetched colour — measured a quarter
        // of the way up, NOT at the endpoint above.
        //
        // The two ends are where the *coordinate* error is largest and the
        // *colour* error is smallest, because the sampler clamps: at `c = 0` the
        // naive `uvw = 0` and the correct `0.5/n` both resolve to texel 0's
        // centre, so the fetch is identical and the defect is invisible exactly
        // where it is worst. In between there is nothing to clamp against, and
        // the offset `(0.5 - c)` texels moves the fetch for real.
        let quarter = 0.25_f32;
        let with = trilinear(&table, inset_uvw([quarter; 3], n));
        let without = trilinear(&table, [quarter; 3]);
        let delta = (with[0] - without[0]).abs();
        assert!(
            delta > 1e-4,
            "the two fetches must genuinely differ; got {with:?} vs {without:?}"
        );
    }

    /// The trilinear reference lands exactly on a stored texel when the input is
    /// a lattice point — which is the only way to know the addressing, the
    /// inset and the filter agree with one another.
    #[test]
    fn a_lattice_input_fetches_its_own_texel_exactly() {
        let table = grade_lut(SHIPPED_PRESET);
        let n = SIZE as f32;
        let last = (SIZE - 1) as f32;
        let worst = (0..SIZE)
            .step_by(4)
            .flat_map(|i| {
                (0..SIZE)
                    .step_by(8)
                    .map(move |j| (i, j))
            })
            .map(|(i, j)| {
                let coord = [i as f32 / last, j as f32 / last, i as f32 / last];
                let fetched = trilinear(&table, inset_uvw(coord, n));
                let stored = texel(&table, i, j, i);
                [0_usize, 1, 2]
                    .map(|lane| (fetched[lane] - stored[lane]).abs())
                    .into_iter()
                    .fold(0.0_f32, f32::max)
            })
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1e-6,
            "a lattice input must fetch its own texel; worst deviation {worst}"
        );
    }

    /// The shoulder normaliser is the point of `shoulderParams`: the curve must
    /// reach exactly 1.0 at input 1.0, and the source's own arithmetic for the
    /// un-normalised version is what it is fixing.
    #[test]
    fn the_shoulder_is_normalised_so_display_white_is_reachable() {
        let preset = SHIPPED_PRESET;
        let shoulder = shoulder_params(preset);
        assert_eq!(shoulder.knee, 0.60);
        assert_eq!(shoulder.softness, 1.20);

        // cMax = 0.5 * (1/0.5)^1.28 = 0.5 * 2^1.28.
        let c_max = 0.5 * f64::powf(2.0, 1.28);
        let expected_norm = 1.0 - f64::exp(-(c_max - 0.60) / 1.20);
        assert!(
            (shoulder.norm - expected_norm).abs() < 1e-15,
            "norm must be {expected_norm}, got {}",
            shoulder.norm
        );

        // Input 1.0 must land on 1.0 exactly (up to the toe's remap, which is
        // `toe + (1 - toe) * 1 == 1`).
        let white = scurve(1.0, preset, shoulder);
        assert!(
            (white - 1.0).abs() < 1e-12,
            "display white must be reachable; scurve(1.0) = {white}"
        );
        assert_eq!(quantize(white), 255, "and must quantise to code 255");

        // The un-normalised form the source replaced asymptotes short of white:
        // 0.66 + 0.34 * (1 - e^-1) = 0.875 -> code 223.
        let old_curve = 0.66 + 0.34 * (1.0 - f64::exp(-1.0_f64));
        assert!(
            (old_curve - 0.875).abs() < 1e-3,
            "the recorded defect is 0.875, computed {old_curve}"
        );
        assert_eq!(quantize(old_curve), 223, "which is the 223 the source names");
    }

    /// The pivot does not move, the toe lifts black off zero, and the curve is
    /// monotone — the three properties that make it a usable grade rather than
    /// a lookup that happens to fill a table.
    #[test]
    fn the_scurve_holds_the_pivot_lifts_the_toe_and_stays_monotone() {
        let preset = SHIPPED_PRESET;
        let shoulder = shoulder_params(preset);

        // The pivot is a fixed point of the contrast power, modulo the toe remap.
        let at_pivot = scurve(0.50, preset, shoulder);
        let expected = preset.toe + (1.0 - preset.toe) * 0.50;
        assert!(
            (at_pivot - expected).abs() < 1e-12,
            "the pivot must not move: expected {expected}, got {at_pivot}"
        );

        // Black lifts to the toe — two code values, "not a hole".
        let black = scurve(0.0, preset, shoulder);
        assert_eq!(black, preset.toe);
        assert_eq!(quantize(black), 2, "0.008 * 255 rounds to two code values");

        // Negative input is floored, not reflected.
        assert_eq!(scurve(-0.5, preset, shoulder), preset.toe);

        // Monotone across the whole domain.
        let samples: Vec<f64> = (0..=64)
            .map(|i| scurve(f64::from(i) / 64.0, preset, shoulder))
            .collect();
        let rising = samples.windows(2).all(|w| w[1] >= w[0]);
        assert!(rising, "the S-curve must be monotone: {samples:?}");
    }

    /// The grade's stages do what their comments claim: cool shadows, warm
    /// highlights, saturation above unity, and highlight desaturation that only
    /// bites at the top.
    #[test]
    fn the_grade_cools_the_shadows_warms_the_highlights_and_lifts_saturation() {
        let preset = SHIPPED_PRESET;
        let shoulder = shoulder_params(preset);

        // A dark neutral picks up blue: shadowTint is [-0.001, 0.006, 0.022].
        let dark = grade([0.12, 0.12, 0.12], preset, shoulder);
        assert!(
            dark[2] > dark[0],
            "shadows must go cool (blue over red): {dark:?}"
        );

        // A bright neutral picks up red: highlightTint is [0.030, 0.014, -0.006].
        let bright = grade([0.88, 0.88, 0.88], preset, shoulder);
        assert!(
            bright[0] > bright[2],
            "highlights must go warm (red over blue): {bright:?}"
        );

        // Saturation is over unity, so a saturated input comes out further from
        // its own luminance than it went in.
        let input = [0.70_f64, 0.40, 0.40];
        let out = grade(input, preset, shoulder);
        let in_spread = input[0] - input[1];
        let out_spread = out[0] - out[1];
        assert!(
            out_spread > in_spread,
            "display saturation must widen the spread: {in_spread} -> {out_spread}"
        );
    }

    /// Highlight desaturation reuses the pre-saturation luminance against the
    /// post-saturation channels. That is only sound because [`LUM`] sums to
    /// exactly one; pin the sum, because it is the assumption.
    #[test]
    fn the_luminance_weights_sum_to_one_which_is_what_makes_saturation_preserving() {
        assert_eq!(LUM[0] + LUM[1] + LUM[2], 1.0);
        let preset = SHIPPED_PRESET;
        let shoulder = shoulder_params(preset);
        // At zero desaturation strength the stage is the identity; the shipped
        // 0.10 is a tenth, which the source's comment says is the point.
        assert_eq!(preset.highlight_desat, 0.10);
        // The desat weight is l2 cubed, so it is negligible in the shadows.
        let hd_dark = preset.highlight_desat * f64::powf(0.12, 3.0);
        let hd_bright = preset.highlight_desat * f64::powf(0.92, 3.0);
        assert!(
            hd_bright > hd_dark * 100.0,
            "the cube must confine desaturation to the top: {hd_dark} vs {hd_bright}"
        );
        // And the graded output stays in gamut regardless.
        let in_gamut = grade([1.0, 0.0, 0.0], preset, shoulder)
            .iter()
            .all(|v| *v >= 0.0 && *v <= 1.0);
        assert!(in_gamut, "the grade must not leave the unit cube");
    }

    /// The generator quantises **once**, straight off the `f64` grade, with no
    /// intermediate narrowing — so [`grade_lut`] and [`grade`] are one
    /// definition rather than two that agree today.
    ///
    /// This is the checkable half of "the table is built in `f64`". The other
    /// half is why it matters, which the next test measures rather than
    /// asserts.
    #[test]
    fn the_generator_is_exactly_the_reference_quantised_once() {
        let preset = SHIPPED_PRESET;
        let shoulder = shoulder_params(preset);
        let table = grade_lut(preset);
        let last = (SIZE - 1) as f64;
        // A sparse sweep of the lattice, including all eight corners.
        let mismatches = (0..SIZE)
            .step_by(8)
            .flat_map(|z| {
                (0..SIZE)
                    .step_by(8)
                    .flat_map(move |y| (0..SIZE).step_by(8).map(move |x| (x, y, z)))
            })
            .filter(|(x, y, z)| {
                let out = grade(
                    [*x as f64 / last, *y as f64 / last, *z as f64 / last],
                    preset,
                    shoulder,
                );
                let at = texel_index(*x, *y, *z);
                (0..3).any(|lane| table[at + lane] != quantize(out[lane]))
            })
            .count();
        assert_eq!(
            mismatches, 0,
            "every sampled entry must be exactly quantize(grade(lattice))"
        );
    }

    /// **Why the `f64` matters, measured rather than asserted.**
    ///
    /// The evaluation width only changes a byte where an entry sits close enough
    /// to a quantisation boundary for the last bits to decide which side it
    /// falls. This counts that population: entries whose `value * 255` lands
    /// within a thousandth of a `k + 0.5` tie.
    ///
    /// It is deliberately *not* an assertion that an `f32` build differs — that
    /// would be a claim about arithmetic this test does not perform. It is the
    /// justification for keeping the source's width: the boundary population is
    /// non-empty, so the width is load-bearing for some entries, and there is no
    /// reason to find out which by guessing.
    #[test]
    fn some_entries_sit_close_enough_to_a_tie_for_the_evaluation_width_to_decide_them() {
        let preset = SHIPPED_PRESET;
        let shoulder = shoulder_params(preset);
        let last = (SIZE - 1) as f64;
        let near_a_tie = (0..SIZE)
            .flat_map(|z| (0..SIZE).map(move |y| (y, z)))
            .flat_map(|(y, z)| {
                (0..SIZE).map(move |x| {
                    grade(
                        [x as f64 / last, y as f64 / last, z as f64 / last],
                        preset,
                        shoulder,
                    )
                })
            })
            .flat_map(|out| out.into_iter().collect::<Vec<f64>>())
            .filter(|v| {
                let scaled = f64::min(1.0, f64::max(0.0, *v)) * 255.0;
                (scaled - scaled.floor() - 0.5).abs() < 1e-3
            })
            .count();
        assert!(
            near_a_tie > 0,
            "no entry sits near a quantisation tie, so this justification is stale; \
             counted {near_a_tie} of {} lanes",
            SIZE * SIZE * SIZE * 3
        );
    }

    /// Quantisation clamps first, so the JS/Rust rounding-tie difference cannot
    /// bite; and the ends land on 0-plus-toe and 255.
    #[test]
    fn quantisation_clamps_before_it_rounds() {
        assert_eq!(quantize(0.0), 0);
        assert_eq!(quantize(1.0), 255);
        assert_eq!(quantize(-5.0), 0, "out of range clamps rather than wrapping");
        assert_eq!(quantize(5.0), 255);
        // Every code value round-trips.
        let round_trips = (0..=255_u16).all(|code| quantize(f64::from(code) / 255.0) == code as u8);
        assert!(round_trips, "byte / 255 must quantise back to the same byte");
    }

    /// The strength blend, and the two strengths the source carries.
    #[test]
    fn the_strength_blend_spans_ungraded_to_fully_graded() {
        let table = grade_lut(SHIPPED_PRESET);
        let disp = [0.42_f32, 0.55, 0.61];
        // Strength 0 is the exact identity — the property that makes an
        // unwired composite bit-identical to one without the LUT.
        assert_eq!(apply(&table, disp, 0.0), disp);
        // Strength 1 is the LUT outright.
        let full = apply(&table, disp, 1.0);
        let sampled = trilinear(&table, inset_uvw(disp, SIZE as f32));
        assert_eq!(full, sampled);
        // The shipped value is 1.0 and the constructor default is not.
        assert_eq!(LUT_STRENGTH, 1.0);
        assert_eq!(CONSTRUCTOR_LUT_STRENGTH, 0.85);
        assert_ne!(LUT_STRENGTH, CONSTRUCTOR_LUT_STRENGTH);
        // A partial blend sits between the two ends on every lane.
        let half = apply(&table, disp, 0.5);
        for lane in 0..3 {
            let lo = f32::min(disp[lane], full[lane]);
            let hi = f32::max(disp[lane], full[lane]);
            assert!(
                half[lane] >= lo && half[lane] <= hi,
                "lane {lane} of a half blend must lie in [{lo}, {hi}], got {}",
                half[lane]
            );
        }
    }

    /// The `??` fallback for `shoulderSoft` is dead in the shipped preset but is
    /// still part of the source, and the `1e-3` floor under it is what stops a
    /// knee of 0.98 producing a divide-by-nearly-zero.
    #[test]
    fn the_shoulder_softness_fallback_and_its_floor_are_ported() {
        // No softness authored: the fallback is 0.55 * (1 - knee).
        let fallback = GradePreset {
            shoulder_soft: None,
            ..SHIPPED_PRESET
        };
        let derived = shoulder_params(fallback);
        assert!(
            (derived.softness - 0.55 * (1.0 - 0.60)).abs() < 1e-12,
            "the fallback must be 0.55 * (1 - k), got {}",
            derived.softness
        );

        // The knee clamps into [0.05, 0.98] ...
        let high_knee = GradePreset {
            shoulder: 5.0,
            shoulder_soft: None,
            ..SHIPPED_PRESET
        };
        let clamped = shoulder_params(high_knee);
        assert_eq!(clamped.knee, 0.98);
        // ... and at 0.98 the fallback softness is 0.011, above the 1e-3 floor,
        // so the floor only bites for an explicitly tiny authored softness.
        assert!(
            (clamped.softness - 0.011).abs() < 1e-12,
            "0.55 * (1 - 0.98) is 0.011, got {}",
            clamped.softness
        );
        let tiny = GradePreset {
            shoulder_soft: Some(0.0),
            ..SHIPPED_PRESET
        };
        assert_eq!(shoulder_params(tiny).softness, 1e-3, "the floor must catch zero");

        let low_knee = GradePreset {
            shoulder: -1.0,
            ..SHIPPED_PRESET
        };
        assert_eq!(shoulder_params(low_knee).knee, 0.05);
    }

    /// The WGSL and the CPU reference must not drift into two definitions of
    /// the inset. The text scan pins the expression that matters most in this
    /// slice.
    #[test]
    fn the_wgsl_carries_the_inset_verbatim() {
        assert!(
            GRADE_LUT_WGSL.contains("return clamped * ((n - 1.0) / n) + (0.5 / n);"),
            "the inset must appear with the source's grouping and both divisions"
        );
        // `clamp` and `mix` written out, as everywhere else in this port.
        assert!(GRADE_LUT_WGSL.contains("min(max(c.x, 0.0), 1.0)"));
        assert!(GRADE_LUT_WGSL.contains("return disp * (1.0 - strength) + graded * strength;"));
        // A real 3D texture, sampled at level zero.
        assert!(GRADE_LUT_WGSL.contains("texture_3d<f32>"));
        assert!(GRADE_LUT_WGSL.contains("textureSampleLevel(axiom_lut_texture"));
        // The folded constant must NOT appear: `(n - 1) / n` is computed.
        assert!(
            !GRADE_LUT_WGSL.contains("0.969696"),
            "32/33 must not be folded into a literal"
        );
    }

    /// `lut.js` also exports `srgbToLinear` / `linearToSrgb`, and **nothing
    /// imports them** — `render/index.js:15` takes only `createGradeLut`. They
    /// are not ported, deliberately, because this crate already has exactly one
    /// definition of that curve in [`crate::surface_encode`] and a second would
    /// be the drift that module exists to prevent.
    ///
    /// The one thing worth checking before declining is that the two are the
    /// same curve. The GLSL writes the exponent `0.41666667` and Axiom writes
    /// `1.0 / 2.4`; they are the **same `f32`**, so the decision costs nothing.
    /// (The knee comparisons differ in strictness — GLSL's `step` takes the
    /// power branch at exactly `0.0031308` where the JS takes the linear one —
    /// which is a single point of measure zero.)
    #[test]
    fn the_unused_transfer_exports_are_the_curve_the_crate_already_has() {
        let source_literal = 0.416_666_67_f32;
        let axiom_form = 1.0_f32 / 2.4_f32;
        assert_eq!(
            source_literal.to_bits(),
            axiom_form.to_bits(),
            "0.41666667 and 1/2.4 must be the same f32, else the curves differ: \
             {source_literal:?} vs {axiom_form:?}"
        );
    }

    /// **The LUT reaches only the HDR composite arm, after `srgb_encode` and
    /// before the grade terms.**
    ///
    /// This replaces the deferral test that stood here ("nothing binds this
    /// yet"), which fired the moment the composite was wired — which is what it
    /// was for. The ordering it now pins is the thing a later edit is most likely
    /// to quietly reverse: the LUT is **display-referred**, calibrated to where
    /// AgX puts 18% grey, so it must run on encoded AgX output with nothing in
    /// between. Moved before the encode it grades linear light; moved after the
    /// grade terms it grades an image the preset was never fitted to. Both still
    /// look like a grade. `crate::agx`'s `only_the_hdr_composite_arm_carries_agx`
    /// is the worked precedent.
    #[test]
    fn the_lut_reaches_only_the_hdr_composite_and_in_the_right_order() {
        let post = include_str!("post_chain.rs");
        // The other present paths still must not carry it.
        [
            include_str!("upscale.rs"),
            include_str!("surface_encode.rs"),
            include_str!("scene_wgsl.rs"),
        ]
        .iter()
        .for_each(|source| {
            assert!(
                !(source.contains("GRADE_LUT_WGSL") | source.contains("axiom_lut_apply")),
                "only the composite may carry the LUT"
            );
        });

        // Spliced once, and only into the arm that also splices AgX.
        assert_eq!(post.matches("crate::lut::GRADE_LUT_WGSL").count(), 1);
        let splice = post.index_of("crate::lut::GRADE_LUT_WGSL");
        let agx = post.index_of("crate::agx::AGX_WGSL");
        assert!(agx < splice, "the LUT rides in the AgX arm's concat");

        // Called once, in the HDR entry point, and NOT in the LDR one.
        assert_eq!(post.matches("axiom_lut_apply(").count(), 1);
        let hdr = post.index_of("fn fs_composite_hdr");
        let call = post.index_of("axiom_lut_apply(srgb_encode(tone)");
        assert!(hdr < call, "the call is inside the HDR entry point");

        // ...and its result feeds the grade terms rather than the reverse.
        let graded = post.index_of("graded_display(display)");
        assert!(call < graded, "the LUT runs BEFORE the grade terms");
        // The LDR arm still calls the unchanged `graded`, so its bytes are the
        // bytes it always presented.
        assert!(post.contains("let out = graded(rolled);"));
    }

    /// `str::find`, as an index, with a message naming what was missing.
    trait IndexOf {
        fn index_of(&self, needle: &str) -> usize;
    }

    impl IndexOf for str {
        fn index_of(&self, needle: &str) -> usize {
            self.find(needle)
                .unwrap_or_else(|| panic!("post_chain.rs must contain {needle:?}"))
        }
    }
}
