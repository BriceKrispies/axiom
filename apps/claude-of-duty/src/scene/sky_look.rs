//! **The sky, as far as it reaches the frame.**
//!
//! `crate::sky` is the whole physical atmosphere model — the Rayleigh/Mie
//! raymarch, the three LUT bakes, the sun/moon ephemeris — ported as a CPU
//! reference because the source bakes them as WebGL2 fragment shaders and this
//! port has no GPU/WGSL emission path (see [`crate::sky::luts`]'s module doc).
//!
//! So: the *dome* cannot be drawn. What can be reached without the unported GPU
//! path is everything the dome would also have driven from the CPU side —
//!
//! * the sun's world **direction**, from the ephemeris ([`Celestial`]);
//! * the sun's **colour**, from the same
//!   [`transmittance_to_space`] the shader uses, so the key light and the sky
//!   it hangs in cannot disagree (`sky/index.js:563-590`);
//! * the **clear colour**, by raymarching the real scattering integral for one
//!   view direction;
//! * a **hemisphere ambient**, by raymarching up and sampling the ground.
//!
//! Each of those is one `raymarch_sky` call against the two LUTs that *are*
//! bakeable on the CPU (transmittance and multiscatter), not a plausible
//! constant. The sky-view LUT itself is not baked: it is 384x192 raymarches for
//! a dome nothing draws, and the handful of directions this file needs are
//! cheaper computed directly.
//!
//! ## The one invented step: the display transform
//!
//! `raymarch_sky` returns radiance in scene units (`SCENE_LUX`-relative), which
//! the source's HDR pipeline exposes and tone-maps in the render frame graph —
//! unported. [`display`] is this file's own Reinhard-ish stand-in for it, so an
//! HDR radiance becomes a displayable clear colour. It is labelled as invented
//! because it is: it is not in the source, and a future render arm replaces it.

use axiom::prelude::{Color, Ratio, Vec3 as EngineVec3};

use crate::sky::atmosphere::{
    lut_uv, raymarch_sky, transmittance_to_space, Vec3, ATMO, SUN_ILLUMINANCE_TOP,
};
use crate::sky::celestial::{Celestial, SITE};
use crate::sky::luts::{
    bake_multiscatter, bake_transmittance, MULTISCATTER_SIZE, MULTISCATTER_SQRT_SAMPLES,
    MULTISCATTER_STEPS, TRANSMITTANCE_HEIGHT, TRANSMITTANCE_STEPS, TRANSMITTANCE_WIDTH,
};

/// Hour of day the level is lit at. Mid-morning: the sun is high enough that
/// the street is genuinely lit and low enough that the geometry has direction
/// to it. `weather.turbidity` defaults to 1 in the source (`sky/index.js`), so
/// the Mie scale is 1.
pub const HOUR: f64 = 9.5;

/// `weather.turbidity`, the Mie density scale.
pub const MIE_SCALE: f64 = 1.0;

/// `tint` — the solar spectrum is a touch warm of D65 even before the
/// atmosphere (`sky/index.js:566`).
const SUN_TINT: [f64; 3] = [1.0, 0.975, 0.94];

/// The raymarch step count for a single direction. The sky-view LUT bake uses
/// `SKYVIEW_STEPS` (40) per texel; this file marches the same integral, so it
/// uses the same figure.
const DIRECTION_STEPS: u32 = 40;

/// What the sky contributes to a frame.
pub struct SkyLook {
    /// Unit world direction **pointing at** the sun.
    pub sun_direction: EngineVec3,
    /// The sun's normalised colour, from the atmospheric transmittance.
    pub sun_color: Color,
    /// The sun's relative intensity, 0 below the horizon.
    pub sun_intensity: f32,
    /// The clear colour: the sky's own radiance, displayed.
    pub clear_color: Color,
    /// Hemisphere ambient — sky above, bounce below.
    pub ambient_sky: Color,
    pub ambient_ground: Color,
    /// The sun's altitude, radians above the horizon — reported so a caller can
    /// see which side of the terminator the level is on.
    pub sun_altitude: f64,
}

/// Bake the two CPU-reachable LUTs and resolve the frame's sky.
///
/// This is the expensive call (the transmittance and multiscatter bakes) and it
/// runs exactly once, at level build.
pub fn resolve(hour: f64) -> SkyLook {
    let transmittance = bake_transmittance(
        TRANSMITTANCE_WIDTH,
        TRANSMITTANCE_HEIGHT,
        TRANSMITTANCE_STEPS,
        MIE_SCALE,
    );
    let multiscatter = bake_multiscatter(
        MULTISCATTER_SIZE,
        MULTISCATTER_STEPS,
        MULTISCATTER_SQRT_SAMPLES,
        MIE_SCALE,
        &transmittance,
    );

    let mut celestial = Celestial::new(SITE);
    celestial.set_hour(hour);
    let sun = celestial.sun;
    let moon = celestial.moon;

    // ---- the key light -----------------------------------------------------
    // `sky/index.js:560-590`, minus the aureole exponent and the beam floor
    // (both are presentation corrections applied to the *renderer's*
    // DirectionalLight intensity, and this port's engine light takes a
    // normalised colour plus a 0..1 intensity).
    let mu_s = celestial.sun_alt.sin();
    let t = transmittance_to_space(mu_s.max(0.0008), MIE_SCALE);
    let sr = t[0] * SUN_TINT[0];
    let sg = t[1] * SUN_TINT[1];
    let sb = t[2] * SUN_TINT[2];
    let smax = sr.max(sg).max(sb).max(1e-6);
    // Fraction of the solar disc above the horizon (`sky/index.js:562`).
    let disc = (0.5 + mu_s / (2.0 * 0.004654)).clamp(0.0, 1.0);

    // ---- the sky -----------------------------------------------------------
    let view_pos = Vec3::new(0.0, ATMO.ground_radius_mm + ATMO.view_altitude_mm, 0.0);
    let sun_irr = Vec3::new(
        SUN_ILLUMINANCE_TOP * SUN_TINT[0],
        SUN_ILLUMINANCE_TOP * SUN_TINT[1],
        SUN_ILLUMINANCE_TOP * SUN_TINT[2],
    );
    // The moon's extraterrestrial irradiance, same shape as the sun's
    // (`sky/index.js:640-660`); at mid-morning it contributes nothing, but it
    // is the same call the sky-view bake makes and costs nothing to keep.
    let moon_irr = Vec3::splat(0.0);

    let radiance = |dir: Vec3| -> Vec3 {
        raymarch_sky(
            view_pos,
            dir,
            sun,
            sun_irr,
            moon,
            moon_irr,
            DIRECTION_STEPS,
            MIE_SCALE,
            |p, d| {
                let (u, v) = lut_uv(p, d);
                transmittance.sample(u, v)
            },
            |p, d| {
                let (u, v) = lut_uv(p, d);
                multiscatter.sample(u, v)
            },
        )
    };

    // The clear colour is the sky a level-eye camera actually looks into: 12
    // degrees above the horizon, on the sun's azimuth's opposite side, which is
    // the band that fills most of a first-person frame.
    let horizon_az = Vec3::new(-sun.x, 0.0, -sun.z).normalize();
    let clear_dir = Vec3::new(
        horizon_az.x * 0.978,
        0.208, // sin(12 deg)
        horizon_az.z * 0.978,
    )
    .normalize();
    let clear = radiance(clear_dir);

    // Hemisphere ambient: straight up is the sky term; the down term is the
    // same sky reflected off a dry-earth albedo, which is what a street is.
    let up = radiance(Vec3::new(0.0, 1.0, 0.0));
    let ground_albedo = Vec3::new(0.30, 0.26, 0.21);

    SkyLook {
        sun_direction: EngineVec3::new(sun.x as f32, sun.y as f32, sun.z as f32),
        sun_color: normalized_color(sr / smax, sg / smax, sb / smax),
        sun_intensity: (disc * smax.min(1.0)) as f32,
        clear_color: display(clear),
        ambient_sky: display(up),
        ambient_ground: display(up.mul(ground_albedo)),
        sun_altitude: celestial.sun_alt,
    }
}

/// **Invented, and labelled as such** — see the module doc comment. The
/// source's HDR radiance reaches the frame through an exposure and an ACES
/// tone-map in the render graph, which is not ported. This is a plain Reinhard
/// at a fixed exposure: monotone, hue-preserving per channel, and enough to
/// turn a scene-unit radiance into a colour that can be a clear colour.
pub fn display(radiance: Vec3) -> Color {
    const EXPOSURE: f64 = 1.0;
    let map = |v: f64| {
        let x = (v * EXPOSURE).max(0.0);
        x / (1.0 + x)
    };
    normalized_color(map(radiance.x), map(radiance.y), map(radiance.z))
}

/// A [`Color`] from three already-normalised channels, guarded so a NaN out of
/// the raymarch can never reach the renderer as an unwrap panic deep in a
/// frame.
fn normalized_color(r: f64, g: f64, b: f64) -> Color {
    let ch = |v: f64| Ratio::new(v.clamp(0.0, 1.0) as f32).unwrap_or(Ratio::finite_or_zero(0.0));
    Color::linear_rgb(ch(r), ch(g), ch(b))
}

impl SkyLook {
    /// The engine's hemisphere ambient wants a single fill colour; this is the
    /// sky and ground terms averaged, which is what an unlit face sees.
    pub fn ambient_fill(&self) -> Color {
        let sky = self.ambient_sky.to_array();
        let ground = self.ambient_ground.to_array();
        normalized_color(
            f64::from(sky[0] + ground[0]) * 0.5,
            f64::from(sky[1] + ground[1]) * 0.5,
            f64::from(sky[2] + ground[2]) * 0.5,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bakes are the expensive part, so the whole module is exercised
    /// through one resolve per hour of interest.
    fn morning() -> SkyLook {
        resolve(HOUR)
    }

    #[test]
    fn the_mid_morning_sun_is_up_and_warm_white() {
        let sky = morning();
        assert!(
            sky.sun_altitude > 0.3,
            "the sun is well above the horizon, alt = {}",
            sky.sun_altitude
        );
        assert!(sky.sun_direction.y > 0.0, "and points upward");
        assert!(sky.sun_intensity > 0.5, "got {}", sky.sun_intensity);
        // Atmospheric extinction removes blue first, so a daytime sun is red >=
        // green >= blue after normalisation.
        let c = sky.sun_color.to_array();
        assert!(c[0] >= c[1]);
        assert!(c[1] >= c[2]);
        assert!((c[0] - 1.0).abs() < 1e-6, "normalised to its max");
    }

    #[test]
    fn the_clear_colour_is_a_daylight_blue_not_black() {
        let sky = morning();
        let c = sky.clear_color.to_array();
        assert!(c[2] > c[0], "the sky is blue: r = {}, b = {}", c[0], c[2]);
        assert!(c[2] > 0.1, "and bright enough to see, b = {}", c[2]);
        assert!(c[2] <= 1.0);
    }

    #[test]
    fn the_ground_ambient_is_warmer_and_darker_than_the_sky_ambient() {
        let sky = morning();
        let ground = sky.ambient_ground.to_array();
        let up = sky.ambient_sky.to_array();
        assert!(ground[2] < up[2]);
        let fill = sky.ambient_fill().to_array();
        assert!(fill[2] <= up[2]);
        assert!(fill[2] >= ground[2]);
    }

    #[test]
    fn midnight_puts_the_sun_below_the_horizon_and_the_key_out() {
        let sky = resolve(0.0);
        assert!(sky.sun_altitude < 0.0);
        assert_eq!(sky.sun_intensity, 0.0, "the disc is fully set");
    }

    #[test]
    fn the_display_transform_is_monotone_and_bounded() {
        let dark = display(Vec3::splat(0.01)).to_array();
        let bright = display(Vec3::splat(100.0)).to_array();
        assert!(dark[0] < bright[0]);
        assert!(bright[0] < 1.0, "Reinhard never reaches one");
        assert_eq!(display(Vec3::splat(0.0)).to_array()[0], 0.0);
        // Negative radiance (impossible physically, but a raymarch can undershoot)
        // clamps rather than panicking.
        assert_eq!(display(Vec3::splat(-5.0)).to_array()[0], 0.0);
    }

    #[test]
    fn a_non_finite_channel_becomes_zero_rather_than_a_panic() {
        assert_eq!(normalized_color(f64::NAN, 0.5, 0.5).to_array()[0], 0.0);
    }
}
