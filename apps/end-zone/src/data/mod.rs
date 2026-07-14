//! Declarative data definitions — the framework's authoring surface. Teams,
//! rosters, archetypes, formations, routes, plays, and behavior tuning are all
//! plain data interpreted by the generic simulation systems; changing a play
//! or an archetype never means changing AI code.

pub mod emblem;
pub mod formation;
pub mod play;
pub mod player;
pub mod team;
pub mod tuning;

pub use formation::{FormationDefinition, FormationSlot};
pub use play::{
    showcase_play, DefenseAssignment, OffenseAssignment, PlayDefinition, RouteDefinition,
    RouteShape,
};
pub use player::{showcase_rosters, PlayerArchetype, PlayerDefinition, RosterDefinition};
pub use team::{TeamDefinition, TeamPalette};
pub use tuning::{BehaviorTuning, CameraTuning, JuiceTuning};
