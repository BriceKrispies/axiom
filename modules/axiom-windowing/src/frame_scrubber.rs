//! The shared **frame scrubber** overlay every browser game gets for free.
//!
//! The web run loop ([`crate::WindowingApi::run_web_multi`] /
//! [`crate::WindowingApi::run_web_streaming`]) is the single chokepoint every
//! live browser game funnels through (directly, via `run_web`, or via
//! `App::run`). Mounting the scrubber there means a uniform dev overlay — pinned
//! to the bottom of the page — appears on *all* game screens without any
//! per-app wiring.
//!
//! Each live frame it records the exact data handed to the backend's `present`
//! (clear colour, lights, light view-projection, draw batches) into an
//! `axiom-recording` timeline as opaque bytes. When the user scrubs (Back / Fwd /
//! Live), the run loop stops calling the app's frame closure — freezing the live
//! sim — and re-presents the recorded frame instead. The timeline is never
//! mutated and no frame is forked; this is purely a read-only view over already
//! presented frames.
//!
//! This is wasm32-only platform-edge code (it owns DOM nodes), like the rest of
//! the live run loop — it never enters the deterministic native core.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use axiom_recording::RecordingApi;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent, MouseEvent};

/// One directional light as the run loop carries it: `(index, dir, colour, intensity)`.
type Light = (u32, [f32; 3], [f32; 3], f32);
/// One draw batch: `(mesh_id, material_id, instance_floats, instance_count)`.
type Batch = (u64, u64, Vec<f32>, u32);
/// The full per-frame argument set handed to the backend's `present`.
pub(crate) type PresentArgs = ([f32; 4], Vec<Light>, [f32; 16], Vec<Batch>);

/// Produces the app's serialized sim state for the current frame (recorded as the
/// frame's opaque `state_bytes`). Supplied by a forkable run-loop entry.
pub(crate) type SnapshotHook = Rc<dyn Fn() -> Vec<u8>>;
/// Restores the app's sim state from a recorded frame's `state_bytes` (the fork).
pub(crate) type RestoreHook = Rc<dyn Fn(&[u8])>;

/// The shared frame scrubber: a recorder, its DOM status line, the
/// reverse-playback flag, and — when the run loop is *forkable* — the app's
/// snapshot/restore hooks. When `reverse` is set the run loop walks the selection
/// one frame older every tick (auto-rewind) until it reaches the oldest frame.
/// When `restore` is present, a `⏏ fork` button restores the selected frame's
/// recorded state into the live app and resumes play from it.
pub(crate) struct FrameScrubber {
    recorder: Rc<RefCell<RecordingApi>>,
    status: Element,
    reverse: Rc<Cell<bool>>,
    snapshot: Option<SnapshotHook>,
    /// Whether new live frames are being recorded. Cleared when the game loses
    /// focus (Escape / window blur / tab hidden) and set again on return, so the
    /// timeline only grows while you're actually playing.
    active: Rc<Cell<bool>>,
}

impl FrameScrubber {
    /// Build the recorder (browser-safe budget) and mount the overlay pinned to
    /// the bottom of the page. `snapshot`/`restore` are the app's fork hooks: when
    /// both are present the overlay records sim state each frame and grows a
    /// `⏏ fork` button. Pass `None`/`None` for scrub-only games. Returns `None` if
    /// there is no DOM (then the run loop simply records nothing and presents live).
    pub(crate) fn mount(
        snapshot: Option<SnapshotHook>,
        restore: Option<RestoreHook>,
    ) -> Option<FrameScrubber> {
        let recorder = Rc::new(RefCell::new(RecordingApi::browser_safe().ok()?));
        let reverse = Rc::new(Cell::new(false));
        let active = Rc::new(Cell::new(true));
        let window = web_sys::window()?;
        let document = window.document()?;
        let body = document.body()?;

        let panel = document.create_element("div").ok()?;
        panel
            .set_attribute(
                "style",
                "position:fixed;left:0;right:0;bottom:0;z-index:2147483600;\
                 display:flex;gap:8px;align-items:center;justify-content:center;\
                 flex-wrap:wrap;padding:6px 10px;\
                 font:600 12px ui-monospace,monospace;color:#e8ecf2;\
                 background:rgba(10,12,16,0.82);border-top:1px solid #2a2e36;\
                 -webkit-user-select:none;user-select:none;",
            )
            .ok()?;

        let status = document.create_element("span").ok()?;
        status.set_attribute("style", "white-space:nowrap;").ok()?;
        panel.append_child(&status).ok()?;

        // Reverse playback: enter scrub at the newest frame (if live) and arm the
        // auto-rewind flag; the run loop walks one frame older each tick.
        add_button(&document, &panel, &recorder, &reverse, &active, &status, "◀◀ rev", |r, rev| {
            r.latest_frame_index()
                .into_iter()
                .for_each(|latest| {
                    let _ = r.enter_scrub(latest.raw());
                });
            rev.set(true);
        });
        // Manual controls cancel reverse playback.
        add_button(&document, &panel, &recorder, &reverse, &active, &status, "◀ back", |r, rev| {
            rev.set(false);
            let _ = r.step_back();
        });
        add_button(&document, &panel, &recorder, &reverse, &active, &status, "fwd ▶", |r, rev| {
            rev.set(false);
            let _ = r.step_forward();
        });
        add_button(&document, &panel, &recorder, &reverse, &active, &status, "▶ live", |r, rev| {
            rev.set(false);
            r.resume();
        });
        // Fork: only when the run loop is forkable (the app supplied a restore
        // hook). Restores the selected frame's recorded state into the live app
        // and resumes play from it — a new timeline branch.
        restore.into_iter().for_each(|restore| {
            add_fork_button(&document, &panel, &recorder, &reverse, &active, &status, restore);
        });

        body.append_child(&panel).ok()?;

        // Pause recording when the game loses focus (Escape / window blur / tab
        // hidden); resume on return (focus / visible / a click back in). This
        // keeps the timeline from filling with idle frames when you step away.
        install_focus_listeners(&window, &document, &recorder, &reverse, &active, &status);

        let scrubber = FrameScrubber {
            recorder,
            status,
            reverse,
            snapshot,
            active,
        };
        scrubber.refresh_status();
        Some(scrubber)
    }

    /// Whether the run loop should keep stepping the app (live) versus freezing
    /// it and re-presenting a recorded frame (scrubbing).
    pub(crate) fn is_live(&self) -> bool {
        self.recorder.borrow().is_live()
    }

    /// Whether the game is focused/running. Cleared on focus loss (Escape /
    /// window blur / tab hidden) and set again on return; while false the run
    /// loop freezes the game entirely (no tick, no app step, no present).
    pub(crate) fn is_active(&self) -> bool {
        self.active.get()
    }

    /// Record one live frame's present arguments (encoded as the opaque render
    /// artifact). Recording errors (e.g. an over-budget frame) are non-fatal.
    pub(crate) fn record(
        &self,
        frame: u64,
        clear: [f32; 4],
        lights: &[Light],
        light_vp: [f32; 16],
        batches: &[Batch],
    ) {
        // Skip recording while paused (game unfocused): the frame still presents
        // live, but the timeline does not grow. `then` keeps this branchless.
        self.active.get().then(|| {
            let render_bytes = encode(clear, lights, light_vp, batches);
            // Capture the app's sim state for this frame (empty when not forkable);
            // it rides in the recorder's `state_bytes` slot for a later fork.
            let state_bytes = self.snapshot.as_ref().map(|snap| snap()).unwrap_or_default();
            let _ = self.recorder.borrow_mut().record_frame(
                frame,
                frame,
                Vec::new(),
                Vec::new(),
                state_bytes,
                render_bytes,
            );
        });
        self.refresh_status();
    }

    /// The frame to present this scrub tick: first advance auto-rewind (if
    /// armed), then return the selected recorded frame's present arguments.
    pub(crate) fn scrub_frame(&self) -> Option<PresentArgs> {
        self.tick_reverse();
        self.selected_present()
    }

    /// If reverse playback is armed, step the selection one frame older. Reaching
    /// the oldest retained frame stops playback there (no wrap-around).
    fn tick_reverse(&self) {
        let armed = self.reverse.get();
        // `then` keeps this a no-op when not armed; `step_back` Errs at the oldest
        // edge, which disarms playback so it rests on the oldest frame.
        let stepped = armed
            .then(|| self.recorder.borrow_mut().step_back())
            .unwrap_or(Ok(()));
        self.reverse.set(armed & stepped.is_ok());
        armed.then(|| self.refresh_status());
    }

    /// The present arguments of the selected recorded frame to re-present;
    /// `None` if the frame was evicted or the payload is unreadable.
    fn selected_present(&self) -> Option<PresentArgs> {
        let recorder = self.recorder.borrow();
        let selected = recorder.selected_frame()?;
        let capture = recorder.frame(selected).ok()?;
        decode(capture.render_bytes())
    }

    /// Refresh the status line from the recorder's current state.
    fn refresh_status(&self) {
        refresh_status_into(
            &self.status,
            &self.recorder.borrow(),
            self.reverse.get(),
            self.active.get(),
        );
    }
}

/// Append a labelled button that runs `action` (over the shared recorder and the
/// reverse-playback flag), then refreshes the status line.
fn add_button(
    document: &web_sys::Document,
    panel: &Element,
    recorder: &Rc<RefCell<RecordingApi>>,
    reverse: &Rc<Cell<bool>>,
    active: &Rc<Cell<bool>>,
    status: &Element,
    label: &str,
    action: fn(&mut RecordingApi, &Cell<bool>),
) {
    let button = match document.create_element("button") {
        Ok(b) => b,
        Err(_) => return,
    };
    button.set_text_content(Some(label));
    let _ = button.set_attribute(
        "style",
        "font:600 12px ui-monospace,monospace;color:#e8ecf2;\
         background:#1b1f27;border:1px solid #3a3f49;border-radius:6px;\
         padding:3px 9px;cursor:pointer;",
    );
    let r = recorder.clone();
    let rev = reverse.clone();
    let act = active.clone();
    let s = status.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |_e: MouseEvent| {
        action(&mut r.borrow_mut(), &rev);
        refresh_status_into(&s, &r.borrow(), rev.get(), act.get());
    });
    let _ = button.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
    cb.forget();
    let _ = panel.append_child(&button);
}

/// Append the `⏏ fork` button: restore the selected frame's recorded sim state
/// into the live app via `restore`, then resume live play from it (a new branch).
fn add_fork_button(
    document: &web_sys::Document,
    panel: &Element,
    recorder: &Rc<RefCell<RecordingApi>>,
    reverse: &Rc<Cell<bool>>,
    active: &Rc<Cell<bool>>,
    status: &Element,
    restore: RestoreHook,
) {
    let button = match document.create_element("button") {
        Ok(b) => b,
        Err(_) => return,
    };
    button.set_text_content(Some("⏏ fork"));
    let _ = button.set_attribute(
        "style",
        "font:600 12px ui-monospace,monospace;color:#0b0d11;\
         background:#7fd6a0;border:1px solid #5fae80;border-radius:6px;\
         padding:3px 9px;cursor:pointer;",
    );
    let r = recorder.clone();
    let rev = reverse.clone();
    let act = active.clone();
    let s = status.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |_e: MouseEvent| {
        // Read the selected frame's recorded sim-state bytes.
        let bytes = {
            let rec = r.borrow();
            rec.selected_frame()
                .and_then(|frame| rec.frame(frame).ok())
                .map(|capture| capture.state_bytes().to_vec())
        };
        // Restore into the live app, cancel any reverse playback, re-enable
        // recording, and resume live play from the forked state. With no
        // selection/bytes this is a no-op.
        bytes.into_iter().for_each(|bytes| {
            restore(&bytes);
            rev.set(false);
            act.set(true);
            r.borrow_mut().resume();
        });
        refresh_status_into(&s, &r.borrow(), rev.get(), act.get());
    });
    let _ = button.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
    cb.forget();
    let _ = panel.append_child(&button);
}

/// Wire the focus/visibility listeners that gate recording. Recording **pauses**
/// when the game loses focus — Escape, window blur, or the tab being hidden — and
/// **resumes** on return: window focus, the tab becoming visible, or a click back
/// into the page (which also covers re-engaging an FPS pointer-lock). Each handler
/// refreshes the status line so `LIVE`/`PAUSED` is always current. The handlers do
/// not consume the events, so each game's own input handling is unaffected.
fn install_focus_listeners(
    window: &web_sys::Window,
    document: &web_sys::Document,
    recorder: &Rc<RefCell<RecordingApi>>,
    reverse: &Rc<Cell<bool>>,
    active: &Rc<Cell<bool>>,
    status: &Element,
) {
    // A shared status refresh bound to the current state.
    let refresh: Rc<dyn Fn()> = {
        let recorder = recorder.clone();
        let reverse = reverse.clone();
        let active = active.clone();
        let status = status.clone();
        Rc::new(move || {
            refresh_status_into(&status, &recorder.borrow(), reverse.get(), active.get());
        })
    };

    // Escape pauses recording (the key is observed, not consumed).
    let act = active.clone();
    let on_refresh = refresh.clone();
    let on_key = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        (e.key() == "Escape").then(|| {
            act.set(false);
            on_refresh();
        });
    });
    let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    // Window blur pauses; focus resumes; a click back in resumes.
    add_toggle(window, "blur", active, false, &refresh);
    add_toggle(window, "focus", active, true, &refresh);
    add_toggle(document, "pointerdown", active, true, &refresh);

    // Tab visibility: hidden pauses, visible resumes.
    let act = active.clone();
    let doc = document.clone();
    let on_refresh = refresh.clone();
    let on_visibility = Closure::<dyn FnMut()>::new(move || {
        act.set(!doc.hidden());
        on_refresh();
    });
    let _ = document
        .add_event_listener_with_callback("visibilitychange", on_visibility.as_ref().unchecked_ref());
    on_visibility.forget();
}

/// Add a listener that sets `active` to a fixed `value` on `event`, then refreshes.
fn add_toggle<T: AsRef<web_sys::EventTarget>>(
    target: &T,
    event: &str,
    active: &Rc<Cell<bool>>,
    value: bool,
    refresh: &Rc<dyn Fn()>,
) {
    let act = active.clone();
    let refresh = refresh.clone();
    let cb = Closure::<dyn FnMut()>::new(move || {
        act.set(value);
        refresh();
    });
    let _ = target
        .as_ref()
        .add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Render the recorder's state into the status element: mode, frame count,
/// retained range, memory used vs. budget, and the focused frame's hash.
fn refresh_status_into(status: &Element, recorder: &RecordingApi, reverse: bool, active: bool) {
    let live = recorder.is_live();
    // While scrubbing, distinguish armed reverse playback ("◀◀ REV") from a held
    // single-frame selection ("SCRUB").
    let label = if reverse { "◀◀ REV" } else { "SCRUB" };
    // While live, show PAUSED when recording is suspended (game unfocused).
    let live_label = if active { "LIVE" } else { "PAUSED" };
    let mode = if live {
        live_label.to_string()
    } else {
        recorder
            .selected_frame()
            .map(|f| format!("{label} @ {}", f.raw()))
            .unwrap_or_else(|| label.to_string())
    };
    let oldest = recorder
        .oldest_frame_index()
        .map(|f| f.raw().to_string())
        .unwrap_or_else(|_| "-".to_string());
    let latest = recorder
        .latest_frame_index()
        .map(|f| f.raw().to_string())
        .unwrap_or_else(|_| "-".to_string());
    let focus = if live {
        recorder.latest_frame_index().ok()
    } else {
        recorder.selected_frame()
    };
    let hash = focus
        .and_then(|f| recorder.frame(f).ok().map(|c| c.final_hash()))
        .map(|h| format!("{h:016x}"))
        .unwrap_or_else(|| "-".repeat(16));
    status.set_text_content(Some(&format!(
        "rec {mode} · frames {} · range {oldest}–{latest} · mem {} / {} KiB · hash {hash}",
        recorder.frame_count(),
        recorder.current_bytes() / 1024,
        recorder.max_bytes() / 1024,
    )));
}

// ----- present-args (de)serialization: opaque to the recorder, symmetric here -----

/// Encode the present arguments into the recorder's opaque render bytes.
fn encode(clear: [f32; 4], lights: &[Light], light_vp: [f32; 16], batches: &[Batch]) -> Vec<u8> {
    let mut bytes = Vec::new();
    clear.iter().for_each(|f| put_f32(&mut bytes, *f));
    light_vp.iter().for_each(|f| put_f32(&mut bytes, *f));
    put_u32(&mut bytes, lights.len() as u32);
    lights.iter().for_each(|(index, dir, colour, intensity)| {
        put_u32(&mut bytes, *index);
        dir.iter().for_each(|f| put_f32(&mut bytes, *f));
        colour.iter().for_each(|f| put_f32(&mut bytes, *f));
        put_f32(&mut bytes, *intensity);
    });
    put_u32(&mut bytes, batches.len() as u32);
    batches.iter().for_each(|(mesh, material, instances, count)| {
        put_u64(&mut bytes, *mesh);
        put_u64(&mut bytes, *material);
        put_u32(&mut bytes, *count);
        put_u32(&mut bytes, instances.len() as u32);
        instances.iter().for_each(|f| put_f32(&mut bytes, *f));
    });
    bytes
}

/// Decode present arguments produced by [`encode`]. Returns `None` on a
/// truncated/unreadable payload (never produced by `encode`), so the caller can
/// safely fall back to presenting nothing.
fn decode(bytes: &[u8]) -> Option<PresentArgs> {
    let mut cur = Cursor { bytes, at: 0 };
    let clear = [cur.f32()?, cur.f32()?, cur.f32()?, cur.f32()?];
    let mut light_vp = [0.0_f32; 16];
    for slot in light_vp.iter_mut() {
        *slot = cur.f32()?;
    }
    let light_count = cur.u32()? as usize;
    let mut lights = Vec::with_capacity(light_count);
    for _ in 0..light_count {
        let index = cur.u32()?;
        let dir = [cur.f32()?, cur.f32()?, cur.f32()?];
        let colour = [cur.f32()?, cur.f32()?, cur.f32()?];
        lights.push((index, dir, colour, cur.f32()?));
    }
    let batch_count = cur.u32()? as usize;
    let mut batches = Vec::with_capacity(batch_count);
    for _ in 0..batch_count {
        let mesh = cur.u64()?;
        let material = cur.u64()?;
        let count = cur.u32()?;
        let instance_len = cur.u32()? as usize;
        let mut instances = Vec::with_capacity(instance_len);
        for _ in 0..instance_len {
            instances.push(cur.f32()?);
        }
        batches.push((mesh, material, instances, count));
    }
    Some((clear, lights, light_vp, batches))
}

fn put_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// A tiny bounds-checked little-endian reader over the recorded payload.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let end = self.at + N;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        slice.try_into().ok()
    }
    fn f32(&mut self) -> Option<f32> {
        self.take::<4>().map(f32::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take::<8>().map(u64::from_le_bytes)
    }
}
