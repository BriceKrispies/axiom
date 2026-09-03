//! **The cloud field** the sky's cloud layer is thresholded against, and the
//! authored value that shapes it.
//!
//! [`crate::FrameSky`] owns the *layer*: how much cloud there is, how large it
//! reads, how it is lit and how it composites over the gradient. This module
//! owns the *medium* — the procedural field itself, the parameters that shape
//! it, and the arithmetic a backend mirrors. The split is the same one
//! [`crate::FrameSky::with_clouds`] and [`FrameCloudDetail`] make at the
//! authoring surface: weather on one side, the substance of the cloud on the
//! other.
//!
//! The field is a sum of separable sinusoids on rotated lattices, renormalised
//! by the amplitudes that contributed — the structure of the reference deck's
//! own `skFbm2` (`apps/shmup/src/sky/noise.js:55-64`), including its
//! `s / max(n, 1e-4)` normaliser, on a basis with no hash and therefore no
//! texture, no integer hashing and no `fract` precision cliff. That is what
//! keeps the whole thing portable to WGSL unchanged.
//!
//! Every constant here is either taken from that reference with a line
//! citation, or labelled **INVENTED** at its definition with the reason it had
//! to be picked. There is no third category.

use axiom_kernel::Ratio;

/// **How the cloud field is shaped** — the medium, as distinct from the weather.
///
/// [`crate::FrameSky::with_clouds`] authors how *much* cloud there is and how *large* it
/// reads. This authors what the field those two numbers are applied to actually
/// **is**. Five numbers, and each one names a specific way an analytic cloud
/// field misreads as something other than cloud when it is left where it was:
///
/// | knob | the defect it answers |
/// |---|---|
/// | [`Self::with_octaves`] | a four-octave field has no texture *inside* a puff, so the silhouette carries all of the detail and the layer reads as cut paper |
/// | [`Self::with_warp`] | an unwarped sum of sinusoids is a **lattice** — every lobe the same size on the same pitch, which is exactly the "discrete blobs" tell |
/// | [`Self::with_softness`] | the width of the window between clear and opaque: the biggest single lever on how hard a cloud edge is |
/// | [`Self::with_opacity`] | the ceiling on how much of the sky a covered pixel takes: the biggest single lever on the layer's contrast against the gradient |
/// | [`Self::with_filtering`] | fades an octave out once its features are finer than the ray's own footprint on the deck — the only thing that makes a high octave count safe in a per-pixel analytic field |
///
/// Every field defaults to the value the sky evaluated with before it was
/// authorable, so [`Self::plain`] is the identity: a sky that never mentions
/// detail is the sky it always was.
///
/// Authored values are returned as authored; [`crate::FrameSky::radiance`] and a
/// backend's mirror each clamp them into the band they can evaluate, the same
/// posture [`crate::FrameSky::cloud_coverage`] and [`crate::FrameSky::haze_height`] take.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameCloudDetail {
    octaves: u32,
    warp: f32,
    softness: f32,
    opacity: f32,
    filtering: f32,
}

impl FrameCloudDetail {
    /// The plain field: four octaves, no domain warp, a `0.22`-wide coverage
    /// window, full opacity, and no footprint filtering.
    ///
    /// These are not round numbers picked to look like defaults — they are the
    /// exact values the cloud layer evaluated with before any of them crossed
    /// the boundary, which is what lets detail be *added to the definition* of
    /// the sky rather than forked around it.
    pub const fn plain() -> Self {
        FrameCloudDetail {
            octaves: DEFAULT_CLOUD_OCTAVES,
            warp: 0.0,
            softness: DEFAULT_CLOUD_SOFTNESS,
            opacity: 1.0,
            filtering: 0.0,
        }
    }

    /// How many octaves of the field are summed, clamped to
    /// `1..=`[`FrameCloudDetail::octave_limit`] when evaluated.
    ///
    /// Each octave is a separable sinusoid on its own rotated lattice at its own
    /// non-integer frequency, and the summed field is renormalised by the
    /// amplitudes that actually contributed — so raising the count adds fine
    /// structure without moving the field's range, and the coverage threshold
    /// keeps meaning what it meant.
    pub const fn with_octaves(mut self, octaves: u32) -> Self {
        self.octaves = octaves;
        self
    }

    /// Displace the field's own sample point by a low-frequency pair of its own
    /// octaves, in cloud-plane units, before the shape octaves are summed.
    ///
    /// This is the difference between a cloud *field* and a cloud *pattern*. A
    /// straight sum of sinusoids has one lobe size on one pitch; warping the
    /// domain stretches some lobes and pinches others, which is what makes the
    /// layer read as weather rather than as a print. `0` is no warp; values
    /// around half a lobe are the useful range, and it is clamped at
    /// [`FrameCloudDetail::warp_limit`] so a runaway authored number cannot fold
    /// the field back through itself.
    pub const fn with_warp(mut self, warp: Ratio) -> Self {
        self.warp = warp.get();
        self
    }

    /// How much field value separates a clear pixel from a fully opaque one.
    ///
    /// The coverage threshold and this window are laid out together so both ends
    /// stay exact whatever it is set to (see [`crate::FrameSky::with_clouds`]). Narrow
    /// gives a distinct puff with a paper edge; wide gives a wispy limb that
    /// dissolves into the sky. Clamped to
    /// [`FrameCloudDetail::softness_limits`] when evaluated — a zero-width
    /// window would divide by zero, and one wider than the field's whole range
    /// could never reach opaque.
    ///
    /// # What a reference deck puts here, and the one thing it does that this
    /// cannot
    ///
    /// The reference cumulus erodes with
    /// `smoothstep(1 - cov, 1 - cov * 0.34 + 0.05, n)`
    /// (`apps/shmup/src/sky/clouds.js:163`), so its window runs from `1 - cov`
    /// to `1 - 0.34 * cov + 0.05` and is therefore
    ///
    /// ```text
    /// width = 0.66 * cov + 0.05
    /// ```
    ///
    /// **wide, where `cov` is the *effective* coverage** — the authored coverage
    /// already scaled by the weather-scale macro field,
    /// `cov = authored * (0.34 + 1.30 * macro)` (`clouds.js:154`), whose macro
    /// term averages `0.5` and so averages `0.99 * authored`.
    ///
    /// Two consequences an app authoring against that reference must carry, and
    /// which the seam cannot carry for it:
    ///
    /// * **The window width is coverage-dependent there and constant here.** An
    ///   app matching a reference deck evaluates `0.66 * cov + 0.05` at its own
    ///   weather and authors the result. It cannot author the *dependence*.
    /// * **The threshold's placement differs, and by a knowable factor.** The
    ///   reference's window *starts* at `1 - cov`; this one starts at
    ///   `1 - coverage * (1 + softness)` (see [`crate::FrameSky::cloud_density`]). To
    ///   put the two thresholds in the same place an app authors
    ///   `coverage = cov / (1 + softness)`, not `cov`. Authoring the
    ///   reference's number directly puts this field's threshold
    ///   `(1 + softness)` times too low, which is a *lot* more sky covered than
    ///   the reference covers — the single largest source of a cloud layer that
    ///   measures too loud.
    pub const fn with_softness(mut self, softness: Ratio) -> Self {
        self.softness = softness.get();
        self
    }

    /// The most of the sky a fully-covered pixel takes, `0` (the layer is
    /// invisible) to `1` (it replaces what is behind it).
    ///
    /// A cumulus deck seen from below is not opaque, and a layer that reaches
    /// full density is not just wrong about that — it is the whole of the
    /// layer's contrast against the gradient, and therefore the whole of its
    /// spectral energy. Lowering this scales every structural scale of the cloud
    /// down together, which is what makes it the right knob for "the clouds are
    /// too loud" and the wrong one for "the clouds are the wrong shape".
    ///
    /// # What this is *not*, and what a reference deck does instead
    ///
    /// This is a **linear ceiling** on a density that is already a smoothstep of
    /// the field. A reference cumulus deck converts density to alpha through an
    /// optical depth instead —
    /// `a = clamp(1 - exp(-thick * 3.4), 0, 1) * fade` with
    /// `thick = d * density * mix(1.0, 1.7, graze)`
    /// (`apps/shmup/src/sky/clouds.js:293-295`), the `graze` term lengthening
    /// the path through the slab for a flat ray and `density` being an authored
    /// multiplier well above one (`1.9` in that source's own weather).
    ///
    /// **That exponential remap is not expressible here and is not approximated
    /// here.** It is a different curve (saturating, not linear), it is
    /// view-angle dependent through `graze`, and faking it with a ceiling would
    /// put the error somewhere a later measurement would misattribute. What a
    /// ceiling *does* reproduce is the part of it that dominates the measured
    /// spectrum — the layer's overall contrast against the sky behind it — and
    /// an app matching a reference should author the alpha that remap produces
    /// at its own weather, not the `density` that feeds it.
    pub const fn with_opacity(mut self, opacity: Ratio) -> Self {
        self.opacity = opacity.get();
        self
    }

    /// How aggressively an octave is faded out once its features are finer than
    /// the view ray's own footprint on the cloud deck, `0` (never) to `1` (at the
    /// sampling limit).
    ///
    /// The cloud plane is sampled where the ray meets it, so the map from screen
    /// space to the deck has a derivative that grows without bound as the ray
    /// flattens toward the horizon. Nothing sampled once per pixel survives
    /// that: a fine octave down there is not detail, it is noise at whatever
    /// frequency the pixel grid happens to beat against, which is why an
    /// unfiltered field's finest band *rises* instead of falling away. Fading
    /// each octave against the footprint — reach, plus the foreshortening term
    /// that runs away with it — is the same answer a real deck's distance haze
    /// gives, and it is what makes a six-octave field legal per-pixel at all.
    pub const fn with_filtering(mut self, filtering: Ratio) -> Self {
        self.filtering = filtering.get();
        self
    }

    /// The authored octave count. See [`Self::with_octaves`].
    pub const fn octaves(&self) -> u32 {
        self.octaves
    }

    /// The authored domain-warp amount. See [`Self::with_warp`].
    pub const fn warp(&self) -> Ratio {
        Ratio::finite_or_zero(self.warp)
    }

    /// The authored coverage-window width. See [`Self::with_softness`].
    pub const fn softness(&self) -> Ratio {
        Ratio::finite_or_zero(self.softness)
    }

    /// The authored density ceiling. See [`Self::with_opacity`].
    pub const fn opacity(&self) -> Ratio {
        Ratio::finite_or_zero(self.opacity)
    }

    /// The authored footprint-filter strength. See [`Self::with_filtering`].
    pub const fn filtering(&self) -> Ratio {
        Ratio::finite_or_zero(self.filtering)
    }

    /// The most octaves the field carries — the length of its octave table.
    ///
    /// Published rather than left implicit because an app authoring a count has
    /// no other way to know where the ceiling is, and a silently clamped count
    /// is a sky that quietly disagrees with what the app asked for.
    pub const fn octave_limit() -> u32 {
        CLOUD_OCTAVE_LIMIT
    }

    /// The largest domain warp evaluated, in cloud-plane units.
    pub const fn warp_limit() -> Ratio {
        Ratio::finite_or_zero(CLOUD_WARP_LIMIT)
    }

    /// The band the coverage window's width is evaluated in, `(min, max)`.
    pub const fn softness_limits() -> (Ratio, Ratio) {
        (
            Ratio::finite_or_zero(CLOUD_SOFTNESS_MIN),
            Ratio::finite_or_zero(CLOUD_SOFTNESS_MAX),
        )
    }

    /// The coverage window's width as evaluated — clamped into
    /// [`Self::softness_limits`] so the window is never degenerate.
    ///
    /// A method rather than an inline clamp because the threshold and the ramp
    /// divisor must be the *same* width or the layer's two exact ends stop being
    /// exact.
    pub(crate) fn edge_width(&self) -> f32 {
        self.softness.clamp(CLOUD_SOFTNESS_MIN, CLOUD_SOFTNESS_MAX)
    }
}

/// The coverage window's default width — how much field value separates a clear
/// pixel from a fully opaque one when no [`FrameCloudDetail`] is authored. Wide
/// enough that a cumulus has a soft limb rather than a paper edge; narrow enough
/// that it still reads as a distinct puff rather than a smear.
const DEFAULT_CLOUD_SOFTNESS: f32 = 0.22;

/// The band the coverage window is evaluated in.
///
/// Both ends are excluded because both are degenerate: at `0` the ramp divides
/// by zero, and past `1` the window is wider than the field's entire range, so
/// no coverage could ever reach opaque. Clamping rather than rejecting keeps
/// this branch-free and keeps a mis-authored sky a *usable* sky — the same
/// posture the haze height and the body's radius take.
///
/// **INVENTED**, and labelled as such: these are guard rails, not values. No
/// reference sky names them, because no reference sky has to survive an
/// arbitrary authored number. `0.02` is simply small enough that the window is
/// still a window, and `1.0` is the field's own range, past which no coverage
/// could reach opaque. Neither is reachable by any authored sky that means
/// anything.
const CLOUD_SOFTNESS_MIN: f32 = 0.02;
const CLOUD_SOFTNESS_MAX: f32 = 1.0;

/// The octave count an unqualified cloud layer sums — the four the field had
/// before the count was authorable.
const DEFAULT_CLOUD_OCTAVES: u32 = 4;

/// The most octaves the field carries.
///
/// Not a budget: `6` is the reference cumulus deck's own high-quality octave
/// count — `apps/shmup/src/sky/clouds.js:202`, `int octD = quality > 0 ? 6 : 3`,
/// fed to `skCumulusDensity`'s `skFbm2(…, oct)` at `clouds.js:160`. A deck that
/// wanted seven would be evidence the reference wanted seven.
///
/// Must equal [`CLOUD_OCTAVES`]'s length;
/// `the_octave_table_and_its_published_limits_agree` pins that.
const CLOUD_OCTAVE_LIMIT: u32 = 6;

/// The largest domain warp evaluated, in cloud-plane units.
///
/// **INVENTED**, and labelled as such — but sized against a number that is not,
/// and the conversion is worth writing out because getting it wrong silently
/// clips the one value an app matching the reference would want to author.
///
/// The reference (`clouds.js:159-160`) computes
/// `n = skFbm2(p * 1.25 + w * 1.6, oct)` with `w = skVal2(p * 0.42) - 0.5`, so
/// `w` is `±0.5` and the displacement is `w * 1.6 = ±0.8`. That `±0.8` is
/// already **in the shape field's own coordinate** — the `1.25` is the sampling
/// scale of the term it is added to, not a further multiplier on it — and
/// `skVal2` is a unit lattice, so the displacement is `±0.8` *noise cells*.
///
/// This field's feature is not a lattice cell but a sinusoid lobe: the base
/// octave is `sin(p.x) * sin(p.y)`, whose sign cell is `PI` wide. And the warp
/// enters as `p + (octave - 0.5) * warp`, so an authored `warp` displaces by
/// `±warp / 2`. Matching `±0.8` lobes therefore needs
///
/// ```text
/// warp / 2 = 0.8 * PI   =>   warp = 1.6 * PI = 5.0265
/// ```
///
/// so the limit has to sit above `5.03`, not below it. `2 * PI` — two full
/// lobes — clears it with headroom while still bounding a runaway number that
/// would stop stretching the field and start folding it repeatedly through
/// itself.
const CLOUD_WARP_LIMIT: f32 = std::f32::consts::TAU;

/// The domain warp's own frequency, relative to the base shape octave.
///
/// Not chosen: the reference warps a shape field sampled at `p * 1.25` with a
/// displacement field sampled at `p * 0.42` (`clouds.js:159-160`), so the warp
/// runs at `0.42 / 1.25 = 0.336` of the shape's frequency. That ratio is the
/// whole point of a warp — it moves *whole lobes* about rather than roughening
/// their edges — so it is the number that transfers between bases, and the two
/// absolute frequencies are the numbers that do not.
const CLOUD_WARP_FREQUENCY: f32 = 0.336;

/// The two rotations the warp's two components are sampled at.
///
/// **INVENTED**, and labelled as such. The reference decorrelates its two
/// displacement components with a hash offset (`clouds.js:159`,
/// `skVal2(p * 0.42)` against `skVal2(p * 0.42 + 19.7)`), and this field has no
/// hash to offset — rotation is its only decorrelation mechanism, so the two
/// values had to be picked rather than ported. What was required of them, and
/// is all that was: far apart from each other, and not equal to any rotation in
/// [`CLOUD_OCTAVES`], so the displacement is not a scaled copy of the field it
/// displaces.
const CLOUD_WARP_ROTATIONS: [f32; 2] = [0.83, 2.91];

/// The deck footprint, in cloud-plane units per radian of view angle, at which a
/// fully authored [`FrameCloudDetail::with_filtering`] has removed an octave
/// entirely.
///
/// **INVENTED**, and labelled as such: the reference solves this problem but
/// does not solve it with a number of this shape. It ends each deck at a
/// *distance* instead — cirrus faded out between 22 km and 90 km
/// (`clouds.js:230`) explicitly because "the derivative there is over 400 m per
/// screen pixel … nothing sampled per-pixel can survive that", and cirrus held
/// to two octaves (`clouds.js:204-208`) because "an octave finer than that is
/// pure aliasing". This field has no deck distance to fade against — it is
/// sampled on a unit plane — so the same judgement had to be expressed against
/// the footprint instead, and the crossover had to be picked.
///
/// It is picked at the sampling limit rather than at taste: `512` units of phase
/// per radian is about `81` cycles across a radian of view, which for a frame a
/// thousand pixels wide is one cycle every twelve pixels — where a per-pixel
/// analytic field stops resolving and starts aliasing. So `filtering == 1` means
/// "cut each octave where it would alias" and anything below cuts earlier and
/// more softly. A frame with a very different pixels-per-radian would want a
/// different number, and that is the honest weakness of expressing it as a
/// constant rather than as a screen derivative the sky pass does not have.
const CLOUD_DETAIL_LIMIT: f32 = 512.0;

/// The cloud field's octaves as `[rotation, frequency, amplitude]`.
///
/// A sum of separable sinusoids, not a hashed lattice noise: it is the same few
/// lines of arithmetic in Rust and in WGSL with no texture, no integer hashing and
/// no `fract` precision cliff, which is what keeps this function portable to the
/// shader unchanged the way the rest of [`crate::FrameSky`] is.
///
/// Each octave is rotated by its own odd angle and scaled by a non-integer
/// frequency ratio. Both matter: axis-aligned harmonics of a common frequency
/// re-align into a visible grid, and a grid is the one thing a sky may not look
/// like.
///
/// **The first four rows are the field as it was before the octave count was
/// authorable**, at the amplitudes and frequencies it had, and they are what
/// [`FrameCloudDetail::plain`] sums — so an existing sky keeps the field it was
/// authored against.
///
/// **Rows five and six are the continuation, and their numbers are not chosen —
/// they are taken from the fbm this field stands in for.** The reference cumulus
/// deck's shape noise is `skFbm2` (`apps/shmup/src/sky/noise.js:55-64`), whose
/// per-octave ratios are a lacunarity of `2.04` (`noise.js:60`,
/// `p = SK_ROT * p * 2.04 + 7.13`), an amplitude gain of `0.5` (`noise.js:61`,
/// `a *= 0.5`) and a cumulative rotation of `atan2(0.6, 0.8) = 0.6435011` rad
/// per octave (`noise.js:53`, `SK_ROT = mat2(0.8, 0.6, -0.6, 0.8)`). Rows five
/// and six apply exactly those three ratios to row four:
///
/// ```text
/// frequency  9.17 * 2.04 = 18.7068,  18.7068 * 2.04 = 38.161872
/// amplitude  0.100 * 0.5 = 0.050,    0.050 * 0.5    = 0.025
/// rotation   3.71 + 0.6435011 = 4.3535011,  + 0.6435011 = 4.9970022
/// ```
///
/// The four legacy rows are themselves within about 3% of those ratios — their
/// frequency steps are `2.31`, `2.048`, `1.939` against `skFbm2`'s `2.04`, and
/// their amplitude steps `0.5`, `0.6`, `0.667` against its `0.5` — which is why
/// continuing them with the reference's exact numbers is a continuation and not
/// a seam. Neither the lacunarity nor the gain is authorable, deliberately: the
/// measured discrepancy this table answers is one of *amplitude and edge*, not
/// of spectral slope (the reference and this field already agree on the
/// band-to-band ratio to within 2%), and a knob that cannot be aimed at a
/// measured defect is a knob that will be aimed at a guess.
///
/// The amplitudes no longer have to sum to `1.0`, because [`cloud_field`]
/// divides by the amplitudes that actually contributed. That normalisation is
/// not an invention either — it is `skFbm2`'s own `s / max(n, 1e-4)`
/// (`noise.js:63`), and it is what makes the field's range `0.0..=1.0` for
/// *every* octave count and every filter strength, which is the property the
/// coverage threshold's two exact ends rest on.
const CLOUD_OCTAVES: [[f32; 3]; 6] = [
    [0.0000000, 1.000000, 0.500],
    [1.1300000, 2.310000, 0.250],
    [2.4700000, 4.730000, 0.150],
    [3.7100000, 9.170000, 0.100],
    [4.3535011, 18.706800, 0.050],
    [4.9970022, 38.161872, 0.025],
];

/// Each octave's index, as a table rather than a cast.
///
/// The octave count is applied as an arithmetic mask (`amplitude * 0/1`) rather
/// than by truncating the iteration, which is what keeps the fold branch-free.
/// That mask needs each row's ordinal, and reading it from a table beside the
/// octaves keeps the whole fold free of integer/float conversion.
const CLOUD_OCTAVE_ORDINAL: [u32; 6] = [0, 1, 2, 3, 4, 5];

/// One octave of the cloud field: a separable sinusoid on a rotated lattice,
/// remapped to `0.0..=1.0`.
pub(crate) fn cloud_octave(p: [f32; 2], rotation: f32, frequency: f32) -> f32 {
    let (sin_r, cos_r) = (rotation.sin(), rotation.cos());
    let x = (p[0] * cos_r + p[1] * sin_r) * frequency;
    let y = (p[1] * cos_r - p[0] * sin_r) * frequency;
    x.sin() * y.sin() * 0.5 + 0.5
}

/// The cloud field at a point on the cloud plane, in `0.0..=1.0`.
///
/// `footprint` is how much of the deck one radian of view angle sweeps at this
/// point — see [`crate::FrameSky::cloud_density`] — and it is what each octave's
/// visibility is measured against when a filter is authored.
///
/// The whole thing is one fold, which is what the octave count being *data*
/// requires: the count cannot select how many times a loop runs without a
/// branch, so instead every row is always visited and its amplitude is
/// multiplied by a `0`/`1` mask. The fold carries the numerator and the live
/// amplitude together so the result can be renormalised by exactly what
/// contributed, which is what keeps the range — and therefore the coverage
/// threshold's exact ends — independent of the count and the filter.
pub(crate) fn cloud_field(p: [f32; 2], detail: FrameCloudDetail, footprint: f32) -> f32 {
    let count = detail.octaves.clamp(1, CLOUD_OCTAVE_LIMIT);
    let filtering = detail.filtering.clamp(0.0, 1.0);

    // Domain warp: displace the sample point by a low-frequency pair of the
    // field's own octaves, centred on zero so the warp moves the field about
    // rather than translating it. At the default amount of `0` the displacement
    // is exactly zero and `q` is `p`, so the plain field is untouched.
    let warp = detail.warp.clamp(0.0, CLOUD_WARP_LIMIT);
    let offset = CLOUD_WARP_ROTATIONS.map(|rotation| {
        (cloud_octave(p, rotation, CLOUD_WARP_FREQUENCY) - 0.5) * warp
    });
    let q = [p[0] + offset[0], p[1] + offset[1]];

    let (weighted, live) = CLOUD_OCTAVES.iter().zip(CLOUD_OCTAVE_ORDINAL).fold(
        (0.0_f32, 0.0_f32),
        |(weighted, live), (octave, ordinal)| {
            // The count, as arithmetic: rows past it contribute a zero amplitude
            // to both the numerator and the normaliser, so they are absent
            // rather than merely small.
            let counted = f32::from(ordinal < count);
            // The footprint filter. At `filtering == 0` this is
            // `smoothstep(1.0)`, which is exactly `1.0` — the identity, so an
            // unfiltered field is the field unchanged to the bit.
            let visible = smoothstep(1.0 - filtering * octave[1] * footprint / CLOUD_DETAIL_LIMIT);
            let amplitude = octave[2] * counted * visible;
            (
                weighted + amplitude * cloud_octave(q, octave[0], octave[1]),
                live + amplitude,
            )
        },
    );
    // Every amplitude is non-negative and every octave is in `0..=1`, so the
    // numerator never exceeds the normaliser and the quotient is in `0..=1` by
    // construction. The floor covers the one degenerate case — a filter strong
    // enough to remove every octave — where the numerator is already zero, so
    // the field is a clear sky rather than a NaN.
    weighted / live.max(f32::MIN_POSITIVE)
}

/// Hermite smoothstep on an already-`0..1` value.
pub(crate) fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_sky::{normalize_or, FrameSky};
    use axiom_kernel::Radians;

    /// Tests author ratios and angles as plain scalars.
    fn q(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }

    fn rad(v: f32) -> Radians {
        Radians::finite_or_zero(v)
    }

    /// A blue day sky with broad cumulus and a sun — the shape an outdoor frame
    /// authors, and the same fixture `frame_sky`'s own tests use.
    fn daylit() -> FrameSky {
        FrameSky::gradient([0.10, 0.28, 0.75], [0.55, 0.72, 0.95])
            .with_body([0.45, 0.30, 1.0], rad(0.03), [3.0, 2.8, 2.4], q(600.0), q(0.6))
            .with_clouds(q(0.55), q(0.5))
    }

    /// The field the whole layer is thresholded against must actually span its
    /// stated range, or neither end of the coverage window is exact.
    #[test]
    fn the_cloud_field_stays_inside_zero_to_one_and_is_not_a_flat_sheet() {
        let plain = FrameCloudDetail::plain();
        let samples: Vec<f32> = (0..64)
            .map(|i| {
                let t = i as f32 * 0.37;
                cloud_field([t.cos() * 9.0 + t, t.sin() * 7.0 - t * 0.5], plain, 1.0)
            })
            .collect();
        assert!(samples.iter().all(|v| (0.0..=1.0).contains(v)), "{samples:?}");
        let lo = samples.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(hi - lo > 0.3, "the field varies rather than sitting flat: {lo}..{hi}");
        // The octaves are genuinely rotated against each other: a single unrotated
        // separable sinusoid is symmetric under swapping x and z, and the field
        // must not be.
        assert!(
            (cloud_field([1.7, 0.4], plain, 1.0) - cloud_field([0.4, 1.7], plain, 1.0)).abs()
                > 1.0e-3
        );
        // The range holds for every octave count and every filter strength, not
        // only for the default — which is the property the renormalising fold
        // exists to give, and the one the coverage threshold's exact ends need.
        (1..=FrameCloudDetail::octave_limit()).for_each(|n| {
            let detail = plain.with_octaves(n).with_filtering(q(0.7)).with_warp(q(0.8));
            (0..48).for_each(|i| {
                let t = i as f32 * 0.41;
                let v = cloud_field([t.sin() * 6.0, t.cos() * 5.0 + t], detail, 30.0);
                assert!((0.0..=1.0).contains(&v), "octaves {n}: {v}");
            });
        });
    }

    /// The four legacy octaves are the numerator of the default field exactly, so
    /// the only thing renormalisation changes is a division by their own sum —
    /// which is `1.0` to within a few parts in a hundred million. Below any
    /// threshold the layer can express, and stated rather than assumed.
    #[test]
    fn the_plain_detail_is_the_field_the_sky_had_before_detail_was_authorable() {
        let plain = FrameCloudDetail::plain();
        assert_eq!(plain.octaves(), 4);
        assert_eq!(plain.warp().get(), 0.0);
        assert_eq!(plain.softness().get(), 0.22);
        assert_eq!(plain.opacity().get(), 1.0);
        assert_eq!(plain.filtering().get(), 0.0);
        // The legacy sum, spelled out: the four fixed weights against the four
        // fixed octaves, with no normaliser and no mask.
        [[0.0, 0.0], [1.7, 0.4], [-3.2, 5.9], [11.0, -0.75]]
            .into_iter()
            .for_each(|p| {
                let legacy: f32 = CLOUD_OCTAVES
                    .iter()
                    .take(4)
                    .map(|o| o[2] * cloud_octave(p, o[0], o[1]))
                    .sum();
                let now = cloud_field(p, plain, 1.0);
                assert!(
                    (now - legacy).abs() < 1.0e-6,
                    "at {p:?}: {now} vs the legacy {legacy}"
                );
            });
        // And a sky that never mentions detail carries the plain one.
        assert_eq!(daylit().cloud_detail(), plain);
        assert_eq!(daylit().with_cloud_detail(plain), daylit());
    }

    /// The octave table and the two limits published beside it are one fact, and a
    /// table that outgrew its limit would silently drop its last rows.
    #[test]
    fn the_octave_table_and_its_published_limits_agree() {
        assert_eq!(CLOUD_OCTAVES.len(), CLOUD_OCTAVE_ORDINAL.len());
        assert_eq!(CLOUD_OCTAVES.len(), FrameCloudDetail::octave_limit() as usize);
        assert_eq!(CLOUD_OCTAVE_ORDINAL, [0, 1, 2, 3, 4, 5]);
        // Each octave is finer and fainter than the one before it — the property
        // that makes the sum an fbm rather than an arbitrary pile of sinusoids.
        assert!(CLOUD_OCTAVES.windows(2).all(|w| w[1][1] > w[0][1] * 1.9));
        assert!(CLOUD_OCTAVES.windows(2).all(|w| w[1][2] < w[0][2]));
        // Rows five and six are the reference fbm's ratios applied to row four,
        // not numbers that looked right: `skFbm2`'s lacunarity (`noise.js:60`),
        // gain (`noise.js:61`) and cumulative `SK_ROT` rotation (`noise.js:53`).
        // Pinned mechanically so the citation cannot rot into prose.
        let (lacunarity, gain, rotation_step) = (2.04_f32, 0.5_f32, 0.6_f32.atan2(0.8));
        // Bound to locals rather than recomputed inside the failure messages: a
        // format argument is only evaluated when the assertion fails, so an
        // expression written inline leaves a region no passing test can reach.
        (4..6).for_each(|i| {
            let (previous, row) = (CLOUD_OCTAVES[i - 1], CLOUD_OCTAVES[i]);
            let expected = [
                previous[0] + rotation_step,
                previous[1] * lacunarity,
                previous[2] * gain,
            ];
            let tolerance = [1.0e-5, 1.0e-4, 1.0e-6];
            (0..3).for_each(|c| {
                assert!(
                    (row[c] - expected[c]).abs() < tolerance[c],
                    "row {i} column {c}: {row:?} against the reference ratios {expected:?}"
                );
            });
        });
        // The limit must sit above the reference-equivalent warp of 1.6 * PI, or
        // authoring the faithful value would be silently clipped.
        assert_eq!(FrameCloudDetail::warp_limit().get(), std::f32::consts::TAU);
        assert!(FrameCloudDetail::warp_limit().get() > 1.6 * std::f32::consts::PI);
        let (soft_min, soft_max) = FrameCloudDetail::softness_limits();
        assert_eq!((soft_min.get(), soft_max.get()), (0.02, 1.0));
    }

    /// Sweeping the sky and summing the frame-to-frame change is a crude spectrum:
    /// finer octaves put more of it at short range. That is the whole point of
    /// authoring a count.
    #[test]
    fn more_octaves_add_fine_structure_the_four_octave_field_does_not_have() {
        let roughness = |detail: FrameCloudDetail| {
            let sky = FrameSky::gradient([0.1; 3], [0.2; 3])
                .with_clouds(q(0.55), q(2.0))
                .with_cloud_detail(detail);
            let samples: Vec<f32> = (0..512)
                .map(|i| {
                    let a = i as f32 * 0.004;
                    sky.cloud_density(normalize_or([a.sin(), 0.6, a.cos()], [0.0, 1.0, 0.0])).get()
                })
                .collect();
            samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>()
        };
        let plain = FrameCloudDetail::plain();
        let (four, six) = (roughness(plain), roughness(plain.with_octaves(6)));
        assert!(six > four * 1.05, "six octaves is busier: {six} vs {four}");
        // A count outside the table is clamped rather than indexing past it or
        // collapsing the field.
        assert_eq!(roughness(plain.with_octaves(99)), roughness(plain.with_octaves(6)));
        assert_eq!(roughness(plain.with_octaves(0)), roughness(plain.with_octaves(1)));
    }

    /// Softness is the edge lever and opacity is the contrast lever, and they are
    /// genuinely different: one flattens the *slope* between clear and cloud, the
    /// other scales the whole layer down without touching where its edges are.
    #[test]
    fn a_softer_edge_flattens_the_limb_and_a_lower_opacity_scales_the_whole_layer() {
        let sweep = |detail: FrameCloudDetail| -> Vec<f32> {
            let sky = FrameSky::gradient([0.1; 3], [0.2; 3])
                .with_clouds(q(0.55), q(2.0))
                .with_cloud_detail(detail);
            (0..512)
                .map(|i| {
                    let a = i as f32 * 0.004;
                    sky.cloud_density(normalize_or([a.sin(), 0.6, a.cos()], [0.0, 1.0, 0.0])).get()
                })
                .collect()
        };
        let steepest = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        let peak = |s: &[f32]| s.iter().copied().fold(0.0, f32::max);
        let plain = FrameCloudDetail::plain();
        let (hard, soft) = (sweep(plain), sweep(plain.with_softness(q(0.6))));
        // Bound to locals, not recomputed inside the failure messages — see the
        // note in `the_octave_table_and_its_published_limits_agree`.
        let (hard_edge, soft_edge) = (steepest(&hard), steepest(&soft));
        assert!(
            soft_edge < hard_edge,
            "a wider window is a gentler edge: {soft_edge} vs {hard_edge}"
        );
        let thin = sweep(plain.with_opacity(q(0.4)));
        let (thin_peak, thin_edge) = (peak(&thin), steepest(&thin));
        assert!((thin_peak - 0.4).abs() < 1.0e-5, "capped at the ceiling: {thin_peak}");
        assert!(thin_edge < hard_edge, "and gentler for it: {thin_edge} vs {hard_edge}");
        // A zero-width window and a runaway one are both clamped into a usable
        // band rather than dividing by zero or making overcast unreachable.
        [0.0, -3.0, 9.0].into_iter().for_each(|s| {
            let d = sweep(plain.with_softness(q(s)));
            assert!(d.iter().all(|v| (0.0..=1.0).contains(v)), "softness {s}: {d:?}");
        });
        // Opacity outside its range is clamped too — a negative one cannot make a
        // hole in the sky, and a large one cannot exceed full replacement.
        assert_eq!(peak(&sweep(plain.with_opacity(q(-2.0)))), 0.0);
        assert!((peak(&sweep(plain.with_opacity(q(4.0)))) - peak(&hard)).abs() < 1.0e-6);
    }

    /// The warp is a coordinate change, so it moves the field about without
    /// leaving its range — which is what lets it break the sinusoid lattice
    /// without disturbing what coverage means.
    #[test]
    fn the_domain_warp_moves_the_field_without_leaving_its_range() {
        let plain = FrameCloudDetail::plain();
        let warped = plain.with_warp(q(1.2));
        let probes = [[0.0, 0.0], [1.7, 0.4], [-3.2, 5.9], [11.0, -0.75], [4.5, 4.5]];
        let moved = probes
            .into_iter()
            .filter(|p| (cloud_field(*p, warped, 1.0) - cloud_field(*p, plain, 1.0)).abs() > 1.0e-3)
            .count();
        assert!(moved >= 4, "the warp displaces the field: {moved} of 5 probes");
        // Still a field: in range everywhere, including past the clamp.
        [0.0, 1.2, 9.0, -1.0].into_iter().for_each(|w| {
            let detail = plain.with_warp(q(w));
            probes.into_iter().for_each(|p| {
                let v = cloud_field(p, detail, 1.0);
                assert!((0.0..=1.0).contains(&v), "warp {w} at {p:?}: {v}");
            });
        });
        // Beyond the limit the warp stops growing rather than folding further.
        assert_eq!(
            cloud_field([1.7, 0.4], plain.with_warp(q(9.0)), 1.0),
            cloud_field([1.7, 0.4], plain.with_warp(FrameCloudDetail::warp_limit()), 1.0)
        );
    }

    /// The filter is the reason a six-octave field is legal per-pixel: it removes
    /// each octave where the deck's own foreshortening has already outrun it.
    #[test]
    fn the_footprint_filter_removes_fine_octaves_where_the_deck_outruns_them() {
        let detail = FrameCloudDetail::plain().with_octaves(6);
        let filtered = detail.with_filtering(q(1.0));
        let roughness = |d: FrameCloudDetail, footprint: f32| {
            let samples: Vec<f32> = (0..256)
                .map(|i| {
                    let t = i as f32 * 0.03;
                    cloud_field([t, t * 0.7], d, footprint)
                })
                .collect();
            samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>()
        };
        // Overhead the footprint is small and the filter barely bites.
        let near = (roughness(detail, 2.0), roughness(filtered, 2.0));
        assert!(near.1 > near.0 * 0.8, "overhead is nearly untouched: {near:?}");
        // Out toward the horizon the same filter has taken the fine octaves out —
        // and, the property that actually matters, it bites *harder* there than
        // overhead. A filter that smoothed the whole sky evenly would be a
        // detail knob wearing an antialiasing label.
        let far = (roughness(detail, 400.0), roughness(filtered, 400.0));
        assert!(far.1 < far.0 * 0.7, "the far deck is smoothed: {far:?}");
        let (far_ratio, near_ratio) = (far.1 / far.0, near.1 / near.0);
        assert!(
            far_ratio < near_ratio * 0.8,
            "and more so than overhead: {far_ratio} vs {near_ratio}"
        );
        // A filter strong enough to take *every* octave leaves a clear sky rather
        // than a division by zero.
        let gone = cloud_field([1.7, 0.4], filtered, 1.0e9);
        assert_eq!(gone, 0.0, "no octave survives, and the field is finite: {gone}");
        // Filtering outside its range is clamped, not extrapolated.
        assert_eq!(
            cloud_field([1.7, 0.4], detail.with_filtering(q(-1.0)), 400.0),
            cloud_field([1.7, 0.4], detail, 400.0)
        );
        assert_eq!(
            cloud_field([1.7, 0.4], detail.with_filtering(q(6.0)), 400.0),
            cloud_field([1.7, 0.4], filtered, 400.0)
        );
    }

    #[test]
    fn cloud_detail_accessors_round_trip_and_the_sky_carries_them() {
        let detail = FrameCloudDetail::plain()
            .with_octaves(6)
            .with_warp(q(0.9))
            .with_softness(q(0.44))
            .with_opacity(q(0.7))
            .with_filtering(q(0.35));
        assert_eq!(detail.octaves(), 6);
        assert_eq!(detail.warp().get(), 0.9);
        assert_eq!(detail.softness().get(), 0.44);
        assert_eq!(detail.opacity().get(), 0.7);
        assert_eq!(detail.filtering().get(), 0.35);
        assert!(format!("{detail:?}").contains("FrameCloudDetail"));
        assert_eq!(detail, detail);
        assert_ne!(detail, FrameCloudDetail::plain());
        let sky = daylit().with_cloud_detail(detail);
        assert_eq!(sky.cloud_detail(), detail);
        assert_ne!(sky, daylit());
        // A detail that only reshapes the field cannot poison the frame.
        assert!(sky.radiance([0.3, 0.6, 0.7]).iter().all(|v| v.is_finite()));
    }
}
