//! The four load scenarios, each driving the real server over real sockets.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};

use crate::args::{Config, Mode};
use crate::http;
use crate::player::{run_player, PlayerReport};
use crate::stats::{self, Aggregate};

/// Dispatch to the configured scenario; returns whether it passed.
pub async fn run(cfg: &Config) -> bool {
    match cfg.mode {
        Mode::Soak => soak(cfg).await,
        Mode::Matchmake => matchmake(cfg).await,
        Mode::Scaleout => scaleout(cfg).await,
        Mode::Resilience => resilience(cfg).await,
    }
}

async fn soak(cfg: &Config) -> bool {
    println!(
        "== soak: {} players across {} room(s) on {} ==",
        cfg.players,
        cfg.rooms,
        cfg.ws_url()
    );
    let assignments: Vec<(String, String)> = (0..cfg.players)
        .map(|i| (cfg.ws_url(), format!("load-{}", i % cfg.rooms)))
        .collect();
    let reports = drive_players(assignments, cfg).await;
    report_and_verdict("soak", &stats::aggregate(&reports), cfg, Vec::new())
}

async fn matchmake(cfg: &Config) -> bool {
    println!(
        "== matchmake: {} requests at concurrency {} against {} ==",
        cfg.requests, cfg.concurrency, cfg.target
    );
    let start = Instant::now();
    let results = fire_matchmake(&cfg.target, cfg.requests, cfg.concurrency).await;
    let elapsed = start.elapsed().as_secs_f64();

    let assignments: Vec<(String, Option<String>)> = results
        .iter()
        .filter_map(|r| r.as_ref().ok().and_then(|(s, v)| assignment_of(*s, v)))
        .collect();
    let failures = results.len() - assignments.len();
    let dist = distribution(&assignments);
    let rate = match elapsed > 0.0 {
        true => cfg.requests as f64 / elapsed,
        false => 0.0,
    };

    println!("\n-- matchmake results --");
    println!("succeeded:    {}/{}", assignments.len(), cfg.requests);
    println!("throughput:   {rate:.0} req/s ({elapsed:.2}s elapsed)");
    println!("distinct rooms: {}", dist.per_room.len());
    print_node_distribution(&dist);
    print_first_errors(&results);

    let ok = failures == 0 && !assignments.is_empty();
    print_verdict(
        ok,
        &match ok {
            true => Vec::new(),
            false => vec![format!("{failures} matchmake request(s) failed")],
        },
    );
    ok
}

async fn scaleout(cfg: &Config) -> bool {
    println!(
        "== scaleout: {} players via director {} ==",
        cfg.players, cfg.target
    );
    if !wait_for_nodes(&cfg.target, cfg.min_nodes, 60.0).await {
        println!("director never reported >= {} nodes", cfg.min_nodes);
        print_verdict(
            false,
            &[format!("fewer than {} nodes registered", cfg.min_nodes)],
        );
        return false;
    }

    let results = fire_matchmake(&cfg.target, cfg.players, cfg.concurrency).await;
    let assignments: Vec<(String, Option<String>)> = results
        .iter()
        .filter_map(|r| r.as_ref().ok().and_then(|(s, v)| assignment_of(*s, v)))
        .collect();
    let dist = distribution(&assignments);

    println!("\n-- scaleout assignment --");
    println!("matched:      {}/{}", assignments.len(), cfg.players);
    print_node_distribution(&dist);

    let players: Vec<(String, String)> = assignments
        .iter()
        .filter_map(|(room, node)| node.clone().map(|n| (n, room.clone())))
        .collect();
    let reports = drive_players(players, cfg).await;
    let agg = stats::aggregate(&reports);

    let spread_ok = dist.per_node.len() >= 2;
    let extra = match spread_ok {
        true => Vec::new(),
        false => vec![format!(
            "rooms did not spread across >= 2 nodes (saw {})",
            dist.per_node.len()
        )],
    };
    report_and_verdict("scaleout", &agg, cfg, extra)
}

async fn resilience(cfg: &Config) -> bool {
    println!(
        "== resilience: {} players on {}, killing '{}' every {:.1}s ==",
        cfg.players,
        cfg.ws_url(),
        cfg.worker_image,
        cfg.kill_every_secs
    );
    println!("(requires the node running with AXIOM_WORKER_MODE=outproc)");
    let assignments: Vec<(String, String)> = (0..cfg.players)
        .map(|i| (cfg.ws_url(), format!("load-{}", i % cfg.rooms)))
        .collect();
    let dur = Duration::from_secs_f64(cfg.duration_secs);

    let (reports, kills) = tokio::join!(
        drive_players(assignments, cfg),
        chaos_loop(cfg.worker_image.clone(), cfg.kill_every_secs, dur),
    );
    println!("\nworker kills delivered: {kills}");

    // The proof requires BOTH that a crash actually happened (kills > 0 — against
    // an in-process node nothing is killed, so the scenario must not pass) AND that
    // recovery kept the authoritative loop near its nominal tick rate. Without the
    // kill gate this scenario would vacuously PASS while proving no recovery at all.
    let agg = stats::aggregate(&reports);
    let extra = resilience_extra_fails(kills, &agg, cfg.duration_secs);
    report_and_verdict("resilience", &agg, cfg, extra)
}

/// Resilience-specific failure conditions layered on the shared thresholds:
/// (1) the chaos must have actually killed a worker — otherwise nothing crashed
/// and "recovery" is meaningless; (2) recovery must have held the slowest welcomed
/// player near the nominal 60 Hz tick budget, so a long mid-run stall fails.
fn resilience_extra_fails(kills: u64, agg: &Aggregate, duration_secs: f64) -> Vec<String> {
    let nominal = (duration_secs * 60.0) as u64;
    let recovery_floor = nominal * 8 / 10;
    let mut fails = Vec::new();
    (kills == 0).then(|| {
        fails.push(
            "no worker process was killed — run the node with AXIOM_WORKER_MODE=outproc; \
             resilience proves nothing without a real crash"
                .to_string(),
        )
    });
    (agg.min_tick_advance < recovery_floor).then(|| {
        fails.push(format!(
            "recovery degraded: slowest welcomed player advanced {} ticks, below the \
             {recovery_floor} floor (80% of {nominal} nominal at 60Hz)",
            agg.min_tick_advance
        ))
    });
    fails
}

/// Spawn one task per player (with a ramp delay) and collect every report.
async fn drive_players(assignments: Vec<(String, String)>, cfg: &Config) -> Vec<PlayerReport> {
    let dur = Duration::from_secs_f64(cfg.duration_secs);
    let hz = cfg.intent_hz;
    let ramp = cfg.ramp_ms;
    let mut set: JoinSet<PlayerReport> = JoinSet::new();
    for (idx, (ws, room)) in assignments.into_iter().enumerate() {
        let delay = Duration::from_millis(ramp.saturating_mul(idx as u64));
        set.spawn(async move {
            tokio::time::sleep(delay).await;
            run_player(ws, room, dur, hz).await
        });
    }
    let mut reports = Vec::new();
    while let Some(joined) = set.join_next().await {
        reports.push(joined.unwrap_or_else(|e| PlayerReport {
            connected: false,
            welcomed: false,
            intents_sent: 0,
            snapshots: 0,
            rejects: 0,
            first_tick_seen: 0,
            max_server_tick: 0,
            latencies_ms: Vec::new(),
            error: Some(format!("player task failed: {e}")),
        }));
    }
    reports
}

/// Print the aggregate and apply the threshold verdict, plus any scenario-specific
/// `extra_fails` (a non-empty list forces FAIL).
fn report_and_verdict(
    label: &str,
    agg: &Aggregate,
    cfg: &Config,
    extra_fails: Vec<String>,
) -> bool {
    let p = agg.latency;
    println!("\n-- {label} results --");
    println!(
        "players:        {}/{} connected",
        agg.connected, agg.attempted
    );
    println!(
        "welcomed:       {}/{} admitted to a room",
        agg.welcomed, agg.attempted
    );
    println!("intents sent:   {}", agg.intents);
    println!("snapshots recv: {}", agg.snapshots);
    println!("accept rate:    {:.3}", agg.accept_rate());
    println!(
        "server tick:    max {}, median advance {} ({:.1} ticks/s sustained)",
        agg.max_server_tick,
        agg.median_tick_advance,
        agg.tick_rate(cfg.duration_secs)
    );
    println!(
        "intent->ack ms: p50 {:.1}  p95 {:.1}  p99 {:.1}  max {:.1}  (n={})",
        p.p50, p.p95, p.p99, p.max, p.count
    );
    print_first_player_errors(agg);
    let (threshold_ok, mut fails) = stats::verdict(agg, cfg);
    fails.extend(extra_fails.iter().cloned());
    let ok = threshold_ok && extra_fails.is_empty();
    print_verdict(ok, &fails);
    ok
}

fn print_verdict(ok: bool, fails: &[String]) {
    match ok {
        true => println!("VERDICT: PASS"),
        false => {
            println!("VERDICT: FAIL");
            fails.iter().for_each(|f| println!("  - {f}"));
        }
    }
}

fn print_first_player_errors(agg: &Aggregate) {
    agg.errors
        .iter()
        .take(3)
        .for_each(|e| println!("  player error: {e}"));
}

fn print_first_errors(results: &[Result<(u16, Value), String>]) {
    results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .take(3)
        .for_each(|e| println!("  request error: {e}"));
}

fn print_node_distribution(dist: &Distribution) {
    match dist.per_node.is_empty() {
        true => println!("nodes:        (same-origin; no node URLs returned)"),
        false => {
            println!("nodes:        {} distinct", dist.per_node.len());
            dist.per_node
                .iter()
                .for_each(|(node, n)| println!("  {node} -> {n} player(s)"));
        }
    }
}

/// Fire `n` `POST /matchmake` calls, bounded to `concurrency` in flight.
async fn fire_matchmake(
    target: &str,
    n: usize,
    concurrency: usize,
) -> Vec<Result<(u16, Value), String>> {
    let sem = Arc::new(Semaphore::new(concurrency));
    let mut set: JoinSet<Result<(u16, Value), String>> = JoinSet::new();
    for _ in 0..n {
        let sem = sem.clone();
        let target = target.to_string();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            http::post_json(&target, "/matchmake").await
        });
    }
    let mut out = Vec::new();
    while let Some(joined) = set.join_next().await {
        out.push(joined.unwrap_or_else(|e| Err(format!("matchmake task failed: {e}"))));
    }
    out
}

/// Poll the director's `/readyz` until at least `want` nodes have registered.
async fn wait_for_nodes(target: &str, want: usize, timeout_secs: f64) -> bool {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout_secs);
    loop {
        if let Ok((_status, v)) = http::get_json(target, "/readyz").await {
            let nodes = v.get("nodes").and_then(Value::as_u64).unwrap_or(0);
            if nodes >= want as u64 {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// A matchmake result parsed into `(roomId, optional nodeUrl)`, or `None` if the
/// response was not a successful assignment.
fn assignment_of(status: u16, v: &Value) -> Option<(String, Option<String>)> {
    if status != 200 {
        return None;
    }
    let room = v.get("roomId").and_then(Value::as_str)?;
    let node = v.get("nodeUrl").and_then(Value::as_str).map(str::to_string);
    Some((room.to_string(), node))
}

/// Counts of assignments per node and per room.
struct Distribution {
    per_node: BTreeMap<String, usize>,
    per_room: BTreeMap<String, usize>,
}

fn distribution(assignments: &[(String, Option<String>)]) -> Distribution {
    let mut per_node = BTreeMap::new();
    let mut per_room = BTreeMap::new();
    for (room, node) in assignments {
        *per_room.entry(room.clone()).or_insert(0) += 1;
        if let Some(n) = node {
            *per_node.entry(n.clone()).or_insert(0) += 1;
        }
    }
    Distribution { per_node, per_room }
}

/// Kill the worker process(es) every `every_secs` until `total` elapses; returns
/// how many kills landed. Killing by image name fells every live worker, so each
/// room's worker must respawn and restore — a strong crash-recovery stress.
async fn chaos_loop(image: String, every_secs: f64, total: Duration) -> u64 {
    let deadline = Instant::now() + total;
    let mut kills = 0;
    loop {
        tokio::time::sleep(Duration::from_secs_f64(every_secs)).await;
        if Instant::now() >= deadline {
            return kills;
        }
        if kill_worker(&image).await {
            kills += 1;
        }
    }
}

/// Kill all processes matching `image` (OS-native; best-effort).
async fn kill_worker(image: &str) -> bool {
    let image = image.to_string();
    tokio::task::spawn_blocking(move || {
        let output = if cfg!(windows) {
            std::process::Command::new("taskkill")
                .args(["/F", "/IM", &format!("{image}.exe")])
                .output()
        } else {
            std::process::Command::new("pkill")
                .args(["-f", &image])
                .output()
        };
        output.map(|o| o.status.success()).unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Percentiles;
    use serde_json::json;

    fn agg(min_advance: u64) -> Aggregate {
        Aggregate {
            attempted: 4,
            connected: 4,
            welcomed: 4,
            intents: 100,
            snapshots: 100,
            rejects: 0,
            latency: Percentiles {
                p50: 16.0,
                p95: 30.0,
                p99: 31.0,
                max: 40.0,
                count: 100,
            },
            min_tick_advance: min_advance,
            median_tick_advance: min_advance,
            max_server_tick: 600,
            errors: Vec::new(),
        }
    }

    #[test]
    fn resilience_fails_when_no_worker_was_killed() {
        // Against an in-process node, kill_worker matches nothing → kills == 0.
        // The scenario must FAIL rather than vacuously pass.
        let fails = resilience_extra_fails(0, &agg(600), 10.0);
        assert!(
            fails
                .iter()
                .any(|f| f.contains("no worker process was killed")),
            "{fails:?}"
        );
    }

    #[test]
    fn resilience_fails_when_recovery_is_degraded() {
        // Kills landed, but the loop barely advanced (a long stall) — floor for a
        // 10s run is 480 ticks; 100 is well below.
        let fails = resilience_extra_fails(3, &agg(100), 10.0);
        assert!(
            fails.iter().any(|f| f.contains("recovery degraded")),
            "{fails:?}"
        );
    }

    #[test]
    fn resilience_passes_when_killed_and_recovered() {
        // Kills landed and the loop stayed near nominal (600 >= 480 floor).
        let fails = resilience_extra_fails(3, &agg(600), 10.0);
        assert!(fails.is_empty(), "{fails:?}");
    }

    #[test]
    fn assignment_parses_a_director_response() {
        let v = json!({"roomId": "mm-1", "nodeUrl": "ws://localhost:8101/ws"});
        assert_eq!(
            assignment_of(200, &v),
            Some((
                "mm-1".to_string(),
                Some("ws://localhost:8101/ws".to_string())
            ))
        );
    }

    #[test]
    fn assignment_parses_an_allinone_response() {
        let v = json!({"roomId": "mm-2"});
        assert_eq!(assignment_of(200, &v), Some(("mm-2".to_string(), None)));
    }

    #[test]
    fn assignment_rejects_non_200_and_missing_room() {
        assert_eq!(assignment_of(503, &json!({"error": "no nodes"})), None);
        assert_eq!(assignment_of(200, &json!({"nope": 1})), None);
    }

    #[test]
    fn distribution_counts_rooms_and_nodes() {
        let assigns = vec![
            ("mm-1".to_string(), Some("ws://a/ws".to_string())),
            ("mm-1".to_string(), Some("ws://a/ws".to_string())),
            ("mm-2".to_string(), Some("ws://b/ws".to_string())),
            ("mm-3".to_string(), None),
        ];
        let d = distribution(&assigns);
        assert_eq!(d.per_node.len(), 2);
        assert_eq!(d.per_node["ws://a/ws"], 2);
        assert_eq!(d.per_node["ws://b/ws"], 1);
        assert_eq!(d.per_room.len(), 3);
        assert_eq!(d.per_room["mm-1"], 2);
    }

    #[test]
    fn distribution_is_empty_for_same_origin() {
        let assigns = vec![("mm-1".to_string(), None)];
        let d = distribution(&assigns);
        assert!(d.per_node.is_empty());
        assert_eq!(d.per_room.len(), 1);
    }
}
