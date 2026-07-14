//! Expanding the demo recipes into a live Axiom scene, and reporting the cost.
//!
//! This is the Player's whole job: bake each recipe into a neutral buffer, hand
//! that buffer to the ordinary runtime-resource hooks
//! (`add_texture_data` / `add_mesh_data` / `add_material`), and place the results
//! in a normal scene with a camera and a light. Nothing here is a parallel
//! "procedural scene" — every generated result becomes a plain Axiom resource.

use std::fmt;
use std::time::Instant;

use axiom::prelude::{
    Angle, App, Camera, Color, DirectionalLight, Material, MeshData, Meters, PerspectiveProjection,
    Ratio, RunningApp, Spawn, Transform, Vec3,
};
use axiom_math::Quat;
use axiom_proc_mesh::{MeshBuffer, ProcMeshApi};
use axiom_proc_texture::{ProcTextureApi, TextureBuffer};
use axiom_recipe::RecipeGraph;

use crate::recipes::DemoRecipes;

/// The size + shape report for one expansion — deliverable #7.
#[derive(Debug, Clone)]
pub struct RoomReport {
    /// Total serialized bytes of every recipe (what ships).
    pub recipe_bytes: usize,
    /// Total RAM of the generated textures (`width·height·4`, what does not ship).
    pub texture_memory_bytes: usize,
    /// Total generated vertex count across every mesh.
    pub mesh_vertices: usize,
    /// Total generated index count across every mesh.
    pub mesh_indices: usize,
    /// Wall-clock microseconds spent baking + registering (a perf figure, not a
    /// determinism input).
    pub expansion_micros: u128,
    /// Number of generated textures registered.
    pub texture_count: usize,
    /// Number of generated meshes registered.
    pub mesh_count: usize,
    /// Number of materials registered.
    pub material_count: usize,
    /// Number of renderables the scene ended up with.
    pub renderable_count: usize,
}

impl RoomReport {
    /// The deterministic fields — everything except wall-clock time. Two
    /// expansions of the same recipes at the same seed share this fingerprint.
    pub fn fingerprint(&self) -> (usize, usize, usize, usize, usize, usize, usize) {
        (
            self.recipe_bytes,
            self.texture_memory_bytes,
            self.mesh_vertices,
            self.mesh_indices,
            self.texture_count,
            self.mesh_count,
            self.material_count,
        )
    }
}

impl fmt::Display for RoomReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Axiom Proc v0 — demo room size report")?;
        writeln!(f, "  recipe bytes (shipped) : {}", self.recipe_bytes)?;
        writeln!(
            f,
            "  texture RAM (generated): {} bytes",
            self.texture_memory_bytes
        )?;
        writeln!(f, "  mesh vertices          : {}", self.mesh_vertices)?;
        writeln!(f, "  mesh indices           : {}", self.mesh_indices)?;
        writeln!(f, "  expansion time         : {} us", self.expansion_micros)?;
        writeln!(
            f,
            "  resources: {} textures, {} meshes, {} materials, {} renderables",
            self.texture_count, self.mesh_count, self.material_count, self.renderable_count
        )?;
        let ratio = self.texture_memory_bytes.max(1) as f64 / self.recipe_bytes.max(1) as f64;
        write!(
            f,
            "  expansion ratio        : {ratio:.1}x (generated RAM / recipe bytes)"
        )
    }
}

/// Convert a neutral mesh buffer into the engine's `MeshData` — the same math
/// vector types on both sides, so this is a plain move of the streams.
fn to_mesh_data(mb: &MeshBuffer) -> MeshData {
    MeshData::new(
        mb.positions().to_vec(),
        mb.normals().to_vec(),
        mb.uvs().to_vec(),
        mb.indices().to_vec(),
    )
}

/// A unit `Ratio`.
fn unit(x: f32) -> Ratio {
    Ratio::new(x).expect("authored ratio is in range")
}

/// Expand the standard demo room at `seed`. Convenience over [`expand`].
pub fn expand_room(seed: u64) -> (RunningApp, RoomReport) {
    expand(&DemoRecipes::build(), seed)
}

/// Expand `recipes` into a running app + report at `seed`. Every resource is
/// generated from the recipe bytes alone — no editor state is consulted.
pub fn expand(recipes: &DemoRecipes, seed: u64) -> (RunningApp, RoomReport) {
    let start = Instant::now();
    let recipe_bytes = recipes.total_bytes();
    let textures = ProcTextureApi::new();
    let meshes = ProcMeshApi::new();

    let mut app = App::new().setup(|_, _, _| {}).build();

    // Textures → registered as runtime texture resources.
    let brick = bake_texture(&textures, &recipes.brick_texture, seed);
    let floor_t = bake_texture(&textures, &recipes.floor_texture, seed);
    let crate_t = bake_texture(&textures, &recipes.crate_texture, seed);
    let texture_memory_bytes = px_bytes(&brick) + px_bytes(&floor_t) + px_bytes(&crate_t);
    let brick_tex = register_texture(&mut app, brick);
    let floor_tex = register_texture(&mut app, floor_t);
    let crate_tex = register_texture(&mut app, crate_t);

    // Materials → generated textures lit as ordinary materials.
    let wall_mat = app.add_material(Material::lit(Color::WHITE).with_custom_texture(brick_tex));
    let floor_mat = app.add_material(Material::lit(Color::WHITE).with_custom_texture(floor_tex));
    let crate_mat = app.add_material(Material::lit(Color::WHITE).with_custom_texture(crate_tex));

    // Meshes → registered as runtime mesh resources.
    let floor_mb = meshes
        .bake(&recipes.floor_mesh, seed)
        .expect("floor mesh recipe is valid");
    let wall_mb = meshes
        .bake(&recipes.wall_mesh, seed)
        .expect("wall mesh recipe is valid");
    let crate_mb = meshes
        .bake(&recipes.crate_mesh, seed)
        .expect("crate mesh recipe is valid");
    let mesh_vertices = floor_mb.vertex_count() + wall_mb.vertex_count() + crate_mb.vertex_count();
    let mesh_indices =
        floor_mb.indices().len() + wall_mb.indices().len() + crate_mb.indices().len();
    let floor = app
        .add_mesh_data(to_mesh_data(&floor_mb))
        .expect("floor mesh is well-formed");
    let wall = app
        .add_mesh_data(to_mesh_data(&wall_mb))
        .expect("wall mesh is well-formed");
    let crate_h = app
        .add_mesh_data(to_mesh_data(&crate_mb))
        .expect("crate mesh is well-formed");

    // Scene: floor flat, a thin wall at the back, a crate resting on the floor.
    // Each spawn adds a renderable node to the scene graph.
    let spawns = [
        Spawn::new(
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            floor,
            floor_mat,
        ),
        Spawn::new(
            Transform::new(
                Vec3::new(0.0, 1.5, -5.0),
                Quat::IDENTITY,
                Vec3::new(6.0, 3.0, 0.3),
            ),
            wall,
            wall_mat,
        ),
        Spawn::new(
            Transform::new(
                Vec3::new(1.5, 0.5, -1.0),
                Quat::IDENTITY,
                Vec3::new(1.0, 1.0, 1.0),
            ),
            crate_h,
            crate_mat,
        ),
    ];
    let renderable_count = spawns
        .into_iter()
        .map(|spec| {
            app.spawn(spec);
        })
        .count();

    // Camera looking into the room, and a key light.
    let projection = PerspectiveProjection {
        fov_y: Angle::degrees(60.0),
        near: Meters::new(0.1).expect("near is finite"),
        far: Meters::new(100.0).expect("far is finite"),
    };
    let eye = Transform::from_translation(Vec3::new(0.0, 3.0, 7.0))
        .looking_at(Vec3::new(0.0, 1.0, -2.0), Vec3::UNIT_Y)
        .expect("camera has a valid orientation");
    app.set_camera(Camera::perspective(projection), eye);
    app.add_light(
        DirectionalLight {
            direction: Vec3::new(-0.3, -1.0, -0.2),
            color: Color::WHITE,
            intensity: unit(1.0),
        },
        Transform::IDENTITY,
    );

    let report = RoomReport {
        recipe_bytes,
        texture_memory_bytes,
        mesh_vertices,
        mesh_indices,
        expansion_micros: start.elapsed().as_micros(),
        texture_count: 3,
        mesh_count: 3,
        material_count: 3,
        renderable_count,
    };
    (app, report)
}

/// Bake one texture recipe (panicking only on an authored-invalid recipe).
fn bake_texture(api: &ProcTextureApi, recipe: &RecipeGraph, seed: u64) -> TextureBuffer {
    api.bake(recipe, seed)
        .expect("authored texture recipe is valid")
}

/// The RAM footprint of a generated texture.
fn px_bytes(t: &TextureBuffer) -> usize {
    t.width() as usize * t.height() as usize * 4
}

/// Register a generated texture as a runtime resource, returning its handle id.
fn register_texture(app: &mut RunningApp, t: TextureBuffer) -> u64 {
    let (w, h) = (t.width(), t.height());
    app.add_texture_data(w, h, t.into_pixels())
        .expect("generated texture is well-formed")
        .id()
}
