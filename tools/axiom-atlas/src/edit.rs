//! Scoped, atomic single-file changes.
//!
//! Paths have already passed `Repo::resolve`, so a write cannot land outside
//! the checkout, and every write goes through [`crate::eol::store`] — a temp
//! file in the same directory followed by a rename — so a killed `ax` never
//! leaves a half-written source file behind.
//!
//! # Line endings live in one place now
//!
//! This module used to carry its own `lf`/`restore` pair, and `apply.rs`
//! carried a different one. Both are gone: [`crate::eol`] loads a file into LF,
//! everything here matches and edits in LF, and the file is rendered back into
//! its own convention — or into whatever `.gitattributes` declares — on the way
//! out. The behaviour that mattered is unchanged and still tested below: a
//! multi-line anchor typed with `\n` matches a CRLF file, and that file is
//! still CRLF afterwards.

use std::path::Path;

use crate::eol::{self, Attributes};

#[derive(Debug)]
pub struct EditOutcome {
    pub replacements: usize,
    pub bytes_before: i64,
    pub bytes_after: i64,
    /// Set when the write also normalised a mixed-ending file, which is a
    /// bigger diff than the caller asked for and must not happen silently.
    pub notice: Option<String>,
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
/// # Why the anchor is normalised
///
/// The anchor an agent types has `\n` in it. Half the files in a Windows
/// checkout have `\r\n` on disk, because git converts on checkout. Matching the
/// two literally means **every multi-line anchor fails** on those files — and
/// fails with "text does not occur", which is untrue and sends the agent
/// hunting for a typo that is not there. That is exactly the friction that
/// makes an agent abandon the tool for a hand-rolled script.
pub fn replace(
    attrs: &Attributes,
    path: &Path,
    label: &str,
    old: &str,
    new: &str,
    all: bool,
) -> Result<EditOutcome, String> {
    let loaded = eol::load(attrs, path, label)?;
    let old = eol::to_lf(old);
    let new = eol::to_lf(new);

    let count = loaded.lf.matches(&old).count();
    if count == 0 {
        return Err(format!(
            "`--replace` text does not occur in `{label}`. Nothing was written."
        ));
    }
    if count > 1 && !all {
        return Err(format!(
            "`--replace` text occurs {count} times in `{label}`. Pass --all to change every \
             occurrence, or extend the anchor until it is unique. Nothing was written."
        ));
    }

    let updated = match all {
        true => loaded.lf.replace(&old, &new),
        false => loaded.lf.replacen(&old, &new, 1),
    };
    // Byte counts are reported in the file's OWN convention, so the number an
    // agent sees matches what landed on disk rather than the normalised form.
    let bytes_after = eol::store(path, label, &loaded, &updated)?;

    Ok(EditOutcome {
        replacements: match all {
            true => count,
            false => 1,
        },
        bytes_before: loaded.bytes_before,
        bytes_after,
        notice: loaded.reflow_notice.clone(),
    })
}

/// Creates or overwrites a file with `content`.
pub fn write(
    attrs: &Attributes,
    path: &Path,
    label: &str,
    content: &str,
) -> Result<EditOutcome, String> {
    let loaded = eol::load(attrs, path, label)?;
    let bytes_after = eol::store(path, label, &loaded, &eol::to_lf(content))?;
    Ok(EditOutcome {
        replacements: 1,
        bytes_before: loaded.bytes_before,
        bytes_after,
        notice: None,
    })
}

/// Reads a file, optionally a 1-based inclusive line range, with line anchors.
pub fn read_lines(
    path: &Path,
    label: &str,
    range: Option<(usize, usize)>,
) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read `{label}`: {e}"))?;
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
    use std::fs;

    fn scratch(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ax-edit-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(name);
        fs::write(&path, body).expect("scratch file");
        path
    }

    fn plain() -> Attributes {
        Attributes::default()
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
        let out = replace(&plain(), &path, "crlf.rs", "    one();\n    two();", "    only();", false)
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
        replace(&plain(), &path, "keep.rs", "b", "beta", false).expect("edit");
        let after = fs::read_to_string(&path).expect("read back");
        assert_eq!(after, "a\r\nbeta\r\nc\r\n", "the file's convention must survive");
    }

    /// An LF file stays LF — the normalisation is symmetric and adds nothing.
    #[test]
    fn an_lf_file_stays_lf_after_an_edit() {
        let path = scratch("lf.rs", "a\nb\nc\n");
        replace(&plain(), &path, "lf.rs", "a\nb", "a\nbeta", false).expect("edit");
        assert_eq!(fs::read_to_string(&path).expect("read back"), "a\nbeta\nc\n");
    }

    /// An anchor that genuinely is not there is still refused, and an ambiguous
    /// one still demands `--all`. Normalising must not make the tool laxer.
    #[test]
    fn a_missing_anchor_and_an_ambiguous_one_are_both_still_refused() {
        let path = scratch("guard.rs", "x\r\nx\r\n");
        assert!(replace(&plain(), &path, "guard.rs", "nowhere", "y", false).is_err());
        assert!(
            replace(&plain(), &path, "guard.rs", "x", "y", false).is_err(),
            "two matches without --all must refuse rather than pick one"
        );
        assert_eq!(
            replace(&plain(), &path, "guard.rs", "x", "y", true).expect("--all").replacements,
            2
        );
    }

    /// A write into a file the repo declares `eol=lf` lands LF even though the
    /// caller's text and the machine are both CRLF. This is the guarantee the
    /// extracted `.wgsl` files depend on.
    #[test]
    fn a_declared_eol_beats_the_callers_text() {
        let path = scratch("pinned.wgsl", "");
        let attrs = eol::Attributes::from_rules("*.wgsl text eol=lf\n");
        write(&attrs, &path, "pinned.wgsl", "a\r\nb\r\n").expect("write");
        assert_eq!(fs::read_to_string(&path).expect("read back"), "a\nb\n");
    }
}
