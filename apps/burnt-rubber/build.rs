//! Stamps the commit this binary was built from into the binary itself.
//!
//! # Why an app needs this
//!
//! Burnt Rubber is developed across several git worktrees at once, each served
//! on its own port by `localhost_servers.py`, each producing its own wasm
//! bundle. A browser tab shows a car on a road; it does not show which tree
//! compiled it. That gap has a specific cost, and it is not hypothetical: a wasm
//! build that fails after the last good one leaves `axiom-serve` serving the
//! previous bundle, so the page keeps painting a build you already replaced, and
//! reasoning about what you are looking at silently becomes fiction.
//!
//! One line in the telemetry panel closes it. See [`crate::telemetry::BUILD`].
//!
//! # Placement
//!
//! This is deliberately in the **app**, not the kernel, even though every app in
//! the repo has the same question. A build stamp in a low layer would have to
//! declare `rerun-if-changed` on `.git/HEAD`, which would rebuild the kernel —
//! and therefore the entire workspace — after every single commit, forever, to
//! serve a diagnostic readout. Paying that at a leaf costs one crate. If a
//! second app wants a stamp, the ~40 lines below move to a shared build-script
//! helper under `tools/`; abstracting for a caller that does not exist yet would
//! be the ceremony this repo is hostile to.

use std::process::Command;

/// Run a git command from the package directory, yielding trimmed stdout on a
/// clean exit and `None` on anything else — git missing, not a repository, a
/// non-zero status, or output that is not UTF-8.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_string())
}

fn main() {
    // The commit, and whether the tree it was built from actually matched that
    // commit. A stamp that reads as an exact commit while the build contains
    // uncommitted edits is worse than no stamp: it is trusted exactly as far as
    // a true one and sends the next investigation somewhere arbitrary. So a
    // modified tree is labelled, never silently rounded to its parent commit.
    let commit = git(&["rev-parse", "--short=12", "HEAD"]);
    let dirty = git(&["status", "--porcelain"]).is_some_and(|status| !status.is_empty());
    let suffix = ["", "+dirty"][usize::from(dirty)];
    let build = commit.map_or_else(
        // Not a git checkout at all — a source tarball, a vendored build. Say so
        // plainly rather than inventing a hash.
        || "unknown".to_string(),
        |commit| format!("{commit}{suffix}"),
    );
    println!("cargo:rustc-env=BURNT_RUBBER_BUILD={build}");

    // What makes the stamp go stale, in the order it goes stale: switching or
    // making a commit (`HEAD`), staging (`index`), and editing this app.
    //
    // The honest limit: an unstaged edit *outside* this app does not by itself
    // re-run this script, so `+dirty` reflects the tree as of this crate's last
    // compile. That is the correct granularity — the stamp describes this
    // binary, and this binary is exactly what was last compiled.
    ["HEAD", "index"]
        .iter()
        .filter_map(|path| git(&["rev-parse", "--git-path", path]))
        .for_each(|path| println!("cargo:rerun-if-changed={path}"));
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
}
