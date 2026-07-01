//! Directional / point light component.

use axiom_kernel::{
    BinaryReader, BinaryWriter, FieldSchema, KernelResult, Ratio, Reflect, TypeSchema,
};
use axiom_math::{MathApi, Vec3};

use crate::light_kind::LightKind;
use crate::scene_error::SceneError;
use crate::scene_result::SceneResult;

/// A light component, stored on the node entity it belongs to.
///
/// Plain data: a kind, a colour, and a positive finite intensity. The node the
/// light follows is the entity this component is keyed by. All validation flows
/// through [`MathApi::validate_finite`] so the light inherits the engine's
/// scalar discipline rather than rolling its own `is_finite` check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    kind: LightKind,
    color: Vec3,
    intensity: Ratio,
}

impl Light {
    /// The reflected shape of a light component.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Light",
        &[
            FieldSchema::new("kind", "u32"),
            FieldSchema::new("color", "Vec3"),
            FieldSchema::new("intensity", "Ratio"),
        ],
    );

    /// Build a directional light. `color` is a linear-RGB triple; each component
    /// must be finite and non-negative. `intensity` must be finite and
    /// non-negative.
    pub fn directional(math: &MathApi, color: Vec3, intensity: Ratio) -> SceneResult<Self> {
        Light::build(math, LightKind::Directional, color, intensity)
    }

    /// Build a point light. Same validation rules as [`Light::directional`].
    pub fn point(math: &MathApi, color: Vec3, intensity: Ratio) -> SceneResult<Self> {
        Light::build(math, LightKind::Point, color, intensity)
    }

    fn build(math: &MathApi, kind: LightKind, color: Vec3, intensity: Ratio) -> SceneResult<Self> {
        // `intensity` is a `Ratio`, so it is already finite; only the raw colour
        // components still need the engine's finite check.
        [color.x, color.y, color.z]
            .iter()
            .all(|&component| math.validate_finite(component).is_ok())
            .then_some(())
            .ok_or_else(|| SceneError::invalid_light_parameters("light parameters must be finite"))
            .and_then(|()| {
                // Colour components must be non-negative (the inverse of the
                // original `x < 0 || y < 0 || z < 0` reject); operands are pure
                // comparisons, so the short-circuiting `||` becomes a bitwise `&`.
                ((color.x >= 0.0) & (color.y >= 0.0) & (color.z >= 0.0))
                    .then_some(())
                    .ok_or_else(|| {
                        SceneError::invalid_light_parameters(
                            "light colour components must be non-negative",
                        )
                    })
            })
            .and_then(|()| {
                (intensity.get() >= 0.0).then_some(()).ok_or_else(|| {
                    SceneError::invalid_light_parameters("light intensity must be non-negative")
                })
            })
            .map(|()| Light {
                kind,
                color,
                intensity,
            })
    }

    pub const fn kind(&self) -> LightKind {
        self.kind
    }

    pub const fn color(&self) -> Vec3 {
        self.color
    }

    pub const fn intensity(&self) -> Ratio {
        self.intensity
    }
}

impl Reflect for Light {
    const SCHEMA: TypeSchema = Light::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.kind.reflect_write(writer);
        self.color.reflect_write(writer);
        self.intensity.reflect_write(writer);
    }

    /// Reconstruct directly from the stored fields (the colour was finite and
    /// non-negative when first built; `Ratio` re-validates finiteness on read).
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        LightKind::reflect_read(reader).and_then(|kind| {
            Vec3::reflect_read(reader).and_then(|color| {
                Ratio::reflect_read(reader).map(|intensity| Light {
                    kind,
                    color,
                    intensity,
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;

    fn math() -> MathApi {
        MathApi::new()
    }

    fn rat(x: f32) -> Ratio {
        Ratio::new(x).unwrap()
    }

    #[test]
    fn directional_light_is_built_with_valid_params() {
        let l = Light::directional(&math(), Vec3::new(1.0, 1.0, 1.0), rat(5.0)).unwrap();
        assert_eq!(l.kind(), LightKind::Directional);
        assert_eq!(l.intensity().get(), 5.0);
    }

    #[test]
    fn point_light_is_built_with_valid_params() {
        let l = Light::point(&math(), Vec3::new(0.5, 0.5, 0.5), rat(2.0)).unwrap();
        assert_eq!(l.kind(), LightKind::Point);
        assert_eq!(l.color().x, 0.5);
    }

    #[test]
    fn negative_intensity_is_rejected() {
        // A `Ratio` is finite but may be negative; the non-negative intensity
        // guard still rejects it.
        let err = Light::directional(&math(), Vec3::ONE, rat(-1.0)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
    }

    #[test]
    fn negative_color_components_are_rejected_per_channel() {
        for bad in [
            Vec3::new(-0.1, 0.0, 0.0),
            Vec3::new(0.0, -0.1, 0.0),
            Vec3::new(0.0, 0.0, -0.1),
        ] {
            let err = Light::directional(&math(), bad, rat(1.0)).unwrap_err();
            assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
        }
    }

    #[test]
    fn nan_color_component_is_rejected() {
        let err = Light::point(&math(), Vec3::new(f32::NAN, 0.0, 0.0), rat(1.0)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
    }

    #[test]
    fn zero_intensity_and_zero_channels_are_allowed() {
        let l = Light::directional(&math(), Vec3::ONE, rat(0.0)).unwrap();
        assert_eq!(l.intensity().get(), 0.0);
        assert_eq!(
            Light::directional(&math(), Vec3::new(0.0, 1.0, 1.0), rat(1.0))
                .unwrap()
                .color()
                .x,
            0.0
        );
        assert_eq!(
            Light::directional(&math(), Vec3::new(1.0, 0.0, 1.0), rat(1.0))
                .unwrap()
                .color()
                .y,
            0.0
        );
        assert_eq!(
            Light::directional(&math(), Vec3::new(1.0, 1.0, 0.0), rat(1.0))
                .unwrap()
                .color()
                .z,
            0.0
        );
    }

    #[test]
    fn schema_names_the_light_fields() {
        assert_eq!(Light::SCHEMA.name(), "Light");
        assert_eq!(Light::SCHEMA.fields().len(), 3);
        assert_eq!(Light::SCHEMA.fields()[1].name(), "color");
    }

    #[test]
    fn reflect_round_trips_both_kinds_and_rejects_truncation() {
        let d = Light::directional(&math(), Vec3::new(0.2, 0.4, 0.6), rat(3.0)).unwrap();
        let p = Light::point(&math(), Vec3::new(1.0, 0.0, 0.0), rat(1.0)).unwrap();
        for light in [d, p] {
            let mut w = BinaryWriter::new();
            light.reflect_write(&mut w);
            assert_eq!(
                Light::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap(),
                light
            );
        }
        assert!(Light::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }
}
