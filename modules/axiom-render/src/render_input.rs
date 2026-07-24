//! Scene-independent render input.

use axiom_kernel::Ratio;
use axiom_math::{Mat4, Vec2, Vec3, Vec4};

use crate::render_camera::RenderCamera;
use crate::render_light::RenderLight;
use crate::render_material::RenderMaterial;
use crate::render_mesh::RenderMesh;
use crate::render_object::RenderObject;
use crate::render_sdf::RenderSdf;

/// The scene-independent input the renderer turns into a
/// [`crate::RenderCommandList`].
///
/// `RenderInput` is plain data: viewport dimensions, an optional
/// camera, a clear colour, deduplicated `RenderMesh` and
/// `RenderMaterial` lists, the lights to apply, and the objects to
/// draw. It contains no scene-graph concepts, no `SceneNodeId`s, no
/// resource ids; the app pre-translates those.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RenderInput {
    viewport_width: u32,
    viewport_height: u32,
    clear_color: [f32; 4],
    camera: Option<RenderCamera>,
    meshes: Vec<RenderMesh>,
    materials: Vec<RenderMaterial>,
    lights: Vec<RenderLight>,
    objects: Vec<RenderObject>,
    sdf_shapes: Vec<RenderSdf>,
}

impl RenderInput {
    pub const fn new(viewport_width: u32, viewport_height: u32) -> Self {
        RenderInput {
            viewport_width,
            viewport_height,
            clear_color: [0.0, 0.0, 0.0, 1.0],
            camera: None,
            meshes: Vec::new(),
            materials: Vec::new(),
            lights: Vec::new(),
            objects: Vec::new(),
            sdf_shapes: Vec::new(),
        }
    }

    /// Clear every list (reusing capacity) and retarget the viewport, resetting
    /// the clear colour + camera — the per-frame reuse entry point. A retained
    /// input is `reset` then refilled each frame instead of allocated fresh,
    /// which is what keeps the render pipeline from churning wasm memory.
    pub fn reset(&mut self, viewport_width: u32, viewport_height: u32) {
        self.viewport_width = viewport_width;
        self.viewport_height = viewport_height;
        self.clear_color = [0.0, 0.0, 0.0, 1.0];
        self.camera = None;
        self.meshes.clear();
        self.materials.clear();
        self.lights.clear();
        self.objects.clear();
        self.sdf_shapes.clear();
    }

    pub fn set_clear_color(&mut self, color: [f32; 4]) {
        self.clear_color = color;
    }

    /// Public builders (counterparts to the `pub(crate)` `add_*`, which take the
    /// opaque render types) that take PRIMITIVES and construct those types
    /// internally — so a composing feature module can fill a retained input it
    /// holds by reference without naming `RenderCamera`/`RenderMesh`/etc.
    pub fn push_camera(&mut self, view: Mat4, projection: Mat4) {
        self.set_camera(RenderCamera::new(view, projection));
    }
    pub fn push_mesh(
        &mut self,
        id: u64,
        positions: Vec<Vec3>,
        normals: Vec<Vec3>,
        uvs: Vec<Vec2>,
        indices: Vec<u32>,
    ) -> u32 {
        self.add_mesh(RenderMesh::new(id, positions, normals, uvs, indices))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn push_lit_material(
        &mut self,
        id: u64,
        base_color: Vec4,
        emissive: Vec3,
        roughness: Ratio,
        opacity: Ratio,
        texture_id: u64,
    ) -> u32 {
        self.add_material(RenderMaterial::new_lit(
            id, base_color, emissive, roughness, opacity, texture_id,
        ))
    }
    pub fn push_object(
        &mut self,
        id: u64,
        world: Mat4,
        mesh_idx: u32,
        material_idx: u32,
        visible: bool,
    ) {
        self.add_object(RenderObject::new(
            id,
            world,
            mesh_idx,
            material_idx,
            visible,
        ));
    }

    pub(crate) fn set_camera(&mut self, camera: RenderCamera) {
        self.camera = Some(camera);
    }

    pub(crate) fn add_mesh(&mut self, mesh: RenderMesh) -> u32 {
        let idx = self.meshes.len() as u32;
        self.meshes.push(mesh);
        idx
    }

    pub(crate) fn add_material(&mut self, material: RenderMaterial) -> u32 {
        let idx = self.materials.len() as u32;
        self.materials.push(material);
        idx
    }

    pub(crate) fn add_light(&mut self, light: RenderLight) {
        self.lights.push(light);
    }

    pub(crate) fn add_object(&mut self, object: RenderObject) {
        self.objects.push(object);
    }

    pub(crate) fn add_sdf_shape(&mut self, shape: RenderSdf) {
        self.sdf_shapes.push(shape);
    }

    pub const fn viewport_width(&self) -> u32 {
        self.viewport_width
    }

    pub const fn viewport_height(&self) -> u32 {
        self.viewport_height
    }

    pub const fn clear_color(&self) -> [f32; 4] {
        self.clear_color
    }

    pub const fn camera(&self) -> Option<RenderCamera> {
        self.camera
    }

    pub fn meshes(&self) -> &[RenderMesh] {
        &self.meshes
    }

    pub fn materials(&self) -> &[RenderMaterial] {
        &self.materials
    }

    pub fn lights(&self) -> &[RenderLight] {
        &self.lights
    }

    pub fn objects(&self) -> &[RenderObject] {
        &self.objects
    }

    pub fn sdf_shapes(&self) -> &[RenderSdf] {
        &self.sdf_shapes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Mat4, Vec2, Vec3, Vec4};

    #[test]
    fn new_input_has_supplied_viewport() {
        let i = RenderInput::new(1920, 1080);
        assert_eq!(i.viewport_width(), 1920);
        assert_eq!(i.viewport_height(), 1080);
        assert!(i.camera().is_none());
        assert!(i.meshes().is_empty());
    }

    #[test]
    fn builder_methods_round_trip() {
        let mut i = RenderInput::new(100, 100);
        i.set_clear_color([0.1, 0.2, 0.3, 1.0]);
        i.set_camera(RenderCamera::new(Mat4::IDENTITY, Mat4::IDENTITY));
        let m = i.add_mesh(RenderMesh::new(
            7,
            vec![Vec3::ZERO],
            vec![Vec3::UNIT_Y],
            vec![Vec2::ZERO],
            vec![0],
        ));
        let mat = i.add_material(RenderMaterial::new(3, Vec4::ONE));
        i.add_object(RenderObject::new(1, Mat4::IDENTITY, m, mat, true));
        i.add_sdf_shape(RenderSdf::new(0, Mat4::IDENTITY, Vec3::ONE, Vec4::ONE));
        assert_eq!(i.clear_color(), [0.1, 0.2, 0.3, 1.0]);
        assert!(i.camera().is_some());
        assert_eq!(i.meshes().len(), 1);
        assert_eq!(i.materials().len(), 1);
        assert_eq!(i.objects().len(), 1);
        assert_eq!(i.sdf_shapes().len(), 1);
    }

    #[test]
    fn equality_requires_same_content() {
        let a = RenderInput::new(100, 100);
        let b = RenderInput::new(100, 100);
        let c = RenderInput::new(200, 100);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
