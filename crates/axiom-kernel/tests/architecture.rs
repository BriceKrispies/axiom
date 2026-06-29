//! Mechanical architecture enforcement for the Axiom kernel (a root of the layer DAG).
//!
//! These tests scan the kernel's own source tree and fail the build if any of
//! the hard architecture rules are violated. They are intentionally crude
//! substring scans: the goal is a fast, dependency-free tripwire, not a parser.
//!
//! This file lives under `tests/` (not `src/`) and only ever scans `src/`, so
//! the forbidden patterns it searches *for* never trip the scan of themselves.

use std::fs;
use std::path::{Path, PathBuf};

/// Collect every `.rs` file under the crate's `src/` directory.
fn kernel_source_files() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(!files.is_empty(), "expected to find kernel source files");
    files.sort();
    files
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

/// Read a file's contents as a string for scanning.
fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("kernel source must be valid UTF-8")
}

/// Assert that no kernel source file contains any of the forbidden substrings.
fn assert_absent(forbidden: &[&str], why: &str) {
    let mut violations = Vec::new();
    for path in kernel_source_files() {
        let text = read(&path);
        for needle in forbidden {
            if text.contains(needle) {
                violations.push(format!(
                    "{}: contains forbidden `{}`",
                    path.display(),
                    needle
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
        "the kernel must not reference browser / JS APIs",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "the kernel must not read wall-clock time",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()"],
        "the kernel must not use randomness",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "the kernel must emit structured records, not print to a console",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "the kernel must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "the kernel must not use global mutable state",
    );
}

#[test]
fn no_utils_module() {
    for path in kernel_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert_ne!(name, "utils", "the kernel must not have a `utils` module");
    }
}

#[test]
fn lib_exports_are_curated_set() {
    // `KernelApi` is the primary facade; the rest are the primitive types
    // future layers must be able to *name* (store, construct, match on). Any
    // change to this set requires explicit justification in ARCHITECTURE.md —
    // mismatches fail the build so accidental surface widening is caught.
    let lib = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"));
    let mut actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    actual.sort();

    let mut expected: Vec<&str> = vec![
        "pub use facade::KernelApi;",
        "pub use error::KernelError;",
        "pub use error_code::KernelErrorCode;",
        "pub use error_scope::KernelErrorScope;",
        "pub use result::KernelResult;",
        "pub use fixed_step::FixedStep;",
        "pub use frame_index::FrameIndex;",
        "pub use simulation_clock::SimulationClock;",
        "pub use tick::Tick;",
        "pub use replay_timeline::ReplayTimeline;",
        "pub use tick_divider::TickDivider;",
        "pub use tick_delta::TickDelta;",
        "pub use tick_schedule::TickSchedule;",
        "pub use deterministic_rng::DeterministicRng;",
        "pub use meters::Meters;",
        "pub use radians::Radians;",
        "pub use ratio::Ratio;",
        "pub use seconds::Seconds;",
        "pub use asset_id::AssetId;",
        "pub use entity_id::EntityId;",
        "pub use handle_id::HandleId;",
        "pub use message_id::MessageId;",
        "pub use binary_reader::BinaryReader;",
        "pub use binary_writer::BinaryWriter;",
        "pub use schema_version::SchemaVersion;",
        "pub use reflect::Reflect;",
        "pub use type_schema::{FieldSchema, TypeSchema};",
        "pub use stable_hash::StableHash;",
        "pub use in_memory_log_sink::InMemoryLogSink;",
        "pub use log_field::LogField;",
        "pub use log_level::LogLevel;",
        "pub use log_record::LogRecord;",
        "pub use log_sink::LogSink;",
        "pub use in_memory_telemetry_sink::InMemoryTelemetrySink;",
        "pub use metric_kind::MetricKind;",
        "pub use metric_value::MetricValue;",
        "pub use telemetry_metric::TelemetryMetric;",
        "pub use telemetry_sink::TelemetrySink;",
    ];
    expected.sort();

    assert_eq!(
        actual, expected,
        "lib.rs public exports must match the curated set; update both lib.rs and this test together"
    );
}
