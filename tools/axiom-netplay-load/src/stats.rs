//! Aggregation of per-player results into a run summary, and the pass/fail
//! verdict that drives the process exit code.

use crate::args::Config;
use crate::player::PlayerReport;

/// Latency percentiles (milliseconds) over a sample set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub count: usize,
}

/// Nearest-rank percentiles over `samples` (consumed and sorted in place).
pub fn percentiles(mut samples: Vec<f64>) -> Percentiles {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pick = |q: f64| match samples.len() {
        0 => 0.0,
        n => samples[(((n - 1) as f64) * q).round() as usize],
    };
    Percentiles {
        p50: pick(0.50),
        p95: pick(0.95),
        p99: pick(0.99),
        max: samples.last().copied().unwrap_or(0.0),
        count: samples.len(),
    }
}

/// The aggregate of a soak/scaleout/resilience run.
#[derive(Debug, Clone)]
pub struct Aggregate {
    pub attempted: usize,
    pub connected: usize,
    pub welcomed: usize,
    pub intents: u64,
    pub snapshots: u64,
    pub rejects: u64,
    pub latency: Percentiles,
    pub min_tick_advance: u64,
    pub median_tick_advance: u64,
    pub max_server_tick: u64,
    pub errors: Vec<String>,
}

impl Aggregate {
    /// Fraction of sent intents the server accepted (no `RejectedIntent`).
    pub fn accept_rate(&self) -> f64 {
        match self.intents {
            0 => 1.0,
            n => 1.0 - (self.rejects as f64 / n as f64),
        }
    }

    /// Effective sustained server tick rate (ticks/sec) over the run.
    pub fn tick_rate(&self, duration_secs: f64) -> f64 {
        match duration_secs > 0.0 {
            true => self.median_tick_advance as f64 / duration_secs,
            false => 0.0,
        }
    }
}

/// Fold per-player reports into one aggregate.
pub fn aggregate(reports: &[PlayerReport]) -> Aggregate {
    // Tick-advance and latency are only meaningful for welcomed players (a
    // connected-but-unwelcomed player never enters a room).
    let welcomed: Vec<&PlayerReport> = reports.iter().filter(|r| r.welcomed).collect();
    let mut latencies: Vec<f64> = reports
        .iter()
        .flat_map(|r| r.latencies_ms.clone())
        .collect();
    latencies.shrink_to_fit();

    let mut advances: Vec<u64> = welcomed.iter().map(|r| r.tick_advance()).collect();
    advances.sort_unstable();
    let median_tick_advance = advances.get(advances.len() / 2).copied().unwrap_or(0);
    let min_tick_advance = advances.first().copied().unwrap_or(0);

    Aggregate {
        attempted: reports.len(),
        connected: reports.iter().filter(|r| r.connected).count(),
        welcomed: welcomed.len(),
        intents: reports.iter().map(|r| r.intents_sent).sum(),
        snapshots: reports.iter().map(|r| r.snapshots).sum(),
        rejects: reports.iter().map(|r| r.rejects).sum(),
        latency: percentiles(latencies),
        min_tick_advance,
        median_tick_advance,
        max_server_tick: reports.iter().map(|r| r.max_server_tick).max().unwrap_or(0),
        errors: reports.iter().filter_map(|r| r.error.clone()).collect(),
    }
}

/// The pass/fail verdict for a soak/scaleout/resilience aggregate, given the
/// configured thresholds. Returns `(passed, reasons_for_failure)`.
pub fn verdict(agg: &Aggregate, cfg: &Config) -> (bool, Vec<String>) {
    let mut fails = Vec::new();
    (agg.connected != agg.attempted).then(|| {
        fails.push(format!(
            "only {}/{} players connected",
            agg.connected, agg.attempted
        ))
    });
    (agg.welcomed != agg.attempted).then(|| {
        fails.push(format!(
            "only {}/{} players were welcomed into a room (over-subscription: raise --rooms or lower --players)",
            agg.welcomed, agg.attempted
        ))
    });
    // Intents must actually round-trip: a server that welcomes players and ticks
    // its loop but never *accepts* an intent (snapshots forever ack 0) produces
    // zero latency samples. Require at least one ack whenever any player was
    // welcomed, so that broken intent path is a FAIL, not a silent pass.
    ((agg.welcomed != 0) & (agg.latency.count == 0)).then(|| {
        fails.push(
            "players were welcomed but no intent was ever acknowledged (intent path broken)"
                .to_string(),
        )
    });
    ((agg.latency.p99 > cfg.max_p99_ms) & (agg.latency.count != 0)).then(|| {
        fails.push(format!(
            "p99 latency {:.1}ms exceeds budget {:.1}ms",
            agg.latency.p99, cfg.max_p99_ms
        ))
    });
    (agg.accept_rate() < cfg.min_accept_rate).then(|| {
        fails.push(format!(
            "accept rate {:.3} below floor {:.3}",
            agg.accept_rate(),
            cfg.min_accept_rate
        ))
    });
    (agg.min_tick_advance < cfg.min_tick_advance).then(|| {
        fails.push(format!(
            "slowest player advanced only {} ticks (need {})",
            agg.min_tick_advance, cfg.min_tick_advance
        ))
    });
    (fails.is_empty(), fails)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(connected: bool, intents: u64, rejects: u64, first: u64, last: u64) -> PlayerReport {
        PlayerReport {
            connected,
            welcomed: connected,
            intents_sent: intents,
            snapshots: last.saturating_sub(first),
            rejects,
            first_tick_seen: first,
            max_server_tick: last,
            latencies_ms: vec![10.0, 20.0, 30.0],
            error: None,
        }
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        // Nearest-rank over index (n-1)*q: for 1..=100, p50 -> idx 50 -> 51, etc.
        let p = percentiles((1..=100).map(|n| n as f64).collect());
        assert_eq!(p.count, 100);
        assert_eq!(p.p50, 51.0);
        assert_eq!(p.p95, 95.0);
        assert_eq!(p.p99, 99.0);
        assert_eq!(p.max, 100.0);
    }

    #[test]
    fn empty_percentiles_are_zero() {
        let p = percentiles(Vec::new());
        assert_eq!(p.p50, 0.0);
        assert_eq!(p.count, 0);
    }

    #[test]
    fn aggregate_folds_players() {
        let reports = vec![report(true, 100, 0, 5, 65), report(true, 100, 10, 5, 60)];
        let agg = aggregate(&reports);
        assert_eq!(agg.attempted, 2);
        assert_eq!(agg.connected, 2);
        assert_eq!(agg.intents, 200);
        assert_eq!(agg.rejects, 10);
        assert_eq!(agg.min_tick_advance, 55); // min(60, 55)
        assert_eq!(agg.max_server_tick, 65);
        assert!((agg.accept_rate() - 0.95).abs() < 1e-9);
        assert!(agg.tick_rate(11.0) > 0.0);
    }

    #[test]
    fn aggregate_ignores_disconnected_for_advance() {
        let reports = vec![report(true, 50, 0, 0, 60), report(false, 0, 0, 0, 0)];
        let agg = aggregate(&reports);
        assert_eq!(agg.connected, 1);
        assert_eq!(agg.min_tick_advance, 60);
    }

    #[test]
    fn verdict_fails_when_welcomed_but_no_intent_acked() {
        // A server that welcomes players and advances ticks but never acks an
        // intent yields zero latency samples — the verdict must catch the broken
        // intent path, not pass on it.
        let cfg = Config::parse(&["soak".to_string()]).unwrap();
        let mut r = report(true, 600, 0, 0, 600);
        r.latencies_ms.clear(); // no acks ever recorded
        let agg = aggregate(&[r]);
        assert_eq!(agg.latency.count, 0);
        let (ok, fails) = verdict(&agg, &cfg);
        assert!(!ok);
        assert!(
            fails
                .iter()
                .any(|f| f.contains("no intent was ever acknowledged")),
            "{fails:?}"
        );
    }

    #[test]
    fn verdict_passes_a_healthy_run() {
        let cfg = Config::parse(&["soak".to_string()]).unwrap();
        let agg = aggregate(&[report(true, 600, 0, 0, 600), report(true, 600, 0, 0, 600)]);
        let (ok, fails) = verdict(&agg, &cfg);
        assert!(ok, "expected pass, got {fails:?}");
    }

    #[test]
    fn verdict_flags_each_failure() {
        let cfg = Config::parse(&[
            "soak".to_string(),
            "--max-p99-ms".to_string(),
            "5".to_string(),
            "--min-accept-rate".to_string(),
            "0.99".to_string(),
            "--min-tick-advance".to_string(),
            "100".to_string(),
        ])
        .unwrap();
        // 1 of 2 connected, p99 30ms > 5ms, accept 0.9 < 0.99, advance 10 < 100.
        let reports = vec![report(true, 100, 10, 0, 10), report(false, 0, 0, 0, 0)];
        let agg = aggregate(&reports);
        let (ok, fails) = verdict(&agg, &cfg);
        assert!(!ok);
        // connected 1/2, welcomed 1/2, p99, accept rate, tick advance.
        assert_eq!(fails.len(), 5, "every threshold should fail: {fails:?}");
    }
}
