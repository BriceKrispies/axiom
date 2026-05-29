//! `cargo xtask` entry point. Today it offers one command:
//!
//! ```text
//! cargo xtask check-architecture [--root <path>]
//! ```
//!
//! It enforces the Axiom Layer Law (see repo-root `CLAUDE.md`).

use std::path::PathBuf;
use std::process::ExitCode;

use xtask::check::check_architecture;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("check-architecture") => run_check(&args[1..]),
        Some(other) => {
            eprintln!("xtask: unknown command `{other}`");
            print_usage();
            ExitCode::from(2)
        }
        None => {
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!("usage: cargo xtask check-architecture [--root <path>]");
}

fn run_check(rest: &[String]) -> ExitCode {
    let root = match parse_root(rest) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("xtask: {message}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    println!("Axiom architecture check — root: {}", root.display());
    let report = check_architecture(&root);

    if report.layers_checked.is_empty() {
        println!("No layer manifests (crates/*/layer.toml) found. Nothing to check.");
    } else {
        println!("Layers checked: {}", report.layers_checked.join(" -> "));
    }

    if report.is_ok() {
        println!("OK: all layers satisfy the Axiom Layer Law.");
        ExitCode::SUCCESS
    } else {
        let violations = report.violations();
        eprintln!("\nFAIL: {} architecture violation(s):", violations.len());
        for v in violations {
            eprintln!("  - {v}");
        }
        eprintln!("\nSee CLAUDE.md for the Axiom Layer Law and how to fix these.");
        ExitCode::FAILURE
    }
}

/// Parse the optional `--root <path>` flag, defaulting to the repo root inferred
/// from this crate's location (so the command is cwd-independent).
fn parse_root(rest: &[String]) -> Result<PathBuf, String> {
    let mut iter = rest.iter();
    let mut root: Option<PathBuf> = None;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--root" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--root requires a path argument".to_string())?;
                root = Some(PathBuf::from(value));
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    Ok(root.unwrap_or_else(default_repo_root))
}

/// `crates/xtask` -> repo root is two levels up.
fn default_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
