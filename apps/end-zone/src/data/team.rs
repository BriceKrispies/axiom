//! Fictional team definitions and configurable palettes. No real-world league,
//! team, or player branding appears anywhere; colors are plain data consumed by
//! the player-model construction and the end-zone paint.

use crate::identity::TeamId;

/// Uniform + trim colors (linear RGB). One palette slot per model part tag —
/// player construction reads the palette and contains zero team branches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamPalette {
    pub helmet: [f32; 3],
    pub facemask: [f32; 3],
    pub jersey: [f32; 3],
    pub pants: [f32; 3],
    pub skin: [f32; 3],
    pub shoes: [f32; 3],
    /// End-zone paint + accents.
    pub trim: [f32; 3],
}

impl TeamPalette {
    /// The palette as part-tag-indexed slots, in the model's tag order:
    /// helmet, facemask, jersey, pants, skin, shoes, trim.
    pub fn slots(&self) -> [[f32; 3]; 7] {
        [
            self.helmet,
            self.facemask,
            self.jersey,
            self.pants,
            self.skin,
            self.shoes,
            self.trim,
        ]
    }
}

/// One fictional team.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamDefinition {
    pub id: TeamId,
    pub name: &'static str,
    pub palette: TeamPalette,
}

/// Home showcase team: the **Magma** (ember red / gold).
pub const fn magma() -> TeamDefinition {
    TeamDefinition {
        id: TeamId(0),
        name: "MAGMA",
        palette: TeamPalette {
            helmet: [0.62, 0.10, 0.08],
            facemask: [0.12, 0.12, 0.13],
            jersey: [0.78, 0.16, 0.10],
            pants: [0.92, 0.78, 0.34],
            skin: [0.82, 0.62, 0.44],
            shoes: [0.14, 0.13, 0.13],
            trim: [0.55, 0.09, 0.07],
        },
    }
}

/// Away showcase team: the **Frostbite** (glacier blue / silver).
pub const fn frostbite() -> TeamDefinition {
    TeamDefinition {
        id: TeamId(1),
        name: "FROSTBITE",
        palette: TeamPalette {
            helmet: [0.12, 0.32, 0.66],
            facemask: [0.85, 0.88, 0.92],
            jersey: [0.16, 0.42, 0.80],
            pants: [0.82, 0.86, 0.90],
            skin: [0.66, 0.46, 0.32],
            shoes: [0.90, 0.91, 0.94],
            trim: [0.10, 0.26, 0.55],
        },
    }
}
