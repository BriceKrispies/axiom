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
//! - **HTML injection** — EVERY served page gets the full-page SSE reload
//!   `<script>` before `</body>`, and TsWebEngine pages lacking an import map
//!   additionally get one injected into `<head>` so the bare
//!   `@axiom/web-engine` specifier resolves to the vendored dist. The two
//!   compose; neither replaces the other.
//!
//!   This used to read "TS pages get NO reload script — their harnesses already
//!   listen to `/events` and hot-swap in place." That was never true: no such
//!   listener exists in `@axiom/web-engine` or `@axiom/game`, so the server
//!   rebuilt on save and broadcast a reload that nothing consumed, and every
//!   pure-TS app silently had no hot reload at all. Injection is now
//!   unconditional and idempotent, so a page that DOES grow its own `/events`
//!   listener still only reloads once.
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

/// The import map injected into TsWebEngine pages that lack one. The target
/// carries the cache-bust version like every other asset URL — it is the entry
/// to the whole vendored engine graph, so a stable URL here would let a cached
/// copy pin the engine no matter how many times it is rebuilt.
fn web_engine_import_map(version: u64) -> String {
    format!(
        "<script type=\"importmap\">{{\"imports\":{{\"@axiom/web-engine\":\"/vendor/axiom-web-engine/index.js?v={version}\"}}}}</script>"
    )
}

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
            // The two injections COMPOSE. The import map is kind-specific; the
            // reload script is not, because no app kind ships its own `/events`
            // listener (see the module docs).
            Ok(text) => {
                let mapped = match ctx.kind {
                    AppKind::TsWebEngine => inject_import_map(&text, ctx.version.load(Ordering::SeqCst)),
                    _ => text,
                };
                let stamped = stamp_page_assets(&mapped, ctx.version.load(Ordering::SeqCst));
                inject_reload_script(&stamped).into_bytes()
            }
            Err(err) => err.into_bytes(),
        };
    }
    // Every served .js, not just the app's own `web/dist`: the vendored
    // `/vendor/axiom-web-engine/*` graph is served by this same process and goes
    // stale in a browser cache exactly like the app's does.
    if ext == "js" {
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

/// True for a page asset URL that should carry a cache-busting version: LOCAL
/// (root- or dot-relative, never protocol-relative `//host` or absolute
/// `https://`) and a script or stylesheet. An already-stamped URL ends in
/// `?v=<n>` rather than the extension, so it fails this test — which is what
/// makes stamping idempotent.
fn is_stampable_asset(url: &str) -> bool {
    let local =
        (url.starts_with('/') && !url.starts_with("//")) || url.starts_with("./") || url.starts_with("../");
    let asset = url.ends_with(".js") || url.ends_with(".mjs") || url.ends_with(".css");
    local && asset
}

/// Append `?v=<version>` to every local `src=`/`href=` URL in a served page.
///
/// `stamp_relative_imports` already versions the *relative* specifiers INSIDE a
/// served `web/dist/*.js`, but nothing versioned the page's own entry
/// references — the `<script src>` and `<link href>` a browser caches hardest.
/// Those URLs were therefore byte-identical forever, so a phone (or any proxy)
/// that held a copy kept serving it no matter how many times the file changed on
/// disk. `Cache-Control: no-store` is advisory and mobile browsers routinely
/// reuse resources across tab restores anyway; a URL that actually CHANGES is
/// the only reliable bust. The version is epoch-ms, re-stamped on every rebuild
/// and reseeded on every server start, so it can never collide with a held copy.
pub fn stamp_page_assets(html: &str, version: u64) -> String {
    let mut out = String::with_capacity(html.len() + 96);
    let mut rest = html;
    loop {
        // Whichever attribute comes first in the remaining text.
        let next = ["src=\"", "href=\""]
            .iter()
            .filter_map(|prefix| rest.find(prefix).map(|at| (at, *prefix)))
            .min_by_key(|(at, _)| *at);
        let Some((at, prefix)) = next else { break };
        let value_start = at + prefix.len();
        out.push_str(&rest[..value_start]);
        let after = &rest[value_start..];
        let Some(end) = after.find('"') else {
            // Unterminated attribute: copy the remainder verbatim.
            rest = after;
            break;
        };
        let url = &after[..end];
        out.push_str(url);
        if is_stampable_asset(url) {
            out.push_str("?v=");
            out.push_str(&version.to_string());
        }
        out.push('"');
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Inject the SSE full-page reload script before `</body>` (or append if the
/// page has no closing body tag). Applied to every served page, and IDEMPOTENT:
/// a page that already carries the script is returned unchanged, so it can never
/// be double-injected into reloading twice.
pub fn inject_reload_script(html: &str) -> String {
    if html.contains(RELOAD_SCRIPT) {
        return html.to_owned();
    }
    match html.rfind("</body>") {
        Some(idx) => format!("{}{RELOAD_SCRIPT}\n{}", &html[..idx], &html[idx..]),
        None => format!("{html}\n{RELOAD_SCRIPT}"),
    }
}

/// Inject the `@axiom/web-engine` import map into `<head>` — but only when
/// the page declares no import map of its own (a page that ships one already
/// controls its specifier resolution). TsWebEngine pages only.
pub fn inject_import_map(html: &str, version: u64) -> String {
    if html.contains("type=\"importmap\"") {
        return html.to_string();
    }
    // Insert right after the opening <head …> tag (import maps must precede
    // the first module script). "<head" alone would also match "<header".
    let insert_at = html.find("<head>").map(|i| i + "<head>".len()).or_else(|| {
        html.find("<head ")
            .and_then(|i| html[i..].find('>').map(|close| i + close + 1))
    });
    let map = web_engine_import_map(version);
    match insert_at {
        Some(idx) => format!("{}\n{map}{}", &html[..idx], &html[idx..]),
        None => format!("{map}\n{html}"),
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
        Some("mp3") => "audio/mpeg",
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
    fn page_assets_are_stamped_and_stamping_is_idempotent() {
        let html = concat!(
            "<link rel=\"stylesheet\" href=\"/styles/a.css\">",
            "<script type=\"module\" src=\"/dist/main.js\"></script>",
            "<script src=\"./rel.mjs\"></script>",
            "<script src=\"https://cdn.example/x.js\"></script>",
            "<script src=\"//cdn.example/y.js\"></script>",
            "<a href=\"/docs/readme.md\">d</a>",
        );
        let out = stamp_page_assets(html, 42);
        // Local scripts and stylesheets get the version.
        assert!(out.contains("href=\"/styles/a.css?v=42\""));
        assert!(out.contains("src=\"/dist/main.js?v=42\""));
        assert!(out.contains("src=\"./rel.mjs?v=42\""));
        // Absolute and protocol-relative hosts are left alone, as is a non-asset.
        assert!(out.contains("src=\"https://cdn.example/x.js\""));
        assert!(out.contains("src=\"//cdn.example/y.js\""));
        assert!(out.contains("href=\"/docs/readme.md\""));
        // Re-stamping is a no-op: an already-stamped URL ends in ?v=N, not .js.
        assert_eq!(stamp_page_assets(&out, 43), out);
    }

    #[test]
    fn reload_injection_is_idempotent() {
        // The script is now injected into EVERY page, so a page that already
        // carries it (re-transformed, or one that hand-rolled the same snippet)
        // must not gain a second copy and reload twice per save.
        let once = inject_reload_script("<html><body><p>hi</p></body></html>");
        assert_eq!(once.matches("EventSource").count(), 1);
        assert_eq!(inject_reload_script(&once), once);
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
        let out = inject_import_map(html, 7);
        let map = out.find("type=\"importmap\"").unwrap();
        assert!(map > out.find("<head>").unwrap());
        assert!(map < out.find("<title>").unwrap());
        // The engine entry carries the cache-bust version like every other asset.
        assert!(out.contains("\"@axiom/web-engine\":\"/vendor/axiom-web-engine/index.js?v=7\""));

        // A page that ships its own import map is untouched.
        let own = "<html><head><script type=\"importmap\">{}</script></head></html>";
        assert_eq!(inject_import_map(own, 7), own);

        // A <head> with attributes still gets the map after its tag; and a
        // page with no <head> gets it prepended, never inside "<header>".
        let attrs = "<html><head lang=\"en\"><title>t</title></head></html>";
        assert!(inject_import_map(attrs, 7).find("importmap").unwrap() > attrs.find('>').unwrap());
        let headless = "<header>x</header>";
        assert!(inject_import_map(headless, 7).starts_with(&web_engine_import_map(7)));
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
