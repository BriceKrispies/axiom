//! The live `wasm32` arm: WebSocket transport, keyboard input, and the
//! netcode-gated render loop. Never compiled on native (the deterministic core
//! and its tests live in `lib.rs`); this is the thin nondeterministic edge.

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::PlayerInput;
use axiom_netcode::NetcodeApi;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, KeyboardEvent, MessageEvent, WebSocket};

use super::{build_netplay_app, encode_delta, input_for, Keys, CANVAS_ID, MOVE_KIND};

/// The relay address (see `tools/axiom-netcode-relay`).
const RELAY_URL: &str = "ws://127.0.0.1:9001";

/// How many ticks a peer may run ahead of confirmation before it stops
/// submitting and waits — bounds the input backlog if one tab is faster.
const MAX_AHEAD: u64 = 6;

/// Shared, single-threaded browser state the callbacks and the frame loop touch.
struct Shared {
    keys: Keys,
    inbound: Vec<Vec<u8>>,
    peer_id: Option<u64>,
    started: bool,
}

/// Log a line to the browser console (visible in devtools), prefixed so the
/// netplay flow is easy to spot.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[netplay] {msg}")));
}

/// The browser entry: open the relay socket, capture the keyboard, and — once
/// the relay assigns this peer its id — start the engine loop.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    log(&format!("start(): connecting to relay {RELAY_URL}"));

    let shared = Rc::new(RefCell::new(Shared {
        keys: Keys::default(),
        inbound: Vec::new(),
        peer_id: None,
        started: false,
    }));

    install_key_listener(&shared, "keydown", true);
    install_key_listener(&shared, "keyup", false);

    let ws = WebSocket::new(RELAY_URL).expect("relay websocket opens");
    ws.set_binary_type(BinaryType::Arraybuffer);

    // Surface the socket lifecycle so a missing/closed relay is obvious.
    let onopen = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| log("websocket open"));
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();
    let onerror = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        log("websocket ERROR (is `make relay` running on ws://127.0.0.1:9001?)")
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();
    let onclose = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| log("websocket closed"));
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    let shared_cb = shared.clone();
    let ws_cb = ws.clone();
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        if let Some(text) = event.data().as_string() {
            let text = text.trim();
            if let Ok(peer_id) = text.parse::<u64>() {
                // The relay assigns this peer its id, but we wait for `go`.
                shared_cb.borrow_mut().peer_id = Some(peer_id);
                log(&format!(
                    "assigned peer id {peer_id} — waiting for the second player"
                ));
            } else if text == "go" {
                // Both players are present: start together (no early-input loss).
                let launch = {
                    let mut s = shared_cb.borrow_mut();
                    match (s.started, s.peer_id) {
                        (false, Some(id)) => {
                            s.started = true;
                            Some(id)
                        }
                        _ => None,
                    }
                };
                if let Some(peer_id) = launch {
                    log("go received — both players present, starting");
                    run_loop(peer_id, shared_cb.clone(), ws_cb.clone());
                }
            } else if text == "full" {
                log("relay is full (a game with two players is already running)");
            }
        } else if let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
            // Every other frame is netcode wire bytes from a peer.
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            shared_cb.borrow_mut().inbound.push(bytes);
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
}

/// Track an arrow key's pressed state into the shared key set.
fn install_key_listener(shared: &Rc<RefCell<Shared>>, event: &str, pressed: bool) {
    let shared = shared.clone();
    let callback = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut s = shared.borrow_mut();
        match e.key().as_str() {
            "ArrowLeft" => s.keys.left = pressed,
            "ArrowRight" => s.keys.right = pressed,
            "ArrowUp" => s.keys.up = pressed,
            "ArrowDown" => s.keys.down = pressed,
            _ => {}
        }
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
        .expect("key listener installs");
    callback.forget();
}

/// Build this peer's engine + lockstep session and drive the windowing loop with
/// a netcode-gated frame closure: ingest peers' inputs, submit ours, confirm the
/// ready ticks, step the real engine, and present the latest frame.
fn run_loop(peer_id: u64, shared: Rc<RefCell<Shared>>, ws: WebSocket) {
    let mut running = build_netplay_app();
    let (vertices, indices) = running.mesh_vertex_stream();
    let max_instances = running.renderable_count() as u32;

    let mut net = NetcodeApi::new(peer_id, &[1, 2]);
    let mut submitted: u64 = 0;
    let mut latest: ([f32; 4], Vec<f32>, u32) = ([0.0, 0.0, 0.0, 1.0], Vec::new(), 0);

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(800, 600)
        .expect("surface dimensions are valid");

    log(&format!(
        "loop launching for peer {peer_id} (renderables={max_instances}); \
         waiting on the WebGPU device + the second player"
    ));
    let mut frames: u64 = 0;
    let mut inbound_total: u64 = 0;
    let mut confirmed_logged = false;

    let _ = windowing.run_web(
        CANVAS_ID,
        vertices,
        indices,
        max_instances,
        move |_raf_tick| {
            frames += 1;

            // 1. Fold in everything the relay delivered since the last frame.
            let delivered = std::mem::take(&mut shared.borrow_mut().inbound);
            inbound_total += delivered.len() as u64;
            for message in delivered {
                let _ = net.ingest(&message);
            }

            // 2. Submit this peer's input for the next tick, with a bounded lead so a
            //    fast tab cannot run unboundedly ahead of a slow one.
            if submitted.saturating_sub(net.confirmed_tick()) < MAX_AHEAD {
                let delta = shared.borrow().keys.delta();
                let bytes = net.submit_local(MOVE_KIND, &encode_delta(delta));
                let _ = ws.send_with_u8_array(&bytes);
                submitted += 1;
            }

            // 3. Step the real engine for every tick whose inputs are all present.
            while let Some(tick) = net.ready_tick() {
                let inputs: Vec<PlayerInput> = net
                    .confirm_tick(tick)
                    .iter()
                    .map(|(peer, _kind, payload)| input_for(*peer, payload))
                    .collect();
                let outcome = running.tick_with(tick, &inputs);
                latest = (
                    outcome.clear_color(),
                    outcome.instance_floats(),
                    outcome.draws().len() as u32,
                );
            }

            // Diagnostics: the first frame proves the WebGPU device came up; the
            // first confirmed tick proves the second player's inputs are arriving.
            let confirmed = net.confirmed_tick();
            if !confirmed_logged && confirmed > 0 {
                confirmed_logged = true;
                log("first tick CONFIRMED — both players present, inputs flowing, rendering");
            }
            if frames == 1 || frames % 180 == 0 {
                log(&format!(
                    "frame {frames}: submitted={submitted} confirmed={confirmed} \
                     inbound_total={inbound_total} draws={}",
                    latest.2
                ));
            }

            (latest.0, latest.1.clone(), latest.2)
        },
    );
}
