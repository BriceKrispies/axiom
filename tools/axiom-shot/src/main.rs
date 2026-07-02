//! `axiom-shot` — render any Axiom app to a PNG, headless, via a chosen backend.
//!
//! It ticks a selected app's scene, pulls `RunningApp`'s neutral live-render data
//! (the same mesh set / material set / per-`(mesh, material)` instance batches and
//! lights that drive the browser), and renders it through a selected backend:
//!
//!   * `--backend gpu` (default) — `axiom-gpu-backend`'s native off-screen arm,
//!     the SAME `scene_renderer` the browser's WebGPU/WebGL2 path runs.
//!   * `--backend canvas2d` — `axiom-canvas2d-backend`'s software z-buffer
//!     rasterizer, the SAME renderer the browser's `?backend=canvas2d` path runs,
//!     fed the SAME backend-neutral `FramePacket` windowing reconstructs from the
//!     instance batches.
//!
//! Either way the screenshot is byte-faithful to what the browser presents on
//! that backend, so this reproduces backend-specific rendering artifacts (e.g.
//! the Canvas 2D contact-shadow blobs) headlessly, with no browser.
//!
//! It can also drive the first-person camera itself, two ways:
//!
//!   * `--script` is a sequence of `ticks:held-inputs` phases applied as
//!     `FirstPersonInput` to controller 0 (the engine's first-person camera, e.g.
//!     the retro FPS player), so an app can be walked to an arbitrary vantage point.
//!   * `--pose "x,z,yaw,pitch"` (retro FPS only) teleports controller 0 to an absolute
//!     pose at tick 0 — the exact pose a player reads off the debug overlay — so a
//!     view-dependent artifact reproduces faithfully in one shot, no blind walking.
//!
//! Usage:
//!   cargo run --manifest-path tools/axiom-shot/Cargo.toml --release -- \
//!     [--app showcase|retro_fps] [--backend gpu|canvas2d] [--tick N] [--out PATH] \
//!     [--quality 0..3] [--script "ticks:key=val,...;..."] [--pose "x,z,yaw,pitch"]
//!
//! Script keys (per-tick held values): `forward`, `back`, `strafe_left`,
//! `strafe_right` (move deltas), `yaw`, `pitch` (look deltas, radians/tick).
//! Example (walk the retro FPS player up the room, then turn right to face the
//! dividing wall):  `--script "83:forward=0.06;35:yaw=-0.045"`.
//! Example (reproduce an exact overlay pose):  `--pose "6.4,5.0,-1.57,0.0"`.

use axiom::prelude::*;
use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};

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
            // A raymarched SDF sphere (no mesh): the engine marches it and depth-
            // composites it with the rasterized meshes — on the GPU backend and,
            // preserving the software-fallback property, on the Canvas 2D backend.
            world.spawn((
                Transform::from_translation(Vec3::new(-4.6, 1.6, 0.0)),
                SdfShape::sphere(
                    Meters::new(1.3).expect("finite radius"),
                    Color::linear_rgb(ch(0.20), ch(0.85), ch(0.90)),
                ),
            ));
            // A raymarched SDF box on the other side, to show box SDF + occlusion.
            world.spawn((
                Transform::from_translation(Vec3::new(4.7, 1.4, 0.0)),
                SdfShape::cuboid(
                    Vec3::new(0.9, 0.9, 0.9),
                    Color::linear_rgb(ch(0.95), ch(0.45), ch(0.85)),
                ),
            ));
            // A procedurally-animated cube (ProcAnim): the engine's scene system
            // bobs it on +Y and spins it from the tick, so it sits at a different
            // pose every frame — the proc-driven rendering capability, on screen.
            let proc_material =
                materials.add(Material::lit(Color::linear_rgb(ch(0.95), ch(0.55), ch(0.12))));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, -0.6, 3.4)),
                Renderable {
                    mesh: cube,
                    material: proc_material,
                },
                ProcAnim::bob(Meters::new(1.6).expect("finite bob"), 120)
                    .spin(Vec3::UNIT_Y, 180),
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

/// The SPEC-11 §7 "nova-roll" render-one-frame slice: a cube + an (emissive)
/// cylinder, a perspective camera, and one directional light — the smallest scene
/// that exercises `Mesh::cylinder` + an emissive material on both backends. The
/// matching `tests/render_parity.rs` authors the same shape and asserts GPU↔canvas2d
/// agreement; this `--app nova-roll` entry renders it live through either backend.
fn nova_roll_app() -> App {
    App::new()
        .window(
            Window::new(WIDTH, HEIGHT).with_clear_color(Color::linear_rgb(
                ch(0.02),
                ch(0.02),
                ch(0.05),
            )),
        )
        .add_plugins(DefaultPlugins)
        .setup(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            let cylinder = meshes.add(Mesh::cylinder());
            let cube_mat =
                materials.add(Material::lit(Color::linear_rgb(ch(0.85), ch(0.30), ch(0.25))));
            // The cylinder carries an emissive colour (carried-but-not-shaded).
            let cyl_mat = materials.add(
                Material::lit(Color::linear_rgb(ch(0.25), ch(0.55), ch(0.90)))
                    .with_emissive(Color::linear_rgb(ch(0.6), ch(0.5), ch(0.1))),
            );
            world.spawn((
                Transform::from_translation(Vec3::new(-1.6, 0.0, 0.0)),
                Renderable {
                    mesh: cube,
                    material: cube_mat,
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(1.6, 0.0, 0.0)),
                Renderable {
                    mesh: cylinder,
                    material: cyl_mat,
                },
            ));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 6.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(55.0),
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
        })
}

/// Build the selected app's `RunningApp`. Apps are named on the command line; new
/// renderable apps are added here (and as a Cargo dependency). `level` (when set)
/// is a path to a `level.axiom` document for the retro FPS app (else its built-in
/// default level is used).
fn build_app(name: &str, level: Option<&str>) -> RunningApp {
    match name {
        "retro_fps" => axiom_gallery::retro_fps::build_retro_fps_app(&retro_fps_doc(level)).0,
        "showcase" => showcase_app().build(),
        "nova-roll" => nova_roll_app().build(),
        "physics-crucible" => axiom_gallery::physics_crucible::build_physics_crucible(),
        other => {
            eprintln!("axiom-shot: unknown --app '{other}', falling back to 'showcase'");
            showcase_app().build()
        }
    }
}

/// The retro FPS level document for `--level PATH` (else the built-in default). Shared
/// by `build_app` and the `--pose` teleport path so both read the same level.
fn retro_fps_doc(level: Option<&str>) -> axiom_gallery::retro_fps::level::LevelDoc {
    match level {
        Some(path) => axiom_gallery::retro_fps::level::LevelDoc::parse(
            &std::fs::read_to_string(path).expect("read --level file"),
        ),
        None => axiom_gallery::retro_fps::level::LevelDoc::default(),
    }
}

/// Parse a `--pose "x,z,yaw,pitch"` argument (world position + look angles in
/// radians) into its four floats, or `None` if it is not exactly four numbers.
fn parse_pose(s: &str) -> Option<(f32, f32, f32, f32)> {
    let v: Vec<f32> = s
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    match v.as_slice() {
        [x, z, yaw, pitch] => Some((*x, *z, *yaw, *pitch)),
        _ => None,
    }
}

/// One phase's held first-person inputs (per-tick deltas).
#[derive(Clone, Copy, Default)]
struct Hold {
    forward: f32,
    strafe: f32,
    yaw: f32,
    pitch: f32,
}

/// Expand a `--script` into one `FirstPersonInput` per tick (controller 0). Each
/// phase is `ticks:key=val,...`; phases are separated by `;`. Move keys map into
/// the camera-local frame (`forward` = local -Z, `strafe_right` = local +X); look
/// keys are per-tick yaw/pitch deltas in radians.
fn parse_script(s: &str) -> Vec<FirstPersonInput> {
    let mut out = Vec::new();
    for phase in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (n_str, rest) = phase.split_once(':').unwrap_or((phase, ""));
        let n: usize = n_str.trim().parse().unwrap_or(0);
        let mut hold = Hold::default();
        for kv in rest.split(',').map(str::trim).filter(|k| !k.is_empty()) {
            let (k, v) = kv.split_once('=').unwrap_or((kv, "0"));
            let val: f32 = v.trim().parse().unwrap_or(0.0);
            match k.trim() {
                "forward" => hold.forward += val,
                "back" | "backward" => hold.forward -= val,
                "strafe_right" => hold.strafe += val,
                "strafe_left" => hold.strafe -= val,
                "yaw" => hold.yaw = val,
                "pitch" => hold.pitch = val,
                other => eprintln!("axiom-shot: ignoring unknown script key '{other}'"),
            }
        }
        let control = FirstPersonInput::new(
            0,
            Vec3::new(hold.strafe, 0.0, -hold.forward),
            Angle::radians(hold.yaw),
            Angle::radians(hold.pitch),
        );
        out.extend(std::iter::repeat(control).take(n));
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = flag(&args, "--app").unwrap_or_else(|| "showcase".to_string());
    let backend = flag(&args, "--backend").unwrap_or_else(|| "gpu".to_string());
    let out = flag(&args, "--out").unwrap_or_else(|| "screenshots/axiom-shot.png".to_string());
    let quality: u8 = flag(&args, "--quality")
        .and_then(|q| q.parse().ok())
        .unwrap_or(1);
    let controls = parse_script(&flag(&args, "--script").unwrap_or_default());
    // Render tick: explicit `--tick`, else the last scripted tick, else 0.
    let render_tick = flag(&args, "--tick")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or_else(|| controls.len().saturating_sub(1) as u64);

    // `--pose "x,z,yaw,pitch"` (retro FPS only): snap controller 0 to an absolute pose
    // at tick 0 via the game's one corrective teleport control — the exact pose a
    // player reads off the debug overlay — so a view-dependent artifact reproduces
    // faithfully in one shot, no blind walking. Overrides the script at tick 0.
    let teleport = match (app.as_str(), flag(&args, "--pose").as_deref().and_then(parse_pose)) {
        ("retro_fps", Some((x, z, yaw, pitch))) => {
            let mut game =
                axiom_gallery::retro_fps::RetroFpsGame::from_level(&retro_fps_doc(flag(&args, "--level").as_deref()));
            Some(game.teleport(x, z, yaw, pitch))
        }
        _ => None,
    };

    // Drive the engine to `render_tick`, applying the scripted control for each
    // tick (controller 0). The meshes are static, so they are pulled once.
    let mut running = build_app(&app, flag(&args, "--level").as_deref());
    let meshes = running.mesh_set();
    let materials = running.material_textures();
    let mut outcome = None;
    for t in 0..=render_tick {
        let frame = match (t, teleport) {
            (0, Some(c)) => running.tick_with_controls(0, &[], std::slice::from_ref(&c)),
            _ => match controls.get(t as usize).copied() {
                Some(c) => running.tick_with_controls(t, &[], std::slice::from_ref(&c)),
                None => running.tick(t),
            },
        };
        outcome = Some(frame);
    }
    let outcome = outcome.expect("at least one frame is ticked");

    // Render through the requested backend and read the pixels back.
    let (pixels, w, h) = match backend.as_str() {
        "canvas2d" | "canvas" => render_canvas2d(&meshes, &outcome, quality),
        _ => render_gpu(&meshes, &materials, &outcome),
    };

    write_png(&out, &pixels, w, h);
    println!(
        "axiom-shot: wrote {out} ({w}x{h}, app={app}, backend={backend}, tick={render_tick})"
    );
}

/// Render the frame through the engine's native off-screen GPU path (the same
/// `scene_renderer` the browser's WebGPU/WebGL2 arm runs).
fn render_gpu(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
) -> (Vec<u8>, u32, u32) {
    let batches = outcome.mesh_batches();
    let lights: Vec<(u32, [f32; 3], [f32; 3], f32)> = outcome
        .lights()
        .iter()
        .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
        .collect();
    let pixels = GpuBackendApi::render_offscreen_rgba(
        WIDTH,
        HEIGHT,
        meshes,
        materials,
        &lights,
        outcome.light_view_proj(),
        &batches,
        outcome.clear_color(),
        outcome.sdf_scene(),
        axiom_host::FrameAmbient::default_hemisphere(),
    )
    .expect("a native GPU adapter is required to render a GPU screenshot");
    (pixels, WIDTH, HEIGHT)
}

/// Render the frame through the software Canvas 2D backend (the same rasterizer
/// the browser's `?backend=canvas2d` path runs), fed the backend-neutral
/// `FramePacket` windowing reconstructs from the instance batches. Returns the
/// internal low-poly framebuffer (its size is the quality tier's resolution).
fn render_canvas2d(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    outcome: &FrameOutcome,
    quality: u8,
) -> (Vec<u8>, u32, u32) {
    let request = present_request(WIDTH, HEIGHT);
    let mut backend = Canvas2dBackendApi::new(&request);
    backend.load_meshes(meshes);
    backend.set_quality_level(quality);
    backend.render_offscreen_rgba(&frame_packet(outcome, WIDTH, HEIGHT))
}

/// Reconstruct the backend-neutral frame packet from the per-`(mesh, material)`
/// instance batches, exactly as `axiom-windowing` does for its Canvas arm: each
/// 36-float instance is `mvp(16) | world(16) | colour(4)`, object ids assigned in
/// draw order.
fn frame_packet(outcome: &FrameOutcome, w: u32, h: u32) -> FramePacket {
    let batches = outcome.mesh_batches();
    // The caster flags align with the `mesh_batches` instance expansion order.
    let casters = outcome.mesh_batch_casters();
    let mut draws = Vec::new();
    let mut object_id: u64 = 0;
    for (mesh_id, material_id, floats, count) in &batches {
        for i in 0..*count {
            let off = i as usize * 36;
            let mvp: [f32; 16] = floats[off..off + 16].try_into().unwrap_or([0.0; 16]);
            let world: [f32; 16] = floats[off + 16..off + 32].try_into().unwrap_or([0.0; 16]);
            let color: [f32; 4] = floats[off + 32..off + 36].try_into().unwrap_or([1.0; 4]);
            let casts = casters.get(object_id as usize).copied().unwrap_or(false);
            draws.push(FrameDrawItem::new(
                object_id, *mesh_id, *material_id, world, mvp, color, casts,
            ));
            object_id += 1;
        }
    }
    let lights: Vec<FrameLight> = outcome
        .lights()
        .iter()
        .map(|l| {
            let c = l.color();
            FrameLight::new(l.kind(), l.vec(), [c[0], c[1], c[2], l.intensity()])
        })
        .collect();
    let directional = outcome.lights().iter().filter(|l| l.kind() == 0).count() as u32;
    let point = outcome.lights().iter().filter(|l| l.kind() == 1).count() as u32;
    let features = FrameFeatureSet::new(false, directional > 0, directional, point);
    // The Canvas planar-shadow pass projects caster geometry through the camera;
    // view/projection are unused by the software path, so identity is fine.
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0_f32,
    ];
    let camera = Some(FrameCamera::new(
        identity,
        identity,
        outcome.camera_view_proj(),
    ));
    let packet = FramePacket::new(
        outcome.tick(),
        outcome.tick(),
        FrameViewport::new(w, h),
        outcome.clear_color(),
        camera,
        draws,
        lights,
        outcome.light_view_proj(),
        features,
    );
    // Attach the frame's SDF scene (if any) so the Canvas2D backend marches and
    // composites the raymarched shapes against the rasterized meshes.
    match outcome.sdf_scene() {
        Some(scene) => packet.with_sdf(scene.clone()),
        None => packet,
    }
}

/// Build the validated host presentation request the Canvas 2D backend is sized
/// from, the way windowing does (the backend reads only the viewport size).
fn present_request(w: u32, h: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(w, h, Ratio::new(1.0).expect("finite scale"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-shot")
        .expect("valid target");
    let surface = host.surface_handle(&kernel, 2).expect("valid surface");
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
    let device = host.device_request(true, HostDeviceProfile::Baseline);
    host.presentation_request(target, surface, descriptor, adapter, device)
        .expect("valid presentation request")
}

/// The value following `name` in `args`, if present.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn write_png(path: &str, rgba: &[u8], width: u32, height: u32) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).expect("create output directory");
    }
    let file = std::fs::File::create(path).expect("create PNG file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write PNG header");
    writer.write_image_data(rgba).expect("write PNG data");
}
