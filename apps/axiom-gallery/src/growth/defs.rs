//! Moddable data-driven config: pipeline stage order, presets, biomes as data.
//! Audit: "Pipeline/modding requirements", moddability. Scaffold — today these
//! are Rust constants; a later loader can read them from data packs.
//!
//! The key invariant preserved from Growth: **stage order is data**, resolved
//! against the registry (pipeline.rs), not hardcoded in the generator.

/// A named pipeline definition (ordered stage ids). Audit: world_gen_pipelines.xml.
#[derive(Debug, Clone)]
pub struct PipelineDef {
    pub id: &'static str,
    pub stages: &'static [&'static str],
}

/// The performance profile uses fewer erosion iterations. Audit: performance_globe.
pub const PERFORMANCE_GLOBE: &[&str] = crate::growth::pipeline::DEFAULT_GLOBE;
