//! Architecture-boundary tests for the `axiom-workspace` app.
//!
//! The workspace is a composition leaf: an app that launches/observes/records/
//! replays runtime sessions. These tests pin its boundaries — it depends on no
//! engine module, imports no browser/GPU/DOM API from its portable Rust, is not
//! depended on by any layer or module, adds no junk-drawer folder, and its
//! browser shell uses no iframe and no UI framework.

use std::fs;
use std::path::{Path, PathBuf};

fn app_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn app_src() -> PathBuf {
    app_root().join("src")
}

fn app_web() -> PathBuf {
    app_root().join("web")
}

fn repo_root() -> PathBuf {
    app_root()
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels up")
}

fn collect_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("readable dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            collect_ext(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("utf-8 source")
}

fn strip_comments_and_strings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let (mut in_string, mut in_char) = (false, false);
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            if c == '\\' {
                chars.next();
            } else if c == '\'' {
                in_char = false;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '\'' => in_char = true,
            _ => out.push(c),
        }
    }
    out
}

fn app_rs_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_ext(&app_src(), "rs", &mut files);
    assert!(!files.is_empty(), "expected app source files");
    files.sort();
    files
}

fn assert_absent_in_rs(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in app_rs_sources() {
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!("{}: forbidden `{}`", path.display(), needle));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

#[test]
fn app_toml_lists_only_consumed_layers_and_no_modules() {
    let text = fs::read_to_string(app_root().join("app.toml")).expect("app.toml exists");
    assert!(
        text.contains("\"kernel\""),
        "app.toml lists the kernel layer"
    );
    assert!(
        text.contains("\"runtime\""),
        "app.toml lists the runtime layer"
    );
    assert!(
        text.contains("allowed_modules = []"),
        "the workspace depends on no engine module yet"
    );
    for forbidden in ["\"scene\"", "\"render\"", "\"webgpu\"", "\"windowing\""] {
        assert!(
            !text.contains(forbidden),
            "app.toml must not list `{forbidden}` — no such dependency exists yet"
        );
    }
}

#[test]
fn portable_rust_has_no_browser_gpu_or_dom() {
    // Browser APIs may appear ONLY in the browser shell (web/), never in the
    // portable Rust app crate.
    assert_absent_in_rs(
        &[
            "web_sys",
            "js_sys",
            "wasm_bindgen",
            "wgpu",
            "WebGpu",
            "WebGL",
            "GPUDevice",
            "HtmlCanvas",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
            "iframe",
        ],
        "the workspace Rust crate is portable: no browser/GPU/DOM/iframe",
    );
}

#[test]
fn rust_is_deterministic_no_wall_clock_or_randomness() {
    assert_absent_in_rs(
        &[
            "std::time",
            "SystemTime",
            "Instant::now",
            "chrono",
            "rand::",
            "thread_rng",
            "getrandom",
            "fastrand",
        ],
        "the workspace is deterministic: no wall-clock time, no randomness",
    );
}

#[test]
fn no_placeholder_macros_in_rust() {
    assert_absent_in_rs(
        &["todo!", "unimplemented!", "println!", "eprintln!", "dbg!"],
        "no placeholder macros or console output in the workspace crate",
    );
}

#[test]
fn imports_only_kernel_runtime_and_self() {
    let mut illegal = Vec::new();
    for path in app_rs_sources() {
        for chunk in strip_comments_and_strings(&read(&path))
            .split(|c: char| !c.is_alphanumeric() && c != '_')
        {
            if chunk.starts_with("axiom_")
                && chunk != "axiom_kernel"
                && chunk != "axiom_runtime"
                && chunk != "axiom_workspace"
            {
                illegal.push(format!("{}: {}", path.display(), chunk));
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "the workspace may only consume axiom-kernel and axiom-runtime:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn no_junk_drawer_folders_or_modules() {
    // Applies to both the Rust crate and the browser shell.
    let mut all = app_rs_sources();
    collect_ext(&app_web(), "ts", &mut all);
    for path in all {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(
                stem,
                banned,
                "no `{banned}` junk-drawer at {}",
                path.display()
            );
        }
    }
}

#[test]
fn no_layer_or_module_depends_on_the_workspace_app() {
    let mut hits = Vec::new();
    for base in ["crates", "modules"] {
        let mut files = Vec::new();
        collect_ext(&repo_root().join(base), "rs", &mut files);
        collect_ext(&repo_root().join(base), "toml", &mut files);
        for path in files {
            if read(&path).contains("axiom-workspace") || read(&path).contains("axiom_workspace") {
                hits.push(path.display().to_string());
            }
        }
    }
    assert!(
        hits.is_empty(),
        "no engine layer/module may depend on the workspace app:\n{}",
        hits.join("\n")
    );
}

#[test]
fn browser_shell_uses_no_iframe() {
    let mut files = Vec::new();
    collect_ext(&app_web(), "ts", &mut files);
    collect_ext(&app_web(), "html", &mut files);
    collect_ext(&app_web(), "css", &mut files);
    let mut hits = Vec::new();
    for path in &files {
        let text = read(path).to_ascii_lowercase();
        if text.contains("iframe") {
            hits.push(path.display().to_string());
        }
    }
    assert!(
        hits.is_empty(),
        "the workspace browser shell must not use iframes:\n{}",
        hits.join("\n")
    );
}

#[test]
fn no_iframe_in_shipped_app_or_shell_code() {
    // Extends the web-only iframe check to the whole *shipped* app: every portable
    // Rust source AND every browser-shell file. The prose docs (`*.md`) and these
    // test sources legitimately discuss the ban by name ("no iframe"), so they are
    // the enforcers, not the subject — they are intentionally not scanned here.
    let mut files = Vec::new();
    collect_ext(&app_src(), "rs", &mut files);
    for ext in ["ts", "html", "css", "js"] {
        collect_ext(&app_web(), ext, &mut files);
    }
    assert!(!files.is_empty(), "expected app + shell files to scan");
    let mut hits = Vec::new();
    for path in &files {
        let raw = read(path);
        // Rust sources are stripped so a banned needle inside a doc comment or
        // string literal cannot mask (or fabricate) a real use — mirrors
        // `assert_absent_in_rs`. Shell files are scanned raw.
        let is_rs = path.extension().and_then(|e| e.to_str()) == Some("rs");
        let text = match is_rs {
            true => strip_comments_and_strings(&raw),
            false => raw,
        };
        if text.to_ascii_lowercase().contains("iframe") {
            hits.push(path.display().to_string());
        }
    }
    assert!(
        hits.is_empty(),
        "no shipped workspace Rust or shell code may use an iframe:\n{}",
        hits.join("\n")
    );
}

#[test]
fn no_junk_drawer_directories() {
    // The file-stem check above bans `utils.rs`/`helpers.ts`/…; this bans the same
    // names as *directory* modules anywhere under the Rust crate or the shell.
    fn walk_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
        if !dir.is_dir() {
            return;
        }
        for entry in fs::read_dir(dir).expect("readable dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                out.push(path.clone());
                walk_dirs(&path, out);
            }
        }
    }
    let mut dirs = Vec::new();
    walk_dirs(&app_src(), &mut dirs);
    walk_dirs(&app_web(), &mut dirs);
    for path in dirs {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(
                name,
                banned,
                "no `{banned}` junk-drawer directory at {}",
                path.display()
            );
        }
    }
}

#[test]
fn lib_exposes_single_api_facade() {
    // The Module/facade rule for the app: exactly one behavioral `*Api` facade,
    // `WorkspaceApi`. The value/state vocabulary it traffics in may be re-exported
    // freely, but no *second* `*Api` may appear.
    let lib = read(&app_src().join("lib.rs"));
    let stripped = strip_comments_and_strings(&lib);
    assert!(
        stripped.contains("WorkspaceApi"),
        "lib.rs must re-export the WorkspaceApi facade"
    );
    let mut api_idents: Vec<String> = stripped
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|tok| !tok.is_empty() && tok.ends_with("Api"))
        .map(str::to_string)
        .collect();
    api_idents.sort();
    api_idents.dedup();
    assert_eq!(
        api_idents,
        vec!["WorkspaceApi".to_string()],
        "WorkspaceApi must be the sole `*Api` facade the workspace exports"
    );
}

#[test]
fn browser_shell_has_a_file_for_every_panel() {
    // The shell is modular: one TS module per panel, plus the shell scaffolding.
    let panels_dir = app_web().join("src").join("panels");
    for name in [
        "project_browser",
        "game_manifest_editor",
        "level_browser",
        "runtime_viewport",
        "object_inspector",
        "asset_browser",
        "console_log_viewer",
        "profiler",
        "input_debugger",
        "timeline_replay",
        "play_controls",
        "package_export",
    ] {
        let file = panels_dir.join(format!("{name}_panel.ts"));
        assert!(
            file.is_file(),
            "missing panel shell file {}",
            file.display()
        );
    }
    for rel in [
        "index.html",
        "src/main.ts",
        "src/workspace_state.ts",
        "src/workspace_events.ts",
        "src/workspace_layout.ts",
        "src/panel_registry.ts",
        "src/dom_mount.ts",
        "styles/workspace.css",
    ] {
        let file = app_web().join(rel);
        assert!(
            file.is_file(),
            "missing shell scaffold file {}",
            file.display()
        );
    }
}

#[test]
fn no_panel_performs_runtime_simulation() {
    // The workspace is a data-contract app: panels STORE ticks/hashes but run no
    // game loop. Ban simulation ENTRYPOINTS only — a getter like
    // `fn tick(&self) -> Tick` returning stored data is legitimate and stays.
    assert_absent_in_rs(
        &[
            "fn step(",
            "fn simulate(",
            "fn advance(",
            "fn run(",
            "fn update(",
            "loop {",
            "std::thread",
            "thread::spawn",
        ],
        "workspace panels are data contracts, not a runtime — no simulation entrypoints",
    );
}

#[test]
fn browser_shell_uses_no_ui_framework() {
    let mut files = Vec::new();
    collect_ext(&app_web(), "ts", &mut files);
    collect_ext(&app_web(), "html", &mut files);
    let banned = [
        "react", "vue", "svelte", "angular", "next", "remix", "electron", "tauri",
    ];
    let mut hits = Vec::new();
    for path in &files {
        let text = read(path).to_ascii_lowercase();
        for framework in banned {
            // Match an import/dependency reference, not an incidental substring.
            if text.contains(&format!("from \"{framework}"))
                || text.contains(&format!("from '{framework}"))
                || text.contains(&format!("\"{framework}\""))
            {
                hits.push(format!("{}: {}", path.display(), framework));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "the workspace browser shell is vanilla TS/HTML/CSS:\n{}",
        hits.join("\n")
    );
}
