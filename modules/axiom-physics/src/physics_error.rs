//! The physics module's deterministic error value.

use axiom_math::MathError;

use crate::physics_error_code::PhysicsErrorCode;

/// A deterministic physics-module error.
///
/// Identity is `(code, math-cause-identity)`. Two errors with the same
/// [`PhysicsErrorCode`] and the same optionally-wrapped [`MathError`] compare
/// equal regardless of the static human message, so error assertions stay
/// machine-stable across builds and replays. No physics operation panics for a
/// normal validation failure — it returns one of these instead.
#[derive(Debug, Clone, Copy)]
pub struct PhysicsError {
    code: PhysicsErrorCode,
    message: &'static str,
    math: Option<MathError>,
}

impl PhysicsError {
    /// A physics-only error without a wrapped math cause.
    pub const fn new(code: PhysicsErrorCode, message: &'static str) -> Self {
        PhysicsError {
            code,
            message,
            math: None,
        }
    }

    /// A physics error that wraps a math validation failure (e.g. a plane
    /// normal that could not be normalized).
    pub const fn with_math(code: PhysicsErrorCode, message: &'static str, cause: MathError) -> Self {
        PhysicsError {
            code,
            message,
            math: Some(cause),
        }
    }

    pub const fn invalid_config(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::InvalidConfig, message)
    }

    pub const fn invalid_mass(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::InvalidMass, message)
    }

    pub const fn invalid_material(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::InvalidMaterial, message)
    }

    pub const fn invalid_collider_shape(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::InvalidColliderShape, message)
    }

    pub const fn body_not_found(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::BodyNotFound, message)
    }

    pub const fn collider_not_found(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::ColliderNotFound, message)
    }

    pub const fn body_capacity_exceeded(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::BodyCapacityExceeded, message)
    }

    pub const fn collider_capacity_exceeded(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::ColliderCapacityExceeded, message)
    }

    pub const fn force_on_non_dynamic_body(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::ForceOnNonDynamicBody, message)
    }

    pub const fn impulse_on_non_dynamic_body(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::ImpulseOnNonDynamicBody, message)
    }

    pub const fn non_finite_input(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::NonFiniteInput, message)
    }

    pub const fn invalid_step(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::InvalidStep, message)
    }

    pub const fn operation_on_disabled_body(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::OperationOnDisabledBody, message)
    }

    pub const fn non_finite_step_result(message: &'static str) -> Self {
        PhysicsError::new(PhysicsErrorCode::NonFiniteStepResult, message)
    }

    pub const fn code(&self) -> PhysicsErrorCode {
        self.code
    }

    pub const fn message(&self) -> &'static str {
        self.message
    }

    pub const fn math(&self) -> Option<MathError> {
        self.math
    }

    /// The stable numeric error code (`PhysicsErrorCode` discriminant). This is
    /// the law-clean way for an external caller to inspect *which* error
    /// occurred without naming the internal code enum: compare against the
    /// documented stable discriminants, or use the `is_*` predicates below.
    pub const fn raw_code(&self) -> u16 {
        self.code.raw()
    }

    /// The error was a non-finite or non-positive mass on a dynamic body.
    pub const fn is_invalid_mass(&self) -> bool {
        matches_code(self.code, PhysicsErrorCode::InvalidMass)
    }

    /// The error referenced a body handle that does not exist.
    pub const fn is_body_not_found(&self) -> bool {
        matches_code(self.code, PhysicsErrorCode::BodyNotFound)
    }

    /// The error was a force applied to a non-dynamic (static/kinematic) body.
    pub const fn is_force_on_non_dynamic_body(&self) -> bool {
        matches_code(self.code, PhysicsErrorCode::ForceOnNonDynamicBody)
    }

    /// The error was an impulse applied to a non-dynamic (static/kinematic) body.
    pub const fn is_impulse_on_non_dynamic_body(&self) -> bool {
        matches_code(self.code, PhysicsErrorCode::ImpulseOnNonDynamicBody)
    }

    /// The error was a force/impulse applied to a currently-disabled body.
    pub const fn is_operation_on_disabled_body(&self) -> bool {
        matches_code(self.code, PhysicsErrorCode::OperationOnDisabledBody)
    }

    /// The error was a rejected step that would have produced non-finite state.
    pub const fn is_non_finite_step_result(&self) -> bool {
        matches_code(self.code, PhysicsErrorCode::NonFiniteStepResult)
    }
}

/// Branchless code equality usable from a `const fn` (the `PhysicsErrorCode`
/// `PartialEq` impl is not `const`).
const fn matches_code(code: PhysicsErrorCode, expected: PhysicsErrorCode) -> bool {
    code.raw() == expected.raw()
}

/// Equality on machine identity only (code + math cause), never the message.
impl PartialEq for PhysicsError {
    fn eq(&self, other: &Self) -> bool {
        // Both operands are pure equality comparisons, so the short-circuiting
        // `&&` would be behaviour-identical to this bitwise `&` (and `&` keeps
        // the spine branchless).
        (self.code == other.code) & (self.math == other.math)
    }
}

impl Eq for PhysicsError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::MathErrorCode;

    fn math_cause() -> MathError {
        MathError::normalize_zero_length("synthetic")
    }

    #[test]
    fn identity_ignores_message() {
        let a = PhysicsError::new(PhysicsErrorCode::InvalidMass, "x");
        let b = PhysicsError::new(PhysicsErrorCode::InvalidMass, "totally different");
        assert_eq!(a, b);
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = PhysicsError::new(PhysicsErrorCode::InvalidMass, "");
        let b = PhysicsError::new(PhysicsErrorCode::InvalidStep, "");
        assert_ne!(a, b);
    }

    #[test]
    fn shorthand_constructors_use_their_codes() {
        assert_eq!(
            PhysicsError::invalid_config("").code(),
            PhysicsErrorCode::InvalidConfig
        );
        assert_eq!(
            PhysicsError::invalid_mass("").code(),
            PhysicsErrorCode::InvalidMass
        );
        assert_eq!(
            PhysicsError::invalid_material("").code(),
            PhysicsErrorCode::InvalidMaterial
        );
        assert_eq!(
            PhysicsError::invalid_collider_shape("").code(),
            PhysicsErrorCode::InvalidColliderShape
        );
        assert_eq!(
            PhysicsError::body_not_found("").code(),
            PhysicsErrorCode::BodyNotFound
        );
        assert_eq!(
            PhysicsError::collider_not_found("").code(),
            PhysicsErrorCode::ColliderNotFound
        );
        assert_eq!(
            PhysicsError::body_capacity_exceeded("").code(),
            PhysicsErrorCode::BodyCapacityExceeded
        );
        assert_eq!(
            PhysicsError::collider_capacity_exceeded("").code(),
            PhysicsErrorCode::ColliderCapacityExceeded
        );
        assert_eq!(
            PhysicsError::force_on_non_dynamic_body("").code(),
            PhysicsErrorCode::ForceOnNonDynamicBody
        );
        assert_eq!(
            PhysicsError::impulse_on_non_dynamic_body("").code(),
            PhysicsErrorCode::ImpulseOnNonDynamicBody
        );
        assert_eq!(
            PhysicsError::non_finite_input("").code(),
            PhysicsErrorCode::NonFiniteInput
        );
        assert_eq!(
            PhysicsError::invalid_step("").code(),
            PhysicsErrorCode::InvalidStep
        );
    }

    #[test]
    fn wraps_a_math_error_and_preserves_identity() {
        let wrapped = PhysicsError::with_math(
            PhysicsErrorCode::InvalidColliderShape,
            "bad normal",
            math_cause(),
        );
        assert_eq!(wrapped.code(), PhysicsErrorCode::InvalidColliderShape);
        assert_eq!(
            wrapped.math().unwrap().code(),
            MathErrorCode::NormalizeZeroLength
        );
    }

    #[test]
    fn wrapped_and_unwrapped_are_not_equal() {
        let bare = PhysicsError::new(PhysicsErrorCode::InvalidColliderShape, "x");
        let wrapped =
            PhysicsError::with_math(PhysicsErrorCode::InvalidColliderShape, "x", math_cause());
        assert_ne!(bare, wrapped);
    }

    #[test]
    fn message_is_preserved_but_not_part_of_identity() {
        let e = PhysicsError::new(PhysicsErrorCode::BodyNotFound, "no such body");
        assert_eq!(e.message(), "no such body");
    }

    #[test]
    fn new_constructors_use_their_codes() {
        assert_eq!(
            PhysicsError::operation_on_disabled_body("").code(),
            PhysicsErrorCode::OperationOnDisabledBody
        );
        assert_eq!(
            PhysicsError::non_finite_step_result("").code(),
            PhysicsErrorCode::NonFiniteStepResult
        );
    }

    #[test]
    fn raw_code_and_predicates_inspect_the_code_without_naming_the_enum() {
        let mass = PhysicsError::invalid_mass("");
        assert_eq!(mass.raw_code(), 2);
        assert!(mass.is_invalid_mass());
        assert!(!mass.is_body_not_found());

        assert!(PhysicsError::body_not_found("").is_body_not_found());
        assert!(PhysicsError::force_on_non_dynamic_body("").is_force_on_non_dynamic_body());
        assert!(PhysicsError::impulse_on_non_dynamic_body("").is_impulse_on_non_dynamic_body());
        assert!(PhysicsError::operation_on_disabled_body("").is_operation_on_disabled_body());
        assert!(PhysicsError::non_finite_step_result("").is_non_finite_step_result());

        // A predicate is false for an unrelated code (covers the false arm).
        let step = PhysicsError::invalid_step("");
        assert!(!step.is_invalid_mass());
        assert!(!step.is_force_on_non_dynamic_body());
        assert!(!step.is_impulse_on_non_dynamic_body());
        assert!(!step.is_operation_on_disabled_body());
        assert!(!step.is_non_finite_step_result());
    }
}
