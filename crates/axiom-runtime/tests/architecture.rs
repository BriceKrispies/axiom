//! Mechanical architecture enforcement for axiom-runtime (an Axiom layer).
//!
//! Two jobs. The first is the **public-surface lock**: `lib.rs`'s exports are a
//! curated set, and any change to it must be a deliberate edit to both `lib.rs`
//! and this file. Everything below the runtime layer is one crate; everything
//! above it is most of the engine, so a silently-widened runtime surface is
//! permanent.
//!
//! The second is the **behavioural lock on the preparation barrier**: `start()`
//! accepts exactly `{Prepared, Paused}` and nothing else. That is the writable
//! form of "the simulation cannot begin until preparation completed" — the
//! stronger statement, "the only public path to `Running` is `start()`", is a
//! whole-crate reachability property no `#[test]` can observe.
//!
//! The remaining tests are the crude substring tripwires every sibling layer
//! carries. This file lives under `tests/` and only ever scans `src/`, so the
//! forbidden patterns it searches *for* never trip the scan of themselves.

use std::fs;
use std::path::{Path, PathBuf};

use axiom_runtime::{
    PreparationSchedule, PreparationTask, Runtime, RuntimeConfig, RuntimeError, RuntimeErrorCode,
    RuntimeResult, RuntimeState,
};

fn runtime_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
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

fn runtime_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&runtime_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected axiom-runtime source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

/// Strip `//` line comments and string-literal contents so a forbidden token
/// that appears only inside a doc comment or a string literal can't fail the
/// scan.
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
            // Consume to end of line, keeping the newline so line positions
            // remain meaningful.
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
    for path in runtime_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "axiom-runtime {}: contains forbidden `{needle}`",
                    path.display()
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

#[test]
fn no_browser_or_js_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "Math.random"],
        "axiom-runtime must not reference browser / JS APIs — it is not a \
         platform-facing layer",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "axiom-runtime must not read wall-clock time; it advances a kernel \
         simulation clock",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()"],
        "axiom-runtime must not use randomness; determinism is its whole point",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-runtime must emit structured records, not print to a console",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-runtime must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "axiom-runtime must not use global mutable state",
    );
}

#[test]
fn no_utils_module() {
    for path in runtime_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert!(
            !matches!(name, "utils" | "helpers" | "common" | "misc"),
            "axiom-runtime must not have a junk-drawer module: {}",
            path.display()
        );
    }
}

#[test]
fn runtime_only_imports_declared_dependencies() {
    // `depends_on = ["kernel"]`, plus the `axiom-zones` support crate every
    // package may name. Nothing else is legal in this layer's source.
    let mut illegal = Vec::new();
    for path in runtime_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for line in stripped.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("axiom_") {
                continue;
            }
            for chunk in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if chunk.starts_with("axiom_")
                    && chunk != "axiom_kernel"
                    && chunk != "axiom_zones"
                    && chunk != "axiom_runtime"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-runtime may only import axiom-kernel (its sole declared \
         dependency) and the axiom-zones support crate:\n{}",
        illegal.join("\n")
    );
}

#[test]
fn lib_exports_are_curated_set() {
    // `Runtime` is the primary facade; the rest are the types a layer above
    // must be able to *name* — the lifecycle vocabulary, the error vocabulary,
    // the per-step surfaces, and the two startup-preparation declarations.
    // Any change to this set is a deliberate widening of what the whole engine
    // above this layer may depend on, so it must be an explicit edit here too.
    let lib = read(&runtime_src_dir().join("lib.rs"));
    let mut actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    actual.sort();

    let mut expected: Vec<&str> = vec![
        "pub use preparation_schedule::PreparationSchedule;",
        "pub use preparation_task::PreparationTask;",
        "pub use runtime::Runtime;",
        "pub use runtime_command::RuntimeCommand;",
        "pub use runtime_command_queue::RuntimeCommandQueue;",
        "pub use runtime_config::RuntimeConfig;",
        "pub use runtime_context::RuntimeContext;",
        "pub use runtime_diagnostics::RuntimeDiagnostics;",
        "pub use runtime_error::RuntimeError;",
        "pub use runtime_error_code::RuntimeErrorCode;",
        "pub use runtime_event::RuntimeEvent;",
        "pub use runtime_event_queue::RuntimeEventQueue;",
        "pub use runtime_result::RuntimeResult;",
        "pub use runtime_scheduler::RuntimeScheduler;",
        "pub use runtime_state::RuntimeState;",
        "pub use runtime_step::RuntimeStep;",
        "pub use runtime_step_record::RuntimeStepRecord;",
        "pub use runtime_system::RuntimeSystem;",
        "pub use runtime_timeline::RuntimeTimeline;",
        "pub use system_outcome::SystemOutcome;",
    ];
    expected.sort();

    assert_eq!(
        actual, expected,
        "axiom-runtime's lib.rs public exports must match the curated set; \
         update both lib.rs and this test together"
    );
}

/// A task that always fails, used only to drive a runtime into `Failed`.
struct AlwaysFails;

impl PreparationTask for AlwaysFails {
    fn prepare(&mut self) -> RuntimeResult<()> {
        Err(RuntimeError::new(
            RuntimeErrorCode::PreparationFailed,
            "intentional",
        ))
    }
}

fn fresh() -> Runtime {
    Runtime::new(RuntimeConfig::new(16_666_667)).expect("a 60 Hz step is valid")
}

fn failing_schedule() -> PreparationSchedule {
    let mut schedule = PreparationSchedule::new();
    schedule.push("always-fails", Box::new(AlwaysFails));
    schedule
}

#[test]
fn start_accepts_exactly_prepared_or_paused() {
    // --- The four rejected states ---
    let mut created = fresh();
    assert_eq!(created.state(), RuntimeState::Created);
    assert_eq!(
        created.start().unwrap_err().code(),
        RuntimeErrorCode::InvalidLifecycleTransition,
        "Created has not even initialized"
    );

    let mut initialized = fresh();
    initialized.initialize().expect("a fresh runtime initializes");
    assert_eq!(initialized.state(), RuntimeState::Initialized);
    assert_eq!(
        initialized.start().unwrap_err().code(),
        RuntimeErrorCode::InvalidLifecycleTransition,
        "Initialized is the barrier: no preparation phase has run"
    );

    let mut stopped = fresh();
    stopped.initialize().expect("a fresh runtime initializes");
    stopped
        .prepare(PreparationSchedule::new())
        .expect("an empty schedule succeeds");
    stopped.stop().expect("a prepared runtime may be stopped");
    assert_eq!(stopped.state(), RuntimeState::Stopped);
    assert_eq!(
        stopped.start().unwrap_err().code(),
        RuntimeErrorCode::InvalidLifecycleTransition,
        "Stopped is terminal"
    );

    let mut failed = fresh();
    failed.initialize().expect("a fresh runtime initializes");
    failed
        .prepare(failing_schedule())
        .expect_err("the task fails");
    assert_eq!(failed.state(), RuntimeState::Failed);
    assert_eq!(
        failed.start().unwrap_err().code(),
        RuntimeErrorCode::InvalidLifecycleTransition,
        "Failed is terminal"
    );

    // --- The two accepted states ---
    let mut prepared = fresh();
    prepared.initialize().expect("a fresh runtime initializes");
    prepared
        .prepare(PreparationSchedule::new())
        .expect("an empty schedule succeeds");
    assert_eq!(prepared.state(), RuntimeState::Prepared);
    assert_eq!(prepared.start(), Ok(()), "Prepared starts");
    assert_eq!(prepared.state(), RuntimeState::Running);

    prepared.pause().expect("a running runtime pauses");
    assert_eq!(prepared.state(), RuntimeState::Paused);
    assert_eq!(
        prepared.start(),
        Ok(()),
        "Paused resumes without a second preparation phase"
    );
    assert_eq!(prepared.state(), RuntimeState::Running);
}
