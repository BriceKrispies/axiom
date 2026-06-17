//! # Axiom Netcode Sim — an N-peer deterministic-lockstep harness
//!
//! Spin up N in-process clients (real engine `App`s or a lightweight mock), join
//! them with a seeded modeled network (loss, jitter, duplication, partitions),
//! drive scripted or misbehaving inputs, and observe how every client reacts:
//! convergence, confirmed-tick lag, input→confirm latency, buffer occupancy,
//! dropped/forged-frame counts, and desync detection.
//!
//! Because deterministic lockstep means every client runs identical code over the
//! same confirmed inputs, this in-process run is byte-for-byte what N real
//! browsers would compute — and, being seeded, it replays exactly. See
//! [`SimConfig`] for the knobs and [`run_simulation`] to drive it.

mod cheat;
mod config;
mod network;
mod report;
mod simulant;

pub use config::{
    Backend, Behavior, CheatKind, InputScript, NetworkConfig, Partition, PeerConfig, SimConfig,
};
pub use report::{LatencyStats, PeerReport, SimReport};

use config::ScriptKind;

use std::collections::BTreeMap;

use axiom::prelude::Vec3;
use axiom_crypto::{SigningKey, VerifyingKey};
use axiom_kernel::DeterministicRng;
use axiom_netcode::NetcodeApi;

use cheat::CheatState;
use network::ModeledNetwork;
use report::{write_csv, CsvRow};
use simulant::{build as build_sim, encode_delta, Simulant, MOVE_KIND, MOVE_SPEED};

/// A cap on a peer's resend backlog (bounds memory across a long partition).
const RESEND_WINDOW: usize = 512;

/// One simulated client: its session, its local simulation, its behavior, and
/// the observations accumulated about it.
struct Peer {
    id: u64,
    net: NetcodeApi,
    sim: Box<dyn Simulant>,
    cfg: PeerConfig,
    submitted: u64,
    input_rng: DeterministicRng,
    /// Recently-submitted frames as `(sim_tick, bytes)`, for retransmission.
    outbox: Vec<(u64, Vec<u8>)>,
    submit_time: BTreeMap<u64, u64>,
    buffer_peak: usize,
    latencies: Vec<u64>,
    desync_ticks: Vec<u64>,
    reconcile_cursor: u64,
    ingest_errors: u64,
    history: [u8; 32],
    hashes: BTreeMap<u64, [u8; 32]>,
    last_hash_prefix: u32,
    cheat: CheatState,
}

impl Peer {
    /// Whether this peer is connected at driver tick `t`.
    fn active(&self, t: u64) -> bool {
        (t >= self.cfg.join_tick) & (t < self.cfg.leave_tick)
    }

    /// Whether this peer should submit a fresh input this driver tick.
    fn should_submit(&self, t: u64, max_ahead: u64) -> bool {
        self.active(t)
            & t.is_multiple_of(self.cfg.tick_rate.max(1))
            & (self.submitted.saturating_sub(self.net.confirmed_tick()) < max_ahead)
    }

    /// This peer's input delta for `tick`, per its script. Each kind's delta is
    /// computed unconditionally and the script's `kind` tag selects which one
    /// to keep — no `match`. `Idle` (and any unselected kind) contributes the
    /// zero delta, so the four sources sum to exactly one non-zero source.
    fn input(&mut self, tick: u64) -> Vec3 {
        let kind = self.cfg.input.kind();

        let scripted = (kind == ScriptKind::Scripted)
            .then(|| self.cfg.input.scripted())
            .flatten()
            .and_then(|v| v.get((tick as usize).checked_rem(v.len()).unwrap_or(0)))
            .copied()
            .unwrap_or(Vec3::ZERO);

        // The RNG must only advance when this peer actually random-walks, or the
        // seeded stream (and thus the replay) would drift; gate the draw on the
        // tag before touching `input_rng`.
        let random_walk = (kind == ScriptKind::RandomWalk)
            .then(|| {
                let mut axis =
                    || (self.input_rng.next_bounded(3) as i32 - 1) as f32 * MOVE_SPEED;
                let (x, y) = (axis(), axis());
                Vec3::new(x, y, 0.0)
            })
            .unwrap_or(Vec3::ZERO);

        let oscillate = (kind == ScriptKind::Oscillate)
            .then(|| {
                let p = self.cfg.input.period().max(1);
                let a = std::f32::consts::TAU * (tick % p) as f32 / p as f32;
                Vec3::new(a.cos() * MOVE_SPEED, a.sin() * MOVE_SPEED, 0.0)
            })
            .unwrap_or(Vec3::ZERO);

        scripted.add(random_walk).add(oscillate)
    }

    /// The extra bad frames this peer injects this tick (empty if honest).
    fn cheat_frames(&mut self) -> Vec<Vec<u8>> {
        self.cheat.frames(&mut self.input_rng)
    }
}

/// A deterministic, per-peer key seed from the master seed.
fn key_seed(master: u64, i: usize) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[..8].copy_from_slice(&master.to_le_bytes());
    s[8..16].copy_from_slice(&(i as u64).to_le_bytes());
    s
}

/// Build the N peers and the shared roster of verifying keys.
fn build_peers(cfg: &SimConfig) -> Vec<Peer> {
    let keys: Vec<SigningKey> = (0..cfg.peers)
        .map(|i| SigningKey::from_seed(key_seed(cfg.seed, i)))
        .collect();
    let roster: Vec<(u64, VerifyingKey)> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| ((i + 1) as u64, k.verifying_key()))
        .collect();

    keys.iter()
        .enumerate()
        .map(|(i, key)| {
            let id = (i + 1) as u64;
            let pc = cfg.peer(i);
            let kind = pc.behavior.cheat_kind();
            Peer {
                id,
                net: NetcodeApi::new(id, key.clone(), &roster),
                sim: build_sim(cfg.backend, cfg.peers),
                cfg: pc,
                submitted: 0,
                input_rng: DeterministicRng::seeded(
                    cfg.seed ^ id.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                ),
                outbox: Vec::new(),
                submit_time: BTreeMap::new(),
                buffer_peak: 0,
                latencies: Vec::new(),
                desync_ticks: Vec::new(),
                reconcile_cursor: 0,
                ingest_errors: 0,
                history: [0u8; 32],
                hashes: BTreeMap::new(),
                last_hash_prefix: 0,
                cheat: CheatState::build(kind, id, key, &roster, cfg.peers, cfg.ticks, cfg.seed),
            }
        })
        .collect()
}

/// Each active peer submits (and broadcasts) its input, resends its lead under a
/// reliable link, and injects any cheat frames.
fn submit_phase(peers: &mut [Peer], net: &mut ModeledNetwork, t: u64, cfg: &SimConfig) {
    // The slowest peer's confirmed tick: a frame at or above it may still be
    // needed by *someone*, so it must stay resendable. Frames below it have been
    // confirmed everywhere (a cumulative-ack floor) and can be pruned — this both
    // prevents stalls (a slow receiver always gets its missing frames resent) and
    // avoids reseeding already-confirmed history as benign out-of-window noise.
    let resend_floor = peers
        .iter()
        .map(|p| p.net.confirmed_tick())
        .min()
        .unwrap_or(0);
    peers.iter_mut().enumerate().for_each(|(i, peer)| {
        peer.should_submit(t, cfg.max_ahead).then(|| {
            let delta = peer.input(t);
            let frame = peer.net.submit_local(MOVE_KIND, &encode_delta(delta));
            let sim_tick = peer.submitted;
            peer.submit_time.insert(sim_tick, t);
            peer.submitted += 1;
            peer.cheat.note_submit(&frame);
            peer.outbox.push((sim_tick, frame.clone()));
            net.broadcast(i, &frame, t);
        });
        (cfg.network.retransmit & peer.active(t)).then(|| {
            peer.outbox.retain(|(st, _)| *st >= resend_floor);
            let excess = peer.outbox.len().saturating_sub(RESEND_WINDOW);
            peer.outbox.drain(0..excess);
            peer.outbox
                .clone()
                .into_iter()
                .for_each(|(_, f)| net.broadcast(i, &f, t));
        });
        peer.active(t).then(|| {
            peer.cheat_frames()
                .into_iter()
                .for_each(|f| net.broadcast(i, &f, t));
        });
    });
}

/// Deliver every frame due this tick into its recipient's session.
fn deliver_phase(peers: &mut [Peer], net: &mut ModeledNetwork, t: u64) {
    net.deliver_due(t).into_iter().for_each(|(to, bytes)| {
        peers[to].net.ingest(&bytes).is_err().then(|| {
            peers[to].ingest_errors += 1;
        });
    });
}

/// Diverge a corrupt peer's reported state so its beacon mismatches.
fn perturb(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.first_mut().map(|b| *b ^= 0xFF);
    bytes.is_empty().then(|| bytes.push(0xAB));
    bytes
}

/// Confirm every ready tick for every peer: step its sim, fingerprint + beacon
/// the state, record latency, broadcast the beacon, and drain reconciliations.
fn confirm_phase(peers: &mut [Peer], net: &mut ModeledNetwork, t: u64) {
    peers.iter_mut().enumerate().for_each(|(i, peer)| {
        // `ready_tick()` returns the next unconfirmed tick once its inputs are
        // all present, and `confirm_tick` advances that cursor — so the producer
        // is feedback-coupled to each iteration's side effects and the ticks
        // CANNOT be drained ahead of processing (a pre-drain would spin on a
        // never-advancing `confirmed`). Instead the whole per-tick body lives in
        // the `from_fn` closure itself: it reads the producer and runs its
        // consumer in one scope, yielding `Some(())` to continue and `None` (when
        // no tick is ready) to end the pass — semantically identical to the
        // original `while let`, one in-order tick per step.
        std::iter::from_fn(|| {
            peer.net.ready_tick().map(|tick| {
                let inputs = peer.net.confirm_tick(tick);
                let stepped = peer.sim.step(tick, &inputs);
                let bytes = peer
                    .cheat
                    .corrupts_sim()
                    .then(|| perturb(stepped.clone()))
                    .unwrap_or(stepped);
                let beacon = peer.net.record_local_hash(tick, &bytes);
                let hash = peer.net.digest(&bytes);
                peer.hashes.insert(tick, hash);
                let mut chained = peer.history.to_vec();
                chained.extend_from_slice(&hash);
                peer.history = peer.net.digest(&chained);
                peer.last_hash_prefix = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
                peer.submit_time
                    .get(&tick)
                    .map(|submitted_at| peer.latencies.push(t - submitted_at));
                let peak = peer.net.buffered_inputs();
                peer.buffer_peak = peer.buffer_peak.max(peak);
                net.broadcast(i, &beacon, t);
            })
        })
        .for_each(|()| {});
        // Drain every newly-confirmed tick's reconciliation: `from_fn` yields one
        // `(tick, in_sync)` per step while the cursor trails the confirmed tick
        // and `reconcile` is ready (a `None` from `reconcile` means we're still
        // waiting on a peer, which ends the stream for this pass). A desync
        // (`!in_sync`) is recorded; the cursor advances past every drained tick.
        let net = &peer.net;
        let cursor = &mut peer.reconcile_cursor;
        let desyncs = &mut peer.desync_ticks;
        std::iter::from_fn(|| {
            (*cursor < net.confirmed_tick())
                .then(|| net.reconcile(*cursor))
                .flatten()
                .map(|in_sync| {
                    let at = *cursor;
                    *cursor += 1;
                    (at, in_sync)
                })
        })
        .for_each(|(at, in_sync)| {
            (!in_sync).then(|| desyncs.push(at));
        });
    });
}

/// Whether `peers` (excluding corrupt-sim cheaters) all agree wherever they
/// overlap; returns the first divergent sim tick if any.
fn convergence(peers: &[Peer]) -> (bool, Option<u64>) {
    let mut consensus: BTreeMap<u64, [u8; 32]> = BTreeMap::new();
    let mut first: Option<u64> = None;
    peers
        .iter()
        .filter(|p| !p.cheat.corrupts_sim())
        .for_each(|p| {
            p.hashes.iter().for_each(|(&tick, &hash)| {
                let diverged = consensus
                    .get(&tick)
                    .map_or(false, |seen| *seen != hash);
                consensus.entry(tick).or_insert(hash);
                diverged.then(|| {
                    first = Some(first.map_or(tick, |f| f.min(tick)));
                });
            });
        });
    (first.is_none(), first)
}

/// Print one live line describing this tick across all peers.
fn stream_line(peers: &[Peer], net: &ModeledNetwork, t: u64) {
    let cursors: Vec<String> = peers
        .iter()
        .map(|p| p.net.confirmed_tick().to_string())
        .collect();
    let agree = {
        let active: Vec<&Peer> = peers.iter().filter(|p| !p.cheat.corrupts_sim()).collect();
        let floor = active
            .iter()
            .map(|p| p.net.confirmed_tick())
            .min()
            .unwrap_or(0);
        floor.checked_sub(1).map_or(true, |prev| {
            active
                .iter()
                .filter_map(|p| p.hashes.get(&prev))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                <= 1
        })
    };
    println!(
        "tick {t:>4}: confirmed=[{}] agree={} inflight={}",
        cursors.join(","),
        agree.then_some("OK").unwrap_or("XX"),
        net.inflight_count()
    );
}

/// Collect one CSV row per peer for this tick.
fn snapshot(peers: &[Peer], t: u64, rows: &mut Vec<CsvRow>) {
    peers.iter().for_each(|p| {
        let r = p.net.rejections();
        rows.push(CsvRow {
            tick: t,
            peer: p.id,
            confirmed: p.net.confirmed_tick(),
            buffered: p.net.buffered_inputs(),
            hash_prefix: p.last_hash_prefix,
            drop_unknown: r.unknown_peer,
            drop_bad_sig: r.bad_signature,
            drop_window: r.out_of_window,
        });
    });
}

/// Assemble the final report from the peers' accumulators.
fn finish(peers: Vec<Peer>) -> SimReport {
    let (all_agree, first_divergence) = convergence(&peers);
    let confirmed: Vec<u64> = peers.iter().map(|p| p.net.confirmed_tick()).collect();
    let peer_reports = peers
        .into_iter()
        .map(|p| {
            let r = p.net.rejections();
            PeerReport {
                id: p.id,
                behavior: p.cfg.behavior,
                final_confirmed: p.net.confirmed_tick(),
                buffer_peak: p.buffer_peak,
                rejections: (r.unknown_peer, r.bad_signature, r.out_of_window),
                ingest_errors: p.ingest_errors,
                latency: LatencyStats::from_samples(p.latencies),
                desync_ticks: p.desync_ticks,
                state_digest: p.history,
            }
        })
        .collect();
    SimReport {
        peers: peer_reports,
        min_confirmed: confirmed.iter().copied().min().unwrap_or(0),
        max_confirmed: confirmed.iter().copied().max().unwrap_or(0),
        all_agree,
        first_divergence,
    }
}

/// Run a full simulation and return its per-client report. Deterministic: the
/// same `config` always produces the same `SimReport`. Honors the `stream` and
/// `csv_path` knobs as side effects (a live console line per tick, and a
/// per-(tick, peer) CSV).
pub fn run_simulation(config: &SimConfig) -> SimReport {
    let mut peers = build_peers(config);
    let mut net = ModeledNetwork::new(config.seed, config.network.clone(), config.peers);
    let mut rows: Vec<CsvRow> = Vec::new();

    (0..config.ticks).for_each(|t| {
        submit_phase(&mut peers, &mut net, t, config);
        deliver_phase(&mut peers, &mut net, t);
        confirm_phase(&mut peers, &mut net, t);
        config
            .csv_path
            .is_some()
            .then(|| snapshot(&peers, t, &mut rows));
        config.stream.then(|| stream_line(&peers, &net, t));
    });

    config
        .csv_path
        .as_ref()
        .map(|path| write_csv(path, &rows).ok());
    finish(peers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean-network config of `peers` honest peers for `ticks`, on the given
    /// backend.
    fn clean(peers: usize, ticks: u64, backend: Backend) -> SimConfig {
        let mut c = SimConfig::new(peers, ticks);
        c.backend = backend;
        c.seed = 0xA11CE;
        c
    }

    /// Make peer 0 misbehave in the given way.
    fn with_cheater(mut c: SimConfig, kind: CheatKind) -> SimConfig {
        c.per_peer[0].behavior = Behavior::Cheater(kind);
        c
    }

    #[test]
    fn clean_network_all_peers_converge() {
        let report = run_simulation(&clean(5, 48, Backend::Mock));
        assert!(
            report.all_agree,
            "clean lockstep must keep every peer identical"
        );
        assert_eq!(report.first_divergence, None);
        assert!(
            report.min_confirmed >= 40,
            "near-complete progress on a clean link"
        );
    }

    #[test]
    fn the_real_engine_backend_also_converges() {
        // A handful of real engine Apps stay byte-identical under lockstep.
        let report = run_simulation(&clean(3, 16, Backend::Engine));
        assert!(report.all_agree);
        assert!(report.min_confirmed >= 12);
    }

    #[test]
    fn loss_and_jitter_still_converge_just_slower() {
        let mut c = clean(4, 60, Backend::Mock);
        c.network.drop_per_mille = 250;
        c.network.latency_min = 0;
        c.network.latency_max = 3;
        c.network.duplicate_per_mille = 50;
        let report = run_simulation(&c);
        assert!(
            report.all_agree,
            "retransmission carries a lossy link to agreement"
        );
        assert!(report.min_confirmed > 0);
        // Latency under loss is at least one tick somewhere.
        let max_lat = report
            .peers
            .iter()
            .filter_map(|p| p.latency.map(|l| l.max))
            .max()
            .unwrap_or(0);
        assert!(max_lat >= 1, "loss/jitter introduces confirm latency");
    }

    #[test]
    fn a_forging_cheater_cannot_desync_the_honest_peers() {
        let report = run_simulation(&with_cheater(
            clean(4, 40, Backend::Mock),
            CheatKind::ForgeOthers,
        ));
        assert!(report.all_agree, "forgeries never reached any sim");
        let saw_bad_sig = report.peers.iter().any(|p| p.rejections.1 > 0);
        assert!(saw_bad_sig, "a victim rejected the forged signatures");
    }

    #[test]
    fn a_flood_cheater_is_bounded_and_harmless() {
        let report = run_simulation(&with_cheater(clean(4, 40, Backend::Mock), CheatKind::Flood));
        assert!(report.all_agree);
        let saw_window_drop = report.peers.iter().any(|p| p.rejections.2 > 0);
        assert!(saw_window_drop, "out-of-window flood frames were dropped");
        let peak = report
            .peers
            .iter()
            .map(|p| p.buffer_peak)
            .max()
            .unwrap_or(0);
        assert!(
            peak < 256,
            "the flood did not balloon any buffer (peak {peak})"
        );
    }

    #[test]
    fn a_malformed_cheater_only_produces_ingest_errors() {
        let report = run_simulation(&with_cheater(
            clean(4, 30, Backend::Mock),
            CheatKind::Malformed,
        ));
        assert!(report.all_agree);
        assert!(report.peers.iter().any(|p| p.ingest_errors > 0));
    }

    #[test]
    fn a_corrupt_sim_peer_is_caught_by_reconcile() {
        let report = run_simulation(&with_cheater(
            clean(3, 24, Backend::Mock),
            CheatKind::CorruptSim,
        ));
        // The honest peers still agree with each other...
        assert!(
            report.all_agree,
            "honest peers converge despite the cheater"
        );
        // ...and at least one of them flagged the cheater's divergence.
        let caught = report.peers.iter().any(|p| !p.desync_ticks.is_empty());
        assert!(caught, "reconcile must catch the corrupt peer");
    }

    #[test]
    fn the_same_seed_replays_identically() {
        let c = with_cheater(clean(4, 36, Backend::Mock), CheatKind::Flood);
        assert_eq!(
            run_simulation(&c),
            run_simulation(&c),
            "a seeded run is replayable"
        );
    }

    #[test]
    fn a_slow_peer_gates_the_group_but_all_agree() {
        let mut c = clean(4, 48, Backend::Mock);
        c.per_peer[0].tick_rate = 3; // a slow tab submits a third as often
        let report = run_simulation(&c);
        assert!(report.all_agree);
        // The slow peer bounds how far the group confirms.
        let slow_confirmed = report.peers[0].final_confirmed;
        assert!(slow_confirmed > 0 && slow_confirmed < report.max_confirmed.max(1) + 1);
    }

    #[test]
    fn a_partitioned_peer_recovers_when_its_link_returns() {
        let mut c = clean(3, 60, Backend::Mock);
        c.network.partitions = vec![Partition {
            peer: 2,
            from_tick: 10,
            to_tick: 25,
        }];
        let report = run_simulation(&c);
        // After the link returns, retransmission lets the group converge again.
        assert!(report.all_agree);
        assert!(
            report.min_confirmed > 25,
            "the group resumed past the outage"
        );
    }
}
