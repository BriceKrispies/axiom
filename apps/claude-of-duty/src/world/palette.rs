//! Ported from Claude-of-Duty `src/world/palette.js:1-390`.
//!
//! The world palette: a named set of material variants pulled from the materials library.
//! Keeping them in one table means the level uses a deliberate, limited palette
//! (which is what makes a real map read as one place) and that every mesh sharing a key
//! merges into the same draw call.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Concrete,
    Metal,
    Wood,
    Dirt,
    Sand,
    Glass,
    Water,
    Foliage,
    Fabric,
    Flesh,
    Rubber,
    Plaster,
}

/// The fixed twelve-entry ordering the physics BVH's per-triangle byte index
/// packs against (`Claude-of-Duty src/physics/surfaces.js:14-27`,
/// `SURFACE_NAMES`). This enum's declaration order already matches that list
/// exactly, so `Surface::ALL[i as usize]` round-trips the source's
/// `SURFACE[name] -> index -> SURFACE_NAMES[index]` chain — see
/// `apps/claude-of-duty/src/physics/surfaces.rs`, which reuses this enum
/// rather than defining a second surface-tag type.
impl Surface {
    pub const ALL: [Surface; 12] = [
        Surface::Concrete,
        Surface::Metal,
        Surface::Wood,
        Surface::Dirt,
        Surface::Sand,
        Surface::Glass,
        Surface::Water,
        Surface::Foliage,
        Surface::Fabric,
        Surface::Flesh,
        Surface::Rubber,
        Surface::Plaster,
    ];

    /// The source's `SURFACE_NAMES[i]` (`surfaces.js:14-27`).
    pub const fn index(self) -> u8 {
        match self {
            Surface::Concrete => 0,
            Surface::Metal => 1,
            Surface::Wood => 2,
            Surface::Dirt => 3,
            Surface::Sand => 4,
            Surface::Glass => 5,
            Surface::Water => 6,
            Surface::Foliage => 7,
            Surface::Fabric => 8,
            Surface::Flesh => 9,
            Surface::Rubber => 10,
            Surface::Plaster => 11,
        }
    }

    /// The source's `surfaceName(i)` (`surfaces.js:104-106`), except the
    /// out-of-range fallback is the caller's problem here: this takes an
    /// already-valid index (`0..12`) and panics on garbage rather than
    /// silently returning `'concrete'`, which is what `surfaceIndex` guards
    /// against before a lookup ever reaches this table.
    pub const fn from_index(i: u8) -> Surface {
        Surface::ALL[i as usize]
    }

    /// The source's `SURFACE_NAMES[i]` string form, used for name-based
    /// surface inference (`surfaces.js:14-27`).
    pub const fn name(self) -> &'static str {
        match self {
            Surface::Concrete => "concrete",
            Surface::Metal => "metal",
            Surface::Wood => "wood",
            Surface::Dirt => "dirt",
            Surface::Sand => "sand",
            Surface::Glass => "glass",
            Surface::Water => "water",
            Surface::Foliage => "foliage",
            Surface::Fabric => "fabric",
            Surface::Flesh => "flesh",
            Surface::Rubber => "rubber",
            Surface::Plaster => "plaster",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThreeOptions {
    pub side: Option<u32>,
    pub emissive: Option<u32>,
    pub emissive_intensity: Option<f32>,
    pub tone_mapped: Option<bool>,
    pub opacity: Option<f32>,
    pub env_map_intensity: Option<f32>,
}

impl Default for ThreeOptions {
    fn default() -> Self {
        Self {
            side: None,
            emissive: None,
            emissive_intensity: None,
            tone_mapped: None,
            opacity: None,
            env_map_intensity: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaletteEntryOpts {
    pub vertex_masks: Option<bool>,
    pub tint: Option<u32>,
    pub scale: f32,
    pub normal_strength: Option<f32>,
    pub weather: Option<[f32; 4]>,
    pub wear: Option<[f32; 4]>,
    pub detile: Option<f32>,
    pub roughness: Option<[f32; 2]>,
    pub three: Option<ThreeOptions>,
}

#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub name: &'static str,
    pub surface: Surface,
    pub opts: PaletteEntryOpts,
}

pub struct Palette;

impl Palette {
    pub const PLASTER_CREAM: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xcfc0a4),
            scale: 2.35,
            normal_strength: None,
            weather: Some([0.4, 0.5, 1.4, 0.55]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const PLASTER_SAND: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xb9a582),
            scale: 2.1,
            normal_strength: None,
            weather: Some([0.45, 0.5, 1.5, 0.6]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const PLASTER_BLUE: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x8f9aa0),
            scale: 2.2,
            normal_strength: None,
            weather: Some([0.4, 0.55, 1.5, 0.6]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const PLASTER_PINK: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xc09a86),
            scale: 2.5,
            normal_strength: None,
            weather: Some([0.45, 0.5, 1.3, 0.55]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const PLASTER_WHITE: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xd8d2c4),
            scale: 1.9,
            normal_strength: None,
            weather: Some([0.3, 0.35, 0.9, 0.5]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const BRICK: PaletteEntry = PaletteEntry {
        name: "brick",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xa8846c),
            scale: 1.3,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const BRICK_FINE: PaletteEntry = PaletteEntry {
        name: "brick",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x9c8068),
            scale: 0.62,
            normal_strength: None,
            weather: Some([0.45, 0.5, 0.8, 0.6]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const CONCRETE: PaletteEntry = PaletteEntry {
        name: "concrete",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xa9a49a),
            scale: 2.5,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    // Prop-scale concrete. A 2.5 m texture tile across a 0.5 m block shows a
    // single smear of noise and reads as untextured plastic; small objects need
    // their own, much tighter tiling.
    pub const CONCRETE_PROP: PaletteEntry = PaletteEntry {
        name: "concrete",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xa5a096),
            scale: 0.9,
            normal_strength: Some(1.3),
            weather: Some([0.45, 0.5, 0.35, 0.55]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const CONCRETE_DARK: PaletteEntry = PaletteEntry {
        name: "concrete",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x7d7a73),
            scale: 2.2,
            normal_strength: None,
            weather: Some([0.4, 0.6, 1.2, 0.6]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const ROOF_SCREED: PaletteEntry = PaletteEntry {
        name: "concrete",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xb5a992),
            scale: 2.8,
            normal_strength: None,
            weather: Some([0.6, 0.2, 0.3, 0.45]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const FLOOR_CONCRETE: PaletteEntry = PaletteEntry {
        name: "concrete_floor",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x9e9a91),
            scale: 3.0,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const TILE_FLOOR: PaletteEntry = PaletteEntry {
        name: "tile",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xa9a08d),
            scale: 1.4,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const ROAD_DUST: PaletteEntry = PaletteEntry {
        name: "gravel",
        surface: Surface::Dirt,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xc9b896),
            scale: 2.2,
            normal_strength: None,
            weather: Some([0.4, 0.04, 0.08, 0.14]),
            wear: Some([0.0, 0.5, 0.45, 0.0]),
            detile: Some(0.9),
            roughness: None,
            three: None,
        },
    };

    pub const ASPHALT: PaletteEntry = PaletteEntry {
        name: "asphalt",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x9d968a),
            scale: 3.2,
            normal_strength: None,
            weather: None,
            wear: Some([0.0, 0.55, 0.45, 0.0]),
            detile: Some(0.6),
            roughness: None,
            three: None,
        },
    };

    pub const ROAD_RUT: PaletteEntry = PaletteEntry {
        name: "asphalt",
        surface: Surface::Concrete,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x6f6a62),
            scale: 1.5,
            normal_strength: None,
            weather: Some([0.3, 0.5, 0.15, 0.28]),
            wear: Some([0.0, 0.55, 0.45, 0.0]),
            detile: Some(0.7),
            roughness: None,
            three: None,
        },
    };

    pub const SAND: PaletteEntry = PaletteEntry {
        name: "sand",
        surface: Surface::Sand,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 2.6,
            normal_strength: None,
            weather: None,
            wear: Some([0.0, 0.45, 0.45, 0.0]),
            detile: Some(0.7),
            roughness: None,
            three: None,
        },
    };

    pub const DIRT: PaletteEntry = PaletteEntry {
        name: "dirt",
        surface: Surface::Dirt,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 2.4,
            normal_strength: None,
            weather: None,
            wear: Some([0.0, 0.5, 0.45, 0.0]),
            detile: Some(0.8),
            roughness: None,
            three: None,
        },
    };

    pub const GRAVEL: PaletteEntry = PaletteEntry {
        name: "gravel",
        surface: Surface::Dirt,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 1.8,
            normal_strength: None,
            weather: None,
            wear: Some([0.0, 0.5, 0.45, 0.0]),
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const DUST_SKIRT: PaletteEntry = PaletteEntry {
        name: "gravel",
        surface: Surface::Dirt,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xa89d86),
            scale: 1.1,
            normal_strength: None,
            weather: Some([0.3, 0.0, 0.0, 0.16]),
            wear: Some([0.0, 0.9, 0.7, 0.0]),
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const METAL_RUST: PaletteEntry = PaletteEntry {
        name: "metal_rust",
        surface: Surface::Metal,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 1.1,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const METAL_RUST_PROP: PaletteEntry = PaletteEntry {
        name: "metal_rust",
        surface: Surface::Metal,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x9d7c66),
            scale: 0.4,
            normal_strength: Some(1.35),
            weather: Some([0.5, 0.35, 0.3, 0.5]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const METAL_BLUE: PaletteEntry = PaletteEntry {
        name: "metal_painted",
        surface: Surface::Metal,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x6d8390),
            scale: 1.3,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const METAL_GREEN: PaletteEntry = PaletteEntry {
        name: "metal_painted",
        surface: Surface::Metal,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x76806a),
            scale: 1.3,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const METAL_DARK: PaletteEntry = PaletteEntry {
        name: "metal_painted",
        surface: Surface::Metal,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x4a4a48),
            scale: 1.0,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const STEEL: PaletteEntry = PaletteEntry {
        name: "metal_brushed",
        surface: Surface::Metal,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 0.9,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const CORRUGATED: PaletteEntry = PaletteEntry {
        name: "corrugated",
        surface: Surface::Metal,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 2.2,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const WOOD: PaletteEntry = PaletteEntry {
        name: "wood",
        surface: Surface::Wood,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 1.8,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const WOOD_PROP: PaletteEntry = PaletteEntry {
        name: "wood",
        surface: Surface::Wood,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xb08a5e),
            scale: 0.55,
            normal_strength: Some(1.45),
            weather: Some([0.35, 0.3, 0.35, 0.5]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const WOOD_PROP_DARK: PaletteEntry = PaletteEntry {
        name: "wood",
        surface: Surface::Wood,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x7d6244),
            scale: 0.5,
            normal_strength: Some(1.45),
            weather: Some([0.35, 0.35, 0.4, 0.55]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const WOOD_DARK: PaletteEntry = PaletteEntry {
        name: "wood",
        surface: Surface::Wood,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x8a6a4a),
            scale: 1.5,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const WOOD_PALE: PaletteEntry = PaletteEntry {
        name: "wood",
        surface: Surface::Wood,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xc0a482),
            scale: 1.2,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const FABRIC_RED: PaletteEntry = PaletteEntry {
        name: "fabric",
        surface: Surface::Fabric,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xa2564a),
            scale: 0.26,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: Some(ThreeOptions {
                side: Some(2),
                emissive: None,
                emissive_intensity: None,
                tone_mapped: None,
                opacity: None,
                env_map_intensity: None,
            }),
        },
    };

    pub const FABRIC_TEAL: PaletteEntry = PaletteEntry {
        name: "fabric",
        surface: Surface::Fabric,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x5f8a8c),
            scale: 0.26,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: Some(ThreeOptions {
                side: Some(2),
                emissive: None,
                emissive_intensity: None,
                tone_mapped: None,
                opacity: None,
                env_map_intensity: None,
            }),
        },
    };

    pub const FABRIC_CREAM: PaletteEntry = PaletteEntry {
        name: "fabric",
        surface: Surface::Fabric,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xbcb298),
            scale: 0.26,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: Some(ThreeOptions {
                side: Some(2),
                emissive: None,
                emissive_intensity: None,
                tone_mapped: None,
                opacity: None,
                env_map_intensity: None,
            }),
        },
    };

    pub const BURLAP: PaletteEntry = PaletteEntry {
        name: "burlap",
        surface: Surface::Fabric,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0xa2957a),
            scale: 0.16,
            normal_strength: None,
            weather: Some([0.5, 0.3, 0.4, 0.5]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const RUBBER: PaletteEntry = PaletteEntry {
        name: "rubber",
        surface: Surface::Rubber,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 0.45,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const GLASS: PaletteEntry = PaletteEntry {
        name: "glass",
        surface: Surface::Glass,
        opts: PaletteEntryOpts {
            vertex_masks: None,
            tint: None,
            scale: 2.0,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const FOLIAGE: PaletteEntry = PaletteEntry {
        name: "foliage",
        surface: Surface::Foliage,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: None,
            scale: 0.0, // Scale is required per source; foliage has no explicit scale.
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const WINDOW_VOID: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x474441),
            scale: 1.1,
            normal_strength: None,
            weather: Some([0.2, 0.7, 0.2, 0.7]),
            wear: None,
            detile: None,
            roughness: Some([1.0, 0.15]),
            three: None,
        },
    };

    pub const INTERIOR_SHELL: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x5f5b56),
            scale: 1.6,
            normal_strength: None,
            weather: Some([0.25, 0.8, 0.3, 0.65]),
            wear: None,
            detile: None,
            roughness: Some([1.0, 0.1]),
            three: None,
        },
    };

    pub const WINDOW_GLASS: PaletteEntry = PaletteEntry {
        name: "glass",
        surface: Surface::Glass,
        opts: PaletteEntryOpts {
            vertex_masks: None,
            tint: None,
            scale: 2.0,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: Some([0.3, 0.06]),
            three: Some(ThreeOptions {
                side: None,
                emissive: None,
                emissive_intensity: None,
                tone_mapped: None,
                opacity: Some(0.16),
                env_map_intensity: Some(2.1),
            }),
        },
    };

    pub const PLYWOOD: PaletteEntry = PaletteEntry {
        name: "wood",
        surface: Surface::Wood,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x7a6549),
            scale: 0.62,
            normal_strength: Some(1.2),
            weather: Some([0.5, 0.45, 0.5, 0.6]),
            wear: None,
            detile: None,
            roughness: None,
            three: None,
        },
    };

    pub const EMISSIVE_WARM: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Glass,
        opts: PaletteEntryOpts {
            vertex_masks: None,
            tint: Some(0xfff0d8),
            scale: 0.4,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: Some(ThreeOptions {
                side: None,
                emissive: Some(0xffd39a),
                emissive_intensity: Some(12.0),
                tone_mapped: Some(true),
                opacity: None,
                env_map_intensity: None,
            }),
        },
    };

    pub const WINDOW_GLOW: PaletteEntry = PaletteEntry {
        name: "plaster",
        surface: Surface::Plaster,
        opts: PaletteEntryOpts {
            vertex_masks: Some(true),
            tint: Some(0x6a5a45),
            scale: 1.2,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: Some(ThreeOptions {
                side: None,
                emissive: Some(0xffb066),
                emissive_intensity: Some(1.1),
                tone_mapped: Some(true),
                opacity: None,
                env_map_intensity: None,
            }),
        },
    };

    pub const LAMP_LENS: PaletteEntry = PaletteEntry {
        name: "glass",
        surface: Surface::Glass,
        opts: PaletteEntryOpts {
            vertex_masks: None,
            tint: None,
            scale: 1.0,
            normal_strength: None,
            weather: None,
            wear: None,
            detile: None,
            roughness: None,
            three: Some(ThreeOptions {
                side: None,
                emissive: Some(0xffc47a),
                emissive_intensity: Some(0.0),
                tone_mapped: None,
                opacity: Some(0.5),
                env_map_intensity: None,
            }),
        },
    };

    pub const ALL: &'static [(&'static str, &'static PaletteEntry)] = &[
        ("plaster_cream", &Self::PLASTER_CREAM),
        ("plaster_sand", &Self::PLASTER_SAND),
        ("plaster_blue", &Self::PLASTER_BLUE),
        ("plaster_pink", &Self::PLASTER_PINK),
        ("plaster_white", &Self::PLASTER_WHITE),
        ("brick", &Self::BRICK),
        ("brick_fine", &Self::BRICK_FINE),
        ("concrete", &Self::CONCRETE),
        ("concrete_prop", &Self::CONCRETE_PROP),
        ("concrete_dark", &Self::CONCRETE_DARK),
        ("roof_screed", &Self::ROOF_SCREED),
        ("floor_concrete", &Self::FLOOR_CONCRETE),
        ("tile_floor", &Self::TILE_FLOOR),
        ("road_dust", &Self::ROAD_DUST),
        ("asphalt", &Self::ASPHALT),
        ("road_rut", &Self::ROAD_RUT),
        ("sand", &Self::SAND),
        ("dirt", &Self::DIRT),
        ("gravel", &Self::GRAVEL),
        ("dust_skirt", &Self::DUST_SKIRT),
        ("metal_rust", &Self::METAL_RUST),
        ("metal_rust_prop", &Self::METAL_RUST_PROP),
        ("metal_blue", &Self::METAL_BLUE),
        ("metal_green", &Self::METAL_GREEN),
        ("metal_dark", &Self::METAL_DARK),
        ("steel", &Self::STEEL),
        ("corrugated", &Self::CORRUGATED),
        ("wood", &Self::WOOD),
        ("wood_prop", &Self::WOOD_PROP),
        ("wood_prop_dark", &Self::WOOD_PROP_DARK),
        ("wood_dark", &Self::WOOD_DARK),
        ("wood_pale", &Self::WOOD_PALE),
        ("fabric_red", &Self::FABRIC_RED),
        ("fabric_teal", &Self::FABRIC_TEAL),
        ("fabric_cream", &Self::FABRIC_CREAM),
        ("burlap", &Self::BURLAP),
        ("rubber", &Self::RUBBER),
        ("glass", &Self::GLASS),
        ("foliage", &Self::FOLIAGE),
        ("window_void", &Self::WINDOW_VOID),
        ("interior_shell", &Self::INTERIOR_SHELL),
        ("window_glass", &Self::WINDOW_GLASS),
        ("plywood", &Self::PLYWOOD),
        ("emissive_warm", &Self::EMISSIVE_WARM),
        ("window_glow", &Self::WINDOW_GLOW),
        ("lamp_lens", &Self::LAMP_LENS),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_count() {
        assert_eq!(Palette::ALL.len(), 46, "Palette should have exactly 46 entries");
    }

    #[test]
    fn plaster_cream_exact_values() {
        let entry = Palette::PLASTER_CREAM;
        assert_eq!(entry.name, "plaster");
        assert_eq!(entry.surface, Surface::Plaster);
        assert_eq!(entry.opts.tint, Some(0xcfc0a4));
        assert_eq!(entry.opts.scale, 2.35);
        assert_eq!(entry.opts.weather, Some([0.4, 0.5, 1.4, 0.55]));
    }

    #[test]
    fn concrete_vs_concrete_prop() {
        let concrete = Palette::CONCRETE;
        let concrete_prop = Palette::CONCRETE_PROP;

        // Both are concrete surface
        assert_eq!(concrete.surface, Surface::Concrete);
        assert_eq!(concrete_prop.surface, Surface::Concrete);

        // Both have same name but different scales
        assert_eq!(concrete.name, "concrete");
        assert_eq!(concrete_prop.name, "concrete");
        assert_eq!(concrete.opts.scale, 2.5);
        assert_eq!(concrete_prop.opts.scale, 0.9);
    }

    #[test]
    fn metal_rust_prop_normal_strength() {
        let entry = Palette::METAL_RUST_PROP;
        assert_eq!(entry.opts.normal_strength, Some(1.35));
        assert_eq!(entry.opts.tint, Some(0x9d7c66));
    }

    #[test]
    fn fabric_red_three_options() {
        let entry = Palette::FABRIC_RED;
        assert_eq!(entry.opts.tint, Some(0xa2564a));
        assert!(entry.opts.three.is_some());
        let three = entry.opts.three.unwrap();
        assert_eq!(three.side, Some(2));
    }

    #[test]
    fn window_glass_roughness() {
        let entry = Palette::WINDOW_GLASS;
        assert_eq!(entry.opts.roughness, Some([0.3, 0.06]));
        assert!(entry.opts.three.is_some());
        let three = entry.opts.three.unwrap();
        assert_eq!(three.opacity, Some(0.16));
        assert_eq!(three.env_map_intensity, Some(2.1));
    }

    #[test]
    fn emissive_warm_tone_mapped() {
        let entry = Palette::EMISSIVE_WARM;
        assert_eq!(entry.opts.scale, 0.4);
        let three = entry.opts.three.unwrap();
        assert_eq!(three.emissive, Some(0xffd39a));
        assert_eq!(three.emissive_intensity, Some(12.0));
        assert_eq!(three.tone_mapped, Some(true));
    }
}
