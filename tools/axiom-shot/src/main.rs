//! `axiom-shot` — render any registered Axiom slice to a PNG, headless, via a
//! chosen backend.
//!
//! It ticks a selected slice's scene, pulls `RunningApp`'s neutral live-render
//! data (the same mesh set / material set / per-`(mesh, material)` instance
//! batches and lights that drive the browser), and renders it through a selected
//! backend:
//!
//!   * `--backend gpu` (default) — `axiom-gpu-backend`'s native off-screen arm,
//!     the SAME `scene_renderer` the browser's WebGPU/WebGL2 path runs. Requires
//!     the `offscreen` feature (`cargo run -p axiom-shot --features offscreen`).
//!   * `--backend canvas2d` — `axiom-canvas2d-backend`'s software z-buffer
//!     rasterizer, fed the SAME backend-neutral `FramePacket` windowing
//!     reconstructs from the instance batches. Available in the default build.
//!
//! The slice is chosen by `--app <name>`; `--app list` prints every registered
//! slice. Adding a renderable slice is adding one row to
//! [`axiom_shot::registry::registry`] (and a `slice.toml`, which
//! `xtask check-slices` cross-checks against the registry).
//!
//! It can also drive the first-person camera itself:
//!
//!   * `--script "ticks:held-inputs;..."` applies `FirstPersonInput` to
//!     controller 0 per tick, so an app can be walked to a vantage point.
//!
//! Usage:
//!   cargo run -p axiom-shot [--features offscreen] -- \
//!     [--app <name>|list] [--backend gpu|canvas2d] [--tick N] [--out PATH] \
//!     [--quality 0..3] [--frame N] \
//!     [--script "ticks:key=val,...;..."]

use axiom::prelude::*;
use axiom_shot::capture;
use axiom_shot::registry::{self, BuildParams, HEIGHT, WIDTH};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app = flag(&args, "--app").unwrap_or_else(|| "showcase".to_string());

    // `--app list` (or `--list`) prints the registered slices and exits.
    if app == "list" || args.iter().any(|a| a == "--list") {
        println!("axiom-shot registered slices:");
        registry::names().iter().for_each(|n| println!("  {n}"));
        return;
    }

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

    let params = BuildParams {
        frame: flag(&args, "--frame")
            .and_then(|f| f.parse().ok())
            .unwrap_or(0),
    };

    let mut running = registry::build(&app, &params).unwrap_or_else(|| {
        eprintln!(
            "axiom-shot: unknown --app '{app}', falling back to 'showcase'. Registered: {:?}",
            registry::names()
        );
        registry::build("showcase", &params).expect("showcase is always registered")
    });

    let meshes = running.mesh_set();
    let skinned_meshes = running.skinned_mesh_set();
    let materials = running.material_textures();
    let mut outcome = None;
    for t in 0..=render_tick {
        let frame = match controls.get(t as usize).copied() {
            Some(c) => running.tick_with_controls(t, &[], std::slice::from_ref(&c)),
            None => running.tick(t),
        };
        outcome = Some(frame);
    }
    let outcome = outcome.expect("at least one frame is ticked");

    // No registered slice carries a retro post profile today.
    let retro_32bit: Option<axiom_host::FrameRetro32BitProfile> = None;
    // Honour the app's authored colour grade, exactly as the live present arm
    // does: an app that authors a `FramePostProcess` (via `set_postprocess`) has
    // it ride onto the `FrameOutcome`, and the capture grades from it instead of
    // presenting the flat, washed-out raster. `None` presents untonemapped.
    let postprocess = outcome.postprocess();

    let (pixels, w, h) = render(
        &backend,
        &meshes,
        &skinned_meshes,
        &materials,
        &outcome,
        quality,
        retro_32bit,
        postprocess,
    );

    capture::write_png(&out, &pixels, w, h);
    println!("axiom-shot: wrote {out} ({w}x{h}, app={app}, backend={backend}, tick={render_tick})");
}

/// Render `outcome` through the requested backend, returning `(pixels, w, h)`.
/// The GPU arm requires the `offscreen` feature; without it, `--backend gpu`
/// warns and falls back to the always-available Canvas 2D path.
#[cfg(feature = "offscreen")]
fn render(
    backend: &str,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
    quality: u8,
    retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
    postprocess: Option<axiom_host::FramePostProcess>,
) -> (Vec<u8>, u32, u32) {
    match backend {
        "canvas2d" | "canvas" => {
            capture::render_canvas2d(meshes, skinned_meshes, outcome, quality, WIDTH, HEIGHT)
        }
        _ => capture::render_gpu(
            meshes,
            skinned_meshes,
            materials,
            outcome,
            WIDTH,
            HEIGHT,
            retro_32bit,
            postprocess,
        ),
    }
}

#[cfg(not(feature = "offscreen"))]
fn render(
    backend: &str,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
    _materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
    quality: u8,
    _retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
    _postprocess: Option<axiom_host::FramePostProcess>,
) -> (Vec<u8>, u32, u32) {
    (backend != "canvas2d" && backend != "canvas").then(|| {
        eprintln!(
            "axiom-shot: --backend {backend} requires the `offscreen` feature \
             (rebuild with `--features offscreen`); rendering canvas2d instead."
        );
    });
    capture::render_canvas2d(meshes, skinned_meshes, outcome, quality, WIDTH, HEIGHT)
}

/// One phase's held first-person inputs (per-tick deltas).
#[derive(Clone, Copy, Default)]
struct Hold {
    forward: f32,
    strafe: f32,
    yaw: f32,
    pitch: f32,
}

/// Expand a `--script` into one `FirstPersonInput` per tick (controller 0).
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

/// The value following `name` in `args`, if present.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
