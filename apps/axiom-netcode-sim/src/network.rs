//! A seeded, replayable model of the wire between peers.

use axiom_kernel::DeterministicRng;

use crate::config::NetworkConfig;

/// A broadcast transport with per-recipient loss, delay/jitter, duplication, and
/// partition outages — all driven by one [`DeterministicRng`], so an entire run
/// is a pure function of the seed.
#[derive(Debug)]
pub(crate) struct ModeledNetwork {
    rng: DeterministicRng,
    cfg: NetworkConfig,
    peers: usize,
    /// Frames in flight: `(deliver_tick, recipient, bytes)`.
    inflight: Vec<(u64, usize, Vec<u8>)>,
}

impl ModeledNetwork {
    /// A network for `peers` peers, seeded by `seed`.
    pub(crate) fn new(seed: u64, cfg: NetworkConfig, peers: usize) -> Self {
        ModeledNetwork {
            rng: DeterministicRng::seeded(seed),
            cfg,
            peers,
            inflight: Vec::new(),
        }
    }

    /// Whether `peer`'s links are cut at `tick`.
    fn partitioned(&self, peer: usize, tick: u64) -> bool {
        self.cfg
            .partitions
            .iter()
            .any(|p| p.peer == peer && tick >= p.from_tick && tick < p.to_tick)
    }

    /// A delivery delay in `[latency_min, latency_max]`.
    fn delay(&mut self) -> u64 {
        let span = self.cfg.latency_max.saturating_sub(self.cfg.latency_min) + 1;
        self.cfg.latency_min + self.rng.next_bounded(span)
    }

    /// Offer `bytes` from peer `from` to every other peer at driver tick `now`,
    /// applying drop / delay / duplication per recipient. A partitioned sender
    /// emits nothing.
    pub(crate) fn broadcast(&mut self, from: usize, bytes: &[u8], now: u64) {
        if self.partitioned(from, now) {
            return;
        }
        for to in 0..self.peers {
            if to == from {
                continue;
            }
            if self.rng.next_bool_in_thousand(self.cfg.drop_per_mille) {
                continue;
            }
            let at = now + self.delay();
            self.inflight.push((at, to, bytes.to_vec()));
            if self.rng.next_bool_in_thousand(self.cfg.duplicate_per_mille) {
                let at2 = now + self.delay();
                self.inflight.push((at2, to, bytes.to_vec()));
            }
        }
    }

    /// Remove and return every frame due by `now` as `(recipient, bytes)`. A
    /// frame whose recipient is partitioned at delivery is dropped.
    pub(crate) fn deliver_due(&mut self, now: u64) -> Vec<(usize, Vec<u8>)> {
        let mut delivered = Vec::new();
        let mut still = Vec::new();
        for (at, to, bytes) in std::mem::take(&mut self.inflight) {
            if at > now {
                still.push((at, to, bytes));
            } else if !self.partitioned(to, now) {
                delivered.push((to, bytes));
            }
        }
        self.inflight = still;
        delivered
    }

    /// How many frames are currently in flight.
    pub(crate) fn inflight_count(&self) -> usize {
        self.inflight.len()
    }
}
