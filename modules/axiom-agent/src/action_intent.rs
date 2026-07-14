//! A neutral, player-equivalent action intent.

/// One action an agent wants to take, expressed in the same neutral vocabulary a
/// *player* would drive — never as a concrete device event.
///
/// It is a compact record with a `kind_code` discriminant field plus a fixed set
/// of numeric payload slots; a given kind uses only the slots it needs and
/// leaves the rest zero. Modelling the kind as a *field* (not a data-carrying
/// enum) keeps every consumer branch-free: reporting reads `kind_code()` with no
/// `match`. All payload is numeric — control/axis/subject/affordance **codes**
/// and fixed-point coordinates — so no keyboard, mouse, controller, or gameplay
/// noun is ever baked in. Lowering an intent into real input is the app's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionIntent {
    kind_code: u16,
    control_code: u32,
    axis_code: u32,
    subject_code: u32,
    affordance_code: u32,
    ticks: u32,
    x: i64,
    y: i64,
    z: i64,
    value: i64,
}

impl ActionIntent {
    /// Do nothing this step.
    pub const KIND_NOOP: u16 = 0;
    /// Hold position for a number of ticks.
    pub const KIND_WAIT_TICKS: u16 = 1;
    /// Begin holding an abstract control.
    pub const KIND_PRESS_CONTROL: u16 = 2;
    /// Stop holding an abstract control.
    pub const KIND_RELEASE_CONTROL: u16 = 3;
    /// Drive a movement axis by a signed amount.
    pub const KIND_MOVE_AXIS: u16 = 4;
    /// Drive a look axis by a signed amount.
    pub const KIND_LOOK_AXIS: u16 = 5;
    /// Move an abstract pointer to a coordinate.
    pub const KIND_POINTER_MOVE: u16 = 6;
    /// Begin a pointer contact.
    pub const KIND_POINTER_DOWN: u16 = 7;
    /// End a pointer contact.
    pub const KIND_POINTER_UP: u16 = 8;
    // High-level kinds start at 100, leaving 9..=99 as reserved headroom for
    // future low-level controls without disturbing the high-level block.
    /// Orient toward a subject (high-level, data only).
    pub const KIND_LOOK_AT_SUBJECT: u16 = 100;
    /// Orient toward a point (high-level, data only).
    pub const KIND_LOOK_AT_POINT: u16 = 101;
    /// Move toward a subject (high-level, data only).
    pub const KIND_MOVE_TOWARD_SUBJECT: u16 = 102;
    /// Move toward a point (high-level, data only).
    pub const KIND_MOVE_TOWARD_POINT: u16 = 103;
    /// Interact with a subject (high-level, data only).
    pub const KIND_INTERACT_WITH_SUBJECT: u16 = 104;
    /// Use a named affordance (high-level, data only).
    pub const KIND_USE_AFFORDANCE: u16 = 105;
    /// Focus an attention slot on a subject (high-level, data only).
    pub const KIND_FOCUS_ATTENTION: u16 = 106;

    /// The one private packing seam: every public constructor below builds the
    /// record through this, so the field layout lives in exactly one place.
    #[allow(clippy::too_many_arguments)]
    const fn new_raw(
        kind_code: u16,
        control_code: u32,
        axis_code: u32,
        subject_code: u32,
        affordance_code: u32,
        ticks: u32,
        x: i64,
        y: i64,
        z: i64,
        value: i64,
    ) -> Self {
        ActionIntent {
            kind_code,
            control_code,
            axis_code,
            subject_code,
            affordance_code,
            ticks,
            x,
            y,
            z,
            value,
        }
    }
}

/// Low-level, player-equivalent constructors.
impl ActionIntent {
    /// Do nothing this step.
    pub const fn noop() -> Self {
        Self::new_raw(Self::KIND_NOOP, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    }

    /// Hold position for `ticks` ticks.
    pub const fn wait_ticks(ticks: u32) -> Self {
        Self::new_raw(Self::KIND_WAIT_TICKS, 0, 0, 0, 0, ticks, 0, 0, 0, 0)
    }

    /// Begin holding the abstract control `control_code`.
    pub const fn press_control(control_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_PRESS_CONTROL,
            control_code,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// Stop holding the abstract control `control_code`.
    pub const fn release_control(control_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_RELEASE_CONTROL,
            control_code,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// Drive movement axis `axis_code` by signed `value`.
    pub const fn move_axis(axis_code: u32, value: i64) -> Self {
        Self::new_raw(Self::KIND_MOVE_AXIS, 0, axis_code, 0, 0, 0, 0, 0, 0, value)
    }

    /// Drive look axis `axis_code` by signed `value`.
    pub const fn look_axis(axis_code: u32, value: i64) -> Self {
        Self::new_raw(Self::KIND_LOOK_AXIS, 0, axis_code, 0, 0, 0, 0, 0, 0, value)
    }

    /// Move an abstract pointer to `(x, y)`.
    pub const fn pointer_move(x: i64, y: i64) -> Self {
        Self::new_raw(Self::KIND_POINTER_MOVE, 0, 0, 0, 0, 0, x, y, 0, 0)
    }

    /// Begin a pointer contact with button `control_code`.
    pub const fn pointer_down(control_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_POINTER_DOWN,
            control_code,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// End a pointer contact with button `control_code`.
    pub const fn pointer_up(control_code: u32) -> Self {
        Self::new_raw(Self::KIND_POINTER_UP, control_code, 0, 0, 0, 0, 0, 0, 0, 0)
    }
}

/// High-level neutral constructors (data only — never lowered here).
impl ActionIntent {
    /// Orient toward subject `subject_code`.
    pub const fn look_at_subject(subject_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_LOOK_AT_SUBJECT,
            0,
            0,
            subject_code,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// Orient toward point `(x, y, z)`.
    pub const fn look_at_point(x: i64, y: i64, z: i64) -> Self {
        Self::new_raw(Self::KIND_LOOK_AT_POINT, 0, 0, 0, 0, 0, x, y, z, 0)
    }

    /// Move toward subject `subject_code`.
    pub const fn move_toward_subject(subject_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_MOVE_TOWARD_SUBJECT,
            0,
            0,
            subject_code,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// Move toward point `(x, y, z)`.
    pub const fn move_toward_point(x: i64, y: i64, z: i64) -> Self {
        Self::new_raw(Self::KIND_MOVE_TOWARD_POINT, 0, 0, 0, 0, 0, x, y, z, 0)
    }

    /// Interact with subject `subject_code`.
    pub const fn interact_with_subject(subject_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_INTERACT_WITH_SUBJECT,
            0,
            0,
            subject_code,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// Use affordance `affordance_code`.
    pub const fn use_affordance(affordance_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_USE_AFFORDANCE,
            0,
            0,
            0,
            affordance_code,
            0,
            0,
            0,
            0,
            0,
        )
    }

    /// Focus an attention slot on subject `subject_code`.
    pub const fn focus_attention(subject_code: u32) -> Self {
        Self::new_raw(
            Self::KIND_FOCUS_ATTENTION,
            0,
            0,
            subject_code,
            0,
            0,
            0,
            0,
            0,
            0,
        )
    }
}

/// Field accessors.
impl ActionIntent {
    /// The kind discriminant.
    pub const fn kind_code(self) -> u16 {
        self.kind_code
    }

    /// The abstract control code (press/release/pointer-button intents).
    pub const fn control_code(self) -> u32 {
        self.control_code
    }

    /// The axis code (move/look-axis intents).
    pub const fn axis_code(self) -> u32 {
        self.axis_code
    }

    /// The subject code (subject-targeted intents).
    pub const fn subject_code(self) -> u32 {
        self.subject_code
    }

    /// The affordance code (`use_affordance`).
    pub const fn affordance_code(self) -> u32 {
        self.affordance_code
    }

    /// The tick count (`wait_ticks`).
    pub const fn ticks(self) -> u32 {
        self.ticks
    }

    /// The x coordinate (pointer/point intents).
    pub const fn x(self) -> i64 {
        self.x
    }

    /// The y coordinate (pointer/point intents).
    pub const fn y(self) -> i64 {
        self.y
    }

    /// The z coordinate (point intents).
    pub const fn z(self) -> i64 {
        self.z
    }

    /// The signed magnitude (axis intents).
    pub const fn value(self) -> i64 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_level_kinds_set_their_discriminant_and_payload() {
        assert_eq!(ActionIntent::noop().kind_code(), ActionIntent::KIND_NOOP);
        assert_eq!(ActionIntent::wait_ticks(9).ticks(), 9);
        assert_eq!(
            ActionIntent::wait_ticks(9).kind_code(),
            ActionIntent::KIND_WAIT_TICKS
        );
        assert_eq!(ActionIntent::press_control(7).control_code(), 7);
        assert_eq!(
            ActionIntent::press_control(7).kind_code(),
            ActionIntent::KIND_PRESS_CONTROL
        );
        assert_eq!(ActionIntent::release_control(7).control_code(), 7);
        assert_eq!(
            ActionIntent::release_control(7).kind_code(),
            ActionIntent::KIND_RELEASE_CONTROL
        );
        assert_eq!(ActionIntent::move_axis(2, -5).axis_code(), 2);
        assert_eq!(ActionIntent::move_axis(2, -5).value(), -5);
        assert_eq!(
            ActionIntent::move_axis(2, -5).kind_code(),
            ActionIntent::KIND_MOVE_AXIS
        );
        assert_eq!(ActionIntent::look_axis(3, 4).axis_code(), 3);
        assert_eq!(ActionIntent::look_axis(3, 4).value(), 4);
        assert_eq!(
            ActionIntent::look_axis(3, 4).kind_code(),
            ActionIntent::KIND_LOOK_AXIS
        );
        let pm = ActionIntent::pointer_move(11, 22);
        assert_eq!((pm.x(), pm.y()), (11, 22));
        assert_eq!(pm.kind_code(), ActionIntent::KIND_POINTER_MOVE);
        assert_eq!(ActionIntent::pointer_down(1).control_code(), 1);
        assert_eq!(
            ActionIntent::pointer_down(1).kind_code(),
            ActionIntent::KIND_POINTER_DOWN
        );
        assert_eq!(ActionIntent::pointer_up(1).control_code(), 1);
        assert_eq!(
            ActionIntent::pointer_up(1).kind_code(),
            ActionIntent::KIND_POINTER_UP
        );
    }

    #[test]
    fn high_level_kinds_set_their_discriminant_and_payload() {
        assert_eq!(ActionIntent::look_at_subject(8).subject_code(), 8);
        assert_eq!(
            ActionIntent::look_at_subject(8).kind_code(),
            ActionIntent::KIND_LOOK_AT_SUBJECT
        );
        let lp = ActionIntent::look_at_point(1, 2, 3);
        assert_eq!((lp.x(), lp.y(), lp.z()), (1, 2, 3));
        assert_eq!(lp.kind_code(), ActionIntent::KIND_LOOK_AT_POINT);
        assert_eq!(ActionIntent::move_toward_subject(8).subject_code(), 8);
        assert_eq!(
            ActionIntent::move_toward_subject(8).kind_code(),
            ActionIntent::KIND_MOVE_TOWARD_SUBJECT
        );
        let mp = ActionIntent::move_toward_point(4, 5, 6);
        assert_eq!((mp.x(), mp.y(), mp.z()), (4, 5, 6));
        assert_eq!(mp.kind_code(), ActionIntent::KIND_MOVE_TOWARD_POINT);
        assert_eq!(ActionIntent::interact_with_subject(8).subject_code(), 8);
        assert_eq!(
            ActionIntent::interact_with_subject(8).kind_code(),
            ActionIntent::KIND_INTERACT_WITH_SUBJECT
        );
        assert_eq!(ActionIntent::use_affordance(9).affordance_code(), 9);
        assert_eq!(
            ActionIntent::use_affordance(9).kind_code(),
            ActionIntent::KIND_USE_AFFORDANCE
        );
        assert_eq!(ActionIntent::focus_attention(8).subject_code(), 8);
        assert_eq!(
            ActionIntent::focus_attention(8).kind_code(),
            ActionIntent::KIND_FOCUS_ATTENTION
        );
    }

    #[test]
    fn kind_constants_have_exact_stable_codes() {
        // Canonical action kind codes — pinned to exact numbers. The distinctness
        // test below cannot catch a wrong absolute value; this one can.
        assert_eq!(ActionIntent::KIND_NOOP, 0);
        assert_eq!(ActionIntent::KIND_WAIT_TICKS, 1);
        assert_eq!(ActionIntent::KIND_PRESS_CONTROL, 2);
        assert_eq!(ActionIntent::KIND_RELEASE_CONTROL, 3);
        assert_eq!(ActionIntent::KIND_MOVE_AXIS, 4);
        assert_eq!(ActionIntent::KIND_LOOK_AXIS, 5);
        assert_eq!(ActionIntent::KIND_POINTER_MOVE, 6);
        assert_eq!(ActionIntent::KIND_POINTER_DOWN, 7);
        assert_eq!(ActionIntent::KIND_POINTER_UP, 8);
        assert_eq!(ActionIntent::KIND_LOOK_AT_SUBJECT, 100);
        assert_eq!(ActionIntent::KIND_LOOK_AT_POINT, 101);
        assert_eq!(ActionIntent::KIND_MOVE_TOWARD_SUBJECT, 102);
        assert_eq!(ActionIntent::KIND_MOVE_TOWARD_POINT, 103);
        assert_eq!(ActionIntent::KIND_INTERACT_WITH_SUBJECT, 104);
        assert_eq!(ActionIntent::KIND_USE_AFFORDANCE, 105);
        assert_eq!(ActionIntent::KIND_FOCUS_ATTENTION, 106);
    }

    #[test]
    fn all_kind_codes_are_distinct() {
        let codes = [
            ActionIntent::KIND_NOOP,
            ActionIntent::KIND_WAIT_TICKS,
            ActionIntent::KIND_PRESS_CONTROL,
            ActionIntent::KIND_RELEASE_CONTROL,
            ActionIntent::KIND_MOVE_AXIS,
            ActionIntent::KIND_LOOK_AXIS,
            ActionIntent::KIND_POINTER_MOVE,
            ActionIntent::KIND_POINTER_DOWN,
            ActionIntent::KIND_POINTER_UP,
            ActionIntent::KIND_LOOK_AT_SUBJECT,
            ActionIntent::KIND_LOOK_AT_POINT,
            ActionIntent::KIND_MOVE_TOWARD_SUBJECT,
            ActionIntent::KIND_MOVE_TOWARD_POINT,
            ActionIntent::KIND_INTERACT_WITH_SUBJECT,
            ActionIntent::KIND_USE_AFFORDANCE,
            ActionIntent::KIND_FOCUS_ATTENTION,
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "kind codes must be distinct");
    }

    #[test]
    fn derives_are_exercised() {
        let a = ActionIntent::press_control(5);
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, ActionIntent::press_control(6));
        assert!(format!("{a:?}").contains("ActionIntent"));
    }
}
