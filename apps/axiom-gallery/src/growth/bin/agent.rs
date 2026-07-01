//! `axiom-growth` agent driver — the reusable `axiom-agent` harness walking the
//! game's player up the Everest-scale mountain. Native + `agent` feature only
//! (`required-features`), so it never enters the wasm build or the default gates.
//!
//! Three modes:
//!
//! * **climb** (default) — headless: hold "forward" until the player reaches the
//!   summit, printing the player's **height** as it rises. The literal
//!   "hold forward until it reaches the top of the mountain, reporting height".
//! * **serve `[addr]`** — an HTTP control loop over the same headless sim:
//!     curl -s -XPOST localhost:7878/step -d '{"keys":["forward"]}'
//!     curl -s -XPOST localhost:7878/reset
//!     curl -s localhost:7878/state
//! * **--bridge `[ws_addr]`** — drive the *live in-browser* viewer: the browser
//!   (opened with `?agent=ws://<addr>`) pushes an observation each frame, the
//!   agent decides "forward" through the harness, and the held controls are pushed
//!   back — the agent climbing the real 3D view.

use std::net::{TcpListener, TcpStream};

use axiom_gallery::growth::agent::{
    control_to_keys, parse_directives, Action, AgentSession, CaptureRequest, Observation,
};
use axiom_gallery::growth::ground::CaptureInputs;
use axiom_agent_harness::AgentHarnessApi;
use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Meters, Ratio};
use tiny_http::{Header, Method, Response, Server};
use tungstenite::Message;

/// The 4×4 identity matrix (column-major), used for the terrain instance's world
/// transform (the terrain sits at the world origin, so its MVP is the camera
/// view-projection) and for the unused view/proj slots of the Canvas camera.
const IDENTITY_4X4: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:7878";
const DEFAULT_WS_ADDR: &str = "127.0.0.1:7879";
/// The agent's stable id ("growth" in ASCII), matching the session driver.
const AGENT_RAW_ID: u64 = 0x67_72_6f_77_74_68;
/// Hard cap on climb ticks so a degenerate plan can never spin forever.
const CLIMB_TICK_CAP: u64 = 20_000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("climb");
    match mode {
        "serve" => serve(args.get(2).map(String::as_str).unwrap_or(DEFAULT_HTTP_ADDR)),
        "--bridge" | "bridge" => {
            run_bridge(args.get(2).map(String::as_str).unwrap_or(DEFAULT_WS_ADDR));
        }
        "shots" | "screenshots" => shots(args.get(2).map(String::as_str).unwrap_or("gpu")),
        "summit" | "lookdown" => summit(args.get(2).map(String::as_str).unwrap_or("gpu")),
        "run" => run(
            args.get(2).map(String::as_str).expect("usage: agent run <script.toml> [gpu|canvas2d]"),
            args.get(3).map(String::as_str).unwrap_or("gpu"),
        ),
        "perceive" => perceive(
            args.get(2)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(2000),
        ),
        _ => climb(),
    }
}

/// Headless perception demo: climb toward the summit and, each tick, print what
/// the agent **perceives** of the heightfield through `axiom-perception` — the
/// slope ahead (with a real distance) and the mountaintop landmark in view. The
/// same sensor model the DOOM agent uses, here against a world of terrain noise
/// with no entities at all.
fn perceive(max_ticks: u64) {
    let mut session = AgentSession::earthlike();
    println!(
        "[growth-agent] perception demo — the agent senses the heightfield (peak={:.0} m)",
        session.observe().peak_height_m
    );
    let forward = Action::seek();
    let mut last = String::new();
    let mut obs = session.observe();
    while !obs.reached_summit && obs.tick < max_ticks {
        obs = session.step(&forward);
        // Print only when what's perceived changes, so the stream reads as the
        // agent *noticing* the world rather than a wall of identical lines.
        let lines = session.sight().report_lines().join("\n");
        if lines != last {
            println!("tick {}:", obs.tick);
            println!("{lines}");
            last = lines;
        }
    }
    println!("[growth-agent] done at tick {} (reached_summit={}).", obs.tick, obs.reached_summit);
}

/// The conventional location of the app's authored world tags (the static-world
/// source). Optional — the runtime tags from the generated vista always exist.
const TAGS_PATH: &str = "apps/axiom-gallery/src/growth/package/world/tags.toml";

/// Data-driven mode: load a directive **script** (TOML) and execute it against
/// the world's tags — "walk to the mountaintop, look at the ground, take a
/// screenshot" expressed as data. Every `capture` directive becomes a PNG under
/// `screenshots/<label>.png`. The same runner drives any script over any tags.
fn run(script_path: &str, backend: &str) {
    let mut session = AgentSession::earthlike();
    // Merge authored tags if the package file is present (both tag sources).
    match std::fs::read_to_string(TAGS_PATH) {
        Ok(toml) => {
            session.register_toml_tags(&toml);
            println!("[growth-agent] loaded authored tags from {TAGS_PATH}");
        }
        Err(_) => println!("[growth-agent] no authored tags at {TAGS_PATH}; using runtime tags only"),
    }

    let script_str = std::fs::read_to_string(script_path)
        .unwrap_or_else(|e| panic!("read directive script {script_path}: {e}"));
    let script = parse_directives(&script_str);
    println!(
        "[growth-agent] running {} directive(s) from {script_path}",
        script.directive.len(),
    );

    let captures = session.run_directives(&script);
    println!(
        "[growth-agent] script done (tick {}, {:.0} m above spawn); {} capture(s)",
        session.observe().tick,
        session.observe().height_above_spawn_m,
        captures.len(),
    );
    render_captures(&captures, backend);
}

/// Render and save every capture a directive run produced, through the chosen
/// backend (`gpu` or `canvas2d`).
fn render_captures(captures: &[CaptureRequest], backend: &str) {
    let is_canvas2d = matches!(backend, "canvas2d" | "canvas");
    let suffix = if is_canvas2d { "_canvas2d" } else { "" };
    for capture in captures {
        let (pixels, w, h) = if is_canvas2d {
            render_capture_canvas2d(&capture.inputs)
        } else {
            (render_capture(&capture.inputs), capture.inputs.width, capture.inputs.height)
        };
        let path = format!("screenshots/{}{suffix}.png", capture.label);
        write_png(&path, &pixels, w, h);
        println!(
            "[growth-agent] wrote {path} ({w}x{h}) — directive capture '{}' ({})",
            capture.label,
            if is_canvas2d { "canvas2d" } else { "gpu" },
        );
    }
}

/// Walk to the top of the mountain, then look back down from the summit at the
/// ground far below and capture one screenshot → `screenshots/summit_lookdown.png`
/// (or `..._canvas2d.png`).
fn summit(backend: &str) {
    let mut session = AgentSession::earthlike();
    let start = session.observe();
    println!(
        "[growth-agent] planet seed=0x{:016x}  peak={:.0} m  prominence={:.0} m  {:.0} m to summit",
        session.seed(),
        start.peak_height_m,
        start.prominence_m,
        start.distance_to_peak_m,
    );
    println!("[growth-agent] walking to the top (holding FORWARD through the harness)…");

    let forward = Action::forward();
    let mut obs = start;
    while !obs.reached_summit && obs.tick < CLIMB_TICK_CAP {
        obs = session.step(&forward);
    }
    println!(
        "[growth-agent] on the summit (tick {}, {:.0} m above spawn); looking back down at the ground below",
        obs.tick, obs.height_above_spawn_m,
    );

    // Look from just above the summit back down at the spawn ground far below;
    // the aim is derived from the spawn point, not a hand-tuned pitch.
    let inputs = session.capture_summit_lookdown();
    let is_canvas2d = matches!(backend, "canvas2d" | "canvas");
    let suffix = if is_canvas2d { "_canvas2d" } else { "" };
    let (pixels, w, h) = if is_canvas2d {
        render_capture_canvas2d(&inputs)
    } else {
        (render_capture(&inputs), inputs.width, inputs.height)
    };
    let path = format!("screenshots/summit_lookdown{suffix}.png");
    write_png(&path, &pixels, w, h);
    println!(
        "[growth-agent] wrote {path} ({w}x{h}) — the view from the summit looking down ({})",
        if is_canvas2d { "canvas2d" } else { "gpu" },
    );
}

/// Climb to the summit, then turn to each cardinal direction and capture a
/// screenshot of the mountain's flank — saved to `screenshots/mountain_<dir>.png`
/// (or `..._canvas2d.png`). The agent drives the climb and the camera; the chosen
/// backend (`gpu` or `canvas2d`, the two `tools/axiom-shot` arms) renders each
/// frame off-screen.
fn shots(backend: &str) {
    let mut session = AgentSession::earthlike();
    let start = session.observe();
    println!(
        "[growth-agent] planet seed=0x{:016x}  peak={:.0} m  prominence={:.0} m  {:.0} m to summit",
        session.seed(),
        start.peak_height_m,
        start.prominence_m,
        start.distance_to_peak_m,
    );
    println!("[growth-agent] climbing to the summit (holding FORWARD through the harness)…");

    let forward = Action::forward();
    let mut obs = start;
    while !obs.reached_summit && obs.tick < CLIMB_TICK_CAP {
        obs = session.step(&forward);
    }
    println!(
        "[growth-agent] reached the mountain (tick {}, {:.0} m above spawn); shooting its sides from each cardinal direction",
        obs.tick, obs.height_above_spawn_m,
    );

    // Stand off the peak on each cardinal side and shoot the mountain rising against
    // the sky. The outward unit direction names the side the camera views from.
    let distance = 3500.0_f32;
    let cardinals = [
        ("north", 0.0_f32, -1.0_f32),
        ("east", 1.0_f32, 0.0_f32),
        ("south", 0.0_f32, 1.0_f32),
        ("west", -1.0_f32, 0.0_f32),
    ];
    let is_canvas2d = matches!(backend, "canvas2d" | "canvas");
    let suffix = if is_canvas2d { "_canvas2d" } else { "" };
    println!("[growth-agent] backend: {}", if is_canvas2d { "canvas2d" } else { "gpu" });
    for (name, dir_x, dir_z) in cardinals {
        let inputs = session.capture_portrait(dir_x, dir_z, distance);
        let (pixels, w, h) = if is_canvas2d {
            render_capture_canvas2d(&inputs)
        } else {
            (render_capture(&inputs), inputs.width, inputs.height)
        };
        let path = format!("screenshots/mountain_{name}{suffix}.png");
        write_png(&path, &pixels, w, h);
        println!(
            "[growth-agent] wrote {path} ({w}x{h}) from the {name} side ({distance:.0} m out)",
        );
    }
    println!("[growth-agent] done — 4 cardinal screenshots in screenshots/");
}

/// Render one capture's neutral inputs through the engine's native off-screen GPU
/// backend (kept here in the bin so wgpu's symbols never enter the crate's
/// `cdylib`). The terrain is one identity-world instance whose MVP is the camera
/// view-projection.
fn render_capture(inputs: &CaptureInputs) -> Vec<u8> {
    let mut instance = Vec::with_capacity(36);
    instance.extend_from_slice(&inputs.view_proj);
    instance.extend_from_slice(&IDENTITY_4X4);
    instance.extend_from_slice(&[1.0, 1.0, 1.0, 1.0]);

    let meshes = vec![(1u64, inputs.vertices.clone(), inputs.indices.clone())];
    let (mat_w, mat_h, mat_rgba) = &inputs.material;
    let materials = vec![(1u64, *mat_w, *mat_h, mat_rgba.clone())];
    let batches = vec![(1u64, 1u64, instance, 1u32)];

    axiom_gpu_backend::GpuBackendApi::render_offscreen_rgba(
        inputs.width,
        inputs.height,
        &meshes,
        &materials,
        &inputs.lights,
        inputs.light_view_proj,
        &batches,
        inputs.clear,
        // No SDF raymarch scene in the growth agent screenshot — meshes only.
        None,
    )
    .expect("a native GPU adapter renders the growth snapshot")
}

/// Render one capture through the software **Canvas 2D** backend (the same
/// rasterizer the browser's `?backend=canvas2d` path runs), fed the backend-neutral
/// `FramePacket` — one terrain draw (its MVP is the camera view-projection), the
/// portrait's lights and camera. Returns `(rgba8, width, height)` at the backend's
/// internal low-poly resolution. Canvas 2D ignores material textures and shades by
/// the mesh's per-vertex colours, so the snow/rock/vegetation bands carry the look.
fn render_capture_canvas2d(inputs: &CaptureInputs) -> (Vec<u8>, u32, u32) {
    let request = present_request(inputs.width, inputs.height);
    let mut backend = Canvas2dBackendApi::new(&request);
    backend.load_meshes(&[(1u64, inputs.vertices.clone(), inputs.indices.clone())]);
    backend.set_quality_level(3);

    let draw = FrameDrawItem::new(
        0,
        1,
        1,
        IDENTITY_4X4,
        inputs.view_proj,
        [1.0, 1.0, 1.0, 1.0],
        false,
    );
    let lights: Vec<FrameLight> = inputs
        .lights
        .iter()
        .map(|(kind, vec, color, intensity)| {
            FrameLight::new(*kind, *vec, [color[0], color[1], color[2], *intensity])
        })
        .collect();
    let directional = inputs.lights.iter().filter(|(k, ..)| *k == 0).count() as u32;
    let point = inputs.lights.iter().filter(|(k, ..)| *k == 1).count() as u32;
    let features = FrameFeatureSet::new(false, directional > 0, directional, point);
    let camera = Some(FrameCamera::new(IDENTITY_4X4, IDENTITY_4X4, inputs.view_proj));
    let packet = FramePacket::new(
        0,
        0,
        FrameViewport::new(inputs.width, inputs.height),
        inputs.clear,
        camera,
        vec![draw],
        lights,
        inputs.light_view_proj,
        features,
    );
    backend.render_offscreen_rgba(&packet)
}

/// Build the validated host presentation request the Canvas 2D backend is sized
/// from, the way windowing does (the backend reads only the viewport size).
/// Mirrors `tools/axiom-shot`'s `present_request`.
fn present_request(w: u32, h: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(w, h, Ratio::new(1.0).expect("finite scale"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-growth-agent")
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

/// Write RGBA8 pixels to a PNG (creating parent dirs), mirroring axiom-shot.
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

/// Headless climb: hold FORWARD through the harness until the summit, printing the
/// player's height as it rises.
fn climb() {
    let mut session = AgentSession::earthlike();
    let start = session.observe();
    println!(
        "[growth-agent] planet seed=0x{:016x}  peak={:.0} m  prominence={:.0} m  start: {:.0} m to summit",
        session.seed(),
        start.peak_height_m,
        start.prominence_m,
        start.distance_to_peak_m,
    );
    println!("[growth-agent] holding FORWARD through axiom-agent-harness…");

    let forward = Action::forward();
    let mut obs = start;
    while !obs.reached_summit && obs.tick < CLIMB_TICK_CAP {
        obs = session.step(&forward);
        if obs.tick % 200 == 0 {
            report(&obs);
        }
    }
    report(&obs);
    let tag = if obs.reached_summit {
        "SUMMIT REACHED"
    } else {
        "tick cap hit before summit"
    };
    println!(
        "[growth-agent] {tag}: tick={}  height_above_spawn={:.1} m  eye_height={:.1} m  dist_to_peak={:.1} m",
        obs.tick, obs.height_above_spawn_m, obs.eye_height_m, obs.distance_to_peak_m,
    );
}

/// One progress line during the climb.
fn report(obs: &Observation) {
    println!(
        "[growth-agent] tick={:>5}  pos=({:>7.0},{:>7.0})  height_above_spawn={:>7.1} m  ground={:>7.0} m  dist_to_peak={:>6.0} m  control=0x{:02x}",
        obs.tick,
        obs.x,
        obs.z,
        obs.height_above_spawn_m,
        obs.ground_height_m,
        obs.distance_to_peak_m,
        obs.control_code,
    );
}

/// HTTP control loop over the headless sim (POST /step, POST /reset, GET /state).
fn serve(addr: &str) {
    let server = Server::http(addr).expect("growth-agent: failed to bind the listen address");
    println!("axiom-growth agent listening on http://{addr}");
    println!("  POST /step {{action}}   POST /reset   GET /state");
    let mut session = AgentSession::earthlike();
    for mut req in server.incoming_requests() {
        let (status, body) = route(&mut session, &mut req);
        let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("static header is valid");
        let response = Response::from_string(body)
            .with_status_code(status)
            .with_header(header);
        let _ = req.respond(response);
    }
}

/// Route one HTTP request to the session, returning `(status, json_body)`.
fn route(session: &mut AgentSession, req: &mut tiny_http::Request) -> (u16, String) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/");
    match (method, path) {
        (Method::Post, "/step") => {
            let mut body = String::new();
            if std::io::Read::read_to_string(req.as_reader(), &mut body).is_err() {
                return (400, error_json("could not read request body"));
            }
            if body.trim().is_empty() {
                body = "{}".to_string();
            }
            match serde_json::from_str::<Action>(&body) {
                Ok(action) => (200, obs_json(&session.step(&action))),
                Err(e) => (400, error_json(&format!("bad action json: {e}"))),
            }
        }
        (Method::Post, "/reset") => {
            session.reset();
            (200, obs_json(&session.observe()))
        }
        (Method::Get, "/state") => (200, obs_json(&session.observe())),
        _ => (
            404,
            error_json("not found; use POST /step, POST /reset, GET /state"),
        ),
    }
}

/// The per-frame observation the live browser viewer pushes over the bridge. Only
/// the fields the agent needs to decide + report; unknown fields are ignored.
#[derive(Debug, Default, serde::Deserialize)]
struct BridgeObs {
    #[serde(default)]
    tick: u64,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    z: f32,
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    ground_height_m: f32,
    #[serde(default)]
    height_above_spawn_m: f32,
    #[serde(default)]
    distance_to_peak_m: f32,
    #[serde(default)]
    peak_x: f32,
    #[serde(default)]
    peak_z: f32,
    #[serde(default)]
    peak_height_m: f32,
    #[serde(default)]
    reached_summit: bool,
}

/// A world-unit `f32` as fixed-point micro-units, through the harness's own codec
/// ([`AgentHarnessApi::micro`]) — the single source of the observation-coordinate
/// convention.
fn micro(value: f32) -> i64 {
    AgentHarnessApi::micro(Meters::finite_or_zero(value))
}

/// Bridge mode: a WebSocket server the live browser viewer connects to. Each frame
/// the browser reports an observation; the agent decides "forward" through the
/// harness (routing the live player's pose + height through `axiom-agent`) and
/// pushes the held controls back.
fn run_bridge(ws_addr: &str) {
    let listener =
        TcpListener::bind(ws_addr).expect("growth-agent: failed to bind the websocket address");
    println!("axiom-growth agent BRIDGE on ws://{ws_addr}");
    println!("  open the viewer with  ?agent=ws://{ws_addr}");
    for stream in listener.incoming().flatten() {
        serve_browser(stream);
    }
}

/// Serve one connected browser: read its per-frame observation, decide FORWARD
/// through the harness, and reply with the held controls.
fn serve_browser(stream: TcpStream) {
    let Ok(mut ws) = tungstenite::accept(stream) else {
        return;
    };
    println!("bridge: browser connected");
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let obs: BridgeObs = serde_json::from_str(&text).unwrap_or_default();
                let self_pose = (
                    micro(obs.x),
                    micro(obs.ground_height_m),
                    micro(obs.z),
                    micro(obs.yaw),
                );
                let goal = (micro(obs.peak_x), micro(obs.peak_height_m), micro(obs.peak_z));
                let (control, _reason, _brain, _emitted) = AgentHarnessApi::decide_hold(
                    AGENT_RAW_ID,
                    obs.tick,
                    self_pose,
                    goal,
                    AgentHarnessApi::FORWARD,
                );
                let keys = control_to_keys(control);
                let action = serde_json::json!({ "keys": keys }).to_string();
                if ws.send(Message::text(action)).is_err() {
                    break;
                }
                if obs.tick % 30 == 0 {
                    println!(
                        "bridge: tick={:>5}  height_above_spawn={:>7.1} m  dist_to_peak={:>6.0} m",
                        obs.tick, obs.height_above_spawn_m, obs.distance_to_peak_m,
                    );
                }
                if obs.reached_summit {
                    println!(
                        "bridge: SUMMIT REACHED at tick {} ({:.1} m above spawn)",
                        obs.tick, obs.height_above_spawn_m,
                    );
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    println!("bridge: browser disconnected");
}

fn obs_json(obs: &Observation) -> String {
    serde_json::to_string(obs).expect("observation serializes")
}

fn error_json(msg: &str) -> String {
    let quoted = serde_json::to_string(msg).unwrap_or_else(|_| "\"error\"".to_string());
    format!("{{\"error\":{quoted}}}")
}
