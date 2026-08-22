//! Repo discovery and the hard path-scoping boundary.
//!
//! Every path an agent hands to `ax` passes through [`Repo::resolve`], which is
//! the single choke point that keeps this tool incapable of reading or writing
//! anything outside the checkout it was invoked in. It resolves symlinks before
//! it compares, so neither `../../etc/passwd` nor a symlink planted inside the
//! repo can escape.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Why a path was refused. Every variant is a refusal to leave the repo.
#[derive(Debug)]
pub enum ScopeError {
    /// The path resolved to somewhere outside the repo root.
    Outside { raw: String, resolved: PathBuf },
    /// The path reaches into git's own internals.
    GitInternals { raw: String },
    /// The path could not be resolved at all.
    Unresolvable { raw: String, err: io::Error },
}

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Outside { raw, resolved } => write!(
                f,
                "path `{raw}` resolves to `{}`, which is outside this repository. \
                 ax only ever touches the repo it was invoked in.",
                resolved.display()
            ),
            Self::GitInternals { raw } => {
                write!(f, "path `{raw}` reaches into `.git/`, which ax never touches.")
            }
            Self::Unresolvable { raw, err } => write!(f, "path `{raw}` cannot be resolved: {err}"),
        }
    }
}

/// A resolved repository checkout.
pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    /// Finds the repo root by walking up from `AXIOM_ATLAS_ROOT` (or the cwd)
    /// until a `.git` entry appears.
    ///
    /// `.git` is matched as either a directory or a file, so this lands on the
    /// *worktree* root when invoked inside one of `.claude/worktrees/*` — which
    /// is what we want: a worktree is its own scope.
    pub fn discover() -> io::Result<Self> {
        let start = env::var_os("AXIOM_ATLAS_ROOT")
            .map(PathBuf::from)
            .map_or_else(env::current_dir, Ok)?
            .canonicalize()?;

        let mut cur: &Path = &start;
        loop {
            if cur.join(".git").exists() {
                return Ok(Self { root: cur.to_path_buf() });
            }
            cur = cur.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("`{}` is not inside a git repository", start.display()),
                )
            })?;
        }
    }

    /// Resolves a user-supplied path and proves it lands inside the repo.
    ///
    /// The leaf need not exist yet (so `ax write` can create files), but every
    /// existing ancestor is symlink-resolved before the containment check.
    pub fn resolve(&self, raw: &str) -> Result<PathBuf, ScopeError> {
        let candidate = {
            let p = Path::new(raw);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.root.join(p)
            }
        };

        let resolved = resolve_deepest_existing(&candidate)
            .map_err(|err| ScopeError::Unresolvable { raw: raw.to_owned(), err })?;

        if !resolved.starts_with(&self.root) {
            return Err(ScopeError::Outside { raw: raw.to_owned(), resolved });
        }
        if resolved.components().any(|c| c.as_os_str() == ".git") {
            return Err(ScopeError::GitInternals { raw: raw.to_owned() });
        }
        Ok(resolved)
    }

    /// Repo-relative, forward-slashed display form — the shape every `ax`
    /// command prints, so output is stable across platforms.
    pub fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

/// Canonicalizes the deepest existing ancestor of `candidate`, then re-applies
/// the not-yet-existing tail, folding `..` lexically.
fn resolve_deepest_existing(candidate: &Path) -> io::Result<PathBuf> {
    let mut tail: Vec<OsString> = Vec::new();
    let mut cur = candidate.to_path_buf();

    loop {
        match cur.canonicalize() {
            Ok(base) => {
                let mut out = base;
                for part in tail.iter().rev() {
                    if part == ".." {
                        out.pop();
                    } else if part != "." {
                        out.push(part);
                    }
                }
                return Ok(out);
            }
            Err(err) => {
                let name = cur.file_name().map(std::ffi::OsStr::to_os_string);
                match name {
                    Some(n) => {
                        tail.push(n);
                        cur.pop();
                    }
                    None => return Err(err),
                }
            }
        }
    }
}
