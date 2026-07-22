//! Declarative data definitions — the framework's authoring surface. Teams,
//! rosters, archetypes, formations, routes, plays, and behavior tuning are all
//! plain data interpreted by the generic simulation systems; changing a play
//! or an archetype never means changing AI code.

pub mod biomech_tuning;
pub mod emblem;
pub mod formation;
pub mod locomotion_tuning;
pub mod play;
pub mod play_diagram;
pub mod playbook;
pub mod player;
pub mod team;
pub mod tuning;

pub use biomech_tuning::BiomechTuning;
pub use formation::{FormationDefinition, FormationSlot};
pub use locomotion_tuning::LocomotionTuning;
pub use play::{
    Coverage, DefenseAssignment, DefenseFront, DefensiveCall, OffenseAssignment, OffenseTag,
    OffensivePlay, PlayDefinition, RouteDefinition, RouteShape,
};
pub use play_diagram::{DiagramMark, DiagramRole, PlayDiagram};
pub use playbook::{defensive_calls, offensive_playbook, showcase_play};
pub use player::{showcase_rosters, PlayerArchetype, PlayerDefinition, RosterDefinition};
pub use team::{TeamDefinition, TeamPalette};
pub use tuning::{BehaviorTuning, CameraTuning, JuiceTuning};
