//! # axiom-dev-reload — retro FPS browser hot-reload dev server
//!
//! Depends on nothing beyond the `tiny_http` crate and the Rust standard
//! library.
//!
//! ## What it does
//!
//! It is a tiny native dev server with two jobs:
//!
//! 1. **Serve static files** for the retro FPS browser app (`index.html`, the wasm
//!    bundle, JS glue, etc.) out of a `static_dir`.
//! 2. **Hot-reload a level file.** It watches one `level_file` on disk and
//!    pushes its full contents to every connected browser over
//!    [Server-Sent Events][sse] (SSE) whenever the file's modification time
//!    changes. A developer edits the level file, saves it, and the running
//!    browser receives the new content live (the page subscribes to `/events`
//!    via an `EventSource` and rebuilds the level from `event.data`).
//!
//! [sse]: https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events
//!
//! ## Usage
//!
//! ```text
//! cargo run -p axiom-dev-reload [port] [static_dir] [level_file]
//! ```
//!
//! All three positional args are optional and default to:
//! - `port`       = `8080`
//! - `static_dir` = `dist` (the packaged gallery; retro FPS is at `dist/retro-fps/`)
//! - `level_file` = `apps/axiom-retro-fps/src/level.axiom`
//!
//! The level file may not exist yet — that is fine. A missing file is treated
//! as empty contents, and the watcher keeps polling so a later-created file is
//! picked up automatically.
//!
//! ## SSE multi-line encoding (the important detail)
//!
//! The level file is multi-line text. The SSE wire format is line-oriented:
//! each event is a run of `data:` lines terminated by a blank line. A payload
//! that itself contains newlines must therefore be split into **one `data:`
//! line per line of content**, and the browser's `EventSource` rejoins them
//! with `\n` into a single `event.data` string. For example, the two-line file
//!
//! ```text
//! AB
//! CD
//! ```
//!
//! is sent on the wire as:
//!
//! ```text
//! data:AB
//! data:CD
//! <blank line>
//! ```
//!
//! and the browser reassembles it into the exact string `"AB\nCD"`. We encode
//! it exactly that way (see [`encode_sse_event`]) so the file's text — newlines
//! intact — arrives whole. An empty payload is sent as a single empty `data:`
//! line followed by the blank dispatch line.
//!
//! ## How the SSE stream is held open over tiny_http
//!
//! `tiny_http` is a blocking, synchronous server. Its high-level
//! `Request::respond(Response)` path is **not** suitable for open-ended SSE: a
//! streaming `Response` body wraps the socket in a chunked encoder that buffers
//! up to 8 KiB and only flushes when that buffer fills, the reader returns EOF,
//! or `respond` returns — so small, infrequent events (a level file is tiny)
//! would sit buffered and never reach the browser. That is a real property of
//! tiny_http, not a tuning knob.
//!
//! The sanctioned escape for streaming is [`Request::into_writer`], which hands
//! back the **raw socket writer** (its documented CGI/streaming use case). We
//! write the status line and SSE headers ourselves, then loop: block on an
//! [`mpsc::Receiver`] for the next level-file snapshot, write it as one
//! [`encode_sse_event`]-formatted event, and **flush after every event** so the
//! browser sees each update immediately. SSE rides a plain connection that
//! stays open until close — no chunked encoding is required.
//!
//! A background watcher thread polls the level file's mtime ~every 200ms and,
//! on first read or any change, broadcasts the file contents to every
//! registered client `Sender`. Each `/events` request registers its own
//! channel; senders whose receiver has hung up are pruned. Every request is
//! handled on its own thread, so a blocked SSE writer never starves other
//! requests (static file fetches keep working while streams are open).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use tiny_http::{Header, Response, Server};

/// Default values for the optional positional CLI args.
const DEFAULT_PORT: u16 = 8080;
const DEFAULT_STATIC_DIR: &str = "dist";
const DEFAULT_LEVEL_FILE: &str = "apps/axiom-retro-fps/src/level.axiom";

/// How often the watcher thread polls the level file's modification time.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// The set of connected SSE clients. Each connected `/events` request registers
/// the `Sender` half of an `mpsc` channel here; the watcher thread broadcasts
/// new file contents to all of them. Shared between the watcher thread and the
/// request-handling threads.
type Clients = Arc<Mutex<Vec<Sender<String>>>>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let port: u16 = args
        .first()
        .map(|s| s.parse().unwrap_or(DEFAULT_PORT))
        .unwrap_or(DEFAULT_PORT);
    let static_dir = PathBuf::from(
        args.get(1)
            .map(String::as_str)
            .unwrap_or(DEFAULT_STATIC_DIR),
    );
    let level_file = PathBuf::from(
        args.get(2)
            .map(String::as_str)
            .unwrap_or(DEFAULT_LEVEL_FILE),
    );

    println!("axiom-dev-reload — retro FPS browser hot-reload dev server");
    println!("  serving    http://localhost:{port}/");
    println!("  static dir {}", static_dir.display());
    println!("  watching   {}", level_file.display());
    println!("  (edit the level file and connected browsers reload it live)");

    let clients: Clients = Arc::new(Mutex::new(Vec::new()));

    {
        let clients = Arc::clone(&clients);
        let level_file = level_file.clone();
        thread::spawn(move || watch_level_file(&level_file, clients));
    }

    let server = match Server::http(("0.0.0.0", port)) {
        Ok(server) => Arc::new(server),
        Err(err) => {
            eprintln!("axiom-dev-reload: failed to bind port {port}: {err}");
            std::process::exit(1);
        }
    };

    for request in server.incoming_requests() {
        let clients = Arc::clone(&clients);
        let static_dir = static_dir.clone();
        let level_file = level_file.clone();
        thread::spawn(move || handle_request(request, &static_dir, &level_file, &clients));
    }
}

/// Poll `level_file`'s modification time forever, broadcasting its contents to
/// every registered client on the first read and on every subsequent change.
/// A missing file is treated as empty contents (mtime `None`) and is picked up
/// automatically once it is created.
fn watch_level_file(level_file: &Path, clients: Clients) {
    let mut last_mtime: Option<SystemTime> = None;
    let mut first = true;

    loop {
        let mtime = fs::metadata(level_file).and_then(|m| m.modified()).ok();

        if first || mtime != last_mtime {
            first = false;
            last_mtime = mtime;
            let contents = read_level(level_file);
            broadcast(&clients, &contents);
        }

        thread::sleep(POLL_INTERVAL);
    }
}

/// Read the level file's contents, treating a missing/unreadable file as empty.
fn read_level(level_file: &Path) -> String {
    fs::read_to_string(level_file).unwrap_or_default()
}

/// Send `contents` to every registered client, pruning any whose receiver has
/// hung up (the client disconnected).
fn broadcast(clients: &Clients, contents: &str) {
    let mut guard = clients.lock().unwrap_or_else(|p| p.into_inner());
    guard.retain(|tx| tx.send(contents.to_string()).is_ok());
}

/// Route a single request: `/events` opens an SSE stream; anything else is a
/// static-file fetch.
fn handle_request(
    request: tiny_http::Request,
    static_dir: &Path,
    level_file: &Path,
    clients: &Clients,
) {
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/");

    if path == "/events" {
        serve_events(request, level_file, clients);
    } else {
        serve_static(request, static_dir, path);
    }
}

/// Open a long-lived SSE stream for one client.
///
/// Registers a fresh channel in `clients`, immediately seeds it with the
/// current level-file contents, takes the raw socket writer via
/// [`Request::into_writer`], writes the status line + SSE headers, then writes
/// one [`encode_sse_event`] per broadcast and flushes after each so updates
/// reach the browser immediately. Returns (closing the connection) as soon as
/// the socket errors (client disconnected) or the channel closes.
fn serve_events(request: tiny_http::Request, level_file: &Path, clients: &Clients) {
    let (tx, rx) = mpsc::channel::<String>();

    // Seed this client with the current contents so it renders immediately on
    // connect, before any file change.
    let _ = tx.send(read_level(level_file));

    clients.lock().unwrap_or_else(|p| p.into_inner()).push(tx);

    let mut writer = request.into_writer();

    // SSE rides a plain (non-chunked) connection held open until close. Write
    // the response head ourselves so we control flushing per event.
    let head = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: keep-alive\r\n",
        "\r\n",
    );
    if writer
        .write_all(head.as_bytes())
        .and_then(|()| writer.flush())
        .is_err()
    {
        return;
    }

    // Block for each broadcast, write it as one SSE event, flush immediately.
    // Any socket error means the client hung up: stop and let the writer drop.
    for payload in rx {
        let event = encode_sse_event(&payload);
        if writer
            .write_all(&event)
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
    }
}

/// Serve a static file from `static_dir`. Maps `/` to `index.html`, sets a
/// correct `Content-Type` per extension, sends `Cache-Control: no-cache`, and
/// rejects path traversal. Returns 404 for missing files.
fn serve_static(request: tiny_http::Request, static_dir: &Path, path: &str) {
    // Reject path traversal outright.
    if path.contains("..") {
        let _ = request.respond(Response::from_string("forbidden").with_status_code(403));
        return;
    }

    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    let file_path = static_dir.join(relative);

    match fs::read(&file_path) {
        Ok(bytes) => {
            let mut response = Response::from_data(bytes)
                .with_header(header("Content-Type", content_type_for(&file_path)))
                .with_header(header("Cache-Control", "no-cache"));
            // (already 200 by default, but be explicit)
            response = response.with_status_code(200);
            let _ = request.respond(response);
        }
        Err(_) => {
            let _ = request.respond(Response::from_string("not found").with_status_code(404));
        }
    }
}

/// Pick a `Content-Type` from a file's extension. Unknown extensions fall back
/// to `application/octet-stream`.
fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

/// Build a `tiny_http::Header` from a field name and value. Both are static or
/// owned strings we control, so `from_bytes` cannot fail here; if it ever did
/// we would rather know loudly than serve a malformed header.
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("static header name/value is always valid")
}

/// Encode a payload string as a single SSE event: one `data:` line per line of
/// content, terminated by a blank line that dispatches the event. An empty
/// payload becomes one empty `data:` line plus the blank dispatch line.
///
/// See the module-level docs for the wire format and why multi-line payloads
/// must be split this way.
fn encode_sse_event(payload: &str) -> Vec<u8> {
    let mut out = String::new();
    // `str::lines` yields nothing for an empty string and drops a trailing
    // newline; we still want exactly one event, so handle the empty case as a
    // single empty `data:` line.
    if payload.is_empty() {
        out.push_str("data:\n");
    } else {
        for line in payload.split('\n') {
            out.push_str("data:");
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push('\n');
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_is_one_empty_data_line() {
        assert_eq!(encode_sse_event(""), b"data:\n\n");
    }

    #[test]
    fn single_line_payload() {
        assert_eq!(encode_sse_event("hello"), b"data:hello\n\n");
    }

    #[test]
    fn multi_line_payload_splits_per_line() {
        // The canonical example from the SSE spec / module docs.
        assert_eq!(encode_sse_event("AB\nCD"), b"data:AB\ndata:CD\n\n");
    }

    #[test]
    fn trailing_newline_yields_trailing_empty_data_line() {
        // A trailing newline means a final empty line of content.
        assert_eq!(encode_sse_event("AB\n"), b"data:AB\ndata:\n\n");
    }

    #[test]
    fn content_types_cover_known_extensions() {
        assert_eq!(content_type_for(Path::new("a/index.html")), "text/html");
        assert_eq!(content_type_for(Path::new("a/app.js")), "text/javascript");
        assert_eq!(
            content_type_for(Path::new("a/app.wasm")),
            "application/wasm"
        );
        assert_eq!(content_type_for(Path::new("a/style.css")), "text/css");
        assert_eq!(
            content_type_for(Path::new("a/data.json")),
            "application/json"
        );
        assert_eq!(
            content_type_for(Path::new("a/thing.bin")),
            "application/octet-stream"
        );
        assert_eq!(
            content_type_for(Path::new("a/noext")),
            "application/octet-stream"
        );
    }

    #[test]
    fn broadcast_prunes_disconnected_clients() {
        let clients: Clients = Arc::new(Mutex::new(Vec::new()));
        let (live_tx, live_rx) = mpsc::channel::<String>();
        let (dead_tx, dead_rx) = mpsc::channel::<String>();
        drop(dead_rx); // simulate a client whose receiver hung up
        clients.lock().unwrap().push(live_tx);
        clients.lock().unwrap().push(dead_tx);

        broadcast(&clients, "AB\nCD");

        assert_eq!(live_rx.recv().unwrap(), "AB\nCD");
        assert_eq!(clients.lock().unwrap().len(), 1);
    }
}
