//! The `wasm32` browser edge for the Animation Lab page (`web/lab.html`): the
//! sanctioned nondeterministic surface for the lab, kept out of the
//! deterministic core exactly like `crate::web`. It builds one engine app +
//! scene, drives a single [`AnimLab`] actor per animation frame, renders the
//! posed player, and forwards the clip picker (a bottom button bar, the arrow
//! keys, and the URL hash) into the lab's selection.

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::{App, Color, DefaultPlugins, Window};
use axiom_debug_overlay::DebugOverlayApi;
use axiom_kernel::Ratio;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent, PointerEvent};

use crate::app::{CANVAS_ID, HEIGHT, WIDTH};
use crate::lab::AnimLab;
use crate::scene::{EndZoneScene, LIVE_CAPACITY};

const PANEL_STYLE: &str = "position:fixed;left:0;right:0;bottom:0;display:flex;flex-wrap:wrap;gap:6px;justify-content:center;padding:8px;background:rgba(6,12,9,0.74);z-index:20;";
const BTN_STYLE: &str =
    "padding:5px 9px;border:1px solid #2b6;border-radius:6px;background:#12241a;color:#bfe;font:12px/1.2 ui-monospace,monospace;cursor:pointer;";
const BTN_ACTIVE: &str =
    "padding:5px 9px;border:1px solid #8f6;border-radius:6px;background:#2f6b3a;color:#eafff0;font:12px/1.2 ui-monospace,monospace;font-weight:700;cursor:pointer;";

#[wasm_bindgen]
pub fn end_zone_lab_start() {
    console_error_panic_hook::set_once();

    let labels: Vec<&'static str> = AnimLab::new().labels();
    let selection: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    let step: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));
    let orbit: Rc<RefCell<Orbit>> = Rc::new(RefCell::new(Orbit::default()));

    let buttons = mount_panel(&labels, &selection);
    install_keys(&step);
    install_orbit(&orbit);

    let mut overlay = DebugOverlayApi::new();
    overlay.mount_to_body();

    let mut windowing = WindowingApi::new();
    if windowing.configure_surface(WIDTH, HEIGHT).is_err() {
        web_sys::console::error_1(&"end-zone lab: surface configuration failed".into());
        return;
    }

    // Sky clear color (also the distance-fog target) — daylight, never black.
    let sky = Color::linear_rgb(
        Ratio::finite_or_zero(0.50),
        Ratio::finite_or_zero(0.67),
        Ratio::finite_or_zero(0.88),
    );
    let mut running = App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(sky),
        )
        .add_plugins(DefaultPlugins)
        .setup(|_world, _meshes, _materials| {})
        .build();
    let mut scene = EndZoneScene::install(&mut running);

    let mut lab = AnimLab::new();
    if let Some(index) = hash_index(&labels) {
        lab.select(index);
    }
    set_active(&buttons, lab.selected_index());

    let meshes = running.mesh_set();
    let materials = running.material_textures();

    let panel = buttons.clone();
    let picker = selection.clone();
    let stepper = step.clone();
    let orbiter = orbit.clone();
    let names = labels.clone();
    let mut frame_n: u64 = 0;

    let frame = move |_tick: u64| {
        // 1. Selection: an explicit button click wins, then arrow-key steps.
        let picked = picker.borrow_mut().take();
        if let Some(index) = picked {
            lab.select(index);
        }
        let mut delta = core::mem::replace(&mut *stepper.borrow_mut(), 0);
        while delta > 0 {
            lab.next();
            delta -= 1;
        }
        while delta < 0 {
            lab.prev();
            delta += 1;
        }
        if picked.is_some() || core::mem::replace(&mut delta, 0) != 0 {
            set_active(&panel, lab.selected_index());
            set_hash(names.get(lab.selected_index()).copied().unwrap_or(""));
        }

        // 2. Apply this frame's accumulated camera drag, then advance + render.
        {
            let mut o = orbiter.borrow_mut();
            let (dyaw, dpitch) = (core::mem::take(&mut o.dyaw), core::mem::take(&mut o.dpitch));
            (dyaw != 0.0 || dpitch != 0.0).then(|| lab.orbit(dyaw, dpitch));
        }
        let out = lab.step();
        scene.update(
            &mut running,
            &out.snapshot,
            &out.poses,
            lab.juice(),
            &out.camera,
            &[],
        );
        overlay.set_frame(frame_n, frame_n, 1, 60_000, 16_666);
        overlay.set_app_rows(&lab.overlay_rows());

        // 3. Tick the engine and hand the render data to the windowing loop.
        let outcome = running.tick(frame_n);
        frame_n += 1;
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
    };

    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, LIVE_CAPACITY, frame);
}

/// Mount the bottom clip-picker bar (one button per catalog clip). Each button
/// posts its index into `selection`; the returned handles let the frame loop
/// highlight the active one.
fn mount_panel(labels: &[&str], selection: &Rc<RefCell<Option<usize>>>) -> Vec<Element> {
    let mut buttons = Vec::new();
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return buttons;
    };
    let (Some(body), Ok(panel)) = (document.body(), document.create_element("div")) else {
        return buttons;
    };
    let _ = panel.set_id("axiom-lab-panel");
    let _ = panel.set_attribute("style", PANEL_STYLE);
    for (index, label) in labels.iter().enumerate() {
        if let Ok(button) = document.create_element("button") {
            let _ = button.set_attribute("style", BTN_STYLE);
            button.set_text_content(Some(label));
            let picker = selection.clone();
            let on_click = Closure::<dyn FnMut()>::new(move || {
                *picker.borrow_mut() = Some(index);
            });
            let _ =
                button.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref());
            on_click.forget();
            let _ = panel.append_child(&button);
            buttons.push(button);
        }
    }
    let _ = body.append_child(&panel);
    buttons
}

/// Highlight the active picker button; reset the rest.
fn set_active(buttons: &[Element], active: usize) {
    for (index, button) in buttons.iter().enumerate() {
        let style = if index == active { BTN_ACTIVE } else { BTN_STYLE };
        let _ = button.set_attribute("style", style);
    }
}

/// Drag sensitivity, radians of orbit per pixel of pointer travel.
const YAW_SENS: f32 = 0.006;
const PITCH_SENS: f32 = 0.005;

/// Pointer-drag state feeding the orbit-follow camera: whether a drag is live,
/// the last pointer position, and the yaw/pitch deltas the frame loop drains.
#[derive(Debug, Default)]
struct Orbit {
    active: bool,
    last_x: f32,
    last_y: f32,
    dyaw: f32,
    dpitch: f32,
}

/// Wire pointer drag (touch + mouse) to the orbit camera: a drag STARTS only on
/// the canvas (so the clip buttons keep their taps), but tracks on the window so
/// it continues even if the finger slides off the canvas.
fn install_orbit(orbit: &Rc<RefCell<Orbit>>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Some(canvas) = window.document().and_then(|d| d.get_element_by_id(CANVAS_ID)) {
        let down = orbit.clone();
        let on_down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
            let mut o = down.borrow_mut();
            o.active = true;
            o.last_x = e.client_x() as f32;
            o.last_y = e.client_y() as f32;
        });
        let _ =
            canvas.add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
        on_down.forget();
    }
    let moved = orbit.clone();
    let on_move = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        let mut o = moved.borrow_mut();
        let (x, y) = (e.client_x() as f32, e.client_y() as f32);
        // Drag right swings the camera around; drag up lifts it (screen y grows
        // downward, hence the negation).
        let (dx, dy) = (x - o.last_x, y - o.last_y);
        o.dyaw += f32::from(o.active) * dx * YAW_SENS;
        o.dpitch += f32::from(o.active) * -dy * PITCH_SENS;
        o.last_x = x;
        o.last_y = y;
    });
    let _ = window.add_event_listener_with_callback("pointermove", on_move.as_ref().unchecked_ref());
    on_move.forget();
    let up = orbit.clone();
    let on_up = Closure::<dyn FnMut(PointerEvent)>::new(move |_e: PointerEvent| {
        up.borrow_mut().active = false;
    });
    for event in ["pointerup", "pointercancel"] {
        let _ = window.add_event_listener_with_callback(event, on_up.as_ref().unchecked_ref());
    }
    on_up.forget();
}

/// Arrow keys (and square brackets) step to the previous / next clip.
fn install_keys(step: &Rc<RefCell<i32>>) {
    let step = step.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let delta = match e.code().as_str() {
            "ArrowRight" | "BracketRight" => 1,
            "ArrowLeft" | "BracketLeft" => -1,
            _ => 0,
        };
        if delta != 0 {
            e.prevent_default();
            *step.borrow_mut() += delta;
        }
    });
    if let Some(window) = web_sys::window() {
        let _ =
            window.add_event_listener_with_callback("keydown", on_down.as_ref().unchecked_ref());
    }
    on_down.forget();
}

/// A hash-safe slug for a clip label ("Drop Back" → "dropback").
fn slug(label: &str) -> String {
    label
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The clip index encoded in `#hash`, if any: a 1-based number or a slug match.
fn hash_index(labels: &[&str]) -> Option<usize> {
    let raw = web_sys::window()?.location().hash().ok()?;
    let key = raw.trim_start_matches('#');
    if key.is_empty() {
        return None;
    }
    if let Ok(n) = key.parse::<usize>() {
        if n >= 1 && n <= labels.len() {
            return Some(n - 1);
        }
    }
    let want = slug(key);
    labels.iter().position(|label| slug(label) == want)
}

/// Persist the framed clip in the URL hash so a hot-reload restores it.
fn set_hash(label: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.location().set_hash(&slug(label));
    }
}
