//! Expanding the recipes into live Axiom scenes: the **title/menu tableau** and
//! the **three-area level**. Both bake every referenced recipe through the
//! existing expander and register the results as ordinary runtime resources
//! (`add_texture_data` / `add_mesh_data` / `add_material`), then place prefab
//! instances from the grammar, add a first-person controller, and light the room.

use std::collections::HashMap;

use axiom::prelude::{
    Angle, App, Camera, Color as EngineColor, DefaultPlugins, DirectionalLight, Entity, Handle, Material, Mesh,
    MeshData, Meters, PerspectiveProjection, PointLight, Ratio, RunningApp, Spawn, Transform, Vec3,
};
use axiom_math::Quat;
use axiom_proc_mesh::ProcMeshApi;
use axiom_proc_texture::ProcTextureApi;

use crate::grammar::{build_level, LevelLayout};
use crate::style::{engine_color, Style};
use crate::{materials, meshes, prefabs, textures};

/// A fully expanded level: the running app plus the layout and the size figures
/// the report and validation need.
pub struct ExpandedLevel {
    /// The running app, scene populated, camera + light placed.
    pub app: RunningApp,
    /// The generated layout (positions the gameplay ruleset reads).
    pub layout: LevelLayout,
    /// Total generated texture RAM (bytes).
    pub texture_bytes: usize,
    /// Total generated vertex count.
    pub mesh_vertices: usize,
    /// Total generated index count.
    pub mesh_indices: usize,
    /// Number of renderable instances placed.
    pub renderable_count: usize,
    /// Total scene entities (renderables + player + light).
    pub entity_count: usize,
}

/// The baked-and-registered resource handles, keyed for prefab resolution.
struct Registry {
    textures: HashMap<u64, u64>,
    meshes: HashMap<u64, Handle<Mesh>>,
    materials: HashMap<&'static str, Handle<Material>>,
    texture_bytes: usize,
    mesh_vertices: usize,
    mesh_indices: usize,
}

/// Bake + register every texture, mesh, and material for `style` into `app`.
fn register_all(app: &mut RunningApp, style: &Style) -> Registry {
    let tex_api = ProcTextureApi::new();
    let mesh_api = ProcMeshApi::new();
    let mut reg = Registry {
        textures: HashMap::new(),
        meshes: HashMap::new(),
        materials: HashMap::new(),
        texture_bytes: 0,
        mesh_vertices: 0,
        mesh_indices: 0,
    };

    for (_, recipe) in textures::catalog(style) {
        let buf = tex_api.bake(&recipe, style.level_seed).expect("texture recipe bakes");
        reg.texture_bytes += buf.width() as usize * buf.height() as usize * 4;
        let (w, h) = (buf.width(), buf.height());
        let handle = app.add_texture_data(w, h, buf.into_pixels()).expect("texture registers");
        reg.textures.insert(recipe.id().raw(), handle.id());
    }

    for (_, recipe) in meshes::catalog(style) {
        let mb = mesh_api.bake(&recipe, style.level_seed).expect("mesh recipe bakes");
        reg.mesh_vertices += mb.vertex_count();
        reg.mesh_indices += mb.indices().len();
        let data = MeshData::new(mb.positions().to_vec(), mb.normals().to_vec(), mb.uvs().to_vec(), mb.indices().to_vec());
        let handle = app.add_mesh_data(data).expect("mesh registers");
        reg.meshes.insert(recipe.id().raw(), handle);
    }

    for spec in materials::catalog(style) {
        let tex_id = reg.textures[&spec.texture_recipe_id];
        let mut m = Material::lit(engine_color(spec.base))
            .with_custom_texture(tex_id)
            .with_roughness(Ratio::new(spec.roughness).expect("roughness in range"));
        if let Some(e) = spec.emissive {
            m = m.with_emissive(engine_color(e));
        }
        reg.materials.insert(spec.name, app.add_material(m));
    }

    reg
}

/// Spawn one prefab instance at a transform, returning its entity.
fn spawn_prefab(app: &mut RunningApp, reg: &Registry, prefab: &str, position: Vec3, yaw: f32) -> Entity {
    let p = prefabs::by_name(prefab).expect("placement names a real prefab");
    let mesh = reg.meshes[&p.mesh_recipe_id];
    let material = reg.materials[p.material];
    let rot = Quat::from_euler_xyz(0.0, yaw, 0.0);
    app.spawn(Spawn::new(Transform::new(position, rot, Vec3::new(1.0, 1.0, 1.0)), mesh, material))
}

/// A first-person controller camera looking forward from `at`. Returns the camera
/// node so a viewmodel can be parented to it.
fn add_player_camera(app: &mut RunningApp, at: Vec3) -> Entity {
    let projection = PerspectiveProjection {
        fov_y: Angle::degrees(75.0),
        near: Meters::new(0.1).expect("near finite"),
        far: Meters::new(300.0).expect("far finite"),
    };
    app.spawn_controller(Camera::perspective(projection), Transform::from_translation(at), 0)
}

/// Add a warm focal point light at every ceiling fixture, plus colored accents at
/// the exit and weapon — the focal lighting that gives the space mood + contrast.
fn add_focal_lights(app: &mut RunningApp, layout: &LevelLayout, style: &Style) {
    for pl in &layout.placements {
        let cfg = match pl.prefab {
            "light" => Some((style.palette.light_glow, 1.0)),
            "exit" => Some((style.palette.exit, 0.8)),
            "weapon_body" => Some((style.palette.weapon_glow, 0.6)),
            _ => None,
        };
        if let Some((color, intensity)) = cfg {
            app.add_point_light(
                PointLight { color: engine_color(color), intensity: Ratio::new(intensity).expect("intensity in range") },
                Transform::from_translation(pl.position),
            );
        }
    }
}

/// A key directional light.
fn add_key_light(app: &mut RunningApp) {
    app.add_light(
        DirectionalLight {
            direction: Vec3::new(-0.3, -1.0, -0.25),
            color: EngineColor::WHITE,
            // A dimmer key so the warm focal point lights + emissive trim create
            // real light/shadow contrast instead of a flat wash.
            intensity: Ratio::new(0.6).expect("intensity in range"),
        },
        Transform::IDENTITY,
    );
}

/// How to place the level's camera when expanding.
#[derive(Debug, Clone, Copy)]
pub enum ShotCamera {
    /// A first-person controller at the player spawn (the playable camera).
    FirstPerson,
    /// A fixed overview camera from `eye` looking at `target` (for screenshots).
    Overview {
        /// Camera position.
        eye: Vec3,
        /// Look-at point.
        target: Vec3,
    },
}

/// Expand the full three-area level with the playable first-person camera.
pub fn expand_level(style: &Style) -> ExpandedLevel {
    expand_level_with(style, ShotCamera::FirstPerson)
}

/// Expand the level with a fixed overview camera — for headless screenshots.
pub fn expand_level_view(style: &Style, eye: Vec3, target: Vec3) -> ExpandedLevel {
    expand_level_with(style, ShotCamera::Overview { eye, target })
}

/// Expand the level with the chosen camera.
pub fn expand_level_with(style: &Style, camera: ShotCamera) -> ExpandedLevel {
    let mut app = App::new().add_plugins(DefaultPlugins).setup(|_, _, _| {}).build();
    let reg = register_all(&mut app, style);
    let layout = build_level(style);

    for pl in &layout.placements {
        spawn_prefab(&mut app, &reg, pl.prefab, pl.position, pl.yaw);
    }
    let renderable_count = layout.placements.len();

    match camera {
        ShotCamera::FirstPerson => {
            add_player_camera(&mut app, layout.player_spawn);
        }
        ShotCamera::Overview { eye, target } => {
            let projection = PerspectiveProjection {
                fov_y: Angle::degrees(70.0),
                near: Meters::new(0.1).expect("near finite"),
                far: Meters::new(400.0).expect("far finite"),
            };
            let t = Transform::from_translation(eye).looking_at(target, Vec3::UNIT_Y).expect("overview camera valid");
            app.set_camera(Camera::perspective(projection), t);
        }
    }
    add_key_light(&mut app);
    add_focal_lights(&mut app, &layout, style);

    ExpandedLevel {
        texture_bytes: reg.texture_bytes,
        mesh_vertices: reg.mesh_vertices,
        mesh_indices: reg.mesh_indices,
        renderable_count,
        entity_count: renderable_count + 2, // + player controller + light
        layout,
        app,
    }
}

/// Expand the title/menu tableau: the weapon glowing on a crate pedestal under
/// two lights, framed by a back wall. There is no text operator, so the "menu"
/// is a readable title tableau, not a labelled UI (see the project notes).
pub fn expand_menu(style: &Style) -> RunningApp {
    let mut app = App::new().add_plugins(DefaultPlugins).setup(|_, _, _| {}).build();
    let reg = register_all(&mut app, style);

    spawn_prefab(&mut app, &reg, "floor", Vec3::new(0.0, 0.0, 0.0), 0.0);
    spawn_prefab(&mut app, &reg, "wall", Vec3::new(0.0, style.room_height * 0.5, -3.0), 0.0);
    spawn_prefab(&mut app, &reg, "light", Vec3::new(-1.5, style.room_height - 0.5, -2.0), 0.0);
    spawn_prefab(&mut app, &reg, "light", Vec3::new(1.5, style.room_height - 0.5, -2.0), 0.0);
    spawn_prefab(&mut app, &reg, "crate", Vec3::new(0.0, 0.6, -1.0), 0.4);
    spawn_prefab(&mut app, &reg, "weapon", Vec3::new(0.0, 1.5, -1.0), 0.8);

    let projection = PerspectiveProjection {
        fov_y: Angle::degrees(60.0),
        near: Meters::new(0.1).expect("near finite"),
        far: Meters::new(50.0).expect("far finite"),
    };
    let eye = Transform::from_translation(Vec3::new(0.0, 1.8, 3.0))
        .looking_at(Vec3::new(0.0, 1.4, -1.0), Vec3::UNIT_Y)
        .expect("menu camera orientation valid");
    app.set_camera(Camera::perspective(projection), eye);
    add_key_light(&mut app);
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_expands_to_a_populated_scene() {
        let x = expand_level(&Style::facility());
        assert!(x.renderable_count > 60, "got {}", x.renderable_count);
        assert!(x.mesh_vertices > 0);
        assert!(x.texture_bytes > 0);
        assert_eq!(x.entity_count, x.renderable_count + 2);
    }

    #[test]
    fn menu_expands_without_panicking() {
        let mut app = expand_menu(&Style::facility());
        // Ticking the menu proves it is a live scene.
        let _ = app.tick(0);
    }

    #[test]
    fn expansion_is_deterministic() {
        let a = expand_level(&Style::facility());
        let b = expand_level(&Style::facility());
        assert_eq!(a.mesh_vertices, b.mesh_vertices);
        assert_eq!(a.mesh_indices, b.mesh_indices);
        assert_eq!(a.texture_bytes, b.texture_bytes);
        assert_eq!(a.renderable_count, b.renderable_count);
    }
}
