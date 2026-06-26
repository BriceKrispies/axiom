//! [`CommandTable`] — a generic, static command-dispatch shape (no `eval`).
//!
//! A table is a fixed `&'static` slice of `(name, summary, handler)` specs over a
//! caller-chosen context type `C`. Dispatch is a name lookup; an unknown name
//! yields a clean error outcome. The *commands* themselves (their names and
//! handlers) belong to the consumer — the debug overlay supplies a
//! `CommandTable<OverlayState>` with its debug commands; this layer owns only the
//! reusable dispatch shape.

use crate::interface_command::{CommandOutcome, ParsedCommand};

/// One registered command over a caller-chosen context `C`: its name, a one-line
/// summary, and the handler that runs it.
pub struct CommandSpec<C> {
    pub name: &'static str,
    pub summary: &'static str,
    pub handler: fn(&mut C, &[String]) -> CommandOutcome,
}

// Manual `Debug` (a `CommandSpec` holds no `C`, so no `C: Debug` bound).
impl<C> core::fmt::Debug for CommandSpec<C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommandSpec")
            .field("name", &self.name)
            .field("summary", &self.summary)
            .finish()
    }
}

/// A dispatcher over a static command table.
pub struct CommandTable<C: 'static> {
    specs: &'static [CommandSpec<C>],
}

impl<C> core::fmt::Debug for CommandTable<C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommandTable")
            .field("commands", &self.specs.len())
            .finish()
    }
}

impl<C: 'static> CommandTable<C> {
    pub fn new(specs: &'static [CommandSpec<C>]) -> Self {
        CommandTable { specs }
    }

    /// Run one already-parsed command against `ctx`; an unknown name yields a
    /// clean error. Branchless `find`/`map`/`unwrap_or_else`.
    pub fn dispatch(&self, ctx: &mut C, command: &ParsedCommand) -> CommandOutcome {
        self.specs
            .iter()
            .find(|spec| spec.name == command.name)
            .map(|spec| (spec.handler)(ctx, &command.args))
            .unwrap_or_else(|| {
                CommandOutcome::error(
                    command.name.clone(),
                    format!("unknown command `{}` — type `help`", command.name),
                )
            })
    }

    /// Every command's `(name, summary)`, in declaration order (for a `help`).
    pub fn summaries(&self) -> Vec<(&'static str, &'static str)> {
        self.specs
            .iter()
            .map(|spec| (spec.name, spec.summary))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ctx {
        count: i64,
    }

    fn cmd_inc(ctx: &mut Ctx, args: &[String]) -> CommandOutcome {
        ctx.count += 1;
        CommandOutcome::ok("inc", format!("count={} args={}", ctx.count, args.len()))
    }

    fn cmd_reset(ctx: &mut Ctx, _args: &[String]) -> CommandOutcome {
        ctx.count = 0;
        CommandOutcome::ok("reset", "")
    }

    const TEST_SPECS: &[CommandSpec<Ctx>] = &[
        CommandSpec {
            name: "inc",
            summary: "increment",
            handler: cmd_inc,
        },
        CommandSpec {
            name: "reset",
            summary: "reset",
            handler: cmd_reset,
        },
    ];

    #[test]
    fn dispatch_runs_the_matching_handler() {
        let table = CommandTable::new(TEST_SPECS);
        let mut ctx = Ctx { count: 0 };
        let out = table.dispatch(&mut ctx, &ParsedCommand::parse("inc x y").unwrap());
        assert!(out.succeeded());
        assert_eq!(ctx.count, 1);
        assert!(out.message().contains("args=2"));
        table.dispatch(&mut ctx, &ParsedCommand::parse("reset").unwrap());
        assert_eq!(ctx.count, 0);
    }

    #[test]
    fn unknown_command_is_a_clean_error() {
        let table = CommandTable::new(TEST_SPECS);
        let mut ctx = Ctx { count: 0 };
        let out = table.dispatch(&mut ctx, &ParsedCommand::parse("nope").unwrap());
        assert!(!out.succeeded());
        assert_eq!(out.command(), "nope");
        assert!(out.message().contains("unknown command"));
    }

    #[test]
    fn summaries_list_every_command() {
        let table = CommandTable::new(TEST_SPECS);
        assert_eq!(
            table.summaries(),
            vec![("inc", "increment"), ("reset", "reset")]
        );
    }

    #[test]
    fn spec_and_table_are_debug() {
        assert!(format!("{:?}", TEST_SPECS[0]).contains("CommandSpec"));
        assert!(format!("{:?}", TEST_SPECS[0]).contains("inc"));
        assert!(format!("{:?}", CommandTable::new(TEST_SPECS)).contains("CommandTable"));
    }
}
