//! **The edit script** — a batch of edits with no escaping anywhere in it.
//!
//! `ax apply` has always taken JSON, and JSON was the wrong wire format for a
//! payload that is *source code*. Every newline becomes `\n`, every quote
//! becomes `\"`, every backslash doubles — so an agent cannot paste the code it
//! means to write, it has to transform it first, and a transform that happens
//! by hand is a transform that goes wrong. The measured cost on this repo: a
//! four-edit batch took three staging files and a JSON index to express, and
//! the standing advice for anything containing a backslash was to route it
//! through a file rather than trust the shell.
//!
//! So `ax apply` now also reads this, and nothing in it is ever escaped:
//!
//! ```text
//! # comments are allowed between blocks
//! edit tools/axiom-atlas/src/main.rs
//! replace <<RS
//! mod search;
//! mod symbols;
//! RS
//! with <<RS
//! mod search;
//! mod symbols;
//! mod wgsl;
//! RS
//!
//! edit .gitattributes
//! append <<TXT
//! *.wgsl text eol=lf
//! TXT
//!
//! edit apps/x/src/main.rs
//! replace: "sql" => cmd_sql(&repo, &args, &mut rec),
//! with: "sql" => cmd_sql(&repo, &args, &mut rec).map(track),
//! ```
//!
//! The payload between a fence and its closing tag is taken **verbatim**. There
//! is no escape character, so there is nothing to get wrong: a line of Rust
//! full of quotes, backslashes and braces is copied in as it stands.
//!
//! # Three ways to give a payload, and when each is right
//!
//! | Form | Payload | Use it for |
//! |---|---|---|
//! | `name <<TAG` … `TAG` | the lines between, each newline-terminated | multi-line code — the common case |
//! | `name: text` | the rest of the line, no trailing newline | a one-line anchor, or a line fragment |
//! | `name < path` | the contents of a repo file | something very large |
//!
//! The trailing-newline split is deliberate. A fenced payload is whole lines,
//! so it keeps its final newline and a `replace`/`with` pair of whole lines
//! behaves as written. An inline payload is a fragment, so it does not — which
//! is what you want when the anchor is part of a line, or sits at end of file.
//! `<<-TAG` gives you a chomped fenced payload for the rare case that needs
//! both multiple lines and no trailing newline.
//!
//! # What it is not
//!
//! It is not a diff. A diff needs hunk headers and exact line counts, which is
//! more for a writer to get right than an anchor is, and it goes stale the
//! moment the file moves. Anchors say what to look for; the tool says whether
//! it found it exactly once. That guard is the reason to use `ax apply` at all,
//! and this format keeps it.

use crate::apply::EditOp;
use crate::eol;

/// Directives that begin an operation. A block may carry exactly one, because
/// two would leave which one runs up to the order of fields in a struct.
const PRIMARY: &[&str] = &[
    "replace",
    "from",
    "insert_before",
    "insert_after",
    "append",
    "content",
];

/// Directives that supply an operation's second half.
const SECONDARY: &[&str] = &["with", "to", "through", "text"];

/// True when the text is an edit script rather than a JSON batch.
///
/// JSON is an array, so it starts with `[`. Anything else is a script. This is
/// the whole detection rule: no flag, no file extension, no ambiguity — a
/// caller that was piping JSON yesterday keeps working today.
pub fn looks_like_script(text: &str) -> bool {
    !text.trim_start().starts_with('[')
}

/// One block, mid-parse.
#[derive(Default)]
struct Block {
    path: String,
    line: usize,
    op: EditOp,
    primary: Option<String>,
}

/// Parses an edit script into the same operations `ax apply` already runs.
///
/// Every error carries the line it is on, because a batch is rejected whole and
/// an agent fixing one needs to be told where to look.
pub fn parse(text: &str) -> Result<Vec<EditOp>, Vec<String>> {
    let text = eol::to_lf(text);
    let mut ops: Vec<EditOp> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut block: Option<Block> = None;
    let mut fence: Option<(String, String, bool, Vec<String>, usize)> = None;

    for (i, raw) in text.lines().enumerate() {
        let no = i + 1;

        // Inside a fence nothing is interpreted — that is the whole point.
        if let Some((name, tag, chomp, lines, opened)) = fence.as_mut() {
            if raw == tag.as_str() {
                let joined = match *chomp {
                    true => lines.join("\n"),
                    false => lines.iter().map(|l| format!("{l}\n")).collect(),
                };
                let (name, opened) = (name.clone(), *opened);
                fence = None;
                match block.as_mut() {
                    Some(b) => assign(b, &name, joined, opened, &mut errors),
                    None => errors.push(format!("line {opened}: `{name}` before any `edit`")),
                }
                continue;
            }
            lines.push(raw.to_owned());
            continue;
        }

        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        // `edit <path>` opens a block and closes the previous one.
        if let Some(path) = line.strip_prefix("edit ") {
            block
                .take()
                .map(|b| finish(b, &mut ops, &mut errors))
                .unwrap_or(());
            let path = path.trim();
            if path.is_empty() {
                errors.push(format!("line {no}: `edit` needs a path"));
                continue;
            }
            block = Some(Block { path: path.to_owned(), line: no, ..Block::default() });
            continue;
        }

        if line == "all" {
            match block.as_mut() {
                Some(b) => b.op.all = true,
                None => errors.push(format!("line {no}: `all` before any `edit`")),
            }
            continue;
        }

        // `at 412` / `at 412:418`. Handled here rather than through
        // `split_directive` because the whole instruction fits on its line, as
        // `edit <path>` does — a heredoc for four characters would be ceremony.
        // It still registers as the block's primary operation, so `at` and
        // `replace` in one block is refused rather than silently resolved.
        if let Some(spec) = line.strip_prefix("at ") {
            match block.as_mut() {
                Some(b) => {
                    note_primary(b, "at", no, &mut errors);
                    b.op.at = Some(spec.trim().to_owned());
                }
                None => errors.push(format!("line {no}: `at` before any `edit`")),
            }
            continue;
        }

        let Some((name, rest)) = split_directive(line) else {
            // The likeliest cause of a stray line is a fence that closed early
            // because the payload contained its own tag — the one way this
            // format can go wrong, and worth naming rather than leaving to be
            // rediscovered.
            errors.push(format!(
                "line {no}: expected `edit <path>`, `<name> <<TAG`, `<name>: text`, \
                 `<name> < path` or `all`, got `{}`. If the payload above contains a line \
                 equal to its own fence tag, the fence closed there — pick a tag the payload \
                 does not contain.",
                preview(line)
            ));
            continue;
        };

        if !PRIMARY.contains(&name.as_str())
            && !SECONDARY.contains(&name.as_str())
            && name != "text_file"
        {
            errors.push(format!(
                "line {no}: unknown directive `{name}`. Known: {}, {}",
                PRIMARY.join(", "),
                SECONDARY.join(", ")
            ));
            continue;
        }

        match rest {
            Payload::Fence { tag, chomp } => {
                fence = Some((name, tag, chomp, Vec::new(), no));
            }
            Payload::Inline(t) => match block.as_mut() {
                Some(b) => assign(b, &name, t, no, &mut errors),
                None => errors.push(format!("line {no}: `{name}` before any `edit`")),
            },
            Payload::File(p) => match block.as_mut() {
                Some(b) => {
                    b.op.text_file = Some(p);
                    // A `< path` payload with no anchor means "replace the whole
                    // file", which is exactly `EditOp`'s own rule.
                    PRIMARY
                        .contains(&name.as_str())
                        .then(|| note_primary(b, &name, no, &mut errors));
                }
                None => errors.push(format!("line {no}: `{name}` before any `edit`")),
            },
        }
    }

    if let Some((name, tag, _, _, opened)) = fence {
        errors.push(format!(
            "line {opened}: `{name} <<{tag}` was never closed — add a line containing exactly `{tag}`"
        ));
    }
    block.map(|b| finish(b, &mut ops, &mut errors)).unwrap_or(());

    match errors.is_empty() {
        true => Ok(ops),
        false => Err(errors),
    }
}

/// What follows a directive name.
enum Payload {
    Fence { tag: String, chomp: bool },
    Inline(String),
    File(String),
}

/// Splits `name <<TAG` / `name: text` / `name < path` into its two halves.
fn split_directive(line: &str) -> Option<(String, Payload)> {
    let name_len = line
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(line.len());
    let (name, rest) = line.split_at(name_len);
    if name.is_empty() {
        return None;
    }

    // `name: text` — the rest of the line, verbatim after one optional space.
    if let Some(inline) = rest.strip_prefix(':') {
        return Some((
            name.to_owned(),
            Payload::Inline(inline.strip_prefix(' ').unwrap_or(inline).to_owned()),
        ));
    }

    let rest = rest.trim_start();
    if let Some(tag) = rest.strip_prefix("<<") {
        let (chomp, tag) = match tag.strip_prefix('-') {
            Some(t) => (true, t),
            None => (false, tag),
        };
        let tag = tag.trim();
        let valid = !tag.is_empty()
            && tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        return valid.then(|| (name.to_owned(), Payload::Fence { tag: tag.to_owned(), chomp }));
    }

    if let Some(path) = rest.strip_prefix('<') {
        let path = path.trim();
        return (!path.is_empty()).then(|| (name.to_owned(), Payload::File(path.to_owned())));
    }

    None
}

/// Records which primary directive a block uses, refusing a second one.
fn note_primary(b: &mut Block, name: &str, no: usize, errors: &mut Vec<String>) {
    match &b.primary {
        Some(first) if first != name => errors.push(format!(
            "line {no}: block for `{}` already has `{first}`; a block carries one operation — \
             start another `edit {}` block for `{name}`",
            b.path, b.path
        )),
        _ => b.primary = Some(name.to_owned()),
    }
}

/// Puts a payload into the field its directive names.
fn assign(b: &mut Block, name: &str, value: String, no: usize, errors: &mut Vec<String>) {
    PRIMARY
        .contains(&name)
        .then(|| note_primary(b, name, no, errors));

    let slot = match name {
        "replace" => &mut b.op.replace,
        "with" => &mut b.op.with,
        "from" => &mut b.op.from,
        "to" => &mut b.op.to,
        "through" => &mut b.op.through,
        "insert_before" => &mut b.op.insert_before,
        "insert_after" => &mut b.op.insert_after,
        "append" => &mut b.op.append,
        "content" => &mut b.op.content,
        "text" => &mut b.op.text,
        "text_file" => &mut b.op.text_file,
        other => {
            errors.push(format!("line {no}: unknown directive `{other}`"));
            return;
        }
    };
    slot.is_some().then(|| {
        errors.push(format!("line {no}: `{name}` given twice in the block for `{}`", b.path))
    });
    *slot = Some(value);
}

/// Closes a block, checking it names an operation at all.
fn finish(b: Block, ops: &mut Vec<EditOp>, errors: &mut Vec<String>) {
    let has_op = b.primary.is_some() || b.op.text_file.is_some();
    match has_op {
        true => ops.push(EditOp { path: b.path, ..b.op }),
        false => errors.push(format!(
            "line {}: `edit {}` names no operation ({} or a `< path` payload)",
            b.line,
            b.path,
            PRIMARY.join("/")
        )),
    }
}

fn preview(s: &str) -> String {
    s.chars().take(48).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_still_json() {
        assert!(!looks_like_script("  [{\"path\":\"a\"}]"));
        assert!(looks_like_script("edit a.rs\nappend: x\n"));
    }

    /// The point of the whole format: a payload full of quotes and backslashes
    /// arrives exactly as written, with nothing escaped on the way in.
    #[test]
    fn a_payload_is_verbatim() {
        let script = "edit a.rs\nreplace <<RS\nlet s = \"a\\\\b\";\nRS\nwith <<RS\nlet s = r#\"a\\b\"#;\nRS\n";
        let ops = parse(script).expect("parses");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].replace.as_deref(), Some("let s = \"a\\\\b\";\n"));
        assert_eq!(ops[0].with.as_deref(), Some("let s = r#\"a\\b\"#;\n"));
    }

    #[test]
    fn a_fenced_payload_keeps_its_trailing_newline_and_an_inline_one_does_not() {
        let ops = parse("edit a.rs\nreplace <<T\nx\nT\nwith: y\n").expect("parses");
        assert_eq!(ops[0].replace.as_deref(), Some("x\n"));
        assert_eq!(ops[0].with.as_deref(), Some("y"));
    }

    #[test]
    fn a_chomped_fence_drops_it() {
        let ops = parse("edit a.rs\nreplace <<-T\nx\ny\nT\nwith: z\n").expect("parses");
        assert_eq!(ops[0].replace.as_deref(), Some("x\ny"));
    }

    /// A line that looks like a directive is still just text inside a fence.
    #[test]
    fn directives_inside_a_fence_are_payload() {
        let ops = parse("edit a.rs\nappend <<T\nedit other.rs\nreplace: nope\nall\nT\n")
            .expect("parses");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].path, "a.rs");
        assert_eq!(ops[0].append.as_deref(), Some("edit other.rs\nreplace: nope\nall\n"));
    }

    #[test]
    fn several_blocks_become_several_ops() {
        let ops = parse("edit a.rs\nreplace: x\nwith: y\nall\n\n# comment\nedit b.md\nappend: z\n")
            .expect("parses");
        assert_eq!(ops.len(), 2);
        assert!(ops[0].all);
        assert_eq!(ops[1].path, "b.md");
        assert_eq!(ops[1].append.as_deref(), Some("z"));
    }

    #[test]
    fn a_file_payload_becomes_text_file() {
        let ops = parse("edit a.rs\ninsert_before: fn main\ntext < staging/body.txt\n")
            .expect("parses");
        assert_eq!(ops[0].text_file.as_deref(), Some("staging/body.txt"));
        assert_eq!(ops[0].insert_before.as_deref(), Some("fn main"));
    }

    #[test]
    fn an_unclosed_fence_names_the_line_it_opened_on() {
        let err = parse("edit a.rs\nreplace <<T\nx\n").expect_err("must fail");
        assert!(err[0].contains("line 2"), "{err:?}");
        assert!(err[0].contains("never closed"), "{err:?}");
    }

    #[test]
    fn two_operations_in_one_block_are_refused() {
        let err = parse("edit a.rs\nreplace: x\nwith: y\nappend: z\n").expect_err("must fail");
        assert!(err[0].contains("one operation"), "{err:?}");
    }

    #[test]
    fn a_block_with_no_operation_is_refused() {
        let err = parse("edit a.rs\nwith: y\n").expect_err("must fail");
        assert!(err[0].contains("names no operation"), "{err:?}");
    }

    #[test]
    fn an_unknown_directive_lists_the_known_ones() {
        let err = parse("edit a.rs\nsubstitute: x\n").expect_err("must fail");
        assert!(err[0].contains("unknown directive"), "{err:?}");
        assert!(err[0].contains("replace"), "{err:?}");
    }

    /// A payload containing its own tag closes the fence early. It cannot be
    /// applied silently — the remains parse as garbage directives — and the
    /// error says what actually happened.
    #[test]
    fn a_tag_inside_its_own_payload_is_diagnosed() {
        let err = parse("edit a.rs\nappend <<T\nline\nT\nstill payload\nT\n").expect_err("fails");
        assert!(err.iter().any(|e| e.contains("fence tag")), "{err:?}");
    }

    #[test]
    fn every_error_is_reported_not_just_the_first() {
        let err = parse("edit a.rs\nbogus: 1\nedit b.rs\nalso_bogus: 2\n").expect_err("must fail");
        assert_eq!(err.len(), 4, "two unknown directives + two empty blocks: {err:?}");
    }
}
