//! `axiom-shot` — render an engine scene to a PNG, headless, via native wgpu.
//!
//! This ticks a scene, pulls `RunningApp`'s neutral live-render data (the same
//! mesh set / material set / per-`(mesh, material)` instance batches and lights
//! that drive the browser), and hands it to `axiom-gpu-backend`'s native
//! off-screen renderer — which runs the **same** `scene_renderer` the browser's
//! live arm runs. So a screenshot here is byte-faithful to what the browser
//! presents, and there is no second renderer copy to drift from.
//!
//! Usage: `cargo run --manifest-path tools/axiom-shot/Cargo.toml --release -- [out.png] [tick]`
//! (defaults: `screenshots/axiom-shot.png`, tick `0`).

use axiom::prelude::*;
use axiom_gpu_backend::GpuBackendApi;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;

fn ch(v: f32) -> Ratio {
    Ratio::new(v).expect("authored colour channel is finite")
}

/// Author the Stage-2/3 textured + lit showcase: three spinning checker cubes, a
/// UV-grid ground plane, a checker sphere, a camera, a directional sun, and three
/// orbiting coloured point lights — the same scene the browser demo renders.
fn showcase_app() -> App {
    App::new()
        .window(
            Window::new(WIDTH, HEIGHT).with_clear_color(Color::linear_rgb(
                ch(0.05),
                ch(0.06),
                ch(0.08),
            )),
        )
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let cubes = [
                (
                    -2.6,
                    Vec3::UNIT_Y,
                    Color::linear_rgb(ch(0.85), ch(0.25), ch(0.25)),
                ),
                (
                    0.0,
                    Vec3::UNIT_X,
                    Color::linear_rgb(ch(0.30), ch(0.80), ch(0.35)),
                ),
                (
                    2.6,
                    Vec3::new(1.0, 1.0, 0.0),
                    Color::linear_rgb(ch(0.30), ch(0.50), ch(0.95)),
                ),
            ];
            cubes.into_iter().for_each(|(offset_x, axis, color)| {
                let material = materials.add(Material::lit(color).with_texture(Texture::Checker));
                world
                    .spawn(Transform::from_translation(Vec3::new(offset_x, 0.0, 0.0)))
                    .with_child((
                        Renderable {
                            mesh: cube,
                            material,
                        },
                        Spin::around(axis).period(360),
                    ));
            });
            let plane = meshes.add(Mesh::plane());
            let ground = materials.add(
                Material::lit(Color::linear_rgb(ch(0.18), ch(0.20), ch(0.24)))
                    .with_texture(Texture::UvGrid),
            );
            world.spawn((
                Transform::combine(
                    Transform::from_translation(Vec3::new(0.0, -2.0, 0.0)),
                    Transform::from_scale(Vec3::new(30.0, 1.0, 30.0)),
                ),
                Renderable {
                    mesh: plane,
                    material: ground,
                },
            ));
            let sphere = meshes.add(Mesh::sphere());
            let sphere_material = materials.add(
                Material::lit(Color::linear_rgb(ch(0.90), ch(0.78), ch(0.30)))
                    .with_texture(Texture::Checker),
            );
            world.spawn((
                Transform::combine(
                    Transform::from_translation(Vec3::new(0.0, 2.6, 0.0)),
                    Transform::from_scale(Vec3::new(1.6, 1.6, 1.6)),
                ),
                Renderable {
                    mesh: sphere,
                    material: sphere_material,
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(60.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
                }),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.3, -1.0, 0.4),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
            // Three orbiting coloured point lights (matches the browser demo).
            let orbit_lights = [
                (Color::linear_rgb(ch(0.95), ch(0.25), ch(0.25)), 200),
                (Color::linear_rgb(ch(0.25), ch(0.95), ch(0.35)), 320),
                (Color::linear_rgb(ch(0.30), ch(0.45), ch(0.98)), 260),
            ];
            orbit_lights.into_iter().for_each(|(color, period)| {
                world
                    .spawn((Transform::IDENTITY, Spin::around(Vec3::UNIT_Y).period(period)))
                    .with_child((
                        Transform::from_translation(Vec3::new(4.5, 1.2, 0.0)),
                        PointLight {
                            color,
                            intensity: ch(9.0),
                        },
                    ));
            });
        })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out_path = args
        .next()
        .unwrap_or_else(|| "screenshots/axiom-shot.png".to_string());
    let tick: u64 = args.next().and_then(|t| t.parse().ok()).unwrap_or(0);

    // Drive the engine to the requested tick and pull its neutral render data —
    // exactly what `WindowingApi::run_web_multi` feeds the browser each frame.
    let mut running = showcase_app().build();
    let meshes = running.mesh_set();
    let materials = running.material_textures();
    let outcome = running.tick(tick);
    let clear = outcome.clear_color();
    let batches = outcome.mesh_batches();
    let light_view_proj = outcome.light_view_proj();
    let lights: Vec<(u32, [f32; 3], [f32; 3], f32)> = outcome
        .lights()
        .iter()
        .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
        .collect();

    // Render it through the engine's own off-screen GPU path (the same renderer
    // the browser uses) and read the pixels back.
    let pixels = GpuBackendApi::render_offscreen_rgba(
        WIDTH,
        HEIGHT,
        &meshes,
        &materials,
        &lights,
        light_view_proj,
        &batches,
        clear,
    )
    .expect("a native GPU adapter is required to render a screenshot");

    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(parent).expect("create output directory");
    }
    write_png(&out_path, &pixels);
    println!("axiom-shot: wrote {out_path} ({WIDTH}x{HEIGHT}, tick {tick})");
}

fn write_png(path: &str, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("create PNG file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write PNG header");
    writer.write_image_data(rgba).expect("write PNG data");
}
