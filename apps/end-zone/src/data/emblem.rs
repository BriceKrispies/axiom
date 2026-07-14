//! Procedural emblem definitions: a small closed vocabulary of base plates
//! and central motifs interpreted by app-local drawing code (frontend team
//! cards, end-zone paint). Never bitmaps, never a scripting language.

/// The emblem's base plate silhouette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmblemBase {
    Shield,
    Disc,
    Hex,
    Pennant,
}

/// The emblem's central motif.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmblemMotif {
    Bolt,
    Wing,
    Claw,
    Star,
    /// A geometric animal head (angular fang/snout silhouette).
    Fang,
    /// An abstract chevron mark.
    Chevrons,
}

/// One team's procedural emblem: base plate + motif + optional initial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmblemDefinition {
    pub base: EmblemBase,
    pub motif: EmblemMotif,
    /// Uppercase ASCII initial overlaid on the plate, when present.
    pub initial: Option<char>,
}

impl EmblemDefinition {
    /// Whether the definition uses only valid procedural primitives.
    pub fn is_valid(&self) -> bool {
        self.initial.map(|c| c.is_ascii_uppercase()).unwrap_or(true)
    }
}
