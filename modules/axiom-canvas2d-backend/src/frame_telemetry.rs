//! Per-frame console telemetry for the software rasterizer, and the clock that
//! times it.
//!
//! Split out of the backend facade because it is a different job: the facade
//! turns a `FramePacket` into pixels, this reports on how that went. It is also
//! the crate's densest concentration of platform `#[cfg]` arms — every entry
//! here is a real implementation on `wasm32` and a no-op on native, so the
//! deterministic, fully-tested native path reads no clock and emits nothing.
//!
//! Everything is off by default and gated on `?profile=1`; see
//! [`profiling_enabled`] for why that gate matters more than it looks.

use crate::software_rasterizer::SoftwareRasterResult;

/// A monotonic millisecond clock for timing. wasm reads `performance.now()`;
/// native returns `0.0`, so the native (tested) path is deterministic and the
/// pure rasterizer is never timed.
#[cfg(target_arch = "wasm32")]
pub(crate) fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn now_ms() -> f64 {
    0.0
}

/// Whether per-frame profile logging is switched on, read ONCE from the page
/// URL (`?profile=1`) and cached.
///
/// Off by default, because these lines fire every frame: ~216k console entries
/// an hour, which the browser retains while devtools is open. That is a real
/// contributor to a session that is fine at first and laggy after a while, and
/// it costs a `format!` per frame even when nobody is reading it.
///
/// `tools/axiom-render-bench` opts in by loading the page with `?profile=1`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn profiling_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        web_sys::window()
            .and_then(|w| w.location().search().ok())
            .map(|search| search.contains("profile=1"))
            .unwrap_or(false)
    })
}

/// The phase sink installed on the rasterizer: log the coarse `convert` /
/// `rasterize` / `post` millisecond split as its own console line (wasm only;
/// native is a no-op, so the deterministic path emits nothing). `convert` is the
/// dominant Canvas2D cost — this line is what the render benchmark parses.
#[cfg(target_arch = "wasm32")]
pub(crate) fn log_phases(convert_ms: f64, rasterize_ms: f64, post_ms: f64) {
    // `then` keeps this branchless AND keeps the `format!` inside, so a frame
    // with profiling off does no string work at all.
    let _ = profiling_enabled().then(|| {
        let msg = format!(
            "axiom-canvas2d PROFILE: convert={convert_ms:.1}ms rasterize={rasterize_ms:.1}ms post={post_ms:.1}ms"
        );
        web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&msg));
    });
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn log_phases(_convert_ms: f64, _rasterize_ms: f64, _post_ms: f64) {}

/// The deep sink installed on the rasterizer: log the `convert`-phase project/shade
/// split as its own `axiom-canvas2d DEEP:` console line. The timing that feeds it is
/// gated to a **debug wasm** build, so this logger only ever sees a non-zero split
/// there; native and release-wasm hand it a zero split (or, on native, nothing at
/// all beyond the deterministic one-shot the discard path exercises). The render
/// benchmark's `--debug` mode parses this line.
#[cfg(all(target_arch = "wasm32", debug_assertions))]
pub(crate) fn deep_log(project_ms: f64, shade_ms: f64, draws: u32, triangles: usize) {
    let msg = format!(
        "axiom-canvas2d DEEP: project={project_ms:.1}ms shade={shade_ms:.1}ms draws={draws} tris={triangles}"
    );
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&msg));
}

#[cfg(not(all(target_arch = "wasm32", debug_assertions)))]
pub(crate) fn deep_log(_project_ms: f64, _shade_ms: f64, _draws: u32, _triangles: usize) {}

/// Log the per-frame raster telemetry + timings (wasm only; native is a no-op so
/// the deterministic path emits nothing and reads no clock).
#[cfg(target_arch = "wasm32")]
pub(crate) fn log_timing(result: &SoftwareRasterResult, raster_ms: f64, blit_ms: f64) {
    // Same gate as the phase line: off by default, and the format! stays inside.
    let _ = profiling_enabled().then(|| log_timing_now(result, raster_ms, blit_ms));
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn log_timing_now(result: &SoftwareRasterResult, raster_ms: f64, blit_ms: f64) {
    let c = result.conversion();
    let msg = format!(
        "axiom-canvas2d: backend=Canvas2d profile=LowPolyFramebuffer depth_cue_profile={} {}x{} \
         raster={raster_ms:.2}ms blit={blit_ms:.2}ms draws(proj/skip)={}/{} \
         tris(proj/rast/cull/decim)={}/{}/{}/{} candidate_px={} depth(test/write/reject)={}/{}/{} \
         cues(lit/tint/falloff)={}/{}/{} fog_px={} grade_px={} shadows={}/{}px outlines={}/{}px \
         horizon={} budget_exhausted={}",
        crate::canvas_depth_cue_profile::CanvasDepthCueProfile::low_poly_framebuffer().name(),
        result.width(),
        result.height(),
        c.projected_draws,
        c.skipped_draws,
        c.projected_triangles,
        result.rasterized_triangles(),
        c.culled_triangles,
        c.terrain_triangles_decimated,
        result.candidate_pixels(),
        result.depth_tested_pixels(),
        result.depth_written_pixels(),
        result.depth_rejected_pixels(),
        c.lit_triangles,
        c.height_tinted_triangles,
        c.distance_falloff_applied_triangles,
        result.depth_fog_applied_pixels(),
        result.vertical_grade_applied_pixels(),
        result.contact_shadows_drawn(),
        result.contact_shadow_pixels(),
        result.outlined_objects(),
        result.outline_pixels(),
        result.horizon_silhouette_drawn(),
        c.budget_exhausted,
    );
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&msg));
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn log_timing(_result: &SoftwareRasterResult, _raster_ms: f64, _blit_ms: f64) {}
