//! Batch edits — the answer to "I reached for a script instead of the tool."
//!
//! `ax edit` handles one literal replacement whose text has to survive argv
//! quoting. That is fine for a one-line change and hopeless for anything real,
//! which is why an agent's reflex is to shell out to a script instead. This
//! module removes that reflex by giving the tool what the script had, plus what
//! it did not:
//!
//! * **No escaping at all**, via the edit script format in [`crate::script`] —
//!   source code pasted verbatim between heredoc fences. JSON on stdin still
//!   works and is still right for a programmatic caller; it is no longer the
//!   only way in, because JSON escaping was making an agent transform its own
//!   payload by hand before sending it.
//! * **Many edits, many files, one invocation.**
//! * **Span and anchor operations** — `from`/`to`, `insert_before`,
//!   `insert_after`, `append` — not just literal replacement.
//! * **All-or-nothing.** Every anchor is resolved against in-memory content
//!   *before* a single byte is written. A batch that would half-apply is
//!   rejected whole. A shell script gives no such guarantee: it fails halfway
//!   and leaves the tree in a state nobody designed.
//!
//! # Line endings are not this module's problem any more
//!
//! Everything here runs on LF text, unconditionally. [`crate::eol`] loads each
//! file into its LF form and renders it back into the file's own convention on
//! the way out, so an anchor written with `\n` matches a CRLF file, a CRLF file
//! stays CRLF, and a `.gitattributes` declaration (`eol=lf`, `binary`) is
//! honoured — none of which this module has to know. It used to convert every
//! incoming anchor *up* to the file's endings before matching, which was a
//! second, subtly different answer to the same question `edit.rs` was already
//! answering its own way.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::eol::{self, Attributes, Loaded};

/// One edit. The operation is inferred from whichever field is present, so the
/// common cases stay terse.
#[derive(Debug, Default, Deserialize)]
pub struct EditOp {
    pub path: String,
    /// Literal text to replace.
    #[serde(default)]
    pub replace: Option<String>,
    /// Start of a span to replace (used with `to`).
    #[serde(default)]
    pub from: Option<String>,
    /// End of a span, **exclusive**. The span runs from the start of `from` up
    /// to, but not including, `to`, so the `to` anchor survives the edit.
    ///
    /// Reach for [`EditOp::through`] instead when you mean "replace this whole
    /// span, both ends included" — which is nearly always what a caller
    /// re-anchoring a function body means.
    #[serde(default)]
    pub to: Option<String>,
    /// End of a span, **inclusive**. The span runs from the start of `from`
    /// through the end of `through`, so the anchor is consumed with the rest.
    ///
    /// This exists because the exclusive form has a failure mode with no
    /// diagnostic: a caller who re-states the `to` anchor at the end of `with`
    /// gets it twice, and a caller who does not gets a cut that stops short.
    /// Both write a file that is wrong in a way the tool reported as success —
    /// which is the same class of defect as a zero-result search that is a lie.
    /// One of the two anchoring conventions had to be spelt in the directive
    /// name rather than left to be remembered.
    #[serde(default)]
    pub through: Option<String>,
    #[serde(default)]
    pub with: Option<String>,
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub insert_before: Option<String>,
    #[serde(default)]
    pub insert_after: Option<String>,
    #[serde(default)]
    pub append: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    /// Supplies this edit's payload from a repo file instead of inline JSON.
    /// Stage that file (e.g. under the gitignored `.axiom-atlas/staging/`) and
    /// no escaping is needed anywhere.
    #[serde(default)]
    pub text_file: Option<String>,
    /// Line-addressed replacement: `"412"` or `"412:418"`, 1-based and
    /// inclusive, replaced by `with`.
    ///
    /// Anchors are the right default — they survive a file moving underneath
    /// them, which line numbers do not. But a batch generated from compiler or
    /// linter output already *has* exact line numbers and no anchor, and
    /// synthesising one from the line's own text is where it goes wrong: the
    /// same text often repeats, so the generator ends up widening anchors
    /// upward until they happen to be unique. That is a script reimplementing
    /// this tool's job, badly.
    ///
    /// So: read the file once, address it by line, and apply through the same
    /// all-or-nothing planner. A line number is only valid against the content
    /// the plan started from, which is exactly why it belongs inside a batch
    /// that resolves everything before writing a byte.
    #[serde(default)]
    pub at: Option<String>,
}

impl EditOp {
    /// True when no anchor-shaped field is set, so a bare `text_file` means
    /// "replace the whole file".
    fn is_whole_file(&self) -> bool {
        self.replace.is_none()
            && self.from.is_none()
            && self.insert_before.is_none()
            && self.insert_after.is_none()
            && self.append.is_none()
    }

    /// The path this edit reads its payload from, if any.
    pub fn payload_path(&self) -> Option<&str> {
        self.text_file.as_deref()
    }

    /// The first line of whichever anchor this edit uses, for a dry run.
    pub fn summary(&self) -> String {
        let (kind, anchor) = match self {
            _ if self.content.is_some() => ("content", None),
            _ if self.append.is_some() => ("append", None),
            _ if self.from.is_some() => ("span", self.from.as_deref()),
            _ if self.insert_before.is_some() => ("insert_before", self.insert_before.as_deref()),
            _ if self.insert_after.is_some() => ("insert_after", self.insert_after.as_deref()),
            _ if self.replace.is_some() => ("replace", self.replace.as_deref()),
            _ => ("whole file", None),
        };
        anchor
            .map(|a| format!("{kind} {}", preview(a)))
            .unwrap_or_else(|| kind.to_owned())
    }
}

/// Applies one operation to `current`, returning the new content.
///
/// Everything — `current`, every anchor, every payload — is LF. See the module
/// doc.
///
/// `payload` overrides the inline payload when `text_file` was given; `label`
/// is the repo-relative path, used only for messages.
fn apply_one(
    current: &str,
    op: &EditOp,
    payload: Option<&str>,
    label: &str,
) -> Result<String, String> {
    let supplied = |inline: Option<&String>| -> Option<String> {
        payload.map(str::to_owned).or_else(|| inline.cloned())
    };

    if op.content.is_some() || (payload.is_some() && op.is_whole_file()) {
        return Ok(supplied(op.content.as_ref()).unwrap_or_default());
    }

    if op.append.is_some() {
        let tail = supplied(op.append.as_ref()).unwrap_or_default();
        let joiner = match current.is_empty() || current.ends_with('\n') {
            true => "",
            false => "\n",
        };
        return Ok(format!("{current}{joiner}{tail}"));
    }

    // Line-addressed replacement.
    if let Some(spec) = &op.at {
        let (first, last) = parse_at(spec).map_err(|e| format!("{label}: {e}"))?;
        let (begin, end) = line_span(current, first, last).map_err(|e| format!("{label}: {e}"))?;
        let new = supplied(op.with.as_ref())
            .ok_or_else(|| format!("{label}: `at` needs a matching `with`"))?;
        return Ok(format!("{}{new}{}", &current[..begin], &current[end..]));
    }

    // Span replacement. `to` ends the span before its anchor; `through` ends it
    // after. Exactly one of the two must be present.
    if let Some(from) = &op.from {
        let (anchor, keyword, inclusive) = match (op.to.as_ref(), op.through.as_ref()) {
            (Some(_), Some(_)) => {
                return Err(format!(
                    "{label}: `from` takes either `to` (exclusive) or `through` (inclusive), not both"
                ))
            }
            (Some(to), None) => (to, "to", false),
            (None, Some(through)) => (through, "through", true),
            (None, None) => {
                return Err(format!(
                    "{label}: `from` needs a matching `to` (exclusive) or `through` (inclusive)"
                ))
            }
        };

        let start = locate(current, from, label, "from")?;
        let rest = &current[start + from.len()..];
        let end_rel = locate(rest, anchor, label, keyword)?;
        let end = start + from.len() + end_rel + [0, anchor.len()][usize::from(inclusive)];

        let new = supplied(op.with.as_ref()).unwrap_or_default();
        return Ok(format!("{}{new}{}", &current[..start], &current[end..]));
    }

    if let Some(anchor) = op.insert_before.as_ref().or(op.insert_after.as_ref()) {
        let text = supplied(op.text.as_ref())
            .ok_or_else(|| format!("{label}: insert_before/insert_after needs `text`"))?;
        locate(current, anchor, label, "anchor")?;

        let replacement = match op.insert_before.is_some() {
            true => format!("{text}{anchor}"),
            false => format!("{anchor}{text}"),
        };
        return Ok(current.replacen(anchor, &replacement, 1));
    }

    let old = op.replace.as_ref().ok_or_else(|| {
        format!("{label}: no operation given (replace/from/insert/append/content)")
    })?;
    let new = supplied(op.with.as_ref())
        .ok_or_else(|| format!("{label}: `replace` needs a matching `with`"))?;

    let hits = occurrences(current, old);
    if hits.is_empty() {
        return Err(format!("{label}: text to replace not found: {}", preview(old)));
    }
    if hits.len() > 1 && !op.all {
        return Err(format!(
            "{label}: text occurs {} times; set \"all\": true (or an `all` line) or extend \
             the anchor: {}",
            hits.len(),
            preview(old)
        ));
    }

    // Splice at the located offsets rather than calling `replace`/`replacen`,
    // which would re-find the needle and so ignore the line anchoring above.
    // Back to front, so each index stays valid as the earlier ones move.
    let chosen = match op.all {
        true => hits.as_slice(),
        false => &hits[..1],
    };
    let mut out = current.to_owned();
    chosen.iter().rev().for_each(|&i| {
        out.replace_range(i..i + old.len(), &new);
    });
    Ok(out)
}

/// Every start index at which `needle` occurs in `haystack`.
///
/// **A newline-terminated needle only matches at a line boundary.** That is not
/// a refinement, it is a correctness fix. A `<<TAG` payload is whole lines by
/// construction, and without the anchoring a shallowly-indented line matches
/// *inside* an identically-worded deeper one:
///
/// ```text
///         if by.length_sq() < 1e-9 {          <- the anchor
///             if by.length_sq() < 1e-9 {      <- contains it, starting at col 4
/// ```
///
/// The tool then reported "occurs 2 times" for a line the caller could only find
/// once, and the advice ("extend the anchor") could not help, because the second
/// match was the first one shifted right. A line fragment — the `name: text`
/// form, which is deliberately *not* newline-terminated — still matches anywhere,
/// since that is what it is for.
fn occurrences(haystack: &str, needle: &str) -> Vec<usize> {
    let whole_lines = needle.ends_with('\n');
    let bytes = haystack.as_bytes();
    haystack
        .match_indices(needle)
        .map(|(i, _)| i)
        .filter(|i| !whole_lines || *i == 0 || bytes[i - 1] == b'\n')
        .collect()
}

/// Finds a unique anchor, or explains precisely why it is unusable.
fn locate(haystack: &str, needle: &str, label: &str, role: &str) -> Result<usize, String> {
    let hits = occurrences(haystack, needle);
    if hits.is_empty() {
        return Err(format!("{label}: {role} not found: {}", preview(needle)));
    }
    if hits.len() > 1 {
        return Err(format!(
            "{label}: {role} occurs {} times; extend it until it is unique: {}",
            hits.len(),
            preview(needle)
        ));
    }
    Ok(hits[0])
}

/// The byte span of lines `first..=last`, 1-based, as `ax read --range` counts.
fn line_span(text: &str, first: usize, last: usize) -> Result<(usize, usize), String> {
    let starts: Vec<usize> = core::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    if first == 0 {
        return Err("line numbers are 1-based; there is no line 0".to_owned());
    }
    if first > last {
        return Err(format!("line {first} is after line {last}"));
    }
    if last > starts.len() {
        return Err(format!(
            "line {last} is past the end of the file, which has {} line(s)",
            starts.len()
        ));
    }
    let begin = starts[first - 1];
    let end = match last < starts.len() {
        true => starts[last],
        false => text.len(),
    };
    Ok((begin, end))
}

/// Parses `at` — `"412"` or `"412:418"`.
fn parse_at(spec: &str) -> Result<(usize, usize), String> {
    let (a, b) = spec.split_once(':').unwrap_or((spec, spec));
    let num = |s: &str| -> Result<usize, String> {
        s.trim()
            .parse::<usize>()
            .map_err(|_| format!("`at` wants a line number or `first:last`, got `{spec}`"))
    };
    Ok((num(a)?, num(b)?))
}

fn preview(s: &str) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    let head: String = first.chars().take(60).collect();
    match first.chars().count() > 60 {
        true => format!("`{head}...`"),
        false => format!("`{head}`"),
    }
}

/// Refuses `at` edits that would read stale line numbers.
///
/// Edits in a batch apply in order against evolving content, so an earlier edit
/// that changes a file's line COUNT shifts every line below it — and a later
/// `at` addressed against the file the caller read is then off by however much
/// moved. That is silent: the edit applies cleanly, to the wrong line.
///
/// Working bottom-up removes the problem rather than warning about it: editing
/// line 40 cannot move line 12, so if every `at` on a file addresses strictly
/// above the previous one, all of them stay valid against the content the
/// caller measured. So that is the rule, and a batch that breaks it is refused
/// with the reason rather than quietly mis-applied.
///
/// Anchor-based edits are unaffected — they find their own text wherever it
/// ended up, which is exactly why they are still the default.
fn check_at_order(
    floor: &mut BTreeMap<PathBuf, usize>,
    path: &PathBuf,
    spec: &str,
    label: &str,
) -> Result<(), String> {
    let (first, last) = parse_at(spec).map_err(|e| format!("{label}: {e}"))?;
    let previous = floor.get(path).copied().unwrap_or(usize::MAX);
    if last >= previous {
        return Err(format!(
            "{label}: `at {spec}` comes after an edit at line {previous} in the same file. \
             Order `at` edits from the LAST line to the first: an earlier edit that adds or \
             removes lines shifts every line below it, so this one would apply to the wrong \
             place without failing."
        ));
    }
    floor.insert(path.clone(), first);
    Ok(())
}

/// A file whose content changed, ready to be written.
pub struct Planned {
    pub path: PathBuf,
    pub label: String,
    /// The new content, in LF. Render it through `loaded` to write it.
    pub content: String,
    pub before: i64,
    /// How the file is to be written back — the convention it arrived in, or
    /// whatever `.gitattributes` declares.
    pub loaded: Loaded,
}

impl Planned {
    /// True when the file on disk already says what the plan says.
    pub fn is_noop(&self) -> bool {
        self.content == self.loaded.lf
    }
}

/// One resolved edit: scoped path, display label, the op, and its payload.
pub type Resolved<'a> = (PathBuf, String, &'a EditOp, Option<String>);

/// Resolves every operation against in-memory content.
///
/// Returns either the full set of files to write, or **every** error found —
/// reporting all of them at once, because an agent fixing a batch wants the
/// whole list, not the first failure.
pub fn plan(attrs: &Attributes, ops: &[Resolved<'_>]) -> Result<Vec<Planned>, Vec<String>> {
    let mut working: BTreeMap<PathBuf, (String, String, Loaded)> = BTreeMap::new();
    let mut errors: Vec<String> = Vec::new();
    // The lowest line any `at` edit has already claimed, per file. See
    // `check_at_order`.
    let mut at_floor: BTreeMap<PathBuf, usize> = BTreeMap::new();

    for (i, (path, label, op, payload)) in ops.iter().enumerate() {
        op.at
            .as_ref()
            .and_then(|spec| check_at_order(&mut at_floor, path, spec, label).err())
            .map(|e| errors.push(e));
        if !working.contains_key(path) {
            match eol::load(attrs, path, label) {
                Ok(loaded) => {
                    working.insert(path.clone(), (loaded.lf.clone(), label.clone(), loaded));
                }
                Err(e) => {
                    errors.push(format!("edit {}: {e}", i + 1));
                    continue;
                }
            }
        }
        let entry = working.get_mut(path).expect("just inserted");

        // A payload from a file is text like any other: LF in, LF out.
        let payload = payload.as_deref().map(eol::to_lf);
        match apply_one(&entry.0, op, payload.as_deref(), label) {
            Ok(next) => entry.0 = next,
            Err(e) => errors.push(format!("edit {}: {e}", i + 1)),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(working
        .into_iter()
        .map(|(path, (content, label, loaded))| Planned { path, label, content, before: loaded.bytes_before, loaded })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(json: &str) -> EditOp {
        serde_json::from_str(json).expect("fixture parses")
    }

    /// The anchor is LF and the file is LF, because `eol` has already
    /// normalised both. This module never sees a carriage return.
    #[test]
    fn replace_is_lf_only_now() {
        let out = apply_one("a\nb\nc\n", &op(r#"{"path":"x","replace":"a\nb","with":"z"}"#), None, "x")
            .expect("applies");
        assert_eq!(out, "z\nc\n");
    }

    #[test]
    fn an_ambiguous_anchor_is_refused_and_all_permits_it() {
        let ambiguous = op(r#"{"path":"x","replace":"x","with":"y"}"#);
        assert!(apply_one("x\nx\n", &ambiguous, None, "x").is_err());
        let every = op(r#"{"path":"x","replace":"x","with":"y","all":true}"#);
        assert_eq!(apply_one("x\nx\n", &every, None, "x").expect("applies"), "y\ny\n");
    }

    #[test]
    fn append_adds_a_separator_only_when_one_is_missing() {
        let a = op(r#"{"path":"x","append":"b\n"}"#);
        assert_eq!(apply_one("a\n", &a, None, "x").expect("applies"), "a\nb\n");
        assert_eq!(apply_one("a", &a, None, "x").expect("applies"), "a\nb\n");
    }

    #[test]
    fn a_span_runs_from_the_start_of_from_to_the_start_of_to() {
        let s = op(r#"{"path":"x","from":"[A]","to":"[B]","with":"[A]\nnew\n"}"#);
        let out = apply_one("head\n[A]\nold\n[B]\ntail\n", &s, None, "x").expect("applies");
        assert_eq!(out, "head\n[A]\nnew\n[B]\ntail\n");
    }

    /// `through` is the inclusive twin, and the reason it exists is the
    /// asymmetry this test and the one above it show side by side: with `to`
    /// the caller must re-state the closing anchor inside `with`, and with
    /// `through` the caller must not. Forgetting either way writes a wrong file
    /// that the tool reports as a success.
    #[test]
    fn a_through_span_consumes_its_closing_anchor() {
        let s = op(r#"{"path":"x","from":"[A]","through":"[B]","with":"gone\n"}"#);
        let out = apply_one("head\n[A]\nold\n[B]\ntail\n", &s, None, "x").expect("applies");
        assert_eq!(out, "head\ngone\n\ntail\n");
    }

    #[test]
    fn a_through_span_ending_at_end_of_file_takes_the_anchor_with_it() {
        let s = op(r#"{"path":"x","from":"fn a","through":"}","with":"fn a() {}"}"#);
        let out = apply_one("fn a() {\n  body\n}", &s, None, "x").expect("applies");
        assert_eq!(out, "fn a() {}");
    }

    #[test]
    fn a_span_refuses_both_end_anchors() {
        let s = op(r#"{"path":"x","from":"[A]","to":"[B]","through":"[B]","with":"n"}"#);
        let err = apply_one("[A]x[B]", &s, None, "x").expect_err("refuses");
        assert!(err.contains("not both"), "{err}");
    }

    #[test]
    fn a_span_with_no_end_anchor_says_which_two_it_wanted() {
        let s = op(r#"{"path":"x","from":"[A]","with":"n"}"#);
        let err = apply_one("[A]x", &s, None, "x").expect_err("refuses");
        assert!(err.contains("`to`") && err.contains("`through`"), "{err}");
    }

    #[test]
    fn a_missing_through_anchor_names_the_directive_that_failed() {
        let s = op(r#"{"path":"x","from":"[A]","through":"[Z]","with":"n"}"#);
        let err = apply_one("[A]x[B]", &s, None, "x").expect_err("refuses");
        assert!(err.contains("through"), "{err}");
    }

    #[test]
    fn a_summary_names_the_operation_and_its_anchor() {
        assert!(op(r#"{"path":"x","replace":"fn main() {","with":""}"#)
            .summary()
            .starts_with("replace `fn main() {`"));
    }
}

#[cfg(test)]
mod line_addressing_tests {
    use super::{line_span, occurrences, parse_at};

    /// The failure that motivated whole-line anchoring: the shallower line is a
    /// substring of the deeper one, so a plain `matches()` counts two and the
    /// caller is told to "extend the anchor" for a line they can only find once.
    #[test]
    fn a_newline_terminated_anchor_only_matches_at_a_line_start() {
        let text = "fn a() {\n    if x {\n}\nfn b() {\n        if x {\n}\n";
        assert_eq!(text.matches("    if x {\n").count(), 2, "substring sees two");
        assert_eq!(
            occurrences(text, "    if x {\n").len(),
            1,
            "line-anchored sees the one that starts a line"
        );
    }

    /// A `name: text` payload is deliberately NOT newline-terminated — it is a
    /// line fragment — so it must still match mid-line.
    #[test]
    fn a_fragment_still_matches_inside_a_line() {
        let text = "let a = foo(1);\nlet b = foo(2);\n";
        assert_eq!(occurrences(text, "foo(").len(), 2);
    }

    #[test]
    fn a_line_span_is_one_based_and_inclusive() {
        let text = "one\ntwo\nthree\n";
        assert_eq!(line_span(text, 1, 1), Ok((0, 4)));
        assert_eq!(line_span(text, 2, 3), Ok((4, 14)));
        assert_eq!(&text[4..14], "two\nthree\n");
    }

    #[test]
    fn a_line_span_reaching_the_last_line_of_an_unterminated_file_ends_at_its_end() {
        let text = "one\ntwo";
        assert_eq!(line_span(text, 2, 2), Ok((4, 7)));
    }

    #[test]
    fn line_zero_and_a_reversed_or_past_the_end_range_are_refused() {
        let text = "one\ntwo\n";
        assert!(line_span(text, 0, 1).is_err());
        assert!(line_span(text, 2, 1).is_err());
        assert!(line_span(text, 1, 99).is_err());
    }

    #[test]
    fn at_reads_a_single_line_or_a_range() {
        assert_eq!(parse_at("412"), Ok((412, 412)));
        assert_eq!(parse_at("412:418"), Ok((412, 418)));
        assert_eq!(parse_at(" 7 : 9 "), Ok((7, 9)));
        assert!(parse_at("x").is_err());
        assert!(parse_at("1:x").is_err());
    }
}
