//! The proof that the deterministic-lockstep session actually keeps peers in
//! sync — entirely in-process, with **no sockets**.
//!
//! Multiplayer correctness reduces to one pure property:
//!
//! > Given the same ordered input timeline, every peer's state hash is identical
//! > at every confirmed tick.
//!
//! Here N peers (each a real `NetcodeApi`) are connected by a **deterministic
//! adversarial transport** that reorders, delays, and drops messages, driven by
//! the kernel's seeded `DeterministicRng` (so the whole run is replayable). Each
//! peer runs a tiny deterministic mock sim standing in for a real `App`. The
//! tests assert: every peer is byte-identical at every confirmed tick; a replay
//! with the same seed is byte-equal; and an injected divergence is caught by
//! `reconcile`.

use std::collections::BTreeMap;

use axiom_crypto::SigningKey;
use axiom_kernel::DeterministicRng;
use axiom_netcode::NetcodeApi;

/// A deterministic signing key for a peer id (so the whole run stays replayable).
fn key_for(id: u64) -> SigningKey {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&id.to_le_bytes());
    SigningKey::from_seed(seed)
}

/// A deterministic fold standing in for a real per-tick simulation: a pure
/// function of the inputs applied, in order, so two peers that apply the same
/// confirmed inputs reach the same state.
fn mix(state: u64, tick: u64, peer: u64, kind: u32, payload: &[u8]) -> u64 {
    let mut s = state ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    s = s.rotate_left(7) ^ peer.wrapping_mul(0xD1B5_4A32_D192_ED03);
    s = s.wrapping_add(kind as u64);
    for &b in payload {
        s = (s ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3);
    }
    s
}

/// A modelled lockstep network of N peers.
struct Harness {
    peers: Vec<NetcodeApi>,
    sims: Vec<u64>,
    hash_logs: Vec<BTreeMap<u64, [u8; 32]>>,
    outboxes: Vec<Vec<Vec<u8>>>,
    inflight: Vec<(u64, usize, Vec<u8>)>,
    rng: DeterministicRng,
    drop_per_mille: u32,
    max_delay: u64,
    corrupt: Option<usize>,
    frame: u64,
}

impl Harness {
    fn new(
        seed: u64,
        peer_ids: &[u64],
        drop_per_mille: u32,
        max_delay: u64,
        corrupt: Option<usize>,
    ) -> Self {
        let roster = peer_ids
            .iter()
            .map(|&id| (id, key_for(id).verifying_key()))
            .collect::<Vec<_>>();
        let peers = peer_ids
            .iter()
            .map(|&id| NetcodeApi::new(id, key_for(id), &roster))
            .collect::<Vec<_>>();
        let n = peers.len();
        Harness {
            peers,
            sims: vec![0u64; n],
            hash_logs: vec![BTreeMap::new(); n],
            outboxes: vec![Vec::new(); n],
            inflight: Vec::new(),
            rng: DeterministicRng::seeded(seed),
            drop_per_mille,
            max_delay,
            corrupt,
            frame: 0,
        }
    }

    fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Broadcast `bytes` from peer `from` to every other peer, each with a
    /// deterministic delay and possible drop.
    fn enqueue(&mut self, from: usize, bytes: Vec<u8>) {
        for to in 0..self.peers.len() {
            if to == from {
                continue;
            }
            let delay = self.rng.next_bounded(self.max_delay + 1);
            let dropped = self.rng.next_bool_in_thousand(self.drop_per_mille);
            if !dropped {
                self.inflight.push((self.frame + delay, to, bytes.clone()));
            }
        }
    }

    /// Advance one frame: optionally submit one local input each, resend
    /// outstanding messages (modelling reliable retransmission over a lossy
    /// link), deliver what is due, then confirm every ready tick. `submit =
    /// false` is a quiescent drain that only propagates already-produced frames.
    fn advance(&mut self, submit: bool) {
        let n = self.peers.len();

        // 1. Each peer submits its input for this frame's tick.
        if submit {
            for i in 0..n {
                let bytes = self.peers[i].submit_local(i as u32 + 1, &[self.frame as u8]);
                self.outboxes[i].push(bytes.clone());
                self.enqueue(i, bytes);
            }
        }

        // 2. Resend every message produced so far (a dropped packet is retried).
        for i in 0..n {
            for msg in self.outboxes[i].clone() {
                self.enqueue(i, msg);
            }
        }

        // 3. Deliver everything due this frame; keep the rest in flight.
        let mut still = Vec::new();
        for (at, to, bytes) in std::mem::take(&mut self.inflight) {
            if at <= self.frame {
                self.peers[to]
                    .ingest(&bytes)
                    .expect("harness only ever sends well-formed frames");
            } else {
                still.push((at, to, bytes));
            }
        }
        self.inflight = still;

        // 4. Confirm every ready tick; advance each peer's mock sim and hash.
        for i in 0..n {
            while let Some(tick) = self.peers[i].ready_tick() {
                for (peer, kind, payload) in self.peers[i].confirm_tick(tick) {
                    self.sims[i] = mix(self.sims[i], tick, peer, kind, &payload);
                }
                if self.corrupt == Some(i) {
                    // A persistent divergence in this peer's simulation state.
                    self.sims[i] = self.sims[i].wrapping_add(0x00BA_D000);
                }
                let state = self.sims[i].to_le_bytes();
                let beacon = self.peers[i].record_local_hash(tick, &state);
                self.outboxes[i].push(beacon.clone());
                self.hash_logs[i].insert(tick, self.peers[i].digest(&state));
                self.enqueue(i, beacon);
            }
        }

        self.frame += 1;
    }

    fn run(&mut self, frames: u64) {
        for _ in 0..frames {
            self.advance(true);
        }
    }

    /// Quiescently drain in-flight traffic (no new inputs) so trailing beacons
    /// for already-confirmed ticks arrive at every peer.
    fn settle(&mut self, rounds: u64) {
        for _ in 0..rounds {
            self.advance(false);
        }
    }

    /// The lowest confirmed-tick cursor across all peers (ticks below it are
    /// confirmed everywhere).
    fn min_confirmed(&self) -> u64 {
        self.peers
            .iter()
            .map(|p| p.confirmed_tick())
            .min()
            .unwrap_or(0)
    }
}

#[test]
fn lockstep_converges_and_all_peers_agree_under_a_lossy_reordering_network() {
    // Three peers, 30% packet loss, up to 3 frames of delay (so reordering).
    let mut h = Harness::new(0xC0FFEE, &[10, 20, 30], 300, 3, None);
    let frames = 80;
    h.run(frames);

    // Liveness: despite loss + delay, retransmission carried the session far.
    let min_confirmed = h.min_confirmed();
    assert!(
        min_confirmed >= frames - 8,
        "expected near-complete progress, got min_confirmed = {min_confirmed} of {frames}"
    );

    // Safety: at every commonly-confirmed tick, every peer's state hash agrees,
    // byte for byte. This is the multiplayer-correctness proof.
    for tick in 0..min_confirmed {
        let reference = h.hash_logs[0]
            .get(&tick)
            .expect("peer 0 confirmed this tick");
        for i in 1..h.peer_count() {
            let other = h.hash_logs[i].get(&tick).expect("peer confirmed this tick");
            assert_eq!(
                other, reference,
                "peer {i} diverged from peer 0 at confirmed tick {tick}"
            );
        }
    }
}

#[test]
fn the_same_seed_replays_byte_identically() {
    let build = || {
        let mut h = Harness::new(0x1234_5678, &[1, 2, 3], 250, 2, None);
        h.run(64);
        (h.hash_logs.clone(), h.min_confirmed())
    };
    let (logs_a, confirmed_a) = build();
    let (logs_b, confirmed_b) = build();
    assert_eq!(confirmed_a, confirmed_b);
    assert_eq!(
        logs_a, logs_b,
        "a replay with the same seed must be byte-identical"
    );
}

#[test]
fn an_injected_divergence_is_caught_by_reconcile() {
    // Peer index 1 runs a corrupted simulation. On a clean (lossless, no-delay)
    // transport every beacon arrives, so a healthy peer must detect the desync.
    let mut h = Harness::new(7, &[100, 200, 300], 0, 0, Some(1));
    h.run(8);

    // Peer 0 has every peer's beacon for the early confirmed ticks; the corrupt
    // peer's hash disagrees, so reconcile reports a desync (Some(false)).
    let saw_desync = (0..h.min_confirmed()).any(|t| h.peers[0].reconcile(t) == Some(false));
    assert!(
        saw_desync,
        "reconcile must catch the injected divergence at some confirmed tick"
    );
}

#[test]
fn a_clean_network_reaches_full_agreement() {
    // No loss, no delay, no corruption: every peer should reconcile In-Sync
    // (Some(true)) at every confirmed tick.
    let mut h = Harness::new(42, &[1, 2], 0, 0, None);
    h.run(16);
    h.settle(3); // let the final ticks' beacons arrive everywhere
    let confirmed = h.min_confirmed();
    assert!(confirmed > 0);
    for tick in 0..confirmed {
        assert_eq!(
            h.peers[0].reconcile(tick),
            Some(true),
            "clean network must be in sync at tick {tick}"
        );
    }
}
