//! Shared overworld data model: the structures every worldgen stage, the atlas
//! builder, and the surface sampler operate on.
//!
//! Design (audit: "Data/model requirements", "Terrain/hydrology requirements"):
//! topology (sites + triangles) is **fixed for one generation**; all geology,
//! climate, erosion and hydrology mutate the flat per-region scalar fields on
//! [`PlanetGlobe`]. The durable, queryable output is [`PlanetSurfaceAtlas`].
//!
//! These are plain-data containers with public fields so the worldgen stage
//! implementations (in `stages/`) and the atlas/sampler can fill them; the
//! algorithms live in their own modules.

use axiom_math::Vec3;

use crate::ids::{BiomeId, PlateId, RegionId};

/// A fixed icosphere: unit-sphere sites (region centres) and triangle faces.
/// Audit: worldgen `topology` stage, OW-E12 primal-quad export.
#[derive(Debug, Clone, Default)]
pub struct Icosphere {
    /// Unit-length region centre directions, indexed by region id.
    pub sites: Vec<Vec3>,
    /// Triangle faces, each three region indices (CCW outward).
    pub triangles: Vec<[u32; 3]>,
    /// Subdivision level used (quantises region count). Audit: perf cap subdiv 9.
    pub subdivisions: u32,
}

impl Icosphere {
    pub fn region_count(&self) -> usize {
        self.sites.len()
    }
}

/// Region adjacency in compressed-sparse-row form. Audit: OW-E1 "neighbours CSR".
#[derive(Debug, Clone, Default)]
pub struct RegionGraph {
    /// `offsets[r]..offsets[r+1]` slices into `neighbours` for region `r`.
    pub offsets: Vec<u32>,
    /// Flattened neighbour region indices.
    pub neighbours: Vec<u32>,
}

impl RegionGraph {
    /// Neighbour region indices of `region`.
    pub fn neighbours_of(&self, region: RegionId) -> &[u32] {
        let i = region.index();
        if i + 1 >= self.offsets.len() {
            return &[];
        }
        let start = self.offsets[i] as usize;
        let end = self.offsets[i + 1] as usize;
        &self.neighbours[start..end]
    }
}

/// Mutable generation state. Worldgen stages read/write these flat fields in
/// place; sea level is fixed at 0 (audit: OW-E21 `fit_land_coverage`).
#[derive(Debug, Clone, Default)]
pub struct PlanetGlobe {
    pub topology: Icosphere,
    pub graph: RegionGraph,

    // --- geology ---
    /// Plate id per region. Audit: `tectonic_plates`.
    pub region_plate: Vec<u32>,
    /// Whether each plate is oceanic. Audit: `plate_properties`.
    pub plate_oceanic: Vec<bool>,

    // --- scalar fields (per region) ---
    /// Elevation; `>= 0` is land after `fit_land_coverage`. Audit: OW-E21.
    pub region_elevation: Vec<f32>,
    /// Moisture in `[0,1]`. Audit: moisture / advection / rain shadow.
    pub region_moisture: Vec<f32>,
    /// Prevailing-wind tangent direction per region (unit). Audit: OW-E7.
    pub region_wind: Vec<Vec3>,
    /// Drainage / flow accumulation per region. Audit: priority_flood, rivers.
    pub region_flow: Vec<f32>,

    // --- triangle scalars ---
    /// Triangle elevations averaged from regions. Audit: `triangle_values`.
    pub triangle_elevation: Vec<f32>,
    /// Per-triangle river flow. Audit: `river_flow`.
    pub triangle_flow: Vec<f32>,
}

impl PlanetGlobe {
    pub fn region_count(&self) -> usize {
        self.topology.region_count()
    }

    /// Allocate all per-region/-triangle fields to match topology.
    pub fn resize_fields(&mut self) {
        let r = self.region_count();
        let t = self.topology.triangles.len();
        self.region_plate.resize(r, 0);
        self.region_elevation.resize(r, 0.0);
        self.region_moisture.resize(r, 0.0);
        self.region_wind.resize(r, Vec3::new(1.0, 0.0, 0.0));
        self.region_flow.resize(r, 0.0);
        self.triangle_elevation.resize(t, 0.0);
        self.triangle_flow.resize(t, 0.0);
    }

    /// Fraction of regions with elevation `>= 0` (land). Audit: OW-E21 gate.
    pub fn land_fraction(&self) -> f32 {
        if self.region_elevation.is_empty() {
            return 0.0;
        }
        let land = self
            .region_elevation
            .iter()
            .filter(|&&e| e >= 0.0)
            .count();
        land as f32 / self.region_elevation.len() as f32
    }
}

/// A single overworld surface query result. Audit: OW-E3 `sample_surface`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceSample {
    pub region: RegionId,
    pub plate: PlateId,
    pub elevation: f32,
    pub moisture: f32,
    /// Derived at query time, not stored. Audit: Climate requirements.
    pub temperature: f32,
    pub biome: BiomeId,
}

/// The durable, queryable overworld output owned for the session.
/// Audit: OW-E1/E2 `PlanetSurfaceAtlas`, OW-E3 query API, surface-atlas reqs.
#[derive(Debug, Clone, Default)]
pub struct PlanetSurfaceAtlas {
    /// Fixed region centre directions (unit). Audit: surface atlas reqs.
    pub sites: Vec<Vec3>,
    pub graph: RegionGraph,
    pub region_plate: Vec<u32>,
    pub plate_oceanic: Vec<bool>,
    pub region_elevation: Vec<f32>,
    pub region_moisture: Vec<f32>,
    /// Planet radius in metres (from genome). Audit: localmap, GW-E1.
    pub planet_radius_m: f32,
    /// Optional coarse spatial index for fast `locate_region`. Audit: perf P1.
    pub locator: RegionLocator,
}

impl PlanetSurfaceAtlas {
    pub fn region_count(&self) -> usize {
        self.sites.len()
    }
}

/// Coarse spatial acceleration for `locate_region(unit_dir)` so it is not an
/// O(R) scan. Audit: "Query/API requirements" spatial index, perf P1.
/// Filled by the atlas builder; an empty locator falls back to linear scan.
#[derive(Debug, Clone, Default)]
pub struct RegionLocator {
    /// Coarse-cell → candidate region indices (implementation-defined binning).
    pub cell_regions: Vec<Vec<u32>>,
    /// Number of latitude/longitude bands the binning uses.
    pub bands: u32,
}
