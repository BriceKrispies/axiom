//! **Step 1's arithmetic** — `_syncSun`, `_cullLights`, `_updateBounceFill` and
//! `_updateViewRig`, transcribed.
//!
//! Everything the scene walk computes before a single draw is issued. It is
//! pure arithmetic over scene state, so it belongs here rather than inside a
//! render pass, and it is where four of the frame's headline ratios are
//! actually set: key:fill, interior:exterior through a doorway,
//! warm-pool:cool-ambient after dark, and the viewmodel's brightness against
//! the street it is standing in.
//!
//! # Arithmetic width
//!
//! JavaScript numbers, so `f64` throughout, narrowed once where a value reaches
//! a uniform. Two of these chains are long enough for the width to be
//! observable: [`bounce_fill`] normalises a hue twice and pushes its chroma out
//! from a Rec.709 luminance in between, and [`view_rig`] takes a fractional
//! power of a ratio.
//!
//! # Groupings that are the specification
//!
//! - `hue.divideScalar(m)` is a **division** by the channel maximum, per
//!   channel, and not a multiply by a precomputed reciprocal. It appears three
//!   times in `_updateBounceFill` and is transcribed as division every time.
//! - `l + (hue.x - l) * k` is the source's chroma push, in that grouping.
//! - `u.owFillGain.value.set(1, s.bounceFill / Math.max(s.groundFill, 1e-6))`
//!   is likewise a division, with the denominator floored rather than the
//!   result guarded.
//! - `REF_DAYLIGHT * Math.pow(Math.min(ref / REF_DAYLIGHT, 1), gamma)`: the
//!   ratio is formed, clamped, raised, and only then scaled back up.
//!
//! # `MathUtils.smoothstep` takes its arguments the other way round
//!
//! three's is `smoothstep(x, min, max)`; GLSL's is `smoothstep(min, max, x)`.
//! [`smoothstep`] is three's, because that is what `_cullLights` calls, and the
//! test asserts the argument order explicitly so a later reader cannot
//! "correct" it into the GLSL convention.

/// Registration range at or below which a punctual light counts as a room or
/// street **practical** rather than as an effect flash.
///
/// The FX flash pool deliberately registers at 90 m so the distance fade never
/// bites it, and so that a muzzle flash is not dimmed by a room-lighting
/// control.
pub(crate) const PRACTICAL_RANGE: f64 = 30.0;

/// Full-daylight key intensity (`SUN_ILLUMINANCE_TOP` through a clear
/// atmosphere). **Only** used to normalise the viewmodel rig, never to light
/// anything.
pub(crate) const REF_DAYLIGHT: f64 = 4.6;

/// The sky's published ambient is 15% of the beam in daylight; this is that
/// constant, and it is the bridge between "how much ambient is there" and "how
/// bright is the scene" in both [`bounce_fill`] and [`view_rig`].
pub(crate) const SKY_AMBIENT_FRACTION: f64 = 0.15;

/// `THREE.MathUtils.smoothstep(x, min, max)`.
///
/// **Note the argument order** — the value first, then the edges, which is the
/// reverse of GLSL's. Transcribed with its two early-outs intact rather than
/// folded into a clamp, because `x <= min` and `x >= max` are exact tests in
/// the source and a clamp of `(x - min) / (max - min)` would divide by zero
/// when the two edges coincide.
pub(crate) fn smoothstep(x: f64, min: f64, max: f64) -> f64 {
    let t = (x - min) / (max - min);
    let shaped = t * t * (3.0 - 2.0 * t);
    // `x <= min → 0`, then `x >= max → 1`, else the cubic. The tests are
    // **ordered** in the source, so the index nests rather than sums: with
    // `min > max` (which nothing calls, but which the source still answers)
    // summing would return the cubic where the source returns 0. A table select
    // rather than two returns, and the cubic is safe to evaluate in every arm
    // except the degenerate `min == max`, where the first test fires and the
    // resulting NaN is discarded unread.
    let index = usize::from(x > min) * (1 + usize::from(x >= max));
    [0.0, shaped, 1.0][index]
}

/// One entry in `this.lights` — a punctual light registered through
/// `r.addLight(light, opts)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegisteredLight {
    /// `opts.range ?? light.distance ?? 25`.
    pub(crate) range: f64,
    /// `opts.priority ?? 1`. Recorded by `addLight` and read by nothing in the
    /// source — dead, and ported because dead computation in the source is
    /// still part of the source.
    pub(crate) priority: f64,
    /// `light.intensity` at registration, re-adopted whenever the owner
    /// animates it (see [`cull_light`]).
    pub(crate) base_intensity: f64,
    /// `e.applied` — what this culler last wrote. `None` is `undefined`, i.e.
    /// the light has never been culled.
    pub(crate) applied: Option<f64>,
}

/// `addLight`'s defaults: `range 25`, `priority 1`.
pub(crate) const DEFAULT_LIGHT_RANGE: f64 = 25.0;

/// What one frame of `_cullLights` does to one light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CulledLight {
    /// The intensity written back onto the light, and remembered as `applied`.
    pub(crate) intensity: f64,
    /// `light.visible = fade > 0.002`.
    pub(crate) visible: bool,
    /// The base intensity going into the next frame — re-adopted from the live
    /// light when the owner animated it.
    pub(crate) base_intensity: f64,
}

/// `_cullLights`, for one light.
///
/// Three things happen, in this order:
///
/// 1. **Adopt an animated base.** If the owner changed `light.intensity` since
///    this culler last wrote it, that new value becomes the base rather than
///    being fought over — which is what lets a flickering lamp flicker.
/// 2. **Distance fade.** `1 - smoothstep(d, range * 0.75, range * 1.15)`.
/// 3. **Practical trim.** A light registered at or below
///    [`PRACTICAL_RANGE`] takes `settings.practical_gain`; anything above it
///    (the FX flash pool at 90 m) takes unity.
///
/// The comparison against `e.applied` is `!==`, an exact float inequality, and
/// it is transcribed as one: an owner that wrote back the identical value has,
/// by definition, not animated anything.
pub(crate) fn cull_light(
    light: RegisteredLight,
    live_intensity: f64,
    distance: f64,
    practical_gain: f64,
) -> CulledLight {
    let animated = light
        .applied
        .is_some_and(|applied| live_intensity != applied);
    let base = [light.base_intensity, live_intensity][usize::from(animated)];
    let fade = 1.0 - smoothstep(distance, light.range * 0.75, light.range * 1.15);
    let gain = [1.0, practical_gain][usize::from(light.range <= PRACTICAL_RANGE)];
    CulledLight {
        intensity: base * fade * gain,
        visible: fade > 0.002,
        base_intensity: base,
    }
}

/// `_syncSun`'s choice: the index of the brightest directional light other than
/// the fallback, or `None` if the fallback should stay on.
///
/// Two details are load-bearing. The running best starts at `-1`, not at zero,
/// so a light with a *negative* intensity can still win the scan — and then
/// fails the `> 0.01` gate afterwards, which is a different thing from never
/// having been considered. And the comparison is strict `>`, so among equals
/// the **first** wins.
pub(crate) fn sync_sun(candidate_intensities: &[f64]) -> Option<usize> {
    candidate_intensities
        .iter()
        .enumerate()
        .fold(None::<(usize, f64)>, |best, (index, &intensity)| {
            let better = best.is_none_or(|(_, value)| intensity > value);
            [best, Some((index, intensity))][usize::from(better)]
        })
        .filter(|&(_, intensity)| intensity > 0.01)
        .map(|(index, _)| index)
}

/// A linear RGB triple, as the source's `THREE.Vector3`/`THREE.Color` holds one
/// — three JavaScript numbers, so `f64`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rgb {
    /// Red.
    pub(crate) r: f64,
    /// Green.
    pub(crate) g: f64,
    /// Blue.
    pub(crate) b: f64,
}

impl Rgb {
    /// A triple.
    pub(crate) const fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    /// `Math.max(x, y, z)`.
    pub(crate) fn max_channel(self) -> f64 {
        self.r.max(self.g).max(self.b)
    }

    /// `Vector3.divideScalar(s)` — a **division** of each channel, not a
    /// multiply by a reciprocal.
    pub(crate) fn divide_scalar(self, s: f64) -> Self {
        Self::new(self.r / s, self.g / s, self.b / s)
    }

    /// `Vector3.multiplyScalar(s)`.
    pub(crate) fn scale(self, s: f64) -> Self {
        Self::new(self.r * s, self.g * s, self.b * s)
    }
}

/// What `_updateBounceFill` writes into the material patch's uniforms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BounceFill {
    /// `owSkyFill` — the cool upper-hemisphere band.
    pub(crate) sky_fill: Rgb,
    /// `owGroundFill` — the warm band off the road.
    pub(crate) ground_fill: Rgb,
    /// `owFillGain` — `(1, bounceFill / max(groundFill, 1e-6))`.
    pub(crate) fill_gain: (f64, f64),
    /// `owIndirect.x` — `iblDiffuse * (sky.indirectScale ?? 1)`.
    pub(crate) ibl_diffuse: f64,
    /// `owIndirect.y` — the interior floor.
    pub(crate) interior_indirect: f64,
    /// `this._ambLevel`, the "how much light is in this scene at all"
    /// reference the viewmodel rig then reads.
    pub(crate) ambient_level: f64,
    /// `this._skyExposureBias` — `sky.exposureBias ?? 0`.
    pub(crate) sky_exposure_bias: f64,
}

/// What the sky subsystem publishes, when there is one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SkyPublication {
    /// `sky.ambientColor` — the sky's CPU stand-in for the whole-sky average.
    pub(crate) ambient_color: Rgb,
    /// `sky.indirectScale ?? 1` — an elevation-dependent indirect budget, so
    /// the key:fill ratio does not invert at golden hour.
    pub(crate) indirect_scale: f64,
    /// `sky.exposureBias ?? 0` — metering compensation for the current sun
    /// elevation.
    pub(crate) exposure_bias: f64,
}

/// The fallback hue when there is no sky subsystem, or its ambient is black.
const FALLBACK_SKY_HUE: Rgb = Rgb::new(0.36, 0.56, 1.0);

/// The chroma push applied to the sky band's hue.
///
/// `sky.ambientColor` is the **whole-sky** average — zenith Rayleigh blue mixed
/// with a horizon band that is nearly achromatic by day. This band is not the
/// whole sky: it is what an up-facing or vertical surface sees of the *upper*
/// hemisphere, which is the bluest part of it. Pushing the chroma out from the
/// hue's own luminance recovers that without inventing a hue, and it is what
/// took a shaded facade from a measured B-R of +0.0002 (4.5% saturation,
/// indistinguishable from grey) to something that reads as skylight.
const CHROMA_PUSH: f64 = 1.18;

/// Rec.709 luminance weights, as `_updateBounceFill` writes them.
const LUMA: (f64, f64, f64) = (0.2126, 0.7152, 0.0722);

/// The ground-bounce hue: the key's colour through the ground albedo the sky
/// dome itself uses — warm, not blue.
const GROUND_ALBEDO: (f64, f64, f64) = (0.33, 0.29, 0.225);

/// `_updateBounceFill`.
pub(crate) fn bounce_fill(
    settings: &super::settings::FrameSettings,
    sun_intensity: f64,
    sun_color: Rgb,
    sky: Option<SkyPublication>,
) -> BounceFill {
    let sun_i = sun_intensity.max(0.0);

    // Hue of the whole-sky band, and the ambient level that comes with it.
    let published = sky.filter(|s| s.ambient_color.max_channel() > 1e-5);
    let hue0 = published.map_or(FALLBACK_SKY_HUE, |s| s.ambient_color);
    let ambient_level = published.map_or(
        SKY_AMBIENT_FRACTION * sun_i,
        // The published level is read from the *unnormalised* colour, before
        // the divide below — reordering those two lines changes the viewmodel's
        // brightness at every time of day.
        |s| s.ambient_color.max_channel(),
    );

    let hue1 = hue0.divide_scalar(hue0.max_channel());
    let l = LUMA.0 * hue1.r + LUMA.1 * hue1.g + LUMA.2 * hue1.b;
    let pushed = Rgb::new(
        (l + (hue1.r - l) * CHROMA_PUSH).max(0.0),
        (l + (hue1.g - l) * CHROMA_PUSH).max(0.0),
        (l + (hue1.b - l) * CHROMA_PUSH).max(0.0),
    );
    let hue = pushed.divide_scalar(pushed.max_channel().max(1e-6));

    // The cool band rides the sky's own published irradiance, not the key: at
    // night the key is a 0.05 moon, and a band scaled off it is nothing — which
    // is how a night frame ends up with a fifth of its pixels under code value
    // 12.
    let sky_ref = ambient_level / SKY_AMBIENT_FRACTION;
    let sky_level = settings.sky_fill * sky_ref;

    let ground0 = Rgb::new(
        sun_color.r * GROUND_ALBEDO.0,
        sun_color.g * GROUND_ALBEDO.1,
        sun_color.b * GROUND_ALBEDO.2,
    );
    let ground = ground0.divide_scalar(ground0.max_channel().max(1e-6));
    let ground_level = settings.ground_fill * sun_i;

    BounceFill {
        sky_fill: hue.scale(sky_level),
        ground_fill: ground.scale(ground_level),
        fill_gain: (1.0, settings.bounce_fill / settings.ground_fill.max(1e-6)),
        ibl_diffuse: settings.ibl_diffuse * sky.map_or(1.0, |s| s.indirect_scale),
        interior_indirect: settings.interior_indirect,
        ambient_level,
        sky_exposure_bias: sky.map_or(0.0, |s| s.exposure_bias),
    }
}

/// One light of the viewmodel's three-point rig (four, with the bounce).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RigLight {
    /// Colour.
    pub(crate) color: Rgb,
    /// Intensity.
    pub(crate) intensity: f64,
}

/// `_updateViewRig`'s output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewRig {
    /// The key, taking the active sun's colour.
    pub(crate) key: RigLight,
    /// The cool fill, taking the sky band's hue.
    pub(crate) fill: RigLight,
    /// The warm rim, taking the key's hue pulled toward orange.
    pub(crate) rim: RigLight,
    /// The hemisphere. Its colours are authored on the light and never touched;
    /// only the intensity moves.
    pub(crate) hemisphere_intensity: f64,
    /// The warm ground bounce from below.
    pub(crate) bounce: RigLight,
}

/// View-space directions the rig's lights arrive **from**, normalised.
///
/// Authored in view space and rotated into the viewmodel scene every frame, so
/// the weapon's key/fill/rim separation is invariant to where the world sun
/// happens to be — which is what every shipped FPS does, and the reason their
/// guns are always legible. Key upper-front-left, fill lower-front-right, rim
/// from behind to catch the top edges of the receiver, rail and optic body,
/// bounce from below-front.
pub(crate) const VIEW_RIG_DIRECTIONS: [(f64, f64, f64); 4] = [
    (-0.45, 0.75, 0.55),
    (0.6, -0.15, 0.5),
    (0.2, 0.35, -0.9),
    (-0.2, -0.86, 0.47),
];

/// How far along its direction each rig light is placed from the view camera.
pub(crate) const VIEW_RIG_DISTANCE: f64 = 4.0;

/// `_updateViewRig`.
///
/// `sky_hue` is `this._fillHue` and `ground_hue` is `this._fillHue2`, both
/// written by [`bounce_fill`] on the same frame — the gun and the gloves pick
/// up the same sand-off-the-street colour the buildings do.
pub(crate) fn view_rig(
    settings: &super::settings::FrameSettings,
    sun_intensity: f64,
    sun_color: Rgb,
    ambient_level: f64,
    sky_hue: Rgb,
    ground_hue: Rgb,
    previous_bounce_color: Rgb,
) -> ViewRig {
    // The reference level has to include the ambient, not just the key: at
    // night the key IS the moon at 0.075, and a rig with an absolute floor
    // would put a glowing white rifle in a moonlit street.
    let reference = sun_intensity.max(ambient_level / SKY_AMBIENT_FRACTION);
    // Sub-linear in the scene level, because the meter is exposure-locked after
    // dark. A no-op in full daylight.
    let shaped = REF_DAYLIGHT
        * (reference / REF_DAYLIGHT)
            .min(1.0)
            .powf(settings.view_key_gamma);
    // The `min` against `view_key_max` is **dead**: the ratio above is clamped
    // at one, so `shaped <= REF_DAYLIGHT` and the product is bounded by
    // `4.6 * 0.55 = 2.53`, under the authored 2.6 ceiling for every input.
    // Ported anyway — dead computation in the source is still part of the
    // source — and pinned by
    // `tests::the_viewmodel_key_is_shaped_and_capped`.
    let key_i = (shaped * settings.view_key_scale).min(settings.view_key_max);

    // The bounce keeps its previous colour when the ground hue is black — the
    // source's `if (max(g) > 1e-5)` guards only the assignment, never the
    // intensity below it.
    let bounce_color = [
        previous_bounce_color,
        Rgb::new(ground_hue.r, ground_hue.g * 0.86, ground_hue.b * 0.62),
    ][usize::from(ground_hue.max_channel() > 1e-5)];

    ViewRig {
        key: RigLight {
            color: sun_color,
            intensity: key_i,
        },
        fill: RigLight {
            color: sky_hue,
            intensity: key_i * settings.view_fill_ratio,
        },
        rim: RigLight {
            color: Rgb::new(sun_color.r, sun_color.g * 0.94, sun_color.b * 0.82),
            intensity: key_i * settings.view_rim_ratio,
        },
        hemisphere_intensity: key_i * settings.view_hemi_ratio,
        bounce: RigLight {
            color: bounce_color,
            intensity: key_i * settings.view_bounce_ratio,
        },
    }
}

/// The number of children `init()` adds to `viewScene` for the rig itself.
///
/// Four directional lights, each added with its own `target`
/// (`viewScene.add(l, l.target)`), plus one hemisphere light: nine. The frame
/// tests `viewScene.children.length > this._viewRigChildren` to decide whether
/// there is a weapon to draw at all, so this count is a *behavioural* constant
/// and not bookkeeping.
///
/// It assumes `viewScene` is empty when the render subsystem initialises, which
/// `RenderSystem.deps = []` makes true in the shipped boot order. A subsystem
/// that added a child to `viewScene` before the renderer initialised would
/// raise the count and hide the viewmodel forever.
pub(crate) const VIEW_RIG_CHILDREN: usize = 9;

/// `this._viewVisible = viewScene.children.length > this._viewRigChildren`.
pub(crate) fn view_visible(view_scene_children: usize) -> bool {
    view_scene_children > VIEW_RIG_CHILDREN
}

/// Whether the viewmodel may sample the cascades at all:
/// `viewCamera` within 0.5 m of the world camera, tested squared.
///
/// The cascade lookup is a world-space one, so a viewmodel camera that does not
/// share the world camera's position would read nonsense.
pub(crate) fn viewmodel_shadows_coherent(distance_squared: f64) -> bool {
    distance_squared < 0.25
}

#[cfg(test)]
mod tests {
    use super::{
        bounce_fill, cull_light, smoothstep, sync_sun, view_rig, view_visible,
        viewmodel_shadows_coherent, RegisteredLight, Rgb, SkyPublication, DEFAULT_LIGHT_RANGE,
        PRACTICAL_RANGE, REF_DAYLIGHT, VIEW_RIG_CHILDREN, VIEW_RIG_DIRECTIONS,
    };
    use crate::frame_graph::settings::SOURCE_SETTINGS;

    /// three's `smoothstep(x, min, max)`, not GLSL's `smoothstep(min, max, x)`.
    #[test]
    fn smoothstep_takes_the_value_first_and_the_edges_after() {
        assert_eq!(smoothstep(0.0, 0.0, 1.0), 0.0);
        assert_eq!(smoothstep(1.0, 0.0, 1.0), 1.0);
        assert_eq!(smoothstep(0.5, 0.0, 1.0), 0.5);
        assert_eq!(smoothstep(-3.0, 0.0, 1.0), 0.0);
        assert_eq!(smoothstep(9.0, 0.0, 1.0), 1.0);
        // The cubic, at a value the GLSL argument order would get wrong.
        let t: f64 = 0.25;
        assert_eq!(smoothstep(0.25, 0.0, 1.0), t * t * (3.0 - 2.0 * t));
        // Degenerate edges: the ordered tests fire and the NaN never escapes.
        assert_eq!(smoothstep(5.0, 2.0, 2.0), 1.0);
        assert_eq!(smoothstep(1.0, 2.0, 2.0), 0.0);
        // Inverted edges, which nothing calls but the source still answers:
        // `x <= min` is tested *first*, so 3 inside [2, 5] reversed is 0.
        assert_eq!(smoothstep(3.0, 5.0, 2.0), 0.0);
        assert_eq!(smoothstep(6.0, 5.0, 2.0), 1.0);
    }

    /// The distance fade, the practical trim, and the visibility cut.
    #[test]
    fn a_practical_is_trimmed_and_a_flash_is_not() {
        let lamp = RegisteredLight {
            range: 20.0,
            priority: 1.0,
            base_intensity: 4.0,
            applied: None,
        };
        // Well inside the fade: full base, trimmed by the practical gain.
        let near = cull_light(lamp, 4.0, 1.0, SOURCE_SETTINGS.practical_gain);
        assert_eq!(near.intensity, 4.0 * SOURCE_SETTINGS.practical_gain);
        assert!(near.visible);
        // Past the far edge (20 * 1.15 = 23): faded to nothing and hidden.
        let far = cull_light(lamp, 4.0, 30.0, SOURCE_SETTINGS.practical_gain);
        assert_eq!(far.intensity, 0.0);
        assert!(!far.visible);

        // The FX flash pool registers at 90 m so neither the fade nor the trim
        // reaches it: a muzzle flash must not be dimmed by a room control.
        let flash = RegisteredLight { range: 90.0, ..lamp };
        assert!(flash.range > PRACTICAL_RANGE);
        let flashed = cull_light(flash, 4.0, 1.0, SOURCE_SETTINGS.practical_gain);
        assert_eq!(flashed.intensity, 4.0, "no trim above the practical range");

        // The boundary is inclusive: exactly 30 m is still a practical.
        let boundary = RegisteredLight { range: PRACTICAL_RANGE, ..lamp };
        assert_eq!(
            cull_light(boundary, 4.0, 1.0, 0.5).intensity,
            2.0,
            "`range <= PRACTICAL_RANGE` includes the boundary"
        );
        // ...and the default registration range is well under it.
        assert!(DEFAULT_LIGHT_RANGE < PRACTICAL_RANGE);
    }

    /// An owner that animates the intensity has its new value adopted as the
    /// base, rather than fought over — which is what lets a lamp flicker.
    #[test]
    fn an_animated_intensity_becomes_the_new_base() {
        let lamp = RegisteredLight {
            range: 90.0,
            priority: 1.0,
            base_intensity: 4.0,
            applied: Some(4.0),
        };
        // The owner wrote 1.5 since we last wrote 4.0: adopt it.
        let animated = cull_light(lamp, 1.5, 1.0, 0.55);
        assert_eq!(animated.base_intensity, 1.5);
        assert_eq!(animated.intensity, 1.5);
        // The owner wrote back exactly what we wrote: nothing was animated.
        let untouched = cull_light(lamp, 4.0, 1.0, 0.55);
        assert_eq!(untouched.base_intensity, 4.0);
        // A light that has never been culled has no `applied` to compare, so
        // its live intensity is ignored entirely this frame.
        let fresh = RegisteredLight { applied: None, ..lamp };
        assert_eq!(cull_light(fresh, 99.0, 1.0, 0.55).base_intensity, 4.0);
    }

    /// The brightest directional wins, ties go to the first, and the running
    /// best starts below zero so a negative intensity is still *considered*.
    #[test]
    fn the_sun_takeover_picks_the_brightest_and_then_checks_it_is_bright_at_all() {
        assert_eq!(sync_sun(&[0.4, 4.3, 1.0]), Some(1));
        assert_eq!(sync_sun(&[4.3, 4.3]), Some(0), "strict `>`, so the first wins");
        assert_eq!(sync_sun(&[]), None, "no candidates: keep the fallback sun");
        // The `> 0.01` gate is separate from the scan: a dim or negative
        // candidate wins the scan and then fails the gate.
        assert_eq!(sync_sun(&[0.005]), None);
        assert_eq!(sync_sun(&[-0.5]), None);
        assert_eq!(sync_sun(&[0.01]), None, "the gate is strict");
        assert_eq!(sync_sun(&[0.0100001]), Some(0));
        // A negative candidate does not shadow a valid one.
        assert_eq!(sync_sun(&[-2.0, 3.0]), Some(1));
    }

    /// The bounce fill, with a sky publishing a cool ambient.
    #[test]
    fn the_sky_band_takes_its_level_from_the_sky_and_its_hue_pushed_outward() {
        let sky = SkyPublication {
            ambient_color: Rgb::new(0.20, 0.30, 0.45),
            indirect_scale: 0.8,
            exposure_bias: 0.3,
        };
        let out = bounce_fill(&SOURCE_SETTINGS, 4.3, Rgb::new(1.0, 0.91, 0.77), Some(sky));

        // `_ambLevel` is the *unnormalised* channel maximum.
        assert_eq!(out.ambient_level, 0.45);
        // ...and the level is `skyFill * (ambLevel / 0.15)`.
        let expected_level = SOURCE_SETTINGS.sky_fill * (0.45 / 0.15);
        assert!((out.sky_fill.max_channel() - expected_level).abs() < 1e-12);
        // The push makes the band bluer than the ambient it came from: blue is
        // the max channel, so it normalises to 1 and red falls below its
        // pre-push ratio.
        let pre_push_red = 0.20 / 0.45;
        assert!(out.sky_fill.r / out.sky_fill.b < pre_push_red);

        // The ground band is warm, taking the key's colour through the ground
        // albedo, and rides the *key* rather than the sky.
        assert!(out.ground_fill.r > out.ground_fill.b);
        assert_eq!(out.ground_fill.max_channel(), SOURCE_SETTINGS.ground_fill * 4.3);

        // The wrap gain is a ratio of two authored settings.
        assert_eq!(out.fill_gain.0, 1.0);
        assert_eq!(
            out.fill_gain.1,
            SOURCE_SETTINGS.bounce_fill / SOURCE_SETTINGS.ground_fill
        );
        // The sky's elevation-dependent budget scales the IBL diffuse.
        assert_eq!(out.ibl_diffuse, SOURCE_SETTINGS.ibl_diffuse * 0.8);
        assert_eq!(out.interior_indirect, SOURCE_SETTINGS.interior_indirect);
        assert_eq!(out.sky_exposure_bias, 0.3);
    }

    /// Without a sky subsystem — or with a black one — the fallback hue is used
    /// and the ambient level is taken as 15% of the beam.
    #[test]
    fn a_missing_sky_falls_back_to_a_hue_and_a_fifteen_percent_ambient() {
        let sun = Rgb::new(1.0, 0.91, 0.77);
        let none = bounce_fill(&SOURCE_SETTINGS, 4.3, sun, None);
        assert!((none.ambient_level - 0.15 * 4.3).abs() < 1e-12);
        assert_eq!(none.ibl_diffuse, SOURCE_SETTINGS.ibl_diffuse);
        assert_eq!(none.sky_exposure_bias, 0.0);
        // A published-but-black ambient takes the same arm.
        let black = bounce_fill(
            &SOURCE_SETTINGS,
            4.3,
            sun,
            Some(SkyPublication {
                ambient_color: Rgb::new(0.0, 0.0, 1e-6),
                indirect_scale: 1.0,
                exposure_bias: 0.0,
            }),
        );
        assert_eq!(black.ambient_level, none.ambient_level);
        assert_eq!(black.sky_fill, none.sky_fill);
        // A negative sun intensity is floored at zero before anything scales by
        // it, so a mis-authored light cannot invert the fill.
        let dark = bounce_fill(&SOURCE_SETTINGS, -3.0, sun, None);
        assert_eq!(dark.ambient_level, 0.0);
        assert_eq!(dark.ground_fill.max_channel(), 0.0);
    }

    /// The viewmodel key is sub-linear in the scene level and capped, so the
    /// gun reads the same at noon and at midnight.
    #[test]
    fn the_viewmodel_key_is_shaped_and_capped() {
        let sun = Rgb::new(1.0, 0.91, 0.77);
        let hue = Rgb::new(0.36, 0.56, 1.0);
        let ground = Rgb::new(1.0, 0.86, 0.62);
        let rig = |intensity, ambient| {
            view_rig(&SOURCE_SETTINGS, intensity, sun, ambient, hue, ground, ground)
        };

        // Full daylight: the shaping is a no-op — `ref / REF_DAYLIGHT` is 1
        // and any power of 1 is 1 — so the key is the plain scale, and the cap
        // sits just above it rather than binding.
        let noon = rig(REF_DAYLIGHT, 0.15 * REF_DAYLIGHT);
        let uncapped = REF_DAYLIGHT * SOURCE_SETTINGS.view_key_scale;
        assert_eq!(noon.key.intensity, uncapped);
        assert!(
            uncapped < SOURCE_SETTINGS.view_key_max,
            "{uncapped} must sit under the {} cap in plain daylight",
            SOURCE_SETTINGS.view_key_max
        );
        // ...and it can never bind at all. `min(ref / REF_DAYLIGHT, 1)` caps
        // the ratio at one, so `shaped <= REF_DAYLIGHT` and the key is bounded
        // by `REF_DAYLIGHT * viewKeyScale = 2.53` — under the 2.6 ceiling for
        // every input there is. **`viewKeyMax` is dead in the source.** Pinned
        // rather than removed: it is one edit to either number away from being
        // live, and a reader will look for it.
        let blazing = rig(1000.0 * REF_DAYLIGHT, 1.0e6);
        assert_eq!(blazing.key.intensity, uncapped);
        assert!(blazing.key.intensity < SOURCE_SETTINGS.view_key_max);

        // Moonlight: the gamma lifts the key well above a linear scaling would.
        let night = rig(0.075, 0.0);
        let linear = 0.075 * SOURCE_SETTINGS.view_key_scale;
        assert!(
            night.key.intensity > 4.0 * linear,
            "gamma 0.65 lifts {} to {}",
            linear,
            night.key.intensity
        );
        assert!(night.key.intensity < noon.key.intensity);

        // A bright ambient with a dim key still drives the rig: `ref` is the
        // max of the key and the ambient scaled back to a beam.
        let overcast = rig(0.05, 0.5);
        assert!(overcast.key.intensity > night.key.intensity);

        // Every other light is a fixed ratio of the key.
        assert_eq!(
            noon.fill.intensity,
            noon.key.intensity * SOURCE_SETTINGS.view_fill_ratio
        );
        assert_eq!(
            noon.rim.intensity,
            noon.key.intensity * SOURCE_SETTINGS.view_rim_ratio
        );
        assert_eq!(
            noon.hemisphere_intensity,
            noon.key.intensity * SOURCE_SETTINGS.view_hemi_ratio
        );
        assert_eq!(
            noon.bounce.intensity,
            noon.key.intensity * SOURCE_SETTINGS.view_bounce_ratio
        );
        // The key takes the sun's colour; the rim pulls it warm; the fill takes
        // the sky band's hue.
        assert_eq!(noon.key.color, sun);
        assert_eq!(noon.fill.color, hue);
        assert_eq!(noon.rim.color, Rgb::new(1.0, 0.91 * 0.94, 0.77 * 0.82));
        assert_eq!(noon.bounce.color, Rgb::new(1.0, 0.86 * 0.86, 0.62 * 0.62));
    }

    /// A black ground hue leaves the bounce light's colour alone — the source
    /// guards the assignment and not the intensity, so an unlit street still
    /// yields a bounce at full ratio in whatever colour it last had.
    #[test]
    fn a_black_ground_hue_keeps_the_bounce_colour_but_not_its_intensity() {
        let sun = Rgb::new(1.0, 0.91, 0.77);
        let hue = Rgb::new(0.36, 0.56, 1.0);
        let previous = Rgb::new(0.25, 0.5, 0.75);
        let rig = view_rig(
            &SOURCE_SETTINGS,
            4.6,
            sun,
            0.69,
            hue,
            Rgb::new(0.0, 0.0, 0.0),
            previous,
        );
        assert_eq!(rig.bounce.color, previous, "the colour is not reassigned");
        assert!(rig.bounce.intensity > 0.0, "the intensity is written anyway");
    }

    /// The rig's own children are what the frame counts against, and the four
    /// authored directions are the rig's shape.
    #[test]
    fn the_rig_contributes_nine_children_and_four_directions() {
        assert_eq!(VIEW_RIG_CHILDREN, 9, "4 lights x (light + target) + 1 hemisphere");
        assert!(!view_visible(VIEW_RIG_CHILDREN));
        assert!(view_visible(VIEW_RIG_CHILDREN + 1));
        assert_eq!(VIEW_RIG_DIRECTIONS.len(), 4);
        // Key upper-front-left, fill lower-front-right, rim behind, bounce
        // below-front: the signs are the rig, and are worth pinning.
        assert!(VIEW_RIG_DIRECTIONS[0].1 > 0.0, "key is above");
        assert!(VIEW_RIG_DIRECTIONS[1].1 < 0.0, "fill is below");
        assert!(VIEW_RIG_DIRECTIONS[2].2 < 0.0, "rim is behind");
        assert!(VIEW_RIG_DIRECTIONS[3].1 < 0.0, "bounce is from the ground");
    }

    /// The viewmodel keeps the cascades only while its camera shares the
    /// world camera's position, tested at half a metre, squared.
    #[test]
    fn the_viewmodel_loses_the_cascades_when_its_camera_diverges() {
        assert!(viewmodel_shadows_coherent(0.0));
        assert!(viewmodel_shadows_coherent(0.24));
        assert!(!viewmodel_shadows_coherent(0.25), "0.5 m, and the test is strict");
        assert!(!viewmodel_shadows_coherent(4.0));
    }
}
