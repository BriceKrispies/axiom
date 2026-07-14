//! The live `wasm32` arm: keyboard + mouse-look capture, the windowing render
//! loop, and the DOM HUD. Never compiled on native — the deterministic game core
//! lives in `lib.rs`; this is the thin nondeterministic edge.
//!
//! Controls. Desktop: click the canvas to capture the mouse (Pointer Lock), then
//! mouse to look (yaw + pitch), WASD to move, left-click to fire, Esc to release.
//! Touch/keyboard: the gallery's synthetic-key on-screen pad — ←/→ turn, ↑/↓
//! move, FIRE — and arrows/WASD/Space — all still work alongside the mouse.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use axiom::prelude::{FrameOutcome, RunningApp};
use axiom_debug_overlay::DebugOverlayApi;
use axiom_interface::{InterfaceInputEvent, KeyBinding, Keymap};
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, EventSource, KeyboardEvent, MessageEvent, MouseEvent, WebSocket};

use super::{build_retro_fps_app, reload_retro_fps, RetroFpsGame, Hud as GameHud, Intent, CANVAS_ID};
use crate::level::LevelDoc;

/// Radians of look per pixel of mouse movement.
const MOUSE_SENSITIVITY: f32 = 0.0025;

/// Held-key state, polled into an [`Intent`] each frame.
#[derive(Default, Clone, Copy)]
struct Keys {
    forward: bool,
    backward: bool,
    turn_left: bool,
    turn_right: bool,
    strafe_left: bool,
    strafe_right: bool,
    fire: bool,
}

impl Keys {
    fn intent(self) -> Intent {
        Intent {
            forward: self.forward,
            backward: self.backward,
            turn_left: self.turn_left,
            turn_right: self.turn_right,
            strafe_left: self.strafe_left,
            strafe_right: self.strafe_right,
            fire: self.fire,
            look_yaw: 0.0,
            look_pitch: 0.0,
        }
    }
}

/// Mouse-look deltas accumulated between frames (radians), drained each tick.
#[derive(Default, Clone, Copy)]
struct Look {
    yaw: f32,
    pitch: f32,
}

/// The held input an external agent is driving over the bridge WebSocket, plus a
/// one-shot request to attach a rendered image to the next observation. Merged
/// into the per-frame intent so the agent and a local player can both drive.
#[derive(Default, Clone, Copy)]
struct Remote {
    keys: Keys,
    yaw: f32,
    pitch: f32,
    render_once: bool,
}

/// Log a line to the browser console, prefixed so the demo is easy to spot.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[retro_fps] {msg}")));
}

/// Format the player pose as debug-overlay read-out rows: the standing position
/// `(x, z)`, the look angles `(yaw, pitch)` in radians, and the unit look
/// direction as a world `x/y/z` vector. With yaw=0 looking −Z and +pitch looking
/// up, the forward vector is `(−sin yaw·cos pitch, sin pitch, −cos yaw·cos pitch)`.
/// These are the exact numbers an agent reproduction teleports to.
fn pose_rows(px: f32, pz: f32, yaw: f32, pitch: f32) -> Vec<(String, String)> {
    let (cp, sp) = (pitch.cos(), pitch.sin());
    let (cy, sy) = (yaw.cos(), yaw.sin());
    let (fx, fy, fz) = (-sy * cp, sp, -cy * cp);
    vec![
        ("pos x z".to_string(), format!("{px:.3}  {pz:.3}")),
        (
            "look yaw pitch".to_string(),
            format!("{yaw:.3}  {pitch:.3}"),
        ),
        (
            "dir x y z".to_string(),
            format!("{fx:+.2} {fy:+.2} {fz:+.2}"),
        ),
    ]
}

/// The current document visibility (`"visible"`/`"hidden"`) for the overlay's
/// visibility row, defaulting to `"visible"` when there is no document.
fn document_visibility() -> &'static str {
    let hidden = web_sys::window()
        .and_then(|window| window.document())
        .map(|document| document.hidden())
        .unwrap_or(false);
    if hidden {
        "hidden"
    } else {
        "visible"
    }
}

/// The browser entry: build the game + engine app, capture the keyboard, mount
/// the HUD, and drive the live windowing loop.
#[wasm_bindgen]
pub fn retro_fps_start() {
    console_error_panic_hook::set_once();
    log("start(): building level");

    let keys = Rc::new(RefCell::new(Keys::default()));
    install_key_listener(&keys, "keydown", true);
    install_key_listener(&keys, "keyup", false);

    // Mouse-look: click captures the pointer; movement accumulates yaw/pitch;
    // left-click fires while captured.
    let look = Rc::new(RefCell::new(Look::default()));
    install_pointer_lock();
    install_mouse_look(&look);
    install_mouse_fire(&keys);

    // Optional external-agent bridge: `?agent=ws://host:port` opens a control
    // socket whose held input is merged into each frame and whose observations
    // (HUD + frame hash, plus a canvas snapshot on request) are streamed back.
    let remote = install_agent_bridge();

    // Subscribe to live level edits (served over SSE by `axiom-dev-reload`); each
    // saved `level.axiom` lands in this slot and is applied at the next frame.
    let pending_level = install_level_reload();

    let hud = Hud::mount();

    // The developer debug overlay: a Backquote-toggled read-out the frame loop
    // pushes the live player pose into (position + look direction), so a
    // view-dependent rendering artifact can be reproduced at an exact pose. Hidden
    // by default; press ` to toggle it on/off (Shift+` density, Ctrl+` pin,
    // Alt+` console).
    let mut overlay = DebugOverlayApi::new();
    overlay.mount_to_body();

    let doc = LevelDoc::default();
    let game = Rc::new(RefCell::new(RetroFpsGame::from_level(&doc)));
    let (running_app, initial_assets) = build_retro_fps_app(&doc);
    // Bind each enemy to its engine Entity so the game classifies raycast/overlap
    // hits from the first tick. The runtime-respawn handles (cube + enemy material)
    // are held alongside, refreshed whenever a reload re-authors the scene.
    game.borrow_mut().bind_entities(&running_app);
    let (vertices, indices) = running_app.mesh_vertex_stream();
    let running = Rc::new(RefCell::new(running_app));
    let assets = Rc::new(Cell::new(initial_assets));
    // Size the live backend's per-instance buffer to the grid's capacity (not the
    // current renderable count), so a reload can add walls/enemies up to the full
    // grid without exceeding the buffer.
    let max_instances = doc.grid_capacity() as u32;

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(960, 600)
        .expect("surface dimensions are valid");

    let tick = Rc::new(Cell::new(0_u64));

    // Fork hooks for the scrubber: snapshot serializes the engine scene
    // (`snapshot_sim`) framed with the retro FPS game state (`write_state`); restore
    // splits the two back apart and reinstates both, so a fork resumes the live
    // game from the recorded frame's exact state.
    let snapshot = make_snapshot(&running, &game);
    let restore = make_restore(&running, &game);

    let frame = {
        let game = game.clone();
        let running = running.clone();
        let assets = assets.clone();
        let tick = tick.clone();
        // The windowing loop hands each live frame its engine-owned identity and
        // cadence read-out: `frame_index` is the loop's frame counter (it advances
        // across scrubs/forks, unlike the game's sim tick), and the timing pair is
        // the smoothed fps×1000 / mean frame-µs the loop measures. We surface both
        // in the debug overlay below.
        move |frame_index: u64, fps_milli: u32, frame_micros: u32| {
            // Apply a pending level edit at this tick boundary: rebuild the game
            // and re-author the engine scene in place. The engine tick keeps
            // counting (the host driver requires a monotone sequence); only the
            // game and scene contents reset to the new document.
            if let Some(text) = pending_level.borrow_mut().take() {
                let new_doc = LevelDoc::parse(&text);
                *game.borrow_mut() = RetroFpsGame::from_level(&new_doc);
                let new_assets = reload_retro_fps(&mut running.borrow_mut(), &new_doc);
                assets.set(new_assets);
                // Re-bind the new scene's enemy nodes to the rebuilt game.
                game.borrow_mut().bind_entities(&running.borrow());
                log("level reloaded from edit");
            }

            // Fold this frame's accumulated mouse-look into the held-key intent,
            // then reset the accumulator.
            let mut intent = keys.borrow().intent();
            {
                let mut l = look.borrow_mut();
                intent.look_yaw = l.yaw;
                intent.look_pitch = l.pitch;
                *l = Look::default();
            }
            // Merge any agent-driven held input on top.
            let render_now = remote
                .as_ref()
                .map(|r| merge_remote(&mut intent, &r.borrow()))
                .unwrap_or(false);

            // The game asks the engine for its spatial answers (walls, hitscan,
            // contact), so step borrows the running app; that borrow is dropped
            // before the app is mutated (despawns) and ticked.
            let commands = {
                let app = running.borrow();
                game.borrow_mut().step(intent, &*app)
            };
            let now = tick.get();
            let outcome = {
                let mut app = running.borrow_mut();
                // Despawn enemies killed this tick / spawn any a respawn revived,
                // rebinding fresh nodes back into the game.
                super::apply_lifecycle(&mut game.borrow_mut(), &mut app, &assets.get(), &commands);
                app.tick_with_controls(now, &commands.enemies, &[commands.control])
            };
            tick.set(now + 1);
            hud.update(&commands.hud);

            // Feed the engine's live frame diagnostics into the overlay so its
            // frame/tick/fps/frame-ms rows read real values: `frame_index` is the
            // windowing frame counter, `now` is the game sim tick (the overlay
            // shows them as distinct fields because they diverge across a scrub or
            // fork), and the cadence pair comes straight from the loop's clock.
            // retro FPS steps the sim exactly once per frame, so `sim ticks` is 1.
            overlay.set_frame(frame_index, now, 1, fps_milli, frame_micros);
            overlay.set_visibility(document_visibility());

            // Surface the post-step player pose in the debug overlay so a bad view
            // can be read off exactly (position + look direction) and handed back
            // for an exact-pose reproduction.
            let (px, pz, yaw, pitch) = game.borrow().pose();
            overlay.set_app_rows(&pose_rows(px, pz, yaw, pitch));

            if let Some(r) = &remote {
                send_observation(r, tick.get(), &commands.hud, &outcome, render_now);
            }
            // `instance_floats` packs draws in submission order into one batch, so
            // the caster flags ride in that same draw order (not the grouped
            // `mesh_batch_casters` order). The camera drives the Canvas backend's
            // planar contact shadows under the enemy cubes.
            let casters = outcome
                .draws()
                .iter()
                .map(|d| d.casts_contact_shadow())
                .collect();
            (
                outcome.clear_color(),
                outcome.instance_floats(),
                outcome.draws().len() as u32,
                outcome.camera_view_proj(),
                casters,
            )
        }
    };

    let _ = windowing.run_web_forkable(
        CANVAS_ID,
        vertices,
        indices,
        max_instances,
        frame,
        snapshot,
        restore,
    );
}

/// Build the scrubber's snapshot hook: serialize the engine scene + retro FPS game
/// state for the current frame, framed as `[u32 scene_len][scene][game]`.
fn make_snapshot(
    running: &Rc<RefCell<RunningApp>>,
    game: &Rc<RefCell<RetroFpsGame>>,
) -> Rc<dyn Fn() -> Vec<u8>> {
    let running = running.clone();
    let game = game.clone();
    Rc::new(move || {
        let scene = running.borrow().snapshot_sim();
        let game_bytes = game.borrow().write_state();
        let mut out = Vec::with_capacity(4 + scene.len() + game_bytes.len());
        out.extend_from_slice(&(scene.len() as u32).to_le_bytes());
        out.extend_from_slice(&scene);
        out.extend_from_slice(&game_bytes);
        out
    })
}

/// Build the scrubber's restore hook: split `[u32 scene_len][scene][game]` and
/// reinstate both the engine scene (`restore_sim`) and the retro FPS game state
/// (`read_state`). A malformed buffer is ignored.
fn make_restore(
    running: &Rc<RefCell<RunningApp>>,
    game: &Rc<RefCell<RetroFpsGame>>,
) -> Rc<dyn Fn(&[u8])> {
    let running = running.clone();
    let game = game.clone();
    Rc::new(move |bytes: &[u8]| {
        let split = bytes
            .get(0..4)
            .and_then(|h| <[u8; 4]>::try_from(h).ok())
            .map(u32::from_le_bytes)
            .map(|n| n as usize)
            .and_then(|n| bytes.get(4..4 + n).map(|scene| (scene, &bytes[4 + n..])));
        if let Some((scene, game_bytes)) = split {
            let _ = running.borrow_mut().restore_sim(scene);
            game.borrow_mut().read_state(game_bytes);
            // The restored game's enemies have no Entity (handles aren't
            // serialized); re-bind them to the restored scene's nodes so spatial
            // hits classify correctly when the fork resumes.
            game.borrow_mut().bind_entities(&running.borrow());
        }
    })
}

/// Subscribe to live level edits over Server-Sent Events. The `axiom-dev-reload`
/// dev server pushes the full contents of `level.axiom` on `/events` whenever the
/// file changes; each message lands in the returned slot, which the frame loop
/// drains and applies. If no `/events` endpoint is reachable (e.g. served by a
/// plain static server), the slot simply never fills and the demo runs the
/// built-in level — hot-reload is a dev convenience, never required.
fn install_level_reload() -> Rc<RefCell<Option<String>>> {
    let pending = Rc::new(RefCell::new(None::<String>));
    match EventSource::new("/events") {
        Ok(es) => {
            let sink = pending.clone();
            let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                if let Some(text) = e.data().as_string() {
                    *sink.borrow_mut() = Some(text);
                }
            });
            es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on_message.forget();
            // Keep the EventSource alive for the lifetime of the page.
            std::mem::forget(es);
            log("level hot-reload: subscribed to /events");
        }
        Err(_) => log("level hot-reload unavailable (no /events endpoint)"),
    }
    pending
}

/// Merge the agent's held input into `intent`; returns whether a snapshot was
/// requested for this frame's observation.
fn merge_remote(intent: &mut Intent, remote: &Remote) -> bool {
    intent.forward |= remote.keys.forward;
    intent.backward |= remote.keys.backward;
    intent.turn_left |= remote.keys.turn_left;
    intent.turn_right |= remote.keys.turn_right;
    intent.strafe_left |= remote.keys.strafe_left;
    intent.strafe_right |= remote.keys.strafe_right;
    intent.fire |= remote.keys.fire;
    intent.look_yaw += remote.yaw;
    intent.look_pitch += remote.pitch;
    remote.render_once
}

/// Open the agent bridge socket if `?agent=<ws-url>` is present, wiring incoming
/// action JSON into the shared [`Remote`] state. Returns `None` when no agent is
/// configured (the demo then runs exactly as before).
fn install_agent_bridge() -> Option<Rc<RefCell<Remote>>> {
    let search = web_sys::window()?.location().search().ok()?;
    let url = agent_url(&search)?;
    let ws = WebSocket::new(&url).ok()?;
    let remote = Rc::new(RefCell::new(Remote::default()));
    let sink = remote.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if let Some(text) = e.data().as_string() {
            apply_action_json(&sink, &text);
        }
    });
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    log(&format!("agent bridge connecting to {url}"));
    // Stash the socket so the frame loop can send observations on it.
    SOCKET.with(|s| *s.borrow_mut() = Some(ws));
    Some(remote)
}

thread_local! {
    /// The bridge socket (single-threaded wasm), used to send observations.
    static SOCKET: RefCell<Option<WebSocket>> = const { RefCell::new(None) };
}

/// Decode the `agent` query parameter (a URL-encoded ws/wss URL) from a
/// `location.search` string, or `None` if absent.
fn agent_url(search: &str) -> Option<String> {
    let query = search.strip_prefix('?').unwrap_or(search);
    let raw = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("agent="))
        .filter(|raw| !raw.is_empty())?;
    js_sys::decode_uri_component(raw)
        .ok()
        .map(String::from)
        .filter(|url| !url.is_empty())
}

/// Apply one action JSON message into the shared remote state.
fn apply_action_json(remote: &Rc<RefCell<Remote>>, text: &str) {
    let Ok(value) = js_sys::JSON::parse(text) else {
        return;
    };
    let mut r = Remote::default();
    r.yaw = field_f32(&value, "yaw");
    r.pitch = field_f32(&value, "pitch");
    r.render_once = field_bool(&value, "render");
    r.keys.fire = field_bool(&value, "fire");
    if let Ok(keys) = js_sys::Reflect::get(&value, &JsValue::from_str("keys")) {
        if let Ok(arr) = keys.dyn_into::<js_sys::Array>() {
            arr.iter().for_each(|k| {
                if let Some(name) = k.as_string() {
                    set_remote_key(&mut r, &name);
                }
            });
        }
    }
    *remote.borrow_mut() = r;
}

fn set_remote_key(r: &mut Remote, name: &str) {
    match name {
        "forward" | "up" => r.keys.forward = true,
        "backward" | "back" | "down" => r.keys.backward = true,
        "left" | "turn_left" => r.keys.turn_left = true,
        "right" | "turn_right" => r.keys.turn_right = true,
        "strafe_left" => r.keys.strafe_left = true,
        "strafe_right" => r.keys.strafe_right = true,
        "fire" => r.keys.fire = true,
        _ => {}
    }
}

fn field_f32(value: &JsValue, key: &str) -> f32 {
    js_sys::Reflect::get(value, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32
}

fn field_bool(value: &JsValue, key: &str) -> bool {
    js_sys::Reflect::get(value, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Send one observation back to the agent: tick, HUD, draw count, a frame hash,
/// and (when requested) a canvas-snapshot PNG data URL.
fn send_observation(
    remote: &Rc<RefCell<Remote>>,
    tick: u64,
    hud: &GameHud,
    outcome: &FrameOutcome,
    render: bool,
) {
    let hash = frame_hash(&outcome.instance_floats());
    let image = render.then(snapshot_data_url).flatten();
    let image_field = image
        .map(|url| format!(",\"image\":\"{url}\""))
        .unwrap_or_default();
    let json = format!(
        "{{\"tick\":{},\"hud\":{{\"hp\":{},\"score\":{},\"ammo\":{},\"enemies\":{}}},\
         \"draw_count\":{},\"state_hash\":\"{hash}\"{image_field}}}",
        tick,
        hud.health.max(0),
        hud.score,
        hud.ammo,
        hud.enemies_alive,
        outcome.draws().len(),
    );
    if render {
        remote.borrow_mut().render_once = false;
    }
    SOCKET.with(|s| {
        if let Some(ws) = s.borrow().as_ref() {
            let _ = ws.send_with_str(&json);
        }
    });
}

/// The current canvas as a PNG data URL (best effort; `None` if unavailable).
fn snapshot_data_url() -> Option<String> {
    web_sys::window()?
        .document()?
        .get_element_by_id(CANVAS_ID)?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?
        .to_data_url()
        .ok()
}

/// FNV-1a fingerprint of the packed instance floats — the same scheme the native
/// agent uses, so a frame has one stable hash.
fn frame_hash(floats: &[f32]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in floats {
        for b in f.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Movement action ids — the neutral `u32`s the retro_fps [`Keymap`] resolves to, one
/// per [`Keys`] field [`apply_movement`] toggles.
const FORWARD: u32 = 0;
const BACKWARD: u32 = 1;
const TURN_LEFT: u32 = 2;
const TURN_RIGHT: u32 = 3;
const STRAFE_LEFT: u32 = 4;
const STRAFE_RIGHT: u32 = 5;
const FIRE: u32 = 6;

/// The retro_fps movement bindings as an interface-layer [`Keymap`]. Built from
/// modifier-insensitive [`KeyBinding::key`] rows matched on the logical `key()`
/// (not `code`), so the gallery's synthetic-keyboard on-screen pad drives it too.
fn retro_fps_keymap() -> Keymap {
    Keymap::new(&[
        KeyBinding::key("ArrowUp", FORWARD),
        KeyBinding::key("w", FORWARD),
        KeyBinding::key("W", FORWARD),
        KeyBinding::key("ArrowDown", BACKWARD),
        KeyBinding::key("s", BACKWARD),
        KeyBinding::key("S", BACKWARD),
        KeyBinding::key("ArrowLeft", TURN_LEFT),
        KeyBinding::key("ArrowRight", TURN_RIGHT),
        KeyBinding::key("a", STRAFE_LEFT),
        KeyBinding::key("A", STRAFE_LEFT),
        KeyBinding::key("d", STRAFE_RIGHT),
        KeyBinding::key("D", STRAFE_RIGHT),
        KeyBinding::key(" ", FIRE),
    ])
}

/// Apply a resolved movement action to the shared key set at `pressed`.
fn apply_movement(k: &mut Keys, action: u32, pressed: bool) {
    match action {
        FORWARD => k.forward = pressed,
        BACKWARD => k.backward = pressed,
        TURN_LEFT => k.turn_left = pressed,
        TURN_RIGHT => k.turn_right = pressed,
        STRAFE_LEFT => k.strafe_left = pressed,
        STRAFE_RIGHT => k.strafe_right = pressed,
        _ => k.fire = pressed,
    }
}

/// Map a key's pressed state into the shared key set, resolving it through the
/// shared interface-layer [`Keymap`]. Matches on `key` (not `code`) so the
/// gallery's synthetic-keyboard on-screen pad drives it too. Keydown only sets
/// held state when the chord routes as a game hotkey (no meta held); keyup always
/// resolves so a key released while a modifier is down never leaves movement stuck.
fn install_key_listener(keys: &Rc<RefCell<Keys>>, event: &str, pressed: bool) {
    let keys = keys.clone();
    let keymap = retro_fps_keymap();
    let callback = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let chord = InterfaceInputEvent {
            shift: e.shift_key(),
            ctrl: e.ctrl_key(),
            alt: e.alt_key(),
            meta: e.meta_key(),
            in_text_field: false,
            console_focus: false,
        };
        let routes = !pressed || chord.routes_global_hotkey();
        let action = routes.then(|| keymap.resolve(&e.key(), chord)).flatten();
        action
            .into_iter()
            .for_each(|a| apply_movement(&mut keys.borrow_mut(), a, pressed));
        // Stop the browser from scrolling on the arrow keys / space.
        action.into_iter().for_each(|_| e.prevent_default());
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
        .expect("key listener installs");
    callback.forget();
}

/// The presentation canvas element.
fn retro_fps_canvas() -> Element {
    web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document")
        .get_element_by_id(CANVAS_ID)
        .expect("the retro_fps canvas is in the page")
}

/// Is the pointer currently locked (to our canvas)?
fn pointer_is_locked() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some()
}

/// Is this retro FPS running as one pane of the gallery's backend-comparison triptych
/// (`?embed=1`)? When embedded, the demo does NOT own the pointer lock — the
/// parent triptych holds a single lock over all three panes and fans synthetic
/// mouse-move deltas (and mouse-button presses) into each iframe. So mouse-look
/// and mouse-fire must honor those forwarded events without requiring THIS pane
/// to hold the lock itself; the parent already gates them on its own lock, so
/// stray movement can't spin the camera when you're not playing.
fn is_embedded() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .map(|s| s.contains("embed=1"))
        .unwrap_or(false)
}

/// True when the mouse should drive look/fire this frame: either this pane holds
/// the pointer lock (standalone play) or it is an embedded triptych pane driven
/// by the parent's forwarded, already-gated events.
fn mouse_input_active() -> bool {
    pointer_is_locked() || is_embedded()
}

/// Capture the pointer when the canvas is clicked (classic FPS mouse-look).
fn install_pointer_lock() {
    let canvas = retro_fps_canvas();
    let target = canvas.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |_e: MouseEvent| {
        let _ = target.request_pointer_lock();
    });
    canvas
        .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
        .expect("click listener installs");
    cb.forget();
}

/// Accumulate relative mouse movement into yaw/pitch while the pointer is locked.
/// Mouse right turns right (−yaw); mouse up looks up (+pitch).
fn install_mouse_look(look: &Rc<RefCell<Look>>) {
    let look = look.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if !mouse_input_active() {
            return;
        }
        let mut l = look.borrow_mut();
        l.yaw += -(e.movement_x() as f32) * MOUSE_SENSITIVITY;
        l.pitch += -(e.movement_y() as f32) * MOUSE_SENSITIVITY;
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref())
        .expect("mousemove listener installs");
    cb.forget();
}

/// Left mouse button fires while the pointer is locked (release always clears,
/// so a button held across an unlock can't stick). The first, lock-engaging
/// click happens before lock, so it does not fire.
fn install_mouse_fire(keys: &Rc<RefCell<Keys>>) {
    let window = web_sys::window().expect("a browser window");
    let down_keys = keys.clone();
    let down = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if e.button() == 0 && mouse_input_active() {
            down_keys.borrow_mut().fire = true;
        }
    });
    let up_keys = keys.clone();
    let up = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if e.button() == 0 {
            up_keys.borrow_mut().fire = false;
        }
    });
    window
        .add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())
        .expect("mousedown listener installs");
    window
        .add_event_listener_with_callback("mouseup", up.as_ref().unchecked_ref())
        .expect("mouseup listener installs");
    down.forget();
    up.forget();
}

/// The DOM heads-up display: a stats bar and a centre crosshair, overlaid on the
/// page. Text rendering is not an engine concern, so the HUD lives in the DOM
/// the app owns — updated each frame from the deterministic [`super::Hud`].
struct Hud {
    bar: Element,
    hint: Element,
}

impl Hud {
    fn mount() -> Hud {
        let document = web_sys::window()
            .expect("a browser window")
            .document()
            .expect("a document");

        // Anchor the overlay to the CANVAS, not the viewport: wrap the canvas in
        // a position:relative box and make the canvas fill it, so the crosshair
        // and HUD bar (position:absolute children) are centred on the canvas and
        // scroll with it — instead of being pinned to the viewport centre. The
        // wrapper also owns the responsive size (overriding the host page's
        // canvas CSS), which keeps the 960x600 (8:5) aspect undistorted.
        let canvas = document
            .get_element_by_id(CANVAS_ID)
            .expect("retro_fps canvas is in the page");
        let parent = canvas.parent_node().expect("canvas has a parent");
        let wrap = document.create_element("div").expect("create div");
        wrap.set_attribute(
            "style",
            "position:relative;display:block;width:100%;max-width:960px;\
             margin:0 auto;line-height:0;",
        )
        .expect("style wrap");
        // Put the wrapper where the canvas was, then move the canvas inside it.
        parent
            .insert_before(&wrap, Some(&canvas))
            .expect("insert wrapper");
        wrap.append_child(&canvas).expect("reparent canvas");
        canvas
            .set_attribute(
                "style",
                "display:block;width:100%;height:auto;max-width:100%;\
                 aspect-ratio:8/5;border:1px solid #2a2e36;border-radius:8px;\
                 background:#000;touch-action:none;",
            )
            .expect("style canvas");

        let bar = document.create_element("div").expect("create div");
        bar.set_attribute(
            "style",
            "position:absolute;top:8px;left:50%;transform:translateX(-50%);\
             z-index:10;pointer-events:none;font:600 15px ui-monospace,monospace;\
             color:#e8ecf2;background:rgba(10,12,16,0.65);padding:6px 14px;\
             border-radius:8px;white-space:nowrap;",
        )
        .expect("style bar");
        wrap.append_child(&bar).expect("append bar");

        let crosshair = document.create_element("div").expect("create div");
        crosshair
            .set_attribute(
                "style",
                "position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);\
                 z-index:10;pointer-events:none;font:700 22px ui-monospace,monospace;\
                 color:rgba(255,255,255,0.8);",
            )
            .expect("style crosshair");
        crosshair.set_text_content(Some("+"));
        wrap.append_child(&crosshair).expect("append crosshair");

        // A discoverability hint, hidden once the pointer is captured.
        let hint = document.create_element("div").expect("create div");
        hint.set_attribute(
            "style",
            "position:absolute;bottom:10px;left:50%;transform:translateX(-50%);\
             z-index:10;pointer-events:none;font:500 13px ui-monospace,monospace;\
             color:#cdd3dc;background:rgba(10,12,16,0.6);padding:5px 12px;\
             border-radius:8px;white-space:nowrap;",
        )
        .expect("style hint");
        hint.set_text_content(Some(
            "Click to look · WASD move · click to fire · Esc to release",
        ));
        wrap.append_child(&hint).expect("append hint");

        Hud { bar, hint }
    }

    fn update(&self, hud: &super::Hud) {
        self.bar.set_text_content(Some(&format!(
            "HP {:>3}   SCORE {:>5}   AMMO {:>3}   ENEMIES {}",
            hud.health.max(0),
            hud.score,
            hud.ammo,
            hud.enemies_alive,
        )));
        // Hide the hint once the mouse is captured; show it again when released.
        // (The `hidden` attribute hides whenever present, so toggle presence.)
        if pointer_is_locked() {
            let _ = self.hint.set_attribute("hidden", "");
        } else {
            let _ = self.hint.remove_attribute("hidden");
        }
    }
}
