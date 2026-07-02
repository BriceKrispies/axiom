//! Axiom Visual Target — the **fixed, versioned scene manifest**.
//!
//! This is deliberately boring: plain `#[derive(Deserialize)]` structs, explicit
//! fields, a required `version`, validated on load. The whole diorama is a pure
//! function of one TOML file. No procedural world generation, no gameplay — just
//! data describing a camera, a sun, fog, a terrain patch, ground materials, and a
//! list of vegetation instances (optionally grown from an explicit, seeded
//! `[scatter]` block — see [`super::scatter`]).
//!
//! Coordinate convention: right-handed, `+y` up, metres. `[f32; 3]` arrays are
//! `[x, y, z]`; `[f32; 2]` terrain slope is `[dy/dx, dy/dz]`; colours are linear
//! RGB in `[0, 1]`.

use serde::Deserialize;

/// The one manifest schema version this runner understands. A file with any other
/// `version` is rejected on load (forward/backward changes bump this deliberately).
pub const MANIFEST_VERSION: u32 = 1;

/// A whole visual-target scene.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Schema version; must equal [`MANIFEST_VERSION`].
    pub version: u32,
    /// Human label (informational only).
    #[serde(default)]
    pub name: String,
    pub camera: Camera,
    pub sun: Sun,
    pub fog: Fog,
    pub terrain: Terrain,
    /// Explicitly authored tree instances.
    #[serde(default, rename = "tree")]
    pub trees: Vec<Tree>,
    /// Optional deterministic expansion appended to `trees` at load time.
    #[serde(default)]
    pub scatter: Option<Scatter>,
    /// Optional deterministic ground-cover (grass/litter tufts) — the abstraction
    /// added to unlock `foreground_material_detail`. The base diorama has no way to
    /// express small ground-level detail; this is the smallest primitive that does.
    #[serde(default)]
    pub groundcover: Option<Groundcover>,
    /// Whether the scene has volumetric light (god-rays) — **neutral frame data**.
    /// When `true`, every backend applies the same `host::apply_frame_volumetrics`
    /// pass (the `FrameVolumetrics::low_poly` preset), so shafts render identically on
    /// Canvas 2D, WebGPU, and WebGL.
    #[serde(default)]
    pub volumetrics: bool,
}

/// The single camera the frame is rendered from.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub fov_deg: f32,
    pub near_m: f32,
    pub far_m: f32,
    pub width_px: u32,
    pub height_px: u32,
}

/// One directional sun (rendered as a real directional light + shadow map).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sun {
    /// The direction the sunlight *travels* (points away from the sun).
    pub direction: [f32; 3],
    /// Linear RGB light colour.
    pub color: [f32; 3],
    pub intensity: f32,
}

/// Linear distance fog, baked toward `color` per-vertex by camera distance.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fog {
    /// Linear RGB the scene fades toward (also the frame clear colour).
    pub color: [f32; 3],
    /// Distance (m) where fog begins (0 fog nearer than this).
    pub start_m: f32,
    /// Distance (m) where fog is full (color fully replaces surface nearer of this).
    pub end_m: f32,
}

/// The explicit sloped terrain patch.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    /// Side length (m) of the square patch, centred on the world origin.
    pub size_m: f32,
    /// Grid vertex spacing (m).
    pub spacing_m: f32,
    /// Constant height offset (m).
    #[serde(default)]
    pub base_height_m: f32,
    /// Linear ground slope `[dy/dx, dy/dz]`.
    pub slope: [f32; 2],
    /// Explicit, small, deterministic value-noise octaves layered on the slope.
    #[serde(default)]
    pub detail: Vec<Octave>,
    /// Ground albedo bands by height (blended); first band whose `max_height_m`
    /// exceeds the vertex height wins, blended toward the next.
    #[serde(default, rename = "ground_band")]
    pub ground_bands: Vec<GroundBand>,
    /// Albedo steep faces pull toward (exposed rock).
    #[serde(default = "default_rock_albedo")]
    pub rock_albedo: [f32; 3],
    /// Slope (rise/run) at which the rock tint starts / is full.
    #[serde(default = "default_rock_slope_start")]
    pub rock_slope_start: f32,
    #[serde(default = "default_rock_slope_full")]
    pub rock_slope_full: f32,
}

/// One deterministic value-noise octave (authored data, not worldgen).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Octave {
    pub amplitude_m: f32,
    pub wavelength_m: f32,
    pub seed: u32,
}

/// A ground albedo band up to `max_height_m`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundBand {
    pub max_height_m: f32,
    pub albedo: [f32; 3],
}

/// One tree instance: a trunk prism + a canopy blob, both instanced.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tree {
    pub x: f32,
    pub z: f32,
    #[serde(default)]
    pub yaw_deg: f32,
    pub trunk_height_m: f32,
    pub trunk_radius_m: f32,
    pub canopy_radius_m: f32,
    /// Linear RGB autumn canopy tint.
    pub canopy_color: [f32; 3],
}

/// Optional deterministic scatter expanded into extra [`Tree`] instances.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scatter {
    pub seed: u64,
    pub count: u32,
    pub min_spacing_m: f32,
    /// Skip candidate sites whose terrain slope (rise/run) exceeds this.
    pub slope_limit: f32,
    /// Keep-clear radius (m) around the camera: reject any candidate within this of
    /// the camera's ground position, so a dense forest never spawns a trunk right in
    /// the camera's face (the abstraction that unblocked vegetation_density). `0` =
    /// no exclusion.
    #[serde(default)]
    pub keep_clear_m: f32,
    /// Number of clump centres. `0` = uniform placement; `> 0` scatters trees around
    /// this many seeded centres (the abstraction that unblocked vegetation_clumping),
    /// producing believable clumps and clearings instead of an even sprinkle.
    #[serde(default)]
    pub clusters: u32,
    /// Spread radius (m) of each clump around its centre (used when `clusters > 0`).
    #[serde(default)]
    pub cluster_radius_m: f32,
    /// Trunk height range `[min, max]` (m).
    pub trunk_height_m: [f32; 2],
    /// Trunk radius range `[min, max]` (m).
    pub trunk_radius_m: [f32; 2],
    /// Canopy radius range `[min, max]` (m).
    pub canopy_radius_m: [f32; 2],
    /// Autumn palette a canopy colour is picked from per instance.
    pub canopy_palette: Vec<[f32; 3]>,
}

/// One ground-cover tuft: a small instanced cluster seated on the terrain. This is
/// the value type the [`Groundcover`] abstraction produces — the ground-level
/// analogue of a [`Tree`], carrying no trunk.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tuft {
    pub x: f32,
    pub z: f32,
    #[serde(default)]
    pub yaw_deg: f32,
    pub height_m: f32,
    pub radius_m: f32,
    /// Linear RGB tint (dry grass / leaf-litter).
    pub color: [f32; 3],
}

/// A deterministic ground-cover layer expanded into [`Tuft`] instances — the
/// smallest primitive that lets a manifest express foreground ground detail.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Groundcover {
    pub seed: u64,
    pub count: u32,
    pub min_spacing_m: f32,
    /// Skip candidate sites steeper than this (rise/run).
    pub slope_limit: f32,
    /// Tuft height range `[min, max]` (m).
    pub height_m: [f32; 2],
    /// Tuft radius range `[min, max]` (m).
    pub radius_m: [f32; 2],
    /// Palette a tuft colour is picked from per instance.
    pub palette: Vec<[f32; 3]>,
}

fn default_rock_albedo() -> [f32; 3] {
    [0.42, 0.40, 0.38]
}
fn default_rock_slope_start() -> f32 {
    0.45
}
fn default_rock_slope_full() -> f32 {
    0.95
}

impl Manifest {
    /// Parse + validate a manifest from TOML text. Returns a human-readable error
    /// string on any structural or semantic problem.
    pub fn parse(toml_str: &str) -> Result<Manifest, String> {
        let manifest: Manifest =
            toml::from_str(toml_str).map_err(|e| format!("manifest parse error: {e}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load + validate a manifest from a file path.
    pub fn load(path: &str) -> Result<Manifest, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read manifest {path}: {e}"))?;
        Manifest::parse(&text)
    }

    /// Reject a manifest whose numbers cannot describe a renderable frame.
    fn validate(&self) -> Result<(), String> {
        (self.version == MANIFEST_VERSION).then_some(()).ok_or_else(|| {
            format!(
                "manifest version {} unsupported (this runner speaks version {MANIFEST_VERSION})",
                self.version
            )
        })?;
        let c = &self.camera;
        (c.width_px > 0 && c.height_px > 0)
            .then_some(())
            .ok_or("camera width_px/height_px must be > 0")?;
        (c.far_m > c.near_m && c.near_m > 0.0)
            .then_some(())
            .ok_or("camera requires far_m > near_m > 0")?;
        (c.fov_deg > 0.0 && c.fov_deg < 180.0)
            .then_some(())
            .ok_or("camera fov_deg must be in (0, 180)")?;
        (self.terrain.size_m > 0.0 && self.terrain.spacing_m > 0.0)
            .then_some(())
            .ok_or("terrain size_m and spacing_m must be > 0")?;
        (self.fog.end_m > self.fog.start_m && self.fog.start_m >= 0.0)
            .then_some(())
            .ok_or("fog requires end_m > start_m >= 0")?;
        if let Some(s) = &self.scatter {
            (!s.canopy_palette.is_empty())
                .then_some(())
                .ok_or("scatter.canopy_palette must have at least one colour")?;
            (s.min_spacing_m > 0.0)
                .then_some(())
                .ok_or("scatter.min_spacing_m must be > 0")?;
        }
        if let Some(g) = &self.groundcover {
            (!g.palette.is_empty())
                .then_some(())
                .ok_or("groundcover.palette must have at least one colour")?;
            (g.min_spacing_m > 0.0)
                .then_some(())
                .ok_or("groundcover.min_spacing_m must be > 0")?;
        }
        Ok(())
    }
}

impl Terrain {
    /// Half the patch side — the patch spans `[-half, +half]` in x and z.
    pub fn half_m(&self) -> f32 {
        self.size_m * 0.5
    }

    /// Absolute terrain height (m) at a world `(x, z)`: the linear slope plus every
    /// authored value-noise octave. Pure and deterministic — the single source of
    /// truth shared by the mesh builder and the scatter slope test.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        let linear = self.base_height_m + self.slope[0] * x + self.slope[1] * z;
        let detail: f32 = self
            .detail
            .iter()
            .map(|o| {
                let w = o.wavelength_m.max(1.0e-3);
                o.amplitude_m * value_noise(o.seed, x / w, z / w)
            })
            .sum();
        linear + detail
    }

    /// Terrain slope magnitude (rise/run) at `(x, z)` via central difference.
    pub fn slope_at(&self, x: f32, z: f32) -> f32 {
        let e = self.spacing_m.max(1.0e-3);
        let dhx = (self.height_at(x + e, z) - self.height_at(x - e, z)) / (2.0 * e);
        let dhz = (self.height_at(x, z + e) - self.height_at(x, z - e)) / (2.0 * e);
        (dhx * dhx + dhz * dhz).sqrt()
    }
}

/// A small deterministic integer hash → `[-1, 1]` at lattice cell `(ix, iz)` for a
/// given octave `seed`. Pure integer math, so identical on every platform.
fn hash2(seed: u32, ix: i32, iz: i32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B1);
    h ^= (ix as u32).wrapping_mul(0x85EB_CA77);
    h = h.wrapping_mul(0xC2B2_AE3D);
    h ^= (iz as u32).wrapping_mul(0x27D4_EB2F);
    h = h.wrapping_mul(0x1656_67B1);
    h ^= h >> 15;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Signed value noise in `[-1, 1]` with smoothstep-interpolated lattice corners.
fn value_noise(seed: u32, x: f32, z: f32) -> f32 {
    let x0 = x.floor();
    let z0 = z.floor();
    let ix = x0 as i32;
    let iz = z0 as i32;
    let fx = x - x0;
    let fz = z - z0;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sz = fz * fz * (3.0 - 2.0 * fz);
    let n00 = hash2(seed, ix, iz);
    let n10 = hash2(seed, ix + 1, iz);
    let n01 = hash2(seed, ix, iz + 1);
    let n11 = hash2(seed, ix + 1, iz + 1);
    let a = n00 + (n10 - n00) * sx;
    let b = n01 + (n11 - n01) * sx;
    a + (b - a) * sz
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
version = 1
name = "test"
[camera]
eye = [0.0, 10.0, 30.0]
target = [0.0, 2.0, 0.0]
fov_deg = 55.0
near_m = 0.1
far_m = 300.0
width_px = 320
height_px = 200
[sun]
direction = [-0.4, -0.8, -0.4]
color = [1.0, 0.95, 0.8]
intensity = 1.1
[fog]
color = [0.7, 0.8, 0.9]
start_m = 20.0
end_m = 150.0
[terrain]
size_m = 64.0
spacing_m = 1.0
slope = [0.05, 0.08]
detail = [ { amplitude_m = 1.0, wavelength_m = 20.0, seed = 3 } ]
[[terrain.ground_band]]
max_height_m = 1.0
albedo = [0.3, 0.25, 0.15]
[[tree]]
x = 5.0
z = -3.0
trunk_height_m = 6.0
trunk_radius_m = 0.3
canopy_radius_m = 3.0
canopy_color = [0.8, 0.4, 0.1]
"#;

    #[test]
    fn parses_a_valid_manifest() {
        let m = Manifest::parse(SAMPLE).expect("valid manifest parses");
        assert_eq!(m.version, 1);
        assert_eq!(m.trees.len(), 1);
        assert_eq!(m.terrain.detail.len(), 1);
        assert!(m.scatter.is_none());
    }

    #[test]
    fn rejects_wrong_version() {
        let bad = SAMPLE.replacen("version = 1", "version = 999", 1);
        assert!(Manifest::parse(&bad).is_err());
    }

    #[test]
    fn rejects_degenerate_camera() {
        let bad = SAMPLE.replacen("far_m = 300.0", "far_m = 0.05", 1);
        assert!(Manifest::parse(&bad).is_err());
    }

    #[test]
    fn rejects_unknown_field() {
        let bad = SAMPLE.replacen("name = \"test\"", "name = \"test\"\nbogus = 1", 1);
        assert!(Manifest::parse(&bad).is_err());
    }

    #[test]
    fn height_and_slope_are_deterministic() {
        let m = Manifest::parse(SAMPLE).unwrap();
        let h1 = m.terrain.height_at(3.0, -2.0);
        let h2 = m.terrain.height_at(3.0, -2.0);
        assert_eq!(h1, h2);
        // The linear slope dominates: height rises moving +x and +z.
        assert!(m.terrain.height_at(10.0, 0.0) > m.terrain.height_at(-10.0, 0.0));
        assert!(m.terrain.slope_at(0.0, 0.0) > 0.0);
    }

    #[test]
    fn parses_groundcover_and_rejects_empty_palette() {
        let gc = "\n[groundcover]\nseed = 1\ncount = 100\nmin_spacing_m = 0.3\n\
                  slope_limit = 1.0\nheight_m = [0.2, 0.5]\nradius_m = [0.1, 0.3]\n\
                  palette = [ [0.6, 0.5, 0.2] ]\n";
        let m = Manifest::parse(&format!("{SAMPLE}{gc}")).unwrap();
        assert!(m.groundcover.is_some());
        let bad = format!("{SAMPLE}{}", gc.replace("[ [0.6, 0.5, 0.2] ]", "[]"));
        assert!(Manifest::parse(&bad).is_err());
    }

    #[test]
    fn value_noise_is_bounded_and_repeatable() {
        for k in 0..50 {
            let x = k as f32 * 0.37;
            let n = value_noise(7, x, x * 1.3);
            assert!((-1.0..=1.0).contains(&n));
            assert_eq!(n, value_noise(7, x, x * 1.3));
        }
    }
}
