//! Worldgen stage implementations, registered into a [`StageRegistry`] by id.
//!
//! Each id in [`DEFAULT_GLOBE`] resolves to a real [`Stage`] implementation in
//! its own submodule. The three pre-pipeline ids — `topology`, `half_edge_mesh`,
//! `region_neighbours` — are produced by the topology module *before* the
//! pipeline runs, so they stay genuine no-ops here. Everything else does real
//! geology / climate / hydrology work.

use crate::growth::model_planet::PlanetGlobe;
use crate::growth::pipeline::{GenContext, Stage, StageRegistry, DEFAULT_GLOBE};

mod elevation;
mod erosion;
mod fit_land_coverage;
mod moisture;
mod moisture_advection;
mod plate_properties;
mod priority_flood;
mod rain_shadow;
mod rivers;
mod tectonic_plates;
mod triangle_values;
mod wind_field;

use elevation::ElevationStage;
use erosion::ErosionStage;
use fit_land_coverage::FitLandCoverageStage;
use moisture::MoistureStage;
use moisture_advection::MoistureAdvectionStage;
use plate_properties::PlatePropertiesStage;
use priority_flood::PriorityFloodStage;
use rain_shadow::RainShadowStage;
use rivers::{RiverCarveStage, RiverDownflowStage, RiverFlowStage};
use tectonic_plates::TectonicPlatesStage;
use triangle_values::TriangleValuesStage;
use wind_field::WindFieldStage;

/// A no-op stage for ids whose data is produced before the pipeline runs
/// (`topology`, `half_edge_mesh`, `region_neighbours`).
struct NoopStage(&'static str);
impl Stage for NoopStage {
    fn id(&self) -> &'static str {
        self.0
    }
    fn run(&self, _globe: &mut PlanetGlobe, _ctx: &mut GenContext) {}
}

/// Register a real implementation for every default-globe stage id.
///
/// Any id in [`DEFAULT_GLOBE`] without an explicit registration below falls back
/// to a no-op, so the pipeline always builds; the only such ids are the three
/// topology-produced stages.
pub fn register_default_stages(reg: &mut StageRegistry) {
    reg.register("tectonic_plates", || Box::new(TectonicPlatesStage));
    reg.register("plate_properties", || Box::new(PlatePropertiesStage));
    reg.register("elevation", || Box::new(ElevationStage));
    reg.register("erosion", || Box::new(ErosionStage));
    reg.register("fit_land_coverage", || Box::new(FitLandCoverageStage));
    reg.register("moisture", || Box::new(MoistureStage));
    reg.register("triangle_values", || Box::new(TriangleValuesStage));
    reg.register("priority_flood", || Box::new(PriorityFloodStage));
    reg.register("river_downflow", || Box::new(RiverDownflowStage));
    reg.register("river_flow", || Box::new(RiverFlowStage));
    reg.register("river_carve", || Box::new(RiverCarveStage));
    reg.register("wind_field", || Box::new(WindFieldStage));
    reg.register("moisture_advection", || Box::new(MoistureAdvectionStage));
    reg.register("rain_shadow", || Box::new(RainShadowStage));

    for id in DEFAULT_GLOBE {
        if !reg.contains(id) {
            let sid: &'static str = id;
            reg.register(sid, move || Box::new(NoopStage(sid)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::pipeline::DEFAULT_GLOBE;

    #[test]
    fn every_default_id_is_registered() {
        let mut reg = StageRegistry::new();
        register_default_stages(&mut reg);
        for id in DEFAULT_GLOBE {
            assert!(reg.contains(id), "stage {} not registered", id);
        }
        assert!(reg.build(DEFAULT_GLOBE).is_ok());
    }
}
