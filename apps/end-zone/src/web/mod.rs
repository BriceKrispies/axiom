//! The `wasm32` browser edge: DOM listeners become the neutral
//! [`FrontendInputFrame`], the [`EndZoneShell`] runs one tick per animation
//! frame, the [`presenter::MenuPresenter`] renders the typed scene view, and
//! the adapters (storage / gamepad / tones / touch) translate between the
//! browser and the app's typed boundaries. Everything nondeterministic
//! lives in this directory.

pub mod emblem;
pub mod gamepad;
pub mod markup;
pub mod presenter;
pub mod storage;
pub mod style;
pub mod tones;
pub mod touch;

use std::cell::RefCell;
use std::rc::Rc;

use axiom_debug_overlay::DebugOverlayApi;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent, PointerEvent};

use crate::app::{TouchInput, CANVAS_ID, HEIGHT, WIDTH};
use crate::frontend::audio::recipe;
use crate::frontend::input::FrontendInputFrame;
use crate::frontend::persistence::FrontendProfile;
use crate::frontend::SimDirective;
use crate::scene::LIVE_CAPACITY;
use crate::shell::EndZoneShell;

use presenter::MenuPresenter;
use storage::{ConsoleSink, LocalStorageStore};
use tones::MenuTones;
use touch::{mount_touch_controls, set_controls_visible, TouchHeld};

/// The fixed frontend base seed: per-match seeds derive deterministically
/// from it and the match counter (shown on the match-setup screen).
const BASE_SEED: u64 = 0x00E2_D02E_F007_BA11;

/// Keys whose browser default (scrolling, help) is suppressed.
const PREVENTED: [&str; 7] = [
    "Space",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "F1",
    "Tab",
];

/// Pointer state shared between DOM listeners and the frame loop.
#[derive(Debug, Default)]
struct PointerShared {
    position: Option<(f32, f32)>,
    pressed_edge: bool,
    is_touch: bool,
}

#[wasm_bindgen]
pub fn end_zone_start() {
    console_error_panic_hook::set_once();

    let keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let pointer: Rc<RefCell<PointerShared>> = Rc::new(RefCell::new(PointerShared::default()));
    let touch: Rc<RefCell<TouchHeld>> = Rc::new(RefCell::new(TouchHeld::default()));
    let audio: Rc<RefCell<MenuTones>> = Rc::new(RefCell::new(MenuTones::new()));

    install_key_listeners(&keys, &audio);
    install_pointer_listeners(&pointer, &audio);
    mount_touch_controls(&touch);

    let mut overlay = DebugOverlayApi::new();
    overlay.mount_to_body();

    let mut windowing = WindowingApi::new();
    if windowing.configure_surface(WIDTH, HEIGHT).is_err() {
        web_sys::console::error_1(&"end-zone: surface configuration failed".into());
        return;
    }

    let mut store = LocalStorageStore;
    let mut sink = ConsoleSink;
    let profile = FrontendProfile::load_from(&store, &mut sink);
    let mut shell = EndZoneShell::new(BASE_SEED, profile);
    let mut menu = MenuPresenter::mount();
    let mut controls_visible = false;

    let meshes = shell.app.running().mesh_set();
    let materials = shell.app.running().material_textures();

    let frame_keys = keys.clone();
    let frame_pointer = pointer.clone();
    let frame_touch = touch.clone();
    let frame_audio = audio.clone();
    let frame = move |_tick: u64| {
        // 1. Assemble the neutral input frame.
        let pad = gamepad::poll();
        let mut keys_down = frame_keys.borrow().clone();
        let touch_frame = frame_touch.borrow_mut().take();
        if touch_frame.pause {
            // The touch PAUSE button is a one-frame KeyP token (the default
            // pause binding), so it flows through the same action model.
            keys_down.push("KeyP".to_string());
        }
        let (position, pressed, is_touch) = {
            let mut p = frame_pointer.borrow_mut();
            (p.position, core::mem::take(&mut p.pressed_edge), p.is_touch)
        };
        let input = FrontendInputFrame {
            keys_down,
            pad_down: pad.tokens,
            pointer: position,
            pointer_pressed: pressed,
            pointer_is_touch: is_touch,
        };
        let touch_input = TouchInput {
            stick_x: (touch_frame.stick_x + pad.stick.0).clamp(-1.0, 1.0),
            stick_y: (touch_frame.stick_y + pad.stick.1).clamp(-1.0, 1.0),
            primary: touch_frame.primary,
            reset: false,
        };
        let (css_w, css_h) = viewport_size();

        // 2. Advance the shell (frontend + game) one tick.
        let out = shell.frame(&input, touch_input, css_w, css_h);

        // 3. Present the menus, tones, persistence, and touch controls.
        menu.render(&out.view, css_w, css_h);
        let muted = shell.frontend.profile().settings.mute_when_unfocused && document_hidden();
        if !muted {
            let gain = shell.frontend.menu_tone_gain();
            let player = frame_audio.borrow();
            for intent in &out.view.sounds {
                player.play(recipe(*intent), gain);
            }
        }
        if out.view.persist {
            shell.frontend.profile().save_to(&mut store, &mut sink);
        }
        let want_controls =
            is_touch_device() && shell.frontend.sim_directive() == SimDirective::Live;
        if want_controls != controls_visible {
            set_controls_visible(want_controls);
            controls_visible = want_controls;
        }

        overlay.set_frame(
            shell.app.frame_index(),
            shell.app.frame_index(),
            1,
            60_000,
            16_666,
        );
        overlay.set_app_rows(&shell.app.overlay_rows());

        // 4. Hand the render data to the windowing loop.
        let outcome = out.outcome;
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

fn viewport_size() -> (f32, f32) {
    web_sys::window()
        .map(|w| {
            let width = w
                .inner_width()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(1280.0);
            let height = w
                .inner_height()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(720.0);
            (width as f32, height as f32)
        })
        .unwrap_or((1280.0, 720.0))
}

fn document_hidden() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .map(|d| d.hidden())
        .unwrap_or(false)
}

fn is_touch_device() -> bool {
    web_sys::window()
        .map(|w| w.navigator().max_touch_points() > 0)
        .unwrap_or(false)
}

/// Track every held `KeyboardEvent.code` (menus, bindings, and rebind
/// capture all consume the same neutral token stream).
fn install_key_listeners(keys: &Rc<RefCell<Vec<String>>>, audio: &Rc<RefCell<MenuTones>>) {
    let down = keys.clone();
    let down_audio = audio.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let code = e.code();
        if PREVENTED.contains(&code.as_str()) {
            e.prevent_default();
        }
        down_audio.borrow_mut().unlock();
        let mut set = down.borrow_mut();
        if !set.contains(&code) {
            set.push(code);
        }
    });
    let up = keys.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let code = e.code();
        up.borrow_mut().retain(|held_code| held_code != &code);
    });
    if let Some(window) = web_sys::window() {
        let _ =
            window.add_event_listener_with_callback("keydown", on_down.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback("keyup", on_up.as_ref().unchecked_ref());
    }
    on_down.forget();
    on_up.forget();
}

/// Track pointer position + press edges for the menu hit model.
fn install_pointer_listeners(pointer: &Rc<RefCell<PointerShared>>, audio: &Rc<RefCell<MenuTones>>) {
    let move_pointer = pointer.clone();
    let on_move = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        let mut p = move_pointer.borrow_mut();
        p.position = Some((e.client_x() as f32, e.client_y() as f32));
        p.is_touch = e.pointer_type() == "touch";
    });
    let down_pointer = pointer.clone();
    let down_audio = audio.clone();
    let on_down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        down_audio.borrow_mut().unlock();
        let mut p = down_pointer.borrow_mut();
        p.position = Some((e.client_x() as f32, e.client_y() as f32));
        p.pressed_edge = true;
        p.is_touch = e.pointer_type() == "touch";
    });
    if let Some(window) = web_sys::window() {
        let _ = window
            .add_event_listener_with_callback("pointermove", on_move.as_ref().unchecked_ref());
        let _ = window
            .add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
    }
    on_move.forget();
    on_down.forget();
}

/// Create one absolutely-positioned DOM element with an inline style.
pub(crate) fn mount_div(id: &str, style: &str, text: Option<&str>) -> Option<Element> {
    let document = web_sys::window()?.document()?;
    if let Some(existing) = document.get_element_by_id(id) {
        return Some(existing);
    }
    let el = document.create_element("div").ok()?;
    el.set_id(id);
    let _ = el.set_attribute("style", style);
    if let Some(text) = text {
        el.set_text_content(Some(text));
    }
    document.body()?.append_child(&el).ok()?;
    Some(el)
}
