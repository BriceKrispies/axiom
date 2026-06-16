//! The single public facade of the `axiom-netcode` module.

use axiom_crypto::{SigningKey, VerifyingKey};
use axiom_kernel::KernelResult;

use crate::digest::digest;
use crate::net_message::NetMessage;
use crate::rejections::Rejections;
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
    /// Open a session for `local` (raw id) holding `signing_key`, among a
    /// `roster` of `(peer raw id, verifying key)` — every participant's public
    /// key, including this one's. `local` is added to the peer set if absent.
    ///
    /// The roster is the trust anchor: every ingested frame must be validly
    /// signed by the roster key for its claimed author, so a peer (or relay)
    /// cannot forge another peer's inputs. How far ahead the app submits is the
    /// app loop's policy; the session confirms a tick once every peer's input
    /// for it has arrived.
    pub fn new(local: u64, signing_key: SigningKey, roster: &[(u64, VerifyingKey)]) -> Self {
        NetcodeApi {
            session: Session::new(local, signing_key, roster),
        }
    }

    /// Schedule a local input, **sign it**, and return the wire bytes the app
    /// must broadcast.
    pub fn submit_local(&mut self, kind: u32, payload: &[u8]) -> Vec<u8> {
        self.session.schedule_local(kind, payload.to_vec()).encode()
    }

    /// Decode a received wire message and fold it into the session. Fails with a
    /// kernel error if the bytes are *malformed* (bad version, unknown tag, or
    /// truncated). A well-formed frame that is *inadmissible* — forged signature,
    /// unknown peer, or an out-of-window tick — decodes fine and is then silently
    /// dropped by the session, never affecting confirmed state.
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

    /// How many inputs are currently buffered (bounded by `peers × HORIZON`).
    /// Telemetry: buffer occupancy, e.g. to watch it stay bounded under a flood.
    pub fn buffered_inputs(&self) -> usize {
        self.session.buffered_inputs()
    }

    /// How many ingested frames this session has dropped, grouped by reason
    /// (unknown peer / bad signature / out-of-window). Telemetry for observing a
    /// session under attack — e.g. how many forgeries a peer shrugged off.
    pub fn rejections(&self) -> Rejections {
        self.session.rejections()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A matched pair of peers (1 and 2), each with its own key and the shared
    /// two-key roster — the facade-level equivalent of two browsers.
    fn pair() -> (NetcodeApi, NetcodeApi) {
        let k1 = SigningKey::from_seed([1u8; 32]);
        let k2 = SigningKey::from_seed([2u8; 32]);
        let roster = [(1u64, k1.verifying_key()), (2u64, k2.verifying_key())];
        (
            NetcodeApi::new(1, k1, &roster),
            NetcodeApi::new(2, k2, &roster),
        )
    }

    fn solo() -> NetcodeApi {
        let k = SigningKey::from_seed([1u8; 32]);
        let roster = [(1u64, k.verifying_key())];
        NetcodeApi::new(1, k, &roster)
    }

    #[test]
    fn ingest_rejects_malformed_bytes() {
        let mut peer = solo();
        assert!(peer.ingest(&[0xFF, 0xFF]).is_err());
        assert_eq!(peer.confirmed_tick(), 0);
    }

    #[test]
    fn submit_then_ingest_round_trips_an_input() {
        // Peer 1 submits locally; peer 2 ingests peer 1's signed bytes and, once
        // it adds its own input, confirms tick 0 with both inputs in peer order.
        let (mut p1, mut p2) = pair();
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
    fn an_impersonated_input_is_ignored_through_the_facade() {
        // Peer 1 signs an input but tags it as peer 2 — a different peer's bytes,
        // verified against peer 1's roster key, fail and are dropped. (Peer 1
        // cannot mint peer-2 bytes: its signature is over `peer = 2` but made
        // with key 1, so peer 2's roster entry rejects it.)
        let mut p2 = pair().1;
        // A rogue holding peer 1's key submits *as peer 2*: a frame claiming
        // peer 2 but signed by key 1.
        let forged = {
            let k = SigningKey::from_seed([1u8; 32]); // peer 1's key
            let roster = [(2u64, k.verifying_key())];
            let mut liar = NetcodeApi::new(2, k, &roster);
            liar.submit_local(5, &[1])
        };
        // p2's roster has the REAL peer-2 key, so the forged frame fails to verify.
        p2.ingest(&forged).unwrap();
        p2.submit_local(0, &[]);
        assert_eq!(p2.ready_tick(), None, "no genuine peer-1 input arrived");
    }

    #[test]
    fn confirm_tick_is_empty_when_not_ready() {
        let mut peer = pair().0;
        peer.submit_local(1, &[]);
        assert!(peer.confirm_tick(0).is_empty(), "peer 2 missing");
    }

    #[test]
    fn reconcile_maps_all_three_states() {
        let (mut p1, mut p2) = pair();
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
        let peer = solo();
        assert_eq!(peer.digest(b"abc"), peer.digest(b"abc"));
        assert_ne!(peer.digest(b"abc"), peer.digest(b"abd"));
    }

    #[test]
    fn telemetry_reports_buffer_occupancy_and_rejections() {
        let (mut p1, mut p2) = pair();
        // A genuine peer-2 input buffers; an unknown-peer forgery is dropped and
        // counted. Both are observable through the facade.
        let real = p2.submit_local(3, &[7]);
        p1.ingest(&real).unwrap();
        assert_eq!(p1.buffered_inputs(), 1);
        assert_eq!(p1.rejections(), Default::default());

        let rogue = SigningKey::from_seed([200u8; 32]);
        let mut outsider = NetcodeApi::new(9, rogue.clone(), &[(9, rogue.verifying_key())]);
        let forged = outsider.submit_local(0, &[]);
        p1.ingest(&forged).unwrap();
        assert_eq!(p1.buffered_inputs(), 1, "the forgery did not buffer");
        assert_eq!(p1.rejections().unknown_peer, 1);
        assert_eq!(p1.rejections().total(), 1);
    }
}
