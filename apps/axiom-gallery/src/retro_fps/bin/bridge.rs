//! Bridge mode for the agent bin: relay an agent's HTTP `/step` actions to a
//! live browser retro FPS (connected over a WebSocket) and return the browser's
//! observations. This is how an agent drives + watches the *rendered* game,
//! locally or — with a public `wss://` — remotely (the deployed page opens
//! `?agent=wss://…`). The browser pushes an observation every frame and applies
//! the actions it receives, so this is a real-time control loop.

use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use axiom_gallery::retro_fps::agent::Action;
use tiny_http::{Header, Method, Response, Server};
use tungstenite::Message;

/// How long an HTTP `/step` waits for the browser to apply the action and report
/// back before giving up.
const STEP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Inner {
    /// The next action JSON to deliver to the browser.
    pending: Option<String>,
    /// The most recent observation JSON from the browser.
    latest_obs: Option<String>,
    /// Bumped on every observation — lets a waiter detect a fresh frame.
    obs_gen: u64,
    /// Whether a browser is currently connected.
    connected: bool,
}

#[derive(Default)]
struct Bridge {
    inner: Mutex<Inner>,
    cv: Condvar,
}

/// Run the bridge: a WebSocket server for the browser plus an HTTP server for the
/// agent.
pub fn run(http_addr: &str, ws_addr: &str) {
    let bridge = Arc::new(Bridge::default());
    let ws_listener =
        TcpListener::bind(ws_addr).expect("bridge: failed to bind the websocket address");
    let ws_bridge = bridge.clone();
    std::thread::spawn(move || ws_accept_loop(&ws_listener, &ws_bridge));

    let server = Server::http(http_addr).expect("bridge: failed to bind the http address");
    println!("axiom-retro_fps agent BRIDGE: http://{http_addr}  (browser ws://{ws_addr})");
    println!("  open the demo with ?agent=ws://{ws_addr} then POST /step here");
    for mut req in server.incoming_requests() {
        let (status, body) = route(&bridge, &mut req);
        let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("static header is valid");
        let _ = req.respond(
            Response::from_string(body)
                .with_status_code(status)
                .with_header(header),
        );
    }
}

/// Accept browser connections (one at a time) and pump frames.
fn ws_accept_loop(listener: &TcpListener, bridge: &Arc<Bridge>) {
    for stream in listener.incoming().flatten() {
        serve_browser(stream, bridge);
    }
}

/// Serve one connected browser: read its per-frame observations, and reply with
/// the pending action whenever one is queued.
fn serve_browser(stream: TcpStream, bridge: &Arc<Bridge>) {
    let Ok(mut ws) = tungstenite::accept(stream) else {
        return;
    };
    {
        let mut inner = bridge.inner.lock().expect("bridge mutex");
        inner.connected = true;
    }
    println!("bridge: browser connected");
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let to_send = {
                    let mut inner = bridge.inner.lock().expect("bridge mutex");
                    inner.latest_obs = Some(text.to_string());
                    inner.obs_gen = inner.obs_gen.wrapping_add(1);
                    bridge.cv.notify_all();
                    inner.pending.take()
                };
                if let Some(action) = to_send {
                    if ws.send(Message::text(action)).is_err() {
                        break;
                    }
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    let mut inner = bridge.inner.lock().expect("bridge mutex");
    inner.connected = false;
    println!("bridge: browser disconnected");
}

fn route(bridge: &Arc<Bridge>, req: &mut tiny_http::Request) -> (u16, String) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/");
    match (method, path) {
        (Method::Post, "/step") => {
            let mut body = String::new();
            if std::io::Read::read_to_string(req.as_reader(), &mut body).is_err() {
                return (400, err("could not read request body"));
            }
            if body.trim().is_empty() {
                body = "{}".to_string();
            }
            match serde_json::from_str::<Action>(&body) {
                Ok(action) => step(bridge, body, action.render),
                Err(e) => (400, err(&format!("bad action json: {e}"))),
            }
        }
        (Method::Get, "/state") => {
            let inner = bridge.inner.lock().expect("bridge mutex");
            match &inner.latest_obs {
                Some(obs) => (200, obs.clone()),
                None => (200, err("no observation yet (is the browser connected?)")),
            }
        }
        _ => (404, err("not found; use POST /step or GET /state")),
    }
}

/// Queue an action for the browser and wait for the frame that reflects it.
fn step(bridge: &Arc<Bridge>, action_json: String, want_image: bool) -> (u16, String) {
    let mut inner = bridge.inner.lock().expect("bridge mutex");
    if !inner.connected {
        return (
            503,
            err("no browser connected; open the demo with ?agent=ws://<this ws addr>"),
        );
    }
    inner.pending = Some(action_json);
    // The action is sent on the browser's next frame and reflected the frame
    // after, so wait for the observation count to advance by two; if an image
    // was asked for, wait until one is present.
    let target = inner.obs_gen.wrapping_add(2);
    let deadline = std::time::Instant::now() + STEP_TIMEOUT;
    loop {
        let reflected = inner.obs_gen.wrapping_sub(target) < u64::MAX / 2;
        let has_image = inner
            .latest_obs
            .as_deref()
            .is_some_and(|o| o.contains("\"image\""));
        if reflected && (!want_image || has_image) {
            return (200, inner.latest_obs.clone().unwrap_or_else(|| "{}".into()));
        }
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return (
                200,
                inner.latest_obs.clone().unwrap_or_else(|| {
                    err("timed out waiting for the browser; returning no frame")
                }),
            );
        };
        let (next, timed_out) = bridge
            .cv
            .wait_timeout(inner, remaining)
            .expect("bridge condvar");
        inner = next;
        if timed_out.timed_out() {
            return (200, inner.latest_obs.clone().unwrap_or_else(|| "{}".into()));
        }
    }
}

fn err(msg: &str) -> String {
    let quoted = serde_json::to_string(msg).unwrap_or_else(|_| "\"error\"".to_string());
    format!("{{\"error\":{quoted}}}")
}
