//! Planet genome: stored physical/orbital knobs; derived values on demand.
//! Audit: worldgen.md §3 (L_star, a, e, S_eff, M_p, R_p, P_rot, obliquity, p0,
//! albedo, greenhouse, water_fraction, precipitation, material_tags, schema).
//!
//! Per the audit ("derived values on demand"), `g`, `P_orb`, `T_eq`, and `S_eff`
//! are computed from the stored knobs by accessors rather than stored, so the
//! genome stays a minimal, replayable description of the planet.

/// Newtonian gravitational constant (m^3 kg^-1 s^-2). Audit: GEN-genome derived g.
const GRAVITATIONAL_CONSTANT: f64 = 6.674e-11;
/// Stefan-Boltzmann constant (W m^-2 K^-4), for equilibrium temperature.
const STEFAN_BOLTZMANN: f64 = 5.670_374e-8;
/// Reference stellar luminosity (Sun, W) for the insolation proxy and Kepler.
const SOLAR_LUMINOSITY: f64 = 3.828e26;
/// Reference stellar mass (Sun, kg) used as the Kepler central-mass assumption.
const SOLAR_MASS: f64 = 1.989e30;
/// Solar constant at 1 AU (W m^-2): the flux that defines S_eff = 1.0.
const SOLAR_CONSTANT_1AU: f64 = 1361.0;
/// One astronomical unit (m): reference orbital distance.
const ONE_AU: f64 = 1.496e11;

/// Relative weights of the dominant surface materials for downstream content
/// (biome palettes, resource yields). Audit: planet_presets.xml `material_weights`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialWeights {
    pub high_silicate: f32,
    pub organic: f32,
}

impl Default for MaterialWeights {
    /// Earthlike default (`high_silicate=0.6`, `organic=0.3`). Audit: earthlike preset.
    fn default() -> Self {
        Self {
            high_silicate: 0.6,
            organic: 0.3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanetGenome {
    pub l_star: f32,
    pub semi_major_axis: f32,
    pub eccentricity: f32,
    pub s_eff: f32,
    pub mass_kg: f32,
    pub radius_m: f32,
    pub rotation_period_s: f32,
    pub obliquity_rad: f32,
    pub surface_pressure_pa: f32,
    pub albedo: f32,
    pub greenhouse: f32,
    pub water_fraction: f32,
    pub precipitation: f32,
    /// Relative surface material weights. Audit: planet_presets.xml material_weights.
    pub material_weights: MaterialWeights,
    pub schema_version: u32,
}

impl Default for PlanetGenome {
    fn default() -> Self {
        Self {
            l_star: 3.828e26,
            semi_major_axis: 1.496e11,
            eccentricity: 0.0167,
            s_eff: 1.0,
            mass_kg: 5.972e24,
            radius_m: 6_371_000.0,
            rotation_period_s: 86_400.0,
            obliquity_rad: 0.4091,
            surface_pressure_pa: 101_325.0,
            albedo: 0.3,
            greenhouse: 1.0,
            water_fraction: 0.7,
            precipitation: 0.5,
            material_weights: MaterialWeights::default(),
            schema_version: 1,
        }
    }
}

impl PlanetGenome {
    /// Land fraction target implied by water_fraction. Audit: land-slider default.
    pub fn implied_land_fraction(&self) -> f32 {
        (1.0 - self.water_fraction).clamp(0.0, 1.0)
    }

    /// Surface gravity (m/s^2) from mass + radius: `g = G M / R^2`.
    /// Audit: GEN-genome "derived values on demand", G = 6.674e-11.
    pub fn gravity_m_s2(&self) -> f32 {
        let m = self.mass_kg as f64;
        let r = self.radius_m as f64;
        if r <= 0.0 {
            return 0.0;
        }
        (GRAVITATIONAL_CONSTANT * m / (r * r)) as f32
    }

    /// Orbital period (s) via Kepler's third law about a Sun-mass star:
    /// `P = 2π sqrt(a^3 / (G M_star))`. Audit: GEN-genome derived P_orb.
    pub fn orbital_period_s(&self) -> f32 {
        let a = self.semi_major_axis as f64;
        if a <= 0.0 {
            return 0.0;
        }
        let mu = GRAVITATIONAL_CONSTANT * SOLAR_MASS;
        (core::f64::consts::TAU * (a * a * a / mu).sqrt()) as f32
    }

    /// Stellar insolation relative to Earth (`S_eff`): the incident flux at the
    /// orbit, scaled by luminosity and inverse-square distance, normalised so an
    /// Earthlike planet at 1 AU around a Sun-luminosity star reads ~1.0.
    /// Audit: GEN-genome derived S_eff / insolation.
    pub fn insolation(&self) -> f32 {
        let l = self.l_star as f64;
        let a = self.semi_major_axis as f64;
        if a <= 0.0 {
            return 0.0;
        }
        // Flux at orbit: L / (4π a^2). Divide by the 1 AU / Sun-luminosity flux.
        let flux = l / (4.0 * core::f64::consts::PI * a * a);
        let ref_flux = SOLAR_LUMINOSITY / (4.0 * core::f64::consts::PI * ONE_AU * ONE_AU);
        (flux / ref_flux) as f32
    }

    /// Equilibrium (radiative) surface temperature (K), greenhouse-adjusted.
    /// Bare-rock `T_eq = ( (1-A) F / (4 σ) )^(1/4)` from the orbital flux, then
    /// multiplied by the greenhouse factor. Audit: GEN-genome derived T_eq.
    pub fn equilibrium_temperature_k(&self) -> f32 {
        let l = self.l_star as f64;
        let a = self.semi_major_axis as f64;
        let albedo = (self.albedo as f64).clamp(0.0, 1.0);
        if a <= 0.0 {
            return 0.0;
        }
        let flux = l / (4.0 * core::f64::consts::PI * a * a);
        let t_eq = ((1.0 - albedo) * flux / (4.0 * STEFAN_BOLTZMANN)).powf(0.25);
        (t_eq * self.greenhouse as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_derived_values_are_earthlike() {
        let g = PlanetGenome::default();
        // Earth surface gravity ~9.81 m/s^2.
        assert!(
            (g.gravity_m_s2() - 9.81).abs() < 0.3,
            "gravity {}",
            g.gravity_m_s2()
        );
        // Earth orbital period ~3.156e7 s (one year).
        let year = g.orbital_period_s();
        assert!((year - 3.156e7).abs() / 3.156e7 < 0.05, "period {year}");
        // Insolation at 1 AU around the Sun ~1.0.
        assert!(
            (g.insolation() - 1.0).abs() < 0.05,
            "insolation {}",
            g.insolation()
        );
        // Greenhouse-free Earth equilibrium temp ~255 K; with greenhouse=1.0 unchanged.
        let t = g.equilibrium_temperature_k();
        assert!((230.0..280.0).contains(&t), "T_eq {t}");
    }

    #[test]
    fn derived_values_finite() {
        let g = PlanetGenome::default();
        assert!(g.gravity_m_s2().is_finite());
        assert!(g.orbital_period_s().is_finite());
        assert!(g.insolation().is_finite());
        assert!(g.equilibrium_temperature_k().is_finite());
    }

    #[test]
    fn material_weights_default_is_earthlike() {
        let w = MaterialWeights::default();
        assert_eq!(w.high_silicate, 0.6);
        assert_eq!(w.organic, 0.3);
        assert_eq!(PlanetGenome::default().material_weights, w);
    }
}
