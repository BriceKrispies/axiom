//! Scoped, atomic file changes.
//!
//! Every write lands through a temp file in the same directory followed by a
//! rename, so a killed `ax` never leaves a half-written source file behind.
//! Paths have already passed `Repo::resolve`, so a write cannot land outside
//! the checkout.

use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct EditOutcome {
    pub replacements: usize,
    pub bytes_before: i64,
    pub bytes_after: i64,
}

impl EditOutcome {
    pub fn delta(&self) -> i64 {
        self.bytes_after - self.bytes_before
    }
}

/// Replaces a literal `old` with `new`.
///
/// Refuses an ambiguous edit: if `old` occurs more than once and `all` is not
/// set, the agent is told to pass `--all` or supply a longer anchor. Silently
/// editing the first of several matches is how agents corrupt files.
///
/// # Line endings are normalised for the match, and restored for the write
///
/// The anchor an agent types has `\n` in it. Half the files in a Windows
/// checkout have `\r\n` on disk, because git converts on checkout. Matching the
/// two literally means **every multi-line anchor fails** on those files — and
/// fails with "text does not occur", which is untrue and sends the agent
/// hunting for a typo that is not there. That is exactly the friction that
/// makes an agent abandon the tool for a hand-rolled script.
///
/// So the haystack and both needles are normalised to `\n` before matching, and
/// the file is written back in whatever convention it already used. A CRLF file
/// stays CRLF; nothing else in the repo sees a spurious whole-file diff.
pub fn replace(
    path: &Path,
    label: &str,
    old: &str,
    new: &str,
    all: bool,
) -> Result<EditOutcome, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("cannot read `{label}`: {e}"))?;
    let crlf = raw.contains("\r\n");
    let text = lf(&raw);
    let old = lf(old);
    let new = lf(new);

    let count = text.matches(&old).count();
    if count == 0 {
        return Err(format!(
            "`--replace` text does not occur in `{label}`. Nothing was written."
        ));
    }
    if count > 1 && !all {
        return Err(format!(
            "`--replace` text occurs {count} times in `{}`. Pass --all to change every \
             occurrence, or extend the anchor until it is unique. Nothing was written.",
            path.display()
        ));
    }

    let updated = if all {
        text.replace(&old, &new)
    } else {
        text.replacen(&old, &new, 1)
    };
    // Byte counts are reported in the file's OWN convention, so the number an
    // agent sees matches what landed on disk rather than the normalised form.
    let restored = restore(&updated, crlf);
    let bytes_before = raw.len() as i64;
    let bytes_after = restored.len() as i64;
    atomic_write(path, label, &restored)?;

    Ok(EditOutcome {
        replacements: if all { count } else { 1 },
        bytes_before,
        bytes_after,
    })
}

/// Creates or overwrites a file with `content`.
pub fn write(path: &Path, label: &str, content: &str) -> Result<EditOutcome, String> {
    let bytes_before = fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create the parent of `{label}`: {e}"))?;
    }
    atomic_write(path, label, content)?;
    Ok(EditOutcome {
        replacements: 1,
        bytes_before,
        bytes_after: content.len() as i64,
    })
}

fn atomic_write(path: &Path, label: &str, content: &str) -> Result<(), String> {
    let tmp = path.with_extension(format!(
        "{}.axtmp{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));
    fs::write(&tmp, content).map_err(|e| format!("cannot write `{label}`: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot replace `{label}`: {e}")
    })
}

/// Every line ending as `\n`, so a match never depends on how git happened to
/// check the file out. `\r\n` first, then any lone `\r` (old-Mac, and the tail
/// of a file someone edited with a mix of both).
fn lf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Puts `\r\n` back when that is what the file used.
fn restore(s: &str, crlf: bool) -> String {
    match crlf {
        true => s.replace('\n', "\r\n"),
        false => s.to_owned(),
    }
}

/// Reads a file, optionally a 1-based inclusive line range, with line anchors.
pub fn read_lines(
    path: &Path,
    label: &str,
    range: Option<(usize, usize)>,
) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read `{label}`: {e}"))?;
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = range.unwrap_or((1, lines.len()));
    let start = start.max(1);
    let end = end.min(lines.len());

    Ok(lines
        .iter()
        .enumerate()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start.saturating_sub(1)))
        .map(|(i, l)| format!("{:>6}\t{l}", i + 1))
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ax-edit-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(name);
        fs::write(&path, body).expect("scratch file");
        path
    }

    /// **A multi-line anchor matches a CRLF file.**
    ///
    /// This is the whole bug: an agent types `\n` in its anchor, git checks the
    /// file out with `\r\n`, and the literal match never lands. Every
    /// multi-line `ax edit` on a Windows checkout failed — reporting "text does
    /// not occur", which is untrue and sends you hunting for a typo.
    #[test]
    fn a_multi_line_anchor_matches_a_crlf_file() {
        let path = scratch("crlf.rs", "fn a() {\r\n    one();\r\n    two();\r\n}\r\n");
        let out = replace(&path, "crlf.rs", "    one();\n    two();", "    only();", false)
            .expect("the anchor must match despite the line endings");
        assert_eq!(out.replacements, 1);
        let after = fs::read_to_string(&path).expect("read back");
        assert!(after.contains("only();"), "the replacement did not land: {after:?}");
    }

    /// **And the file keeps its own line endings.**
    ///
    /// Normalising for the match must not rewrite the whole file to LF — that
    /// would put a spurious every-line diff in front of a reviewer and make a
    /// one-line change unreadable.
    #[test]
    fn a_crlf_file_stays_crlf_after_an_edit() {
        let path = scratch("keep.rs", "a\r\nb\r\nc\r\n");
        replace(&path, "keep.rs", "b", "beta", false).expect("edit");
        let after = fs::read_to_string(&path).expect("read back");
        assert_eq!(after, "a\r\nbeta\r\nc\r\n", "the file's convention must survive");
        assert!(!after.contains("\n\n"), "no stray bare newline was introduced");
    }

    /// An LF file stays LF — the normalisation is symmetric and adds nothing.
    #[test]
    fn an_lf_file_stays_lf_after_an_edit() {
        let path = scratch("lf.rs", "a\nb\nc\n");
        replace(&path, "lf.rs", "a\nb", "a\nbeta", false).expect("edit");
        assert_eq!(fs::read_to_string(&path).expect("read back"), "a\nbeta\nc\n");
    }

    /// An anchor that genuinely is not there is still refused, and an ambiguous
    /// one still demands `--all`. Normalising must not make the tool laxer.
    #[test]
    fn a_missing_anchor_and_an_ambiguous_one_are_both_still_refused() {
        let path = scratch("guard.rs", "x\r\nx\r\n");
        assert!(replace(&path, "guard.rs", "nowhere", "y", false).is_err());
        assert!(
            replace(&path, "guard.rs", "x", "y", false).is_err(),
            "two matches without --all must refuse rather than pick one"
        );
        assert_eq!(
            replace(&path, "guard.rs", "x", "y", true).expect("--all").replacements,
            2
        );
    }
}
