//! The twelve visual quality **axes**, a 0–5 **scorecard** over them, and the
//! weighted **final score**.
//!
//! This is the machinery that replaces a vague "looks better" with a disciplined
//! per-axis verdict. A [`Scorecard`] carries one integer score in `0..=5` for each
//! [`Axis`]; the [`Scorecard::final_score`] deliberately is **not** a plain average
//! — it is dominated by the *weakest* axis so a scene cannot hide one broken axis
//! behind eleven good ones:
//!
//! ```text
//! final_score = lowest_axis_score * 0.7 + average_axis_score * 0.3
//! ```
//!
//! The scores themselves are a human/agent judgement (the "visual scorecard"
//! artifact), authored as a flat TOML table keyed by the snake_case axis name. This
//! module owns only the *arithmetic and selection* over those scores — which is
//! fully deterministic — never the judgement.

use serde::{Deserialize, Serialize};

/// The lowest and highest score any axis may carry.
pub const MIN_SCORE: u8 = 0;
pub const MAX_SCORE: u8 = 5;

/// The score at or above which an axis is considered "good enough" — the per-axis
/// bar the completion criterion checks (every axis `>= PASS_SCORE`).
pub const PASS_SCORE: u8 = 4;

/// One scored visual quality axis. The set is **fixed** and the declaration order
/// is meaningful: it is the deterministic tie-break for [`Scorecard::lowest_axis`]
/// (the first axis in this order wins a tie) and the canonical iteration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// The read of the terrain's outline against the sky / far fog.
    TerrainSilhouette,
    /// Close-up material richness of the ground in the foreground.
    ForegroundMaterialDetail,
    /// How much vegetation there is (sparse vs. full).
    VegetationDensity,
    /// Whether vegetation forms believable clumps vs. an even sprinkle.
    VegetationClumping,
    /// Readable separation between near, mid, and far depth planes.
    DepthSeparation,
    /// Atmospheric fog / haze quality and its distance falloff.
    FogAndHaze,
    /// Whether the lighting reads as coming from one clear direction.
    LightingDirectionality,
    /// Cohesion and appeal of the overall colour palette.
    ColorPalette,
    /// Tonal spread — neither crushed-dark nor blown-out.
    ContrastAndExposure,
    /// Whether objects read at a believable real-world size.
    ObjectScale,
    /// Placement of the horizon line and overall framing.
    HorizonComposition,
    /// Freedom from rendering artifacts (z-fighting, shadow acne, seams).
    ArtifactLevel,
}

impl Axis {
    /// Every axis, in canonical order. This order *is* the tie-break for
    /// lowest-axis selection and the order the scorecard iterates.
    pub const ALL: [Axis; 12] = [
        Axis::TerrainSilhouette,
        Axis::ForegroundMaterialDetail,
        Axis::VegetationDensity,
        Axis::VegetationClumping,
        Axis::DepthSeparation,
        Axis::FogAndHaze,
        Axis::LightingDirectionality,
        Axis::ColorPalette,
        Axis::ContrastAndExposure,
        Axis::ObjectScale,
        Axis::HorizonComposition,
        Axis::ArtifactLevel,
    ];

    /// The snake_case name — the TOML key and the ledger/CLI spelling.
    pub fn key(self) -> &'static str {
        match self {
            Axis::TerrainSilhouette => "terrain_silhouette",
            Axis::ForegroundMaterialDetail => "foreground_material_detail",
            Axis::VegetationDensity => "vegetation_density",
            Axis::VegetationClumping => "vegetation_clumping",
            Axis::DepthSeparation => "depth_separation",
            Axis::FogAndHaze => "fog_and_haze",
            Axis::LightingDirectionality => "lighting_directionality",
            Axis::ColorPalette => "color_palette",
            Axis::ContrastAndExposure => "contrast_and_exposure",
            Axis::ObjectScale => "object_scale",
            Axis::HorizonComposition => "horizon_composition",
            Axis::ArtifactLevel => "artifact_level",
        }
    }

    /// Parse an axis from its snake_case key.
    pub fn parse(key: &str) -> Result<Axis, String> {
        Axis::ALL
            .into_iter()
            .find(|a| a.key() == key)
            .ok_or_else(|| format!("unknown axis '{key}'"))
    }
}

impl std::fmt::Display for Axis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// A full 0–5 scorecard: one score per [`Axis`]. Serialized as a flat TOML table
/// keyed by the snake_case axis name (every axis required, no extras).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scorecard {
    pub terrain_silhouette: u8,
    pub foreground_material_detail: u8,
    pub vegetation_density: u8,
    pub vegetation_clumping: u8,
    pub depth_separation: u8,
    pub fog_and_haze: u8,
    pub lighting_directionality: u8,
    pub color_palette: u8,
    pub contrast_and_exposure: u8,
    pub object_scale: u8,
    pub horizon_composition: u8,
    pub artifact_level: u8,
}

impl Scorecard {
    /// A scorecard with every axis at the same score (handy for tests and a flat
    /// starting baseline).
    pub fn uniform(score: u8) -> Scorecard {
        Scorecard {
            terrain_silhouette: score,
            foreground_material_detail: score,
            vegetation_density: score,
            vegetation_clumping: score,
            depth_separation: score,
            fog_and_haze: score,
            lighting_directionality: score,
            color_palette: score,
            contrast_and_exposure: score,
            object_scale: score,
            horizon_composition: score,
            artifact_level: score,
        }
    }

    /// The score for one axis.
    pub fn get(&self, axis: Axis) -> u8 {
        match axis {
            Axis::TerrainSilhouette => self.terrain_silhouette,
            Axis::ForegroundMaterialDetail => self.foreground_material_detail,
            Axis::VegetationDensity => self.vegetation_density,
            Axis::VegetationClumping => self.vegetation_clumping,
            Axis::DepthSeparation => self.depth_separation,
            Axis::FogAndHaze => self.fog_and_haze,
            Axis::LightingDirectionality => self.lighting_directionality,
            Axis::ColorPalette => self.color_palette,
            Axis::ContrastAndExposure => self.contrast_and_exposure,
            Axis::ObjectScale => self.object_scale,
            Axis::HorizonComposition => self.horizon_composition,
            Axis::ArtifactLevel => self.artifact_level,
        }
    }

    /// Overwrite the score for one axis (used by tests and tooling).
    pub fn set(&mut self, axis: Axis, score: u8) {
        let slot = match axis {
            Axis::TerrainSilhouette => &mut self.terrain_silhouette,
            Axis::ForegroundMaterialDetail => &mut self.foreground_material_detail,
            Axis::VegetationDensity => &mut self.vegetation_density,
            Axis::VegetationClumping => &mut self.vegetation_clumping,
            Axis::DepthSeparation => &mut self.depth_separation,
            Axis::FogAndHaze => &mut self.fog_and_haze,
            Axis::LightingDirectionality => &mut self.lighting_directionality,
            Axis::ColorPalette => &mut self.color_palette,
            Axis::ContrastAndExposure => &mut self.contrast_and_exposure,
            Axis::ObjectScale => &mut self.object_scale,
            Axis::HorizonComposition => &mut self.horizon_composition,
            Axis::ArtifactLevel => &mut self.artifact_level,
        };
        *slot = score;
    }

    /// `(axis, score)` for every axis, in canonical order.
    pub fn scores(&self) -> impl Iterator<Item = (Axis, u8)> + '_ {
        Axis::ALL.into_iter().map(move |a| (a, self.get(a)))
    }

    /// Reject a scorecard whose any axis is out of `0..=5`.
    pub fn validate(&self) -> Result<(), String> {
        match self.scores().find(|&(_, s)| s > MAX_SCORE) {
            Some((axis, score)) => {
                Err(format!("axis '{axis}' score {score} out of range 0..={MAX_SCORE}"))
            }
            None => Ok(()),
        }
    }

    /// Parse + validate a scorecard from TOML text.
    pub fn parse(toml_str: &str) -> Result<Scorecard, String> {
        let card: Scorecard =
            toml::from_str(toml_str).map_err(|e| format!("scorecard parse error: {e}"))?;
        card.validate()?;
        Ok(card)
    }

    /// Load + validate a scorecard from a file path.
    pub fn load(path: &std::path::Path) -> Result<Scorecard, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read scorecard {}: {e}", path.display()))?;
        Scorecard::parse(&text)
    }

    /// Serialize to TOML text (a flat, hand-editable table).
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("a scorecard always serializes")
    }

    /// The single lowest score across all axes.
    pub fn lowest_score(&self) -> u8 {
        self.scores().map(|(_, s)| s).min().unwrap_or(0)
    }

    /// The lowest-scoring axis — the next flaw to attack. Ties are broken by
    /// canonical [`Axis::ALL`] order (the first axis at the minimum wins), so this
    /// is fully deterministic.
    pub fn lowest_axis(&self) -> Axis {
        // `min_by_key` returns the *first* element on ties, and `scores()` yields
        // canonical order, so the tie-break is the axis declaration order.
        self.scores().min_by_key(|&(_, s)| s).map(|(a, _)| a).unwrap_or(Axis::TerrainSilhouette)
    }

    /// The mean score across all twelve axes.
    pub fn average(&self) -> f32 {
        let sum: u32 = self.scores().map(|(_, s)| u32::from(s)).sum();
        sum as f32 / Axis::ALL.len() as f32
    }

    /// The weighted final score: **lowest-dominated**, not a plain average.
    ///
    /// `final_score = lowest_axis_score * 0.7 + average_axis_score * 0.3`.
    pub fn final_score(&self) -> f32 {
        f32::from(self.lowest_score()) * 0.7 + self.average() * 0.3
    }

    /// Whether every axis clears the per-axis bar (`>= PASS_SCORE`). This is the
    /// machine half of the completion criterion; the other half is an explicit
    /// human acceptance.
    pub fn all_axes_pass(&self) -> bool {
        self.scores().all(|(_, s)| s >= PASS_SCORE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_axes_have_unique_keys_and_round_trip() {
        let mut seen = std::collections::BTreeSet::new();
        for a in Axis::ALL {
            assert!(seen.insert(a.key()), "duplicate key {}", a.key());
            assert_eq!(Axis::parse(a.key()).unwrap(), a);
            assert_eq!(a.to_string(), a.key());
        }
        assert_eq!(Axis::ALL.len(), 12);
        assert!(Axis::parse("nope").is_err());
    }

    #[test]
    fn get_set_cover_every_axis() {
        let mut card = Scorecard::uniform(0);
        for (i, a) in Axis::ALL.into_iter().enumerate() {
            card.set(a, (i % 6) as u8);
        }
        for (i, a) in Axis::ALL.into_iter().enumerate() {
            assert_eq!(card.get(a), (i % 6) as u8);
        }
    }

    #[test]
    fn final_score_is_lowest_dominated_not_average() {
        // Eleven 5s and one 0: plain average would be 55/12 ≈ 4.58, but the
        // lowest-dominated score must be far lower.
        let mut card = Scorecard::uniform(5);
        card.set(Axis::ArtifactLevel, 0);
        let avg = card.average();
        assert!((avg - 55.0 / 12.0).abs() < 1e-4);
        // lowest = 0 → final = 0*0.7 + avg*0.3 = 0.3*avg ≈ 1.37, well under the avg.
        let expected = 0.0 * 0.7 + avg * 0.3;
        assert!((card.final_score() - expected).abs() < 1e-5);
        assert!(card.final_score() < avg);
    }

    #[test]
    fn uniform_final_score_equals_the_score() {
        // With every axis equal, lowest == average == score, so final == score.
        let card = Scorecard::uniform(3);
        assert!((card.final_score() - 3.0).abs() < 1e-6);
    }

    #[test]
    fn lowest_axis_breaks_ties_by_canonical_order() {
        // Two axes tie at the minimum; the earlier one in ALL order wins.
        let mut card = Scorecard::uniform(4);
        card.set(Axis::VegetationDensity, 1);
        card.set(Axis::ColorPalette, 1);
        assert_eq!(card.lowest_axis(), Axis::VegetationDensity);
        assert_eq!(card.lowest_score(), 1);
    }

    #[test]
    fn all_axes_pass_reflects_the_bar() {
        assert!(Scorecard::uniform(PASS_SCORE).all_axes_pass());
        let mut card = Scorecard::uniform(PASS_SCORE);
        card.set(Axis::FogAndHaze, PASS_SCORE - 1);
        assert!(!card.all_axes_pass());
    }

    #[test]
    fn toml_round_trips_and_validates() {
        let mut card = Scorecard::uniform(3);
        card.set(Axis::TerrainSilhouette, 5);
        card.set(Axis::ArtifactLevel, 2);
        let text = card.to_toml();
        let back = Scorecard::parse(&text).unwrap();
        assert_eq!(card, back);
    }

    #[test]
    fn parse_rejects_out_of_range_and_unknown_fields() {
        // Out of range.
        let mut card = Scorecard::uniform(3);
        card.artifact_level = 9;
        assert!(card.validate().is_err());
        // A missing axis or an extra key both fail to parse.
        assert!(Scorecard::parse("terrain_silhouette = 3").is_err());
        let mut full = Scorecard::uniform(3).to_toml();
        full.push_str("\nbogus_axis = 1\n");
        assert!(Scorecard::parse(&full).is_err());
    }
}
