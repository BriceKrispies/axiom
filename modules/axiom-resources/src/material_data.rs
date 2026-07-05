//! CPU-side material description.

use axiom_kernel::Ratio;
use axiom_math::{Vec3, Vec4};

use crate::resource_id::ResourceId;

/// A const `Ratio` from a literal, built in const context. The `match` lives in a
/// macro expansion, so the branchless lint skips it and the fallible conversion
/// never runs at runtime.
macro_rules! ratio_lit {
    ($value:expr) => {{
        const R: Ratio = match Ratio::new($value) {
            Ok(r) => r,
            Err(_) => panic!("material ratio literal is finite"),
        };
        R
    }};
}

/// One CPU-side material: a stable id, a name, and the **full render material
/// catalog** — a linear-RGBA `base_color`, an optional albedo `texture`, an
/// `emissive` self-illumination colour, a `roughness` (`0` mirror-smooth … `1`
/// matte), and an `opacity` (`1` opaque). This is the resource tier's one
/// material contract, grown to match what the render tier
/// (`axiom_render::RenderMaterial`) accepts so a full material is authorable
/// through the resource table — not only through the render pipeline's own
/// material builder (audit M9).
///
/// ## Id-space mapping (scene `MaterialRef` ⇄ resources `ResourceId`)
/// The scene's opaque `MaterialRef` and this module's [`ResourceId`] share **one
/// numeric identity space by convention**: an app registers a material here,
/// receives a `ResourceId`, and stamps that same `u64` (`ResourceId::raw()`) onto
/// the scene renderable's `MaterialRef` (`MaterialRef::from_raw(id.raw())`). The
/// two modules never name each other's type — the app is the single owner that
/// bridges them — but the `u64` is the same on both sides, so a renderable's
/// material ref resolves to this material by equal raw id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialData {
    id: ResourceId,
    name: &'static str,
    base_color: Vec4,
    texture: Option<ResourceId>,
    emissive: Vec3,
    roughness: Ratio,
    opacity: Ratio,
}

impl MaterialData {
    /// A material with the default catalog surface: non-emissive, fully-matte,
    /// fully-opaque. The common "basic lit" case.
    pub const fn new(
        id: ResourceId,
        name: &'static str,
        base_color: Vec4,
        texture: Option<ResourceId>,
    ) -> Self {
        MaterialData::new_lit(
            id,
            name,
            base_color,
            texture,
            Vec3::ZERO,
            ratio_lit!(1.0),
            ratio_lit!(1.0),
        )
    }

    /// A material with the full catalog surface specified — the resource-tier
    /// mirror of `axiom_render::RenderApi::add_input_lit_material`.
    pub const fn new_lit(
        id: ResourceId,
        name: &'static str,
        base_color: Vec4,
        texture: Option<ResourceId>,
        emissive: Vec3,
        roughness: Ratio,
        opacity: Ratio,
    ) -> Self {
        MaterialData {
            id,
            name,
            base_color,
            texture,
            emissive,
            roughness,
            opacity,
        }
    }

    pub const fn id(&self) -> ResourceId {
        self.id
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn base_color(&self) -> Vec4 {
        self.base_color
    }

    pub const fn texture(&self) -> Option<ResourceId> {
        self.texture
    }

    /// The self-illumination colour added on top of the lit result.
    pub const fn emissive(&self) -> Vec3 {
        self.emissive
    }

    /// The surface roughness (`0` = mirror-smooth, `1` = matte).
    pub const fn roughness(&self) -> Ratio {
        self.roughness
    }

    /// The material opacity (`1` = opaque).
    pub const fn opacity(&self) -> Ratio {
        self.opacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn half() -> Ratio {
        Ratio::new(0.5).expect("finite")
    }

    #[test]
    fn accessors_round_trip() {
        let m = MaterialData::new(
            ResourceId::from_raw(1),
            "basic",
            Vec4::new(1.0, 0.0, 0.0, 1.0),
            Some(ResourceId::from_raw(2)),
        );
        assert_eq!(m.id().raw(), 1);
        assert_eq!(m.name(), "basic");
        assert_eq!(m.base_color(), Vec4::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(m.texture(), Some(ResourceId::from_raw(2)));
    }

    #[test]
    fn new_defaults_the_catalog_fields() {
        let m = MaterialData::new(ResourceId::from_raw(1), "basic", Vec4::ONE, None);
        assert_eq!(m.emissive(), Vec3::ZERO);
        assert_eq!(m.roughness().get(), 1.0);
        assert_eq!(m.opacity().get(), 1.0);
    }

    #[test]
    fn new_lit_carries_the_full_catalog() {
        let m = MaterialData::new_lit(
            ResourceId::from_raw(2),
            "lit",
            Vec4::ONE,
            Some(ResourceId::from_raw(3)),
            Vec3::new(0.0, 1.0, 0.0),
            half(),
            half(),
        );
        assert_eq!(m.emissive(), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(m.roughness().get(), 0.5);
        assert_eq!(m.opacity().get(), 0.5);
        assert_eq!(m.texture(), Some(ResourceId::from_raw(3)));
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = MaterialData::new(ResourceId::from_raw(1), "x", Vec4::ONE, None);
        let b = MaterialData::new(ResourceId::from_raw(1), "x", Vec4::ONE, None);
        let c = MaterialData::new(ResourceId::from_raw(1), "x", Vec4::ZERO, None);
        let d = MaterialData::new_lit(
            ResourceId::from_raw(1),
            "x",
            Vec4::ONE,
            None,
            Vec3::ONE,
            half(),
            half(),
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }
}
