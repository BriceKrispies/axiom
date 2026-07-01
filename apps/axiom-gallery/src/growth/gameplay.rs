//! Downstream gameplay scaffolds: player, survival, emergence, spirit, ecology,
//! presentation.

pub mod player {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct PlayerPose {
        pub x: f32,
        pub y: f32,
        pub z: f32,
        pub yaw: f32,
        pub pitch: f32,
    }
}

pub mod survival {
    #[derive(Debug, Clone, Copy)]
    pub struct Need {
        pub id: u32,
        pub value: f32,
        pub decay_per_tick: f32,
    }
    #[derive(Debug, Clone, Copy)]
    pub struct Threat {
        pub id: u32,
    }
}

pub mod emergence {
    #[derive(Debug, Clone, Default)]
    pub struct BiasSet {
        pub weights: std::collections::HashMap<String, f32>,
    }
}

pub mod spirit {
    /// Contradicted by IDEAS.md ("decided against"); kept unresolved deliberately.
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

pub mod ecology {
    #[derive(Debug, Clone, Copy)]
    pub struct Population {
        pub species: u32,
        pub region: u32,
        pub count: u32,
    }
}

pub mod presentation {
    pub const LAND_DISPLACEMENT_SCALE: f32 = 0.04;
    pub const SUBSEA_DISPLACEMENT_FACTOR: f32 = 0.3;
}
