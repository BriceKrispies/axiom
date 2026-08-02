//! The `wasm32` live arm: the browser edge, and nothing else.
//!
//! This is the app's **platform boundary**, and it is deliberately thin. It
//! captures keyboard and gamepad state into neutral values, drives the windowing
//! render loop, hands the accumulated audio batch to a real `AudioContext`, and
//! paints a DOM HUD. Every decision it makes about the *game* it makes by asking
//! something else: [`Controls`] for the command, [`BurntRubber`] for the step,
//! [`HudModel`] for what to show.
//!
//! Nothing here is native-testable, which is exactly why nothing here is allowed
//! to be interesting. The rule the whole app is built around is that the browser
//! supplies elapsed time and key state, and receives pixels — and every
//! judgement in between happens in code that runs under `cargo test`.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_input::KeyToken;
use axiom_math::Vec2;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{KeyboardEvent, PointerEvent};

use crate::app::BurntRubber;
use crate::controls::{AnalogueInput, Controls, HeldKeys};
use crate::hud::{HudModel, CONTROLS_HINT};
use crate::profile::PlayProfile;
use crate::touch::TouchControls;
use crate::tuning::Tuning;
use crate::{CANVAS_ID, DEFAULT_SEED, HEIGHT, WIDTH};


/// The live per-instance buffer capacity. Comfortably above the road chunks,
/// scenery pools, traffic, car parts and effects put together.
pub const LIVE_CAPACITY: u32 = 16_384;

/// Gamepad button indices, in the standard mapping.
const PAD_SOUTH: usize = 0;
const PAD_WEST: usize = 2;
const PAD_LEFT_TRIGGER: usize = 6;
const PAD_RIGHT_TRIGGER: usize = 7;
/// Stick deflection below which the axis is treated as centred.
const STICK_DEADZONE: f64 = 0.12;

/// Start the live game.
#[wasm_bindgen]
pub fn burnt_rubber_start() {
    console_error_panic_hook::set_once();

    let held = Rc::new(RefCell::new(HeldKeys::default()));
    install_key_listeners(&held);

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(WIDTH, HEIGHT)
        .expect("the surface dimensions are valid");

    // THE seam. "Is this a phone?" is asked exactly once, here, and everything
    // that follows from the answer — the lane game vs the driving game, lane
    // buttons vs a joystick — is derived from this one value. See
    // `crate::PlayProfile` for the full contract.
    let (view_w, view_h) = viewport();
    let profile = PlayProfile::for_presentation(view_w, coarse_pointer());
    let app = BurntRubber::with_profile(DEFAULT_SEED, Tuning::DEFAULT, WIDTH, HEIGHT, profile);
    let mut touch = TouchControls::for_profile(view_w, view_h, profile);
    // A device that reports touch points gets the pad immediately, rather than
    // after a first blind tap.
    if touch_capable() {
        touch.engage();
    }
    let state = Rc::new(RefCell::new(LiveState {
        app,
        controls: Controls::new(),
        context: None,
        last_ms: 0.0,
        touch,
    }));
    install_pointer_listeners(&state);
    install_focus_listeners(&state, &held);

    let meshes = state.borrow_mut().app.running().mesh_set();
    let materials = state.borrow_mut().app.running().material_textures();

    // Hand the driver the whole render look the scene authored — the dark cool
    // ambient, the night's depth fog, the moonlit sky, and the bloom that makes
    // the emissive cues read as lights — so the live backend binds with it.
    // Without this the browser render silently uses the engine's default daylight
    // hemisphere and no fog, which is why the live race read like an overcast
    // afternoon with a cut-out horizon while the same scene captured correctly
    // off-screen. Each part is forwarded only when the scene authored it, so a
    // part the scene leaves unset stays on the backend's own default.
    {
        let mut guard = state.borrow_mut();
        let running = guard.app.running();
        let (ambient, depth_fog) = (running.ambient(), running.depth_fog());
        let (sky, bloom) = (running.sky(), running.bloom());
        windowing.set_ambient(ambient);
        if let Some(fog) = depth_fog {
            windowing.set_depth_fog(fog);
        }
        if let Some(sky) = sky {
            windowing.set_sky(sky);
        }
        if let Some(bloom) = bloom {
            windowing.set_bloom(bloom);
        }
    }

    // Which backend the cascade picks is not known yet — it is chosen
    // asynchronously, after `run_web_multi` below has consumed the driver — so
    // take the reading now and consult it per frame. The Canvas 2D software
    // rasterizer runs a framebuffer small enough that distant lane markings
    // project to less than a pixel and shimmer, so it gets the near-field paint
    // window and the GPU does not. Asking the driver rather than re-reading
    // `?backend=` is deliberate: the URL misses every fallback, and a page with
    // no parameter that landed on Canvas 2D because the GPU refused a device is
    // exactly the case that needs the window.
    let bound_backend = windowing.observe_bound_backend();

    let frame_state = state.clone();
    let frame_held = held.clone();
    let frame = move |tick: u64| {
        let mut guard = frame_state.borrow_mut();
        // Read, not latch: a device-loss rebuild re-runs the cascade, so this
        // can change mid-session in either direction.
        let software_raster = bound_backend() == Some(axiom_host::BackendKind::Canvas2d);
        guard.app.set_paint_near_field_only(software_raster);
        let elapsed = guard.elapsed_nanos();
        // Keyboard, gamepad and the on-screen pad all feed the same action
        // table: the pad's buttons and the gamepad's face buttons both arrive as
        // synthetic key tokens, so there is exactly one binding table and one
        // command path for all three devices.
        let (view_w, view_h) = viewport();
        guard.touch.resize(view_w, view_h);
        let mut keys = frame_held.borrow().tokens();
        keys.extend(gamepad_keys().into_iter().map(KeyToken::new));
        keys.extend(guard.touch.keys().into_iter().map(KeyToken::new));

        let pad_steer = guard.touch.steer();
        let mut analogue = read_gamepad();
        // The on-screen stick wins only when it is actually being pushed, so a
        // player with both a pad and a touchscreen is never fighting a centred
        // virtual stick.
        if pad_steer.abs() > analogue.steer.abs() {
            analogue.steer = pad_steer;
        }
        let command = guard.controls.command(&keys, analogue);
        if guard.controls.debug_pressed() {
            let showing = guard.app.debug_enabled();
            guard.app.set_debug(!showing);
        }

        // The first real input is what lets a browser start an AudioContext, so
        // the sound bank arms itself the moment the player touches anything.
        let interacted = !keys.is_empty();
        guard.arm_audio(interacted);

        // The simulation ticks the sound bank itself, once per fixed step — the
        // browser arm only hands the finished batch to Web Audio.
        guard.app.advance(elapsed, command);
        guard.realize_audio();

        update_hud(&guard.app.hud());
        update_touch_pad(&guard.touch);

        let outcome = guard.app.present();
        let lights = outcome
            .lights()
            .iter()
            .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
            .collect();
        let _ = tick;
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

/// Everything the frame closure owns.
struct LiveState {
    app: BurntRubber,
    controls: Controls,
    context: Option<web_sys::AudioContext>,
    last_ms: f64,
    touch: TouchControls,
}

impl LiveState {
    /// Real elapsed nanoseconds since the previous frame.
    ///
    /// This is the **only** place in the entire app that reads a clock, and the
    /// value goes straight into the fixed-step accumulator. Nothing downstream
    /// ever sees a duration.
    fn elapsed_nanos(&mut self) -> u64 {
        let now = web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0);
        let delta = if self.last_ms <= 0.0 {
            crate::tuning::FIXED_STEP_NANOS as f64 / 1.0e6
        } else {
            (now - self.last_ms).max(0.0)
        };
        self.last_ms = now;
        (delta * 1.0e6) as u64
    }

    /// Create the `AudioContext` on the first real interaction, and only then.
    fn arm_audio(&mut self, interacted: bool) {
        if !interacted || self.context.is_some() {
            return;
        }
        if let Ok(context) = web_sys::AudioContext::new() {
            self.context = Some(context);
            let audio = self.app.audio_mut();
            audio.enable(true);
            audio.set_volume(0.6);
        }
    }

    /// Hand the accumulated batch to Web Audio.
    ///
    /// This is the whole of the platform arm's involvement in sound. What plays
    /// and when was decided on the fixed step, inside the simulation, so the
    /// game sounds identical whatever the render backend is doing.
    fn realize_audio(&mut self) {
        if let Some(context) = &self.context {
            let _ = self.app.audio_mut().api().realize_into(context);
        }
    }
}

/// Install keydown/keyup listeners. `code` is used rather than `key` so the
/// bindings are layout-independent, with `key` as a fallback for the synthetic
/// events an on-screen keypad dispatches.
fn install_key_listeners(held: &Rc<RefCell<HeldKeys>>) {
    let down = held.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        // Both names are handed over, and `HeldKeys` picks ONE to track by. It
        // must not be `key`: that is the character produced, and it changes with
        // the modifiers, so a key pressed unshifted and released with Shift down
        // never matches its own release. See `HeldKeys::identity`.
        down.borrow_mut().press(&e.code(), &e.key());
        if DRIVING_CODES.contains(&e.code().as_str()) {
            e.prevent_default();
        }
    });
    let up = held.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        up.borrow_mut().release(&e.code(), &e.key());
    });
    let window = web_sys::window().expect("a browser window");
    window
        .add_event_listener_with_callback("keydown", on_down.as_ref().unchecked_ref())
        .expect("the keydown listener installs");
    window
        .add_event_listener_with_callback("keyup", on_up.as_ref().unchecked_ref())
        .expect("the keyup listener installs");
    on_down.forget();
    on_up.forget();
}

/// Release every held input when the page stops receiving events.
///
/// A browser delivers `keydown` and then simply never delivers the matching
/// `keyup` if focus moved away in between — alt-tab, a devtools panel, another
/// window. The key is then held for the rest of the session, and because it is
/// *input* rather than car state, resetting the car does not clear it: the car
/// is put back on the road and immediately told to turn again.
///
/// The same applies to a pointer that goes away without a `pointerup`.
fn install_focus_listeners(state: &Rc<RefCell<LiveState>>, held: &Rc<RefCell<HeldKeys>>) {
    let window = web_sys::window().expect("a browser window");

    let blur_state = state.clone();
    let blur_held = held.clone();
    let on_blur = Closure::<dyn FnMut()>::new(move || {
        blur_held.borrow_mut().clear();
        if let Ok(mut guard) = blur_state.try_borrow_mut() {
            guard.touch.release_all();
            // Restart the frame clock. A backgrounded tab keeps advancing
            // `performance.now()` while `requestAnimationFrame` is throttled, so
            // the first frame back would otherwise report the whole away-time as
            // one elapsed delta. `BurntRubber::advance` clamps that anyway, but
            // there is no reason to hand it a number we know is meaningless.
            guard.last_ms = 0.0;
        }
    });
    for name in ["blur", "pagehide"] {
        window
            .add_event_listener_with_callback(name, on_blur.as_ref().unchecked_ref())
            .expect("the focus listener installs");
    }
    if let Some(document) = window.document() {
        document
            .add_event_listener_with_callback(
                "visibilitychange",
                on_blur.as_ref().unchecked_ref(),
            )
            .expect("the visibility listener installs");
    }
    on_blur.forget();
}

/// The codes whose default browser behaviour (scrolling, mostly) is suppressed.
const DRIVING_CODES: &[&str] = &[
    "KeyW", "KeyA", "KeyS", "KeyD", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Space",
];

/// Poll the first connected gamepad.
///
/// Left stick steers, the triggers drive, the south face button is the
/// handbrake and the west one is boost — the layout every racing game on a pad
/// has used for twenty years.
fn read_gamepad() -> AnalogueInput {
    let Some(pads) = web_sys::window()
        .and_then(|w| w.navigator().get_gamepads().ok())
    else {
        return AnalogueInput::default();
    };
    for entry in pads.iter() {
        let Ok(pad) = entry.dyn_into::<web_sys::Gamepad>() else {
            continue;
        };
        if !pad.connected() {
            continue;
        }
        let axes = pad.axes();
        let buttons = pad.buttons();
        let axis = |index: u32| {
            axes.get(index)
                .as_f64()
                .filter(|v| v.abs() > STICK_DEADZONE)
                .unwrap_or(0.0) as f32
        };
        let button = |index: usize| {
            buttons
                .get(index as u32)
                .dyn_into::<web_sys::GamepadButton>()
                .map(|b| b.value() as f32)
                .unwrap_or(0.0)
        };
        return AnalogueInput {
            throttle: button(PAD_RIGHT_TRIGGER),
            brake: button(PAD_LEFT_TRIGGER),
            steer: axis(0),
        };
    }
    AnalogueInput::default()
}

/// The gamepad's digital buttons, folded into synthetic key tokens so they run
/// through the same action table the keyboard does.
pub fn gamepad_keys() -> Vec<&'static str> {
    let Some(pads) = web_sys::window().and_then(|w| w.navigator().get_gamepads().ok()) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    for entry in pads.iter() {
        let Ok(pad) = entry.dyn_into::<web_sys::Gamepad>() else {
            continue;
        };
        if !pad.connected() {
            continue;
        }
        let pressed = |index: usize| {
            pad.buttons()
                .get(index as u32)
                .dyn_into::<web_sys::GamepadButton>()
                .map(|b| b.pressed())
                .unwrap_or(false)
        };
        if pressed(PAD_SOUTH) {
            keys.push("Space");
        }
        if pressed(PAD_WEST) {
            keys.push("ShiftLeft");
        }
        break;
    }
    keys
}

/// The viewport size in CSS pixels.
fn viewport() -> (f32, f32) {
    let Some(window) = web_sys::window() else {
        return (crate::WIDTH as f32, crate::HEIGHT as f32);
    };
    let width = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(crate::WIDTH as f64);
    let height = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(crate::HEIGHT as f64);
    (width as f32, height as f32)
}

/// Whether the device reports any touch points.
/// Whether the device's primary pointer is coarse (a finger), the `pointer:
/// coarse` half of the media query in `web/index.html`.
///
/// Distinct from [`touch_capable`], deliberately: a laptop with a touchscreen
/// reports touch points but has a mouse as its *primary* pointer, so it gets the
/// on-screen pad if it wants one but keeps the driving game. `matchMedia` is the
/// browser's own answer to the same question the stylesheet asks, which is what
/// keeps layout and gameplay from disagreeing.
fn coarse_pointer() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
        .map(|m| m.matches())
        .unwrap_or(false)
}

fn touch_capable() -> bool {
    web_sys::window()
        .map(|w| w.navigator().max_touch_points() > 0)
        .unwrap_or(false)
}

/// Install pointer listeners for the on-screen controls.
///
/// Pointer events rather than touch events: one code path covers a finger, a
/// stylus and a mouse, and the pointer id the API already supplies is exactly
/// the identity [`TouchControls`] tracks presses by.
fn install_pointer_listeners(state: &Rc<RefCell<LiveState>>) {
    let window = web_sys::window().expect("a browser window");

    let down_state = state.clone();
    let on_down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        // A mouse is not a thumb. Without this, clicking anywhere in the lower
        // left of the page — including just clicking the canvas to focus it —
        // plants a virtual joystick and starts steering, and if the release is
        // ever missed the car is stuck turning with no on-screen pad visible to
        // explain why.
        if e.pointer_type() == "mouse" {
            return;
        }
        if let Ok(mut guard) = down_state.try_borrow_mut() {
            guard
                .touch
                .press(e.pointer_id(), Vec2::new(e.client_x() as f32, e.client_y() as f32));
        }
        e.prevent_default();
    });
    let move_state = state.clone();
    let on_move = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        if let Ok(mut guard) = move_state.try_borrow_mut() {
            guard
                .touch
                .drag(e.pointer_id(), Vec2::new(e.client_x() as f32, e.client_y() as f32));
        }
    });
    let up_state = state.clone();
    let on_up = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        if let Ok(mut guard) = up_state.try_borrow_mut() {
            guard.touch.release(e.pointer_id());
        }
    });

    for (name, closure) in [
        ("pointerdown", on_down.as_ref()),
        ("pointermove", on_move.as_ref()),
        ("pointerup", on_up.as_ref()),
        ("pointercancel", on_up.as_ref()),
        ("pointerleave", on_up.as_ref()),
    ] {
        window
            .add_event_listener_with_callback(name, closure.unchecked_ref())
            .expect("the pointer listener installs");
    }
    on_down.forget();
    on_move.forget();
    on_up.forget();
}

/// Create (once) and refresh the on-screen control pad.
///
/// The pad is DOM rather than scene geometry for the same reason the HUD is: it
/// is screen-space UI over a 3D view, it must stay crisp at any device pixel
/// ratio, and drawing it in the scene would mean a second camera and a second
/// pass to show five circles.
fn update_touch_pad(touch: &TouchControls) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let root = match document.get_element_by_id(PAD_ID) {
        Some(element) => element,
        None => {
            let element = document.create_element("div").expect("the pad div is created");
            element.set_id(PAD_ID);
            let _ = element.set_attribute("style", PAD_STYLE);
            if let Some(body) = document.body() {
                let _ = body.append_child(&element);
            }
            element
        }
    };
    if !touch.engaged() {
        let _ = root.set_attribute("style", &format!("{PAD_STYLE}display:none;"));
        return;
    }
    let _ = root.set_attribute("style", PAD_STYLE);

    let layout = touch.layout();
    let mut html = String::with_capacity(1_024);
    for slot in &layout.slots {
        let held = touch.is_held(slot.button);
        let (fill, border, text) = if held {
            ("rgba(255,209,102,.34)", "rgba(255,209,102,.95)", "#12161f")
        } else {
            ("rgba(12,18,28,.34)", "rgba(226,236,255,.42)", "#e6eeff")
        };
        let size = slot.radius * 2.0;
        let font = (slot.radius * 0.36).clamp(10.0, 20.0);
        html.push_str(&format!(
            "<div style=\"position:absolute;left:{left}px;top:{top}px;width:{size}px;\
             height:{size}px;border-radius:50%;background:{fill};border:2px solid {border};\
             color:{text};display:flex;align-items:center;justify-content:center;\
             font:700 {font}px ui-monospace,Menlo,Consolas,monospace;letter-spacing:.08em;\
             box-shadow:0 6px 22px rgba(0,0,0,.45)\">{label}</div>",
            left = slot.centre.x - slot.radius,
            top = slot.centre.y - slot.radius,
            size = size,
            fill = fill,
            border = border,
            text = text,
            font = font,
            label = slot.button.label(),
        ));
    }

    // The joystick only exists while a thumb is on it, which is the whole point
    // of a dynamic stick: nothing sits on screen until it is being used.
    if let Some(stick) = touch.stick() {
        let ring = layout.stick_radius;
        html.push_str(&format!(
            "<div style=\"position:absolute;left:{rl}px;top:{rt}px;width:{rs}px;height:{rs}px;\
             border-radius:50%;border:2px solid rgba(226,236,255,.34);\
             background:rgba(12,18,28,.26)\"></div>\
             <div style=\"position:absolute;left:{kl}px;top:{kt}px;width:{ks}px;height:{ks}px;\
             border-radius:50%;background:rgba(255,209,102,.85);\
             box-shadow:0 4px 18px rgba(0,0,0,.5)\"></div>",
            rl = stick.origin.x - ring,
            rt = stick.origin.y - ring,
            rs = ring * 2.0,
            kl = stick.knob.x - ring * 0.38,
            kt = stick.knob.y - ring * 0.38,
            ks = ring * 0.76,
        ));
    }
    root.set_inner_html(&html);
}

/// The on-screen pad's element id.
const PAD_ID: &str = "burnt-rubber-pad";

/// The pad's container style. `pointer-events: none` because the listeners are
/// on the window and work in viewport coordinates — the DOM circles are purely
/// a picture of state the model already owns, and must never intercept a touch.
const PAD_STYLE: &str = "position:fixed;inset:0;z-index:25;pointer-events:none;\
     user-select:none;-webkit-user-select:none;touch-action:none;";

/// Create (once) and refresh the DOM HUD.
///
/// A DOM overlay rather than in-scene text: the engine's text module produces
/// neutral glyph batches with no path into the renderer's draw list, so drawing
/// the HUD in 3D would mean building that bridge here — a general engine
/// capability, in an app, to show a speedometer. The established pattern in this
/// repository is a DOM overlay, and that is what this is.
fn update_hud(hud: &HudModel) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let root = match document.get_element_by_id(HUD_ID) {
        Some(element) => element,
        None => {
            let element = document.create_element("div").expect("the HUD div is created");
            element.set_id(HUD_ID);
            let _ = element.set_attribute("style", HUD_STYLE);
            if let Some(body) = document.body() {
                let _ = body.append_child(&element);
            }
            element
        }
    };

    let boost_bar = bar(hud.boost, 16);
    let progress_bar = bar(hud.progress, 20);
    let banner = hud.banner().unwrap_or_default();
    let hint = if hud.show_controls_hint {
        CONTROLS_HINT
    } else {
        ""
    };
    let state = [
        hud.boosting.then_some("BOOST"),
        hud.drifting.then_some("DRIFT"),
        hud.off_road.then_some("OFF ROAD"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("  ");

    root.set_inner_html(&format!(
        "<div style=\"font-size:52px;line-height:1;font-weight:700\">{speed}<span style=\"font-size:18px;opacity:.7\"> KM/H</span></div>\
         <div style=\"margin-top:6px\">BOOST [{boost_bar}]</div>\
         <div>{section}</div>\
         <div>[{progress_bar}] {percent}%  ·  {time}</div>\
         <div style=\"color:#ffd166;min-height:1.2em\">{state}</div>\
         <div style=\"color:#8ef;min-height:1.2em\">NEAR MISSES {near}</div>\
         <div style=\"position:fixed;left:0;right:0;top:34%;text-align:center;font-size:64px;\
                     font-weight:800;letter-spacing:.06em;text-shadow:0 0 24px #000\">{banner}</div>\
         <div style=\"position:fixed;left:0;right:0;bottom:14px;text-align:center;font-size:13px;opacity:.65\">{hint}</div>",
        speed = hud.speed_kmh,
        boost_bar = boost_bar,
        section = hud.section.name(),
        progress_bar = progress_bar,
        percent = hud.progress_percent(),
        time = hud.formatted_time(),
        state = state,
        near = hud.near_miss_count,
        banner = banner,
        hint = hint,
    ));
}

/// The HUD element id.
const HUD_ID: &str = "burnt-rubber-hud";

/// The HUD's style. Anchored top-left and `pointer-events: none`, so it never
/// covers the road or eats a click.
const HUD_STYLE: &str = "position:fixed;top:14px;left:18px;z-index:20;color:#f2f6ff;\
     font:16px/1.45 ui-monospace,Menlo,Consolas,monospace;\
     text-shadow:0 2px 8px #000,0 0 2px #000;pointer-events:none;user-select:none;";

/// A `[####----]` style bar of `width` cells.
fn bar(fraction: f32, width: usize) -> String {
    let filled = ((fraction.clamp(0.0, 1.0) * width as f32).round() as usize).min(width);
    let mut out = String::with_capacity(width);
    out.extend(std::iter::repeat('#').take(filled));
    out.extend(std::iter::repeat('-').take(width - filled));
    out
}
