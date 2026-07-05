//! `cargo xtask` entry point. Today it offers one command:
//!
//! ```text
//! cargo xtask check-architecture       [--root <path>]
//! cargo xtask check-slices             [--root <path>]
//! cargo xtask check-slice-placement    [--root <path>]
//! ```
//!
//! `check-architecture` enforces the Axiom Layer/Module Laws.
//! `check-slices` enforces the semantic vertical-slice contract (`slice.toml`).
//! `check-slice-placement` flags engine render logic hiding in an app/game.
//! See repo-root `CLAUDE.md` and `docs/audits/vertical-slice-audit.md`.

use std::path::PathBuf;
use std::process::ExitCode;

use xtask::check::check_architecture;
use xtask::slice_check::{check_slice_placement, check_slices};
use xtask::violation::CheckReport;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `check-architecture` runs the check; an unknown command names itself in
    // the error; no command just prints usage.
    args.first().map(String::as_str).map_or_else(
        || {
            print_usage();
            ExitCode::from(2)
        },
        |cmd| {
            let checked = match cmd {
                "check-architecture" => Some(run_check(&args[1..])),
                "check-slices" => Some(run_slice_check(&args[1..], SliceCommand::Slices)),
                "check-slice-placement" => {
                    Some(run_slice_check(&args[1..], SliceCommand::Placement))
                }
                _ => None,
            };
            checked.unwrap_or_else(|| {
                eprintln!("xtask: unknown command `{cmd}`");
                print_usage();
                ExitCode::from(2)
            })
        },
    )
}

fn print_usage() {
    eprintln!(
        "usage: cargo xtask <check-architecture|check-slices|check-slice-placement> [--root <path>]"
    );
}

/// Which semantic slice check to run.
#[derive(Clone, Copy)]
enum SliceCommand {
    Slices,
    Placement,
}

impl SliceCommand {
    fn label(self) -> &'static str {
        match self {
            SliceCommand::Slices => "slice contract",
            SliceCommand::Placement => "slice placement",
        }
    }

    fn run(self, root: &PathBuf, report: &mut CheckReport) {
        match self {
            SliceCommand::Slices => check_slices(root, report),
            SliceCommand::Placement => check_slice_placement(root, report),
        }
    }
}

fn run_slice_check(rest: &[String], command: SliceCommand) -> ExitCode {
    parse_root(rest).map_or_else(
        |message| {
            eprintln!("xtask: {message}");
            print_usage();
            ExitCode::from(2)
        },
        |root| {
            println!(
                "Axiom {} check — root: {}",
                command.label(),
                root.display()
            );
            let mut report = CheckReport::default();
            command.run(&root, &mut report);
            let report = report.finish();

            report.is_ok().then_some(()).map_or_else(
                || {
                    let violations = report.violations();
                    eprintln!("\nFAIL: {} {} violation(s):", violations.len(), command.label());
                    violations.iter().for_each(|v| eprintln!("  - {v}"));
                    ExitCode::FAILURE
                },
                |()| {
                    println!("OK: the Axiom {} checks pass.", command.label());
                    ExitCode::SUCCESS
                },
            )
        },
    )
}

fn run_check(rest: &[String]) -> ExitCode {
    parse_root(rest).map_or_else(
        |message| {
            eprintln!("xtask: {message}");
            print_usage();
            ExitCode::from(2)
        },
        |root| {
            println!("Axiom architecture check — root: {}", root.display());
            let report = check_architecture(&root);

            let summary = report.layers_checked.is_empty().then(|| {
                "No layer manifests (crates/*/layer.toml) found. Nothing to check.".to_string()
            });
            let summary = summary.unwrap_or_else(|| {
                format!("Layers checked: {}", report.layers_checked.join(" -> "))
            });
            println!("{summary}");

            report.is_ok().then_some(()).map_or_else(
                || {
                    let violations = report.violations();
                    eprintln!("\nFAIL: {} architecture violation(s):", violations.len());
                    violations.iter().for_each(|v| eprintln!("  - {v}"));
                    eprintln!("\nSee CLAUDE.md for the Axiom Layer Law and how to fix these.");
                    ExitCode::FAILURE
                },
                |()| {
                    println!("OK: all layers satisfy the Axiom Layer Law.");
                    ExitCode::SUCCESS
                },
            )
        },
    )
}

/// Parse the optional `--root <path>` flag, defaulting to the repo root inferred
/// from this crate's location (so the command is cwd-independent).
fn parse_root(rest: &[String]) -> Result<PathBuf, String> {
    // A tiny state machine folded over the args. State carries the root chosen
    // so far and whether the previous arg (`--root`) still needs its value.
    struct State {
        root: Option<PathBuf>,
        awaiting_value: bool,
    }
    let initial: Result<State, String> = Ok(State {
        root: None,
        awaiting_value: false,
    });

    rest.iter()
        .fold(initial, |state, arg| {
            state.and_then(|state| {
                // When awaiting a value, this arg is the `--root` path; else this
                // arg must itself be `--root`, otherwise it is unexpected.
                let as_value = state.awaiting_value.then(|| {
                    Ok(State {
                        root: Some(PathBuf::from(arg)),
                        awaiting_value: false,
                    })
                });
                as_value.unwrap_or_else(|| {
                    let as_flag = (arg.as_str() == "--root").then(|| {
                        Ok(State {
                            root: state.root.clone(),
                            awaiting_value: true,
                        })
                    });
                    as_flag.unwrap_or_else(|| Err(format!("unexpected argument `{arg}`")))
                })
            })
        })
        .and_then(|state| {
            // A dangling `--root` with no following value is an error.
            let dangling = state
                .awaiting_value
                .then(|| Err("--root requires a path argument".to_string()));
            dangling.unwrap_or_else(|| Ok(state.root.unwrap_or_else(default_repo_root)))
        })
}

/// `crates/xtask` -> repo root is two levels up.
fn default_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
