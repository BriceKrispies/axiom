//! **The whole pyramid over a CPU image** — the semantic definition of the
//! chain, not merely of its arithmetic.
//!
//! [`filters`](crate::bloom_pyramid::filters) says what one tap set becomes.
//! This says what the *chain* is: which level reads which, in what order, at what
//! size, with which radius and blend weight, and what half-float storage does
//! between each pair. It is the transcription of `Bloom.render`:
//!
//! ```js
//! for (let i = 0; i < n; i++) {
//!   const src = i === 0 ? sourceTexture : this.mips[i - 1].rt.texture;
//!   du.uTexel.value.set(1 / sw, 1 / sh);                    // the SOURCE's texel
//!   du.uParams.value.set(i === 0 ? 1 : 0, threshold, knee, 0);
//!   this.down.render(renderer, this.mips[i].rt);
//! }
//! for (let i = n - 1; i > 0; i--) {
//!   uu.tSrc.value = this.mips[i].rt.texture;
//!   uu.uTexel.value.set(1 / this.mips[i].w, 1 / this.mips[i].h);
//!   this.up.render(renderer, this.mips[i - 1].rt);          // BLENDED, not replaced
//! }
//! this.texture = this.mips[0].rt.texture;
//! ```
//!
//! Three things a reader should not have to infer, and which no arithmetic test
//! would catch:
//!
//! - **The downsample's texel is the *source's*, not the destination's.** `1/sw`
//!   where `sw` is the previous level's width. Using the destination's would
//!   halve every offset and turn the 13-tap into a 5-tap-shaped blur.
//! - **The upsample walks down the index and blends into the larger level.** It
//!   does not build a separate accumulation buffer; each larger mip is
//!   overwritten in place by `lerp(itself, tent(smaller), weight)`. Running the
//!   loop upward instead would blur a level with a version of itself it had
//!   already contributed to.
//! - **`t = uTexel * uRadius`** — the reciprocal first, then the radius. Writing
//!   `radius / sw` is the same value in exact arithmetic and a different `f32`.
//!
//! # Sampling
//!
//! Bilinear with clamp-to-edge, matching a `FilterMode::Linear` /
//! `AddressMode::ClampToEdge` sampler: `p = uv·dim - 0.5`, two `mix`es across
//! `x` then one across `y`, each written as `a + (b-a)·t`. At the ±1 and ±2
//! offsets of a halving downsample every tap lands on a texel *corner*, so those
//! weights are exactly `0.5` and the filter is an exact 2x2 box average — which
//! is why the downsample's parity is tight and the tent upsample's (at radius
//! `0.62`) is not.

use crate::bloom_pyramid::filters::{
    blend, downsample_karis, downsample_plain, upsample_tent, DOWN_TAPS, UP_TAPS,
};
use crate::bloom_pyramid::half_storage::quantize;
use crate::bloom_pyramid::schedule::{mip_sizes, upsample_step};
use crate::bloom_pyramid::BloomTuning;

/// A linear-light RGB image, one level of the pyramid.
///
/// Every texel is a value an `Rgba16Float` attachment can hold exactly — the
/// constructors quantise — so reading one back is lossless and only the *writes*
/// round, exactly as on the GPU.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Image {
    width: u32,
    height: u32,
    texels: Vec<[f32; 3]>,
}

impl Image {
    /// An image of `width x height` linear-light texels, each produced from its
    /// `(x, y)` — row-major in **memory order**, which is the order a texture
    /// upload uses and the order row `0` (the one a UV near `v = 0` samples)
    /// comes first in.
    ///
    /// A generator rather than a buffer, so an image's length cannot disagree
    /// with its dimensions: there is no shape to validate and therefore no
    /// validation branch to write.
    ///
    /// Every texel is quantised on the way in, because that is what storing it
    /// into an `Rgba16Float` attachment did.
    pub(crate) fn from_fn(
        width: u32,
        height: u32,
        texel: impl Fn(u32, u32) -> [f32; 3],
    ) -> Image {
        Image {
            width,
            height,
            texels: (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| texel(x, y).map(quantize))
                .collect(),
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn texels(&self) -> &[[f32; 3]] {
        &self.texels
    }

    /// One texel, with the sampler's clamp-to-edge addressing.
    fn texel(&self, x: i32, y: i32) -> [f32; 3] {
        let cx = x.clamp(0, self.width as i32 - 1) as usize;
        let cy = y.clamp(0, self.height as i32 - 1) as usize;
        self.texels[cy * self.width as usize + cx]
    }

    /// A bilinear, clamp-to-edge sample at `uv`.
    fn sample(&self, uv: [f32; 2]) -> [f32; 3] {
        let px = uv[0] * self.width as f32 - 0.5;
        let py = uv[1] * self.height as f32 - 0.5;
        let fx = px - px.floor();
        let fy = py - py.floor();
        let ix = px.floor() as i32;
        let iy = py.floor() as i32;
        let lower = mix(self.texel(ix, iy), self.texel(ix + 1, iy), fx);
        let upper = mix(self.texel(ix, iy + 1), self.texel(ix + 1, iy + 1), fx);
        mix(lower, upper, fy)
    }
}

/// `a + (b - a)·t`, the form a `mix` builtin is *permitted* to use and the one
/// written out so both sides agree on which it is.
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|lane| a[lane] + (b[lane] - a[lane]) * t)
}

/// The UV of the centre of destination texel `(x, y)` in a `width x height`
/// target — what the rasteriser hands the fragment stage.
fn texel_centre(x: u32, y: u32, width: u32, height: u32) -> [f32; 2] {
    [
        (x as f32 + 0.5) / width as f32,
        (y as f32 + 0.5) / height as f32,
    ]
}

/// One downsample pass: `source` into a `width x height` level, on the karis arm
/// when `karis` (level 0) and the plain arm otherwise.
fn downsample(
    source: &Image,
    width: u32,
    height: u32,
    karis: bool,
    tuning: BloomTuning,
) -> Image {
    // `uTexel` is the SOURCE's texel size. See the module header.
    let texel = [1.0 / source.width as f32, 1.0 / source.height as f32];
    Image::from_fn(width, height, |x, y| {
        let uv = texel_centre(x, y, width, height);
        let taps =
            DOWN_TAPS.map(|o| source.sample([uv[0] + o[0] * texel[0], uv[1] + o[1] * texel[1]]));
        karis
            .then(|| downsample_karis(taps, tuning.exposure, tuning.threshold, tuning.knee))
            .unwrap_or_else(|| downsample_plain(taps))
    })
}

/// One upsample pass: the tent of `source` blended into `destination` at
/// `weight`, with the tent's reach scaled by `radius`.
fn upsample(source: &Image, destination: &Image, radius: f32, weight: f32) -> Image {
    // `t = uTexel * uRadius` — reciprocal first, then the radius.
    let texel = [
        (1.0 / source.width as f32) * radius,
        (1.0 / source.height as f32) * radius,
    ];
    let width = destination.width;
    let height = destination.height;
    Image::from_fn(width, height, |x, y| {
        let uv = texel_centre(x, y, width, height);
        let taps =
            UP_TAPS.map(|o| source.sample([uv[0] + o[0] * texel[0], uv[1] + o[1] * texel[1]]));
        blend(
            destination.texels[(y as usize) * (width as usize) + x as usize],
            upsample_tent(taps),
            weight,
        )
    })
}

/// **`Bloom.render`** — the finished pyramid's level 0, or `None` when the
/// pyramid has no levels at all (the source's `if (n === 0) return null`).
pub(crate) fn render(source: &Image, tuning: BloomTuning, levels: usize) -> Option<Image> {
    let sizes = mip_sizes(source.width, source.height, levels);
    let count = sizes.len();
    let descended = sizes
        .into_iter()
        .enumerate()
        .fold(Vec::<Image>::new(), |mut chain, (index, (width, height))| {
            let level = {
                let previous = chain.last().unwrap_or(source);
                downsample(previous, width, height, index == 0, tuning)
            };
            chain.push(level);
            chain
        });
    let ascended = (1..count).rev().fold(descended, |mut chain, index| {
        let (radius, weight) = upsample_step(index, count);
        chain[index - 1] = upsample(&chain[index], &chain[index - 1], radius, weight);
        chain
    });
    ascended.into_iter().next()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{downsample, mix, render, texel_centre, upsample, Image};
    use crate::bloom_pyramid::filters::FIREFLY_CLAMP;
    use crate::bloom_pyramid::schedule::{mip_sizes, LEVELS_HIGH};
    use crate::bloom_pyramid::{BloomTuning, SOURCE_SETTINGS};

    /// A deterministic HDR test image: a dim, hue-varied field with two hot
    /// specular events in it — one white, one saturated red. Both are needed:
    /// the white one proves the pyramid spreads energy, the red one proves the
    /// max-channel prefilter admits it.
    pub(crate) fn scene(width: u32, height: u32) -> Image {
        Image::from_fn(width, height, |x, y| {
            let u = x as f32 / width as f32;
            let v = y as f32 / height as f32;
            let base = [0.12 + u * 0.30, 0.18 + v * 0.22, 0.30 - u * 0.10];
            let white = f32::from(u8::from((x == width / 4) & (y == height / 2))) * 9.0;
            let red = f32::from(u8::from((x == (width * 3) / 4) & (y == height / 3)));
            [base[0] + white + red * 6.0, base[1] + white, base[2] + white]
        })
    }

    /// A flat field at `value`, the simplest thing whose behaviour through the
    /// whole chain is predictable by hand.
    pub(crate) fn flat(width: u32, height: u32, value: [f32; 3]) -> Image {
        Image::from_fn(width, height, |_, _| value)
    }

    /// `Image::from_fn` is a value type over a quantised buffer, and its
    /// accessors report what went in.
    #[test]
    fn an_image_is_its_dimensions_and_a_quantised_buffer() {
        let image = Image::from_fn(2, 1, |x, _| [1.0 + x as f32 * 3.0, 2.0, 3.0]);
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
        assert_eq!(image.texels(), &[[1.0, 2.0, 3.0], [4.0, 2.0, 3.0]]);
        // A value with more precision than a half can hold is stored rounded —
        // which is what the attachment does, so the reference must too.
        let precise = Image::from_fn(1, 1, |_, _| [1.0 + 1.0 / 4096.0, 0.0, 0.0]);
        assert_eq!(precise.texels()[0][0], 1.0);
        // The buffer's length is its area by construction, so there is no shape
        // to validate and no validation branch to write.
        assert_eq!(Image::from_fn(3, 5, |_, _| [0.0; 3]).texels().len(), 15);
    }

    /// `mix` is the two-endpoint lerp, exactly at both ends.
    #[test]
    fn mix_is_exact_at_its_endpoints() {
        let a = [1.0, 2.0, 3.0];
        let b = [5.0, 7.0, 9.0];
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), [3.0, 4.5, 6.0]);
    }

    /// A fragment's UV is its texel centre, so the first and last texel of a row
    /// sit half a texel inside the edges rather than on them.
    #[test]
    fn a_fragment_samples_at_its_texel_centre() {
        assert_eq!(texel_centre(0, 0, 4, 2), [0.125, 0.25]);
        assert_eq!(texel_centre(3, 1, 4, 2), [0.875, 0.75]);
    }

    /// Clamp-to-edge: a sample well outside the image repeats its border texel
    /// rather than wrapping a bright corner to the far side.
    #[test]
    fn sampling_clamps_to_the_edge_rather_than_wrapping() {
        let image = Image::from_fn(2, 1, |x, _| [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]][x as usize]);
        assert_eq!(image.sample([-5.0, 0.5]), [1.0, 0.0, 0.0]);
        assert_eq!(image.sample([5.0, 0.5]), [0.0, 0.0, 1.0]);
        // And the midpoint is the exact half-and-half the corner tap relies on.
        assert_eq!(image.sample([0.5, 0.5]), [0.5, 0.0, 0.5]);
    }

    /// A flat field survives a plain downsample, because the 13 weights sum to
    /// one and clamp-to-edge makes the border no different from the interior.
    #[test]
    fn a_flat_field_survives_a_plain_downsample() {
        let source = flat(16, 16, [0.5, 0.25, 2.0]);
        let out = downsample(&source, 8, 8, false, SOURCE_SETTINGS);
        assert_eq!(out.width(), 8);
        assert_eq!(out.height(), 8);
        out.texels().iter().enumerate().for_each(|(n, c)| {
            (0..3).for_each(|lane| {
                let got = c[lane];
                assert!(
                    (got - source.texels()[0][lane]).abs() <= 1e-3,
                    "texel {n} lane {lane} drifted: {got}"
                );
            });
        });
    }

    /// **The downsample reads the SOURCE's texel size.** Halving the source's
    /// dimensions while holding the destination's fixed must change the result —
    /// if the pass used the destination's texel instead, it would not.
    #[test]
    fn the_downsample_offsets_scale_with_the_source_not_the_destination() {
        let big = scene(32, 32);
        let small = scene(16, 16);
        let from_big = downsample(&big, 8, 8, false, SOURCE_SETTINGS);
        let from_small = downsample(&small, 8, 8, false, SOURCE_SETTINGS);
        assert_ne!(from_big, from_small);
        // And a source-sized texel reaches a genuinely different neighbourhood:
        // the 32-wide source's ±2 taps span a sixteenth of the image, the
        // 16-wide source's an eighth.
        assert_eq!(from_big.width(), from_small.width());
    }

    /// The upsample blends into its destination rather than replacing it: at
    /// weight zero the destination is untouched, at weight one it is the tent.
    #[test]
    fn the_upsample_blends_into_its_destination() {
        let small = flat(4, 4, [1.0, 1.0, 1.0]);
        let large = flat(8, 8, [0.25, 0.5, 0.75]);
        let untouched = upsample(&small, &large, 1.0, 0.0);
        assert_eq!(untouched, large);
        let replaced = upsample(&small, &large, 1.0, 1.0);
        replaced
            .texels()
            .iter()
            .for_each(|c| {
                let got = c[0];
                assert!((got - 1.0).abs() <= 1e-3, "got {got}");
            });
        // The source's own weight lands between the two.
        let half = upsample(&small, &large, 1.0, 0.5);
        assert!((half.texels()[0][0] - 0.625).abs() <= 1e-3);
    }

    /// **The radius scales the tent's reach.** A narrower radius keeps the
    /// pyramid's widest levels from becoming a thirty-pixel halo, which is the
    /// source's stated reason for `0.62`.
    #[test]
    fn a_narrower_radius_reaches_less_far() {
        let small = Image::from_fn(8, 8, |x, y| {
            [[0.0_f32; 3], [16.0, 16.0, 16.0]][usize::from((x == 4) & (y == 4))]
        });
        let large = flat(16, 16, [0.0, 0.0, 0.0]);
        let wide = upsample(&small, &large, 1.0, 1.0);
        let tight = upsample(&small, &large, 0.62, 1.0);
        let energy_far = |image: &Image| {
            image
                .texels()
                .iter()
                .enumerate()
                .filter(|(n, _)| {
                    let x = (n % 16) as i32;
                    let y = (n / 16) as i32;
                    (x - 8).abs() + (y - 8).abs() >= 4
                })
                .map(|(_, c)| c[0])
                .sum::<f32>()
        };
        let (near, far) = (energy_far(&tight), energy_far(&wide));
        assert!(
            near < far,
            "radius 0.62 must reach less far than radius 1.0: {near} vs {far}"
        );
    }

    /// **The whole chain.** A pyramid of the expected depth, at level 0's size,
    /// finite everywhere, and carrying the hot pixels' energy spread across it.
    #[test]
    fn the_pyramid_renders_at_level_zeros_size_and_spreads_the_highlights() {
        let source = scene(64, 64);
        let out = render(&source, SOURCE_SETTINGS, LEVELS_HIGH).expect("a 64x64 frame has levels");
        let sizes = mip_sizes(64, 64, LEVELS_HIGH);
        assert_eq!((out.width(), out.height()), sizes[0]);
        assert!(out.texels().iter().flatten().all(|v| v.is_finite()));
        // The two hot pixels' energy is now spread: far more than two texels of
        // the result are non-zero.
        let lit = out.texels().iter().filter(|c| c[0] > 1e-4).count();
        assert!(lit > 64, "the pyramid must spread the highlights, lit {lit}");
        // And nothing in the frame that was below the knee contributes: a scene
        // with the highlights removed blooms nowhere.
        let dim = flat(64, 64, [0.3, 0.3, 0.3]);
        let none = render(&dim, SOURCE_SETTINGS, LEVELS_HIGH).expect("levels");
        assert!(none.texels().iter().flatten().all(|v| *v == 0.0));
    }

    /// The pyramid is deterministic: the same input renders the same bits twice.
    #[test]
    fn the_pyramid_is_deterministic() {
        let source = scene(32, 32);
        let first = render(&source, SOURCE_SETTINGS, LEVELS_HIGH).expect("levels");
        let second = render(&source, SOURCE_SETTINGS, LEVELS_HIGH).expect("levels");
        assert_eq!(first, second);
    }

    /// Zero levels is `return null`.
    #[test]
    fn a_zero_level_pyramid_is_none() {
        assert!(render(&scene(64, 64), SOURCE_SETTINGS, 0).is_none());
    }

    /// A one-level pyramid runs the karis downsample and no upsample at all —
    /// the loop `for (i = n-1; i > 0; i--)` does not execute. It is still a
    /// bloom, just an unsoftened one.
    #[test]
    fn a_one_level_pyramid_skips_the_upsample_entirely() {
        let source = scene(32, 32);
        let out = render(&source, SOURCE_SETTINGS, 1).expect("one level");
        let expected = downsample(&source, 16, 16, true, SOURCE_SETTINGS);
        assert_eq!(out, expected);
    }

    /// The firefly clamp survives to the output: a scene of nothing but a
    /// blinding field cannot produce a level-0 texel above 24 before the
    /// upsample blends it, and the blend cannot lift it either.
    #[test]
    fn the_pyramid_output_respects_the_firefly_clamp() {
        let source = flat(32, 32, [1.0e4, 1.0e4, 1.0e4]);
        let out = render(&source, SOURCE_SETTINGS, LEVELS_HIGH).expect("levels");
        assert!(
            out.texels().iter().flatten().all(|v| *v <= FIREFLY_CLAMP + 1e-2),
            "a level-0 texel exceeded the clamp"
        );
    }

    /// Exposure moves the whole pyramid, and it moves it *through the threshold*
    /// rather than merely scaling the result — halving the exposure on a scene
    /// whose highlights sit just above the knee extinguishes the bloom entirely.
    #[test]
    fn metered_exposure_decides_whether_the_frame_blooms_at_all() {
        let source = flat(32, 32, [1.0, 1.0, 1.0]);
        let bright = BloomTuning { exposure: 4.0, ..SOURCE_SETTINGS };
        let dark = BloomTuning { exposure: 0.25, ..SOURCE_SETTINGS };
        let lit = render(&source, bright, LEVELS_HIGH).expect("levels");
        let unlit = render(&source, dark, LEVELS_HIGH).expect("levels");
        assert!(lit.texels().iter().any(|c| c[0] > 0.1));
        assert!(unlit.texels().iter().flatten().all(|v| *v == 0.0));
    }
}
