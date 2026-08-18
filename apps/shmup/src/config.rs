//! Central tuning + quality configuration.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/core/config.js:1-105` — the whole file.
//!
//! Subsystems read from here rather than hardcoding magic numbers, so the
//! quality scaler and the capture harness can drive everything from one place.
//!
//! This is data, and it ports as data. The one shape change is that JS's
//! `createConfig({ ...overrides })` — a spread over a plain object — becomes
//! [`Config::default`] plus field assignment: Rust has no object spread, and a
//! struct with public fields says the same thing with the field names checked.
//!
//! Where a constant carries a unit, it is typed with the kernel quantity for
//! that unit (`Seconds`, `Meters`, `Ratio`) so a subsystem cannot receive
//! "0.72" and have to guess whether that is a scale, a distance or a duration.
//! The raw literals stay visible next to them, in the source's own order and
//! spelling, so this file diffs against `config.js` by eye.

use axiom_kernel::{Meters, Ratio, Seconds};

use crate::error::CoreError;

/// Fixed simulation rate, in hertz.
pub const PHYSICS_HZ: u32 = 120;

/// The fixed step, in seconds. `1 / PHYSICS_HZ`.
///
/// `f64` because the frame accumulator it drives is `f64` (a JS number is an
/// `f64`, and the accumulator's job is to not drift); [`FIXED_STEP`] is the same
/// quantity at the subsystem boundary.
pub const FIXED_DT: f64 = 1.0 / PHYSICS_HZ as f64;

/// [`FIXED_DT`] as the dimensioned duration handed to `fixed_update`.
pub const FIXED_STEP: Seconds = Seconds::finite_or_zero(1.0 / PHYSICS_HZ as f32);

/// Never simulate more than this many physics steps in one frame
/// (spiral-of-death guard).
pub const MAX_SUBSTEPS: u32 = 8;

/// Real-world units are metres, seconds, kilograms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Units {
    /// Metres per second squared. Games use exaggerated gravity; CoD-like feel.
    /// A rate, not a length — so it stays a plain scalar rather than borrowing
    /// a kernel quantity that would misname it.
    pub gravity: f32,
    pub player_height: Meters,
    pub player_crouch_height: Meters,
    pub player_radius: Meters,
    /// Below the top of the capsule.
    pub eye_offset: Meters,
}

/// The source's `UNITS` block, value for value.
pub const UNITS: Units = Units {
    gravity: -9.81 * 2.1,
    player_height: Meters::finite_or_zero(1.78),
    player_crouch_height: Meters::finite_or_zero(1.12),
    player_radius: Meters::finite_or_zero(0.32),
    eye_offset: Meters::finite_or_zero(0.12),
};

/// One row of `QUALITY_PRESETS`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityPreset {
    pub render_scale: Ratio,
    pub shadow_map_size: u32,
    pub cascades: u32,
    pub shadow_distance: Meters,
    pub taa: bool,
    pub gtao: bool,
    pub ssr: bool,
    pub volumetrics: bool,
    pub motion_blur: bool,
    pub bloom: bool,
    pub anisotropy: u32,
    pub particle_budget: u32,
    pub decal_budget: u32,
}

/// The four presets. In JS these are keys of one object and the name is a
/// string; here the name is a type, so `setQuality("uhltra")` cannot compile in
/// the first place and only the string-fed path (a saved setting, a URL
/// parameter) has to be fallible — see [`Quality::from_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quality {
    Low,
    Medium,
    High,
    Ultra,
}

/// `QUALITY_PRESETS.low`.
pub const LOW: QualityPreset = QualityPreset {
    render_scale: Ratio::finite_or_zero(0.72),
    shadow_map_size: 1024,
    cascades: 3,
    shadow_distance: Meters::finite_or_zero(60.0),
    taa: false,
    gtao: false,
    ssr: false,
    volumetrics: false,
    motion_blur: false,
    bloom: true,
    anisotropy: 4,
    particle_budget: 2000,
    decal_budget: 64,
};

/// `QUALITY_PRESETS.medium`.
pub const MEDIUM: QualityPreset = QualityPreset {
    render_scale: Ratio::finite_or_zero(0.85),
    shadow_map_size: 2048,
    cascades: 3,
    shadow_distance: Meters::finite_or_zero(90.0),
    taa: true,
    gtao: true,
    ssr: false,
    volumetrics: true,
    motion_blur: true,
    bloom: true,
    anisotropy: 8,
    particle_budget: 6000,
    decal_budget: 128,
};

/// `QUALITY_PRESETS.high`.
pub const HIGH: QualityPreset = QualityPreset {
    render_scale: Ratio::finite_or_zero(1.0),
    shadow_map_size: 2048,
    cascades: 4,
    shadow_distance: Meters::finite_or_zero(140.0),
    taa: true,
    gtao: true,
    ssr: true,
    volumetrics: true,
    motion_blur: true,
    bloom: true,
    anisotropy: 16,
    particle_budget: 12000,
    decal_budget: 256,
};

/// `QUALITY_PRESETS.ultra`.
pub const ULTRA: QualityPreset = QualityPreset {
    render_scale: Ratio::finite_or_zero(1.0),
    shadow_map_size: 4096,
    cascades: 4,
    shadow_distance: Meters::finite_or_zero(200.0),
    taa: true,
    gtao: true,
    ssr: true,
    volumetrics: true,
    motion_blur: true,
    bloom: true,
    anisotropy: 16,
    particle_budget: 24000,
    decal_budget: 512,
};

impl Quality {
    /// Every preset, in the source's declaration order.
    pub const ALL: [Quality; 4] = [
        Quality::Low,
        Quality::Medium,
        Quality::High,
        Quality::Ultra,
    ];

    /// The preset row.
    pub fn preset(self) -> QualityPreset {
        match self {
            Quality::Low => LOW,
            Quality::Medium => MEDIUM,
            Quality::High => HIGH,
            Quality::Ultra => ULTRA,
        }
    }

    /// The key this preset has in the source's `QUALITY_PRESETS` object — the
    /// spelling a saved setting or a URL parameter uses.
    pub fn name(self) -> &'static str {
        match self {
            Quality::Low => "low",
            Quality::Medium => "medium",
            Quality::High => "high",
            Quality::Ultra => "ultra",
        }
    }

    /// The inverse of [`Quality::name`]. `setQuality`'s
    /// `unknown quality preset "…"` throw, as a returned error.
    pub fn from_name(name: &str) -> Result<Quality, CoreError> {
        Quality::ALL
            .into_iter()
            .find(|q| q.name() == name)
            .ok_or_else(|| CoreError::new(format!("unknown quality preset \"{name}\"")))
    }
}

/// The source's `DEFAULTS`, plus the live `q` preset copy `createConfig` builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub quality: Quality,
    /// Horizontal-ish vertical FOV, in **degrees**. CoD default feel.
    ///
    /// Degrees, not `Radians`, because that is what the source authors and what
    /// a settings slider shows; the conversion belongs at the camera, which is
    /// the one consumer that needs radians.
    pub fov: f32,
    pub ads_fov_scale: Ratio,
    /// Radians of yaw per pixel of raw pointer movement. A rate with two units,
    /// so no single kernel quantity names it.
    pub sensitivity: f32,
    pub ads_sens_scale: Ratio,
    pub invert_y: bool,
    pub exposure: Ratio,
    /// Capture mode disables anything nondeterministic so screenshots are
    /// stable.
    pub deterministic: bool,
    /// The live copy of the active preset. The source copies rather than
    /// aliases (`cfg.q = { ...QUALITY_PRESETS[cfg.quality] }`) precisely so the
    /// runtime quality scaler can nudge one knob without editing the preset
    /// table itself; `QualityPreset` is `Copy`, so the port gets that for free.
    pub q: QualityPreset,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            quality: Quality::Ultra,
            fov: 80.0,
            ads_fov_scale: Ratio::finite_or_zero(0.72),
            sensitivity: 0.0022,
            ads_sens_scale: Ratio::finite_or_zero(0.65),
            invert_y: false,
            exposure: Ratio::finite_or_zero(1.0),
            deterministic: false,
            q: Quality::Ultra.preset(),
        }
    }
}

impl Config {
    /// `createConfig({ quality })` — defaults with one preset selected.
    pub fn with_quality(quality: Quality) -> Self {
        let mut cfg = Config::default();
        cfg.set_quality(quality);
        cfg
    }

    /// `cfg.setQuality(name)`. Replaces the live preset copy wholesale, so any
    /// knob the quality scaler had nudged returns to the preset's value.
    pub fn set_quality(&mut self, quality: Quality) {
        self.quality = quality;
        self.q = quality.preset();
    }
}
