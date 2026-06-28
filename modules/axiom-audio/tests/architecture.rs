//! Architecture-boundary tests for the `axiom-audio` engine module.
//!
//! The workspace `xtask` checker enforces the global Module Law (allowed layers,
//! no module-to-module deps, single facade) and owns the `PLATFORM_FACING_MODULES`
//! allowlist that lets this module's `wasm32` arm reference Web Audio. These
//! per-module tests are the second line of defence: they scan this crate's
//! `src/` tree for forbidden tokens so module-internal regressions fail at
//! `cargo test` time. Tests are exempt from the Branchless Law, so this file uses
//! ordinary control flow.
//!
//! The platform/determinism hygiene scans below run over the **native core only**
//! (`core_source_files`), excluding the `#[cfg(target_arch = "wasm32")]` Web Audio
//! arm under `src/audio_api/`. That arm legitimately owns `web_sys`/`AudioContext`
//! symbols — that is the whole point of the `PLATFORM_FACING_MODULES` amendment —
//! and never compiles on native, exactly like `axiom-windowing`'s `web` arm. The
//! invariant these tests pin is that the *core* (the spine, the covered code) is
//! browser-free.

use std::fs;
use std::path::{Path, PathBuf};

// Facade-only imports for the determinism check below: drive the module solely
// through its public facade plus the kernel value types that cross it. The rich
// batch return type stays sealed and is never named.
use axiom_audio::{AudioApi, AudioSeconds, Hertz, PlayOpts, ToneSpec, Wave};
use axiom_kernel::Ratio;

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn module_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels above modules/axiom-audio")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("directory must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&src_dir(), &mut files);
    assert!(!files.is_empty(), "expected audio source files");
    files.sort();
    files
}

/// The native-compiled spine only: every source file **except** the wasm32 Web
/// Audio arm under `src/audio_api/`. The platform-hygiene scans run over this so
/// the sanctioned, compiled-out arm's Web Audio symbols are not misread as a core
/// violation.
fn core_source_files() -> Vec<PathBuf> {
    source_files()
        .into_iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "audio_api"))
        .collect()
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

fn assert_absent_in(files: &[PathBuf], forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in files {
        let stripped = strip_comments_and_strings(&read(path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "axiom-audio {}: contains forbidden `{}`",
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

// ---------- manifest + facade ----------

#[test]
fn module_toml_exists_and_is_isolated() {
    let manifest = module_root().join("module.toml");
    assert!(manifest.is_file(), "expected modules/axiom-audio/module.toml");
    let stripped = strip_comments_and_strings(&fs::read_to_string(&manifest).unwrap());
    assert!(
        stripped.contains("allowed_modules = []"),
        "axiom-audio must declare `allowed_modules = []`"
    );
}

#[test]
fn lib_rs_exports_one_facade_plus_identity_vocabulary() {
    // Module Law #8: exactly one behavioral facade (AudioApi), plus the identity
    // vocabulary (the value-type nouns). All other public exports forbidden.
    let lib = read(&src_dir().join("lib.rs"));
    let pub_uses: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub use"))
        .collect();
    let facades: Vec<&str> = pub_uses
        .iter()
        .copied()
        .filter(|line| !line.contains("ids::"))
        .collect();
    assert_eq!(
        facades,
        vec!["pub use audio_api::AudioApi;"],
        "axiom-audio must expose exactly one behavioral facade (AudioApi)"
    );
    let id_lines = pub_uses.iter().filter(|line| line.contains("ids::")).count();
    assert_eq!(
        id_lines, 1,
        "axiom-audio re-exports its identity vocabulary via exactly one `pub use ids::{{…}}` line"
    );
}

// ---------- legal layer imports only ----------

#[test]
fn audio_imports_only_legal_layers() {
    // allowed_layers = ["kernel"]; the wasm arm also refers to crate-local
    // `axiom_audio`. No other `axiom_*` crate may appear anywhere in src.
    let mut illegal = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for line in stripped.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("axiom_") {
                continue;
            }
            for chunk in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if chunk.starts_with("axiom_")
                    && chunk != "axiom_kernel"
                    && chunk != "axiom_audio"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-audio may only import axiom-kernel:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn audio_imports_no_other_modules() {
    let modules_dir = repo_root().join("modules");
    let other_modules: Vec<String> = fs::read_dir(&modules_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace('-', "_"))
        .filter(|name| name != "axiom_audio")
        .collect();
    assert!(!other_modules.is_empty(), "expected sibling modules to exist");
    let mut violations = Vec::new();
    for path in source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        let tokens: std::collections::HashSet<&str> = stripped
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();
        for other in &other_modules {
            if tokens.contains(other.as_str()) {
                violations.push(format!("{}: references other module `{}`", path.display(), other));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "axiom-audio must not depend on any other module:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_layer_imports_axiom_audio() {
    let crates_dir = repo_root().join("crates");
    for entry in fs::read_dir(&crates_dir).expect("crates dir must exist") {
        let layer_dir = entry.expect("readable entry").path();
        let src = layer_dir.join("src");
        let label = layer_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("layer")
            .to_string();
        assert_absent_in_other(
            src,
            &label,
            &["axiom_audio", "axiom-audio"],
            &format!("layer `{label}` must not import axiom-audio"),
        );
    }
}

// ---------- source hygiene: the CORE (spine) is browser-free ----------

#[test]
fn core_has_no_browser_or_js_bindgen_apis() {
    assert_absent_in(
        &core_source_files(),
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "the axiom-audio CORE must not reference browser / JS bindings (those live only in the wasm32 arm)",
    );
}

#[test]
fn core_has_no_web_audio_or_dom_symbols() {
    assert_absent_in(
        &core_source_files(),
        &[
            "AudioContext",
            "OscillatorNode",
            "GainNode",
            "AnalyserNode",
            "AudioBuffer",
            "getUserMedia",
            "MediaStream",
            "requestAnimationFrame",
            "document.",
            "window.",
            "navigator.",
            "canvas",
        ],
        "the axiom-audio CORE must own no Web Audio / DOM symbols — it decides what plays and when as data",
    );
}

#[test]
fn core_has_no_wall_clock_time() {
    assert_absent_in(
        &core_source_files(),
        &["std::time", "SystemTime", "Instant", "chrono"],
        "the axiom-audio CORE must read no wall-clock time — the audio clock is the wasm arm's AudioContext",
    );
}

#[test]
fn core_has_no_randomness() {
    assert_absent_in(
        &core_source_files(),
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "the axiom-audio CORE must use no randomness",
    );
}

#[test]
fn core_has_no_threads_or_async_runtimes() {
    assert_absent_in(
        &core_source_files(),
        &["thread::spawn", "tokio", "async_std", "std::net", "std::process"],
        "the axiom-audio CORE must not spawn threads, use async runtimes, or touch net/process",
    );
}

#[test]
fn no_console_printing_anywhere() {
    // Module Law #10 bans console output even in the platform arm.
    assert_absent_in(
        &source_files(),
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-audio must not print to the console",
    );
}

#[test]
fn no_placeholder_macros_anywhere() {
    // Module Law #10 bans placeholder macros even in the platform arm.
    assert_absent_in(
        &source_files(),
        &["todo!", "unimplemented!"],
        "axiom-audio must contain no placeholder architecture",
    );
}

#[test]
fn core_has_no_global_mutable_state() {
    assert_absent_in(
        &core_source_files(),
        &["static mut", "lazy_static", "once_cell", "OnceLock"],
        "the axiom-audio CORE must use no global mutable state",
    );
}

#[test]
fn core_has_no_nondeterministic_or_unordered_collections() {
    assert_absent_in(
        &core_source_files(),
        &["HashMap", "HashSet", "BTreeMap", "BTreeSet", "LinkedList"],
        "the axiom-audio CORE must use insertion-ordered Vec storage so batches replay byte-identically",
    );
}

#[test]
fn core_has_no_foreign_engine_subsystem_concepts() {
    // Audio legitimately owns its own nouns (Tone/Voice/Wave/Envelope/Lfo); it
    // must not absorb the concepts owned by other subsystems.
    assert_absent_in(
        &core_source_files(),
        &[
            "axiom_scene",
            "axiom_render",
            "axiom_physics",
            "axiom_assets",
            "axiom_input",
            "axiom_ecs",
            "Renderable",
            "RigidBody",
            "Collider",
            "Skeleton",
            "KeyCode",
            "EditorPanel",
            "rodio",
            "cpal",
            "kira",
        ],
        "the axiom-audio CORE must not absorb scene/render/physics/asset/input concepts or external audio engines",
    );
}

#[test]
fn no_junk_drawer_modules() {
    for path in source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for banned in ["utils", "helpers", "common", "misc", "shared", "prelude"] {
            assert_ne!(name, banned, "axiom-audio must not have a `{banned}` module");
        }
    }
}

/// Every top-level `src/*.rs` file (other than `lib.rs`) must be wired into
/// `lib.rs` with a matching `mod <stem>;`, so the facade is the genuine single
/// entry point. The wasm arm lives under `src/audio_api/` and is declared inside
/// `audio_api.rs`, so this top-level (non-recursive) scan does not require it.
#[test]
fn every_top_level_source_module_is_declared_in_lib_rs() {
    let lib = strip_comments_and_strings(&read(&src_dir().join("lib.rs")));
    let mut missing = Vec::new();
    for entry in fs::read_dir(src_dir()).expect("src dir must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem == "lib" {
            continue;
        }
        let decl = format!("mod {stem};");
        if !lib.contains(&decl) {
            missing.push(format!("{stem} (expected `{decl}` in lib.rs)"));
        }
    }
    assert!(
        missing.is_empty(),
        "every top-level src/*.rs file must be declared in lib.rs — orphan modules:\n{}",
        missing.join("\n")
    );
}

// ---------- facade-level determinism proof ----------

/// Two independently-built mixers, driven by the identical sequence of public
/// facade calls, must drain to equal batches — the core replay invariant,
/// asserted through the public surface (the batch type stays sealed; we only
/// compare two of them).
#[test]
fn identical_call_sequences_replay_through_the_facade() {
    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }
    let run = || {
        let mut api = AudioApi::new();
        let s = api.load_sound("hit.ogg");
        let _ = api.play_tone(ToneSpec {
            wave: Wave::Sawtooth,
            freq: Hertz::new(330.0),
            duration: AudioSeconds::from_seconds(0.25),
            envelope: None,
            lfo: None,
            volume: ratio(0.9),
        });
        let _ = api.schedule_sound(
            s,
            AudioSeconds::from_seconds(1.0),
            PlayOpts {
                volume: ratio(0.7),
                pitch: ratio(1.0),
                looping: false,
            },
        );
        api.set_master_volume(ratio(0.5));
        api.set_muted(false);
        api.take_pending()
    };
    assert_eq!(
        run(),
        run(),
        "identical facade call sequences must replay to equal batches"
    );
}
