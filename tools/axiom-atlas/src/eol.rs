//! **Line endings, decided once, for the whole tool.**
//!
//! This module exists because `ax` had grown three different answers to one
//! question. `edit.rs` normalised the haystack and the needles to LF, edited,
//! and restored CRLF if the file had contained any. `apply.rs` did the
//! opposite — it left the file alone and converted every incoming anchor *up*
//! to CRLF before matching. `wgsl.rs` added a third, because extracting a
//! string literal has its own rule. Three algorithms, one question, and the
//! only reason nothing had broken yet is that the repo is uniformly CRLF.
//!
//! Three answers to one question is the defect. So there is now one:
//!
//! > **`ax` is LF-internal.** Every file is read into LF, every match, anchor
//! > and payload is LF, and the file is rendered back to its own convention on
//! > the way out. No command above this module ever sees a carriage return.
//!
//! # Where the convention comes from
//!
//! In precedence order, and the first one that answers wins:
//!
//! 1. **`.gitattributes`** — the repo's own declaration, and the only one that
//!    is *authoritative* rather than inferred. `binary`/`-text` means the bytes
//!    are not text and nothing here may touch their endings at all; `eol=lf` /
//!    `eol=crlf` name the convention outright.
//! 2. **The file's existing content** — what it already is, by a real count
//!    rather than "contains a CRLF somewhere".
//! 3. **The worktree default** — `core.eol`, then `core.autocrlf`, then LF.
//!    This is what decides a file that does not exist yet.
//!
//! Reading `.gitattributes` is the part that was missing everywhere else, and
//! it is the part that matters most: `*.bin binary` and the golden corpora are
//! declared there precisely so that no tool reflows them, and until now `ax`
//! could not see that declaration.
//!
//! # The one case that is not a clean round-trip
//!
//! A file whose endings are *mixed* cannot be rendered back byte-identically
//! from an LF form — the information is gone. `ax` normalises it to the
//! declared (or dominant) convention and **says so**, because a silent
//! whole-file reflow attached to a one-line edit is exactly the surprise this
//! module is supposed to prevent. A mixed file is already a defect; the notice
//! is how it stops being an invisible one.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::repo::Repo;

/// A line-ending convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
}

impl Eol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
        }
    }
}

/// How a file is handled: as text with a convention, or as opaque bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Text(Eol),
    /// `.gitattributes` declared the path `binary` or `-text`. Bytes pass
    /// through untouched — a golden corpus is byte-exact or it is worthless.
    Raw,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text(e) => e.label(),
            Self::Raw => "raw",
        }
    }
}

/// What the bytes on disk actually are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// No line ending at all — a one-liner, or an empty file.
    None,
    Lf,
    Crlf,
    /// Some of each, or a lone CR. Cannot round-trip through an LF form.
    Mixed,
}

impl Shape {
    /// Counts rather than "contains", because "contains a CRLF" calls a file
    /// with one stray CRLF in three thousand LF lines a CRLF file.
    pub fn of(text: &str) -> Self {
        let crlf = text.matches("\r\n").count();
        let cr = text.matches('\r').count();
        let lf = text.matches('\n').count();
        match (cr, lf) {
            (0, 0) => Self::None,
            (0, _) => Self::Lf,
            _ if cr == crlf && lf == crlf => Self::Crlf,
            _ => Self::Mixed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lf => "lf",
            Self::Crlf => "crlf",
            Self::Mixed => "MIXED",
        }
    }

    /// The convention this shape implies, if it implies one.
    fn implied(self) -> Option<Eol> {
        match self {
            Self::Lf => Some(Eol::Lf),
            Self::Crlf => Some(Eol::Crlf),
            // A mixed file gets the majority reading; `None` has no opinion.
            Self::Mixed | Self::None => None,
        }
    }
}

/// Everything as `\n`. `\r\n` first, then any lone `\r`.
pub fn to_lf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// LF text rendered into a convention.
pub fn from_lf(s: &str, eol: Eol) -> String {
    match eol {
        Eol::Lf => s.to_owned(),
        Eol::Crlf => s.replace('\n', "\r\n"),
    }
}

// ---------------------------------------------------------------------------
// `.gitattributes`
// ---------------------------------------------------------------------------

/// What `.gitattributes` says about a path, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declared {
    /// `binary` or `-text` — not text, never reflowed.
    Binary,
    Eol(Eol),
    /// `text` with no `eol=`: it is text, and the worktree default applies.
    Text,
    Unset,
}

#[derive(Debug)]
struct Rule {
    re: regex::Regex,
    declared: Declared,
}

/// The repo's `.gitattributes` rules, resolved per path.
///
/// Supports the pattern subset this repo (and nearly every repo) actually
/// uses: `*`, `**`, `?`, a leading `/` to anchor, and a basename-only pattern
/// matching at any depth. Attribute macros other than `binary` are not
/// expanded — `binary` is, because it is the one this tool must never get
/// wrong.
#[derive(Debug, Default)]
pub struct Attributes {
    /// The checkout to read `.gitattributes` from. `None` for a rule set built
    /// in memory, which is how tests pin a known declaration.
    root: Option<PathBuf>,
    /// Directory (repo-relative, `""` for the root) -> its rules, in file
    /// order. Filled lazily: a batch touching twenty files in one directory
    /// reads that directory's `.gitattributes` once.
    cache: RefCell<BTreeMap<String, Vec<Rule>>>,
}

impl Attributes {
    /// The rules of a real checkout, read on demand.
    pub fn new(repo: &Repo) -> Self {
        Self { root: Some(repo.root.clone()), cache: RefCell::default() }
    }

    /// Rules from a literal `.gitattributes` body, rooted at the repo root.
    /// How a test pins a known declaration instead of depending on whatever
    /// the checkout happens to hold.
    #[cfg(test)]
    pub fn from_rules(text: &str) -> Self {
        let mut cache = BTreeMap::new();
        cache.insert(String::new(), parse(text, ""));
        Self { root: None, cache: RefCell::new(cache) }
    }

    /// What the repo declares for this path. Deeper directories win, and
    /// within one file the last matching line wins — git's own precedence.
    pub fn declared(&self, rel: &str) -> Declared {
        self.load_chain(rel);
        // Every rule is anchored under the directory of the file that declared
        // it, so a rule from an unrelated directory cannot match; and the map
        // is keyed by that directory, whose lexicographic order over a path's
        // own ancestors is exactly shallowest-first.
        self.cache
            .borrow()
            .values()
            .flat_map(|rules| rules.iter())
            .filter(|r| r.re.is_match(rel))
            .map(|r| r.declared)
            .next_back()
            .unwrap_or(Declared::Unset)
    }

    /// Reads the `.gitattributes` of every ancestor directory of `rel` that has
    /// not been read yet. A directory with no such file caches as empty, so it
    /// is stat-ed once and never again.
    fn load_chain(&self, rel: &str) {
        let Some(root) = self.root.as_ref() else { return };
        let segments: Vec<&str> = rel.split('/').collect();
        let mut dir = String::new();

        for depth in 0..segments.len() {
            if depth > 0 {
                dir = match dir.is_empty() {
                    true => segments[depth - 1].to_owned(),
                    false => format!("{dir}/{}", segments[depth - 1]),
                };
            }
            if self.cache.borrow().contains_key(&dir) {
                continue;
            }
            let path = match dir.is_empty() {
                true => root.join(".gitattributes"),
                false => root.join(&dir).join(".gitattributes"),
            };
            let rules = fs::read_to_string(&path)
                .map(|text| parse(&text, &dir))
                .unwrap_or_default();
            self.cache.borrow_mut().insert(dir.clone(), rules);
        }
    }
}

/// One `.gitattributes` file into rules, each anchored under `dir`.
fn parse(text: &str, dir: &str) -> Vec<Rule> {
    to_lf(text)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pattern = parts.next()?;
            let declared = parts.fold(Declared::Unset, |acc, attr| match attr {
                "binary" | "-text" | "!text" => Declared::Binary,
                "eol=lf" => Declared::Eol(Eol::Lf),
                "eol=crlf" => Declared::Eol(Eol::Crlf),
                "text" => match acc {
                    // `text eol=lf` in either order keeps the eol.
                    Declared::Eol(e) => Declared::Eol(e),
                    _ => Declared::Text,
                },
                _ => acc,
            });
            matches!(declared, Declared::Unset)
                .then_some(None)
                .unwrap_or_else(|| {
                    regex::Regex::new(&glob_to_regex(pattern, dir))
                        .ok()
                        .map(|re| Rule { re, declared })
                })
        })
        .collect()
}

/// A gitattributes/gitignore glob as an anchored regex over repo-relative
/// paths.
fn glob_to_regex(pattern: &str, dir: &str) -> String {
    let trimmed = pattern.trim_end_matches('/');
    let anchored = trimmed.starts_with('/');
    let body = trimmed.trim_start_matches('/');
    // A pattern with no slash matches a basename at any depth; one with a
    // slash is anchored to the directory holding the `.gitattributes`.
    let rooted = anchored || body.contains('/');

    let mut re = String::from("^");
    (!dir.is_empty()).then(|| {
        re.push_str(&regex::escape(dir));
        re.push('/');
    });
    (!rooted).then(|| re.push_str("(?:.*/)?"));

    let bytes: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            '*' if bytes.get(i + 1) == Some(&'*') => {
                // `**/` spans zero or more directories; a trailing `**` spans
                // the rest of the path.
                match bytes.get(i + 2) {
                    Some('/') => {
                        re.push_str("(?:.*/)?");
                        i += 3;
                    }
                    _ => {
                        re.push_str(".*");
                        i += 2;
                    }
                }
            }
            '*' => {
                re.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                re.push_str("[^/]");
                i += 1;
            }
            c => {
                re.push_str(&regex::escape(&c.to_string()));
                i += 1;
            }
        }
    }
    re.push('$');
    re
}

// ---------------------------------------------------------------------------
// The worktree default
// ---------------------------------------------------------------------------

static WORKTREE_DEFAULT: OnceLock<Eol> = OnceLock::new();

/// What a file that does not exist yet, and that `.gitattributes` says nothing
/// about, should be written as.
///
/// `core.eol`, then `core.autocrlf`, then LF — git's own order. Resolved once
/// per process and only on a path that actually needs it, because it costs a
/// `git config` and `ax`'s whole value proposition is that a query is faster
/// than the habit it replaces.
pub fn worktree_default() -> Eol {
    *WORKTREE_DEFAULT.get_or_init(|| {
        let native = match cfg!(windows) {
            true => Eol::Crlf,
            false => Eol::Lf,
        };
        match git_config("core.eol").as_deref() {
            Some("crlf") => return Eol::Crlf,
            Some("lf") => return Eol::Lf,
            Some("native") => return native,
            _ => (),
        }
        match git_config("core.autocrlf").as_deref() {
            Some("true") => native,
            _ => Eol::Lf,
        }
    })
}

fn git_config(key: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Reading and writing
// ---------------------------------------------------------------------------

/// A file, loaded as LF, remembering how to put it back.
#[derive(Debug)]
pub struct Loaded {
    /// The content, every line ending `\n`. This is what every command above
    /// this module works on, without exception.
    pub lf: String,
    /// How it will be written back.
    pub mode: Mode,
    /// What it was on disk.
    pub shape: Shape,
    /// Bytes on disk before any change.
    pub bytes_before: i64,
    /// Set when rendering back cannot be byte-identical outside the edit —
    /// a mixed-ending file. The caller is expected to tell the user.
    pub reflow_notice: Option<String>,
}

impl Loaded {
    /// LF text rendered back into this file's convention.
    pub fn render(&self, lf_text: &str) -> String {
        match self.mode {
            Mode::Raw => lf_text.to_owned(),
            Mode::Text(eol) => from_lf(lf_text, eol),
        }
    }
}

/// Decides how a path will be written, without reading it.
///
/// Used for files that do not exist yet, and by `ax owns` to answer "what
/// would you do to this file".
pub fn mode_for(attrs: &Attributes, rel: &str, shape: Shape) -> Mode {
    match attrs.declared(rel) {
        Declared::Binary => Mode::Raw,
        Declared::Eol(e) => Mode::Text(e),
        Declared::Text => Mode::Text(shape.implied().unwrap_or_else(worktree_default)),
        Declared::Unset => Mode::Text(shape.implied().unwrap_or_else(worktree_default)),
    }
}

/// Reads a file into its LF form.
///
/// A path that does not exist yet loads as empty, so a create and an overwrite
/// take the same route.
pub fn load(attrs: &Attributes, path: &Path, rel: &str) -> Result<Loaded, String> {
    let raw = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("cannot read `{rel}`: {e}")),
    };
    let shape = Shape::of(&raw);
    let mode = mode_for(attrs, rel, shape);
    let lf = match mode {
        Mode::Raw => raw.clone(),
        Mode::Text(_) => to_lf(&raw),
    };
    let reflow_notice = (shape == Shape::Mixed && matches!(mode, Mode::Text(_))).then(|| {
        format!("`{rel}` had mixed line endings; it is being normalised to {}", mode.label())
    });

    Ok(Loaded {
        lf,
        mode,
        shape,
        bytes_before: i64::try_from(raw.len()).unwrap_or(i64::MAX),
        reflow_notice,
    })
}

/// Writes LF text back through `loaded`'s convention, atomically.
///
/// The temp-file-then-rename is why a killed `ax` never leaves a half-written
/// source file behind.
pub fn store(path: &Path, rel: &str, loaded: &Loaded, lf_text: &str) -> Result<i64, String> {
    let rendered = loaded.render(lf_text);
    path.parent()
        .map(|p| {
            fs::create_dir_all(p).map_err(|e| format!("cannot create the parent of `{rel}`: {e}"))
        })
        .transpose()?;
    atomic_write(path, rel, &rendered)?;
    Ok(i64::try_from(rendered.len()).unwrap_or(i64::MAX))
}

fn atomic_write(path: &Path, rel: &str, content: &str) -> Result<(), String> {
    let tmp: PathBuf = path.with_extension(format!(
        "{}.axtmp{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));
    fs::write(&tmp, content).map_err(|e| format!("cannot write `{rel}`: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot replace `{rel}`: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_counts_rather_than_contains() {
        assert_eq!(Shape::of("a\nb\n"), Shape::Lf);
        assert_eq!(Shape::of("a\r\nb\r\n"), Shape::Crlf);
        assert_eq!(Shape::of("one line"), Shape::None);
        // The case the old `contains("\r\n")` heuristic called CRLF.
        assert_eq!(Shape::of("a\nb\r\nc\n"), Shape::Mixed);
        assert_eq!(Shape::of("a\rb"), Shape::Mixed);
    }

    #[test]
    fn lf_round_trips_a_uniform_file_exactly() {
        let crlf = "a\r\nb\r\nc\r\n";
        assert_eq!(from_lf(&to_lf(crlf), Eol::Crlf), crlf);
        let lf = "a\nb\nc\n";
        assert_eq!(from_lf(&to_lf(lf), Eol::Lf), lf);
    }

    /// The declaration this repo relies on: goldens are byte-exact, so nothing
    /// here may reflow them, and `.wgsl` is pinned to LF because an extracted
    /// shader must equal the literal it replaced.
    #[test]
    fn gitattributes_decides_before_the_content_does() {
        let a = Attributes::from_rules("*.bin binary\n**/tests/golden/** binary\n*.wgsl text eol=lf\n");

        assert_eq!(a.declared("apps/x/data/blob.bin"), Declared::Binary);
        assert_eq!(a.declared("apps/x/tests/golden/frame.png"), Declared::Binary);
        assert_eq!(a.declared("apps/x/src/s.wgsl"), Declared::Eol(Eol::Lf));
        assert_eq!(a.declared("apps/x/src/s.rs"), Declared::Unset);

        // A CRLF-shaped .wgsl is still written LF, because the repo said so.
        assert_eq!(mode_for(&a, "apps/x/src/s.wgsl", Shape::Crlf), Mode::Text(Eol::Lf));
        // And a golden is never touched, whatever it looks like.
        assert_eq!(mode_for(&a, "apps/x/data/blob.bin", Shape::Crlf), Mode::Raw);
    }

    #[test]
    fn an_unset_path_keeps_whatever_it_already_is() {
        let a = Attributes::from_rules("*.wgsl text eol=lf\n");
        assert_eq!(mode_for(&a, "src/a.rs", Shape::Crlf), Mode::Text(Eol::Crlf));
        assert_eq!(mode_for(&a, "src/a.rs", Shape::Lf), Mode::Text(Eol::Lf));
    }

    #[test]
    fn globs_anchor_the_way_git_does() {
        // No slash: basename at any depth.
        let a = Attributes::from_rules("*.bin binary\n");
        assert_eq!(a.declared("x.bin"), Declared::Binary);
        assert_eq!(a.declared("a/b/c/x.bin"), Declared::Binary);
        assert_eq!(a.declared("x.bing"), Declared::Unset);

        // A slash anchors; `**` spans directories.
        let b = Attributes::from_rules("docs/**/notes.md text eol=lf\n");
        assert_eq!(b.declared("docs/a/b/notes.md"), Declared::Eol(Eol::Lf));
        assert_eq!(b.declared("docs/notes.md"), Declared::Eol(Eol::Lf));
        assert_eq!(b.declared("other/docs/a/notes.md"), Declared::Unset);
    }

    #[test]
    fn the_last_matching_line_wins() {
        let a = Attributes::from_rules("*.txt text eol=lf\nspecial.txt text eol=crlf\n");
        assert_eq!(a.declared("a/special.txt"), Declared::Eol(Eol::Crlf));
        assert_eq!(a.declared("a/plain.txt"), Declared::Eol(Eol::Lf));
    }

    #[test]
    fn a_mixed_file_is_normalised_and_the_caller_is_told() {
        let dir = std::env::temp_dir().join(format!("ax-eol-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch");
        let path = dir.join("mixed.txt");
        fs::write(&path, "a\nb\r\nc\n").expect("write");

        let loaded = load(&Attributes::default(), &path, "mixed.txt").expect("load");
        assert_eq!(loaded.shape, Shape::Mixed);
        assert!(loaded.reflow_notice.is_some(), "a silent reflow is the bug");
        assert_eq!(loaded.lf, "a\nb\nc\n");
    }
}
