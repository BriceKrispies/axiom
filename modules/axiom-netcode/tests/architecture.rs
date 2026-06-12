//! Architecture-boundary tests for the `axiom-netcode` engine module.
//!
//! The workspace's `xtask` checker enforces the global module law (allowed
//! layers, no module-to-module deps, etc.). These per-module tests are the
//! second line of defence: they scan this crate's source tree for forbidden
//! tokens so module-internal regressions fail the build at `cargo test` time.

use std::fs;
use std::path::{Path, PathBuf};

fn netcode_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn netcode_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels above modules/axiom-netcode")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src directory must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn netcode_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&netcode_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected netcode source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

fn strip_comments_and_strings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut in_char = false;
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '\'' {
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
        if c == '"' {
            in_string = true;
            continue;
        }
        if c == '\'' {
            in_char = true;
            continue;
        }
        out.push(c);
    }
    out
}

fn assert_absent(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in netcode_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "axiom-netcode {}: contains forbidden `{}`",
                    path.display(),
                    needle
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

fn assert_absent_in_other(dir: PathBuf, label: &str, forbidden: &[&str], why: &str) {
    let mut files = Vec::new();
    if dir.is_dir() {
        collect_rs(&dir, &mut files);
    }
    files.sort();
    let mut violations = Vec::new();
    for path in &files {
        let stripped = strip_comments_and_strings(&read(path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{label} {}: contains forbidden `{}`",
                    path.display(),
                    needle
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

// ---------- manifest presence ----------

#[test]
fn module_toml_exists() {
    assert!(
        netcode_root().join("module.toml").is_file(),
        "expected modules/axiom-netcode/module.toml to exist"
    );
}

#[test]
fn module_toml_has_empty_allowed_modules() {
    let text = fs::read_to_string(netcode_root().join("module.toml")).unwrap();
    let stripped = strip_comments_and_strings(&text);
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-netcode's module.toml must declare `allowed_modules = []`"
    );
}

#[test]
fn lib_rs_exports_only_netcode_api() {
    let lib = read(&netcode_src_dir().join("lib.rs"));
    let actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    assert_eq!(
        actual,
        vec!["pub use netcode_api::NetcodeApi;"],
        "axiom-netcode's lib.rs must publicly export exactly one item: NetcodeApi"
    );
}

// ---------- legal layer imports only ----------

#[test]
fn netcode_imports_only_the_kernel_layer() {
    // Netcode is a kernel-only module: the deterministic session needs the
    // kernel's time/id/codec/result primitives and nothing higher.
    let mut illegal = Vec::new();
    for path in netcode_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for line in stripped.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("axiom_") {
                continue;
            }
            for chunk in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if chunk.starts_with("axiom_")
                    && chunk != "axiom_kernel"
                    && chunk != "axiom_netcode"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-netcode may only import axiom-kernel:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn netcode_imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    if !modules_dir.is_dir() {
        return;
    }
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_netcode")
        .collect();
    let mut violations = Vec::new();
    for path in netcode_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        let tokens: std::collections::HashSet<&str> = stripped
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();
        for other in &other_modules {
            if tokens.contains(other.as_str()) {
                violations.push(format!(
                    "{}: references other module `{}`",
                    path.display(),
                    other
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "axiom-netcode must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

// ---------- lower layers must not import netcode ----------

#[test]
fn no_layer_imports_axiom_netcode() {
    for layer in [
        "axiom-kernel",
        "axiom-runtime",
        "axiom-math",
        "axiom-host",
        "axiom-frame",
        "axiom-ecs",
        "axiom-introspect",
    ] {
        let src = repo_root().join("crates").join(layer).join("src");
        assert_absent_in_other(
            src,
            layer,
            &["axiom_netcode", "axiom-netcode"],
            &format!("layer `{layer}` must not import axiom-netcode"),
        );
    }
}

#[test]
fn no_app_imports_axiom_netcode_unless_app_manifest_allows_it() {
    let apps_dir = repo_root().join("apps");
    if !apps_dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(&apps_dir).unwrap() {
        let path = entry.unwrap().path();
        let app_src = path.join("src");
        if !app_src.is_dir() {
            continue;
        }
        let mut sources = Vec::new();
        collect_rs(&app_src, &mut sources);
        let imports_netcode = sources
            .iter()
            .any(|src| strip_comments_and_strings(&read(src)).contains("axiom_netcode"));
        if !imports_netcode {
            continue;
        }
        let manifest_text = fs::read_to_string(path.join("app.toml")).unwrap_or_default();
        let no_comments: String = manifest_text
            .lines()
            .map(|l| match l.find('#') {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            no_comments.contains("\"netcode\""),
            "app `{}` imports axiom_netcode but its app.toml does not list \"netcode\" in \
             allowed_modules",
            path.display()
        );
    }
}

// ---------- source hygiene ----------

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-netcode must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &[
            "HtmlCanvas",
            "HtmlElement",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
        ],
        "axiom-netcode must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "webgpu", "WebGpu", "WebGL", "webgl", "GPUDevice"],
        "axiom-netcode must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_sockets_or_network_io() {
    // The deterministic core speaks plain bytes; the app owns the socket. No
    // real transport may leak into this module.
    assert_absent(
        &[
            "std::net",
            "TcpStream",
            "TcpListener",
            "UdpSocket",
            "WebSocket",
            "tokio",
            "mio",
        ],
        "axiom-netcode must not perform network I/O — the boundary is plain bytes",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "axiom-netcode must not read wall-clock time",
    );
}

#[test]
fn no_randomness() {
    // The seeded DeterministicRng lives in the kernel and is used only by the
    // convergence proof under tests/; the module's own src is RNG-free.
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-netcode src must not use a nondeterministic random source",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-netcode must not print to a console",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-netcode must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "axiom-netcode must not use global mutable state",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in netcode_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc"] {
            assert_ne!(
                name, banned,
                "axiom-netcode must not have a `{banned}` module"
            );
        }
    }
}
