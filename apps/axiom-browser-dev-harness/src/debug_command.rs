//! The console's command value types: a parsed command line and a command
//! result.
//!
//! Parsing is deliberately tiny and safe: trim the raw line, split on
//! whitespace, take the first token as the command name and the rest as
//! arguments. There is **no** `eval`, no dynamic dispatch over arbitrary text —
//! a name either matches a registered command (see
//! [`crate::debug_command_registry`]) or it does not.

/// A parsed console line: a command name plus its whitespace-separated arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
}

impl ParsedCommand {
    /// Parse a raw console line. Returns `None` for empty or whitespace-only
    /// input (an empty command does nothing and adds no console noise).
    pub fn parse(raw: &str) -> Option<ParsedCommand> {
        let mut tokens = raw.split_whitespace();
        let name = tokens.next()?.to_string();
        let args = tokens.map(str::to_string).collect();
        Some(ParsedCommand { name, args })
    }

    /// The canonical single-line form (`name arg1 arg2`), used as the history
    /// entry for a submitted command.
    pub fn canonical(&self) -> String {
        let mut out = self.name.clone();
        self.args.iter().for_each(|a| {
            out.push(' ');
            out.push_str(a);
        });
        out
    }
}

/// The outcome of dispatching one command. `command` echoes the resolved command
/// name, `ok` is the success flag, and `message` is the (deterministic) text the
/// console renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub command: String,
    pub ok: bool,
    pub message: String,
}

impl CommandResult {
    /// A successful result.
    pub fn ok(command: impl Into<String>, message: impl Into<String>) -> Self {
        CommandResult {
            command: command.into(),
            ok: true,
            message: message.into(),
        }
    }

    /// A failed result (e.g. an unknown command).
    pub fn error(command: impl Into<String>, message: impl Into<String>) -> Self {
        CommandResult {
            command: command.into(),
            ok: false,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_name_and_args() {
        let p = ParsedCommand::parse("overlay.compact").unwrap();
        assert_eq!(p.name, "overlay.compact");
        assert!(p.args.is_empty());

        let p = ParsedCommand::parse("perf.mark frame budget").unwrap();
        assert_eq!(p.name, "perf.mark");
        assert_eq!(p.args, vec!["frame", "budget"]);
    }

    #[test]
    fn parse_trims_and_collapses_whitespace() {
        let p = ParsedCommand::parse("   help    me   ").unwrap();
        assert_eq!(p.name, "help");
        assert_eq!(p.args, vec!["me"]);
    }

    #[test]
    fn empty_or_whitespace_parses_to_none() {
        assert_eq!(ParsedCommand::parse(""), None);
        assert_eq!(ParsedCommand::parse("   "), None);
        assert_eq!(ParsedCommand::parse("\t\n "), None);
    }

    #[test]
    fn canonical_round_trips_name_and_args() {
        assert_eq!(ParsedCommand::parse("a b c").unwrap().canonical(), "a b c");
        assert_eq!(ParsedCommand::parse("solo").unwrap().canonical(), "solo");
    }

    #[test]
    fn result_constructors_set_the_ok_flag() {
        let good = CommandResult::ok("help", "commands: …");
        assert!(good.ok);
        assert_eq!(good.command, "help");

        let bad = CommandResult::error("nope", "unknown command");
        assert!(!bad.ok);
        assert_eq!(bad.message, "unknown command");
    }
}
