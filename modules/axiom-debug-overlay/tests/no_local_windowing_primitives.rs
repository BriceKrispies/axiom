//! Migration guard: the debug overlay must **compose** `axiom-interface` for its
//! generic windowing, never re-grow its own copy.
//!
//! After the `axiom-interface` migration, the overlay owns only debug-specific
//! code (density, diagnostics, the command *set*, the Backquote *binding*). The
//! generic primitives — drag/layout, the console model, key chords, the parsed
//! command, the command result, the command registry, and the label/value row —
//! live in the layer. This source-scan fails if any of those primitive *types*
//! is re-declared in the overlay's `src/`. Referencing the layer's imported types
//! (e.g. `&ParsedCommand`, `CommandOutcome`) is fine — only a local `struct`/`enum`
//! declaration of the moved concept is forbidden.

use std::fs;
use std::path::Path;

/// Type declarations that moved to `axiom-interface` and must not reappear here.
const FORBIDDEN_DECLARATIONS: &[&str] = &[
    "struct DragState",
    "struct ConsoleState",
    "struct KeyChord",
    "struct ParsedCommand",
    "struct CommandResult",
    "struct CommandRegistry",
    "struct Row",
    "enum ConsoleKey",
];

/// Strip `//` line comments so a doc/comment mention can neither mask nor fake a
/// violation (mirrors the architecture checker's text scan).
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_src_files() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for entry in fs::read_dir(&src).expect("overlay src/ is readable") {
        let path = entry.expect("dir entry").path();
        let is_rust = path.extension().is_some_and(|ext| ext == "rs");
        if is_rust {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let body = fs::read_to_string(&path).expect("read source file");
            files.push((name, strip_line_comments(&body)));
        }
    }
    assert!(!files.is_empty(), "expected overlay src/ to contain Rust files");
    files
}

#[test]
fn overlay_declares_no_generic_windowing_primitive() {
    let files = read_src_files();
    for (name, body) in &files {
        for forbidden in FORBIDDEN_DECLARATIONS {
            assert!(
                !body.contains(forbidden),
                "{name} re-declares `{forbidden}` — that generic windowing primitive \
                 belongs to axiom-interface; compose the layer instead of re-growing it"
            );
        }
    }
}

#[test]
fn the_deleted_primitive_modules_are_gone() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for stale in ["drag.rs", "console.rs", "command.rs", "command_registry.rs", "keyboard.rs"] {
        assert!(
            !src.join(stale).exists(),
            "{stale} should have been deleted — its concern moved to axiom-interface"
        );
    }
}

#[test]
fn overlay_actually_composes_the_interface_layer() {
    let files = read_src_files();
    let references_layer = files.iter().any(|(_, body)| body.contains("axiom_interface"));
    assert!(
        references_layer,
        "the overlay must compose axiom-interface — no source file references the layer"
    );
}
