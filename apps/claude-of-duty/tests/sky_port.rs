//! Golden captures for the ported `src/sky/` — pinned against the original
//! JavaScript (and, where no JavaScript oracle exists, against a
//! hand-transcription of the GLSL that a human can read side by side with
//! this file and `src/sky/{atmosphere,luts,noise}.rs`).
//!
//! `tests/sky/capture.mjs` produces `tests/sky/golden.json`. Two different
//! kinds of value live in it — see that script's module doc for the full
//! explanation:
//!
//! * **Genuine oracle** — `celestial.js` in full, and `atmosphere.js`'s CPU
//!   tail (`transmittanceToSpace`, `luminance`, the constants). The capture
//!   script imports and calls the *original* source directly.
//! * **No oracle** — every `*_FRAG`/`*_GLSL` shader body. There is no
//!   JavaScript form of these anywhere; they only ever ran on a browser GPU.
//!   The capture script hand-transcribes each one into plain JS, independent
//!   of (but line-referenced against, the same as) this crate's Rust
//!   transcription. Pinning against it catches drift between the Rust port
//!   and *a* careful reading of the GLSL — it cannot catch a mistake both
//!   transcriptions share.
//!
//! ## Tolerances
//!
//! * **Exact** — the literal `ATMO`/`SCENE_LUX`/... constants (no arithmetic
//!   at all on the Rust side, just literals copied from the same source
//!   literals).
//! * **[`REL`] (1e-9 relative)** — everything else. `sin`/`cos`/`exp`/`sqrt`/
//!   `pow`/`atan2` are not bit-guaranteed across V8 and Rust's libm, and the
//!   LUT bakes chain dozens of them per texel, so this is looser than
//!   `core_port.rs`'s `1e-12` for the RNG's Box-Muller draws — still an
//!   extremely tight pin (a real photometric-scale regression, e.g. a
//!   dropped or added `pi`, moves these numbers by 2-3 decimal *digits*, not
//!   in the ninth).
//! * **Order-of-magnitude sanity bounds** — the derived photometric-contract
//!   constants the source's own doc comment states as approximate (`~3.9`,
//!   `~0.06`). These guard against exactly the class of regression the
//!   photometric-contract note in `atmosphere.rs` describes (a stray `pi`
//!   putting the sky 1.65 stops over-bright) without pretending the source's
//!   own "~" figures are exact pins.

use std::sync::OnceLock;

use serde_json::Value;

use axiom_claude_of_duty::sky::atmosphere::{
    hg_phase, lut_uv, luminance, medium, mie_phase, ray_sphere, raymarch_sky, rayleigh_phase,
    transmittance_to_space, Vec3, ATMO, ISO_PHASE, MOON_ILLUMINANCE_NIGHT, SCENE_LUX,
    SUN_ILLUMINANCE_TOP,
};
use axiom_claude_of_duty::sky::celestial::{alt_az, dir_from_alt_az, solar_declination, Celestial, SITE};
use axiom_claude_of_duty::sky::luts::{
    bake_ambient, bake_multiscatter, bake_sky_view, bake_transmittance, sky_view_lookup,
    SkyViewParams,
};
use axiom_claude_of_duty::sky::noise::{fbm2, fbm3, hash12, hash13, hash33, ign, ridge2, val2, val3, Vec2 as NoiseVec2};

/// See the module doc: a real photometric-scale bug moves these numbers by
/// decimal digits, not in the ninth.
const REL: f64 = 1e-9;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("sky/golden.json")).expect("golden.json parses"))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn vec3_of(v: &Value) -> Vec3 {
    Vec3::new(num(&v[0]), num(&v[1]), num(&v[2]))
}

fn close(actual: f64, expected: f64, rel: f64, what: &str) {
    if actual == expected {
        return;
    }
    let scale = expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= rel * scale,
        "{what}: expected {expected:.17e}, got {actual:.17e} (rel {:.3e})",
        (actual - expected).abs() / scale
    );
}

fn close3(actual: Vec3, expected: Vec3, rel: f64, what: &str) {
    close(actual.x, expected.x, rel, &format!("{what}.x"));
    close(actual.y, expected.y, rel, &format!("{what}.y"));
    close(actual.z, expected.z, rel, &format!("{what}.z"));
}

/* ==================================================================== */
/* Constants — exact, literal-for-literal.                              */
/* ==================================================================== */

#[test]
fn photometric_and_media_constants_match_the_javascript() {
    let g = golden();
    let c = &g["constants"];
    assert_eq!(SCENE_LUX, num(&c["sceneLux"]));
    assert_eq!(SUN_ILLUMINANCE_TOP, num(&c["sunIlluminanceTop"]));
    assert_eq!(SUN_ILLUMINANCE_TOP, 5.12, "128000 / 25000");
    assert_eq!(MOON_ILLUMINANCE_NIGHT, num(&c["moonIlluminanceNight"]));
    assert_eq!(ISO_PHASE, num(&c["isoPhase"]));

    let atmo = &c["atmo"];
    assert_eq!(ATMO.ground_radius_mm, num(&atmo["groundRadiusMM"]));
    assert_eq!(ATMO.atmosphere_radius_mm, num(&atmo["atmosphereRadiusMM"]));
    assert_eq!(ATMO.view_altitude_mm, num(&atmo["viewAltitudeMM"]));
    assert_eq!(ATMO.rayleigh[0], num(&atmo["rayleigh"][0]));
    assert_eq!(ATMO.rayleigh[1], num(&atmo["rayleigh"][1]));
    assert_eq!(ATMO.rayleigh[2], num(&atmo["rayleigh"][2]));
    assert_eq!(ATMO.rayleigh_scale_height_km, num(&atmo["rayleighScaleHeightKM"]));
    assert_eq!(ATMO.mie_scattering, num(&atmo["mieScattering"]));
    assert_eq!(ATMO.mie_absorption, num(&atmo["mieAbsorption"]));
    assert_eq!(ATMO.mie_scale_height_km, num(&atmo["mieScaleHeightKM"]));
    assert_eq!(ATMO.ozone[0], num(&atmo["ozone"][0]));
    assert_eq!(ATMO.ozone[1], num(&atmo["ozone"][1]));
    assert_eq!(ATMO.ozone[2], num(&atmo["ozone"][2]));
    assert_eq!(ATMO.ozone_centre_km, num(&atmo["ozoneCentreKM"]));
    assert_eq!(ATMO.ozone_width_km, num(&atmo["ozoneWidthKM"]));
    assert_eq!(ATMO.ground_albedo, num(&atmo["groundAlbedo"]));
}

/* ==================================================================== */
/* Phase functions.                                                      */
/* ==================================================================== */

#[test]
fn phase_functions_match_the_transcribed_glsl() {
    for row in golden()["phaseFunctions"].as_array().unwrap() {
        let c = num(&row["cosTheta"]);
        close(mie_phase(c), num(&row["mie"]), REL, "mie_phase");
        close(rayleigh_phase(c), num(&row["rayleigh"]), REL, "rayleigh_phase");
        close(hg_phase(c, 0.76), num(&row["hg_g0_76"]), REL, "hg_phase");
    }
}

/* ==================================================================== */
/* ray_sphere.                                                           */
/* ==================================================================== */

#[test]
fn ray_sphere_matches_the_transcribed_glsl() {
    for row in golden()["raySphere"].as_array().unwrap() {
        let ro = vec3_of(&row["ro"]);
        let rd = vec3_of(&row["rd"]);
        let rad = num(&row["rad"]);
        let expected = num(&row["t"]);
        let actual = ray_sphere(ro, rd, rad);
        close(actual, expected, REL, "ray_sphere");
    }
}

/* ==================================================================== */
/* medium.                                                               */
/* ==================================================================== */

#[test]
fn medium_matches_the_transcribed_glsl() {
    for row in golden()["medium"].as_array().unwrap() {
        let alt_km = num(&row["altKm"]);
        let mie_scale = num(&row["mieScale"]);
        let pos = Vec3::new(0.0, ATMO.ground_radius_mm + alt_km / 1000.0, 0.0);
        let m = medium(pos, mie_scale);
        close3(m.rayleigh_s, vec3_of(&row["rayleighS"]), REL, "medium.rayleigh_s");
        close(m.mie_s, num(&row["mieS"]), REL, "medium.mie_s");
        close3(m.extinction, vec3_of(&row["extinction"]), REL, "medium.extinction");
    }
}

/* ==================================================================== */
/* lut_uv.                                                               */
/* ==================================================================== */

#[test]
fn lut_uv_matches_the_transcribed_glsl() {
    for row in golden()["lutUv"].as_array().unwrap() {
        let pos = vec3_of(&row["pos"]);
        let dir = vec3_of(&row["dir"]);
        let (u, v) = lut_uv(pos, dir);
        close(u, num(&row["uv"][0]), REL, "lut_uv.u");
        close(v, num(&row["uv"][1]), REL, "lut_uv.v");
    }
}

/* ==================================================================== */
/* transmittance_to_space / luminance — genuine JS oracle.               */
/* ==================================================================== */

#[test]
fn transmittance_to_space_matches_the_javascript() {
    for row in golden()["transmittanceToSpace"].as_array().unwrap() {
        let mu = num(&row["mu"]);
        let mie_scale = num(&row["mieScale"]);
        let expected = vec3_of(&row["rgb"]);
        let actual = transmittance_to_space(mu, mie_scale);
        close3(Vec3::new(actual[0], actual[1], actual[2]), expected, REL, "transmittance_to_space");
    }
}

#[test]
fn luminance_matches_the_javascript() {
    for row in golden()["luminance"].as_array().unwrap() {
        let rgb = vec3_of(&row["rgb"]);
        let expected = num(&row["value"]);
        close(luminance([rgb.x, rgb.y, rgb.z]), expected, REL, "luminance");
    }
}

/* ==================================================================== */
/* raymarch_sky — the analytic segment integral, bake-independent.       */
/* ==================================================================== */

#[test]
fn raymarch_sky_segment_matches_the_transcribed_glsl() {
    for row in golden()["raymarchSkySegment"].as_array().unwrap() {
        let ray_dir = vec3_of(&row["rayDir"]);
        let sun_dir = vec3_of(&row["sunDir"]);
        let sun_irr = vec3_of(&row["sunIrr"]);
        let moon_dir = vec3_of(&row["moonDir"]);
        let moon_irr = vec3_of(&row["moonIrr"]);
        let steps = num(&row["steps"]) as u32;
        let mie_scale = num(&row["mieScale"]);
        let expected = vec3_of(&row["lum"]);

        let pos = Vec3::new(0.0, ATMO.ground_radius_mm + ATMO.view_altitude_mm, 0.0);
        let stub_t = |_p: Vec3, _d: Vec3| Vec3::new(1.0, 1.0, 1.0);
        let stub_m = |_p: Vec3, _d: Vec3| Vec3::new(0.01, 0.02, 0.03);
        let actual = raymarch_sky(
            pos, ray_dir, sun_dir, sun_irr, moon_dir, moon_irr, steps, mie_scale, stub_t, stub_m,
        );
        close3(actual, expected, REL, "raymarch_sky segment");
    }
}

/* ==================================================================== */
/* The three LUT bakes, full-grid comparison.                            */
/* ==================================================================== */

#[test]
fn transmittance_lut_matches_the_transcribed_glsl() {
    let g = &golden()["transmittanceLut"];
    let width = num(&g["width"]) as usize;
    let height = num(&g["height"]) as usize;
    let steps = num(&g["steps"]) as u32;
    let mie_scale = num(&g["mieScale"]);
    let expected = g["data"].as_array().unwrap();

    let lut = bake_transmittance(width, height, steps, mie_scale);
    assert_eq!(lut.data.len(), expected.len());
    for (i, (actual, exp)) in lut.data.iter().zip(expected.iter()).enumerate() {
        close3(*actual, vec3_of(exp), REL, &format!("transmittance texel {i}"));
    }
}

#[test]
fn multiscatter_lut_matches_the_transcribed_glsl() {
    let g = &golden()["multiscatterLut"];
    let size = num(&g["size"]) as usize;
    let steps = num(&g["steps"]) as u32;
    let sqrt_samples = num(&g["sqrtSamples"]) as usize;
    let mie_scale = num(&g["mieScale"]);
    let expected = g["data"].as_array().unwrap();

    // The multiscatter bake reads a transmittance LUT — the same
    // 256x64/40-step production bake the capture script fed it.
    let transmittance = bake_transmittance(256, 64, 40, mie_scale);
    let lut = bake_multiscatter(size, steps, sqrt_samples, mie_scale, &transmittance);
    assert_eq!(lut.data.len(), expected.len());
    for (i, (actual, exp)) in lut.data.iter().zip(expected.iter()).enumerate() {
        close3(*actual, vec3_of(exp), REL, &format!("multiscatter texel {i}"));
    }
}

fn params_from(g: &Value) -> (SkyViewParams, usize, usize, u32) {
    let params = SkyViewParams {
        sun_irradiance: vec3_of(&g["params"]["sunIrradiance"]),
        moon_irradiance: vec3_of(&g["params"]["moonIrradiance"]),
        sun_altitude: num(&g["params"]["sunAltitude"]),
        moon_rel_az: num(&g["params"]["moonRelAz"]),
        moon_altitude: num(&g["params"]["moonAltitude"]),
        view_pos: vec3_of(&g["params"]["viewPos"]),
        mie_scale: num(&g["params"]["mieScale"]),
    };
    (params, num(&g["width"]) as usize, num(&g["height"]) as usize, num(&g["steps"]) as u32)
}

#[test]
fn sky_view_lut_matches_the_transcribed_glsl() {
    let g = &golden()["skyViewLut"];
    let (params, width, height, steps) = params_from(g);
    let expected = g["data"].as_array().unwrap();

    let transmittance = bake_transmittance(256, 64, 40, params.mie_scale);
    let multiscatter = bake_multiscatter(32, 20, 8, params.mie_scale, &transmittance);
    let lut = bake_sky_view(width, height, steps, &params, &transmittance, &multiscatter);
    assert_eq!(lut.data.len(), expected.len());
    for (i, (actual, exp)) in lut.data.iter().zip(expected.iter()).enumerate() {
        close3(*actual, vec3_of(exp), REL, &format!("sky-view texel {i}"));
    }

    // `sky_view_lookup` against that same baked LUT, at fixed near-zenith-
    // avoiding ray directions (see capture.mjs's note on the skSkyView
    // singularity at exact zenith).
    let sun_dir = Vec3::new(0.0, params.sun_altitude.sin(), -params.sun_altitude.cos());
    for row in golden()["skyViewLookup"].as_array().unwrap() {
        let ray_dir = vec3_of(&row["rayDir"]);
        let expected = vec3_of(&row["rgb"]);
        let actual = sky_view_lookup(&lut, ray_dir, sun_dir, params.view_pos);
        close3(actual, expected, REL, "sky_view_lookup");
    }

    // Ambient probe, baked from the same LUT.
    let ap = &golden()["ambientProbe"];
    let sun_altitude = num(&ap["sunAltitude"]);
    let view_pos = vec3_of(&ap["viewPos"]);
    let [texel0, texel1] = bake_ambient(&lut, sun_altitude, view_pos);
    close3(texel0, vec3_of(&ap["texel0"]), REL, "ambient texel0 (sky)");
    close3(texel1, vec3_of(&ap["texel1"]), REL, "ambient texel1 (horizon)");
}

/* ==================================================================== */
/* Derived photometric-contract constants.                               */
/* ==================================================================== */

#[test]
fn derived_photometric_constants_reproduce_the_javascript_exactly() {
    let d = &golden()["derivedConstants"];
    assert_eq!(SUN_ILLUMINANCE_TOP, num(&d["sunIlluminanceTop"]));
    close(luminance([num(&d["noonSunRgb"][0]), num(&d["noonSunRgb"][1]), num(&d["noonSunRgb"][2])]), num(&d["noonSunLuminance"]), REL, "noon sun luminance (golden self-check)");
}

/// The source's own doc comment (`atmosphere.js:46-51`) states these as
/// approximate ("~3.9", "~0.06 radiance units"), so this is an
/// order-of-magnitude regression guard, not an exact pin — see the module
/// doc. A dropped or doubled `pi` (the exact historical bug the photometric-
/// contract note describes) moves either figure by a factor of ~3.14, which
/// these bounds would catch.
#[test]
fn photometric_derived_constants_are_the_right_order_of_magnitude() {
    assert_eq!(SUN_ILLUMINANCE_TOP, 5.12);

    // "noon sun after atmospheric extinction -> ~3.9 units (matches 4.3)".
    // `transmittance_to_space` returns a bare transmittance; scale by
    // SUN_ILLUMINANCE_TOP to reach scene light units, matching the golden's
    // `noonSunRgb = transmittanceToSpace(mu) * SUN_ILLUMINANCE_TOP`.
    let noon_mu = (68.4397829394163_f64 * std::f64::consts::PI / 180.0).sin();
    let noon_rgb = transmittance_to_space(noon_mu, 1.35);
    let noon_luminance = luminance([
        noon_rgb[0] * SUN_ILLUMINANCE_TOP,
        noon_rgb[1] * SUN_ILLUMINANCE_TOP,
        noon_rgb[2] * SUN_ILLUMINANCE_TOP,
    ]);
    assert!(
        (2.5..6.0).contains(&noon_luminance),
        "noon sun luminance {noon_luminance} is not order-of-magnitude ~3.9-4.4"
    );

    // "clear zenith sky ~1500 cd/m^2 -> ~0.06 radiance units".
    let zenith = &golden()["derivedConstants"]["zenithSkyLuminance"];
    let zenith_luminance = num(zenith);
    assert!(
        (0.02..0.2).contains(&zenith_luminance),
        "zenith sky luminance {zenith_luminance} is not order-of-magnitude ~0.06"
    );
}

/* ==================================================================== */
/* celestial.js — genuine JS oracle.                                     */
/* ==================================================================== */

#[test]
fn solar_declination_matches_the_javascript() {
    for row in golden()["celestial"]["solarDeclination"].as_array().unwrap() {
        let day = num(&row["dayOfYear"]);
        close(solar_declination(day), num(&row["decl"]), REL, "solar_declination");
    }
}

#[test]
fn alt_az_matches_the_javascript() {
    for row in golden()["celestial"]["altAz"].as_array().unwrap() {
        let hour_angle = num(&row["hourAngle"]);
        let decl = num(&row["decl"]);
        let lat = num(&row["lat"]);
        let aa = alt_az(hour_angle, decl, lat);
        close(aa.alt, num(&row["alt"]), REL, "alt_az.alt");
        close(aa.az, num(&row["az"]), REL, "alt_az.az");
    }
}

#[test]
fn dir_from_alt_az_matches_the_javascript() {
    for row in golden()["celestial"]["dirFromAltAz"].as_array().unwrap() {
        let alt = num(&row["alt"]);
        let az = num(&row["az"]);
        let north = num(&row["north"]);
        let dir = dir_from_alt_az(alt, az, north);
        close3(dir, vec3_of(&row["dir"]), REL, "dir_from_alt_az");
    }
}

#[test]
fn celestial_set_hour_matches_the_javascript() {
    for row in golden()["celestial"]["setHour"].as_array().unwrap() {
        let hour = num(&row["hour"]);
        let mut c = Celestial::new(SITE);
        c.set_hour(hour);
        close3(c.sun, vec3_of(&row["sun"]), REL, "celestial.sun");
        close3(c.moon, vec3_of(&row["moon"]), REL, "celestial.moon");
        close(c.sun_alt, num(&row["sunAlt"]), REL, "celestial.sun_alt");
        close(c.sun_az, num(&row["sunAz"]), REL, "celestial.sun_az");
        close(c.moon_alt, num(&row["moonAlt"]), REL, "celestial.moon_alt");
        close(c.moon_az, num(&row["moonAz"]), REL, "celestial.moon_az");
        close(c.moon_phase, num(&row["moonPhase"]), REL, "celestial.moon_phase");
        close(c.moon_elongation, num(&row["moonElongation"]), REL, "celestial.moon_elongation");

        let m = c.celestial_matrix();
        let rows = row["celestialMatrixRows"].as_array().unwrap();
        for (i, r) in rows.iter().enumerate() {
            let cols = r.as_array().unwrap();
            for (j, cell) in cols.iter().enumerate() {
                close(m.0[i][j], num(cell), REL, &format!("celestial_matrix[{i}][{j}]"));
            }
        }
    }
}

/* ==================================================================== */
/* noise.js — no oracle, pinned against the capture script's own          */
/* transcription (see that script's module doc and `sky/noise.rs`'s).     */
/* ==================================================================== */

fn vec2_of(v: &Value) -> NoiseVec2 {
    NoiseVec2::new(num(&v[0]), num(&v[1]))
}

#[test]
fn noise_hashes_and_value_noise_match_the_transcribed_glsl() {
    let n = &golden()["noise"];
    for row in n["hash12"].as_array().unwrap() {
        close(hash12(vec2_of(&row["p"])), num(&row["v"]), REL, "hash12");
    }
    for row in n["hash13"].as_array().unwrap() {
        close(hash13(vec3_of(&row["p"])), num(&row["v"]), REL, "hash13");
    }
    for row in n["hash33"].as_array().unwrap() {
        close3(hash33(vec3_of(&row["p"])), vec3_of(&row["v"]), REL, "hash33");
    }
    for row in n["ign"].as_array().unwrap() {
        close(ign(vec2_of(&row["p"])), num(&row["v"]), REL, "ign");
    }
    for row in n["val2"].as_array().unwrap() {
        close(val2(vec2_of(&row["p"])), num(&row["v"]), REL, "val2");
    }
    for row in n["val3"].as_array().unwrap() {
        close(val3(vec3_of(&row["p"])), num(&row["v"]), REL, "val3");
    }
}

#[test]
fn noise_fbm_families_match_the_transcribed_glsl() {
    let n = &golden()["noise"];
    for row in n["fbm2"].as_array().unwrap() {
        let oct = num(&row["oct"]) as i32;
        close(fbm2(vec2_of(&row["p"]), oct), num(&row["v"]), REL, "fbm2");
    }
    for row in n["ridge2"].as_array().unwrap() {
        let oct = num(&row["oct"]) as i32;
        close(ridge2(vec2_of(&row["p"]), oct), num(&row["v"]), REL, "ridge2");
    }
    for row in n["fbm3"].as_array().unwrap() {
        let oct = num(&row["oct"]) as i32;
        close(fbm3(vec3_of(&row["p"]), oct), num(&row["v"]), REL, "fbm3");
    }
}
