//! The neutral command model: a parsed command line and a command outcome.
//!
//! Parsing is branchless and safe — trim, split on whitespace, first token is the
//! name. There is no `eval`/dynamic execution; a name either matches a registered
//! entry in a [`crate::command_table::CommandTable`] or it does not.

/// A parsed console line: a command name plus whitespace-separated arguments.
/// Public so a consumer can parse input and feed it to a
/// [`crate::CommandTable`]; the fields stay crate-private (build one via
/// [`Self::parse`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub(crate) name: String,
    pub(crate) args: Vec<String>,
}

impl ParsedCommand {
    /// Parse a raw line. `None` for empty / whitespace-only input.
    pub fn parse(raw: &str) -> Option<ParsedCommand> {
        let mut tokens = raw.split_whitespace();
        tokens.next().map(|name| ParsedCommand {
            name: name.to_string(),
            args: tokens.map(str::to_string).collect(),
        })
    }

    /// The canonical `name arg1 arg2` form — the history entry for a submission.
    pub fn canonical(&self) -> String {
        let mut out = self.name.clone();
        self.args.iter().for_each(|arg| {
            out.push(' ');
            out.push_str(arg);
        });
        out
    }
}

/// The outcome of dispatching one command: which command, success, and message.
/// Returned by command handlers and read back by the consumer when echoing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    command: String,
    ok: bool,
    message: String,
}

impl CommandOutcome {
    pub fn ok(command: impl Into<String>, message: impl Into<String>) -> Self {
        CommandOutcome {
            command: command.into(),
            ok: true,
            message: message.into(),
        }
    }

    pub fn error(command: impl Into<String>, message: impl Into<String>) -> Self {
        CommandOutcome {
            command: command.into(),
            ok: false,
            message: message.into(),
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn succeeded(&self) -> bool {
        self.ok
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_name_and_args_and_trims() {
        let p = ParsedCommand::parse("  perf.mark  frame budget ").unwrap();
        assert_eq!(p.name, "perf.mark");
        assert_eq!(p.args, vec!["frame", "budget"]);
        assert!(ParsedCommand::parse("solo").unwrap().args.is_empty());
    }

    #[test]
    fn empty_or_whitespace_is_none() {
        assert_eq!(ParsedCommand::parse(""), None);
        assert_eq!(ParsedCommand::parse("  \t\n "), None);
    }

    #[test]
    fn canonical_round_trips() {
        assert_eq!(ParsedCommand::parse("a b c").unwrap().canonical(), "a b c");
        assert_eq!(ParsedCommand::parse("solo").unwrap().canonical(), "solo");
    }

    #[test]
    fn outcome_carries_command_ok_and_message() {
        let good = CommandOutcome::ok("help", "commands: …");
        assert_eq!(good.command(), "help");
        assert!(good.succeeded());
        assert_eq!(good.message(), "commands: …");
        let bad = CommandOutcome::error("nope", "unknown");
        assert!(!bad.succeeded());
    }
}
