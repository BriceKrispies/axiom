//! The replay driver: for each [`CapturePoint`], build the app's `RunningApp`
//! deterministically at that tick (through `axiom_shot::registry`), read its
//! neutral live-render data, and render one frame through the chosen backend
//! (`axiom_shot::capture`). No renderer is duplicated — this only orchestrates
//! repeated captures.
//!
//! "Advance to tick T" is expressed via axiom-shot's `BuildParams::shot_tick`,
//! which folds the deterministic scenario forward T ticks (for soccer, the
//! penalty shot). Each frame is captured at the app's authored aspect fitted into
//! the requested viewport, so nothing is ever stretched.

use axiom::prelude::FrameOutcome;
use axiom_shot::capture;
use axiom_shot::registry::{self, BuildParams};

use crate::app_registry::FilmstripApp;
use crate::capture_plan::{fit_viewport, Backend, CapturePlan, CapturePoint};
use crate::FilmstripError;

/// The Canvas 2D quality tier used for captures (0..3). The top tier gives the
/// sharpest software framebuffer for human review.
const CANVAS_QUALITY: u8 = 3;

/// One captured frame: its RGBA8 pixels + actual size, and the point it shows.
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub point: CapturePoint,
}

/// Capture every point in `plan` for `app`, in order.
pub fn capture(
    plan: &CapturePlan,
    app: &FilmstripApp,
) -> Result<Vec<CapturedFrame>, FilmstripError> {
    let (rw, rh) = fit_viewport(app.native, plan.viewport);
    plan.points
        .iter()
        .map(|point| capture_one(plan, app, point, rw, rh))
        .collect()
}

fn capture_one(
    plan: &CapturePlan,
    app: &FilmstripApp,
    point: &CapturePoint,
    rw: u32,
    rh: u32,
) -> Result<CapturedFrame, FilmstripError> {
    let params = BuildParams {
        level: None,
        shot_tick: Some(point.tick as u32),
        frame: 0,
        stress_count: 0,
        soccer_debug: plan.debug_overlays,
    };
    let mut running =
        registry::build(app.shot_name, &params).ok_or_else(|| FilmstripError::CaptureFailed {
            tick: point.tick,
            reason: format!("axiom-shot has no slice '{}'", app.shot_name),
        })?;

    let meshes = running.mesh_set();
    let skinned = running.skinned_mesh_set();
    let outcome = running.tick(0);

    // Only the GPU path needs the material textures and the cinematic grade, so
    // they are computed inside the GPU arm — the software build touches neither.
    let (rgba, width, height) = match plan.backend {
        Backend::Canvas2d => {
            capture::render_canvas2d(&meshes, &skinned, &outcome, CANVAS_QUALITY, rw, rh)
        }
        Backend::Gpu => render_gpu_frame(
            &meshes,
            &skinned,
            &running.material_textures(),
            &outcome,
            rw,
            rh,
            plan.cinematic,
        ),
    };
    Ok(CapturedFrame {
        rgba,
        width,
        height,
        point: point.clone(),
    })
}

/// GPU capture (real off-screen wgpu). Compiled only with the `offscreen`
/// feature; `main` downgrades `--backend gpu` to canvas2d before capture when the
/// feature is off, so the fallback arm below is defensive. `cinematic` selects the
/// soccer retro-32-bit + exposure/contrast grade (matching the browser).
#[cfg(feature = "offscreen")]
fn render_gpu_frame(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    skinned: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
    w: u32,
    h: u32,
    cinematic: bool,
) -> (Vec<u8>, u32, u32) {
    let retro_32bit = cinematic.then(axiom_host::FrameRetro32BitProfile::retro_32bit);
    let postprocess = cinematic.then(axiom_host::FramePostProcess::cinematic);
    capture::render_gpu(
        meshes,
        skinned,
        materials,
        outcome,
        w,
        h,
        retro_32bit,
        postprocess,
    )
}

#[cfg(not(feature = "offscreen"))]
fn render_gpu_frame(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    skinned: &[(u64, Vec<f32>, Vec<u32>)],
    _materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
    w: u32,
    h: u32,
    _cinematic: bool,
) -> (Vec<u8>, u32, u32) {
    capture::render_canvas2d(meshes, skinned, outcome, CANVAS_QUALITY, w, h)
}
