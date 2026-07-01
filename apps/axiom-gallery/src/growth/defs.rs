//! Moddable data-driven config: pipeline stage order, presets, biomes as data.
//!
//! The overworld stage order — once resolved against an app-side registry — is
//! now the fixed, straight-line call sequence inside `axiom_planetgen`. This
//! descriptive stage list is retained as documentation of that canonical order.

/// A named pipeline definition (ordered stage ids). Audit: world_gen_pipelines.xml.
#[derive(Debug, Clone)]
pub struct PipelineDef {
    pub id: &'static str,
    pub stages: &'static [&'static str],
}

/// The canonical overworld stage order, in the sequence `axiom_planetgen`
/// executes it. Descriptive only (the generator no longer resolves it against a
/// registry). Audit: performance_globe / world_gen_pipelines.xml.
pub const PERFORMANCE_GLOBE: &[&str] = &[
    "topology",
    "half_edge_mesh",
    "region_neighbours",
    "tectonic_plates",
    "plate_properties",
    "elevation",
    "erosion",
    "fit_land_coverage",
    "moisture",
    "wind_field",
    "moisture_advection",
    "rain_shadow",
    "triangle_values",
    "priority_flood",
    "river_downflow",
    "river_flow",
    "river_carve",
];
