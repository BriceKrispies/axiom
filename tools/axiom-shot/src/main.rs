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

use std::time::Instant;

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

    // `--profile-frames N`: render the SAME frame N times and report what it
    // costs, instead of writing a picture.
    //
    // This is the only realistic headless way to ask "why is this part of the
    // game slow". A section's cost is mostly *fragment* work — how much of the
    // frame is covered, how many lights and shadow taps each covered pixel runs,
    // how much of it is spent on pixels something later draws over — and none of
    // that is visible to a CPU profiler timing the simulation, nor to a counter
    // of triangles. Re-rendering one pinned frame on the real GPU measures it
    // directly, with no browser, no clock skew from a variable simulation, and
    // no frame-to-frame content change to average over.
    //
    // The frame is pinned by the registry entry (`--app burnt-rubber-tunnel`
    // places the car in the tunnel deterministically), so two runs differ only
    // by the renderer.
    // Render size overrides. The default is the slice's registered size, which is
    // a *composition* choice; fragment cost is a different question and needs the
    // size the game actually presents at. The phone arm renders 940x1672 at DPR 2
    // on ExtendedLimits (a 2x supersampled target), i.e. ~6.3 Mpix against this
    // tool's default 0.58 Mpix — an 11x difference in fragment count, which is
    // enough to hide a fill-bound cost completely.
    let pw: u32 = flag(&args, "--width")
        .and_then(|v| v.parse().ok())
        .unwrap_or(WIDTH);
    let ph: u32 = flag(&args, "--height")
        .and_then(|v| v.parse().ok())
        .unwrap_or(HEIGHT);
    let profile_frames: u32 = flag(&args, "--profile-frames")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    // `--profile-compare a,b[,c] --profile-frames N`: measure several slices
    // INTERLEAVED IN ONE PROCESS and report medians.
    //
    // Measuring them in separate runs does not work on this machine. Two
    // consecutive runs of the *same* slice at 1880x3344 measured 3.29 ms and
    // 13.52 ms per frame — a 4x swing from whatever else the GPU was doing —
    // so any A/B conclusion drawn across processes is noise. Interleaving the
    // slices inside one process and taking a median per slice makes the drift
    // common-mode: it moves every sample together and cancels in the ratio,
    // which is the only comparison being asked for.
    let compare = flag(&args, "--profile-compare").unwrap_or_default();
    if !compare.is_empty() {
        let names: Vec<String> = compare.split(',').map(|n| n.trim().to_string()).collect();
        let n = profile_frames.max(2);
        let trials = flag(&args, "--profile-trials")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5);

        // Build and tick each slice once; the frame is what we re-render.
        let built: Vec<(String, FrameOutcome, Vec<(u64, Vec<f32>, Vec<u32>)>, Vec<(u64, Vec<f32>, Vec<u32>)>, Vec<axiom_host::MaterialTexture>)> = names
            .iter()
            .filter_map(|name| {
                let mut r = registry::build(name, &params)?;
                let m = r.mesh_set();
                let sm = r.skinned_mesh_set();
                let mt = r.material_textures();
                let mut oc = None;
                for t in 0..=render_tick {
                    oc = Some(r.tick(t));
                }
                oc.map(|o| (name.clone(), o, m, sm, mt))
            })
            .collect();

        let mut samples: Vec<Vec<f64>> = vec![Vec::new(); built.len()];
        for _ in 0..trials {
            for (i, (_, oc, m, sm, mt)) in built.iter().enumerate() {
                let a = Instant::now();
                let (p1, _, _) = render(
                    &backend, m, sm, mt, oc, quality, retro_32bit, oc.postprocess(), 1, pw, ph,
                );
                let t1 = a.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(p1.len());
                let b = Instant::now();
                let (pn, _, _) = render(
                    &backend, m, sm, mt, oc, quality, retro_32bit, oc.postprocess(), n, pw, ph,
                );
                let tn = b.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(pn.len());
                samples[i].push((tn - t1) / f64::from(n - 1));
            }
        }

        println!("axiom-shot: {pw}x{ph} backend={backend} frames={n} trials={trials}");
        let mut baseline = 0.0f64;
        for (i, (name, ..)) in built.iter().enumerate() {
            samples[i].sort_by(f64::total_cmp);
            let med = samples[i][samples[i].len() / 2];
            let lo = samples[i][0];
            let hi = samples[i][samples[i].len() - 1];
            (i == 0).then(|| baseline = med);
            let rel = med / baseline.max(1.0e-9);
            println!("  {name:28} median {med:7.3}ms  (spread {lo:6.3}..{hi:6.3})  x{rel:.2}");
        }
        return;
    }

    if profile_frames > 0 {
        // Two runs, differenced. `render` pays full device+pipeline setup on
        // every call — instance, adapter, device, every pipeline, every buffer —
        // which on this machine is ~500 ms and swamps a single frame completely.
        // Timing repeated calls therefore measures wgpu start-up, not the game:
        // the first version of this flag reported three sections as identical to
        // within noise, which is exactly what setup-dominated timing looks like.
        //
        // Rendering the scene n times inside ONE call and differencing against a
        // 1-frame call cancels that constant exactly:
        //     T(n) = setup + n*frame        T(1) = setup + frame
        //     frame = (T(n) - T(1)) / (n - 1)
        let n = profile_frames.max(2);
        let warm = Instant::now();
        let _ = render(
            &backend, &meshes, &skinned_meshes, &materials, &outcome, quality, retro_32bit,
            postprocess, 1, pw, ph,
        );
        let _ = warm.elapsed();

        let t1s = Instant::now();
        let (px1, _, _) = render(
            &backend, &meshes, &skinned_meshes, &materials, &outcome, quality, retro_32bit,
            postprocess, 1, pw, ph,
        );
        let t1 = t1s.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(px1.len());

        let tns = Instant::now();
        let (pxn, _, _) = render(
            &backend, &meshes, &skinned_meshes, &materials, &outcome, quality, retro_32bit,
            postprocess, n, pw, ph,
        );
        let tn = tns.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(pxn.len());

        let per_frame = (tn - t1) / f64::from(n - 1);
        println!(
            "axiom-shot: app={app} backend={backend} {pw}x{ph}               setup+1={t1:.1}ms  setup+{n}={tn:.1}ms  =>  frame {per_frame:.3}ms               ({:.0} fps if frame-bound)",
            1000.0 / per_frame.max(1.0e-6)
        );
        return;
    }

    let (pixels, w, h) = render(
        &backend,
        &meshes,
        &skinned_meshes,
        &materials,
        &outcome,
        quality,
        retro_32bit,
        postprocess,
        1,
        pw,
        ph,
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
    materials: &[axiom_host::MaterialTexture],
    outcome: &FrameOutcome,
    quality: u8,
    retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
    postprocess: Option<axiom_host::FramePostProcess>,
    repeat: u32,
    w: u32,
    h: u32,
) -> (Vec<u8>, u32, u32) {
    match backend {
        "canvas2d" | "canvas" => {
            capture::render_canvas2d(meshes, skinned_meshes, outcome, quality, w, h)
        }
        _ => capture::render_gpu(
            meshes,
            skinned_meshes,
            materials,
            outcome,
            w,
            h,
            retro_32bit,
            postprocess,
            repeat,
        ),
    }
}

#[cfg(not(feature = "offscreen"))]
fn render(
    backend: &str,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
    _materials: &[axiom_host::MaterialTexture],
    outcome: &FrameOutcome,
    quality: u8,
    _retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
    _postprocess: Option<axiom_host::FramePostProcess>,
    _repeat: u32,
    w: u32,
    h: u32,
) -> (Vec<u8>, u32, u32) {
    (backend != "canvas2d" && backend != "canvas").then(|| {
        eprintln!(
            "axiom-shot: --backend {backend} requires the `offscreen` feature \
             (rebuild with `--features offscreen`); rendering canvas2d instead."
        );
    });
    capture::render_canvas2d(meshes, skinned_meshes, outcome, quality, w, h)
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
