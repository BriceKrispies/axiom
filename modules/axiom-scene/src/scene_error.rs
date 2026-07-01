//! The scene module's deterministic error value.

use axiom_math::MathError;

use crate::scene_error_code::SceneErrorCode;

/// A deterministic scene-module error.
///
/// Identity is `(code, math-cause-identity)`. Two errors with the same
/// [`SceneErrorCode`] and the same wrapped [`MathError`] compare equal
/// regardless of the static human message — error checks stay
/// machine-stable across builds and replays.
#[derive(Debug, Clone, Copy)]
pub struct SceneError {
    code: SceneErrorCode,
    message: &'static str,
    math: Option<MathError>,
}

impl SceneError {
    /// A scene-only error without a wrapped math cause.
    pub const fn new(code: SceneErrorCode, message: &'static str) -> Self {
        SceneError {
            code,
            message,
            math: None,
        }
    }

    /// A scene error that wraps a math validation failure (e.g. an
    /// invalid camera projection parameter).
    pub const fn with_math(code: SceneErrorCode, message: &'static str, cause: MathError) -> Self {
        SceneError {
            code,
            message,
            math: Some(cause),
        }
    }

    pub const fn missing_node(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::MissingNode, message)
    }

    pub const fn missing_camera(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::MissingCamera, message)
    }

    pub const fn missing_light(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::MissingLight, message)
    }

    pub const fn missing_renderable(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::MissingRenderable, message)
    }

    pub const fn missing_bounds(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::MissingBounds, message)
    }

    pub const fn self_parenting(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::SelfParenting, message)
    }

    pub const fn hierarchy_cycle(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::HierarchyCycle, message)
    }

    pub const fn invalid_camera_parameters(message: &'static str, cause: MathError) -> Self {
        SceneError::with_math(SceneErrorCode::InvalidCameraParameters, message, cause)
    }

    pub const fn invalid_light_parameters(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::InvalidLightParameters, message)
    }

    pub const fn invalid_renderable_reference(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::InvalidRenderableReference, message)
    }

    pub const fn missing_sdf_shape(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::MissingSdfShape, message)
    }

    pub const fn invalid_sdf_shape_parameters(message: &'static str) -> Self {
        SceneError::new(SceneErrorCode::InvalidSdfShapeParameters, message)
    }

    pub const fn code(&self) -> SceneErrorCode {
        self.code
    }

    pub const fn message(&self) -> &'static str {
        self.message
    }

    pub const fn math(&self) -> Option<MathError> {
        self.math
    }
}

/// Equality on machine identity only.
impl PartialEq for SceneError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.math == other.math)
    }
}

impl Eq for SceneError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::MathErrorCode;

    fn math_cause() -> MathError {
        MathError::invalid_matrix_operation("synthetic")
    }

    #[test]
    fn identity_ignores_message() {
        let a = SceneError::new(SceneErrorCode::MissingNode, "x");
        let b = SceneError::new(SceneErrorCode::MissingNode, "totally different");
        assert_eq!(a, b);
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = SceneError::new(SceneErrorCode::MissingNode, "");
        let b = SceneError::new(SceneErrorCode::HierarchyCycle, "");
        assert_ne!(a, b);
    }

    #[test]
    fn shorthand_constructors_use_their_codes() {
        assert_eq!(
            SceneError::missing_node("").code(),
            SceneErrorCode::MissingNode
        );
        assert_eq!(
            SceneError::missing_camera("").code(),
            SceneErrorCode::MissingCamera
        );
        assert_eq!(
            SceneError::missing_light("").code(),
            SceneErrorCode::MissingLight
        );
        assert_eq!(
            SceneError::missing_renderable("").code(),
            SceneErrorCode::MissingRenderable
        );
        assert_eq!(
            SceneError::missing_bounds("").code(),
            SceneErrorCode::MissingBounds
        );
        assert_eq!(
            SceneError::self_parenting("").code(),
            SceneErrorCode::SelfParenting
        );
        assert_eq!(
            SceneError::hierarchy_cycle("").code(),
            SceneErrorCode::HierarchyCycle
        );
        assert_eq!(
            SceneError::invalid_light_parameters("").code(),
            SceneErrorCode::InvalidLightParameters
        );
        assert_eq!(
            SceneError::invalid_renderable_reference("").code(),
            SceneErrorCode::InvalidRenderableReference
        );
        assert_eq!(
            SceneError::missing_sdf_shape("").code(),
            SceneErrorCode::MissingSdfShape
        );
        assert_eq!(
            SceneError::invalid_sdf_shape_parameters("").code(),
            SceneErrorCode::InvalidSdfShapeParameters
        );
    }

    #[test]
    fn wraps_a_math_error_and_preserves_identity() {
        let wrapped = SceneError::invalid_camera_parameters("bad", math_cause());
        assert_eq!(wrapped.code(), SceneErrorCode::InvalidCameraParameters);
        assert_eq!(
            wrapped.math().unwrap().code(),
            MathErrorCode::InvalidMatrixOperation
        );
    }

    #[test]
    fn wrapped_and_unwrapped_are_not_equal() {
        let bare = SceneError::new(SceneErrorCode::InvalidCameraParameters, "x");
        let wrapped =
            SceneError::with_math(SceneErrorCode::InvalidCameraParameters, "x", math_cause());
        assert_ne!(bare, wrapped);
    }

    #[test]
    fn message_is_preserved_but_not_part_of_identity() {
        let e = SceneError::new(SceneErrorCode::MissingNode, "node not found");
        assert_eq!(e.message(), "node not found");
    }
}
