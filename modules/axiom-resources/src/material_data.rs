//! CPU-side material description.

use axiom_math::Vec4;

use crate::resource_id::ResourceId;

/// One CPU-side material: a stable id, a name, a base colour, and an
/// optional texture id.
///
/// "Basic lit" is the only material category the vertical slice
/// supports today. The colour is in linear RGBA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialData {
    id: ResourceId,
    name: &'static str,
    base_color: Vec4,
    texture: Option<ResourceId>,
}

impl MaterialData {
    pub const fn new(
        id: ResourceId,
        name: &'static str,
        base_color: Vec4,
        texture: Option<ResourceId>,
    ) -> Self {
        MaterialData {
            id,
            name,
            base_color,
            texture,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn equality_requires_all_fields() {
        let a = MaterialData::new(ResourceId::from_raw(1), "x", Vec4::ONE, None);
        let b = MaterialData::new(ResourceId::from_raw(1), "x", Vec4::ONE, None);
        let c = MaterialData::new(ResourceId::from_raw(1), "x", Vec4::ZERO, None);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
