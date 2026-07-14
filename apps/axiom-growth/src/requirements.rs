//! Requirement traceability registry.
//! Every requirement from `docs/growth-port/worldgen_simulator_requirements_audit.md`
//! is listed here with a category and an implementation status, so "everything
//! in the audit is accounted for" is a checkable fact, not a claim. The
//! adversarial review agents (see `docs/growth-port/adversarial-review-plan.md`)
//! cross-check this registry against both the audit and the real code.
//! Status is updated as subsystems land. `Implemented` requires real code +
//! tests; `Scaffolded` is a typed stub + wiring; `Deferred` is represented but
//! intentionally later-phase; `Engine` is owned by an Axiom layer/module.

/// Implementation status of a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Implemented,
    Scaffolded,
    Deferred,
    Engine,
}

/// One traced requirement.
#[derive(Debug, Clone, Copy)]
pub struct Requirement {
    /// Audit id (story id where present, else a stable synthetic id).
    pub id: &'static str,
    /// Audit category.
    pub category: &'static str,
    /// Current status in this app.
    pub status: Status,
    /// Where it is addressed (module path or note).
    pub site: &'static str,
}

const fn r(
    id: &'static str,
    category: &'static str,
    status: Status,
    site: &'static str,
) -> Requirement {
    Requirement {
        id,
        category,
        status,
        site,
    }
}

use Status::*;

/// The full requirement set traced from the audit.
pub const REQUIREMENTS: &[Requirement] = &[
    // --- Platform / Sim API (SA) ---
    r("SA-0.1", "platform", Engine, "axiom boot/app framework"),
    r(
        "SA-0.2",
        "platform",
        Deferred,
        "async gen → cooperative tick job",
    ),
    r(
        "SA-E1",
        "platform",
        Scaffolded,
        "lib::Growth session owns gen job",
    ),
    // R1 coverage audit: site not yet wired — honest Deferred until a form layer.
    r(
        "SA-E4",
        "platform",
        Deferred,
        "world_size→sim_sites (not yet wired)",
    ),
    r(
        "SA-E5",
        "platform",
        Deferred,
        "site-count guardrails (topology cap only)",
    ),
    // --- Overworld generation (OW) ---
    r("OW-cfg", "overworld", Scaffolded, "seed/genome form"),
    r(
        "OW-det",
        "overworld",
        Implemented,
        "rng + deterministic stages",
    ),
    r(
        "OW-4.4",
        "atlas",
        Implemented,
        "lib::Growth stores seed+genome",
    ),
    r("OW-E1", "atlas", Implemented, "atlas::build_atlas"),
    r("OW-E2", "atlas", Implemented, "lib::Growth owns atlas"),
    r(
        "OW-E3",
        "atlas",
        Implemented,
        "sampler::sample_surface/locate_region",
    ),
    r("OW-E4", "atlas", Scaffolded, "sampler::SurfaceSample dict"),
    r(
        "OW-E5",
        "pipeline",
        Implemented,
        "pipeline::StageRegistry MissingStage",
    ),
    // R2 honesty audit: these are fully implemented + pipeline-registered.
    r("OW-E7", "climate", Implemented, "stages::wind_field"),
    r(
        "OW-E8",
        "climate",
        Implemented,
        "stages::moisture_advection",
    ),
    r("OW-E9", "climate", Implemented, "stages::rain_shadow"),
    r("OW-E11", "hydrology", Implemented, "stages::priority_flood"),
    r(
        "OW-E12",
        "hydrology",
        Engine,
        "primal mesh export (presentation)",
    ),
    r(
        "OW-E13",
        "hydrology",
        Scaffolded,
        "presentation displacement scale",
    ),
    r("OW-E14", "hydrology", Implemented, "stages::river_carve"),
    r(
        "OW-E15",
        "hydrology",
        Scaffolded,
        "stages::elevation smoothing",
    ),
    r(
        "OW-E16",
        "hydrology",
        Implemented,
        "stages::erosion (stream-power)",
    ),
    r(
        "OW-E18",
        "qa",
        Implemented,
        "axiom_geosphere::validate_region_rings",
    ),
    r("OW-E19", "perf", Deferred, "marshal caps (presentation)"),
    r(
        "OW-E20",
        "perf",
        Scaffolded,
        "globe dropped after atlas build",
    ),
    r(
        "OW-E21",
        "hydrology",
        Implemented,
        "stages::fit_land_coverage",
    ),
    r("OW-P1", "persist", Deferred, "atlas serialize (later)"),
    // --- Planet/preset/genome ---
    r("GEN-genome", "atlas", Implemented, "genome::PlanetGenome"),
    r(
        "GEN-preset",
        "overworld",
        Implemented,
        "presets::PlanetPreset",
    ),
    r(
        "GEN-land",
        "hydrology",
        Implemented,
        "land vs water fraction",
    ),
    // --- Generation primitives ---
    r(
        "PRIM-rng",
        "determinism",
        Implemented,
        "axiom_entropy::EntropyStream + distributions float sampling",
    ),
    r(
        "PRIM-noise",
        "procgen",
        Engine,
        "axiom-noise layer (value_noise/Fbm)",
    ),
    r("PRIM-geo", "procgen", Implemented, "axiom_math:: spherical math"),
    r(
        "PRIM-icosphere",
        "procgen",
        Implemented,
        "axiom_geosphere::build_icosphere",
    ),
    // --- Pipeline stages (concrete) ---
    r(
        "PIPE-globe",
        "pipeline",
        Implemented,
        "pipeline::DEFAULT_GLOBE",
    ),
    r(
        "PIPE-tectonics",
        "overworld",
        Implemented,
        "stages::tectonic_plates",
    ),
    r(
        "PIPE-elevation",
        "overworld",
        Implemented,
        "stages::elevation",
    ),
    r("PIPE-moisture", "climate", Implemented, "stages::moisture"),
    r(
        "PIPE-rivers",
        "hydrology",
        Implemented,
        "stages::river_downflow/flow",
    ),
    r(
        "PIPE-triangles",
        "hydrology",
        Implemented,
        "stages::triangle_values",
    ),
    // --- Climate (derived) ---
    r(
        "CLIM-temp",
        "climate",
        Implemented,
        "sampler::derive_temperature",
    ),
    r("CLIM-biome", "climate", Scaffolded, "sampler::derive_biome"),
    // --- Game-world streaming (GW) ---
    r(
        "GW-E1",
        "streaming",
        Implemented,
        "localmap::GameWorldLocalMap",
    ),
    r(
        "GW-E2",
        "streaming",
        Implemented,
        "gameworld::generate_chunk",
    ),
    r(
        "GW-E3",
        "streaming",
        Implemented,
        "chunkstore preserves edited",
    ),
    r("GW-E4", "gameplay", Scaffolded, "dig::apply_dig"),
    r(
        "GW-E9",
        "streaming",
        Implemented,
        "chunkstore::stream (axiom_streaming::Residency)",
    ),
    r("GW-E10", "gameplay", Scaffolded, "intent::IntentRouter"),
    r("GW-E11", "gameplay", Scaffolded, "dig handler"),
    r(
        "GW-E12",
        "streaming",
        Implemented,
        "gameworld sample_macro from atlas",
    ),
    r("GW-E14", "gameplay", Scaffolded, "inventory::Inventory"),
    r("GW-E15", "gameplay", Scaffolded, "dig yield → inventory"),
    r(
        "GW-E16",
        "streaming",
        Implemented,
        "gameworld data-driven chunk pipeline",
    ),
    r(
        "GW-E17",
        "streaming",
        Implemented,
        "gameworld::GameWorldPipeline",
    ),
    r(
        "GW-E18",
        "streaming",
        Implemented,
        "gameworld::sample_macro_continuous (IDW)",
    ),
    r(
        "GW-E19",
        "streaming",
        Implemented,
        "gameworld coherent detail + seams",
    ),
    r(
        "GW-7.0",
        "streaming",
        Scaffolded,
        "lib::Growth enter_game_world",
    ),
    r(
        "GW-7.1",
        "presentation",
        Deferred,
        "chunk mesh build (presentation)",
    ),
    // --- Correctness / QA (SC) ---
    r(
        "SC-E1",
        "qa",
        Implemented,
        "axiom_geosphere::validate_region_rings (wired in generate)",
    ),
    r(
        "SC-E2",
        "qa",
        Engine,
        "primal mesh winding (presentation, OW-E12)",
    ),
    r(
        "SC-E3",
        "qa",
        Implemented,
        "determinism::world_hash + generate_is_deterministic",
    ),
    r("SC-E8", "qa", Implemented, "gameworld seam/adjacency test"),
    // R1 audit: deterministic_globe variant + persistence are represented as
    // deferred so they are accounted for, not silently dropped.
    r(
        "PIPE-det",
        "qa",
        Deferred,
        "deterministic_globe pipeline variant",
    ),
    r(
        "GW-E7",
        "persist",
        Deferred,
        "chunk-edit save/load (Reflect-ready)",
    ),
    // --- Gameplay layers (downstream) ---
    r(
        "PL-0.x",
        "gameplay",
        Deferred,
        "player::PlayerController scaffold",
    ),
    r(
        "SV-0.x",
        "gameplay",
        Deferred,
        "survival::Need/Threat scaffold",
    ),
    r(
        "GE-0.x",
        "gameplay",
        Deferred,
        "emergence::BiasSet scaffold",
    ),
    r("SP-0.x", "gameplay", Deferred, "spirit::time_gate scaffold"),
    r(
        "EC-0.x",
        "gameplay",
        Deferred,
        "ecology::Population scaffold",
    ),
    r(
        "PR-0.x",
        "presentation",
        Deferred,
        "presentation cel/biome tint scaffold",
    ),
    // --- Moddability ---
    r(
        "MOD-defs",
        "moddability",
        Scaffolded,
        "defs:: data-driven config",
    ),
    r(
        "MOD-pipeline",
        "moddability",
        Implemented,
        "pipeline stage order as data",
    ),
];

/// Count requirements by status.
pub fn count(status: Status) -> usize {
    REQUIREMENTS.iter().filter(|r| r.status == status).count()
}

/// Look up a requirement by id.
pub fn get(id: &str) -> Option<&'static Requirement> {
    REQUIREMENTS.iter().find(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_is_populated() {
        assert!(REQUIREMENTS.len() >= 50, "audit coverage too thin");
    }

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for req in REQUIREMENTS {
            assert!(seen.insert(req.id), "duplicate requirement id: {}", req.id);
        }
    }

    #[test]
    fn report_status_breakdown() {
        let total = REQUIREMENTS.len();
        let impl_n = count(Status::Implemented);
        let scaf = count(Status::Scaffolded);
        let defer = count(Status::Deferred);
        let eng = count(Status::Engine);
        assert_eq!(total, impl_n + scaf + defer + eng);
    }
}
