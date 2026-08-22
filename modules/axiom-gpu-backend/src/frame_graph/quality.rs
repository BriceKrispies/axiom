//! **The quality tiers** — `src/core/config.js`'s `QUALITY_PRESETS` and
//! `index.js`'s `QUALITY_LEVEL`, transcribed.
//!
//! This is the frame graph's main structure. The tier decides which passes are
//! constructed at all, the internal render scale, the cascade count and map
//! size, the viewmodel's MSAA sample count and the bloom pyramid's depth — and
//! it is what the original announces on boot:
//!
//! ```text
//! [render] WebGL2 · ultra · 4x2048 CSM · taa:true gtao:true ssr:true mb:true
//! ```
//!
//! # The `4096` that is never 4096
//!
//! The `ultra` preset asks for `shadowMapSize: 4096`. It never gets one:
//! `CascadedShadowMaps`'s constructor is
//! `this.mapSize = Math.min(opts.mapSize ?? 2048, 2048)`, with the source's own
//! reason ("4 x 4096 x R32F is a quarter of a gigabyte for shadows nobody can
//! see; 2048 with PCSS reads sharper than 4096 without it"). So the request is
//! silently clamped and the boot line reads `4x2048`, which is exactly the
//! string the original prints. Ported as a clamp rather than as a corrected
//! preset value, because the preset value is what the source file contains —
//! see [`QualityPreset::shadow_map_size`] and [`CsmConfig::map_size`], and the
//! test that pins their disagreement.
//!
//! # Two of these numbers are not the renderer's
//!
//! `volumetrics`, `particleBudget`, `decalBudget` and `anisotropy` are in the
//! preset table but `RenderSystem` reads only the last of them
//! (`Math.min(q.anisotropy, renderer.capabilities.getMaxAnisotropy())`). The
//! other three are consumed by the sky and FX subsystems. They are carried here
//! anyway, because the preset table is one datum and splitting it across two
//! owners is how a tier ends up meaning two different things.

use crate::bloom_pyramid::schedule::{LEVELS_HIGH, LEVELS_LOW};
use crate::cascade::{MAP_SIZE, MAX_CASCADES};

/// `QUALITY_LEVEL = { low: 0, medium: 1, high: 2, ultra: 3 }`.
///
/// **The discriminant is the table index.** `index.js` compares against it
/// numerically in five places (`qLevel >= 1` for contact shadows and ADS depth
/// of field, `qLevel >= 2` for the bloom level count and the 4x viewmodel MSAA,
/// `qLevel >= 1` for 2x) and hands it to `MaterialPatcher` and
/// [`crate::cascade::quality_tier`], which index tap-count tables with it. An
/// enum used as a table index is order-dependent; this order is the source's
/// and must not be re-sorted.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum QualityTier {
    /// `renderScale 0.72`, 3x1024 cascades, no TAA / GTAO / SSR / motion blur.
    Low = 0,
    /// `renderScale 0.85`, 3x2048, TAA + GTAO + motion blur, still no SSR.
    Medium = 1,
    /// Full internal resolution, 4x2048, everything on.
    High = 2,
    /// The default, and what the original boots with. Differs from `High` only
    /// in shadow *distance* and the budgets — its `4096` map is clamped away.
    Ultra = 3,
}

/// Every tier, in discriminant order. The one place the set is enumerated.
pub(crate) const QUALITY_TIERS: [QualityTier; 4] = [
    QualityTier::Low,
    QualityTier::Medium,
    QualityTier::High,
    QualityTier::Ultra,
];

/// The preset keys, in the same order — the strings `cfg.quality` holds and the
/// boot line prints.
pub(crate) const QUALITY_NAMES: [&str; 4] = ["low", "medium", "high", "ultra"];

/// `DEFAULTS.quality = 'ultra'`, and also the `?? 3` fallback in
/// `QUALITY_LEVEL[cfg.quality] ?? 3` — the same tier by coincidence of the
/// source, and pinned as one value so it stays that way.
pub(crate) const DEFAULT_QUALITY: QualityTier = QualityTier::Ultra;

/// One row of `QUALITY_PRESETS`, field for field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct QualityPreset {
    /// Fraction of the *device* backbuffer the internal HDR chain runs at.
    /// A JS number, so `f64`: `Math.floor(dw * renderScale)` is evaluated at
    /// double width and the floor is where the width stops mattering.
    pub(crate) render_scale: f64,
    /// What the preset **asks** for. `ultra`'s `4096` is clamped to 2048 by the
    /// CSM constructor; see [`CsmConfig::map_size`].
    pub(crate) shadow_map_size: u32,
    /// Cascade count, before the constructor's `max(1, min(4, n | 0))`.
    pub(crate) cascades: u32,
    /// `maxDistance` for the cascade split scheme, in metres.
    pub(crate) shadow_distance: f32,
    /// Temporal anti-aliasing. Also decides FXAA (`q.taa ? null : createFxaa()`),
    /// the camera jitter, the CSM's per-frame jitter index, and the composite's
    /// sharpen term.
    pub(crate) taa: bool,
    /// Horizon-arc AO.
    pub(crate) gtao: bool,
    /// Screen-space reflections.
    pub(crate) ssr: bool,
    /// Read by the sky subsystem, not by `RenderSystem`.
    pub(crate) volumetrics: bool,
    /// Velocity-tile motion blur.
    pub(crate) motion_blur: bool,
    /// The bloom pyramid. True in every shipped preset; the gate is ported
    /// because the source has it, not because a preset exercises it.
    pub(crate) bloom: bool,
    /// Requested anisotropy, clamped at bind against the adapter's maximum.
    pub(crate) anisotropy: u32,
    /// FX subsystem budget, carried for completeness.
    pub(crate) particle_budget: u32,
    /// FX subsystem budget, carried for completeness.
    pub(crate) decal_budget: u32,
}

/// `QUALITY_PRESETS`, transcribed from `src/core/config.js` in table order.
pub(crate) const QUALITY_PRESETS: [QualityPreset; 4] = [
    QualityPreset {
        render_scale: 0.72,
        shadow_map_size: 1024,
        cascades: 3,
        shadow_distance: 60.0,
        taa: false,
        gtao: false,
        ssr: false,
        volumetrics: false,
        motion_blur: false,
        bloom: true,
        anisotropy: 4,
        particle_budget: 2000,
        decal_budget: 64,
    },
    QualityPreset {
        render_scale: 0.85,
        shadow_map_size: 2048,
        cascades: 3,
        shadow_distance: 90.0,
        taa: true,
        gtao: true,
        ssr: false,
        volumetrics: true,
        motion_blur: true,
        bloom: true,
        anisotropy: 8,
        particle_budget: 6000,
        decal_budget: 128,
    },
    QualityPreset {
        render_scale: 1.0,
        shadow_map_size: 2048,
        cascades: 4,
        shadow_distance: 140.0,
        taa: true,
        gtao: true,
        ssr: true,
        volumetrics: true,
        motion_blur: true,
        bloom: true,
        anisotropy: 16,
        particle_budget: 12000,
        decal_budget: 256,
    },
    QualityPreset {
        render_scale: 1.0,
        shadow_map_size: 4096,
        cascades: 4,
        shadow_distance: 200.0,
        taa: true,
        gtao: true,
        ssr: true,
        volumetrics: true,
        motion_blur: true,
        bloom: true,
        anisotropy: 16,
        particle_budget: 24000,
        decal_budget: 512,
    },
];

/// What `CascadedShadowMaps`'s constructor makes of a preset's shadow numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CsmConfig {
    /// `Math.max(1, Math.min(4, opts.cascades | 0))`.
    pub(crate) cascades: usize,
    /// `Math.min(opts.mapSize ?? 2048, 2048)` — **the clamp that makes `ultra`
    /// print `4x2048` rather than `4x4096`.**
    pub(crate) map_size: u32,
    /// `opts.maxDistance ?? 140`, supplied by the preset in every tier, so the
    /// `?? 140` default is dead here. [`crate::cascade::MAX_DISTANCE`] is that
    /// dead default, **not** the value any tier runs with.
    pub(crate) max_distance: f32,
}

impl QualityTier {
    /// The numeric level `QUALITY_LEVEL` maps this tier to — its discriminant.
    pub(crate) const fn level(self) -> u32 {
        self as u32
    }

    /// The preset key, as `cfg.quality` holds it.
    pub(crate) const fn name(self) -> &'static str {
        QUALITY_NAMES[self as usize]
    }

    /// This tier's row of `QUALITY_PRESETS`.
    pub(crate) const fn preset(self) -> QualityPreset {
        QUALITY_PRESETS[self as usize]
    }

    /// `QUALITY_LEVEL[cfg.quality] ?? 3` — an unrecognised name resolves to
    /// [`DEFAULT_QUALITY`] rather than failing, exactly as the source does.
    pub(crate) fn from_name(name: &str) -> Self {
        QUALITY_TIERS[QUALITY_NAMES
            .iter()
            .position(|candidate| *candidate == name)
            .unwrap_or(DEFAULT_QUALITY as usize)]
    }

    /// What the CSM constructor makes of this tier's shadow numbers.
    ///
    /// `| 0` is `ToInt32`, which **wraps** where a Rust cast saturates. Every
    /// shipped preset holds 3 or 4, so the conversion is exact and the wrap has
    /// no referent; the clamp below is what the source's `max`/`min` do.
    pub(crate) const fn csm(self) -> CsmConfig {
        let preset = self.preset();
        CsmConfig {
            cascades: clamp_cascades(preset.cascades),
            map_size: min_u32(preset.shadow_map_size, MAP_SIZE),
            max_distance: preset.shadow_distance,
        }
    }

    /// `this._viewSamples = qLevel >= 2 ? 4 : qLevel >= 1 ? 2 : 0` — MSAA on
    /// the viewmodel target only, because it is the one buffer whose geometric
    /// edges no longer get a temporal filter.
    pub(crate) const fn view_samples(self) -> u32 {
        [0, 2, 4, 4][self as usize]
    }

    /// `new Bloom(this.qLevel >= 2 ? 6 : 5)`, gated by `q.bloom`.
    ///
    /// `None` is the source's `this.bloom = null`, which the frame turns into
    /// `bloomTex = null` and a composite strength of zero.
    pub(crate) const fn bloom_levels(self) -> Option<usize> {
        let levels = [LEVELS_LOW, LEVELS_LOW, LEVELS_HIGH, LEVELS_HIGH][self as usize];
        [None, Some(levels)][self.preset().bloom as usize]
    }
}

/// `Math.max(1, Math.min(4, n))`, with the ceiling taken from
/// [`crate::cascade::MAX_CASCADES`] so the two cannot drift.
const fn clamp_cascades(n: u32) -> usize {
    let capped = min_u32(n, MAX_CASCADES as u32);
    [capped, 1][(capped < 1) as usize] as usize
}

/// `Math.min` over two `u32`, as a `const fn` (`u32::min` is not one here).
const fn min_u32(a: u32, b: u32) -> u32 {
    [b, a][(a < b) as usize]
}

/// The boot line `index.js` prints at the end of `init`, reproduced exactly:
///
/// ```text
/// `[render] WebGL2 · ${cfg.quality} · ${this.csm.cascades}x${this.csm.mapSize} CSM · ` +
///   `taa:${!!this.taa} gtao:${!!this.gtao} ssr:${!!this.ssr} mb:${!!this.motionBlur}`
/// ```
///
/// Returned as a `String` rather than logged: no console output is permitted
/// anywhere in the spine, and a value is testable where a side effect is not.
pub(crate) fn boot_line(tier: QualityTier) -> String {
    let preset = tier.preset();
    let csm = tier.csm();
    format!(
        "[render] WebGL2 · {} · {}x{} CSM · taa:{} gtao:{} ssr:{} mb:{}",
        tier.name(),
        csm.cascades,
        csm.map_size,
        preset.taa,
        preset.gtao,
        preset.ssr,
        preset.motion_blur,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        boot_line, clamp_cascades, QualityTier, DEFAULT_QUALITY, QUALITY_NAMES, QUALITY_PRESETS,
        QUALITY_TIERS,
    };
    use crate::bloom_pyramid::schedule::{LEVELS_HIGH, LEVELS_LOW};
    use crate::cascade::MAP_SIZE;

    /// `QUALITY_LEVEL`'s values *are* the discriminants, and five comparisons
    /// in `index.js` read them numerically. Re-sorting this enum would silently
    /// move contact shadows, the ADS depth of field, the bloom depth and the
    /// viewmodel MSAA onto different tiers.
    #[test]
    fn the_tier_order_is_the_source_table_order() {
        assert_eq!(QualityTier::Low.level(), 0);
        assert_eq!(QualityTier::Medium.level(), 1);
        assert_eq!(QualityTier::High.level(), 2);
        assert_eq!(QualityTier::Ultra.level(), 3);
        // The three parallel tables are indexed by that same discriminant.
        QUALITY_TIERS.iter().enumerate().for_each(|(i, &tier)| {
            assert_eq!(tier.level() as usize, i, "tier {tier:?} moved");
            assert_eq!(tier.name(), QUALITY_NAMES[i]);
            assert_eq!(tier.preset(), QUALITY_PRESETS[i]);
        });
        assert_eq!(DEFAULT_QUALITY, QualityTier::Ultra);
    }

    /// Every tier's banner, so the string the original prints is a fixture
    /// rather than a memory.
    #[test]
    fn each_tier_announces_itself_the_way_the_original_does() {
        assert_eq!(
            boot_line(QualityTier::Ultra),
            "[render] WebGL2 · ultra · 4x2048 CSM · taa:true gtao:true ssr:true mb:true"
        );
        assert_eq!(
            boot_line(QualityTier::High),
            "[render] WebGL2 · high · 4x2048 CSM · taa:true gtao:true ssr:true mb:true"
        );
        assert_eq!(
            boot_line(QualityTier::Medium),
            "[render] WebGL2 · medium · 3x2048 CSM · taa:true gtao:true ssr:false mb:true"
        );
        assert_eq!(
            boot_line(QualityTier::Low),
            "[render] WebGL2 · low · 3x1024 CSM · taa:false gtao:false ssr:false mb:false"
        );
    }

    /// `high` and `ultra` print an **identical** CSM fragment despite asking
    /// for different map sizes, because the constructor clamps `ultra`'s
    /// request away. That is the port's evidence that the clamp is real.
    #[test]
    fn the_ultra_tiers_four_thousand_ninety_six_is_clamped_to_two_thousand_forty_eight() {
        let asked = QualityTier::Ultra.preset().shadow_map_size;
        let got = QualityTier::Ultra.csm().map_size;
        assert_eq!(asked, 4096, "the preset must still contain the source value");
        assert_eq!(got, MAP_SIZE, "the constructor clamps it to the CSM's ceiling");
        assert_ne!(asked, got, "clamping {asked} to {got} is the whole point");
        assert_eq!(QualityTier::High.csm(), QualityTier::Ultra.csm());
        // The *distance* is where the two tiers genuinely differ.
        assert_eq!(QualityTier::High.preset().shadow_distance, 140.0);
        assert_eq!(QualityTier::Ultra.preset().shadow_distance, 200.0);
    }

    /// The cascade clamp's three arms: below the floor, inside the range, above
    /// the ceiling. Only the middle one is reachable from a shipped preset, so
    /// the other two are exercised directly rather than left as dead table rows.
    #[test]
    fn the_cascade_count_is_clamped_into_one_through_four() {
        assert_eq!(clamp_cascades(0), 1);
        assert_eq!(clamp_cascades(1), 1);
        assert_eq!(clamp_cascades(3), 3);
        assert_eq!(clamp_cascades(4), 4);
        assert_eq!(clamp_cascades(9), 4);
        // ...and the shipped presets all sit inside it.
        assert_eq!(QualityTier::Low.csm().cascades, 3);
        assert_eq!(QualityTier::Ultra.csm().cascades, 4);
    }

    /// `qLevel >= 2 ? 4 : qLevel >= 1 ? 2 : 0`, and the bloom depth beside it.
    #[test]
    fn the_two_level_keyed_ladders_step_where_the_source_steps() {
        let samples: Vec<u32> = QUALITY_TIERS.iter().map(|t| t.view_samples()).collect();
        assert_eq!(samples, vec![0, 2, 4, 4]);
        let levels: Vec<Option<usize>> = QUALITY_TIERS.iter().map(|t| t.bloom_levels()).collect();
        assert_eq!(
            levels,
            vec![
                Some(LEVELS_LOW),
                Some(LEVELS_LOW),
                Some(LEVELS_HIGH),
                Some(LEVELS_HIGH)
            ]
        );
        // The bloom gate is `q.bloom`, and every shipped preset sets it.
        assert!(QUALITY_PRESETS.iter().all(|p| p.bloom));
    }

    /// An unrecognised `?quality=` falls through to ultra rather than throwing,
    /// which is `?? 3`.
    #[test]
    fn an_unknown_quality_name_resolves_to_the_default_tier() {
        assert_eq!(QualityTier::from_name("low"), QualityTier::Low);
        assert_eq!(QualityTier::from_name("medium"), QualityTier::Medium);
        assert_eq!(QualityTier::from_name("high"), QualityTier::High);
        assert_eq!(QualityTier::from_name("ultra"), QualityTier::Ultra);
        assert_eq!(QualityTier::from_name("potato"), DEFAULT_QUALITY);
        assert_eq!(QualityTier::from_name(""), DEFAULT_QUALITY);
    }

    /// The render scale is the tier's most visible number and the one every
    /// target size is derived from.
    #[test]
    fn the_render_scale_ladder_is_the_source_table() {
        let scales: Vec<f64> = QUALITY_TIERS.iter().map(|t| t.preset().render_scale).collect();
        assert_eq!(scales, vec![0.72, 0.85, 1.0, 1.0]);
        // Anisotropy is the only other preset field `RenderSystem` itself reads.
        let aniso: Vec<u32> = QUALITY_TIERS.iter().map(|t| t.preset().anisotropy).collect();
        assert_eq!(aniso, vec![4, 8, 16, 16]);
    }

    /// The three fields no render pass reads, carried so the preset table stays
    /// one datum with one owner.
    #[test]
    fn the_budgets_the_renderer_never_reads_are_still_the_source_values() {
        let particles: Vec<u32> = QUALITY_TIERS
            .iter()
            .map(|t| t.preset().particle_budget)
            .collect();
        assert_eq!(particles, vec![2000, 6000, 12000, 24000]);
        let decals: Vec<u32> = QUALITY_TIERS.iter().map(|t| t.preset().decal_budget).collect();
        assert_eq!(decals, vec![64, 128, 256, 512]);
        let vol: Vec<bool> = QUALITY_TIERS.iter().map(|t| t.preset().volumetrics).collect();
        assert_eq!(vol, vec![false, true, true, true]);
    }
}
