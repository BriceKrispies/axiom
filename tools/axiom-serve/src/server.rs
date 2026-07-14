//! HTTP routing, static serving, page injection, import stamping, and SSE.
//!
//! Routes, in order:
//!
//! 1. `/events` — the SSE reload stream (contract shared with
//!    `scripts/axiom_dev_server.mjs`: `retry: 1000\n: connected\n\n` on
//!    connect, then `event: reload\ndata: <epoch-ms>\n\n` per rebuild).
//! 2. TsSdkHosted only: `/vendor/axiom-game/*` → `packages/axiom-game/dist/*`,
//!    `/pkg/*` → `apps/axiom-game-runtime/web/pkg/*`.
//! 3. TsWebEngine only: `/vendor/axiom-web-engine/*` →
//!    `packages/axiom-web-engine/dist/*`.
//! 4. Everything else: static from the app's `web/` (`/` → `index.html`).
//!
//! Every response carries `Cache-Control: no-store`; `..` traversal is 403.
//!
//! Two serve-time transforms (both ported from the mjs dev server):
//!
//! - **Import stamping** — quoted *relative* `.js` specifiers inside served
//!   `web/dist/*.js` files get `?v=<version>` appended, so a hot reload
//!   re-fetches the whole compiled module graph, not just the entry the
//!   harness re-imports. Absolute (`/dist`, `/vendor`, `/pkg`) and bare
//!   (`@axiom/…`) specifiers are left alone.
//! - **HTML injection** — RustWasm pages get a full-page SSE reload `<script>`
//!   before `</body>` (their pages know nothing of `/events`); TsWebEngine
//!   pages lacking an import map get one injected into `<head>` so the bare
//!   `@axiom/web-engine` specifier resolves to the vendored dist. TS pages get
//!   NO reload script — their harnesses already listen to `/events` and
//!   hot-swap in place.
//!
//! ## SSE over tiny_http
//!
//! tiny_http's `respond()` path wraps streaming bodies in a chunked encoder
//! that buffers ~8 KiB — unusable for tiny, infrequent SSE events. As in
//! `tools/axiom-dev-reload`, the sanctioned escape is `Request::into_writer`:
//! take the raw socket, write the head ourselves, and flush after every event.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Request, Response};

use crate::app::AppKind;

/// The set of connected SSE clients: each `/events` request registers the
/// `Sender` half of an mpsc channel; the watcher broadcasts reload versions.
pub type Clients = Arc<Mutex<Vec<Sender<u64>>>>;

/// Everything a request handler needs, shared across request threads.
pub struct ServeCtx {
    pub root: PathBuf,
    pub app_dir: PathBuf,
    pub kind: AppKind,
    pub version: Arc<AtomicU64>,
    pub clients: Clients,
}

/// The full-page reload script injected into RustWasm pages.
const RELOAD_SCRIPT: &str = "<script>new EventSource(\"/events\").addEventListener(\"reload\",()=>location.reload());</script>";

/// The import map injected into TsWebEngine pages that lack one.
const WEB_ENGINE_IMPORT_MAP: &str = "<script type=\"importmap\">{\"imports\":{\"@axiom/web-engine\":\"/vendor/axiom-web-engine/index.js\"}}</script>";

/// Send `version` to every connected SSE client, pruning hung-up ones.
pub fn broadcast(clients: &Clients, version: u64) {
    let mut guard = clients.lock().unwrap_or_else(|p| p.into_inner());
    guard.retain(|tx| tx.send(version).is_ok());
}

/// Route one request (each runs on its own thread, so a blocked SSE stream
/// never starves static fetches).
pub fn handle(request: Request, ctx: &ServeCtx) {
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();

    if path == "/events" {
        serve_events(request, ctx);
        return;
    }
    // Reject path traversal outright, on every file-serving route.
    if path.contains("..") {
        let _ = request.respond(Response::from_string("forbidden").with_status_code(403));
        return;
    }
    if ctx.kind == AppKind::TsSdkHosted {
        if let Some(rest) = path.strip_prefix("/vendor/axiom-game/") {
            let base = ctx.root.join("packages").join("axiom-game").join("dist");
            serve_file(request, ctx, &base, rest);
            return;
        }
        if let Some(rest) = path.strip_prefix("/pkg/") {
            let base = ctx
                .root
                .join("apps")
                .join("axiom-game-runtime")
                .join("web")
                .join("pkg");
            serve_file(request, ctx, &base, rest);
            return;
        }
    }
    if ctx.kind == AppKind::TsWebEngine {
        if let Some(rest) = path.strip_prefix("/vendor/axiom-web-engine/") {
            let base = ctx
                .root
                .join("packages")
                .join("axiom-web-engine")
                .join("dist");
            serve_file(request, ctx, &base, rest);
            return;
        }
    }
    let rel = if path == "/" {
        "index.html".to_string()
    } else {
        path.trim_start_matches('/').to_string()
    };
    let base = ctx.app_dir.join("web");
    serve_file(request, ctx, &base, &rel);
}

/// Serve one file from `base`, applying the serve-time transforms, with
/// `Cache-Control: no-store` and a per-extension content type. 404 if missing.
fn serve_file(request: Request, ctx: &ServeCtx, base: &Path, rel: &str) {
    let file_path = base.join(rel);
    match fs::read(&file_path) {
        Ok(bytes) => {
            let bytes = transform(ctx, &file_path, bytes);
            let response = Response::from_data(bytes)
                .with_header(header("Content-Type", content_type_for(&file_path)))
                .with_header(header("Cache-Control", "no-store"))
                .with_status_code(200);
            let _ = request.respond(response);
        }
        Err(_) => {
            let _ =
                request.respond(Response::from_string(format!("404 {rel}")).with_status_code(404));
        }
    }
}

/// Apply the serve-time transforms: HTML injection per kind, and import
/// version-stamping for the app's compiled `web/dist/*.js` modules.
fn transform(ctx: &ServeCtx, file_path: &Path, bytes: Vec<u8>) -> Vec<u8> {
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "html" {
        return match String::from_utf8(bytes) {
            Ok(text) => match ctx.kind {
                AppKind::RustWasm { .. } => inject_reload_script(&text).into_bytes(),
                AppKind::TsWebEngine => inject_import_map(&text).into_bytes(),
                _ => text.into_bytes(),
            },
            Err(err) => err.into_bytes(),
        };
    }
    let dist = ctx.app_dir.join("web").join("dist");
    if ext == "js" && file_path.starts_with(&dist) {
        return match String::from_utf8(bytes) {
            Ok(text) => {
                stamp_relative_imports(&text, ctx.version.load(Ordering::SeqCst)).into_bytes()
            }
            Err(err) => err.into_bytes(),
        };
    }
    bytes
}

/// Append `?v=<version>` to every quoted **relative** `.js` specifier
/// (`"./x.js"`, `'../a/b.js'`) so a hot reload re-fetches the whole compiled
/// module graph. Absolute and bare specifiers are untouched; a quoted region
/// never spans a newline. Std string scan — no regex crate.
pub fn stamp_relative_imports(src: &str, version: u64) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    let mut rest = src;
    while let Some(start) = rest.find(['"', '\'']) {
        let quote = rest.as_bytes()[start] as char;
        // Copy everything up to and including the opening quote.
        out.push_str(&rest[..=start]);
        let after = &rest[start + 1..];
        // The closing quote must come before any newline (same-line string).
        match after.find([quote, '\n']) {
            Some(end) if after.as_bytes()[end] as char == quote => {
                let spec = &after[..end];
                out.push_str(spec);
                if (spec.starts_with("./") || spec.starts_with("../")) && spec.ends_with(".js") {
                    out.push_str("?v=");
                    out.push_str(&version.to_string());
                }
                out.push(quote);
                rest = &after[end + 1..];
            }
            _ => {
                // Unterminated on this line: not a specifier — copy on.
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Inject the SSE full-page reload script before `</body>` (or append if the
/// page has no closing body tag). RustWasm pages only.
pub fn inject_reload_script(html: &str) -> String {
    match html.rfind("</body>") {
        Some(idx) => format!("{}{RELOAD_SCRIPT}\n{}", &html[..idx], &html[idx..]),
        None => format!("{html}\n{RELOAD_SCRIPT}"),
    }
}

/// Inject the `@axiom/web-engine` import map into `<head>` — but only when
/// the page declares no import map of its own (a page that ships one already
/// controls its specifier resolution). TsWebEngine pages only.
pub fn inject_import_map(html: &str) -> String {
    if html.contains("type=\"importmap\"") {
        return html.to_string();
    }
    // Insert right after the opening <head …> tag (import maps must precede
    // the first module script). "<head" alone would also match "<header".
    let insert_at = html.find("<head>").map(|i| i + "<head>".len()).or_else(|| {
        html.find("<head ")
            .and_then(|i| html[i..].find('>').map(|close| i + close + 1))
    });
    match insert_at {
        Some(idx) => format!("{}\n{WEB_ENGINE_IMPORT_MAP}{}", &html[..idx], &html[idx..]),
        None => format!("{WEB_ENGINE_IMPORT_MAP}\n{html}"),
    }
}

/// Pick a `Content-Type` from a file's extension — axiom-dev-reload's table,
/// extended with .mjs/.map/.png/.svg/.ts for the TS app shapes.
pub fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ts") => "text/typescript; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Build a `tiny_http::Header` from a name/value pair we control.
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("static header name/value is always valid")
}

/// Open a long-lived SSE stream for one client.
///
/// Registers a channel in `clients`, takes the raw socket writer via
/// `Request::into_writer` (tiny_http's chunked `respond` path cannot stream
/// SSE — see the module docs), writes the head + the connect preamble, then
/// one `reload` event per broadcast version, flushing after each.
fn serve_events(request: Request, ctx: &ServeCtx) {
    let (tx, rx) = mpsc::channel::<u64>();
    ctx.clients
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(tx);

    let mut writer = request.into_writer();
    let head = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream\r\n",
        "Cache-Control: no-store\r\n",
        "Connection: keep-alive\r\n",
        "\r\n",
        "retry: 1000\n: connected\n\n",
    );
    if writer
        .write_all(head.as_bytes())
        .and_then(|()| writer.flush())
        .is_err()
    {
        return;
    }
    for version in rx {
        let event = format!("event: reload\ndata: {version}\n\n");
        if writer
            .write_all(event.as_bytes())
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_relative_js_imports_only() {
        let src = concat!(
            "import { a } from \"./a.js\";\n",
            "import { b } from '../lib/b.js';\n",
            "import { sdk } from \"@axiom/web-engine\";\n",
            "import { abs } from \"/dist/game.js\";\n",
            "const notJs = \"./readme.md\";\n",
        );
        let out = stamp_relative_imports(src, 42);
        assert!(out.contains("\"./a.js?v=42\""));
        assert!(out.contains("'../lib/b.js?v=42'"));
        assert!(out.contains("\"@axiom/web-engine\""));
        assert!(out.contains("\"/dist/game.js\""));
        assert!(out.contains("\"./readme.md\""));
    }

    #[test]
    fn stamping_is_idempotent_and_newline_safe() {
        // Already-stamped specifiers end with ?v=N, not .js — untouched.
        let once = stamp_relative_imports("import x from \"./a.js\";", 1);
        assert_eq!(stamp_relative_imports(&once, 2), once);
        // An apostrophe in a comment must not swallow the rest of the file.
        let tricky = "// it's a comment\nimport y from \"./y.js\";\n";
        assert!(stamp_relative_imports(tricky, 7).contains("\"./y.js?v=7\""));
        // No quotes at all: unchanged.
        assert_eq!(stamp_relative_imports("const x = 1;", 3), "const x = 1;");
    }

    #[test]
    fn reload_script_lands_before_body_close() {
        let html = "<html><body><p>hi</p></body></html>";
        let out = inject_reload_script(html);
        let script = out.find("EventSource").unwrap();
        assert!(script < out.find("</body>").unwrap());
        // No </body>: appended at the end.
        assert!(inject_reload_script("<p>x</p>").ends_with(RELOAD_SCRIPT));
    }

    #[test]
    fn import_map_injected_into_head_only_when_absent() {
        let html = "<html><head><title>t</title></head><body></body></html>";
        let out = inject_import_map(html);
        let map = out.find("type=\"importmap\"").unwrap();
        assert!(map > out.find("<head>").unwrap());
        assert!(map < out.find("<title>").unwrap());
        assert!(out.contains("\"@axiom/web-engine\":\"/vendor/axiom-web-engine/index.js\""));

        // A page that ships its own import map is untouched.
        let own = "<html><head><script type=\"importmap\">{}</script></head></html>";
        assert_eq!(inject_import_map(own), own);

        // A <head> with attributes still gets the map after its tag; and a
        // page with no <head> gets it prepended, never inside "<header>".
        let attrs = "<html><head lang=\"en\"><title>t</title></head></html>";
        assert!(inject_import_map(attrs).find("importmap").unwrap() > attrs.find('>').unwrap());
        let headless = "<header>x</header>";
        assert!(inject_import_map(headless).starts_with(WEB_ENGINE_IMPORT_MAP));
    }

    #[test]
    fn content_types_cover_the_extended_table() {
        assert_eq!(
            content_type_for(Path::new("i.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("a.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("a.mjs")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("s.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("d.json")),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("a.js.map")),
            "application/json; charset=utf-8"
        );
        assert_eq!(content_type_for(Path::new("m.wasm")), "application/wasm");
        assert_eq!(content_type_for(Path::new("p.png")), "image/png");
        assert_eq!(content_type_for(Path::new("v.svg")), "image/svg+xml");
        assert_eq!(
            content_type_for(Path::new("s.ts")),
            "text/typescript; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("x.bin")),
            "application/octet-stream"
        );
    }

    #[test]
    fn broadcast_prunes_disconnected_clients() {
        let clients: Clients = Arc::new(Mutex::new(Vec::new()));
        let (live_tx, live_rx) = mpsc::channel::<u64>();
        let (dead_tx, dead_rx) = mpsc::channel::<u64>();
        drop(dead_rx);
        clients.lock().unwrap().push(live_tx);
        clients.lock().unwrap().push(dead_tx);

        broadcast(&clients, 1234);

        assert_eq!(live_rx.recv().unwrap(), 1234);
        assert_eq!(clients.lock().unwrap().len(), 1);
    }
}
