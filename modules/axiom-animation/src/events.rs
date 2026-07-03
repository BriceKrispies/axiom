//! Discrete animation events: gameplay-relevant moments a clip fires on exact
//! frames, decoupled from the continuous pose.
//!
//! The kick clip carries a single [`EventKind::KickContact`] on its strike
//! frame, targeting the right foot — the frame a consumer would read as "the
//! ball is struck now". Events are pure data on integer frames, so
//! [`EventTrack::at`] is deterministic: the same frame always yields the same
//! events.

/// What an [`AnimationEvent`] signifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// The kicking foot contacts the ball. The canonical strike-moment marker.
    KickContact,
    /// A foot is planted on the ground (weight transfer / support).
    FootPlant,
}

/// A discrete event on a clip: which frame it fires on, what it is, and which
/// bone it concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationEvent {
    /// The frame the event fires on.
    pub frame: u32,
    /// What the event is.
    pub kind: EventKind,
    /// Index of the bone the event targets (e.g. the right-foot bone for a
    /// [`EventKind::KickContact`]).
    pub target_bone: usize,
}

impl AnimationEvent {
    /// Construct an event.
    pub const fn new(frame: u32, kind: EventKind, target_bone: usize) -> Self {
        Self {
            frame,
            kind,
            target_bone,
        }
    }
}

/// A clip's discrete events, in frame order.
#[derive(Debug, Clone, PartialEq)]
pub struct EventTrack {
    /// The events.
    pub events: Vec<AnimationEvent>,
}

impl EventTrack {
    /// Wrap an event list.
    pub fn new(events: Vec<AnimationEvent>) -> Self {
        Self { events }
    }

    /// Every event firing exactly on `frame`.
    pub fn at(&self, frame: u32) -> Vec<AnimationEvent> {
        self.events
            .iter()
            .copied()
            .filter(|e| e.frame == frame)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_returns_only_matching_frame() {
        let track = EventTrack::new(vec![
            AnimationEvent::new(3, EventKind::FootPlant, 14),
            AnimationEvent::new(9, EventKind::KickContact, 17),
        ]);
        assert_eq!(track.at(3), vec![AnimationEvent::new(3, EventKind::FootPlant, 14)]);
        assert_eq!(
            track.at(9),
            vec![AnimationEvent::new(9, EventKind::KickContact, 17)]
        );
        assert!(track.at(5).is_empty());
    }

    #[test]
    fn event_fields_are_readable() {
        let e = AnimationEvent::new(9, EventKind::KickContact, 17);
        assert_eq!(e.frame, 9);
        assert_eq!(e.kind, EventKind::KickContact);
        assert_eq!(e.target_bone, 17);
    }
}
