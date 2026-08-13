//! The `wasm32` live arm: the browser edge, and nothing else.
//!
//! It reads two query parameters (`?detail=` and `?view=`), sizes the surface to
//! the device it actually landed on, builds the scene those two values name,
//! registers every generated mesh through the engine's ordinary `add_mesh_data`
//! path, spawns one node per object, sets a light rig, installs the orbit
//! camera's pointer gestures, and hands the per-frame closure to
//! `axiom-windowing`.
//!
//! Nothing here decides anything about geometry. Every mesh it registers came
//! out of [`crate::crucible_meshes`], which is native-testable and browser-free
//! — the browser's whole contribution is a canvas, a frame clock and a finger.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use axiom::prelude::*;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::debug_view::{chart_rgba, DebugView, CHART_SIZE};
use crate::orbit::OrbitState;
use crate::variant::CrucibleVariant;
use crate::{CANVAS_ID, HEIGHT, WIDTH};

/// The live backend's instance-buffer capacity, in **total instances across all
/// batches** — the renderer packs every batch back-to-back into one buffer and
/// silently drops whatever will not fit (see `SceneRenderer::record`), so a
/// capacity below the scene's instance count does not error, it just stops
/// drawing dogs partway round the ring.
///
/// The crucible spawns 1 terrain + 19 dogs × 23 bones = 438 instances. 2048
/// leaves room for a wider ring or a third one without another silent truncation
/// hunt, at a cost of one 2048-slot vertex buffer.
const LIVE_CAPACITY: u32 = 2048;

/// The fraction of the viewport width the page's stylesheet gives the canvas,
/// and the widest it will ever lay it out. These mirror the `width: min(94vw,
/// 1180px)` rule in `web/index.html`, and are used only when the canvas has not
/// been laid out yet — the measured box is always preferred over the predicted
/// one, because the page is the authority on its own layout.
const SURFACE_WIDTH_FRACTION: f64 = 0.94;
const MAX_SURFACE_CSS_WIDTH: f64 = 1180.0;

/// The canvas's authored aspect (also its CSS `aspect-ratio`). Holding it fixed
/// is what keeps the opening shot identical on every device: the projection
/// resolves against this aspect, so only the *resolution* follows the screen.
const SURFACE_ASPECT: f64 = 16.0 / 9.0;

/// The device-pixel band the surface is allowed to land in. The floor keeps a
/// tiny window from asking for a degenerate framebuffer; the ceiling is a hard
/// stop on how much a hostile viewport can ask a mobile GPU for.
const MIN_SURFACE_PIXELS: f64 = 240.0;
const MAX_SURFACE_PIXELS: f64 = 2560.0;

/// The device-pixel ratio is capped: a 3× phone would otherwise ask a mobile GPU
/// for a nine-times-oversampled framebuffer, which is the single easiest way to
/// turn a smooth scene into a slideshow. 2× is the point past which the extra
/// samples stop being visible on a hand-held display.
const MAX_DEVICE_PIXEL_RATIO: f64 = 2.0;

/// Delay before a settled resize/orientation change reloads the page.
const RESIZE_SETTLE_MS: i32 = 350;

/// Browser entry: build the crucible and present it.
#[wasm_bindgen]
pub fn crucible_start() {
    console_error_panic_hook::set_once();

    let variant = CrucibleVariant::from_label(&query_value("detail"));
    let view = DebugView::from_label(&query_value("view"));
    let (width, height) = surface_pixels();

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(width, height)
        .expect("surface dimensions are valid");

    let (mut running, installed) = build_running(variant, view, width, height);
    let meshes = running.mesh_set();
    let materials = running.material_textures();

    // The camera the page drives. `OrbitState::framed` seeds itself from the
    // authored eye/target in `install.rs`, so the first frame is the shot the
    // app has always opened on; every later frame is whatever the user's
    // gestures have made of it.
    let orbit: Rc<RefCell<OrbitState>> = Rc::new(RefCell::new(OrbitState::framed()));
    crate::pointer_input::install(CANVAS_ID, orbit.clone());
    install_resize_reload(width, height);

    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, LIVE_CAPACITY, move |tick| {
        // Re-author the camera before the tick that reads it. `set_camera`
        // reuses the existing camera node in place, so a moving camera costs no
        // allocation and leaks no scene nodes.
        orbit.borrow().apply(&mut running);
        // Re-author the two creatures' bones for this tick. The pose is a pure
        // function of `tick` — the browser supplies a frame number and nothing
        // else, so the same frame number always draws the same pose.
        installed.animate(&mut running, tick);
        let outcome = running.tick(tick);
        let lights = outcome
            .lights()
            .iter()
            .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
            .collect();
        (
            outcome.clear_color(),
            lights,
            outcome.light_view_proj(),
            outcome.mesh_batches(),
            outcome.camera_view_proj(),
            outcome.mesh_batch_casters(),
            outcome.sdf_scene().cloned(),
        )
    });
}

/// Realize the app and install the whole crucible into it.
fn build_running(
    variant: CrucibleVariant,
    view: DebugView,
    width: u32,
    height: u32,
) -> (RunningApp, crate::InstalledCrucible) {
    let mut running = App::new()
        .window(
            Window::new(width, height)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(chan(0.05), chan(0.07), chan(0.11))),
        )
        .add_plugins(DefaultPlugins)
        .setup(|_world, _meshes, _materials| {})
        .build();
    // Registered before the objects, and only for the view that samples it, so
    // the chart holds texture id 1 and no other view pays for its upload.
    let chart = view.uses_chart().then(|| {
        running
            .add_texture_data(CHART_SIZE, CHART_SIZE, chart_rgba())
            .expect("the authored normal chart is a well-formed RGBA8 image")
            .id()
    });
    let installed = crate::install_crucible(&mut running, variant, view, chart);
    (running, installed)
}

/// The surface to render into, in **device pixels**: the CSS box the page lays
/// the canvas out in, multiplied by the capped device-pixel ratio.
///
/// The engine has no live-resize path (see `NOTES.md`), so this is read once,
/// and it is read from the browser rather than hard-coded: a 1280×720 surface
/// stretched across a 390pt phone is both blurry and, on a 3× screen, needlessly
/// expensive — and a surface whose aspect disagrees with its box is *stretched*,
/// which no camera can undo.
///
/// There are three answers, in descending order of authority:
///
/// 1. the canvas's own laid-out content box — the page is the authority on its
///    own layout, and this is the only reading that cannot drift out of sync
///    with the stylesheet;
/// 2. the box the stylesheet's `min(94vw, 1180px)` rule *will* produce, from
///    `innerWidth`, for the moment before layout has happened;
/// 3. [`WIDTH`]/[`HEIGHT`], for a headless or hostile environment that will not
///    answer at all.
fn surface_pixels() -> (u32, u32) {
    let Some(window) = web_sys::window() else {
        return (WIDTH, HEIGHT);
    };
    let ratio = window.device_pixel_ratio();
    let ratio = if ratio.is_finite() && ratio >= 1.0 {
        ratio.min(MAX_DEVICE_PIXEL_RATIO)
    } else {
        1.0
    };
    let css_width = measured_css_width().or_else(predicted_css_width);
    let Some(css_width) = css_width else {
        return (WIDTH, HEIGHT);
    };
    let width = (css_width * ratio).clamp(MIN_SURFACE_PIXELS, MAX_SURFACE_PIXELS);
    (
        width.round() as u32,
        (width / SURFACE_ASPECT).round().max(1.0) as u32,
    )
}

/// The canvas's laid-out content width in CSS pixels, if the page has laid it
/// out. `client_width` excludes the border, which the drawing surface is not
/// stretched across.
fn measured_css_width() -> Option<f64> {
    let width = web_sys::window()?
        .document()?
        .get_element_by_id(CANVAS_ID)?
        .client_width();
    (width >= 1).then_some(width as f64)
}

/// The width the page's stylesheet will give the canvas, derived from the
/// viewport. Used only before layout; see [`surface_pixels`].
fn predicted_css_width() -> Option<f64> {
    let window = web_sys::window()?;
    let inner_width = number(window.inner_width())?;
    Some((inner_width * SURFACE_WIDTH_FRACTION).min(MAX_SURFACE_CSS_WIDTH))
}

/// A `JsValue` measurement as an `f64`, if the browser gave us a number at all.
fn number(value: Result<JsValue, JsValue>) -> Option<f64> {
    value.ok().and_then(|value| value.as_f64())
}

/// Reload the page when a resize or orientation change settles on a *different*
/// surface size.
///
/// `axiom-windowing` configures its surface once, before the run loop takes
/// ownership of the driver, and exposes no way to reconfigure it afterwards.
/// Rather than reach into the module to add one — the presentation surface is
/// the engine's to own, and a resize path there is a real design with a real
/// coverage burden, not an app's errand — the app does the one honest thing an
/// app can: it starts over. `reload()` re-runs `crucible_start` with the query
/// string (and so the `?detail=`/`?view=` selection) intact. The debounce keeps
/// a desktop window-drag from reloading on every intermediate pixel, and the
/// size comparison keeps a soft-keyboard or URL-bar reflow — which changes
/// `innerHeight` without changing the box we chose — from reloading at all.
fn install_resize_reload(width: u32, height: u32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let pending: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
    let handler = Closure::wrap(Box::new({
        let window = window.clone();
        let pending = pending.clone();
        move || {
            if let Some(handle) = pending.take() {
                window.clear_timeout_with_handle(handle);
            }
            let settled = Closure::once_into_js(move || {
                if surface_pixels() != (width, height) {
                    let _ = web_sys::window().map(|window| window.location().reload());
                }
            });
            let handle = web_sys::window().and_then(|window| {
                window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        settled.unchecked_ref(),
                        RESIZE_SETTLE_MS,
                    )
                    .ok()
            });
            pending.set(handle);
        }
    }) as Box<dyn FnMut()>);
    for event in ["resize", "orientationchange"] {
        let _ = window.add_event_listener_with_callback(event, handler.as_ref().unchecked_ref());
    }
    handler.forget();
}

/// A `0..1` intensity as a validated ratio.
fn chan(value: f32) -> Ratio {
    Ratio::finite_or_zero(value)
}

/// One query-string value from the page URL, or the empty string.
///
/// This is the app's only read of the browser environment at *scene-build* time,
/// and it happens once at startup — the scene it selects is then fixed for the
/// session, which is what keeps a "debug view" from being a piece of per-frame
/// state.
fn query_value(key: &str) -> String {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .map(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}
