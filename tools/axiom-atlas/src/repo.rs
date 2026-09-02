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
use std::process::Command;

/// A file's contents at a git revision, or `None` if it did not exist there.
///
/// Reading history is how a command checks its own work: `ax cite` resolves a
/// citation against a baseline revision, and `ax wgsl --verify` proves an
/// extracted `.wgsl` still equals the string literal it replaced. Both need the
/// same three lines, so they share them.
pub fn git_show(root: &Path, rev: &str, path: &str) -> Option<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["show", &format!("{rev}:{path}")])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

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

//// A resolved repository checkout, plus any read-only reference roots.
pub struct Repo {
    pub root: PathBuf,
    /// Trees `ax read` may look into but that nothing may ever write to, from
    /// `AXIOM_ATLAS_REF_ROOTS` (platform path separator, like `PATH`).
    ///
    /// This exists because a PORT reads its source from outside the checkout.
    /// Without it every one of those reads goes around `ax`, which loses the
    /// scoping story for the files an agent spends most of its time in and
    /// makes them invisible to the ledger. The ledger is worth having because
    /// it records what agents actually look at; a port that reads sixty
    /// thousand lines of JavaScript through `cat` reports none of it.
    ///
    /// Read-only is the whole point, and it is enforced structurally: `resolve`
    /// — which every mutating command calls — still checks `root` alone. A
    /// reference root is somewhere to LOOK, never somewhere to change.
    ///
    /// Empty unless the variable is set, so the default is what it always was.
    pub refs: Vec<PathBuf>,
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
                return Ok(Self { root: cur.to_path_buf(), refs: reference_roots() });
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
                // Relative paths resolve against the CWD, like every other
                // CLI, so `ax owns src/lib.rs` works from inside a crate
                // rather than silently resolving to <root>/src/lib.rs. That
                // bug was invisible while everything ran from the repo root
                // and only surfaced once `ax` was installed on PATH.
                //
                // A repo-root-relative path stays valid as a fallback when it
                // exists and the CWD form does not, because `ax apply`
                // batches and the Claude Code hooks both speak repo-relative
                // paths. The CWD wins any tie, and a path that exists in
                // neither place is reported against the CWD, which is where a
                // caller creating a new file means to put it.
                let from_cwd = env::current_dir()
                    .unwrap_or_else(|_| self.root.clone())
                    .join(p);
                let from_root = self.root.join(p);
                if from_cwd.exists() || !from_root.exists() {
                    from_cwd
                } else {
                    from_root
                }
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

    /// Resolves a path for READING, which may also land in a reference root.
    ///
    /// Separate from [`Self::resolve`] on purpose, and the separation is the
    /// safety property: every command that changes a file calls `resolve`, so
    /// no reference root is reachable by anything that writes. Adding a
    /// readable tree can therefore never widen what `ax` can modify.
    pub fn resolve_read(&self, raw: &str) -> Result<PathBuf, ScopeError> {
        self.resolve(raw).or_else(|err| match &err {
            // A `.git/` refusal stands even for a reference root: it is never
            // an interesting thing to read and always an expensive mistake.
            ScopeError::GitInternals { .. } | ScopeError::Unresolvable { .. } => Err(err),
            ScopeError::Outside { resolved, raw } => {
                // `resolve` reports `Outside` BEFORE it ever looks for `.git`,
                // so the git test has to be re-applied here — a reference root
                // has its own `.git/`, and it is never an interesting thing to
                // read.
                let in_git = resolved.components().any(|c| c.as_os_str() == ".git");
                let reachable = self.refs.iter().any(|r| resolved.starts_with(r));
                match (reachable, in_git) {
                    (true, false) => Ok(resolved.clone()),
                    (true, true) => Err(ScopeError::GitInternals { raw: raw.clone() }),
                    (false, _) => Err(err),
                }
            }
        })
    }

    /// Repo-relative, forward-slashed display form — the shape every `ax`
    /// command prints, so output is stable across platforms.
    /// A path in a reference root has no repo-relative form, so it prints
    /// absolute with a `ref:` marker — an agent reading the output should never
    /// be left thinking a file outside the checkout is inside it.
    pub fn rel(&self, p: &Path) -> String {
        if !p.starts_with(&self.root) {
            return format!("ref:{}", p.to_string_lossy().replace('\\', "/"));
        }
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

//// The read-only reference roots from `AXIOM_ATLAS_REF_ROOTS`.
///
/// Split on the platform separator, like `PATH`. A root that does not exist, or
/// cannot be canonicalized, is dropped silently: a stale entry in an
/// environment variable should never stop `ax` from working on the repo, which
/// is the job it is actually there to do.
fn reference_roots() -> Vec<PathBuf> {
    env::var_os("AXIOM_ATLAS_REF_ROOTS")
        .map(|raw| {
            env::split_paths(&raw)
                .filter_map(|p| p.canonicalize().ok())
                .collect()
        })
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A `Repo` with an explicit ref list, so a test never depends on the
    /// ambient `AXIOM_ATLAS_REF_ROOTS` (and never has to set a process-global
    /// env var, which two tests running in parallel would fight over).
    fn repo_with_refs(refs: Vec<PathBuf>) -> Repo {
        Repo {
            root: env::current_dir()
                .expect("a cwd")
                .canonicalize()
                .expect("cwd canonicalizes"),
            refs,
        }
    }

    /// A scratch directory containing one file and one `.git/HEAD`, standing in
    /// for a reference checkout.
    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("ax-scope-{name}-{}", std::process::id()));
        fs::create_dir_all(dir.join(".git")).expect("scratch .git");
        fs::write(dir.join("source.js"), "// reference\n").expect("scratch file");
        fs::write(dir.join(".git").join("HEAD"), "ref: refs/heads/main\n").expect("scratch HEAD");
        dir.canonicalize().expect("scratch canonicalizes")
    }

    /// **The three escape shapes are refused.** Absolute, traversal, and
    /// `.git/` — the guarantee the whole tool rests on, asserted rather than
    /// checked by hand once.
    #[test]
    fn every_escape_shape_is_refused() {
        let repo = repo_with_refs(Vec::new());
        let up = env::temp_dir();
        assert!(
            matches!(repo.resolve(&up.to_string_lossy()), Err(ScopeError::Outside { .. })),
            "an absolute path outside the repo must be refused"
        );
        assert!(
            matches!(repo.resolve("../.."), Err(ScopeError::Outside { .. })),
            "a traversal out of the repo must be refused"
        );
        assert!(
            matches!(repo.resolve(".git/HEAD"), Err(ScopeError::GitInternals { .. })),
            "git internals are never touched"
        );
    }

    /// **A path that does not exist yet still resolves**, so `ax write` can
    /// create a file — but only inside the repo.
    #[test]
    fn a_not_yet_existing_path_resolves_inside_the_repo() {
        let repo = repo_with_refs(Vec::new());
        assert!(repo.resolve("does/not/exist/yet.rs").is_ok());
    }

    /// **With no reference roots configured, reading and writing scope
    /// identically.** The default behaviour is exactly what it was before
    /// reference roots existed.
    #[test]
    fn without_reference_roots_reads_are_scoped_like_writes() {
        let repo = repo_with_refs(Vec::new());
        let outside = scratch("noref").join("source.js");
        let raw = outside.to_string_lossy().to_string();
        assert!(repo.resolve(&raw).is_err());
        assert!(
            repo.resolve_read(&raw).is_err(),
            "a reference root that was never configured must not be reachable"
        );
    }

    /// **A reference root is readable and never writable.**
    ///
    /// This is the property that makes widening the readable set safe: the
    /// writable set is decided by `resolve`, which every mutating command
    /// calls and which does not consult `refs` at all.
    #[test]
    fn a_reference_root_is_readable_but_never_writable() {
        let dir = scratch("readonly");
        let repo = repo_with_refs(vec![dir.clone()]);
        let file = dir.join("source.js").to_string_lossy().to_string();

        assert!(repo.resolve_read(&file).is_ok(), "a reference root must be readable");
        assert!(
            matches!(repo.resolve(&file), Err(ScopeError::Outside { .. })),
            "and must STILL be refused by the resolver every write goes through"
        );
    }

    /// **`.git/` stays refused inside a reference root too.**
    ///
    /// Regression: `resolve` reports `Outside` *before* it looks for `.git`, so
    /// the fallback that accepts reference roots skipped the git test entirely
    /// and happily read `.git/HEAD` out of the reference checkout. The doc
    /// comment claimed otherwise, which is worse than the hole.
    #[test]
    fn git_internals_are_refused_inside_a_reference_root() {
        let dir = scratch("gitref");
        let repo = repo_with_refs(vec![dir.clone()]);
        let head = dir.join(".git").join("HEAD").to_string_lossy().to_string();
        assert!(
            matches!(repo.resolve_read(&head), Err(ScopeError::GitInternals { .. })),
            "a reference root's git internals are as off-limits as the repo's"
        );
    }

    /// **Somewhere that is neither the repo nor a reference root is still
    /// refused**, so configuring one root does not open the filesystem.
    #[test]
    fn a_third_location_is_refused_even_with_a_reference_root_set() {
        let allowed = scratch("allowed");
        let other = scratch("other");
        let repo = repo_with_refs(vec![allowed]);
        let file = other.join("source.js").to_string_lossy().to_string();
        assert!(
            matches!(repo.resolve_read(&file), Err(ScopeError::Outside { .. })),
            "only the roots that were configured are reachable"
        );
    }

    /// A reference path prints with a `ref:` marker, so nothing in the output
    /// can be mistaken for a file inside the checkout.
    #[test]
    fn a_reference_path_prints_as_a_reference() {
        let dir = scratch("display");
        let repo = repo_with_refs(vec![dir.clone()]);
        let shown = repo.rel(&dir.join("source.js"));
        assert!(shown.starts_with("ref:"), "got `{shown}`");
        assert!(!shown.contains('\\'), "paths print forward-slashed: `{shown}`");
    }
}
