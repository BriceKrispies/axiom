//! The single public facade of the `axiom-netcode` module.

use axiom_kernel::KernelResult;

use crate::digest::digest;
use crate::net_message::NetMessage;
use crate::session::Session;
use crate::sync_status::SyncStatus;

/// One peer's deterministic-lockstep session — the only public export of
/// `axiom-netcode`.
///
/// Construct one `NetcodeApi` per participant. The app owns the socket loop and
/// the simulation; this facade owns the input timeline, the readiness gate, the
/// wire codec, and state-hash reconciliation. Everything crosses the boundary as
/// **plain bytes / primitives** — there is no socket and no module type to name.
///
/// The same per-frame loop runs identically on server and client:
///
/// ```text
/// let bytes = peer.submit_local(kind, &payload);   // -> send on your socket
/// for msg in socket.drain() { peer.ingest(&msg)?; }
/// while let Some(t) = peer.ready_tick() {
///     for (id, kind, payload) in peer.confirm_tick(t) { sim.apply(id, kind, payload); }
///     sim.tick(t);
///     let beacon = peer.record_local_hash(t, &sim.snapshot_bytes());
///     socket.broadcast(beacon);
///     if peer.reconcile(t) == Some(false) { halt_desync(t); }
/// }
/// ```
#[derive(Debug)]
pub struct NetcodeApi {
    session: Session,
}

impl NetcodeApi {
    /// Open a session for `local` among `peers` (raw ids; `local` is added if
    /// absent). How far ahead the app submits inputs is the app loop's policy,
    /// not the session's — the session simply confirms a tick once every peer's
    /// input for it has arrived.
    pub fn new(local: u64, peers: &[u64]) -> Self {
        NetcodeApi {
            session: Session::new(local, peers),
        }
    }

    /// Schedule a local input and return the wire bytes the app must broadcast.
    pub fn submit_local(&mut self, kind: u32, payload: &[u8]) -> Vec<u8> {
        self.session.schedule_local(kind, payload.to_vec()).encode()
    }

    /// Decode a received wire message and fold it into the session. Fails with a
    /// kernel error if the bytes are malformed (bad version, unknown tag, or
    /// truncated).
    pub fn ingest(&mut self, message: &[u8]) -> KernelResult<()> {
        self.session.accept(NetMessage::decode(message)?);
        Ok(())
    }

    /// The next tick whose inputs are all present (the lockstep gate), or `None`
    /// while still waiting on a peer.
    pub fn ready_tick(&self) -> Option<u64> {
        self.session.ready_tick()
    }

    /// The next tick awaiting confirmation (ticks below it are confirmed).
    pub fn confirmed_tick(&self) -> u64 {
        self.session.confirmed_tick()
    }

    /// Confirm `tick`, returning its ordered `(peer, kind, payload)` inputs for
    /// the app to apply. Empty unless `tick` is the next unconfirmed tick with
    /// all inputs present (confirmation is strictly in order).
    pub fn confirm_tick(&mut self, tick: u64) -> Vec<(u64, u32, Vec<u8>)> {
        self.session
            .confirm(tick)
            .into_iter()
            .map(|(peer, command)| (peer.raw(), command.kind(), command.payload().to_vec()))
            .collect()
    }

    /// Record this peer's state hash for `tick` (digesting `state`) and return
    /// the beacon bytes to broadcast.
    pub fn record_local_hash(&mut self, tick: u64, state: &[u8]) -> Vec<u8> {
        let hash = digest(state);
        self.session.record_local_hash(tick, hash).encode()
    }

    /// Reconcile the peers' hashes for `tick`: `None` while waiting on a peer,
    /// `Some(true)` if all agree (in sync), `Some(false)` if they diverge (a
    /// desync at `tick` — the app should halt/resync).
    pub fn reconcile(&self, tick: u64) -> Option<bool> {
        match self.session.reconcile(tick) {
            SyncStatus::Pending => None,
            SyncStatus::InSync => Some(true),
            SyncStatus::Desync { .. } => Some(false),
        }
    }

    /// The canonical deterministic 256-bit digest of `bytes`. Apps hash their
    /// per-tick snapshot through this so every peer fingerprints state the same
    /// way.
    pub fn digest(&self, bytes: &[u8]) -> [u8; 32] {
        digest(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_rejects_malformed_bytes() {
        let mut peer = NetcodeApi::new(1, &[2]);
        assert!(peer.ingest(&[0xFF, 0xFF]).is_err());
        assert_eq!(peer.confirmed_tick(), 0);
    }

    #[test]
    fn submit_then_ingest_round_trips_an_input() {
        // Peer 1 submits locally; peer 2 ingests peer 1's bytes and, once it adds
        // its own input, confirms tick 0 with both inputs in peer order.
        let mut p1 = NetcodeApi::new(1, &[2]);
        let mut p2 = NetcodeApi::new(2, &[1]);
        let bytes = p1.submit_local(7, &[1, 2, 3]);
        p2.ingest(&bytes).unwrap();
        assert_eq!(p2.ready_tick(), None, "peer 2 still owes tick 0");
        p2.submit_local(9, &[4]);
        assert_eq!(p2.ready_tick(), Some(0));
        let inputs = p2.confirm_tick(0);
        assert_eq!(
            inputs,
            vec![(1u64, 7u32, vec![1, 2, 3]), (2u64, 9u32, vec![4])]
        );
        assert_eq!(p2.confirmed_tick(), 1);
    }

    #[test]
    fn confirm_tick_is_empty_when_not_ready() {
        let mut peer = NetcodeApi::new(1, &[2]);
        peer.submit_local(1, &[]);
        assert!(peer.confirm_tick(0).is_empty(), "peer 2 missing");
    }

    #[test]
    fn reconcile_maps_all_three_states() {
        let mut p1 = NetcodeApi::new(1, &[2]);
        let mut p2 = NetcodeApi::new(2, &[1]);
        // Pending: no hashes yet.
        assert_eq!(p1.reconcile(0), None);
        // In sync: both peers report the same hash for the same state.
        p1.record_local_hash(0, b"state-0");
        let p2_beacon = p2.record_local_hash(0, b"state-0");
        p1.ingest(&p2_beacon).unwrap();
        assert_eq!(p1.reconcile(0), Some(true));
        // Desync: peer 2 reports a different hash at tick 1.
        p1.record_local_hash(1, b"state-1");
        let p2_bad = p2.record_local_hash(1, b"DIFFERENT");
        p1.ingest(&p2_bad).unwrap();
        assert_eq!(p1.reconcile(1), Some(false));
    }

    #[test]
    fn digest_is_deterministic_through_the_facade() {
        let peer = NetcodeApi::new(1, &[]);
        assert_eq!(peer.digest(b"abc"), peer.digest(b"abc"));
        assert_ne!(peer.digest(b"abc"), peer.digest(b"abd"));
    }
}
