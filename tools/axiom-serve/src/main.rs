//! # axiom-serve — build + serve any `apps/` browser app locally with hot reload
//!
//! Repo tooling (a Tool, by its `tools/` location) — outside the engine
//! dependency graph, the coverage gate, and the branchless gate. Depends on
//! nothing beyond the `tiny_http` crate and the Rust standard library, in the
//! same spirit as `tools/axiom-dev-reload`.
//!
//! ```text
//! cargo run -p axiom-serve -- <app> [--port N] [--no-open] [--debug]
//! cargo run -p axiom-serve -- home-run
//! cargo run -p axiom-serve -- gravix --port 9000 --no-open
//! ```
//!
//! ## What it does
//!
//! 1. **Resolves** `<app>` to an app crate directory (a path, `<root>/<arg>`,
//!    `<root>/apps/<arg>`, or `<root>/apps/axiom-<arg>` — whichever holds a
//!    `web/` dir), mirroring `scripts/package_app.py`'s `resolve_app`.
//! 2. **Detects its shape** (see [`app::AppKind`]): a TypeScript app over the
//!    `@axiom/game` SDK, over `@axiom/web-engine`, plain TypeScript, or a Rust
//!    wasm app built through `wasm-bindgen`.
//! 3. **Builds it** (`cargo build --target wasm32-unknown-unknown` +
//!    `wasm-bindgen`, or `tsgo -p web/tsconfig.json`), running any one-time
//!    prerequisites first (SDK dist builds, the shared game-runtime wasm pkg).
//! 4. **Serves** the app's `web/` dir on `0.0.0.0:<port>` with the vendor/pkg
//!    routes the dev pages expect, `Cache-Control: no-store` everywhere, and
//!    the same import version-stamping + SSE reload contract as
//!    `scripts/axiom_dev_server.mjs`.
//! 5. **Watches** the app's sources by mtime polling and rebuilds on change,
//!    broadcasting `event: reload` over `/events` after each successful build.
//!
//! An initial build failure still starts the server: fix the error and save,
//! and the watcher rebuilds and reloads the browser.

mod app;
mod build;
mod server;
mod watch;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default listen port (the conventional Axiom dev-server port).
const DEFAULT_PORT: u16 = 8080;

/// Parsed CLI options.
#[derive(Debug)]
struct Args {
    app: String,
    port: u16,
    open: bool,
    debug: bool,
}

fn help() -> String {
    "axiom-serve — build + serve one apps/ browser app locally with hot reload\n\
     \n\
     usage: cargo run -p axiom-serve -- <app> [--port N] [--no-open] [--debug]\n\
     \n\
     <app>        app name or path: home-run | axiom-home-run | apps/axiom-home-run\n\
     --port N     listen port (default: 8080)\n\
     --no-open    do not open the browser after the initial build\n\
     --debug      Rust wasm apps: build the debug profile instead of release\n\
     \n\
     app shapes (auto-detected):\n\
     \x20 web/tsconfig.json mentioning @axiom/game        TypeScript over the @axiom/game SDK\n\
     \x20 web/tsconfig.json mentioning @axiom/web-engine  TypeScript over @axiom/web-engine\n\
     \x20 web/tsconfig.json (anything else)               plain TypeScript (tsgo)\n\
     \x20 Cargo.toml with a cdylib crate-type + web/      Rust wasm via wasm-bindgen"
        .to_string()
}

/// Hand-rolled arg loop in the style of `tools/axiom-render-bench`.
fn parse_args(argv: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut app: Option<String> = None;
    let mut port = DEFAULT_PORT;
    let mut open = true;
    let mut debug = false;
    let mut it = argv;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--port" => {
                let value = it.next().ok_or("--port needs a value")?;
                port = value
                    .parse()
                    .map_err(|_| format!("bad --port value: {value}"))?;
            }
            "--no-open" => open = false,
            "--debug" => debug = true,
            "-h" | "--help" => return Err(help()),
            flag if flag.starts_with('-') => {
                return Err(format!("unknown flag: {flag}\n\n{}", help()));
            }
            positional => {
                if app.is_some() {
                    return Err(format!(
                        "unexpected extra argument: {positional}\n\n{}",
                        help()
                    ));
                }
                app = Some(positional.to_string());
            }
        }
    }
    let app = app.ok_or_else(|| format!("missing <app> argument\n\n{}", help()))?;
    Ok(Args {
        app,
        port,
        open,
        debug,
    })
}

/// The repository root (this crate lives at `<root>/tools/axiom-serve`), so the
/// tool works from any working directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("repo root two levels above the crate")
}

/// Milliseconds since the Unix epoch — the reload/cache-bust version.
fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Open `url` in the platform browser; failure is a warning, never fatal.
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(err) = result {
        eprintln!("axiom-serve: could not open the browser ({err}) — open {url} yourself");
    }
}

fn run() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1))?;
    let root = repo_root();

    let app_dir = app::resolve_app_dir(&root, &args.app)?;
    let kind = app::detect_kind(&app_dir)?;
    println!("axiom-serve: app  {}", app_dir.display());
    println!("axiom-serve: kind {}", kind.label());

    let plan = build::BuildPlan {
        root: root.clone(),
        app_dir: app_dir.clone(),
        kind: kind.clone(),
        debug: args.debug,
    };
    plan.prepare()?;

    if let Err(err) = plan.build() {
        eprintln!("axiom-serve: initial build failed: {err}");
        eprintln!("axiom-serve: starting the server anyway — fix the error and save to rebuild");
    }

    let version = Arc::new(AtomicU64::new(epoch_ms()));
    let clients: server::Clients = Arc::new(Mutex::new(Vec::new()));

    // The watcher thread: poll the app's sources, rebuild on change, and
    // broadcast a reload (with a fresh version) after each successful build.
    {
        let spec = app::watch_spec(&app_dir, &kind);
        let plan = plan.clone();
        let version = Arc::clone(&version);
        let clients = Arc::clone(&clients);
        thread::spawn(move || {
            watch::run(&spec, || {
                println!("axiom-serve: change detected — rebuilding…");
                match plan.build() {
                    Ok(()) => {
                        let v = epoch_ms();
                        version.store(v, Ordering::SeqCst);
                        server::broadcast(&clients, v);
                        println!("axiom-serve: rebuilt — reloading connected browsers");
                    }
                    Err(err) => {
                        eprintln!("axiom-serve: rebuild failed: {err} (not reloading)");
                    }
                }
            });
        });
    }

    let http = tiny_http::Server::http(("0.0.0.0", args.port))
        .map_err(|err| format!("failed to bind port {}: {err}", args.port))?;
    let url = format!("http://localhost:{}/", args.port);
    println!("axiom-serve: serving {url}  (Ctrl+C to stop)");
    if args.open {
        open_browser(&url);
    }

    let ctx = Arc::new(server::ServeCtx {
        root,
        app_dir,
        kind,
        version,
        clients,
    });
    for request in http.incoming_requests() {
        let ctx = Arc::clone(&ctx);
        thread::spawn(move || server::handle(request, &ctx));
    }
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> impl Iterator<Item = String> {
        args.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn parses_app_and_defaults() {
        let args = parse_args(argv(&["home-run"])).unwrap();
        assert_eq!(args.app, "home-run");
        assert_eq!(args.port, DEFAULT_PORT);
        assert!(args.open);
        assert!(!args.debug);
    }

    #[test]
    fn parses_flags() {
        let args = parse_args(argv(&[
            "home-run",
            "--port",
            "9000",
            "--no-open",
            "--debug",
        ]))
        .unwrap();
        assert_eq!(args.port, 9000);
        assert!(!args.open);
        assert!(args.debug);
    }

    #[test]
    fn rejects_unknown_flag_missing_app_and_extra_positional() {
        assert!(parse_args(argv(&["x", "--nope"]))
            .unwrap_err()
            .contains("unknown flag"));
        assert!(parse_args(argv(&[])).unwrap_err().contains("missing <app>"));
        assert!(parse_args(argv(&["a", "b"]))
            .unwrap_err()
            .contains("unexpected extra"));
        assert!(parse_args(argv(&["a", "--port", "zzz"]))
            .unwrap_err()
            .contains("bad --port"));
    }

    #[test]
    fn repo_root_holds_this_crate() {
        assert!(repo_root().join("tools").join("axiom-serve").is_dir());
    }
}
