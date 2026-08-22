//! The append-only ledger.
//!
//! Every `ax` invocation appends exactly one NDJSON line under
//! `.axiom-atlas/ledger/raw/<YYYY-MM-DD>.ndjson`. A single short `write_all` to
//! a handle opened in append mode is atomic enough that many agents can search
//! concurrently without locking or losing records — which is why this is NDJSON
//! and not a database file holding an exclusive write lock.
//!
//! Closed days are rolled into Parquet by `ax compact`, and both raw and
//! compacted forms are readable by DuckDB in one query. See `ax sql`.
//!
//! A ledger failure must never fail the command the agent actually asked for.
//! [`append`] swallows its errors unless `AXIOM_ATLAS_DEBUG` is set.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::repo::Repo;

/// What was searched over, when the agent narrowed it.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Scope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

/// One ledger row: a single `ax` invocation.
#[derive(Debug, Serialize)]
pub struct Record {
    pub ts: String,
    pub day: String,
    pub session: String,
    pub agent: String,
    pub cmd: String,
    /// Stable identity for a friction row, so `ax resolve` can name it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub scope: Scope,
    pub hits: usize,
    pub files_matched: usize,
    /// The signal that answers "what is this repo missing?" — a search that
    /// found nothing is a question the repo could not answer.
    pub zero_result: bool,
    pub top_paths: Vec<String>,
    pub bytes_changed: i64,
    pub duration_us: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The read-back shape. Deliberately lenient so an older row still parses.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ReadRecord {
    pub ts: String,
    pub day: String,
    pub session: String,
    pub agent: String,
    pub cmd: String,
    pub id: Option<String>,
    pub query: Option<String>,
    pub scope: Scope,
    pub hits: usize,
    pub files_matched: usize,
    pub zero_result: bool,
    pub top_paths: Vec<String>,
    pub bytes_changed: i64,
    pub duration_us: u64,
    pub ok: bool,
    pub note: Option<String>,
}

impl Record {
    /// Starts a record for `cmd`, stamping the wall clock once.
    pub fn new(cmd: &str) -> Self {
        let (ts, day) = now_rfc3339_and_day();
        Self {
            ts,
            day,
            session: std::env::var("AXIOM_ATLAS_SESSION")
                .unwrap_or_else(|_| std::process::id().to_string()),
            agent: std::env::var("AXIOM_ATLAS_AGENT").unwrap_or_else(|_| "unknown".to_owned()),
            cmd: cmd.to_owned(),
            id: None,
            query: None,
            scope: Scope::default(),
            hits: 0,
            files_matched: 0,
            zero_result: false,
            top_paths: Vec::new(),
            bytes_changed: 0,
            duration_us: 0,
            ok: true,
            note: None,
        }
    }
}

pub fn raw_dir(repo: &Repo) -> PathBuf {
    repo.root.join(".axiom-atlas").join("ledger").join("raw")
}

pub fn parquet_dir(repo: &Repo) -> PathBuf {
    repo.root.join(".axiom-atlas").join("ledger").join("parquet")
}

/// Appends one record. Never fails the caller's command.
pub fn append(repo: &Repo, rec: &Record) {
    if std::env::var_os("AXIOM_ATLAS_NO_LEDGER").is_some() {
        return;
    }
    let result = (|| -> std::io::Result<()> {
        let dir = raw_dir(repo);
        fs::create_dir_all(&dir)?;
        let mut line = serde_json::to_string(rec)?;
        line.push('\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{}.ndjson", rec.day)))?;
        f.write_all(line.as_bytes())
    })();

    if let Err(err) = result {
        if std::env::var_os("AXIOM_ATLAS_DEBUG").is_some() {
            eprintln!("ax: ledger append failed: {err}");
        }
    }
}

/// Reads every raw NDJSON row. Malformed lines are skipped, not fatal.
pub fn read_all(repo: &Repo) -> Vec<ReadRecord> {
    let dir = raw_dir(repo);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "ndjson"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    files
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .flat_map(|text| {
            text.lines()
                .filter_map(|l| serde_json::from_str::<ReadRecord>(l).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Counts occurrences, returned highest-first with a stable tiebreak.
pub fn rank<K: Clone + Ord + std::hash::Hash>(items: impl Iterator<Item = K>) -> Vec<(K, usize)> {
    let mut counts: HashMap<K, usize> = HashMap::new();
    for k in items {
        *counts.entry(k).or_insert(0) += 1;
    }
    let mut out: Vec<(K, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// A stable, dependency-free id for a friction description.
///
/// FNV-1a over the whitespace-normalised text. Deterministic across runs and
/// machines, unlike `DefaultHasher`, whose output carries no stability
/// guarantee. The same complaint therefore always yields the same id, which is
/// what lets `ax resolve` name one and what makes a repeated complaint count
/// rather than duplicate.
pub fn friction_id(text: &str) -> String {
    let normalised = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in normalised.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // FNV alone leaves the high hex digits barely touched by the final bytes,
    // so two near-identical complaints would share a displayed prefix. A fmix64
    // finalizer diffuses every input byte across all 64 bits, which is what
    // makes a 7-character prefix safe to type at `ax resolve`.
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    hash ^= hash >> 33;

    format!("{hash:016x}")
}

// ---------------------------------------------------------------------------
// Wall clock, without a date dependency.
// ---------------------------------------------------------------------------

/// Returns `(RFC3339 UTC timestamp, YYYY-MM-DD)`.
fn now_rfc3339_and_day() -> (String, String) {
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();

    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);

    (
        format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z"),
        format!("{y:04}-{m:02}-{d:02}"),
    )
}

/// Howard Hinnant's days-from-civil inverse: converts a day count since the
/// Unix epoch into a civil `(year, month, day)`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (y + i64::from(m <= 2), m, d)
}
