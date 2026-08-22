//! Batch edits — the answer to "I reached for a script instead of the tool."
//!
//! `ax edit` handles one literal replacement whose text has to survive argv
//! quoting. That is fine for a one-line change and hopeless for anything real,
//! which is why an agent's reflex is to shell out to a script instead. This
//! module removes that reflex by giving the tool what the script had, plus what
//! it did not:
//!
//! * **Text arrives as JSON on stdin**, or from a file via `text_file`, so a
//!   large payload never has to survive shell quoting *or* JSON escaping.
//! * **Many edits, many files, one invocation.**
//! * **Span and anchor operations** — `from`/`to`, `insert_before`,
//!   `insert_after`, `append` — not just literal replacement.
//! * **All-or-nothing.** Every anchor is resolved against in-memory content
//!   *before* a single byte is written. A batch that would half-apply is
//!   rejected whole. A shell script gives no such guarantee: it fails halfway
//!   and leaves the tree in a state nobody designed.
//!
//! It also normalises incoming text to each file's existing line endings, so a
//! caller holding LF text cannot silently corrupt a CRLF file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

/// One edit. The operation is inferred from whichever field is present, so the
/// common cases stay terse.
#[derive(Debug, Deserialize)]
pub struct EditOp {
    pub path: String,
    /// Literal text to replace.
    #[serde(default)]
    pub replace: Option<String>,
    /// Start of a span to replace (used with `to`).
    #[serde(default)]
    pub from: Option<String>,
    /// End of a span. The span runs from the start of `from` up to, but not
    /// including, `to`.
    #[serde(default)]
    pub to: Option<String>,
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
}

/// The line ending a file already uses.
pub fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Rewrites `text` to use `eol`, whatever it arrived as.
pub fn to_eol(text: &str, eol: &str) -> String {
    let lf = text.replace("\r\n", "\n");
    if eol == "\r\n" {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

/// Applies one operation to `current`, returning the new content.
///
/// `payload` overrides the inline payload when `text_file` was given; `label`
/// is the repo-relative path, used only for messages.
fn apply_one(
    current: &str,
    op: &EditOp,
    payload: Option<&str>,
    label: &str,
) -> Result<String, String> {
    let eol = eol_of(current);
    let supplied =
        |inline: Option<&String>| -> Option<String> { payload.map(str::to_owned).or_else(|| inline.cloned()) };

    if op.content.is_some() || (payload.is_some() && op.is_whole_file()) {
        let content = supplied(op.content.as_ref()).unwrap_or_default();
        return Ok(to_eol(&content, eol));
    }

    if op.append.is_some() {
        let append = supplied(op.append.as_ref()).unwrap_or_default();
        let tail = to_eol(&append, eol);
        let joiner = if current.is_empty() || current.ends_with('\n') {
            String::new()
        } else {
            eol.to_owned()
        };
        return Ok(format!("{current}{joiner}{tail}"));
    }

    // Span replacement: everything from `from` up to (not including) `to`.
    if let Some(from) = &op.from {
        let to = op
            .to
            .as_ref()
            .ok_or_else(|| format!("{label}: `from` needs a matching `to`"))?;
        let from = to_eol(from, eol);
        let to = to_eol(to, eol);

        let start = locate(current, &from, label, "from")?;
        let rest = &current[start + from.len()..];
        let end_rel = locate(rest, &to, label, "to")?;
        let end = start + from.len() + end_rel;

        let new = supplied(op.with.as_ref()).unwrap_or_default();
        let new = to_eol(&new, eol);
        return Ok(format!("{}{new}{}", &current[..start], &current[end..]));
    }

    if let Some(anchor) = op.insert_before.as_ref().or(op.insert_after.as_ref()) {
        let text = supplied(op.text.as_ref())
            .ok_or_else(|| format!("{label}: insert_before/insert_after needs `text`"))?;
        let anchor = to_eol(anchor, eol);
        locate(current, &anchor, label, "anchor")?;

        let text = to_eol(&text, eol);
        let replacement = if op.insert_before.is_some() {
            format!("{text}{anchor}")
        } else {
            format!("{anchor}{text}")
        };
        return Ok(current.replacen(&anchor, &replacement, 1));
    }

    let old = op.replace.as_ref().ok_or_else(|| {
        format!("{label}: no operation given (replace/from/insert/append/content)")
    })?;
    let new = supplied(op.with.as_ref())
        .ok_or_else(|| format!("{label}: `replace` needs a matching `with`"))?;

    let old = to_eol(old, eol);
    let new = to_eol(&new, eol);
    let count = current.matches(&old).count();
    if count == 0 {
        return Err(format!("{label}: text to replace not found: {}", preview(&old)));
    }
    if count > 1 && !op.all {
        return Err(format!(
            "{label}: text occurs {count} times; set \"all\": true or extend the anchor: {}",
            preview(&old)
        ));
    }

    Ok(if op.all {
        current.replace(&old, &new)
    } else {
        current.replacen(&old, &new, 1)
    })
}

/// Finds a unique anchor, or explains precisely why it is unusable.
fn locate(haystack: &str, needle: &str, label: &str, role: &str) -> Result<usize, String> {
    let count = haystack.matches(needle).count();
    if count == 0 {
        return Err(format!("{label}: {role} not found: {}", preview(needle)));
    }
    if count > 1 {
        return Err(format!(
            "{label}: {role} occurs {count} times; extend it until it is unique: {}",
            preview(needle)
        ));
    }
    haystack
        .find(needle)
        .ok_or_else(|| format!("{label}: {role} vanished between checks"))
}

fn preview(s: &str) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    let head: String = first.chars().take(60).collect();
    if first.chars().count() > 60 {
        format!("`{head}...`")
    } else {
        format!("`{head}`")
    }
}

/// A file whose content changed, ready to be written.
pub struct Planned {
    pub path: PathBuf,
    pub label: String,
    pub content: String,
    pub before: i64,
    pub after: i64,
}

/// One resolved edit: scoped path, display label, the op, and its payload.
pub type Resolved<'a> = (PathBuf, String, &'a EditOp, Option<String>);

/// Resolves every operation against in-memory content.
///
/// Returns either the full set of files to write, or **every** error found —
/// reporting all of them at once, because an agent fixing a batch wants the
/// whole list, not the first failure.
pub fn plan(ops: &[Resolved<'_>]) -> Result<Vec<Planned>, Vec<String>> {
    let mut working: BTreeMap<PathBuf, (String, String, i64)> = BTreeMap::new();
    let mut errors: Vec<String> = Vec::new();

    for (i, (path, label, op, payload)) in ops.iter().enumerate() {
        let entry = working.entry(path.clone()).or_insert_with(|| {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let len = text.len() as i64;
            (text, label.clone(), len)
        });

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
        .map(|(path, (content, label, before))| Planned {
            path,
            label,
            after: content.len() as i64,
            content,
            before,
        })
        .collect())
}
