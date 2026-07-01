//! Shared overworld data model: the structures every worldgen stage, the atlas
//! builder, and the surface sampler operate on.
//!
//! Topology (sites + triangles) is **fixed for one generation**; all geology,
//! climate, erosion and hydrology mutate the flat per-region scalar fields on
//! [`PlanetGlobe`]. The durable, queryable output is [`PlanetSurfaceAtlas`].
//!
//! These are plain-data containers with public fields so the worldgen stage
//! implementations (in `stages/`) and the atlas/sampler can fill them; the
//! algorithms live in their own modules.

use axiom_math::Vec3;

use crate::growth::ids::{BiomeId, PlateId, RegionId};

/// A fixed icosphere: unit-sphere sites (region centres) and triangle faces.
#[derive(Debug, Clone, Default)]
pub struct Icosphere {
    /// Unit-length region centre directions, indexed by region id.
    pub sites: Vec<Vec3>,
    /// Triangle faces, each three region indices (CCW outward).
    pub triangles: Vec<[u32; 3]>,
    /// Subdivision level used (quantises region count).
    pub subdivisions: u32,
}

impl Icosphere {
    pub fn region_count(&self) -> usize {
        self.sites.len()
    }
}

/// Region adjacency in compressed-sparse-row form.
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
/// place; sea level is fixed at 0.
#[derive(Debug, Clone, Default)]
pub struct PlanetGlobe {
    pub topology: Icosphere,
    pub graph: RegionGraph,

    // Geology.
    pub region_plate: Vec<u32>,
    /// Whether each plate is oceanic.
    pub plate_oceanic: Vec<bool>,

    // Scalar fields (per region).
    /// Elevation; `>= 0` is land after `fit_land_coverage`.
    pub region_elevation: Vec<f32>,
    /// Moisture in `[0,1]`.
    pub region_moisture: Vec<f32>,
    /// Prevailing-wind tangent direction per region (unit).
    pub region_wind: Vec<Vec3>,
    /// Drainage / flow accumulation per region.
    pub region_flow: Vec<f32>,

    // Triangle scalars.
    /// Triangle elevations averaged from regions.
    pub triangle_elevation: Vec<f32>,
    /// Per-triangle river flow.
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

    /// Fraction of regions with elevation `>= 0` (land).
    pub fn land_fraction(&self) -> f32 {
        if self.region_elevation.is_empty() {
            return 0.0;
        }
        let land = self.region_elevation.iter().filter(|&&e| e >= 0.0).count();
        land as f32 / self.region_elevation.len() as f32
    }
}

/// A single overworld surface query result.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceSample {
    pub region: RegionId,
    pub plate: PlateId,
    pub elevation: f32,
    pub moisture: f32,
    /// Derived at query time, not stored.
    pub temperature: f32,
    pub biome: BiomeId,
}

/// The durable, queryable overworld output owned for the session.
#[derive(Debug, Clone, Default)]
pub struct PlanetSurfaceAtlas {
    pub sites: Vec<Vec3>,
    pub graph: RegionGraph,
    pub region_plate: Vec<u32>,
    pub plate_oceanic: Vec<bool>,
    pub region_elevation: Vec<f32>,
    pub region_moisture: Vec<f32>,
    /// Planet radius in metres (from genome).
    pub planet_radius_m: f32,
    /// Optional coarse spatial index for fast `locate_region`.
    pub locator: RegionLocator,
}

impl PlanetSurfaceAtlas {
    pub fn region_count(&self) -> usize {
        self.sites.len()
    }
}

/// Coarse spatial acceleration for `locate_region(unit_dir)` so it is not an
/// O(R) scan. Filled by the atlas builder; an empty locator falls back to
/// linear scan.
#[derive(Debug, Clone, Default)]
pub struct RegionLocator {
    /// Coarse-cell → candidate region indices (implementation-defined binning).
    pub cell_regions: Vec<Vec<u32>>,
    /// Number of latitude/longitude bands the binning uses.
    pub bands: u32,
}
