//! The console's command registry and dispatcher.
//!
//! Commands are a fixed, static table of `(name, summary, handler)` specs. There
//! is **no** `eval`, `Function` constructor, dynamic import, or arbitrary script
//! execution: dispatch is a name lookup against [`SPECS`], and an unmatched name
//! returns a clean error result. Each handler is an ordinary Rust `fn` that reads
//! and mutates the pure [`OverlayState`] and returns a deterministic
//! [`CommandResult`]. Today the side-effecting commands are real (they change
//! density / pin / visibility / the log); the reporting commands return stub
//! text routed through this same real pipeline.

use crate::debug_command::{CommandResult, ParsedCommand};
use crate::debug_overlay_density::OverlayDensity;
use crate::debug_overlay_state::OverlayState;

/// One registered command: its name, a one-line summary (for `help`), and the
/// handler that runs it against the overlay state.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub handler: fn(&mut OverlayState, &[String]) -> CommandResult,
}

/// The complete command set. Static, so it cannot be extended at runtime by
/// untrusted input — the registry is a closed list, not an interpreter.
pub const SPECS: &[CommandSpec] = &[
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
        summary: "set overlay density to compact",
        handler: cmd_overlay_compact,
    },
    CommandSpec {
        name: "overlay.normal",
        summary: "set overlay density to normal",
        handler: cmd_overlay_normal,
    },
    CommandSpec {
        name: "overlay.verbose",
        summary: "set overlay density to verbose",
        handler: cmd_overlay_verbose,
    },
    CommandSpec {
        name: "overlay.pin",
        summary: "pin the overlay open",
        handler: cmd_overlay_pin,
    },
    CommandSpec {
        name: "overlay.unpin",
        summary: "unpin the overlay",
        handler: cmd_overlay_unpin,
    },
    CommandSpec {
        name: "overlay.hide",
        summary: "hide the overlay",
        handler: cmd_overlay_hide,
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

/// A dispatcher over a static command table.
#[derive(Debug, Clone, Copy)]
pub struct CommandRegistry {
    specs: &'static [CommandSpec],
}

impl Default for CommandRegistry {
    fn default() -> Self {
        CommandRegistry::standard()
    }
}

impl CommandRegistry {
    /// The registry over the standard [`SPECS`] command set.
    pub fn standard() -> Self {
        CommandRegistry { specs: SPECS }
    }

    /// The registered command names, in declaration order.
    pub fn names(&self) -> Vec<&'static str> {
        self.specs.iter().map(|s| s.name).collect()
    }

    /// Run one already-parsed command. An unknown name yields a clean error
    /// result rather than panicking or doing anything dynamic.
    pub fn dispatch(&self, state: &mut OverlayState, command: &ParsedCommand) -> CommandResult {
        match self.specs.iter().find(|s| s.name == command.name) {
            Some(spec) => (spec.handler)(state, &command.args),
            None => CommandResult::error(
                command.name.clone(),
                format!("unknown command `{}` — type `help`", command.name),
            ),
        }
    }

    /// Parse, record, dispatch, and echo a raw console line.
    ///
    /// * Empty / whitespace-only input is ignored entirely — `None`, no history
    ///   entry, no output noise.
    /// * A submitted non-empty command is recorded in history, dispatched, and
    ///   its result appended to the console log — unless the result message is
    ///   empty (how `clear` leaves the log empty without echoing itself).
    pub fn execute(&self, state: &mut OverlayState, raw: &str) -> Option<CommandResult> {
        let parsed = ParsedCommand::parse(raw)?;
        state.console_mut().record(&parsed.canonical());
        let result = self.dispatch(state, &parsed);
        if !result.message.is_empty() {
            state.console_mut().append_result(result.clone());
        }
        Some(result)
    }
}

// --- handlers ---------------------------------------------------------------
// Each is a deterministic `fn`. Side-effecting commands mutate `state`; reporting
// commands read it. No handler performs any dynamic execution.

fn cmd_help(_state: &mut OverlayState, _args: &[String]) -> CommandResult {
    let names = SPECS
        .iter()
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(", ");
    CommandResult::ok("help", format!("commands: {names}"))
}

fn cmd_clear(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    state.console_mut().clear_results();
    // An empty message is the signal to `execute` not to echo this back —
    // otherwise clearing would immediately re-populate the log with itself.
    CommandResult::ok("clear", String::new())
}

fn cmd_overlay_compact(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    state.set_density(OverlayDensity::Compact);
    CommandResult::ok("overlay.compact", "density set to compact")
}

fn cmd_overlay_normal(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    state.set_density(OverlayDensity::Normal);
    CommandResult::ok("overlay.normal", "density set to normal")
}

fn cmd_overlay_verbose(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    state.set_density(OverlayDensity::Verbose);
    CommandResult::ok("overlay.verbose", "density set to verbose")
}

fn cmd_overlay_pin(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    state.pin();
    CommandResult::ok("overlay.pin", "overlay pinned")
}

fn cmd_overlay_unpin(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    state.unpin();
    CommandResult::ok("overlay.unpin", "overlay unpinned")
}

fn cmd_overlay_hide(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    state.hide();
    CommandResult::ok("overlay.hide", "overlay hidden")
}

fn cmd_diagnostics_snapshot(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    CommandResult::ok("diagnostics.snapshot", state.diagnostics().snapshot_text())
}

fn cmd_backend_report(state: &mut OverlayState, _args: &[String]) -> CommandResult {
    CommandResult::ok("backend.report", state.diagnostics().backend_report_text())
}

fn cmd_replay_mark(_state: &mut OverlayState, _args: &[String]) -> CommandResult {
    // Stub: a real build would hand this to the recording module. The pipeline
    // (parse → dispatch → result) is already real and tested.
    CommandResult::ok(
        "replay.mark",
        "replay marker acknowledged (stub — would be recorded later)",
    )
}

fn cmd_perf_mark(_state: &mut OverlayState, _args: &[String]) -> CommandResult {
    CommandResult::ok(
        "perf.mark",
        "perf marker acknowledged (stub — would be recorded later)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(state: &mut OverlayState, raw: &str) -> CommandResult {
        CommandRegistry::standard().execute(state, raw).expect("non-empty command")
    }

    #[test]
    fn help_lists_every_command() {
        let mut s = OverlayState::new();
        let r = run(&mut s, "help");
        assert!(r.ok);
        for name in CommandRegistry::standard().names() {
            assert!(r.message.contains(name), "help should mention `{name}`");
        }
    }

    #[test]
    fn clear_empties_the_log_without_echoing_itself() {
        let mut s = OverlayState::new();
        run(&mut s, "help"); // log now has one line
        assert_eq!(s.recent_results().len(), 1);
        let r = run(&mut s, "clear");
        assert!(r.ok);
        // Cleared, and `clear` did not re-add a line.
        assert!(s.recent_results().is_empty());
    }

    #[test]
    fn overlay_density_commands_set_density() {
        let mut s = OverlayState::new();
        run(&mut s, "overlay.compact");
        assert_eq!(s.density(), OverlayDensity::Compact);
        run(&mut s, "overlay.verbose");
        assert_eq!(s.density(), OverlayDensity::Verbose);
        run(&mut s, "overlay.normal");
        assert_eq!(s.density(), OverlayDensity::Normal);
    }

    #[test]
    fn overlay_pin_and_unpin_commands_toggle_the_pin() {
        let mut s = OverlayState::new();
        run(&mut s, "overlay.pin");
        assert!(s.is_pinned() && s.is_visible());
        run(&mut s, "overlay.unpin");
        assert!(!s.is_pinned());
    }

    #[test]
    fn overlay_hide_command_hides_even_when_pinned() {
        let mut s = OverlayState::new();
        run(&mut s, "overlay.pin");
        run(&mut s, "overlay.hide");
        assert!(!s.is_visible());
    }

    #[test]
    fn reporting_commands_return_stub_text() {
        let mut s = OverlayState::new();
        assert!(run(&mut s, "diagnostics.snapshot").message.contains("renderer=webgpu"));
        assert!(run(&mut s, "backend.report").message.contains("canvas:   axiom-windowing"));
        assert!(run(&mut s, "replay.mark").message.contains("stub"));
        assert!(run(&mut s, "perf.mark").message.contains("stub"));
    }

    #[test]
    fn unknown_command_returns_a_clean_error() {
        let mut s = OverlayState::new();
        let r = run(&mut s, "frobnicate the cubes");
        assert!(!r.ok);
        assert_eq!(r.command, "frobnicate");
        assert!(r.message.contains("unknown command"));
        // An unknown command still records to history and echoes its error.
        assert_eq!(s.command_history_count(), 1);
        assert_eq!(s.recent_results().len(), 1);
    }

    #[test]
    fn empty_commands_are_ignored_completely() {
        let mut s = OverlayState::new();
        assert!(CommandRegistry::standard().execute(&mut s, "").is_none());
        assert!(CommandRegistry::standard().execute(&mut s, "   ").is_none());
        // No history, no output noise.
        assert_eq!(s.command_history_count(), 0);
        assert!(s.recent_results().is_empty());
    }

    #[test]
    fn submitting_a_command_records_it_in_history() {
        let mut s = OverlayState::new();
        run(&mut s, "overlay.verbose");
        run(&mut s, "help");
        assert_eq!(s.command_history_count(), 2);
    }
}
