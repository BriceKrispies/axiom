//! The live `wasm32` arm: WebSocket transport, keyboard input, a public-key
//! handshake, and the netcode-gated render loop. Never compiled on native (the
//! deterministic core and its tests live in `lib.rs`); this is the thin
//! nondeterministic edge.
//!
//! ## Trust model (and its honest boundary)
//!
//! Every input/beacon is signed by its author's key (see `axiom-netcode`), and a
//! peer admits a frame only if it verifies against that peer's key in the roster.
//! So a **compromised client** cannot forge the other player's inputs (it lacks
//! their private key), and flood/out-of-window traffic is dropped — the engine
//! property proven natively in `axiom-netcode`.
//!
//! The roster here is exchanged over the **untrusted relay** on a trust-on-first-
//! use basis: each browser mints its own keypair (the private key never leaves
//! the tab) and sends its public key once both players are present. That fully
//! defeats a compromised *client*. It does **not** defeat a relay that is
//! malicious *at the moment of key exchange* (it could MITM the public keys);
//! closing that needs an out-of-band fingerprint check, which is out of scope for
//! this demo.

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::PlayerInput;
use axiom_crypto::{SigningKey, VerifyingKey};
use axiom_netcode::NetcodeApi;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, KeyboardEvent, MessageEvent, WebSocket};

use super::{build_netplay_app, encode_delta, input_for, Keys, CANVAS_ID, MOVE_KIND};

/// The local-dev relay address (see `tools/axiom-netcode-relay`), used when the
/// page does not override it. The deployed gallery page has no relay of its own,
/// so it points the demo at a hosted relay via a `?relay=<url>` query param —
/// see [`relay_url`].
const RELAY_URL: &str = "ws://127.0.0.1:9001";

/// The relay address to connect to: a `?relay=<url>` query param if the page
/// supplies one (so a static deploy can target a hosted `wss://` relay), else
/// the local-dev [`RELAY_URL`] default.
fn relay_url() -> String {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|search| relay_from_query(&search))
        .unwrap_or_else(|| RELAY_URL.to_string())
}

/// Extract and URL-decode the `relay` parameter from a `location.search` string
/// (e.g. `"?relay=wss%3A%2F%2Fhost%3A443"`). Returns `None` when the parameter
/// is absent or decodes to empty.
fn relay_from_query(search: &str) -> Option<String> {
    let query = search.strip_prefix('?').unwrap_or(search);
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("relay="))
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| js_sys::decode_uri_component(raw).ok())
        .map(String::from)
        .filter(|url| !url.is_empty())
}

/// How many ticks a peer may run ahead of confirmation before it stops
/// submitting and waits — bounds the input backlog if one tab is faster.
const MAX_AHEAD: u64 = 6;

/// App-level frame tags multiplexed over the one binary channel the relay
/// forwards: a netcode input/beacon, or a public-key handshake frame.
const FRAME_INPUT: u8 = 1;
const FRAME_PUBKEY: u8 = 2;

/// Shared, single-threaded browser state the callbacks and the frame loop touch.
struct Shared {
    keys: Keys,
    inbound: Vec<Vec<u8>>,
    peer_id: Option<u64>,
    local_key: Option<SigningKey>,
    local_pubkey: [u8; VerifyingKey::LEN],
    peer_key: Option<VerifyingKey>,
    go: bool,
    started: bool,
}

/// Log a line to the browser console (visible in devtools), prefixed so the
/// netplay flow is easy to spot.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[netplay] {msg}")));
}

/// The browser entry: mint this peer's keypair, open the relay socket, capture
/// the keyboard, and — once both players are present and have exchanged public
/// keys — start the engine loop.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    let relay = relay_url();
    log(&format!("start(): connecting to relay {relay}"));

    // Mint this browser's signing keypair from the browser CSPRNG. The private
    // key never leaves this tab; only the public key is published at handshake.
    let mut seed = [0u8; SigningKey::SEED_LEN];
    getrandom::getrandom(&mut seed).expect("browser CSPRNG is available");
    let local_key = SigningKey::from_seed(seed);
    let local_pubkey = local_key.verifying_key().to_bytes();

    let shared = Rc::new(RefCell::new(Shared {
        keys: Keys::default(),
        inbound: Vec::new(),
        peer_id: None,
        local_key: Some(local_key),
        local_pubkey,
        peer_key: None,
        go: false,
        started: false,
    }));

    install_key_listener(&shared, "keydown", true);
    install_key_listener(&shared, "keyup", false);

    let ws = WebSocket::new(&relay).expect("relay websocket opens");
    ws.set_binary_type(BinaryType::Arraybuffer);

    // Surface the socket lifecycle so a missing/closed relay is obvious.
    let onopen = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| log("websocket open"));
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();
    let onerror = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        log("websocket ERROR (no relay reachable — run one locally with `make relay`, or point the page at a hosted one via ?relay=wss://host:port)")
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
                // The relay assigns this peer its id; we still wait for `go`.
                shared_cb.borrow_mut().peer_id = Some(peer_id);
                log(&format!(
                    "assigned peer id {peer_id} — waiting for the second player"
                ));
            } else if text == "go" {
                // Both players are present (and both subscribed to the relay), so
                // it is safe to broadcast our public key now.
                shared_cb.borrow_mut().go = true;
                send_pubkey(&shared_cb, &ws_cb);
                log("go received — exchanging public keys");
                try_launch(&shared_cb, &ws_cb);
            } else if text == "full" {
                log("relay is full (a game with two players is already running)");
            }
        } else if let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            match bytes.split_first() {
                Some((&FRAME_PUBKEY, rest)) => receive_pubkey(&shared_cb, &ws_cb, rest),
                Some((&FRAME_INPUT, rest)) => shared_cb.borrow_mut().inbound.push(rest.to_vec()),
                _ => {}
            }
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
}

/// Broadcast this peer's public key (tagged) so the other peer can pin it.
fn send_pubkey(shared: &Rc<RefCell<Shared>>, ws: &WebSocket) {
    let mut framed = Vec::with_capacity(1 + VerifyingKey::LEN);
    framed.push(FRAME_PUBKEY);
    framed.extend_from_slice(&shared.borrow().local_pubkey);
    let _ = ws.send_with_u8_array(&framed);
}

/// Pin the peer's public key from a handshake frame, then try to launch.
fn receive_pubkey(shared: &Rc<RefCell<Shared>>, ws: &WebSocket, key_bytes: &[u8]) {
    match <[u8; VerifyingKey::LEN]>::try_from(key_bytes) {
        Ok(arr) => match VerifyingKey::try_from_bytes(&arr) {
            Some(vk) => {
                shared.borrow_mut().peer_key = Some(vk);
                log("received the other player's public key");
                try_launch(shared, ws);
            }
            None => log("peer sent an invalid public key"),
        },
        Err(_) => log("peer sent a wrong-length public key"),
    }
}

/// Start the engine loop once we have our id, the relay's `go`, and the peer's
/// public key — building the roster from both keys. Idempotent: only the first
/// fully-satisfied call launches.
fn try_launch(shared: &Rc<RefCell<Shared>>, ws: &WebSocket) {
    let launch = {
        let mut s = shared.borrow_mut();
        match (s.go, s.started, s.peer_id, s.local_key.clone(), s.peer_key) {
            (true, false, Some(id), Some(local), Some(peer)) => {
                s.started = true;
                Some((id, local, peer))
            }
            _ => None,
        }
    };
    if let Some((peer_id, local_key, peer_key)) = launch {
        log("both players present and keys exchanged — starting");
        run_loop(peer_id, local_key, peer_key, shared.clone(), ws.clone());
    }
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

/// Build this peer's engine + signed lockstep session and drive the windowing
/// loop with a netcode-gated frame closure: ingest peers' inputs, submit ours
/// (signed), confirm the ready ticks, step the real engine, and present.
fn run_loop(
    peer_id: u64,
    local_key: SigningKey,
    peer_key: VerifyingKey,
    shared: Rc<RefCell<Shared>>,
    ws: WebSocket,
) {
    let mut running = build_netplay_app();
    let (vertices, indices) = running.mesh_vertex_stream();
    let max_instances = running.renderable_count() as u32;

    // The roster: this peer's own key plus the peer's, keyed by the fixed ids.
    let other_id = if peer_id == 1 { 2 } else { 1 };
    let roster = [(peer_id, local_key.verifying_key()), (other_id, peer_key)];
    let mut net = NetcodeApi::new(peer_id, local_key, &roster);
    let mut submitted: u64 = 0;
    let mut latest: ([f32; 4], Vec<f32>, u32) = ([0.0, 0.0, 0.0, 1.0], Vec::new(), 0);

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(800, 600)
        .expect("surface dimensions are valid");

    log(&format!(
        "loop launching for peer {peer_id} (renderables={max_instances}); waiting on the WebGPU device"
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

            // 2. Submit this peer's signed input for the next tick, with a bounded
            //    lead so a fast tab cannot run unboundedly ahead of a slow one.
            if submitted.saturating_sub(net.confirmed_tick()) < MAX_AHEAD {
                let delta = shared.borrow().keys.delta();
                let bytes = net.submit_local(MOVE_KIND, &encode_delta(delta));
                let mut framed = Vec::with_capacity(bytes.len() + 1);
                framed.push(FRAME_INPUT);
                framed.extend_from_slice(&bytes);
                let _ = ws.send_with_u8_array(&framed);
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

            // Diagnostics: the first confirmed tick proves the second player's
            // (validly signed) inputs are arriving.
            let confirmed = net.confirmed_tick();
            if !confirmed_logged && confirmed > 0 {
                confirmed_logged = true;
                log("first tick CONFIRMED — both players present, signed inputs flowing");
            }
            if frames == 1 || frames.is_multiple_of(180) {
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
