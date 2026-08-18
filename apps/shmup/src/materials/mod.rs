//! Ported from Claude-of-Duty `src/materials/library.js:16-401`.
//!
//! The materials library: 19 base surface definitions with bake parameters,
//! material settings, and Three.js rendering options. The texture generation
//! (GLSL generators) is handled separately; this module defines the data that
//! parameterizes it.

/// Tileable procedural noise library (`src/materials/glsl/noise.js`) ported
/// as CPU-evaluable maths — the foundation every generator identified by
/// [`LibraryEntry::generator`] builds on.
pub mod noise;

/// The procedural texture forge's CPU bake pipeline (`src/materials/
/// generator.js`) — the shared detail/macro maps, the Sobel-derived normal,
/// and the `owSurface` contract every future per-material generator
/// implements.
pub mod bake;

/// The curvature-driven vertex wear/grime/AO bake (`src/materials/masks.js`).
pub mod masks;

/// Per-material `owSurface` generators (`src/materials/glsl/surfaces-*.js`).
pub mod surfaces;

use crate::world::palette::Surface;

/// Baking parameters for material texture generation.
#[derive(Debug, Clone, Copy)]
pub struct BakeParams {
    /// Texture resolution in pixels.
    pub size: u32,
    /// Real-world size in metres covered by a single texture tile.
    pub world_size: f32,
    /// Height relief scale (0-1) for normal map generation.
    pub relief: f32,
    /// Deterministic seed for noise generation.
    pub seed: u32,
    /// Per-generator parameters [x, y, z, w].
    pub param: [f32; 4],
    /// Optional tint color A (hex RGB).
    pub tint_a: Option<u32>,
    /// Optional tint color B (hex RGB).
    pub tint_b: Option<u32>,
}

/// Material rendering parameters.
#[derive(Debug, Clone)]
pub struct MatParams {
    /// UV projection mode.
    pub uv_mode: Option<&'static str>,
    /// Texture scale factor.
    pub scale: f32,
    /// Parallax depth (in fractions of texture height).
    pub parallax: Option<f32>,
    /// Parallax layer count (for accurate steep mapping).
    pub parallax_layers: Option<u32>,
    /// Detiling strength (reduces obvious tiling).
    pub detile: Option<f32>,
    /// Detail microstructure [scale, contrast, opacity, frequency].
    pub detail: Option<[f32; 4]>,
    /// Macro variation [scale, contrast, opacity, frequency].
    pub macro_: Option<[f32; 4]>,
    /// Large macro variation for long-distance readability.
    pub macro_big: Option<[f32; 4]>,
    /// Patch variation (replastered areas, damage).
    pub patch: Option<[f32; 4]>,
    /// Weather streaks and runoff [amplitude, direction, saturation, frequency].
    pub weather: Option<[f32; 4]>,
    /// Wear colour overlay (hex RGB).
    pub wear_color: Option<u32>,
    /// Dust colour overlay (hex RGB).
    pub dust_color: Option<u32>,
    /// Grime colour overlay (hex RGB).
    pub grime_color: Option<u32>,
    /// Tint colour (hex RGB).
    pub tint: Option<u32>,
    /// Wear material parameters [roughness, metallic, unused, strength].
    pub wear_material: Option<[f32; 4]>,
    /// Normal strength multiplier.
    pub normal_strength: Option<f32>,
    /// Roughness parameters [base, offset, scale].
    pub roughness: Option<[f32; 3]>,
    /// Macro relief scale (affects parallax).
    pub macro_relief: Option<f32>,
    /// Alpha mask mode (for foliage, etc).
    pub alpha_mask: Option<bool>,
    /// Cloth parameters [transmission, thickness, roughness, unused].
    pub cloth: Option<[f32; 4]>,
}

/// Three.js material options.
#[derive(Debug, Clone, Copy)]
pub struct ThreeOptions {
    /// Material side (1=front, 2=double, 3=back).
    pub side: Option<u32>,
    /// Anisotropy strength (for brushed metal).
    pub anisotropy: Option<f32>,
    /// Anisotropy rotation.
    pub anisotropy_rotation: Option<f32>,
    /// Use physical material model.
    pub physical: Option<bool>,
    /// Double-sided rendering.
    pub double_sided: Option<bool>,
    /// Sheen strength (fabric, canvas).
    pub sheen: Option<f32>,
    /// Sheen roughness.
    pub sheen_roughness: Option<f32>,
    /// Sheen colour (hex RGB).
    pub sheen_color: Option<u32>,
    /// Alpha test threshold.
    pub alpha_test: Option<f32>,
    /// Opacity (0-1, for transparent materials).
    pub opacity: Option<f32>,
    /// Environment map intensity.
    pub env_map_intensity: Option<f32>,
    /// Index of refraction (for glass).
    pub ior: Option<f32>,
    /// Specular intensity.
    pub specular_intensity: Option<f32>,
    /// Disable depth writing (for transparent materials).
    pub depth_write: Option<bool>,
}

/// A complete material library entry.
#[derive(Debug, Clone)]
pub struct LibraryEntry {
    /// Canonical name (matches JavaScript key).
    pub name: &'static str,
    /// Identifier for the GLSL texture generator used.
    pub generator: &'static str,
    /// Physics surface tag.
    pub surface: Surface,
    /// Texture baking parameters.
    pub bake: BakeParams,
    /// Material rendering parameters.
    pub mat: MatParams,
    /// Three.js options (for the browser renderer).
    pub three: Option<ThreeOptions>,
}

/// The 19-entry material library.
pub const LIBRARY: &[LibraryEntry] = &[
    LibraryEntry {
        name: "concrete",
        generator: "concrete",
        surface: Surface::Concrete,
        bake: BakeParams {
            size: 1024,
            world_size: 2.5,
            relief: 0.09,
            seed: 11,
            param: [1.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 2.5,
            parallax: Some(0.016),
            parallax_layers: None,
            detile: Some(0.4),
            detail: Some([9.0, 0.95, 0.58, 26.0]),
            macro_: Some([0.085, 0.62, 0.24, 0.45]),
            macro_big: Some([2.05, 0.130, 0.028, 0.0]),
            patch: Some([0.28, 2.0, 0.145, -0.08]),
            weather: Some([0.42, 0.4, 0.55, 0.5]),
            wear_color: Some(0x9a978f),
            dust_color: Some(0x8b7f6a),
            grime_color: Some(0x2b2823),
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([0.98, -0.01, 0.24]),
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "concrete_floor",
        generator: "concrete",
        surface: Surface::Concrete,
        bake: BakeParams {
            size: 1024,
            world_size: 2.5,
            relief: 0.075,
            seed: 47,
            param: [0.0, 1.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 3.2,
            parallax: Some(0.01),
            parallax_layers: None,
            detile: Some(0.0),
            detail: Some([9.0, 0.90, 0.52, 26.0]),
            macro_: Some([0.075, 0.48, 0.18, 0.3]),
            macro_big: None,
            patch: None,
            weather: Some([0.55, 0.1, 0.15, 0.5]),
            wear_color: None,
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([1.0, 0.0, 0.22]),
            macro_relief: Some(0.3),
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "brick",
        generator: "brick",
        surface: Surface::Concrete,
        bake: BakeParams {
            size: 1024,
            world_size: 1.35,
            relief: 0.055,
            seed: 23,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 1.35,
            parallax: Some(0.024),
            parallax_layers: Some(24),
            detile: Some(0.0),
            detail: Some([7.0, 0.88, 0.48, 22.0]),
            macro_: Some([0.09, 0.58, 0.22, 0.55]),
            macro_big: Some([1.95, 0.115, 0.03, 0.0]),
            patch: None,
            weather: Some([0.4, 0.5, 0.6, 0.55]),
            wear_color: Some(0xa08678),
            dust_color: None,
            grime_color: Some(0x241f19),
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([0.98, -0.01, 0.26]),
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "plaster",
        generator: "plaster",
        surface: Surface::Plaster,
        bake: BakeParams {
            size: 1024,
            world_size: 2.2,
            relief: 0.06,
            seed: 5,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 2.2,
            parallax: Some(0.014),
            parallax_layers: None,
            detile: Some(0.8),
            detail: Some([10.0, 0.95, 0.54, 24.0]),
            macro_: Some([0.085, 0.72, 0.26, 0.5]),
            macro_big: Some([2.15, 0.150, 0.026, 0.0]),
            patch: Some([0.34, 2.2, 0.175, -0.10]),
            weather: Some([0.34, 0.5, 0.6, 0.5]),
            wear_color: Some(0xb0a692),
            dust_color: Some(0x9c8a6c),
            grime_color: Some(0x2a251d),
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([0.97, -0.02, 0.26]),
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "tile",
        generator: "tile",
        surface: Surface::Concrete,
        bake: BakeParams {
            size: 1024,
            world_size: 1.5,
            relief: 0.03,
            seed: 31,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 1.5,
            parallax: Some(0.03),
            parallax_layers: Some(20),
            detile: None,
            detail: Some([8.0, 0.6, 0.36, 18.0]),
            macro_: Some([0.09, 0.40, 0.16, 0.3]),
            macro_big: Some([1.7, 0.075, 0.032, 0.0]),
            patch: Some([0.14, 1.7, 0.10, -0.05]),
            weather: Some([0.3, 0.2, 0.3, 0.5]),
            wear_color: None,
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([0.9, -0.04, 0.16]),
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "asphalt",
        generator: "asphalt",
        surface: Surface::Concrete,
        bake: BakeParams {
            size: 1024,
            world_size: 3.0,
            relief: 0.075,
            seed: 71,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 3.0,
            parallax: Some(0.014),
            parallax_layers: None,
            detile: Some(1.0),
            detail: Some([8.0, 0.8, 0.42, 18.0]),
            macro_: Some([0.062, 0.52, 0.22, 0.25]),
            macro_big: None,
            patch: None,
            weather: Some([0.45, 0.05, 0.1, 0.26]),
            wear_color: None,
            dust_color: Some(0x8b8071),
            grime_color: Some(0x232120),
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([0.98, -0.02, 0.3]),
            macro_relief: Some(0.55),
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "sand",
        generator: "sand",
        surface: Surface::Sand,
        bake: BakeParams {
            size: 1024,
            world_size: 2.5,
            relief: 0.10,
            seed: 91,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: Some("triplanar"),
            scale: 2.5,
            parallax: None,
            parallax_layers: None,
            detile: Some(0.0),
            detail: Some([8.0, 0.7, 0.30, 18.0]),
            macro_: Some([0.050, 0.44, 0.14, 0.35]),
            macro_big: None,
            patch: None,
            weather: Some([0.15, 0.0, 0.0, 0.18]),
            wear_color: None,
            dust_color: Some(0xa89066),
            grime_color: Some(0x4c4132),
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([1.0, 0.0, 0.3]),
            macro_relief: Some(0.45),
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "dirt",
        generator: "dirt",
        surface: Surface::Dirt,
        bake: BakeParams {
            size: 1024,
            world_size: 2.5,
            relief: 0.12,
            seed: 13,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: Some("triplanar"),
            scale: 2.5,
            parallax: None,
            parallax_layers: None,
            detile: None,
            detail: Some([7.0, 0.85, 0.36, 18.0]),
            macro_: Some([0.055, 0.48, 0.18, 0.4]),
            macro_big: None,
            patch: None,
            weather: Some([0.2, 0.0, 0.0, 0.22]),
            wear_color: None,
            dust_color: Some(0x94805c),
            grime_color: Some(0x37301f),
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([0.98, -0.02, 0.3]),
            macro_relief: Some(0.6),
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "gravel",
        generator: "gravel",
        surface: Surface::Dirt,
        bake: BakeParams {
            size: 1024,
            world_size: 1.6,
            relief: 0.055,
            seed: 57,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: Some("triplanar"),
            scale: 1.6,
            parallax: None,
            parallax_layers: None,
            detile: None,
            detail: Some([6.0, 0.8, 0.34, 20.0]),
            macro_: Some([0.070, 0.44, 0.2, 0.3]),
            macro_big: None,
            patch: None,
            weather: Some([0.2, 0.0, 0.0, 0.16]),
            wear_color: None,
            dust_color: Some(0xa2947a),
            grime_color: Some(0x4a4238),
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: Some([0.96, -0.03, 0.28]),
            macro_relief: Some(0.7),
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "metal_rust",
        generator: "metal_rust",
        surface: Surface::Metal,
        bake: BakeParams {
            size: 1024,
            world_size: 1.2,
            relief: 0.035,
            seed: 37,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 1.2,
            parallax: Some(0.004),
            parallax_layers: None,
            detile: None,
            detail: Some([9.0, 0.7, 0.36, 16.0]),
            macro_: Some([0.10, 0.30, 0.14, 0.4]),
            macro_big: None,
            patch: None,
            weather: Some([0.25, 0.4, 0.5, 0.35]),
            wear_color: Some(0x8c8f93),
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: Some([0.28, 1.0, 0.0, 0.85]),
            normal_strength: None,
            roughness: None,
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "metal_painted",
        generator: "metal_painted",
        surface: Surface::Metal,
        bake: BakeParams {
            size: 1024,
            world_size: 1.5,
            relief: 0.018,
            seed: 61,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: Some(0x4a5340),
            tint_b: Some(0x2a2f26),
        },
        mat: MatParams {
            uv_mode: None,
            scale: 1.5,
            parallax: Some(0.003),
            parallax_layers: None,
            detile: None,
            detail: Some([10.0, 0.6, 0.32, 16.0]),
            macro_: Some([0.10, 0.28, 0.14, 0.35]),
            macro_big: None,
            patch: None,
            weather: Some([0.3, 0.45, 0.35, 0.35]),
            wear_color: Some(0x8f9296),
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: Some([0.3, 1.0, 0.0, 0.9]),
            normal_strength: None,
            roughness: Some([0.92, -0.03, 0.22]),
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "metal_brushed",
        generator: "metal_brushed",
        surface: Surface::Metal,
        bake: BakeParams {
            size: 512,
            world_size: 0.8,
            relief: 0.004,
            seed: 83,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 0.8,
            parallax: None,
            parallax_layers: None,
            detile: None,
            detail: Some([8.0, 0.25, 0.15, 8.0]),
            macro_: Some([0.09, 0.14, 0.1, 0.2]),
            macro_big: None,
            patch: None,
            weather: Some([0.15, 0.15, 0.2, 0.2]),
            wear_color: Some(0xb9bcc0),
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: Some([0.16, 1.0, 0.0, 0.9]),
            normal_strength: None,
            roughness: None,
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: Some(ThreeOptions {
            side: None,
            anisotropy: Some(0.65),
            anisotropy_rotation: Some(0.0),
            physical: Some(true),
            double_sided: None,
            sheen: None,
            sheen_roughness: None,
            sheen_color: None,
            alpha_test: None,
            opacity: None,
            env_map_intensity: None,
            ior: None,
            specular_intensity: None,
            depth_write: None,
        }),
    },
    LibraryEntry {
        name: "corrugated",
        generator: "corrugated",
        surface: Surface::Metal,
        bake: BakeParams {
            size: 1024,
            world_size: 2.4,
            relief: 0.075,
            seed: 29,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 2.4,
            parallax: Some(0.03),
            parallax_layers: Some(24),
            detile: None,
            detail: Some([10.0, 0.6, 0.32, 18.0]),
            macro_: Some([0.09, 0.26, 0.12, 0.3]),
            macro_big: None,
            patch: None,
            weather: Some([0.3, 0.5, 0.5, 0.4]),
            wear_color: Some(0x9aa0a4),
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: Some([0.32, 1.0, 0.0, 0.85]),
            normal_strength: None,
            roughness: None,
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "wood",
        generator: "wood",
        surface: Surface::Wood,
        bake: BakeParams {
            size: 1024,
            world_size: 2.0,
            relief: 0.038,
            seed: 19,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 2.0,
            parallax: Some(0.008),
            parallax_layers: None,
            detile: None,
            detail: Some([10.0, 0.8, 0.42, 18.0]),
            macro_: Some([0.085, 0.34, 0.14, 0.5]),
            macro_big: None,
            patch: None,
            weather: Some([0.3, 0.35, 0.5, 0.45]),
            wear_color: Some(0xa88b62),
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: Some([0.5, 0.0, 0.0, 0.7]),
            normal_strength: None,
            roughness: None,
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "fabric",
        generator: "fabric",
        surface: Surface::Fabric,
        bake: BakeParams {
            size: 512,
            world_size: 0.7,
            relief: 0.008,
            seed: 43,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: Some(0x5a5445),
            tint_b: Some(0x3a3830),
        },
        mat: MatParams {
            uv_mode: None,
            scale: 0.7,
            parallax: None,
            parallax_layers: None,
            detile: None,
            detail: Some([6.0, 0.42, 0.28, 10.0]),
            macro_: Some([0.12, 0.34, 0.12, 0.3]),
            macro_big: Some([1.8, 0.07, 0.09, 0.0]),
            patch: None,
            weather: Some([0.25, 0.2, 0.3, 0.35]),
            wear_color: None,
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: None,
            normal_strength: Some(1.15),
            roughness: None,
            macro_relief: None,
            alpha_mask: None,
            cloth: Some([0.20, 0.72, 0.26, 0.0]),
        },
        three: Some(ThreeOptions {
            side: None,
            anisotropy: None,
            anisotropy_rotation: None,
            physical: Some(true),
            double_sided: None,
            sheen: Some(0.55),
            sheen_roughness: Some(0.85),
            sheen_color: Some(0x8a8272),
            alpha_test: None,
            opacity: None,
            env_map_intensity: None,
            ior: None,
            specular_intensity: None,
            depth_write: None,
        }),
    },
    LibraryEntry {
        name: "burlap",
        generator: "burlap",
        surface: Surface::Fabric,
        bake: BakeParams {
            size: 512,
            world_size: 0.5,
            relief: 0.018,
            seed: 67,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 0.5,
            parallax: Some(0.003),
            parallax_layers: None,
            detile: None,
            detail: Some([6.0, 0.4, 0.28, 9.0]),
            macro_: Some([0.14, 0.32, 0.12, 0.35]),
            macro_big: Some([1.7, 0.06, 0.11, 0.0]),
            patch: None,
            weather: Some([0.4, 0.15, 0.35, 0.4]),
            wear_color: None,
            dust_color: Some(0x9c8760),
            grime_color: None,
            tint: None,
            wear_material: None,
            normal_strength: Some(1.15),
            roughness: None,
            macro_relief: None,
            alpha_mask: None,
            cloth: Some([0.06, 0.86, 0.10, 0.0]),
        },
        three: Some(ThreeOptions {
            side: None,
            anisotropy: None,
            anisotropy_rotation: None,
            physical: Some(true),
            double_sided: None,
            sheen: Some(0.4),
            sheen_roughness: Some(0.95),
            sheen_color: Some(0x9c8b68),
            alpha_test: None,
            opacity: None,
            env_map_intensity: None,
            ior: None,
            specular_intensity: None,
            depth_write: None,
        }),
    },
    LibraryEntry {
        name: "foliage",
        generator: "foliage",
        surface: Surface::Foliage,
        bake: BakeParams {
            size: 512,
            world_size: 0.6,
            relief: 0.02,
            seed: 79,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: Some("mesh"),
            scale: 1.0,
            parallax: None,
            parallax_layers: None,
            detile: None,
            detail: Some([4.0, 0.25, 0.15, 8.0]),
            macro_: Some([0.16, 0.3, 0.08, 0.6]),
            macro_big: None,
            patch: None,
            weather: Some([0.15, 0.0, 0.0, 0.2]),
            wear_color: None,
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: None,
            normal_strength: None,
            roughness: None,
            macro_relief: None,
            alpha_mask: Some(true),
            cloth: None,
        },
        three: Some(ThreeOptions {
            side: Some(2), // THREE.DoubleSide
            anisotropy: None,
            anisotropy_rotation: None,
            physical: Some(true),
            double_sided: None,
            sheen: Some(0.3),
            sheen_roughness: Some(0.8),
            sheen_color: Some(0x9fbd6a),
            alpha_test: Some(0.45),
            opacity: None,
            env_map_intensity: None,
            ior: None,
            specular_intensity: None,
            depth_write: None,
        }),
    },
    LibraryEntry {
        name: "rubber",
        generator: "rubber",
        surface: Surface::Rubber,
        bake: BakeParams {
            size: 512,
            world_size: 0.5,
            relief: 0.013,
            seed: 97,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 0.45,
            parallax: None,
            parallax_layers: None,
            detile: None,
            detail: Some([7.0, 0.62, 0.42, 13.0]),
            macro_: Some([0.16, 0.36, 0.20, 0.18]),
            macro_big: Some([1.8, 0.10, 0.11, 0.0]),
            patch: None,
            weather: Some([0.40, 0.18, 0.22, 0.45]),
            wear_color: None,
            dust_color: Some(0x8d8478),
            grime_color: Some(0x181715),
            tint: Some(0xfffaf2),
            wear_material: None,
            normal_strength: Some(1.25),
            roughness: Some([0.94, -0.03, 0.34]),
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: None,
    },
    LibraryEntry {
        name: "glass",
        generator: "glass",
        surface: Surface::Glass,
        bake: BakeParams {
            size: 512,
            world_size: 2.0,
            relief: 0.0008,
            seed: 3,
            param: [0.0, 0.0, 0.0, 0.0],
            tint_a: None,
            tint_b: None,
        },
        mat: MatParams {
            uv_mode: None,
            scale: 2.0,
            parallax: None,
            parallax_layers: None,
            detile: None,
            detail: Some([3.0, 0.06, 0.05, 6.0]),
            macro_: Some([0.05, 0.1, 0.06, 0.1]),
            macro_big: None,
            patch: None,
            weather: Some([0.1, 0.3, 0.4, 0.15]),
            wear_color: None,
            dust_color: None,
            grime_color: None,
            tint: None,
            wear_material: None,
            normal_strength: Some(0.35),
            roughness: Some([0.9, -0.01, 0.03]),
            macro_relief: None,
            alpha_mask: None,
            cloth: None,
        },
        three: Some(ThreeOptions {
            side: Some(2), // THREE.DoubleSide
            anisotropy: None,
            anisotropy_rotation: None,
            physical: Some(true),
            double_sided: None,
            sheen: None,
            sheen_roughness: None,
            sheen_color: None,
            alpha_test: None,
            opacity: Some(0.22),
            env_map_intensity: Some(1.6),
            ior: Some(1.52),
            specular_intensity: Some(1.0),
            depth_write: Some(false),
        }),
    },
];

/// Alias mapping: user-friendly name → canonical library key.
pub const ALIASES: &[(&str, &str)] = &[
    ("metal", "metal_painted"),
    ("steel", "metal_brushed"),
    ("rust", "metal_rust"),
    ("sandbag", "burlap"),
    ("ground", "dirt"),
    ("road", "asphalt"),
    ("stucco", "plaster"),
    ("wall", "concrete"),
    ("floor", "concrete_floor"),
    ("plank", "wood"),
    ("leaf", "foliage"),
    ("window", "glass"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_has_nineteen_surfaces() {
        assert_eq!(
            LIBRARY.len(),
            19,
            "library must contain exactly 19 surfaces"
        );
    }

    #[test]
    fn all_surfaces_have_canonical_names() {
        let names = LIBRARY.iter().map(|e| e.name).collect::<Vec<_>>();
        let expected = vec![
            "concrete",
            "concrete_floor",
            "brick",
            "plaster",
            "tile",
            "asphalt",
            "sand",
            "dirt",
            "gravel",
            "metal_rust",
            "metal_painted",
            "metal_brushed",
            "corrugated",
            "wood",
            "fabric",
            "burlap",
            "foliage",
            "rubber",
            "glass",
        ];
        assert_eq!(names, expected, "surface names must match expected order");
    }

    #[test]
    fn aliases_resolve_to_real_entries() {
        let lib_names: std::collections::HashSet<_> =
            LIBRARY.iter().map(|e| e.name).collect();

        for (alias, target) in ALIASES {
            assert!(
                lib_names.contains(target),
                "alias '{}' -> '{}' but '{}' not in library",
                alias,
                target,
                target
            );
        }
    }

    #[test]
    fn all_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (alias, _) in ALIASES {
            assert!(
                seen.insert(alias),
                "alias '{}' appears more than once",
                alias
            );
        }
    }

    #[test]
    fn concrete_exact_bake_params() {
        let concrete = &LIBRARY[0];
        assert_eq!(concrete.name, "concrete");
        assert_eq!(concrete.bake.size, 1024);
        assert_eq!(concrete.bake.world_size, 2.5);
        assert_eq!(concrete.bake.relief, 0.09);
        assert_eq!(concrete.bake.seed, 11);
        assert_eq!(concrete.bake.param, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn metal_painted_tints() {
        let metal_painted = &LIBRARY[10];
        assert_eq!(metal_painted.name, "metal_painted");
        assert_eq!(metal_painted.bake.tint_a, Some(0x4a5340));
        assert_eq!(metal_painted.bake.tint_b, Some(0x2a2f26));
    }

    #[test]
    fn glass_exact_values() {
        let glass = &LIBRARY[18];
        assert_eq!(glass.name, "glass");
        assert_eq!(glass.bake.size, 512);
        assert_eq!(glass.bake.world_size, 2.0);
        assert_eq!(glass.bake.relief, 0.0008);
        assert_eq!(glass.bake.seed, 3);

        let three = glass.three.as_ref().expect("glass must have three options");
        assert_eq!(three.opacity, Some(0.22));
        assert_eq!(three.ior, Some(1.52));
        assert_eq!(three.env_map_intensity, Some(1.6));
        assert_eq!(three.depth_write, Some(false));
    }
}
