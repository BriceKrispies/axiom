//! The debug overlay's command set — the **debug-specific** specs and handlers.
//!
//! The generic dispatch *shape* (`CommandTable`/`CommandSpec`, parse, outcome)
//! lives in `axiom-interface`; this file supplies the concrete commands over the
//! overlay's own [`OverlayState`] context. The overlay's `submit_command` runs
//! these through a `CommandTable<OverlayState>`.

use axiom_interface::{CommandOutcome, CommandSpec};

use crate::overlay_density::OverlayDensity;
use crate::overlay_state::OverlayState;

/// The overlay's command table entries.
pub(crate) const OVERLAY_SPECS: &[CommandSpec<OverlayState>] = &[
    CommandSpec {
        name: "help",
        summary: "list the available commands",
        handler: cmd_help,
    },
    CommandSpec {
        name: "clear",
        summary: "clear the command output",
        handler: cmd_clear,
    },
    CommandSpec {
        name: "overlay.compact",
        summary: "set density to compact",
        handler: cmd_compact,
    },
    CommandSpec {
        name: "overlay.normal",
        summary: "set density to normal",
        handler: cmd_normal,
    },
    CommandSpec {
        name: "overlay.verbose",
        summary: "set density to verbose",
        handler: cmd_verbose,
    },
    CommandSpec {
        name: "overlay.pin",
        summary: "pin the overlay open",
        handler: cmd_pin,
    },
    CommandSpec {
        name: "overlay.unpin",
        summary: "unpin the overlay",
        handler: cmd_unpin,
    },
    CommandSpec {
        name: "overlay.hide",
        summary: "hide the overlay",
        handler: cmd_hide,
    },
    CommandSpec {
        name: "diagnostics.snapshot",
        summary: "print a text snapshot of the current diagnostics",
        handler: cmd_diagnostics_snapshot,
    },
    CommandSpec {
        name: "backend.report",
        summary: "print the renderer/canvas/sim/storage/audio/network backends",
        handler: cmd_backend_report,
    },
    CommandSpec {
        name: "copy",
        summary: "copy text to the clipboard (default: the diagnostics snapshot)",
        handler: cmd_copy,
    },
    CommandSpec {
        name: "replay.mark",
        summary: "record a replay marker (stub)",
        handler: cmd_replay_mark,
    },
    CommandSpec {
        name: "perf.mark",
        summary: "record a performance marker (stub)",
        handler: cmd_perf_mark,
    },
];

fn cmd_help(_state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    let lines = OVERLAY_SPECS
        .iter()
        .map(|spec| format!("{} — {}", spec.name, spec.summary))
        .collect::<Vec<_>>()
        .join("\n");
    CommandOutcome::ok("help", format!("commands:\n{lines}"))
}

fn cmd_clear(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    state.clear_console_results();
    CommandOutcome::ok("clear", String::new())
}

fn cmd_compact(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    state.set_density(OverlayDensity::Compact);
    CommandOutcome::ok("overlay.compact", "density set to compact")
}

fn cmd_normal(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    state.set_density(OverlayDensity::Normal);
    CommandOutcome::ok("overlay.normal", "density set to normal")
}

fn cmd_verbose(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    state.set_density(OverlayDensity::Verbose);
    CommandOutcome::ok("overlay.verbose", "density set to verbose")
}

fn cmd_pin(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    state.pin();
    CommandOutcome::ok("overlay.pin", "overlay pinned")
}

fn cmd_unpin(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    state.unpin();
    CommandOutcome::ok("overlay.unpin", "overlay unpinned")
}

fn cmd_hide(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    state.hide();
    CommandOutcome::ok("overlay.hide", "overlay hidden")
}

fn cmd_diagnostics_snapshot(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    CommandOutcome::ok("diagnostics.snapshot", state.diagnostics().snapshot_text())
}

fn cmd_backend_report(state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    CommandOutcome::ok("backend.report", state.diagnostics().backend_report_text())
}

/// `copy` queues text into the interface's neutral clipboard outbox; the wasm
/// arm drains it and writes to `navigator.clipboard`. Bare `copy` copies the
/// diagnostics snapshot; `copy <text…>` copies the literal joined arguments.
/// Branchless: the joined args are chosen when present, else the snapshot.
fn cmd_copy(state: &mut OverlayState, args: &[String]) -> CommandOutcome {
    let text_opt = (!args.is_empty()).then(|| args.join(" "));
    let text = text_opt.unwrap_or_else(|| state.diagnostics().snapshot_text());
    let count = text.chars().count();
    state.request_clipboard(text);
    CommandOutcome::ok("copy", format!("copied {count} chars to the clipboard"))
}

fn cmd_replay_mark(_state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    CommandOutcome::ok(
        "replay.mark",
        "replay marker acknowledged (stub — would be recorded later)",
    )
}

fn cmd_perf_mark(_state: &mut OverlayState, _args: &[String]) -> CommandOutcome {
    CommandOutcome::ok(
        "perf.mark",
        "perf marker acknowledged (stub — would be recorded later)",
    )
}
