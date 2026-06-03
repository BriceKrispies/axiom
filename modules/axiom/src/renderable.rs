//! A renderable component an app spawns onto a node: a mesh drawn with a
//! material.

use crate::handle::Handle;
use crate::material::Material;
use crate::mesh::Mesh;

/// A renderable: a [`Mesh`] handle drawn with a [`Material`] handle. Both
/// handles come from the app's `Assets<Mesh>` / `Assets<Material>` collections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Renderable {
    pub mesh: Handle<Mesh>,
    pub material: Handle<Material>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;

    #[test]
    fn carries_its_mesh_and_material_handles() {
        let mut meshes: Assets<Mesh> = Assets::new();
        let mut materials: Assets<Material> = Assets::new();
        let mesh = meshes.add(Mesh::cube());
        let material = materials.add(Material::lit(crate::color::Color::WHITE));
        let r = Renderable { mesh, material };
        assert_eq!(r.mesh, mesh);
        assert_eq!(r.material, material);
    }
}
