//! **The visible sky** — the composition step that turns the ported
//! [`crate::sky::system::SkySystem`] into the engine's own sky pass
//! ([`axiom_host::FrameSky`]), so the frame has a dome, a sun and a cloud layer
//! in it instead of a flat clear colour.
//!
//! This is the *display* half of the sky. The *lighting* half already exists and
//! is untouched: [`crate::scene::wiring::look::SkyDriver`] publishes
//! `key_light`/`ambient`/`depth_fog`/`clear_color`, and this module reads that
//! same driver rather than standing up a second sky model. Nothing here
//! re-derives a celestial quantity, a radiance or a weather value — every number
//! below is either measured off `SkyDriver`'s existing raymarch or read straight
//! out of `SkySystem`'s published state.
//!
//! # Why this is `FrameSky` and not a dome mesh
//!
//! The port carries a complete dome shader ([`crate::sky::dome::sample`]: sky-view
//! LUT, two aureoles, two cloud decks, the night sky, ground bounce, horizon murk,
//! roll-off, two discs). The engine carries its own sky pass. **They are mutually
//! exclusive** — a dome drawn as app geometry occludes the engine's sky pass, and
//! vice versa — so this slice had to pick one, and it picked the engine's:
//!
//! * `axiom_host::frame_sky`'s own module doc makes the argument, and it is about
//!   this exact case: cloud (and by extension the whole sky) "belongs here, in the
//!   sky's own evaluation, and not in the app as billboard cards", because
//!   app-tier sky geometry "would survive on a backend that has already declared
//!   it drops `RenderCapability::Sky`, which is precisely the silent divergence
//!   the capability system exists to prevent."
//! * A dome mesh would be lit, depth-fogged and far-plane-clipped like any other
//!   geometry. The engine's sky is none of those.
//! * The port's dome is a **CPU reference implementation of a fragment shader**.
//!   Drawing it as geometry means baking an equirect texture: 384x192 raymarches
//!   for the sky-view LUT it needs, then one [`crate::sky::dome::sample`] per
//!   texel — each of which runs a six-octave cumulus fbm, a two-octave cirrus
//!   ridge, a five-octave Milky Way and two disc evaluations. That is a
//!   multi-second freeze at startup in wasm, to reproduce on the CPU what the
//!   engine's sky pass already does per pixel on the GPU.
//!
//! So: the engine's sky pass draws the sky, and this module's job is to **author
//! it from the port's real numbers** rather than from hand-picked constants.
//!
//! # What crosses the boundary, and what does not
//!
//! `FrameSky` is a two-stop vertical gradient + one celestial body (disc + halo)
//! + one procedural cloud layer. The port's sky is richer, and the difference is
//! named rather than quietly dropped:
//!
//! | the port has | `FrameSky` | verdict |
//! |---|---|---|
//! | 2D sky-view LUT (azimuth x altitude) | azimuth-invariant gradient | the azimuthal term is lost; the two stops are **measured** off the port's own raymarch along the band the camera actually sees |
//! | sun **and** moon discs, both drawn | one body | the *key* body only ([`crate::sky::system::SkySystem::key_light`]); the other is dropped |
//! | [`crate::sky::dome::moon_disc`]: gnomonic maria, terminator, earthshine | a limb-softened uniform disc | the moon's phase and surface are lost |
//! | two aureoles, Mie-derived | `cos^falloff * strength` | **fitted** to the port's own [`crate::sky::dome::aureole`] at two probe angles |
//! | cumulus deck (1.5 km) + cirrus deck (7.8 km), wind-advected | one static field | cumulus coverage carries over and its scale is derived; **cirrus has no home, and the wind does not move** |
//! | [`crate::sky::stars`]: star layers, blackbody tint, twinkle, Milky Way | nothing | **no engine counterpart** — see [`self`]'s "the star field" note |
//! | [`crate::sky::volumetrics`]: half-res fog raymarch, CSM shafts, temporal resolve | nothing reachable | **no engine counterpart** — see "volumetrics" below |
//!
//! ## The star field
//!
//! `FrameSky` has no star term, and there is no other engine seam that accepts
//! one: `Material`'s emissive is a flat `Color` (not modulated by a map), so even
//! a night-sky dome mesh could not carry a star texture without an
//! `axiom_surface::LightingModel::Unlit` surface plus a baked equirect albedo —
//! i.e. the dome-mesh path this module already rejected, taken for the one layer
//! that is invisible at this level's authored hour anyway
//! ([`crate::scene::wiring::look::HOUR`] is 16.5, the source's own). The honest fix is
//! an engine one — a star/night term on `FrameSky`, evaluated by the sky pass
//! alongside the cloud layer it already carries — and that is a new engine
//! capability, which this wave does not add. `crate::sky::stars` therefore stays
//! unreferenced, and that is reported rather than papered over.
//!
//! ## Volumetrics
//!
//! `axiom_host::FrameVolumetrics` exists — a screen-space god-ray post-pass, with
//! `axiom_host::RenderCapability::Volumetrics` gating it — but **no app-tier
//! setter reaches it**: `FramePacket::with_volumetrics` is host-internal, and
//! neither `axiom::RunningApp` nor `axiom_windowing::WindowingApi` exposes a
//! `set_volumetrics`, the way both expose `set_sky`, `set_bloom` and
//! `set_depth_fog`. Even with one, it is a radial screen-space blur around a light
//! position; the port's `crate::sky::volumetrics` is a half-resolution raymarch
//! through a height-fogged, phase-functioned medium, sampling a cascaded shadow
//! atlas per step and resolved temporally against a history buffer. The engine has
//! no depth-buffer or shadow-atlas seam an app can march. **That is the boundary,
//! and this slice stops at it** rather than approximating light shafts in the app
//! tier.
//!
//! # When it runs
//!
//! Once, at build, exactly like the ambient and the depth fog next to it. With the
//! source's default `time_rate == 0` the sun does not move, so
//! [`crate::scene::wiring::look::SkyDriver::frame`] re-derives the radiance on the
//! first frame and never again. The moment the clock does move, this wants
//! re-pushing on that same signal — and it hits the same wall the ambient and the
//! fog already do: `WindowingApi` is consumed by `run_web_multi`, so nothing
//! inside the frame closure can reach a setter on it. One restructuring fixes all
//! three.

use std::f64::consts::PI;

use axiom::prelude::{FrameSky, Ratio};
use axiom_kernel::Radians;

use crate::scene::wiring::look::{scene_radiance, SkyDriver};
use crate::sky::atmosphere::{luminance, lut_uv, raymarch_sky, Vec3};
use crate::sky::clouds::CUMULUS_KM;
use crate::sky::dome::aureole;
use crate::sky::luts::{Lut2D, SKYVIEW_STEPS};
use crate::sky::system::KeyLight;

/// The up-component the horizon stop is measured at: `sin(12 deg)`.
///
/// Not zero, deliberately. The horizon stop has to agree with two things the
/// frame already carries — the window's clear colour and the colour
/// [`crate::scene::wiring::look::SkyDriver::depth_fog`] fades distance into — and
/// both of those are `SkyRadiance::clear_color`, which is measured at 12 degrees
/// of elevation because that is the band that fills most of a first-person frame.
/// A horizon stop measured anywhere else would draw a seam along the horizon of
/// every outdoor shot, which is exactly the failure `axiom_host::FrameSky`'s own
/// doc warns the gradient's endpoints exist to avoid.
const HORIZON_UP: f64 = 0.208;

/// How many elevations the gradient's half-way point is searched over, between
/// [`HORIZON_UP`] and the zenith.
///
/// The search is a bisection-free scan because the underlying function is one
/// raymarch per probe and 24 of them is under a thousand integration steps —
/// cheaper than the two LUT bakes `SkyDriver::new` already pays by three orders
/// of magnitude.
const HAZE_PROBES: usize = 24;

/// The haze height to fall back on when the scan finds no crossing (a sky whose
/// zenith is not between its two stops — a degenerate atmosphere, or a night in
/// which both ends are near zero).
///
/// `0.5` is `axiom_host::FrameSky`'s own identity value: the gradient it
/// evaluated before the parameter existed. A fallback that changes the sky is a
/// fallback that hides its own trigger.
const HAZE_FALLBACK: f64 = 0.5;

/// The two angles, in radians, at which the port's aureole is probed to fit the
/// engine's halo. Inside [`crate::sky::dome`]'s own `AUREOLE_CUT` (24 degrees),
/// and far enough apart that the ratio between them is a real slope rather than
/// two samples of the same number.
const HALO_INNER: f64 = 2.0 * PI / 180.0;
const HALO_OUTER: f64 = 12.0 * PI / 180.0;

/// The cumulus density field's base frequency, per kilometre on the deck —
/// `fbm2(p.scale(1.25), oct)` in [`crate::sky::clouds::cumulus_density`]. `val2`
/// is value noise on a unit lattice, so one cell is `1 / 1.25` km across.
const CUMULUS_FBM_FREQUENCY: f64 = 1.25;

/// The spacing of one lobe of the engine's cloud field.
///
/// `axiom_host::FrameSky`'s field is a sum of separable sinusoids whose base
/// octave is `sin(x) * sin(y)`, remapped to `0..1`. That is a checkerboard of
/// lobes on a `PI` pitch — the full `2*PI` period contains two of them, so the
/// *feature* spacing, which is what a cloud's size is, is `PI`.
const ENGINE_CLOUD_LOBE: f64 = PI;

/// The engine's cloud scale that puts its lobes at the same apparent size as the
/// port's cumulus.
///
/// The engine samples its field on a plane **one unit** overhead, at
/// `dir.xz / dir.y * scale`; the port samples its cumulus on a real deck
/// [`CUMULUS_KM`] up, at `dir.xz / dir.y * CUMULUS_KM` kilometres. So one port
/// cell subtends `1 / (CUMULUS_FBM_FREQUENCY * CUMULUS_KM)` in the shared
/// tangent coordinate and one engine lobe subtends `ENGINE_CLOUD_LOBE / scale`;
/// equating them gives this.
///
/// It matches feature *spacing* and nothing else — the port's field is a
/// domain-warped value-noise fbm eroded by a coverage threshold and cauliflowered
/// by a ridge, and the engine's is four sinusoids. The shapes are not the same
/// shape and no single number could make them so. What this buys is that the
/// clouds are the size the port's weather says they are, instead of the size
/// another app's palette happened to pick.
const CLOUD_SCALE: f64 = ENGINE_CLOUD_LOBE * CUMULUS_KM * CUMULUS_FBM_FREQUENCY;

/// **The visible sky**, authored from the ported sky.
///
/// Reads the driver the lighting path already owns — its raymarched radiance for
/// the two gradient stops, its `SkySystem` for the body and the weather, and its
/// two baked LUTs for the halo fit — and returns the frame's
/// [`axiom_host::FrameSky`]. Costs about 26 short raymarches; the expensive LUT
/// bakes belong to `SkyDriver::new` and are not repeated here.
pub fn visible_sky(driver: &SkyDriver) -> FrameSky {
    let system = &driver.system;
    let (transmittance, multiscatter) = driver.luts();
    let shared = system.shared;
    let sun = system.sun_direction();
    let moon = system.moon_direction();

    // The same integral `SkyDriver`'s own `raymarch` runs, against the same two
    // LUTs and the same published irradiances — not a second sky model. The step
    // count is the sky-view bake's own, which is where the driver's 40 came from.
    let radiance = |dir: Vec3| -> Vec3 {
        raymarch_sky(
            shared.view_pos,
            dir,
            sun,
            shared.sun_irradiance,
            moon,
            shared.moon_irradiance,
            SKYVIEW_STEPS,
            shared.mie_scale,
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

    // ---- the two stops -------------------------------------------------------
    // Both are already on the driver: `ambient_sky` IS the displayed radiance
    // straight up, and `clear_color` IS the displayed radiance in the 12-degree
    // band. Recomputing either here would be the fifth hand-inlined duplicate
    // this wave exists to remove.
    let zenith = rgb(driver.radiance().ambient_sky.to_array());
    let horizon = rgb(driver.radiance().clear_color.to_array());

    let column = probe_column(sun);
    let haze = haze_height(&radiance, column, luma(horizon), luma(zenith));

    // ---- the body ------------------------------------------------------------
    let moon_key = system.key_light == KeyLight::Moon;
    let body_dir = [sun, moon][usize::from(moon_key)];
    let body_irradiance = [shared.sun_irradiance, shared.moon_irradiance][usize::from(moon_key)];
    let disc_radiance = [shared.sun_disc_radiance, shared.moon_disc_radiance][usize::from(moon_key)];
    // `uDisc` packs (sun radius, moon radius, sun draw scale, moon draw scale);
    // `dome::sun_disc`'s drawn limb is `ang_radius * draw_scale` and its radiance
    // is divided by `draw_scale^2` so enlarging the disc adds no energy. Both
    // halves of that carry over — the engine's disc is the drawn one.
    let angular_radius = shared.disc[usize::from(moon_key)] * shared.disc[2 + usize::from(moon_key)];
    let draw_scale = shared.disc[2 + usize::from(moon_key)];
    let (tu, tv) = lut_uv(shared.view_pos, body_dir);
    let to_body = transmittance.sample(tu, tv);
    let body_linear = disc_radiance
        .mul(to_body)
        .div(Vec3::splat(draw_scale * draw_scale));
    // Scene-referred and deliberately unbounded: the sun's disc radiance is a
    // four-figure linear value, and `FrameSky`'s body colour is a raw `[f32; 3]`
    // for exactly that reason (its own tests author a sun at `[3.0, 2.8, 2.4]`).
    // While this went through a Reinhard the disc was flattened to 1.0 — display
    // white — which is also what made `halo_fit` below meaningless.
    let body_color = rgb(scene_radiance(body_linear).to_array());

    // ---- the halo ------------------------------------------------------------
    let (halo_falloff, halo_strength) = halo_fit(
        &radiance,
        transmittance,
        shared.view_pos,
        body_dir,
        body_irradiance,
        shared.mie_scale,
        luma(body_color),
    );

    FrameSky::gradient(zenith, horizon)
        .with_haze_height(Ratio::finite_or_zero(haze as f32))
        .with_body(
            [body_dir.x as f32, body_dir.y as f32, body_dir.z as f32],
            Radians::finite_or_zero(angular_radius as f32),
            body_color,
            Ratio::finite_or_zero(halo_falloff as f32),
            Ratio::finite_or_zero(halo_strength as f32),
        )
        // The cumulus deck's coverage, straight off the weather block. The two
        // fields differ but the parameter means the same thing in both: the port
        // scales its authored coverage by `0.34 + 1.30 * cloud_macro`, whose
        // macro field averages 0.5, so the *mean* effective coverage over the
        // deck is the authored number itself. `cirrus_coverage` has nowhere to go
        // — see the module doc.
        .with_clouds(
            Ratio::finite_or_zero(system.weather.cloud_coverage as f32),
            Ratio::finite_or_zero(CLOUD_SCALE as f32),
        )
}

/// The horizontal bearing the gradient is probed along: the anti-solar azimuth,
/// which is the bearing `SkyRadiance::clear_color` is measured on. Probing the
/// haze band on any other bearing would measure a different column from the one
/// the horizon stop came out of.
///
/// Falls back to `-Z` when the sun is within a whisker of the zenith and the
/// bearing is undefined — that is a degenerate azimuth, not a degenerate sky.
fn probe_column(sun: Vec3) -> Vec3 {
    let flat = Vec3::new(-sun.x, 0.0, -sun.z);
    let len = flat.length();
    [Vec3::new(0.0, 0.0, -1.0), flat.scale(1.0 / len.max(1.0e-9))][usize::from(len > 1.0e-6)]
}

/// Measure the up-component at which the sky's own gradient stands halfway
/// between the horizon stop and the zenith stop.
///
/// This is *exactly* `axiom_host::FrameSky::with_haze_height`'s parameter, and it
/// is exact rather than fitted: the engine reshapes the up-component with
/// `haze_lift(up, h) = up / (up + (1 - up) * k)`, `k = h / (1 - h)`, then
/// smoothsteps it. `smoothstep(t) = 0.5` only at `t = 0.5`, and `haze_lift` is
/// `0.5` only when `k = up / (1 - up)` — i.e. when `h == up`. So the half-way
/// elevation *is* the haze height, and measuring one gives the other with no
/// approximation in between.
///
/// Why it has to be measured at all: the engine's default puts the midpoint at 30
/// degrees of elevation. Real optical depth goes as `1 / sin(elevation)`, so a
/// clear morning collapses to its zenith colour far lower than that. The port
/// models the real thing; this is the one number that lets the engine's two-stop
/// dome carry the port's *shape* and not just its two colours.
fn haze_height(
    radiance: &impl Fn(Vec3) -> Vec3,
    column: Vec3,
    horizon_luma: f64,
    zenith_luma: f64,
) -> f64 {
    let target = 0.5 * (horizon_luma + zenith_luma);
    let up_at = |i: usize| HORIZON_UP + (1.0 - HORIZON_UP) * (i as f64) / (HAZE_PROBES as f64);
    let sample = |up: f64| {
        let flat = (1.0 - up * up).max(0.0).sqrt();
        let dir = Vec3::new(column.x * flat, up, column.z * flat);
        luma(rgb(scene_radiance(radiance(dir)).to_array()))
    };

    let mut prev_up = up_at(0);
    let mut prev_value = sample(prev_up);
    let side = (prev_value - target).signum();
    for i in 1..=HAZE_PROBES {
        let up = up_at(i);
        let value = sample(up);
        if (value - target).signum() != side {
            // Linear between the bracketing probes. The denominator cannot be
            // zero: the two samples sit on opposite sides of `target`.
            let t = (target - prev_value) / (value - prev_value);
            return (prev_up + (up - prev_up) * t).clamp(HORIZON_UP, 1.0);
        }
        prev_up = up;
        prev_value = value;
    }
    HAZE_FALLBACK
}

/// Fit the engine's `cos(theta)^falloff * strength` halo to the port's
/// [`crate::sky::dome::aureole`] at [`HALO_INNER`] and [`HALO_OUTER`].
///
/// Two probes, two unknowns, one closed-form solve — no search and no eyeballed
/// constant. The quantity fitted is the halo as the engine *composites* it: the
/// engine adds `body_color * halo` to the gradient, so each probe measures how
/// much luminance the port's aureole adds on top of the sky at that angle,
/// divided by the body's own luminance.
///
/// That ratio is now **exact**. While `scene_radiance` was a Reinhard this doc
/// argued the opposite — that the probes had to be taken in the composite's own
/// space because "the display transform is not additive" — and the argument was
/// sound about a curve and wrong about the frame: the curve also crushed the
/// body's own luminance to ~1.0, so `strength` came out as the aureole's
/// absolute luminance rather than as a fraction of the disc, three orders of
/// magnitude too large. The transform is a linear scale, so
/// `SCENE_RADIANCE_SCALE` cancels top and bottom and what is left is the
/// dimensionless ratio the engine's shader actually wants.
///
/// The port sums **two** aureoles, one per body. Only the key body's is fitted;
/// the other has nowhere to go, exactly as the other body's disc does not.
///
/// Returns `(1.0, 0.0)` — no halo — whenever the probes carry no usable slope: a
/// body below the horizon, an irradiance that has ramped to zero, or an outer
/// probe that is not dimmer than the inner one. A halo fitted through noise is
/// worse than no halo.
fn halo_fit(
    radiance: &impl Fn(Vec3) -> Vec3,
    transmittance: &Lut2D,
    view_pos: Vec3,
    body_dir: Vec3,
    irradiance: Vec3,
    mie_scale: f64,
    body_luma: f64,
) -> (f64, f64) {
    let probe = |theta: f64| -> f64 {
        let dir = tilt_toward_zenith(body_dir, theta);
        let base = radiance(dir);
        let (u, v) = lut_uv(view_pos, dir);
        let along = transmittance.sample(u, v);
        let glow = aureole(dir.y, irradiance, along, theta.cos(), mie_scale);
        let lit = luma(rgb(scene_radiance(base.add(glow)).to_array()));
        let plain = luma(rgb(scene_radiance(base).to_array()));
        (lit - plain).max(0.0) / body_luma.max(1.0e-6)
    };

    let inner = probe(HALO_INNER);
    let outer = probe(HALO_OUTER);
    let usable = inner > 1.0e-6 && outer > 1.0e-9 && inner > outer;
    if !usable {
        return (1.0, 0.0);
    }

    // inner = cos(HALO_INNER)^p * s and outer = cos(HALO_OUTER)^p * s, so the
    // ratio eliminates `s` and leaves one log divide for `p`.
    let falloff = (inner / outer).ln() / (HALO_INNER.cos() / HALO_OUTER.cos()).ln();
    // The engine evaluates `powf(halo_falloff.max(1.0))`, so a flatter fit than
    // that is unrepresentable and is clamped here rather than silently there.
    let falloff = falloff.max(1.0);
    let strength = inner / HALO_INNER.cos().powf(falloff);
    if !falloff.is_finite() || !strength.is_finite() {
        return (1.0, 0.0);
    }
    (falloff, strength)
}

/// A unit direction `theta` radians from `body` in the vertical plane through it.
///
/// The plane matters: the aureole's own strength depends on the ray's elevation
/// (`mie_od` divides by `ray_dir.y + 0.055`), so probing across the sky at
/// constant elevation would measure a different function from the one the halo
/// has to stand in for.
fn tilt_toward_zenith(body: Vec3, theta: f64) -> Vec3 {
    let up = Vec3::new(0.0, 1.0, 0.0);
    let along = up.sub(body.scale(body.dot(up)));
    let len = along.length();
    // A body at the zenith has no "toward the zenith"; any perpendicular does.
    let axis = [Vec3::new(1.0, 0.0, 0.0), along.scale(1.0 / len.max(1.0e-9))]
        [usize::from(len > 1.0e-6)];
    body.scale(theta.cos()).add(axis.scale(theta.sin())).normalize()
}

/// The RGB of an engine `Color`'s four-channel array. `FrameSky` takes three.
fn rgb(color: [f32; 4]) -> [f32; 3] {
    [color[0], color[1], color[2]]
}

/// Rec. 709 luminance of a displayed colour, through the port's own
/// [`crate::sky::atmosphere::luminance`] so the sky is never weighted two
/// different ways in one frame.
fn luma(color: [f32; 3]) -> f64 {
    luminance([
        f64::from(color[0]),
        f64::from(color[1]),
        f64::from(color[2]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Quality;
    use crate::scene::wiring::look::HOUR;

    /// The LUT bakes are the expensive part, so the sky is exercised through as
    /// few drivers as the assertions allow — the same posture `look`'s own tests
    /// take.
    fn authored_hour() -> SkyDriver {
        SkyDriver::new(Quality::High, HOUR)
    }

    #[test]
    fn the_dome_is_the_drivers_own_two_stops_and_not_a_second_sky() {
        let driver = authored_hour();
        let sky = visible_sky(&driver);
        assert_eq!(sky.zenith(), rgb(driver.radiance().ambient_sky.to_array()));
        assert_eq!(sky.horizon(), rgb(driver.radiance().clear_color.to_array()));
        // A daytime sky is darker overhead than at the horizon, where the path
        // through the air is longest.
        assert!(
            luma(sky.zenith()) < luma(sky.horizon()),
            "zenith {:?} horizon {:?}",
            sky.zenith(),
            sky.horizon()
        );
    }

    #[test]
    fn the_haze_band_is_measured_and_is_tighter_than_the_engine_default() {
        let sky = visible_sky(&authored_hour());
        let haze = sky.haze_height().get();
        assert!(haze >= HORIZON_UP as f32, "the scan starts at the horizon stop");
        assert!(haze <= 1.0, "and cannot exceed the zenith: {haze}");
        assert!(
            (haze - HAZE_FALLBACK as f32).abs() > 1.0e-6,
            "the band was measured off the port's own raymarch, not fallen back to \
             the engine's default midpoint"
        );
    }

    #[test]
    fn the_key_body_is_the_sun_with_its_drawn_radius_and_a_fitted_halo() {
        let driver = authored_hour();
        let sky = visible_sky(&driver);
        let sun = driver.system.sun_direction();
        let body = sky.body_direction();
        // Pointing AT the sun, which is where the port's ephemeris points.
        assert!((f64::from(body[1]) - sun.y).abs() < 1.0e-5, "{body:?}");
        assert!(body[1] > 0.0, "the sun is up at 16.5 hours");

        // The drawn limb, not the true one: `uDisc.x * uDisc.z`.
        let true_radius = driver.system.shared.disc[0];
        let drawn = sky.body_angular_radius().get();
        assert!(drawn > true_radius as f32, "the disc is drawn enlarged");
        assert!(drawn < 0.05, "and it is still a disc, not a smear: {drawn}");

        // The halo is fitted, so it is present and tight.
        assert!(sky.halo_strength().get() > 0.0, "a daytime sun has an aureole");
        assert!(
            sky.halo_falloff().get() > 1.0,
            "and the fit is a real slope, not the clamp: {}",
            sky.halo_falloff().get()
        );
    }

    #[test]
    fn the_cloud_layer_carries_the_weathers_coverage_and_the_decks_own_scale() {
        let driver = authored_hour();
        let sky = visible_sky(&driver);
        assert_eq!(
            sky.cloud_coverage().get(),
            driver.system.weather.cloud_coverage as f32
        );
        // Derived from the deck, not copied from another app: the port's cumulus
        // is a good deal busier than the engine's own 0.5 sweet spot.
        assert_eq!(sky.cloud_scale().get(), CLOUD_SCALE as f32);
        assert!(sky.cloud_scale().get() > 5.0);
    }

    #[test]
    fn midnight_hands_the_body_to_the_moon_and_drops_the_suns_halo() {
        let driver = SkyDriver::new(Quality::High, 0.0);
        let sky = visible_sky(&driver);
        let moon = driver.system.moon_direction();
        let body = sky.body_direction();
        assert!((f64::from(body[1]) - moon.y).abs() < 1.0e-5, "{body:?}");
        // Whatever the moon's aureole fits to, it must be finite and non-negative
        // — the guard in `halo_fit` exists so a dead beam cannot produce a NaN
        // deep in a frame.
        assert!(sky.halo_strength().get().is_finite());
        assert!(sky.halo_strength().get() >= 0.0);
        assert!(sky.halo_falloff().get() >= 1.0);
    }

    #[test]
    fn the_probe_column_is_the_anti_solar_bearing_and_survives_a_zenith_sun() {
        let column = probe_column(Vec3::new(0.6, 0.5, -0.6).normalize());
        assert!(column.x < 0.0 && column.z > 0.0, "opposite the sun");
        assert!((column.length() - 1.0).abs() < 1.0e-9);
        // Straight overhead there is no bearing to be opposite of.
        let degenerate = probe_column(Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(degenerate, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn a_tilt_stays_on_the_unit_sphere_and_opens_the_authored_angle() {
        let body = Vec3::new(0.3, 0.7, -0.2).normalize();
        let tilted = tilt_toward_zenith(body, HALO_OUTER);
        assert!((tilted.length() - 1.0).abs() < 1.0e-9);
        assert!((tilted.dot(body) - HALO_OUTER.cos()).abs() < 1.0e-9);
        // And it tilts UP, which is the half of the plane the aureole's elevation
        // term actually varies over.
        assert!(tilted.y > body.y);
        // A body at the zenith has no "toward the zenith"; the fallback axis
        // still produces a unit direction at the authored angle.
        let overhead = tilt_toward_zenith(Vec3::new(0.0, 1.0, 0.0), HALO_OUTER);
        assert!((overhead.length() - 1.0).abs() < 1.0e-9);
        assert!((overhead.y - HALO_OUTER.cos()).abs() < 1.0e-9);
    }
}
