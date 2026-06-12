//! The per-peer deterministic-lockstep state machine.

use std::collections::{BTreeMap, BTreeSet};

use crate::input_timeline::InputTimeline;
use crate::net_command::NetCommand;
use crate::net_message::NetMessage;
use crate::peer_id::PeerId;
use crate::sync_status::SyncStatus;

/// One peer's view of a lockstep session.
///
/// It tracks the full peer set, the input timeline, a confirmed-tick cursor, and
/// the state hashes peers have reported. Every peer that ingests the same set of
/// messages drives this machine to the same confirmed inputs in the same order —
/// so their simulations stay byte-identical, and any divergence shows up as a
/// hash mismatch through [`Self::reconcile`].
#[derive(Debug)]
pub(crate) struct Session {
    local: PeerId,
    peers: Vec<PeerId>,
    confirmed: u64,
    next_local_tick: u64,
    timeline: InputTimeline,
    hashes: BTreeMap<(u64, PeerId), [u8; 32]>,
}

impl Session {
    /// Build a session for `local` among `peers` (raw ids). `local` is always
    /// part of the peer set; the set is deduplicated and kept in ascending order
    /// so every peer agrees on input ordering.
    pub(crate) fn new(local: u64, peers: &[u64]) -> Self {
        let local = PeerId::from_raw(local);
        let mut set: BTreeSet<PeerId> = peers.iter().map(|&r| PeerId::from_raw(r)).collect();
        set.insert(local);
        Session {
            local,
            peers: set.into_iter().collect(),
            confirmed: 0,
            next_local_tick: 0,
            timeline: InputTimeline::new(),
            hashes: BTreeMap::new(),
        }
    }

    /// The next tick awaiting confirmation (ticks below it are confirmed).
    pub(crate) fn confirmed_tick(&self) -> u64 {
        self.confirmed
    }

    /// Schedule a local input at the next local tick and return the wire frame
    /// to broadcast. The input is also recorded in this peer's own timeline.
    pub(crate) fn schedule_local(&mut self, kind: u32, payload: Vec<u8>) -> NetMessage {
        let tick = self.next_local_tick;
        self.next_local_tick = self.next_local_tick.saturating_add(1);
        let command = NetCommand::new(kind, payload);
        self.timeline.insert(tick, self.local, command.clone());
        NetMessage::Input {
            peer: self.local,
            tick,
            command,
        }
    }

    /// Fold a received frame into local state: an input joins the timeline; a
    /// hash beacon joins the per-peer hash table.
    pub(crate) fn accept(&mut self, message: NetMessage) {
        match message {
            NetMessage::Input {
                peer,
                tick,
                command,
            } => self.timeline.insert(tick, peer, command),
            NetMessage::HashBeacon { peer, tick, hash } => {
                self.hashes.insert((tick, peer), hash);
            }
        }
    }

    /// The next tick whose inputs are all present, or `None` if the lockstep
    /// gate is still waiting on a peer.
    pub(crate) fn ready_tick(&self) -> Option<u64> {
        if self.timeline.has_all(self.confirmed, &self.peers) {
            Some(self.confirmed)
        } else {
            None
        }
    }

    /// Confirm `tick`, advancing the cursor and returning its ordered inputs.
    /// A no-op (empty result) unless `tick` is exactly the next unconfirmed tick
    /// and all its inputs are present — so confirmation is strictly in order.
    pub(crate) fn confirm(&mut self, tick: u64) -> Vec<(PeerId, NetCommand)> {
        if tick == self.confirmed && self.timeline.has_all(tick, &self.peers) {
            self.confirmed = self.confirmed.saturating_add(1);
            self.timeline.ordered_at(tick)
        } else {
            Vec::new()
        }
    }

    /// Record this peer's own state hash for `tick` and return the beacon frame
    /// to broadcast.
    pub(crate) fn record_local_hash(&mut self, tick: u64, hash: [u8; 32]) -> NetMessage {
        self.hashes.insert((tick, self.local), hash);
        NetMessage::HashBeacon {
            peer: self.local,
            tick,
            hash,
        }
    }

    /// Compare every peer's reported hash for `tick`.
    pub(crate) fn reconcile(&self, tick: u64) -> SyncStatus {
        let mut agreed: Option<[u8; 32]> = None;
        for peer in &self.peers {
            match self.hashes.get(&(tick, *peer)) {
                None => return SyncStatus::Pending,
                Some(hash) => match agreed {
                    None => agreed = Some(*hash),
                    Some(first) => {
                        if *hash != first {
                            return SyncStatus::Desync { tick };
                        }
                    }
                },
            }
        }
        SyncStatus::InSync
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(peer: u64, tick: u64, kind: u32) -> NetMessage {
        NetMessage::Input {
            peer: PeerId::from_raw(peer),
            tick,
            command: NetCommand::new(kind, vec![kind as u8]),
        }
    }

    #[test]
    fn new_includes_local_dedups_and_sorts_via_confirm_order() {
        // Peer 2 among {3, 1, 3}: the set becomes {1, 2, 3} (local included, the
        // duplicate 3 collapsed). Confirm order reveals the sorted peer set.
        let mut s = Session::new(2, &[3, 1, 3]);
        s.schedule_local(20, vec![0]); // local (peer 2) input for tick 0
        s.accept(input(1, 0, 10));
        s.accept(input(3, 0, 30));
        let ordered: Vec<u64> = s.confirm(0).iter().map(|(p, _)| p.raw()).collect();
        assert_eq!(ordered, vec![1, 2, 3]);
        assert_eq!(s.confirmed_tick(), 1);
    }

    #[test]
    fn schedule_local_increments_tick() {
        let mut s = Session::new(1, &[]);
        let m0 = s.schedule_local(7, vec![1]);
        let m1 = s.schedule_local(8, vec![2]);
        assert_eq!(
            m0,
            NetMessage::Input {
                peer: PeerId::from_raw(1),
                tick: 0,
                command: NetCommand::new(7, vec![1]),
            }
        );
        assert_eq!(
            m1,
            NetMessage::Input {
                peer: PeerId::from_raw(1),
                tick: 1,
                command: NetCommand::new(8, vec![2]),
            }
        );
        // Single peer: tick 0 is immediately ready from its own input.
        assert_eq!(s.ready_tick(), Some(0));
    }

    #[test]
    fn ready_tick_waits_for_all_peers_then_confirms_in_order() {
        let mut s = Session::new(1, &[2]);
        s.schedule_local(10, vec![0]);
        assert_eq!(s.ready_tick(), None, "peer 2 has not sent tick 0 yet");
        s.accept(input(2, 0, 20));
        assert_eq!(s.ready_tick(), Some(0));

        let ids: Vec<u64> = s.confirm(0).iter().map(|(p, _)| p.raw()).collect();
        assert_eq!(ids, vec![1, 2], "inputs ordered by peer");
        assert_eq!(s.confirmed_tick(), 1);
    }

    #[test]
    fn confirm_is_a_noop_out_of_order_or_when_incomplete() {
        let mut s = Session::new(1, &[2]);
        s.schedule_local(10, vec![0]);
        // Incomplete: peer 2 missing.
        assert!(s.confirm(0).is_empty());
        assert_eq!(s.confirmed_tick(), 0);
        // Wrong tick: not the next unconfirmed one.
        s.accept(input(2, 0, 20));
        assert!(s.confirm(5).is_empty());
        assert_eq!(s.confirmed_tick(), 0);
        // Correct tick advances.
        assert!(!s.confirm(0).is_empty());
        assert_eq!(s.confirmed_tick(), 1);
    }

    #[test]
    fn reconcile_reports_pending_in_sync_and_desync() {
        let mut s = Session::new(1, &[2]);
        // Pending: nobody reported.
        assert_eq!(s.reconcile(0), SyncStatus::Pending);
        // Local reports; peer 2 still missing -> still Pending.
        s.record_local_hash(0, [1u8; 32]);
        assert_eq!(s.reconcile(0), SyncStatus::Pending);
        // Peer 2 agrees -> InSync.
        s.accept(NetMessage::HashBeacon {
            peer: PeerId::from_raw(2),
            tick: 0,
            hash: [1u8; 32],
        });
        assert_eq!(s.reconcile(0), SyncStatus::InSync);
        // A divergent hash at the next tick -> Desync.
        s.record_local_hash(1, [1u8; 32]);
        s.accept(NetMessage::HashBeacon {
            peer: PeerId::from_raw(2),
            tick: 1,
            hash: [2u8; 32],
        });
        assert_eq!(s.reconcile(1), SyncStatus::Desync { tick: 1 });
    }
}
