//! `axiom-render-bench` — a render benchmark harness.
//!
//! It builds + serves the gallery, drives a demo (default `generia`) headlessly
//! through the Playwright controller with the autonomous agent walk (`?agent=1`),
//! runs it for a fixed duration, and reduces the Canvas2D backend's per-frame
//! console telemetry to an FPS + phase-breakdown report. `--debug` builds a debug
//! wasm bundle so the `convert` deep project/shade split is present too.
//!
//! ```text
//! cargo run -p axiom-render-bench -- --demo generia --backend canvas2d --duration 10
//! cargo run -p axiom-render-bench -- --demo generia --backend canvas2d --debug --json
//! ```
//!
//! It is repo tooling — outside the engine dependency graph and the coverage gate.
//! GPU backends do not log per-frame timing to the console (Canvas2D is the
//! instrumented path), so FPS-via-telemetry is Canvas2D-only today.

mod report;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

/// Parsed CLI options.
struct Args {
    demo: String,
    backend: String,
    duration_s: u64,
    warmup_s: u64,
    agent: bool,
    debug: bool,
    json: bool,
    port: u16,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            demo: "generia".to_string(),
            backend: "canvas2d".to_string(),
            duration_s: 10,
            warmup_s: 2,
            agent: true,
            debug: false,
            json: false,
            port: 8199,
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--demo" => a.demo = value()?,
            "--backend" => a.backend = value()?,
            "--duration" => a.duration_s = value()?.parse().map_err(|_| "bad --duration")?,
            "--warmup" => a.warmup_s = value()?.parse().map_err(|_| "bad --warmup")?,
            "--port" => a.port = value()?.parse().map_err(|_| "bad --port")?,
            "--agent" => a.agent = true,
            "--no-agent" => a.agent = false,
            "--debug" => a.debug = true,
            "--json" => a.json = true,
            "-h" | "--help" => return Err(help()),
            other => return Err(format!("unknown flag: {other}\n{}", help())),
        }
    }
    Ok(a)
}

fn help() -> String {
    "axiom-render-bench — build+serve the gallery, walk a demo, report FPS + phases\n\
     \n\
     --demo <name>        demo to benchmark (default: generia)\n\
     --backend <b>        canvas2d | webgl2 | webgpu (default: canvas2d; only\n\
     \x20                    canvas2d logs per-frame telemetry today)\n\
     --duration <secs>    measurement window (default: 10)\n\
     --warmup <secs>      warm-up window skipped before measuring (default: 2)\n\
     --no-agent           disable the autonomous agent walk (?agent=1)\n\
     --debug              build a debug wasm bundle (adds the convert deep-split)\n\
     --json               emit machine-readable JSON\n\
     --port <n>           local server port (default: 8199)"
        .to_string()
}

/// The repository root (this crate lives at `<root>/tools/axiom-render-bench`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("repo root two levels above the crate")
}

/// Build the gallery bundle into `dist/` (release, or a debug wasm build for the
/// deep profiler). Shells out to the packaging script.
fn build_gallery(root: &Path, debug: bool) -> Result<(), String> {
    eprintln!(
        "[render-bench] building gallery ({} wasm)…",
        if debug { "debug" } else { "release" }
    );
    // Use `uv run --no-project python` — the interpreter the Makefile uses (it
    // guarantees a modern Python with `tomllib`, unlike a bare `python`).
    let mut cmd = Command::new("uv");
    cmd.args(["run", "--no-project", "python", "scripts/package_gallery.py", "--fast"]);
    debug.then(|| cmd.arg("--debug"));
    let status = cmd
        .current_dir(root)
        .status()
        .map_err(|e| format!("failed to run packaging script (is `uv` on PATH?): {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "gallery build failed".to_string())
}

/// Serve `dist/` on `port` in the background (a plain static file server).
///
/// Spawned as bare `python` (stdlib `http.server` needs no `tomllib`, so the system
/// interpreter is fine) rather than `uv run python` — so [`Child::kill`] actually
/// kills the server. Killing `uv` would only kill the wrapper and orphan its python
/// child, leaving the port bound and the next run serving stale content.
fn serve(root: &Path, port: u16) -> Result<Child, String> {
    let child = Command::new("python")
        .args(["-m", "http.server", &port.to_string(), "--directory", "dist"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start static server (is python on PATH?): {e}"))?;
    Ok(child)
}

/// Run one Playwright-controller command and return its stdout (JSON).
fn pw(root: &Path, action: &str, extra: &[&str]) -> String {
    let out = Command::new("uv")
        .args(["run", "scripts/playwright_controller.py", action])
        .args(extra)
        .current_dir(root)
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(e) => {
            eprintln!("[render-bench] playwright `{action}` failed: {e}");
            String::new()
        }
    }
}

/// Skip the first `n` samples of each telemetry population (the warm-up window).
fn skip_warmup(t: report::Telemetry, n: usize) -> report::Telemetry {
    report::Telemetry {
        frames: t.frames.into_iter().skip(n).collect(),
        phases: t.phases.into_iter().skip(n).collect(),
        deeps: t.deeps.into_iter().skip(n).collect(),
    }
}

fn run(a: &Args) -> Result<String, String> {
    let root = repo_root();
    build_gallery(&root, a.debug)?;

    let mut server = serve(&root, a.port)?;
    // Give the static server a moment to bind before the browser navigates.
    sleep(Duration::from_secs(2));

    let agent = a.agent.then_some("&agent=1").unwrap_or("");
    let url = format!(
        "http://localhost:{}/{}/?backend={}{agent}",
        a.port, a.demo, a.backend
    );
    eprintln!("[render-bench] loading {url}");
    pw(&root, "goto", &[&url]);
    pw(&root, "wait", &[&(a.warmup_s * 1000).to_string()]);
    // Everything logged so far is warm-up; count it so the measurement skips it.
    let warmup_frames = report::frame_count(&pw(&root, "console", &[]));
    eprintln!(
        "[render-bench] warm-up done ({warmup_frames} frames); measuring {}s…",
        a.duration_s
    );
    pw(&root, "wait", &[&(a.duration_s * 1000).to_string()]);
    let full = report::parse(&pw(&root, "console", &[]));
    let measured = skip_warmup(full, warmup_frames);

    // Best-effort teardown (do not fail the report if these do).
    pw(&root, "stop", &[]);
    let _ = server.kill();
    let _ = server.wait();

    if measured.frames.is_empty() {
        return Err(format!(
            "no Canvas2D telemetry captured — is `{}` a Canvas2D run? (GPU backends log no \
             per-frame timing). Check the page loaded without errors.",
            a.backend
        ));
    }

    let label = format!(
        "{} / {} / {}{}",
        a.demo,
        a.backend,
        if a.debug { "debug" } else { "release" },
        if a.agent { " / agent-walk" } else { "" }
    );
    Ok(report::render(&measured, &label, a.json))
}

fn main() {
    let code = match parse_args().and_then(|a| run(&a)) {
        Ok(report) => {
            println!("{report}");
            0
        }
        Err(e) => {
            eprintln!("[render-bench] {e}");
            1
        }
    };
    std::process::exit(code);
}
