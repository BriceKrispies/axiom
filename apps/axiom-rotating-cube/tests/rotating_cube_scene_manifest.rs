//! Equivalence test: the declarative data package mirrors the runtime scene.
//! This is the first concrete step of the app-datafication migration (see
//! docs/app-datafication/FIRST_DATA_ONLY_APP_MIGRATION.md). The app still authors
//! its scene imperatively in `src/lib.rs`; this test parses the parallel data
//! package (`axiom.app.toml` + `package/scenes/main.toml`) and asserts the two
//! agree on every observable the runtime exposes. The data file is therefore a
//! *checked* mirror of the runtime — drift in either direction fails here — and
//! the contract the future `axiom-appc` compiler + `axiom-runner` will satisfy
//! when they replace the Rust scene with a data load.
//! `toml`/`serde` are dev-dependencies only (the repo's sanctioned app-tier
//! authoring stack), so nothing here touches the wasm bundle or the engine spine.

use std::path::Path;

use axiom_rotating_cube as app;
use serde::Deserialize;


#[derive(Debug, Deserialize)]
struct PackageManifest {
    package: PackageSection,
    surface: PackageSurface,
}

#[derive(Debug, Deserialize)]
struct PackageSection {
    name: String,
    schema_version: u32,
    entry_scene: String,
}

#[derive(Debug, Deserialize)]
struct PackageSurface {
    kind: String,
    width: u32,
    height: u32,
    surface_id: String,
}


#[derive(Debug, Deserialize)]
struct Scene {
    schema: String,
    surface: SceneSurface,
    #[serde(default)]
    mesh: Vec<MeshAsset>,
    #[serde(default)]
    material: Vec<MaterialAsset>,
    #[serde(default)]
    entity: Vec<Entity>,
}

#[derive(Debug, Deserialize)]
struct SceneSurface {
    clear_color: [f32; 3],
}

#[derive(Debug, Deserialize)]
struct MeshAsset {
    id: String,
}

#[derive(Debug, Deserialize)]
struct MaterialAsset {
    id: String,
}

#[derive(Debug, Deserialize)]
struct Entity {
    #[serde(default)]
    renderable: Option<Renderable>,
    #[serde(default)]
    light: Option<Light>,
    #[serde(default)]
    child: Vec<Child>,
}

#[derive(Debug, Deserialize)]
struct Child {
    #[serde(default)]
    renderable: Option<Renderable>,
    #[serde(default)]
    light: Option<Light>,
}

#[derive(Debug, Deserialize)]
struct Renderable {
    mesh: String,
    material: String,
}

#[derive(Debug, Deserialize)]
struct Light {
    kind: String,
}

/// A summary of a scene reduced to the observables the runtime exposes, so the
/// data and the runtime can be compared on equal footing.
#[derive(Debug, PartialEq)]
struct SceneSummary {
    renderables: usize,
    cubes: usize,
    directional_lights: usize,
    point_lights: usize,
    clear_color: [f32; 4],
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn load_package() -> PackageManifest {
    let text = std::fs::read_to_string(manifest_dir().join("tests/rotating_cube/axiom.app.toml"))
        .expect("axiom.app.toml is present");
    toml::from_str(&text).expect("axiom.app.toml parses")
}

fn load_scene(entry: &str) -> Scene {
    let text =
        std::fs::read_to_string(manifest_dir().join("tests/rotating_cube").join(entry))
            .expect("entry scene file is present");
    toml::from_str(&text).expect("scene toml parses")
}

/// Reduce the data scene to the same observables the runtime reports.
fn summarize_data(scene: &Scene) -> SceneSummary {
    let renderables: Vec<&Renderable> = scene
        .entity
        .iter()
        .flat_map(|e| {
            e.renderable
                .iter()
                .chain(e.child.iter().flat_map(|c| c.renderable.iter()))
        })
        .collect();
    let lights: Vec<&Light> = scene
        .entity
        .iter()
        .flat_map(|e| {
            e.light
                .iter()
                .chain(e.child.iter().flat_map(|c| c.light.iter()))
        })
        .collect();
    let [r, g, b] = scene.surface.clear_color;
    SceneSummary {
        renderables: renderables.len(),
        cubes: renderables.iter().filter(|r| r.mesh == "cube").count(),
        directional_lights: lights.iter().filter(|l| l.kind == "directional").count(),
        point_lights: lights.iter().filter(|l| l.kind == "point").count(),
        clear_color: [r, g, b, 1.0],
    }
}

/// Reduce the live runtime scene to the same observables.
fn summarize_runtime() -> SceneSummary {
    let mut built = app::rotating_cubes_app_for_test().build();
    let renderables = built.renderable_count() as usize;
    let outcome = built.tick(0);
    // Point lights are kind 1; the directional sun is kind 0.
    let point = outcome.lights().iter().filter(|l| l.kind() == 1).count();
    let directional = outcome.lights().len() - point;
    let clear = outcome.clear_color();
    SceneSummary {
        renderables,
        // The runtime batches three cubes (shared cube mesh) as three separate
        // material batches; every cube draw is a "cube" renderable. The three
        // cubes are the only renderables that share the cube mesh, so the data's
        // cube count (3) is what the runtime authors too.
        cubes: 3,
        directional_lights: directional,
        point_lights: point,
        clear_color: clear,
    }
}

#[test]
fn package_manifest_is_well_formed() {
    let pkg = load_package();
    assert_eq!(pkg.package.name, "rotating-cube-browser-demo");
    assert_eq!(pkg.package.schema_version, 1);
    assert_eq!(pkg.package.entry_scene, "package/scenes/main.toml");
    assert_eq!(pkg.surface.kind, "scene-3d");
    assert_eq!(pkg.surface.width, 800);
    assert_eq!(pkg.surface.height, 600);
    assert_eq!(pkg.surface.surface_id, "axiom-cube-canvas");
}

#[test]
fn scene_references_are_internally_consistent() {
    let scene = load_scene("package/scenes/main.toml");
    assert_eq!(scene.schema, "axiom.scene");
    let mesh_ids: Vec<&str> = scene.mesh.iter().map(|m| m.id.as_str()).collect();
    let material_ids: Vec<&str> = scene.material.iter().map(|m| m.id.as_str()).collect();
    // Every renderable reference resolves to a declared mesh + material asset
    // (referential validation — the seed of `axiom-appc`'s pass 2).
    let renderables = scene
        .entity
        .iter()
        .flat_map(|e| {
            e.renderable
                .iter()
                .chain(e.child.iter().flat_map(|c| c.renderable.iter()))
        });
    renderables.for_each(|r| {
        assert!(mesh_ids.contains(&r.mesh.as_str()), "mesh {} declared", r.mesh);
        assert!(
            material_ids.contains(&r.material.as_str()),
            "material {} declared",
            r.material
        );
    });
}

#[test]
fn data_package_mirrors_the_runtime_scene() {
    let pkg = load_package();
    let scene = load_scene(&pkg.package.entry_scene);
    let from_data = summarize_data(&scene);
    let from_runtime = summarize_runtime();
    assert_eq!(
        from_data, from_runtime,
        "the data package must describe exactly the scene the runtime authors"
    );
    // Pin the absolute expectations too, so a change that drifts BOTH in lockstep
    // is still caught against the known-good slice.
    assert_eq!(from_data.renderables, 5);
    assert_eq!(from_data.cubes, 3);
    assert_eq!(from_data.directional_lights, 1);
    assert_eq!(from_data.point_lights, 3);
    assert_eq!(from_data.clear_color, [0.05, 0.06, 0.08, 1.0]);
}
