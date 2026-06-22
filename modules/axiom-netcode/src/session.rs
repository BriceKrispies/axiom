//! The per-peer deterministic-lockstep state machine.

use std::collections::{BTreeMap, BTreeSet};

use axiom_crypto::{SigningKey, VerifyingKey};

use crate::input_timeline::InputTimeline;
use crate::net_command::NetCommand;
use crate::net_message::{NetMessage, KIND_HASH_BEACON, KIND_INPUT};
use crate::peer_id::PeerId;
use crate::rejections::Rejections;
use crate::sync_status::SyncStatus;

/// How far ahead of the confirmed cursor a peer's inputs may be buffered.
///
/// Any input outside `[confirmed, confirmed + HORIZON)` is dropped on arrival,
/// and confirmed ticks are pruned, so the live timeline holds at most
/// `peers × HORIZON` inputs no matter how much traffic an attacker injects. It
/// is far larger than any honest lead (the app submits only a handful of ticks
/// ahead), so it never constrains legitimate play.
const HORIZON: u64 = 256;

/// One peer's view of a lockstep session.
///
/// It tracks the roster (each peer's verifying key), the input timeline, a
/// confirmed-tick cursor, and the state hashes peers have reported. Every frame
/// it ingests must be **validly signed by the roster key for its claimed author**
/// and fall **within the admission window** — otherwise it is silently dropped.
/// So an honest peer's confirmed input stream is a pure function of the
/// validly-signed, in-window inputs, immune to any forged or flooding traffic; a
/// cheater can only desync *itself*, which [`Self::reconcile`] detects.
#[derive(Debug)]
pub(crate) struct Session {
    local: PeerId,
    signing_key: SigningKey,
    roster: BTreeMap<PeerId, VerifyingKey>,
    peers: Vec<PeerId>,
    confirmed: u64,
    next_local_tick: u64,
    timeline: InputTimeline,
    hashes: BTreeMap<(u64, PeerId), [u8; 32]>,
    rejections: Rejections,
}

impl Session {
    /// Build a session for `local` (raw id) holding `signing_key`, among a
    /// `roster` of `(peer raw id, verifying key)`. `local` is always part of the
    /// peer set; the set is deduplicated and kept ascending so every peer agrees
    /// on input ordering. The roster keys are how ingested frames are
    /// authenticated — a frame from a peer not in the roster is dropped.
    pub(crate) fn new(local: u64, signing_key: SigningKey, roster: &[(u64, VerifyingKey)]) -> Self {
        let local = PeerId::from_raw(local);
        let roster: BTreeMap<PeerId, VerifyingKey> = roster
            .iter()
            .map(|(raw, vk)| (PeerId::from_raw(*raw), *vk))
            .collect();
        let mut set: BTreeSet<PeerId> = roster.keys().copied().collect();
        set.insert(local);
        Session {
            local,
            signing_key,
            roster,
            peers: set.into_iter().collect(),
            confirmed: 0,
            next_local_tick: 0,
            timeline: InputTimeline::new(),
            hashes: BTreeMap::new(),
            rejections: Rejections::default(),
        }
    }

    /// The next tick awaiting confirmation (ticks below it are confirmed).
    pub(crate) fn confirmed_tick(&self) -> u64 {
        self.confirmed
    }

    /// How many inputs are currently buffered (bounded by `peers × HORIZON`) —
    /// session telemetry for buffer occupancy under load.
    pub(crate) fn buffered_inputs(&self) -> usize {
        self.timeline.entry_count()
    }

    /// How many ingested frames this session has dropped, by reason.
    pub(crate) fn rejections(&self) -> Rejections {
        self.rejections
    }

    /// Whether `tick` is inside the input admission window.
    fn in_input_window(&self, tick: u64) -> bool {
        // `&` not `&&`: both operands are pure, always-safe `u64` comparisons,
        // so eager evaluation is behavior-identical and branchless.
        (tick >= self.confirmed) & (tick < self.confirmed.saturating_add(HORIZON))
    }

    /// Whether `tick` is inside the hash-beacon admission window. Beacons report
    /// *already-confirmed* ticks, so the window reaches `HORIZON` ticks back as
    /// well as forward — old enough beacons are rejected so the table stays
    /// bounded.
    fn in_beacon_window(&self, tick: u64) -> bool {
        // `&` not `&&`: both operands are pure, always-safe `u64` comparisons.
        (tick >= self.confirmed.saturating_sub(HORIZON))
            & (tick < self.confirmed.saturating_add(HORIZON))
    }

    /// Schedule a local input at the next local tick, **sign it**, and return the
    /// wire frame to broadcast. The input is also recorded in this peer's own
    /// timeline.
    pub(crate) fn schedule_local(&mut self, kind: u32, payload: Vec<u8>) -> NetMessage {
        let tick = self.next_local_tick;
        self.next_local_tick = self.next_local_tick.saturating_add(1);
        let command = NetCommand::new(kind, payload);
        let signature = self.signing_key.sign(&NetMessage::input_signing_payload(
            self.local, tick, &command,
        ));
        self.timeline.insert(tick, self.local, command.clone());
        NetMessage::input(self.local, tick, command, signature)
    }

    /// Fold a received frame into local state — **only if** its signature
    /// verifies against the roster key for its claimed author and it falls in the
    /// admission window. An inadmissible frame (unknown peer, bad signature, or
    /// out-of-window tick) is silently dropped: adversarial traffic is expected,
    /// not an error that should halt an honest peer.
    pub(crate) fn accept(&mut self, message: NetMessage) {
        // Authenticate first, branchlessly. `roster.get(...).copied()` ends the
        // borrow of `self.roster` so the admission step can mutate `self`.
        // `map_or_else` is the `let-else` replacement: None -> count unknown
        // peer; Some(key) -> verify-then-admit. The verify result then selects
        // between counting a bad signature and admitting the frame, again with
        // `then/unwrap_or_else` rather than an `if`.
        let key = self.roster.get(&message.peer()).copied();
        let present = key.is_some();
        // `verified` is true iff the peer is in the roster AND its signature
        // matches; absent key folds to `false` via `unwrap_or`.
        let verified = key
            .map(|verifying_key| verifying_key.verify(&message.signed_bytes(), message.signature()))
            .unwrap_or(false);
        // The three outcomes are mutually exclusive by construction, applied as
        // separate guarded statements (no `if`): unknown peer, bad signature, or
        // admit. Exactly one guard is true for any frame.
        (!present).then(|| self.rejections.unknown_peer += 1);
        (present & !verified).then(|| self.rejections.bad_signature += 1);
        verified.then(|| self.admit(message));
    }

    /// Fold an already-authenticated frame into local state, counting it as
    /// out-of-window if its tick is outside the admission window. The frame is a
    /// tagged struct, so the dispatch is a kind-gated guard (no `match`): the
    /// `kind` selects the input timeline or the hash table via two mutually
    /// exclusive `then` guards — exactly one runs for any frame, mirroring the
    /// branchless guards in [`Self::accept`]. Each kind's in-window guard is
    /// branchless, and an out-of-window frame of either kind is counted once.
    fn admit(&mut self, message: NetMessage) {
        let (peer, tick, kind) = (message.peer(), message.tick(), message.kind());
        // Input: take the command (present iff this is an input frame) and admit
        // it to the timeline iff in the input window. The `.expect` cannot fire:
        // the guard is true exactly when `command()` is `Some`.
        (kind == KIND_INPUT).then(|| {
            let command = message
                .command()
                .expect("an input-kind frame always carries a command")
                .clone();
            self.in_input_window(tick)
                .then(|| self.timeline.insert(tick, peer, command))
                .unwrap_or_else(|| self.rejections.out_of_window += 1)
        });
        // HashBeacon: take the hash (present iff this is a beacon frame) and
        // admit it to the table iff in the beacon window.
        (kind == KIND_HASH_BEACON).then(|| {
            let hash = *message
                .hash()
                .expect("a beacon-kind frame always carries a hash");
            self.in_beacon_window(tick)
                .then(|| {
                    self.hashes.insert((tick, peer), hash);
                })
                .unwrap_or_else(|| self.rejections.out_of_window += 1)
        });
    }

    /// The next tick whose inputs are all present, or `None` if the lockstep
    /// gate is still waiting on a peer.
    pub(crate) fn ready_tick(&self) -> Option<u64> {
        self.timeline
            .has_all(self.confirmed, &self.peers)
            .then_some(self.confirmed)
    }

    /// Confirm `tick`, advancing the cursor and returning its ordered inputs.
    /// A no-op (empty result) unless `tick` is exactly the next unconfirmed tick
    /// and all its inputs are present — so confirmation is strictly in order. The
    /// tick's inputs are pruned afterwards (confirmed history is immutable), so
    /// the timeline never accumulates past ticks.
    pub(crate) fn confirm(&mut self, tick: u64) -> Vec<(PeerId, NetCommand)> {
        // `&` not `&&`: `has_all` is a pure read, always safe to evaluate. The
        // `then(..).unwrap_or_default()` runs the mutating body only when the
        // tick is exactly next and complete — identical to the original guard.
        ((tick == self.confirmed) & self.timeline.has_all(tick, &self.peers))
            .then(|| {
                let ordered = self.timeline.ordered_at(tick);
                self.timeline.remove_tick(tick);
                self.confirmed = self.confirmed.saturating_add(1);
                self.prune_stale_hashes();
                ordered
            })
            .unwrap_or_default()
    }

    /// Drop hash beacons that have fallen out of the (backward) admission window.
    fn prune_stale_hashes(&mut self) {
        let low = self.confirmed.saturating_sub(HORIZON);
        self.hashes.retain(|(tick, _), _| *tick >= low);
    }

    /// Record this peer's own state hash for `tick`, **sign it**, and return the
    /// beacon frame to broadcast.
    pub(crate) fn record_local_hash(&mut self, tick: u64, hash: [u8; 32]) -> NetMessage {
        let signature = self
            .signing_key
            .sign(&NetMessage::beacon_signing_payload(self.local, tick, &hash));
        self.hashes.insert((tick, self.local), hash);
        NetMessage::hash_beacon(self.local, tick, hash, signature)
    }

    /// Compare every peer's reported hash for `tick`.
    pub(crate) fn reconcile(&self, tick: u64) -> SyncStatus {
        // Branchless fold over the peers, threading the first-seen agreed hash.
        // `try_fold` short-circuits to a verdict (`Err`) exactly where the
        // original loop did an early `return`: a missing hash -> Pending, a
        // mismatch -> Desync. The accumulator is `Ok(Option<hash>)`; an `Ok` at
        // the end (no early exit) means every peer reported and all agreed ->
        // InSync. `map_or`/`map_or_else` replace the inner `match`/`if`.
        self.peers
            .iter()
            .try_fold(None::<[u8; 32]>, |agreed, peer| {
                self.hashes
                    .get(&(tick, *peer))
                    .map_or(Err(SyncStatus::Pending), |hash| {
                        agreed.map_or(Ok(Some(*hash)), |first| {
                            (*hash == first)
                                .then_some(Ok(agreed))
                                .unwrap_or(Err(SyncStatus::Desync { tick }))
                        })
                    })
            })
            .map_or_else(|verdict| verdict, |_| SyncStatus::InSync)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_seed([seed; 32])
    }

    /// A `Session` for peer 1 in a `{1, 2}` game, plus peer 2's signing key (so
    /// tests can mint genuine peer-2 traffic) and the roster.
    fn duo() -> (Session, SigningKey) {
        let (k1, k2) = (key(1), key(2));
        let roster = [(1u64, k1.verifying_key()), (2u64, k2.verifying_key())];
        (Session::new(1, k1, &roster), k2)
    }

    /// A genuine signed input from `signer` claiming `peer`/`tick`.
    fn signed_input(signer: &SigningKey, peer: u64, tick: u64, kind: u32) -> NetMessage {
        let peer = PeerId::from_raw(peer);
        let command = NetCommand::new(kind, vec![kind as u8]);
        let signature = signer.sign(&NetMessage::input_signing_payload(peer, tick, &command));
        NetMessage::input(peer, tick, command, signature)
    }

    #[test]
    fn new_includes_local_dedups_and_sorts_via_confirm_order() {
        // Peer 2 among a roster {3, 1, 3}: the peer set becomes {1, 2, 3} (local
        // included, the duplicate 3 collapsed). Confirm order is the sorted set.
        let k2 = key(2);
        let roster = [
            (3u64, key(3).verifying_key()),
            (1u64, key(1).verifying_key()),
            (3u64, key(3).verifying_key()),
        ];
        let mut s = Session::new(2, k2.clone(), &roster);
        s.schedule_local(20, vec![0]); // local (peer 2) input for tick 0
        s.accept(signed_input(&key(1), 1, 0, 10));
        s.accept(signed_input(&key(3), 3, 0, 30));
        let ordered: Vec<u64> = s.confirm(0).iter().map(|(p, _)| p.raw()).collect();
        assert_eq!(ordered, vec![1, 2, 3]);
        assert_eq!(s.confirmed_tick(), 1);
    }

    #[test]
    fn schedule_local_signs_and_increments_tick() {
        let (mut s, _) = duo();
        let m0 = s.schedule_local(7, vec![1]);
        // The returned frame is genuinely signed by the local key.
        assert!(key(1)
            .verifying_key()
            .verify(&m0.signed_bytes(), m0.signature()));
        assert_eq!(m0.peer(), PeerId::from_raw(1));
        // The second local input lands on tick 1 (the cursor advanced) and is a
        // fully-formed, correctly-signed frame. Compared by value, so there is no
        // catch-all match arm left unexercised.
        let m1 = s.schedule_local(8, vec![2]);
        let command = NetCommand::new(8, vec![2]);
        let signature = key(1).sign(&NetMessage::input_signing_payload(
            PeerId::from_raw(1),
            1,
            &command,
        ));
        assert_eq!(
            m1,
            NetMessage::input(PeerId::from_raw(1), 1, command, signature)
        );
    }

    #[test]
    fn ready_tick_waits_for_all_peers_then_confirms_in_order() {
        let (mut s, k2) = duo();
        s.schedule_local(10, vec![0]);
        assert_eq!(s.ready_tick(), None, "peer 2 has not sent tick 0 yet");
        s.accept(signed_input(&k2, 2, 0, 20));
        assert_eq!(s.ready_tick(), Some(0));
        let ids: Vec<u64> = s.confirm(0).iter().map(|(p, _)| p.raw()).collect();
        assert_eq!(ids, vec![1, 2], "inputs ordered by peer");
        assert_eq!(s.confirmed_tick(), 1);
    }

    #[test]
    fn confirm_is_a_noop_out_of_order_or_when_incomplete() {
        let (mut s, k2) = duo();
        s.schedule_local(10, vec![0]);
        assert!(s.confirm(0).is_empty(), "incomplete: peer 2 missing");
        assert_eq!(s.confirmed_tick(), 0);
        s.accept(signed_input(&k2, 2, 0, 20));
        assert!(s.confirm(5).is_empty(), "wrong tick: not the next one");
        assert_eq!(s.confirmed_tick(), 0);
        assert!(!s.confirm(0).is_empty(), "correct tick advances");
        assert_eq!(s.confirmed_tick(), 1);
    }

    #[test]
    fn an_unknown_peer_is_dropped() {
        let (mut s, _) = duo();
        // Peer 99 is not in the roster, even with a valid self-signature.
        s.accept(signed_input(&key(99), 99, 0, 5));
        assert_eq!(s.buffered_inputs(), 0);
        assert_eq!(s.rejections().unknown_peer, 1);
        assert_eq!(s.rejections().total(), 1);
    }

    #[test]
    fn a_forged_signature_is_dropped() {
        let (mut s, _) = duo();
        // Claims peer 2 but is signed by the wrong key (an impersonation attempt).
        s.accept(signed_input(&key(7), 2, 0, 5));
        s.schedule_local(1, vec![]);
        assert_eq!(
            s.ready_tick(),
            None,
            "the forged peer-2 input did not count"
        );
        assert_eq!(s.buffered_inputs(), 1, "only the local input is buffered");
        assert_eq!(s.rejections().bad_signature, 1);
    }

    #[test]
    fn out_of_window_inputs_are_dropped() {
        let (mut s, k2) = duo();
        // A tick far in the future (beyond HORIZON) is rejected.
        s.accept(signed_input(&k2, 2, HORIZON, 5));
        assert_eq!(s.buffered_inputs(), 0, "future-flood input dropped");
        // Advance confirmed to 1, then a past tick (0 < confirmed) is rejected.
        s.schedule_local(1, vec![]);
        s.accept(signed_input(&k2, 2, 0, 5));
        s.confirm(0);
        assert_eq!(s.confirmed_tick(), 1);
        s.accept(signed_input(&k2, 2, 0, 9)); // tick 0 < confirmed 1
                                              // Only peer 1's tick-1 local will exist; peer 2's stale tick 0 ignored.
        assert!(s.ready_tick().is_none());
        // Both the future and the past input were counted as out-of-window.
        assert_eq!(s.rejections().out_of_window, 2);
    }

    #[test]
    fn confirm_prunes_so_a_future_flood_stays_bounded() {
        let (mut s, k2) = duo();
        s.schedule_local(1, vec![]);
        s.accept(signed_input(&k2, 2, 0, 2));
        // A valid-signed flood of distinct future ticks from peer 2.
        for t in 1..50 {
            s.accept(signed_input(&k2, 2, t, 2));
        }
        let before = s.buffered_inputs();
        s.confirm(0);
        // Tick 0's two inputs are pruned; the in-window flood remains bounded.
        assert!(s.buffered_inputs() < before);
        assert!(s.buffered_inputs() as u64 <= 2 * HORIZON);
    }

    #[test]
    fn reconcile_reports_pending_in_sync_and_desync() {
        let (mut s, k2) = duo();
        assert_eq!(s.reconcile(0), SyncStatus::Pending, "nobody reported");
        s.record_local_hash(0, [1u8; 32]);
        assert_eq!(s.reconcile(0), SyncStatus::Pending, "peer 2 still missing");
        // Peer 2 agrees -> InSync (a genuinely signed beacon).
        let beacon = {
            let hash = [1u8; 32];
            let sig = k2.sign(&NetMessage::beacon_signing_payload(
                PeerId::from_raw(2),
                0,
                &hash,
            ));
            NetMessage::hash_beacon(PeerId::from_raw(2), 0, hash, sig)
        };
        s.accept(beacon);
        assert_eq!(s.reconcile(0), SyncStatus::InSync);
        // A divergent hash at tick 1 -> Desync.
        s.record_local_hash(1, [1u8; 32]);
        let bad = {
            let hash = [2u8; 32];
            let sig = k2.sign(&NetMessage::beacon_signing_payload(
                PeerId::from_raw(2),
                1,
                &hash,
            ));
            NetMessage::hash_beacon(PeerId::from_raw(2), 1, hash, sig)
        };
        s.accept(bad);
        assert_eq!(s.reconcile(1), SyncStatus::Desync { tick: 1 });
    }

    #[test]
    fn a_forged_beacon_is_dropped() {
        let (mut s, _) = duo();
        s.record_local_hash(0, [1u8; 32]);
        // A beacon claiming peer 2 but signed by the wrong key is ignored, so
        // reconcile still waits on peer 2.
        let forged = {
            let hash = [1u8; 32];
            let sig = key(8).sign(&NetMessage::beacon_signing_payload(
                PeerId::from_raw(2),
                0,
                &hash,
            ));
            NetMessage::hash_beacon(PeerId::from_raw(2), 0, hash, sig)
        };
        s.accept(forged);
        assert_eq!(s.reconcile(0), SyncStatus::Pending);
        assert_eq!(s.rejections().bad_signature, 1);
    }

    #[test]
    fn beacons_outside_the_window_are_dropped_and_old_ones_pruned() {
        // Drive confirmed past HORIZON so the backward window edge is non-zero.
        let (mut s, k2) = duo();
        for t in 0..=HORIZON + 1 {
            s.schedule_local(0, vec![]);
            s.accept(signed_input(&k2, 2, t, 0));
            assert!(!s.confirm(t).is_empty());
        }
        let confirmed = s.confirmed_tick();
        assert_eq!(confirmed, HORIZON + 2);
        // A beacon too far in the future is dropped.
        let future = confirmed + HORIZON;
        let mk = |tick: u64| {
            let hash = [9u8; 32];
            let sig = k2.sign(&NetMessage::beacon_signing_payload(
                PeerId::from_raw(2),
                tick,
                &hash,
            ));
            NetMessage::hash_beacon(PeerId::from_raw(2), tick, hash, sig)
        };
        s.accept(mk(future));
        assert_eq!(
            s.reconcile(future),
            SyncStatus::Pending,
            "future beacon dropped"
        );
        // A beacon older than the backward edge (confirmed - HORIZON) is dropped.
        let stale = confirmed - HORIZON - 1;
        s.accept(mk(stale));
        assert_eq!(
            s.reconcile(stale),
            SyncStatus::Pending,
            "stale beacon dropped"
        );
        // Both out-of-window beacons were counted.
        assert_eq!(s.rejections().out_of_window, 2);
    }

    #[test]
    fn an_attacker_without_a_roster_key_cannot_change_the_confirmed_stream() {
        // The honest confirmed stream, computed twice: once clean, once with a
        // storm of attacker traffic interleaved. The attacker (a compromised
        // relay or third party) holds NO roster private key, so it can only
        // forge, replay, or flood — never author a roster peer's input. The two
        // streams must be byte-identical, and the polluted buffer stays bounded.
        //
        // (A *roster* peer can of course choose its own input — including a bad
        // one — but that only desyncs itself, which `reconcile` catches; that is
        // a different property from forgery resistance.)
        fn run(with_attacker: bool) -> (Vec<Vec<(u64, u32)>>, usize) {
            let (mut s, k2) = duo();
            let mut stream = Vec::new();
            let mut peak = 0usize;
            for tick in 0..8u64 {
                s.schedule_local(tick as u32, vec![tick as u8]);
                let genuine = signed_input(&k2, 2, tick, 100 + tick as u32);
                s.accept(genuine.clone());
                if with_attacker {
                    // Forgeries: claim a roster peer but sign with a non-roster
                    // key (7), so verification fails. None can author peer 1 or 2.
                    s.accept(signed_input(&key(7), 2, tick, 9)); // forged peer 2
                    s.accept(signed_input(&key(7), 1, tick, 9)); // forged peer 1
                    s.accept(signed_input(&key(99), 99, tick, 9)); // unknown peer
                                                                   // Replay of a genuine frame: idempotent, no effect.
                    s.accept(genuine.clone());
                    // Forged future flood + forged replayed-past: all dropped.
                    for f in 0..40 {
                        s.accept(signed_input(&key(7), 2, tick + 1 + f, 9));
                    }
                    s.accept(signed_input(&key(7), 2, tick.saturating_sub(1), 9));
                }
                peak = peak.max(s.buffered_inputs());
                let confirmed: Vec<(u64, u32)> = s
                    .confirm(tick)
                    .iter()
                    .map(|(p, c)| (p.raw(), c.kind()))
                    .collect();
                stream.push(confirmed);
            }
            (stream, peak)
        }
        let (clean, _) = run(false);
        let (polluted, peak) = run(true);
        assert_eq!(
            clean, polluted,
            "attacker changed nothing the honest peer confirmed"
        );
        assert_eq!(clean.len(), 8);
        assert!(
            (peak as u64) <= 2 * HORIZON,
            "buffer stayed bounded under flood"
        );
    }
}
