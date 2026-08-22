//! **Observed writes** — what changed, recorded without being mediated.
//!
//! `ax` used to require that every change go through it, so the ledger would
//! know where work landed. That trade did not pay:
//!
//! * **Git already has the writes**, with content and ordering, in a form
//!   nothing here improves on.
//! * **The mandate cannot be complete.** An agent fixing `ax` itself cannot use
//!   `ax` to do it — that happened, on the first real slice of work through the
//!   tool — and any conclusion drawn from a ledger with holes is biased by
//!   exactly which agents complied.
//! * **It cost a round-trip per file** and a class of shell-quoting hazards, for
//!   a duplicate of data you already had.
//!
//! So writes are *observed* instead. Every so often `ax` asks git what changed
//! since it last looked and records that. Coverage is total — it sees edits made
//! by any route, by any tool, by another agent in the same checkout — and it
//! needs nothing from the agent at all.
//!
//! # What this buys that git does not
//!
//! Git records committed writes attributed to a human. It does not record which
//! *agent* and *session*, the order of changes within a session, work that was
//! overwritten before it was committed, or — the valuable one — the **causal
//! link** between the queries an agent ran and the files it then changed.
//! Reads and writes land in one ledger with one session id, so "searched X,
//! read Y, changed Z" is reconstructable. That is the question the ledger exists
//! to answer, and it is the one git cannot.
//!
//! # Why a detached child, and not a thread
//!
//! `git status --porcelain` costs ~190 ms on this repo — twice a whole query.
//! Paying it inline would make the tool slower than the habit it replaces, which
//! the README rightly treats as a correctness property rather than a nicety. A
//! spawned thread is no good either: a CLI exits and takes its threads with it.
//!
//! So the observation runs as a **detached child process** that outlives its
//! parent, and the parent exits immediately. The rate limit below keeps that
//! from happening on every invocation.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ledger::{self, Record};
use crate::repo::Repo;

/// The hidden subcommand the detached child runs.
pub const OBSERVE_CMD: &str = "__observe";

/// How long to wait between observations.
///
/// Short enough that a change is attributed to roughly the right moment in a
/// session; long enough that a burst of twenty queries spawns one child, not
/// twenty. Agents work in bursts, so this matters.
const INTERVAL_SECS: u64 = 3;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Snapshot {
    /// When this was taken, epoch seconds.
    at: u64,
    /// `git status --porcelain` lines: the dirty set, path -> status code.
    dirty: Vec<(String, String)>,
}

fn snapshot_path(repo: &Repo) -> PathBuf {
    repo.root.join(".axiom-atlas").join("worktree.json")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Spawns the observer if the interval has elapsed, and returns immediately.
///
/// Never blocks and never fails the command that was actually asked for: an
/// observation that does not happen is a gap in the data, which is a smaller
/// problem than a tool that is slow or that errors on a query.
pub fn maybe_spawn(repo: &Repo) {
    let due = fs::read_to_string(snapshot_path(repo))
        .ok()
        .and_then(|t| serde_json::from_str::<Snapshot>(&t).ok())
        .is_none_or(|s| now().saturating_sub(s.at) >= INTERVAL_SECS);
    if !due {
        return;
    }
    // Touch the snapshot's timestamp BEFORE spawning, so a burst of invocations
    // in the same second spawns one child rather than one each.
    let _ = fs::create_dir_all(snapshot_path(repo).parent().unwrap_or(&repo.root));
    let existing = read_snapshot(repo);
    write_snapshot(
        repo,
        &Snapshot {
            at: now(),
            dirty: existing.dirty,
        },
    );

    let exe = std::env::current_exe();
    let _ = exe.map(|exe| {
        Command::new(exe)
            .arg(OBSERVE_CMD)
            .current_dir(&repo.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    });
}

fn read_snapshot(repo: &Repo) -> Snapshot {
    fs::read_to_string(snapshot_path(repo))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_snapshot(repo: &Repo, snap: &Snapshot) {
    let _ = serde_json::to_string(snap).map(|t| fs::write(snapshot_path(repo), t));
}

/// Runs one observation: diff the worktree against the last snapshot and append
/// a `change` record per path whose state moved.
///
/// This is what the detached child executes. It records the *transition*, not
/// the current state — a file that is dirty across ten observations is one
/// change, not ten.
pub fn observe(repo: &Repo) {
    let Some(dirty) = git_status(repo) else {
        return;
    };
    let previous = read_snapshot(repo);
    // **The first observation establishes a baseline and records nothing.**
    //
    // A checkout is usually already dirty when an agent starts. Treating that
    // existing state as "changes" would attribute another session's work — or
    // the human's — to this one, and the whole value of the record is that it
    // says who did what. An empty snapshot means "we have not looked before",
    // not "the tree was clean".
    let first_look = previous.dirty.is_empty() && previous.at == 0;
    let before: Vec<&(String, String)> = previous.dirty.iter().collect();

    // Anything whose status code is new or different since last time.
    let moved: Vec<&(String, String)> = dirty
        .iter()
        .filter(|(path, code)| {
            !before
                .iter()
                .any(|(p, c)| p == path && c == code)
        })
        .collect();

    let record = !first_look;
    moved.iter().filter(|_| record).for_each(|(path, code)| {
        let mut rec = Record::new("change");
        rec.query = Some(code.clone());
        rec.top_paths = vec![path.clone()];
        rec.files_matched = 1;
        rec.hits = 1;
        ledger::append(repo, &rec);
    });

    write_snapshot(repo, &Snapshot { at: now(), dirty });
}

/// `git status --porcelain`, as `(path, status code)` pairs.
///
/// `None` when git is unavailable or the command fails — the observer is an
/// observer, and a checkout it cannot read is simply not observed.
fn git_status(repo: &Repo) -> Option<Vec<(String, String)>> {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(&repo.root)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    Some(
        text.lines()
            .filter_map(|line| {
                // `XY path`, where XY is two status characters. A rename is
                // `R  old -> new`; the new name is what later work will touch.
                let (code, rest) = line.split_at_checked(2)?;
                let path = rest.trim();
                let path = path.rsplit(" -> ").next().unwrap_or(path);
                Some((path.trim().to_owned(), code.trim().to_owned()))
            })
            .collect(),
    )
}
