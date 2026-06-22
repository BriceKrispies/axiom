//! Generic body surfaces: targetable anatomical surfaces for contact/residue.

use crate::ids::{BodyPartId, BodySurfaceId};

/// The category of a body surface. Opaque to sim-core behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodySurfaceKind {
    /// An outer surface.
    Outer,
    /// An inner surface.
    Inner,
    /// A mouth surface.
    Mouth,
    /// A wound surface.
    Wound,
    /// An uncategorized surface.
    Generic,
}

const SURFACE_KINDS: [BodySurfaceKind; 5] = [
    BodySurfaceKind::Outer,
    BodySurfaceKind::Inner,
    BodySurfaceKind::Mouth,
    BodySurfaceKind::Wound,
    BodySurfaceKind::Generic,
];

impl BodySurfaceKind {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<BodySurfaceKind> {
        SURFACE_KINDS.get(code as usize).copied()
    }

    /// The kind's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// How exposed a surface is. Affects which routes may reach it (later phases use
/// this; sim-core only stores and compares it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceExposure {
    /// Reachable from outside.
    External,
    /// Reachable only from inside.
    Internal,
}

/// An opaque, domain-defined surface state code (e.g. clean vs fouled — later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodySurfaceState(u32);

impl BodySurfaceState {
    /// A surface state from a deterministic code.
    pub const fn new(code: u32) -> Self {
        BodySurfaceState(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// An instantiated body surface on a body part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodySurface {
    id: BodySurfaceId,
    part: BodyPartId,
    kind: BodySurfaceKind,
    exposure: SurfaceExposure,
    state: BodySurfaceState,
}

impl BodySurface {
    /// Construct a surface (used by body instantiation).
    pub(crate) const fn new(
        id: BodySurfaceId,
        part: BodyPartId,
        kind: BodySurfaceKind,
        exposure: SurfaceExposure,
    ) -> Self {
        BodySurface {
            id,
            part,
            kind,
            exposure,
            state: BodySurfaceState::new(0),
        }
    }

    /// This surface's id.
    pub const fn id(&self) -> BodySurfaceId {
        self.id
    }

    /// The body part this surface belongs to.
    pub const fn part(&self) -> BodyPartId {
        self.part
    }

    /// The surface kind.
    pub const fn kind(&self) -> BodySurfaceKind {
        self.kind
    }

    /// The surface exposure.
    pub const fn exposure(&self) -> SurfaceExposure {
        self.exposure
    }

    /// The surface state.
    pub const fn state(&self) -> BodySurfaceState {
        self.state
    }

    /// Set the surface state (used by the body store).
    pub(crate) fn set_state(&mut self, state: BodySurfaceState) {
        self.state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_codes_validate_and_round_trip() {
        assert_eq!(BodySurfaceKind::from_code(0), Some(BodySurfaceKind::Outer));
        assert_eq!(
            BodySurfaceKind::from_code(4),
            Some(BodySurfaceKind::Generic)
        );
        assert_eq!(BodySurfaceKind::from_code(5), None);
        assert_eq!(BodySurfaceKind::Mouth.code(), 2);
    }

    #[test]
    fn surface_carries_fields_and_state() {
        let mut surface = BodySurface::new(
            BodySurfaceId::from_raw(1),
            BodyPartId::from_raw(2),
            BodySurfaceKind::Outer,
            SurfaceExposure::External,
        );
        assert_eq!(surface.id(), BodySurfaceId::from_raw(1));
        assert_eq!(surface.part(), BodyPartId::from_raw(2));
        assert_eq!(surface.kind(), BodySurfaceKind::Outer);
        assert_eq!(surface.exposure(), SurfaceExposure::External);
        assert_eq!(surface.state(), BodySurfaceState::new(0));
        surface.set_state(BodySurfaceState::new(3));
        assert_eq!(surface.state(), BodySurfaceState::new(3));
        assert_ne!(SurfaceExposure::External, SurfaceExposure::Internal);
    }
}
