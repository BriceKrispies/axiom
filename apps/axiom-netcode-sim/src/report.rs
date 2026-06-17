//! The per-client observations a run produces, plus console + CSV rendering.

use std::fs;
use std::io;
use std::path::Path;

use crate::config::Behavior;

/// A summary of one peer's input→confirm latency, in driver ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyStats {
    /// Fastest confirm.
    pub min: u64,
    /// Median confirm.
    pub median: u64,
    /// 95th-percentile confirm.
    pub p95: u64,
    /// Slowest confirm.
    pub max: u64,
    /// How many ticks contributed a sample.
    pub samples: usize,
}

impl LatencyStats {
    /// Summarize a set of per-tick latencies (`None` if the peer confirmed
    /// nothing).
    pub(crate) fn from_samples(mut v: Vec<u64>) -> Option<Self> {
        (!v.is_empty()).then(|| {
            v.sort_unstable();
            let pick = |num: usize, den: usize| v[((v.len() - 1) * num) / den];
            LatencyStats {
                min: v[0],
                median: pick(1, 2),
                p95: pick(95, 100),
                max: v[v.len() - 1],
                samples: v.len(),
            }
        })
    }
}

/// How one peer fared over the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerReport {
    /// The peer's raw id (1-based).
    pub id: u64,
    /// Whether it played fair, and if not, how it cheated.
    pub behavior: Behavior,
    /// The last tick it confirmed.
    pub final_confirmed: u64,
    /// The largest its input buffer ever grew (bounded even under flood).
    pub buffer_peak: usize,
    /// Frames it dropped: `(unknown_peer, bad_signature, out_of_window)`.
    pub rejections: (u64, u64, u64),
    /// Structurally-malformed frames whose `ingest` failed to decode.
    pub ingest_errors: u64,
    /// Its input→confirm latency distribution.
    pub latency: Option<LatencyStats>,
    /// Confirmed ticks at which it detected a desync via `reconcile`.
    pub desync_ticks: Vec<u64>,
    /// A fingerprint of its entire confirmed-state history (for the replay check).
    pub state_digest: [u8; 32],
}

/// The whole run's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimReport {
    /// Per-peer outcomes, in peer order.
    pub peers: Vec<PeerReport>,
    /// The lowest final confirmed tick across peers (liveness floor).
    pub min_confirmed: u64,
    /// The highest final confirmed tick across peers.
    pub max_confirmed: u64,
    /// Whether every peer agreed at every commonly-confirmed tick.
    pub all_agree: bool,
    /// The first sim tick at which two peers' state hashes disagreed, if any.
    pub first_divergence: Option<u64>,
}

impl SimReport {
    /// Print a human-readable summary table to stdout.
    pub fn print_summary(&self) {
        println!("\n=== simulation summary ===");
        println!(
            "peers={}  confirmed[min={}, max={}]  converged={}{}",
            self.peers.len(),
            self.min_confirmed,
            self.max_confirmed,
            self.all_agree.then_some("YES").unwrap_or("NO"),
            self.first_divergence
                .map_or(String::new(), |t| format!(" (first divergence at tick {t})"))
        );
        println!(
            "{:>4}  {:<16} {:>9} {:>6} {:>20} {:>20} {:>6}",
            "peer",
            "behavior",
            "confirmed",
            "bufpk",
            "latency(mn/md/p95/mx)",
            "drops(unk/sig/win/mal)",
            "desync"
        );
        self.peers.iter().for_each(|p| {
            let lat = p.latency.map_or_else(
                || "-".to_string(),
                |l| format!("{}/{}/{}/{}", l.min, l.median, l.p95, l.max),
            );
            let (u, s, w) = p.rejections;
            println!(
                "{:>4}  {:<16} {:>9} {:>6} {:>20} {:>20} {:>6}",
                p.id,
                format!("{:?}", p.behavior),
                p.final_confirmed,
                p.buffer_peak,
                lat,
                format!("{u}/{s}/{w}/{}", p.ingest_errors),
                p.desync_ticks.len(),
            );
        });
    }
}

/// One CSV row per `(tick, peer)`: how that client looked at that tick.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CsvRow {
    pub tick: u64,
    pub peer: u64,
    pub confirmed: u64,
    pub buffered: usize,
    pub hash_prefix: u32,
    pub drop_unknown: u64,
    pub drop_bad_sig: u64,
    pub drop_window: u64,
}

/// Write the per-tick CSV (one row per peer per tick) to `path`.
pub(crate) fn write_csv(path: &Path, rows: &[CsvRow]) -> io::Result<()> {
    let mut out = String::from(
        "tick,peer,confirmed,buffered,hash_prefix,drop_unknown,drop_bad_sig,drop_window\n",
    );
    rows.iter().for_each(|r| {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            r.tick,
            r.peer,
            r.confirmed,
            r.buffered,
            r.hash_prefix,
            r.drop_unknown,
            r.drop_bad_sig,
            r.drop_window,
        ));
    });
    fs::write(path, out)
}
