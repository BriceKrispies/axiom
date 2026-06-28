//! Doc-comment hygiene gate for `axiom-physics`.
//!
//! The module's in-source documentation once carried stale Phase-1 scaffolding
//! text that the now-current collision pipeline contradicts. These tests read
//! every `src/*.rs` file and fail if any of those stale phrases reappear, so the
//! rot cannot return. They scan `src/` only — never the markdown docs (where
//! `ROADMAP.md`'s "Phase 1 — … (done)" history is legitimate) and never this
//! `tests/` directory (so the test's own literals do not trip it).
//!
//! Tests are exempt from the Branchless Law, so ordinary control flow is used.

use std::fs;
use std::path::PathBuf;

/// Phrases that, used to describe *current* behavior, are stale lies about the
/// real pipeline (live broad/narrow phase + sequential-impulse solver).
const STALE_PHRASES: &[&str] = &[
    "empty scaffold",
    "always 0",
    "no contact work",
    "no broad phase",
    "no narrow phase",
    "no contact solver",
    "phase 1",
];

/// The module's `src` directory, resolved from the crate manifest.
fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `*.rs` file directly under `src/`, sorted by file name for determinism.
fn src_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(src_dir())
        .expect("src directory must be readable")
        .map(|entry| entry.expect("dir entry must be readable").path())
        .filter(|path| path.extension().map(|e| e == "rs").unwrap_or(false))
        .collect();
    files.sort();
    files
}

#[test]
fn source_docs_contain_no_stale_phase1_claims() {
    let files = src_files();
    assert!(!files.is_empty(), "expected to find src/*.rs files to scan");

    for path in files {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
            .to_lowercase();
        for phrase in STALE_PHRASES {
            assert!(
                !text.contains(phrase),
                "{} contains the stale phrase {phrase:?}; rewrite the doc comment \
                 to describe the real current pipeline (see CLAUDE.md / ROADMAP.md)",
                path.display()
            );
        }
    }
}

#[test]
fn source_docs_describe_the_real_pipeline() {
    let lib = fs::read_to_string(src_dir().join("lib.rs"))
        .expect("lib.rs must be readable")
        .to_lowercase();
    assert!(
        !lib.contains("empty scaffold"),
        "lib.rs still calls the collision pipeline an empty scaffold"
    );
    assert!(
        lib.contains("broad phase") || lib.contains("solver"),
        "lib.rs should describe the real pipeline (e.g. mention the broad phase or solver)"
    );
}
