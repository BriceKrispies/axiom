//! `axiom-doom-browser` agent bridge — an HTTP server that drives the real DOOM
//! game from JSON so an external agent can send inputs and read back structured
//! state (and, with `agent-render`, images). Native + `agent` feature only
//! (`required-features`), so it never enters the wasm build or the default gates.
//!
//! Drive it with `curl`:
//!   curl -s -XPOST localhost:7878/step -d '{"keys":["forward"],"fire":true}'
//!   curl -s -XPOST localhost:7878/reset
//!   curl -s localhost:7878/state

use axiom_gallery::doom::agent::{Action, AgentSession, Observation};
use axiom_gallery::doom::perception::DoomPerceiver;
use tiny_http::{Header, Method, Response, Server};

// The offscreen renderer lives in the bin (not the lib) so wgpu's symbols never
// enter the crate's cdylib. Only compiled with the `agent-render` feature.
#[cfg(feature = "doom-agent-render")]
mod render;

// Browser bridge mode (relay actions to a live browser, return its frames).
mod bridge;

const DEFAULT_ADDR: &str = "127.0.0.1:7878";
const DEFAULT_WS_ADDR: &str = "127.0.0.1:7879";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `--perceive [ticks]`: run the live perception demo — the agent sees walls
    // and tracks moving enemies, printing what it perceives each tick. No server.
    if args.iter().any(|a| a == "--perceive") {
        let ticks = args
            .iter()
            .skip_while(|a| *a != "--perceive")
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(180);
        run_perception_demo(ticks);
        return;
    }

    // `--bridge [http_addr] [ws_addr]`: drive a live browser instead of an
    // in-process headless game.
    if args.iter().any(|a| a == "--bridge") {
        let rest: Vec<&String> = args
            .iter()
            .skip_while(|a| *a != "--bridge")
            .skip(1)
            .collect();
        let http = rest.first().map(|s| s.as_str()).unwrap_or(DEFAULT_ADDR);
        let ws = rest.get(1).map(|s| s.as_str()).unwrap_or(DEFAULT_WS_ADDR);
        bridge::run(http, ws);
        return;
    }

    let addr = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| DEFAULT_ADDR.to_string());
    let server = Server::http(&addr).expect("agent: failed to bind the listen address");
    println!("axiom-doom agent listening on http://{addr}");
    println!("  POST /step {{action}}   POST /reset   GET /state");
    let mut session = AgentSession::new();
    for mut req in server.incoming_requests() {
        let (status, body) = route(&mut session, &mut req);
        let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("static header is valid");
        let response = Response::from_string(body)
            .with_status_code(status)
            .with_header(header);
        let _ = req.respond(response);
    }
}

/// Run the headless perception demo: advance the perceiver `ticks` ticks and
/// print, each tick, what the agent sees (the wall ahead and its distance, every
/// visible enemy, and the velocity of each tracked moving enemy). Pure perception
/// — there is no scripted route here; the agent reacts to what it perceives.
fn run_perception_demo(ticks: u32) {
    println!("axiom-doom perception demo — the agent sees and tracks ({ticks} ticks)");
    let mut perceiver = DoomPerceiver::new();
    for _ in 0..ticks {
        let sight = perceiver.advance();
        // Only print ticks where something noteworthy is perceived, so the stream
        // reads as "the agent noticed X" rather than a wall of identical lines.
        if !sight.visible.is_empty() || !sight.tracked.is_empty() {
            println!("tick {}:", perceiver.tick());
            for line in sight.report_lines() {
                println!("{line}");
            }
        }
    }
    println!("done.");
}

/// Route one request to the session, returning `(status, json_body)`.
fn route(session: &mut AgentSession, req: &mut tiny_http::Request) -> (u16, String) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/");
    match (method, path) {
        (Method::Post, "/step") => {
            let mut body = String::new();
            if req.as_reader().read_to_string(&mut body).is_err() {
                return (400, error_json("could not read request body"));
            }
            if body.trim().is_empty() {
                body = "{}".to_string();
            }
            match serde_json::from_str::<Action>(&body) {
                Ok(action) => {
                    let mut obs = session.step(&action);
                    maybe_render(session, &action, &mut obs);
                    (200, obs_json(&obs))
                }
                Err(e) => (400, error_json(&format!("bad action json: {e}"))),
            }
        }
        (Method::Post, "/reset") => {
            session.reset();
            (200, obs_json(&session.observe()))
        }
        (Method::Get, "/state") => (200, obs_json(&session.observe())),
        _ => (
            404,
            error_json("not found; use POST /step, POST /reset, GET /state"),
        ),
    }
}

/// Render the current frame to a PNG and attach its path, when `render` was
/// requested and the `agent-render` feature (and a GPU) are available.
#[cfg(feature = "doom-agent-render")]
fn maybe_render(session: &AgentSession, action: &Action, obs: &mut Observation) {
    if !action.render {
        return;
    }
    let (vertices, indices, max) = session.geometry();
    let frame = session.frame();
    obs.image = render::render_frame(
        &vertices,
        &indices,
        max,
        &frame.instance_floats(),
        frame.clear_color(),
        session.tick(),
    );
}

#[cfg(not(feature = "doom-agent-render"))]
fn maybe_render(_session: &AgentSession, _action: &Action, _obs: &mut Observation) {}

fn obs_json(obs: &Observation) -> String {
    serde_json::to_string(obs).expect("observation serializes")
}

fn error_json(msg: &str) -> String {
    let quoted = serde_json::to_string(msg).unwrap_or_else(|_| "\"error\"".to_string());
    format!("{{\"error\":{quoted}}}")
}
