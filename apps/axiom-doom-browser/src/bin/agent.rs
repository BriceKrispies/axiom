//! `axiom-doom-browser` agent bridge — an HTTP server that drives the real DOOM
//! game from JSON so an external agent can send inputs and read back structured
//! state (and, with `agent-render`, images). Native + `agent` feature only
//! (`required-features`), so it never enters the wasm build or the default gates.
//!
//! Drive it with `curl`:
//!   curl -s -XPOST localhost:7878/step -d '{"keys":["forward"],"fire":true}'
//!   curl -s -XPOST localhost:7878/reset
//!   curl -s localhost:7878/state

use std::io::Read;

use axiom_doom_browser::agent::{Action, AgentSession};
use tiny_http::{Header, Method, Response, Server};

const DEFAULT_ADDR: &str = "127.0.0.1:7878";

fn main() {
    let addr = std::env::args()
        .nth(1)
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
                    let obs = session.step(&action);
                    (200, obs_json(&obs))
                }
                Err(e) => (400, error_json(&format!("bad action json: {e}"))),
            }
        }
        (Method::Post, "/reset") => {
            session.reset();
            (200, obs_json(&session.observe(false)))
        }
        (Method::Get, "/state") => (200, obs_json(&session.observe(false))),
        _ => (
            404,
            error_json("not found; use POST /step, POST /reset, GET /state"),
        ),
    }
}

fn obs_json(obs: &axiom_doom_browser::agent::Observation) -> String {
    serde_json::to_string(obs).expect("observation serializes")
}

fn error_json(msg: &str) -> String {
    let quoted = serde_json::to_string(msg).unwrap_or_else(|_| "\"error\"".to_string());
    format!("{{\"error\":{quoted}}}")
}
