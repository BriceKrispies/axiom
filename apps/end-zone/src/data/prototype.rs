//! The decision-window prototype's ONE offensive concept and the defensive
//! answers it may draw.
//!
//! The prototype asks a single design question — *is watching a play simulate
//! itself and intervening at a dramatic read more fun than steering every
//! frame?* — so the offense never changes: one formation, one concept, three
//! eligible targets whose routes are deliberately **different tactical
//! propositions** rather than three shapes of the same idea:
//!
//! | Read | Slot | Route | Available | Proposition |
//! |---|---|---|---|---|
//! | 1 | 6 (slot right) | quick slant | ~1.1 s | small, safe, almost always there |
//! | 2 | 5 (flanker) | deep dig across the middle | ~2.4 s | a chunk, but it crosses traffic |
//! | 3 | 4 (split end) | post | ~3.2 s | the big play — if the pocket holds |
//!
//! Waiting for read 3 is the whole game: the later reads pay more and the rush
//! is closer every tick. The defense is picked per attempt from the existing
//! deterministic selector, so the coverage a read must beat changes between
//! attempts without the offense ever changing.

use crate::field::{DriveDirection, OffensePoint};
use crate::identity::{PlayId, TeamId};

use super::formation::spread_offense;
use super::play::{
    OffenseAssignment, OffenseTag, OffensivePlay, PlayDefinition, RouteDefinition, RouteShape,
};
use super::playbook::cover_man;

/// The offensive roster slots the three reads map to, in **read order**: read
/// one is the shortest route, read three is the deepest. The decision window
/// labels receivers by this index, so `1` always means "the safe one".
pub const READ_SLOTS: [usize; 3] = [6, 5, 4];

/// The number of eligible targets the prototype fields.
pub const READ_COUNT: usize = READ_SLOTS.len();

/// The short name of each read, shown on the decision prompt.
pub const READ_NAMES: [&str; READ_COUNT] = ["SLANT", "DIG", "POST"];

/// Roughly how many yards past the line each read is worth when it is caught
/// clean — the *reward* half of the wait/risk trade the prototype is testing.
/// Presentation only; the real gain is always measured from where the play
/// actually ends.
pub const READ_REWARD: [f32; READ_COUNT] = [5.0, 12.0, 20.0];

/// Where the prototype always snaps from. Mid-field keeps every read legal (the
/// post has room to run) and keeps the spot honest — a fixed line of scrimmage
/// means the yards a read gains are comparable attempt to attempt.
pub const PROTOTYPE_LINE: f32 = 40.0;

/// The single offensive concept. Slots: 0 quarterback, 1 snapper, 2/3 pass
/// blockers, 4/5/6 the three eligible reads.
///
/// The quarterback's drop is deep enough (5 yd) that the pocket is a real,
/// visible space that collapses over the play instead of a formality.
pub fn triple_read() -> OffensivePlay {
    OffensivePlay {
        id: PlayId(9),
        name: "TRIPLE READ",
        tag: OffenseTag::DeepPass,
        formation: spread_offense(),
        assignments: [
            OffenseAssignment::Quarterback { drop_depth: 5.0 },
            OffenseAssignment::Snapper,
            OffenseAssignment::PassBlock,
            OffenseAssignment::PassBlock,
            // Read 3 — the split end's post: a stem then a hard break to the
            // deep middle, ~19 yards downfield. The biggest gain on the field
            // and the last one to come open. It was authored deeper at first
            // and the harness said so: at 22 yards it completed four percent of
            // the time, which is not a choice, it is a trap.
            OffenseAssignment::Route(RouteDefinition::Shape(RouteShape::Post {
                stem: 8.0,
                cut: 8.0,
            })),
            // Read 2 — the flanker's dig: eleven yards, then a square-in that
            // crosses the whole formation through the underneath coverage.
            OffenseAssignment::Route(RouteDefinition::Shape(RouteShape::In {
                stem: 11.0,
                cut: 16.0,
            })),
            // Read 1 — the slot's quick slant: three-and-a-half yards and break
            // INSIDE. It breaks inside rather than out on purpose — a slant is
            // caught in traffic, so it is the read that reliably completes and
            // reliably gains almost nothing. An out-breaking version of this
            // route caught clean on the sideline and ran thirty yards, which
            // made the safe read pay like the dangerous one.
            OffenseAssignment::Route(RouteDefinition::Shape(RouteShape::Slant {
                stem: 3.5,
                cut: 3.5,
            })),
        ],
    }
}

/// The offense-relative alignment of a read's receiver (the pre-snap picture the
/// player learns to recognize).
pub fn read_alignment(read: usize) -> OffensePoint {
    let slot = READ_SLOTS[read.min(READ_COUNT - 1)];
    spread_offense().slots[slot].position
}

/// The composed play the prototype lines up when no defensive answer has been
/// chosen yet (the bootstrap state before the first attempt is built).
pub fn prototype_play() -> PlayDefinition {
    PlayDefinition::compose(
        &triple_read(),
        &cover_man(),
        TeamId(0),
        DriveDirection::PlusZ,
        PROTOTYPE_LINE,
    )
}
