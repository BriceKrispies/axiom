//! Planet presets as distributions sampled into a genome under constraints.
//! Audit: planet_presets.xml (earthlike/ocean_world/dry), "Planet genome reqs".
//!
//! Each preset is a set of per-knob distributions plus validation constraints.
//! `sample_genome` draws every knob from the preset's distribution, derives the
//! on-demand values (gravity, insolation, surface temp), and validates them
//! against the constraint band — resampling up to `MAX_ATTEMPTS` times. All
//! draws come from an `axiom_entropy::EntropyStream` (via `crate::growth::distributions`),
//! so the same seed reproduces the same genome bit-for-bit every run.
use axiom_entropy::EntropyStream;

use crate::growth::distributions;
use crate::growth::genome::{MaterialWeights, PlanetGenome};

/// Audit: planet_presets.xml earthlike `max_attempts="32"`.
const MAX_ATTEMPTS: u32 = 32;

/// Reference solar luminosity (W) used to back out semi-major axis from an
/// insolation target, matching the genome's insolation normalisation.
const SOLAR_LUMINOSITY: f64 = 3.828e26;
/// One astronomical unit (m).
const ONE_AU: f64 = 1.496e11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanetPreset {
    Earthlike,
    OceanWorld,
    Dry,
}

impl PlanetPreset {
    pub fn id(self) -> &'static str {
        match self {
            PlanetPreset::Earthlike => "earthlike",
            PlanetPreset::OceanWorld => "ocean_world",
            PlanetPreset::Dry => "dry",
        }
    }
}

/// The validation band a sampled genome must satisfy. Audit: planet_presets.xml
/// `<constraints>`. Bounds default to "always pass" so presets that omit a field
/// (ocean_world/dry only set a few) impose no extra constraint.
#[derive(Debug, Clone, Copy)]
struct Constraints {
    insolation_min: f32,
    insolation_max: f32,
    surface_temp_min: f32,
    surface_temp_max: f32,
    gravity_min: f32,
    gravity_max: f32,
    pressure_min: f32,
    pressure_max: f32,
    eccentricity_max: f32,
}

impl Constraints {
    /// A permissive default: every field passes. Presets tighten the fields they
    /// declare in XML.
    const ANY: Constraints = Constraints {
        insolation_min: f32::NEG_INFINITY,
        insolation_max: f32::INFINITY,
        surface_temp_min: f32::NEG_INFINITY,
        surface_temp_max: f32::INFINITY,
        gravity_min: f32::NEG_INFINITY,
        gravity_max: f32::INFINITY,
        pressure_min: f32::NEG_INFINITY,
        pressure_max: f32::INFINITY,
        eccentricity_max: f32::INFINITY,
    };

    /// Surface temperature is the equilibrium temperature plus a fixed offset
    /// approximating habitable-surface warming, so a 255 K bare-rock Earthlike
    /// lands inside the 273-310 K band. (Greenhouse is already folded into the
    /// genome's equilibrium_temperature_k.)
    fn passes(&self, g: &PlanetGenome) -> bool {
        let ins = g.insolation();
        let temp = surface_temp_k(g);
        let grav = g.gravity_m_s2();
        let p = g.surface_pressure_pa;
        ins >= self.insolation_min
            && ins <= self.insolation_max
            && temp >= self.surface_temp_min
            && temp <= self.surface_temp_max
            && grav >= self.gravity_min
            && grav <= self.gravity_max
            && p >= self.pressure_min
            && p <= self.pressure_max
            && g.eccentricity <= self.eccentricity_max
    }
}

/// Approximate habitable surface temperature: the radiative equilibrium temp
/// warmed by a fixed habitable offset. Earth's equilibrium temp (~255 K) maps to
/// ~288 K, matching the 273-310 K constraint band. Audit: surface_temp_* bounds.
const SURFACE_TEMP_OFFSET_K: f32 = 33.0;

fn surface_temp_k(g: &PlanetGenome) -> f32 {
    g.equilibrium_temperature_k() + SURFACE_TEMP_OFFSET_K
}

/// Sample a uniform value in `[a, b)`.
fn uniform(stream: &mut EntropyStream, a: f32, b: f32) -> f32 {
    distributions::range(stream, a, b)
}

/// Sample a normal value `N(mean, std)` clamped to `[lo, hi]`. Audit: XML Normal
/// dists carry clamp bounds in `c`/`d` (S_eff_target, obliquity).
fn normal_clamped(stream: &mut EntropyStream, mean: f32, std: f32, lo: f32, hi: f32) -> f32 {
    (mean + std * distributions::normal(stream)).clamp(lo, hi)
}

/// Derive the semi-major axis (m) that yields a given insolation `S_eff` for a
/// star of luminosity `l_star`. Inverts `S_eff = (L/4πa²) / (L_sun/4πAU²)`,
/// i.e. `a = AU * sqrt( (L/L_sun) / S_eff )`. This ties the orbit to the
/// sampled S_eff target so the insolation constraint is satisfiable.
fn semi_major_axis_for_seff(l_star: f32, s_eff: f32) -> f32 {
    let l_ratio = (l_star as f64) / SOLAR_LUMINOSITY;
    let s = (s_eff as f64).max(1.0e-3);
    (ONE_AU * (l_ratio / s).sqrt()) as f32
}

/// Material weights for a preset. Audit: planet_presets.xml material_weights
/// (all three presets currently declare high_silicate=0.6, organic=0.3).
fn material_weights(preset: PlanetPreset) -> MaterialWeights {
    match preset {
        PlanetPreset::Earthlike | PlanetPreset::OceanWorld | PlanetPreset::Dry => MaterialWeights {
            high_silicate: 0.6,
            organic: 0.3,
        },
    }
}

/// Validation constraints for a preset. Audit: planet_presets.xml `<constraints>`.
fn constraints(preset: PlanetPreset) -> Constraints {
    match preset {
        PlanetPreset::Earthlike => Constraints {
            insolation_min: 0.85,
            insolation_max: 1.15,
            surface_temp_min: 273.0,
            surface_temp_max: 310.0,
            gravity_min: 8.5,
            gravity_max: 11.5,
            pressure_min: 70_000.0,
            pressure_max: 140_000.0,
            eccentricity_max: 0.2,
        },
        // ocean_world declares only surface_temp_max="320".
        PlanetPreset::OceanWorld => Constraints {
            surface_temp_max: 320.0,
            ..Constraints::ANY
        },
        // dry declares no constraints.
        PlanetPreset::Dry => Constraints::ANY,
    }
}

/// Draw one candidate genome from the preset's distributions (no validation).
/// ocean_world and dry override only the knobs they declare in XML; the rest
/// fall back to the earthlike-shaped baseline so the genome is always complete.
fn sample_once(preset: PlanetPreset, rng: &mut EntropyStream) -> PlanetGenome {
    // Shared earthlike-shaped baseline knobs (all presets inherit these unless
    // they override below). material_weights is rng-free; l_star is the first
    // rng draw, so listing it first in the literal preserves the draw order.
    let mut g = PlanetGenome {
        material_weights: material_weights(preset),
        l_star: uniform(rng, 3.4452e26, 4.2108e26),
        ..Default::default()
    };
    let s_eff_target = normal_clamped(rng, 1.0, 0.15, 0.8, 1.2);
    g.eccentricity = uniform(rng, 0.0, 0.08);
    g.mass_kg = uniform(rng, 4.7776e24, 7.1664e24);
    g.radius_m = uniform(rng, 6.05245e6, 6.68955e6);
    g.rotation_period_s = uniform(rng, 75_600.0, 97_200.0);
    g.obliquity_rad = normal_clamped(rng, 0.4091, 0.1, 0.2, 0.6);
    g.albedo = uniform(rng, 0.28, 0.35);
    g.greenhouse = uniform(rng, 0.95, 1.08);
    g.water_fraction = uniform(rng, 0.65, 0.75);
    g.precipitation = uniform(rng, 0.4, 0.7);

    // Per-preset overrides (planet_presets.xml).
    let p0 = match preset {
        PlanetPreset::Earthlike => uniform(rng, 90_000.0, 111_000.0),
        PlanetPreset::OceanWorld => uniform(rng, 100_000.0, 150_000.0),
        PlanetPreset::Dry => uniform(rng, 90_000.0, 111_000.0),
    };
    g.surface_pressure_pa = p0;

    match preset {
        PlanetPreset::Earthlike => {}
        PlanetPreset::OceanWorld => {
            g.water_fraction = uniform(rng, 0.85, 0.95);
            g.precipitation = uniform(rng, 0.6, 0.95);
        }
        PlanetPreset::Dry => {
            g.water_fraction = uniform(rng, 0.1, 0.35);
            g.precipitation = uniform(rng, 0.05, 0.25);
            g.albedo = uniform(rng, 0.32, 0.42);
        }
    }

    // Tie orbit to the sampled S_eff target so insolation is the target, then
    // store the realised insolation as s_eff. Audit: S_eff_target distribution.
    g.semi_major_axis = semi_major_axis_for_seff(g.l_star, s_eff_target);
    g.s_eff = g.insolation();
    g
}

/// Sample a genome from a preset, validating against the preset's constraints and
/// resampling up to `MAX_ATTEMPTS` times. Returns the last attempt if none pass.
/// Audit: GEN-preset, planet_presets.xml constraints + max_attempts=32.
pub fn sample_genome(preset: PlanetPreset, rng: &mut EntropyStream) -> PlanetGenome {
    let cons = constraints(preset);
    let mut last = sample_once(preset, rng);
    if cons.passes(&last) {
        return last;
    }
    for _ in 1..MAX_ATTEMPTS {
        last = sample_once(preset, rng);
        if cons.passes(&last) {
            return last;
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::seed::WorldSeed;

    fn genome_for(preset: PlanetPreset, seed: &str) -> PlanetGenome {
        let mut rng =
            crate::growth::pipeline::worldgen_stream(WorldSeed::from_str_seed(seed).value);
        sample_genome(preset, &mut rng)
    }

    fn assert_genome_eq(a: &PlanetGenome, b: &PlanetGenome) {
        assert_eq!(a.l_star, b.l_star);
        assert_eq!(a.semi_major_axis, b.semi_major_axis);
        assert_eq!(a.eccentricity, b.eccentricity);
        assert_eq!(a.s_eff, b.s_eff);
        assert_eq!(a.mass_kg, b.mass_kg);
        assert_eq!(a.radius_m, b.radius_m);
        assert_eq!(a.rotation_period_s, b.rotation_period_s);
        assert_eq!(a.obliquity_rad, b.obliquity_rad);
        assert_eq!(a.surface_pressure_pa, b.surface_pressure_pa);
        assert_eq!(a.albedo, b.albedo);
        assert_eq!(a.greenhouse, b.greenhouse);
        assert_eq!(a.water_fraction, b.water_fraction);
        assert_eq!(a.precipitation, b.precipitation);
        assert_eq!(a.material_weights, b.material_weights);
        assert_eq!(a.schema_version, b.schema_version);
    }

    #[test]
    fn same_preset_and_seed_is_identical_field_by_field() {
        for preset in [
            PlanetPreset::Earthlike,
            PlanetPreset::OceanWorld,
            PlanetPreset::Dry,
        ] {
            let a = genome_for(preset, "deterministic-seed");
            let b = genome_for(preset, "deterministic-seed");
            assert_genome_eq(&a, &b);
        }
    }

    #[test]
    fn earthlike_satisfies_constraints() {
        let cons = constraints(PlanetPreset::Earthlike);
        // Try several seeds: every earthlike genome should pass the band.
        for s in ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"] {
            let g = genome_for(PlanetPreset::Earthlike, s);
            assert!(cons.passes(&g), "earthlike seed {s} failed constraints");
            // Gravity in band.
            let grav = g.gravity_m_s2();
            assert!((8.5..=11.5).contains(&grav), "gravity {grav} seed {s}");
            // Water fraction in the earthlike band.
            assert!(
                (0.65..=0.75).contains(&g.water_fraction),
                "water {} seed {s}",
                g.water_fraction
            );
            // Insolation in band.
            assert!(
                (0.85..=1.15).contains(&g.insolation()),
                "insolation seed {s}"
            );
        }
    }

    #[test]
    fn ocean_world_wetter_than_dry() {
        for s in ["one", "two", "three", "four"] {
            let ocean = genome_for(PlanetPreset::OceanWorld, s);
            let dry = genome_for(PlanetPreset::Dry, s);
            assert!(
                ocean.water_fraction > dry.water_fraction,
                "ocean {} !> dry {} (seed {s})",
                ocean.water_fraction,
                dry.water_fraction,
            );
            assert!((0.85..=0.95).contains(&ocean.water_fraction));
            assert!((0.1..=0.35).contains(&dry.water_fraction));
        }
    }

    #[test]
    fn derived_values_finite_and_sane() {
        for preset in [
            PlanetPreset::Earthlike,
            PlanetPreset::OceanWorld,
            PlanetPreset::Dry,
        ] {
            let g = genome_for(preset, "sanity");
            let grav = g.gravity_m_s2();
            let temp = g.equilibrium_temperature_k();
            assert!(grav.is_finite() && grav > 0.0, "gravity {grav}");
            assert!(temp.is_finite() && temp > 0.0, "temp {temp}");
            // Earthlike-mass/radius gravity stays in a planetary range.
            assert!(
                (5.0..15.0).contains(&grav),
                "gravity {grav} for {:?}",
                preset
            );
            assert!(g.orbital_period_s().is_finite() && g.orbital_period_s() > 0.0);
            assert!(g.insolation().is_finite() && g.insolation() > 0.0);
        }
    }

    #[test]
    fn material_weights_set_per_preset() {
        for preset in [
            PlanetPreset::Earthlike,
            PlanetPreset::OceanWorld,
            PlanetPreset::Dry,
        ] {
            let g = genome_for(preset, "mat");
            assert_eq!(
                g.material_weights,
                MaterialWeights {
                    high_silicate: 0.6,
                    organic: 0.3
                }
            );
        }
    }
}
