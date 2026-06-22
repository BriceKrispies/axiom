//! Render-facing object: world transform + mesh + material + visibility.

use axiom_math::Mat4;

/// One render-facing object: a stable identity (`id`) + world transform
/// (`Mat4`) + mesh index (into [`crate::RenderInput::meshes`]) + material
/// index + visibility flag. The id is carried through to the object's
/// `DrawIndexed` command so a backend can preserve object identity (picking /
/// hit-testing) from the command list alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderObject {
    id: u64,
    world: Mat4,
    mesh_idx: u32,
    material_idx: u32,
    visible: bool,
}

impl RenderObject {
    pub const fn new(id: u64, world: Mat4, mesh_idx: u32, material_idx: u32, visible: bool) -> Self {
        RenderObject {
            id,
            world,
            mesh_idx,
            material_idx,
            visible,
        }
    }

    /// The object's stable identity, threaded into its `DrawIndexed` command.
    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn world(&self) -> Mat4 {
        self.world
    }

    pub const fn mesh_idx(&self) -> u32 {
        self.mesh_idx
    }

    pub const fn material_idx(&self) -> u32 {
        self.material_idx
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let o = RenderObject::new(42, Mat4::IDENTITY, 1, 2, true);
        assert_eq!(o.id(), 42);
        assert_eq!(o.world(), Mat4::IDENTITY);
        assert_eq!(o.mesh_idx(), 1);
        assert_eq!(o.material_idx(), 2);
        assert!(o.visible());
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderObject::new(1, Mat4::IDENTITY, 0, 0, true);
        let b = RenderObject::new(1, Mat4::IDENTITY, 0, 0, true);
        let c = RenderObject::new(1, Mat4::IDENTITY, 0, 0, false);
        // A differing id alone breaks equality.
        let d = RenderObject::new(2, Mat4::IDENTITY, 0, 0, true);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }
}
