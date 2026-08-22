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
pub fn replace(
    path: &Path,
    label: &str,
    old: &str,
    new: &str,
    all: bool,
) -> Result<EditOutcome, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read `{label}`: {e}"))?;

    let count = text.matches(old).count();
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

    let updated = if all { text.replace(old, new) } else { text.replacen(old, new, 1) };
    let bytes_before = text.len() as i64;
    let bytes_after = updated.len() as i64;
    atomic_write(path, label, &updated)?;

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
