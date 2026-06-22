//! Generic body interaction routes that refine Phase-3 interaction routes.
//!
//! A body route names *how* contact reaches a body surface (surface-contact,
//! mouth-contact, ingestion-entry, …). sim-core only represents and validates
//! routes; it performs no ingestion, inhalation, breathing, or infection.

use crate::body_surface::BodySurfaceKind;
use crate::ids::BodySurfaceId;
use crate::interaction::InteractionRoute;

/// The category of a body route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyRouteKind {
    /// Contact with an outer surface.
    SurfaceContact,
    /// Contact with a mouth surface.
    MouthContact,
    /// Entry by ingestion (through a mouth).
    IngestionEntry,
    /// Entry by inhalation (through a mouth/inner surface).
    InhalationEntry,
    /// Entry through a wound surface.
    WoundEntry,
    /// Entry by being embedded (through a wound).
    EmbeddedEntry,
    /// Contact with an inner surface.
    InternalContact,
    /// An unclassified body route.
    Generic,
}

const ROUTE_KINDS: [BodyRouteKind; 8] = [
    BodyRouteKind::SurfaceContact,
    BodyRouteKind::MouthContact,
    BodyRouteKind::IngestionEntry,
    BodyRouteKind::InhalationEntry,
    BodyRouteKind::WoundEntry,
    BodyRouteKind::EmbeddedEntry,
    BodyRouteKind::InternalContact,
    BodyRouteKind::Generic,
];

impl BodyRouteKind {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<BodyRouteKind> {
        ROUTE_KINDS.get(code as usize).copied()
    }

    /// The kind's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A body route plus the surface it targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BodyRouteTarget {
    route: BodyRouteKind,
    surface: BodySurfaceId,
}

impl BodyRouteTarget {
    /// A route targeting a surface.
    pub const fn new(route: BodyRouteKind, surface: BodySurfaceId) -> Self {
        BodyRouteTarget { route, surface }
    }

    /// The body route kind.
    pub const fn route(self) -> BodyRouteKind {
        self.route
    }

    /// The targeted surface.
    pub const fn surface(self) -> BodySurfaceId {
        self.surface
    }
}

/// A body route and the surface kinds it may legally target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyRoute {
    kind: BodyRouteKind,
}

// Whether route-kind R may target surface-kind S — a static table indexed by
// `[route_code][surface_code]`. Surface order: Outer, Inner, Mouth, Wound, Generic.
const TARGETS: [[bool; 5]; 8] = [
    //              Outer  Inner  Mouth  Wound  Generic
    /* Surface   */
    [true, false, false, false, true],
    /* Mouth     */ [false, false, true, false, true],
    /* Ingestion */ [false, false, true, false, true],
    /* Inhalation*/ [false, true, true, false, true],
    /* Wound     */ [false, false, false, true, true],
    /* Embedded  */ [false, false, false, true, true],
    /* Internal  */ [false, true, false, false, true],
    /* Generic   */ [true, true, true, true, true],
];

impl BodyRoute {
    /// A body route of `kind`.
    pub const fn new(kind: BodyRouteKind) -> Self {
        BodyRoute { kind }
    }

    /// The body route a Phase-3 [`InteractionRoute`] maps to.
    pub fn from_interaction(route: InteractionRoute) -> BodyRoute {
        // Map by interaction route code (Touch=0..Generic=7).
        const MAP: [BodyRouteKind; 8] = [
            BodyRouteKind::SurfaceContact,  // Touch
            BodyRouteKind::IngestionEntry,  // Ingestion
            BodyRouteKind::InhalationEntry, // Inhalation
            BodyRouteKind::WoundEntry,      // WoundContact
            BodyRouteKind::EmbeddedEntry,   // Embedded
            BodyRouteKind::InternalContact, // Contained
            BodyRouteKind::SurfaceContact,  // Adjacent
            BodyRouteKind::Generic,         // Generic
        ];
        BodyRoute::new(MAP[route.code() as usize])
    }

    /// The route kind.
    pub const fn kind(self) -> BodyRouteKind {
        self.kind
    }

    /// Whether this route may target a surface of `surface_kind`.
    pub fn can_target(self, surface_kind: BodySurfaceKind) -> bool {
        TARGETS[self.kind.code() as usize][surface_kind.code() as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_kind_codes_validate_and_round_trip() {
        assert_eq!(
            BodyRouteKind::from_code(0),
            Some(BodyRouteKind::SurfaceContact)
        );
        assert_eq!(BodyRouteKind::from_code(7), Some(BodyRouteKind::Generic));
        assert_eq!(BodyRouteKind::from_code(8), None);
        assert_eq!(BodyRouteKind::MouthContact.code(), 1);
    }

    #[test]
    fn interaction_routes_map_to_body_routes() {
        assert_eq!(
            BodyRoute::from_interaction(InteractionRoute::Touch).kind(),
            BodyRouteKind::SurfaceContact
        );
        assert_eq!(
            BodyRoute::from_interaction(InteractionRoute::Ingestion).kind(),
            BodyRouteKind::IngestionEntry
        );
        assert_eq!(
            BodyRoute::from_interaction(InteractionRoute::WoundContact).kind(),
            BodyRouteKind::WoundEntry
        );
        assert_eq!(
            BodyRoute::from_interaction(InteractionRoute::Generic).kind(),
            BodyRouteKind::Generic
        );
    }

    #[test]
    fn route_validates_allowed_target_surface_kinds() {
        let surface = BodyRoute::new(BodyRouteKind::SurfaceContact);
        assert!(surface.can_target(BodySurfaceKind::Outer));
        assert!(!surface.can_target(BodySurfaceKind::Mouth));
        assert!(!surface.can_target(BodySurfaceKind::Wound));

        let mouth = BodyRoute::new(BodyRouteKind::MouthContact);
        assert!(mouth.can_target(BodySurfaceKind::Mouth));
        assert!(!mouth.can_target(BodySurfaceKind::Outer));

        let wound = BodyRoute::new(BodyRouteKind::WoundEntry);
        assert!(wound.can_target(BodySurfaceKind::Wound));
        assert!(!wound.can_target(BodySurfaceKind::Outer));

        // Generic route may target anything; generic surface accepts anything.
        let generic = BodyRoute::new(BodyRouteKind::Generic);
        assert!(generic.can_target(BodySurfaceKind::Inner));
        assert!(surface.can_target(BodySurfaceKind::Generic));
    }

    #[test]
    fn route_target_carries_route_and_surface() {
        let target = BodyRouteTarget::new(BodyRouteKind::MouthContact, BodySurfaceId::from_raw(5));
        assert_eq!(target.route(), BodyRouteKind::MouthContact);
        assert_eq!(target.surface(), BodySurfaceId::from_raw(5));
    }
}
