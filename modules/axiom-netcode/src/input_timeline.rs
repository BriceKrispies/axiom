//! The tick-indexed, per-peer input buffer a confirmed tick replays from.

use std::collections::BTreeMap;

use crate::net_command::NetCommand;
use crate::peer_id::PeerId;

/// Every peer's inputs, keyed by `(tick, peer)` in stable order.
///
/// A [`BTreeMap`] keeps iteration deterministic: inputs for a given tick come
/// back sorted by peer, so every peer assembles a confirmed tick's commands in
/// the identical order — the precondition for byte-identical simulation.
#[derive(Debug, Default)]
pub struct InputTimeline {
    inputs: BTreeMap<(u64, PeerId), NetCommand>,
}

impl InputTimeline {
    /// An empty timeline.
    pub fn new() -> Self {
        InputTimeline {
            inputs: BTreeMap::new(),
        }
    }

    /// Record `peer`'s input for `tick`. Idempotent: a resent input for an
    /// already-recorded `(tick, peer)` is ignored, so duplicate delivery cannot
    /// change the timeline.
    pub fn insert(&mut self, tick: u64, peer: PeerId, command: NetCommand) {
        self.inputs.entry((tick, peer)).or_insert(command);
    }

    /// Whether every peer in `peers` has an input recorded at `tick`.
    pub fn has_all(&self, tick: u64, peers: &[PeerId]) -> bool {
        peers.iter().all(|p| self.inputs.contains_key(&(tick, *p)))
    }

    /// The `(peer, command)` inputs recorded at `tick`, in ascending peer order.
    pub fn ordered_at(&self, tick: u64) -> Vec<(PeerId, NetCommand)> {
        self.inputs
            .iter()
            .filter(|((t, _), _)| *t == tick)
            .map(|((_, peer), command)| (*peer, command.clone()))
            .collect()
    }

    /// Drop every input recorded at `tick`. Called once a tick is confirmed (its
    /// inputs are immutable thereafter), so confirmed history does not leak and
    /// the live timeline stays bounded.
    pub fn remove_tick(&mut self, tick: u64) {
        self.inputs.retain(|(t, _), _| *t != tick);
    }

    /// The total number of inputs currently buffered (across all ticks/peers).
    /// Surfaced as session telemetry (buffer occupancy under load).
    pub fn entry_count(&self) -> usize {
        self.inputs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(kind: u32) -> NetCommand {
        NetCommand::new(kind, vec![kind as u8])
    }

    #[test]
    fn new_and_default_are_empty() {
        assert!(InputTimeline::new().ordered_at(0).is_empty());
        assert!(InputTimeline::default().ordered_at(0).is_empty());
    }

    #[test]
    fn has_all_requires_every_peer() {
        let peers = [PeerId::from_raw(1), PeerId::from_raw(2)];
        let mut t = InputTimeline::new();
        t.insert(0, peers[0], cmd(1));
        assert!(!t.has_all(0, &peers), "missing peer 2");
        t.insert(0, peers[1], cmd(2));
        assert!(t.has_all(0, &peers));
        assert!(!t.has_all(1, &peers));
    }

    #[test]
    fn ordered_at_is_sorted_by_peer_and_scoped_to_the_tick() {
        let mut t = InputTimeline::new();
        t.insert(0, PeerId::from_raw(2), cmd(20));
        t.insert(0, PeerId::from_raw(1), cmd(10));
        t.insert(1, PeerId::from_raw(1), cmd(11));
        let at0 = t.ordered_at(0);
        assert_eq!(at0.len(), 2);
        assert_eq!(at0[0].0, PeerId::from_raw(1));
        assert_eq!(at0[1].0, PeerId::from_raw(2));
        assert_eq!(t.ordered_at(1), vec![(PeerId::from_raw(1), cmd(11))]);
    }

    #[test]
    fn insert_is_idempotent() {
        let mut t = InputTimeline::new();
        let p = PeerId::from_raw(1);
        t.insert(0, p, cmd(10));
        t.insert(0, p, cmd(99));
        assert_eq!(t.ordered_at(0), vec![(p, cmd(10))]);
    }

    #[test]
    fn remove_tick_drops_only_that_tick_and_tracks_len() {
        let mut t = InputTimeline::new();
        t.insert(0, PeerId::from_raw(1), cmd(1));
        t.insert(0, PeerId::from_raw(2), cmd(2));
        t.insert(1, PeerId::from_raw(1), cmd(3));
        assert_eq!(t.entry_count(), 3);
        t.remove_tick(0);
        assert_eq!(t.entry_count(), 1);
        assert!(t.ordered_at(0).is_empty());
        assert_eq!(t.ordered_at(1), vec![(PeerId::from_raw(1), cmd(3))]);
    }
}
