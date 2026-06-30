//! Downstream gameplay scaffolds: player, survival, emergence, spirit, ecology,
//! presentation. These are intentionally deferred in the audit (M2+); they are
//! represented here as typed stubs so the requirement registry can trace them
//! and the adversarial review can see they exist and are not yet implemented.

/// Player avatar + play camera + interaction. Audit: PL-0.x (M2). Scaffold.
pub mod player {
    /// Sim-authoritative player pose. Audit: PL-0.2 movement on chunks.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct PlayerPose {
        pub x: f32,
        pub y: f32,
        pub z: f32,
        pub yaw: f32,
        pub pitch: f32,
    }
}

/// Survival needs/threats + place/build. Audit: SV-0.x (M3). Scaffold.
pub mod survival {
    /// A depleting need. Audit: SV-0.1.
    #[derive(Debug, Clone, Copy)]
    pub struct Need {
        pub id: u32,
        pub value: f32,
        pub decay_per_tick: f32,
    }
    /// An environmental threat tied to biome/overworld. Audit: SV-0.2.
    #[derive(Debug, Clone, Copy)]
    pub struct Threat {
        pub id: u32,
    }
}

/// Guardrailed emergence: profile bias weights. Audit: GE-0.x (M4). Scaffold.
pub mod emergence {
    /// A set of bias weights steering gen/spawn tables. Audit: GE-0.1/0.2.
    #[derive(Debug, Clone, Default)]
    pub struct BiasSet {
        pub weights: std::collections::HashMap<String, f32>,
    }
}

/// Spirit + meta time. Audit: SP-0.x (M5). Scaffold.
pub mod spirit {
    /// Sim-time gate: world advances only while possessing. Audit: SP-0.1.
    /// NOTE: the audit flags this as contradicted by IDEAS.md ("decided
    /// against"); represented but explicitly unresolved.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct TimeGate {
        pub possession_active: bool,
    }
    impl TimeGate {
        pub fn should_advance(&self) -> bool {
            self.possession_active
        }
    }
}

/// Ecology: template species, regional populations, agent LOD. Audit: EC-0.x (M7). Scaffold.
pub mod ecology {
    /// Per-region population scalar. Audit: EC-0.2 (deterministic from seed).
    #[derive(Debug, Clone, Copy)]
    pub struct Population {
        pub species: u32,
        pub region: u32,
        pub count: u32,
    }
}

/// Presentation glue: cel materials, biome tint, displacement scale. Audit:
/// PR-0.x (M8), OW-E13. Scaffold — real rendering wires the scene/render modules.
pub mod presentation {
    /// Radial displacement scales for the debug globe mesh. Audit: OW-E13.
    pub const LAND_DISPLACEMENT_SCALE: f32 = 0.04;
    pub const SUBSEA_DISPLACEMENT_FACTOR: f32 = 0.3;
}
