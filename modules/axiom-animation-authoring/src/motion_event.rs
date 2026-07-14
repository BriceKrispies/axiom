//! [`MotionEvent`] — the authored event vocabulary, and [`ResolvedEvent`], its
//! name-resolved form.
//!
//! An event fires at a specific tick. A `Named` event carries only a label (a
//! generic gameplay cue); a `BallContact` event carries the contact surface
//! (effector), the aim target, the direction target, and a power scalar — the
//! moment a strike connects.

use axiom_kernel::Tick;

use crate::ids::{EffectorId, TargetId};

/// The event vocabulary. The discriminant is a stable classification index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A generic named cue.
    Named,
    /// A ball-contact event with surface, target, direction, and power.
    BallContact,
}

/// An authored event referencing its surface/targets by name.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionEvent {
    kind: EventKind,
    tick: Tick,
    name: String,
    contact_surface: Option<String>,
    target_name: Option<String>,
    direction_target_name: Option<String>,
    power: f32,
}

impl MotionEvent {
    /// A named cue `name` at `tick`.
    pub(crate) fn named(tick: Tick, name: &str) -> Self {
        MotionEvent {
            kind: EventKind::Named,
            tick,
            name: name.to_string(),
            contact_surface: None,
            target_name: None,
            direction_target_name: None,
            power: 0.0,
        }
    }

    /// A ball-contact event at `tick`: `contact_surface` (effector) strikes
    /// `target` in the direction of `direction_target` with `power`.
    pub(crate) fn ball_contact(
        tick: Tick,
        contact_surface: &str,
        target: &str,
        direction_target: &str,
        power: f32,
    ) -> Self {
        MotionEvent {
            kind: EventKind::BallContact,
            tick,
            name: "ball_contact".to_string(),
            contact_surface: Some(contact_surface.to_string()),
            target_name: Some(target.to_string()),
            direction_target_name: Some(direction_target.to_string()),
            power,
        }
    }

    /// The event kind.
    pub(crate) fn kind(&self) -> EventKind {
        self.kind
    }

    /// The tick the event fires at.
    pub(crate) fn tick(&self) -> Tick {
        self.tick
    }

    /// The event's name/label.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The contact-surface effector name (ball-contact only).
    pub(crate) fn contact_surface(&self) -> Option<&str> {
        self.contact_surface.as_deref()
    }

    /// The aim target name (ball-contact only).
    pub(crate) fn target_name(&self) -> Option<&str> {
        self.target_name.as_deref()
    }

    /// The direction target name (ball-contact only).
    pub(crate) fn direction_target_name(&self) -> Option<&str> {
        self.direction_target_name.as_deref()
    }

    /// The power scalar (ball-contact only).
    pub(crate) fn power(&self) -> f32 {
        self.power
    }
}

/// A resolved event: names replaced by ids. `Named` events leave the id fields at
/// their harmless defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEvent {
    kind: EventKind,
    tick: Tick,
    name: String,
    contact_surface: EffectorId,
    target: TargetId,
    direction_target: TargetId,
    power: f32,
}

impl ResolvedEvent {
    /// Construct a resolved event.
    pub(crate) fn new(
        kind: EventKind,
        tick: Tick,
        name: String,
        contact_surface: EffectorId,
        target: TargetId,
        direction_target: TargetId,
        power: f32,
    ) -> Self {
        ResolvedEvent {
            kind,
            tick,
            name,
            contact_surface,
            target,
            direction_target,
            power,
        }
    }

    /// The tick the event fires at.
    pub(crate) fn tick(&self) -> Tick {
        self.tick
    }

    /// The event's name/label.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The contact-surface effector (ball-contact only).
    pub(crate) fn contact_surface(&self) -> EffectorId {
        self.contact_surface
    }

    /// The aim target (ball-contact only).
    pub(crate) fn target(&self) -> TargetId {
        self.target
    }

    /// The direction target (ball-contact only).
    pub(crate) fn direction_target(&self) -> TargetId {
        self.direction_target
    }

    /// The power scalar (ball-contact only).
    pub(crate) fn power(&self) -> f32 {
        self.power
    }

    /// Whether this event is a ball-contact.
    pub(crate) fn is_ball_contact(&self) -> bool {
        self.kind == EventKind::BallContact
    }
}

/// A default resolved id endpoint used by the compiler for `Named` events, which
/// carry no effector/target references.
pub const UNUSED_TARGET: TargetId = TargetId::from_raw(0);
/// A default resolved effector endpoint for `Named` events.
pub const UNUSED_EFFECTOR: EffectorId = EffectorId::from_raw(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_event_carries_only_a_label() {
        let e = MotionEvent::named(Tick::new(4), "whistle");
        assert_eq!(e.kind(), EventKind::Named);
        assert_eq!(e.tick(), Tick::new(4));
        assert_eq!(e.name(), "whistle");
        assert_eq!(e.contact_surface(), None);
        assert_eq!(e.target_name(), None);
        assert_eq!(e.direction_target_name(), None);
        assert_eq!(e.power(), 0.0);
    }

    #[test]
    fn ball_contact_carries_surface_targets_and_power() {
        let e = MotionEvent::ball_contact(
            Tick::new(30),
            "right_foot_instep",
            "ball",
            "net_center",
            0.75,
        );
        assert_eq!(e.kind(), EventKind::BallContact);
        assert_eq!(e.name(), "ball_contact");
        assert_eq!(e.contact_surface(), Some("right_foot_instep"));
        assert_eq!(e.target_name(), Some("ball"));
        assert_eq!(e.direction_target_name(), Some("net_center"));
        assert_eq!(e.power(), 0.75);
    }

    #[test]
    fn resolved_event_round_trips_and_flags_ball_contact() {
        let r = ResolvedEvent::new(
            EventKind::BallContact,
            Tick::new(30),
            "ball_contact".to_string(),
            EffectorId::from_raw(2),
            TargetId::from_raw(0),
            TargetId::from_raw(1),
            0.75,
        );
        assert_eq!(r.tick(), Tick::new(30));
        assert_eq!(r.name(), "ball_contact");
        assert_eq!(r.contact_surface(), EffectorId::from_raw(2));
        assert_eq!(r.target(), TargetId::from_raw(0));
        assert_eq!(r.direction_target(), TargetId::from_raw(1));
        assert_eq!(r.power(), 0.75);
        assert!(r.is_ball_contact());

        let named = ResolvedEvent::new(
            EventKind::Named,
            Tick::new(1),
            "n".to_string(),
            UNUSED_EFFECTOR,
            UNUSED_TARGET,
            UNUSED_TARGET,
            0.0,
        );
        assert!(!named.is_ball_contact());
    }
}
