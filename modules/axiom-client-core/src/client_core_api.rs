//! The single public facade of the `axiom-client-core` module.

use axiom_kernel::Tick;

use crate::connection_state::ConnectionState;

/// The portable client-side multiplayer state machine — the only public export
/// of `axiom-client-core`.
///
/// One `ClientCoreApi` is one client's session bookkeeping. It holds no socket
/// and no wire codec: the app feeds it the authoritative values a server sent
/// (decoded elsewhere via `axiom-net-protocol`) and reads back the outbound
/// intent fields to encode and send. Every method is deterministic; given the
/// same calls it produces the same state.
///
/// State-machine rejections are **normal outcomes**, not errors: an operation
/// that does not apply in the current state returns `false` (transitions) or
/// `None` (intent creation), never a panic and never a kernel error.
///
/// ```text
/// let mut client = ClientCoreApi::new();          // Disconnected
/// client.connect();                               // → Connecting
/// client.accept_welcome(server_tick);             // → Connected (on Welcome)
/// if let Some(intent) = client.next_intent(t, s, &payload) { send(encode(intent)); }
/// client.accept_snapshot(server_tick, last_acked_seq);   // drains pending
/// client.accept_rejected_intent(seq);             // drops one pending intent
/// ```
#[derive(Debug, Clone)]
pub struct ClientCoreApi {
    state: ConnectionState,
    next_client_sequence: u64,
    latest_server_tick: Tick,
    last_acked_client_sequence: u64,
    pending: Vec<u64>,
}

impl ClientCoreApi {
    /// Connection status: no session.
    pub const STATUS_DISCONNECTED: u8 = 0;
    /// Connection status: establishing a session, not yet welcomed.
    pub const STATUS_CONNECTING: u8 = 1;
    /// Connection status: welcomed by the server; intents may flow.
    pub const STATUS_CONNECTED: u8 = 2;

    /// The first client sequence assigned to an outbound intent.
    pub const FIRST_CLIENT_SEQUENCE: u64 = 1;

    /// Create a fresh, disconnected client. The first intent it ever produces
    /// will carry sequence [`Self::FIRST_CLIENT_SEQUENCE`] (`1`).
    pub fn new() -> Self {
        ClientCoreApi {
            state: ConnectionState::Disconnected,
            next_client_sequence: Self::FIRST_CLIENT_SEQUENCE,
            latest_server_tick: Tick::ZERO,
            last_acked_client_sequence: 0,
            pending: Vec::new(),
        }
    }

    /// Begin connecting. Valid only from `Disconnected`; returns whether the
    /// transition happened (`false` if already connecting or connected).
    pub fn connect(&mut self) -> bool {
        let ok = self.state == ConnectionState::Disconnected;
        ok.then(|| self.state = ConnectionState::Connecting);
        ok
    }

    /// Apply a server `Welcome`, becoming `Connected` and seeding the latest
    /// authoritative tick. Valid only from `Connecting`; returns whether the
    /// transition happened (a `Welcome` outside `Connecting` is ignored).
    pub fn accept_welcome(&mut self, server_tick: u64) -> bool {
        let ok = self.state == ConnectionState::Connecting;
        ok.then(|| {
            self.state = ConnectionState::Connected;
            self.latest_server_tick = Tick::new(server_tick);
        });
        ok
    }

    /// Produce the next outbound `ClientIntent`'s fields, assigning the next
    /// monotonic `client_sequence` and recording it as pending. Returns the
    /// tuple `(client_sequence, predicted_client_tick, last_seen_server_tick,
    /// payload)` when `Connected`, or `None` otherwise — a client cannot send an
    /// intent while `Disconnected` or `Connecting`.
    pub fn next_intent(
        &mut self,
        predicted_client_tick: u64,
        last_seen_server_tick: u64,
        payload: &[u8],
    ) -> Option<(u64, u64, u64, Vec<u8>)> {
        (self.state == ConnectionState::Connected).then(|| {
            let sequence = self.next_client_sequence;
            self.next_client_sequence = self.next_client_sequence.saturating_add(1);
            self.pending.push(sequence);
            (
                sequence,
                predicted_client_tick,
                last_seen_server_tick,
                payload.to_vec(),
            )
        })
    }

    /// Apply an authoritative `ServerSnapshot`: advance the latest server tick
    /// and drop every pending intent whose sequence is `<=
    /// last_accepted_client_sequence`. Accepted only when `Connected` and the
    /// snapshot's tick is **not older** than the latest applied tick (an equal
    /// tick is allowed and idempotent). Returns whether it was applied; a
    /// snapshot before `Welcome`, or an older snapshot, returns `false`.
    pub fn accept_snapshot(
        &mut self,
        server_tick: u64,
        last_accepted_client_sequence: u64,
    ) -> bool {
        let tick = Tick::new(server_tick);
        let ok = (self.state == ConnectionState::Connected) & (tick >= self.latest_server_tick);
        ok.then(|| {
            self.latest_server_tick = tick;
            self.last_acked_client_sequence = last_accepted_client_sequence;
            self.pending
                .retain(|&sequence| sequence > last_accepted_client_sequence);
        });
        ok
    }

    /// Apply a `RejectedIntent`: drop exactly the pending intent with the named
    /// `client_sequence` (a no-op if it is not pending). Accepted only when
    /// `Connected`; a rejection before `Welcome` returns `false`.
    pub fn accept_rejected_intent(&mut self, client_sequence: u64) -> bool {
        let ok = self.state == ConnectionState::Connected;
        ok.then(|| self.pending.retain(|&sequence| sequence != client_sequence));
        ok
    }

    /// The connection status as a stable code (`STATUS_*`).
    pub fn status_code(&self) -> u8 {
        self.state.code()
    }

    /// Whether the client is disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.state == ConnectionState::Disconnected
    }

    /// Whether the client is connecting.
    pub fn is_connecting(&self) -> bool {
        self.state == ConnectionState::Connecting
    }

    /// Whether the client is connected.
    pub fn is_connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }

    /// The latest authoritative server tick the client has applied.
    pub fn latest_server_tick(&self) -> u64 {
        self.latest_server_tick.raw()
    }

    /// The sequence the next outbound intent will carry.
    pub fn next_client_sequence(&self) -> u64 {
        self.next_client_sequence
    }

    /// How many outbound intents are pending (sent, not yet acknowledged).
    pub fn pending_intent_count(&self) -> usize {
        self.pending.len()
    }

    /// The newest client sequence the server has acknowledged via a snapshot.
    pub fn last_acked_client_sequence(&self) -> u64 {
        self.last_acked_client_sequence
    }
}

impl Default for ClientCoreApi {
    fn default() -> Self {
        ClientCoreApi::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client driven all the way to `Connected`.
    fn connected() -> ClientCoreApi {
        let mut c = ClientCoreApi::new();
        c.connect();
        c.accept_welcome(0);
        c
    }

    #[test]
    fn initial_state_is_disconnected() {
        let c = ClientCoreApi::new();
        assert!(c.is_disconnected());
        assert_eq!(c.status_code(), ClientCoreApi::STATUS_DISCONNECTED);
        assert_eq!(c.next_client_sequence(), 1);
        assert_eq!(c.latest_server_tick(), 0);
        assert_eq!(c.pending_intent_count(), 0);
        assert_eq!(c.last_acked_client_sequence(), 0);
    }

    #[test]
    fn default_matches_new() {
        assert_eq!(
            ClientCoreApi::default().status_code(),
            ClientCoreApi::new().status_code()
        );
    }

    #[test]
    fn connecting_transition_works() {
        let mut c = ClientCoreApi::new();
        assert!(c.connect());
        assert!(c.is_connecting());
        assert_eq!(c.status_code(), ClientCoreApi::STATUS_CONNECTING);
        // A second connect from Connecting is rejected.
        assert!(!c.connect());
    }

    #[test]
    fn welcome_transitions_to_connected() {
        let mut c = ClientCoreApi::new();
        c.connect();
        assert!(c.accept_welcome(7));
        assert!(c.is_connected());
        assert_eq!(c.status_code(), ClientCoreApi::STATUS_CONNECTED);
        assert_eq!(c.latest_server_tick(), 7);
    }

    #[test]
    fn welcome_is_ignored_unless_connecting() {
        // From Disconnected: ignored.
        let mut c = ClientCoreApi::new();
        assert!(!c.accept_welcome(7));
        assert!(c.is_disconnected());
        // From Connected: ignored (a second welcome).
        let mut c = connected();
        assert!(!c.accept_welcome(99));
    }

    #[test]
    fn send_intent_fails_while_disconnected() {
        let mut c = ClientCoreApi::new();
        assert_eq!(c.next_intent(0, 0, b"x"), None);
        assert_eq!(c.pending_intent_count(), 0);
        assert_eq!(c.next_client_sequence(), 1);
    }

    #[test]
    fn send_intent_fails_while_connecting() {
        let mut c = ClientCoreApi::new();
        c.connect();
        assert_eq!(c.next_intent(0, 0, b"x"), None);
        assert_eq!(c.pending_intent_count(), 0);
    }

    #[test]
    fn send_intent_succeeds_while_connected() {
        let mut c = connected();
        let intent = c.next_intent(100, 98, b"move").unwrap();
        assert_eq!(intent, (1, 100, 98, b"move".to_vec()));
        assert_eq!(c.pending_intent_count(), 1);
    }

    #[test]
    fn sequence_starts_at_one_and_increments_deterministically() {
        let mut c = connected();
        assert_eq!(c.next_intent(0, 0, b"a").unwrap().0, 1);
        assert_eq!(c.next_intent(0, 0, b"b").unwrap().0, 2);
        assert_eq!(c.next_intent(0, 0, b"c").unwrap().0, 3);
        assert_eq!(c.next_client_sequence(), 4);
        assert_eq!(c.pending_intent_count(), 3);
    }

    #[test]
    fn snapshot_ack_drains_pending_up_to_last_accepted() {
        let mut c = connected();
        (0..5).for_each(|_| {
            c.next_intent(0, 0, b"").unwrap();
        });
        assert_eq!(c.pending_intent_count(), 5); // sequences 1..=5
        assert!(c.accept_snapshot(10, 3));
        // Sequences 1,2,3 acknowledged; 4 and 5 remain.
        assert_eq!(c.pending_intent_count(), 2);
        assert_eq!(c.last_acked_client_sequence(), 3);
        assert_eq!(c.latest_server_tick(), 10);
    }

    #[test]
    fn snapshot_preserves_pending_insertion_order() {
        let mut c = connected();
        (0..4).for_each(|_| {
            c.next_intent(0, 0, b"").unwrap();
        });
        // Ack the first two; the remaining must still be 3 then 4.
        c.accept_snapshot(1, 2);
        // Reject the now-front intent (3); only 4 must remain.
        assert!(c.accept_rejected_intent(3));
        assert_eq!(c.pending_intent_count(), 1);
        // Acking up to 4 clears it.
        c.accept_snapshot(2, 4);
        assert_eq!(c.pending_intent_count(), 0);
    }

    #[test]
    fn rejected_intent_removes_exactly_that_sequence() {
        let mut c = connected();
        (0..3).for_each(|_| {
            c.next_intent(0, 0, b"").unwrap();
        }); // 1,2,3
        assert!(c.accept_rejected_intent(2));
        assert_eq!(c.pending_intent_count(), 2); // 1 and 3 remain
                                                 // Rejecting an absent sequence is a harmless no-op (still Connected).
        assert!(c.accept_rejected_intent(99));
        assert_eq!(c.pending_intent_count(), 2);
    }

    #[test]
    fn snapshot_before_welcome_is_rejected() {
        let mut c = ClientCoreApi::new();
        assert!(!c.accept_snapshot(5, 0));
        assert_eq!(c.latest_server_tick(), 0);
        c.connect();
        assert!(!c.accept_snapshot(5, 0)); // still not Connected
    }

    #[test]
    fn rejected_intent_before_welcome_is_rejected() {
        let mut c = ClientCoreApi::new();
        assert!(!c.accept_rejected_intent(1));
        c.connect();
        assert!(!c.accept_rejected_intent(1));
    }

    #[test]
    fn older_snapshots_are_rejected_equal_is_allowed() {
        let mut c = connected();
        assert!(c.accept_snapshot(10, 0));
        // Older tick: rejected, state unchanged.
        assert!(!c.accept_snapshot(9, 0));
        assert_eq!(c.latest_server_tick(), 10);
        // Equal tick: allowed and idempotent.
        assert!(c.accept_snapshot(10, 0));
        assert_eq!(c.latest_server_tick(), 10);
    }

    #[test]
    fn latest_server_tick_updates_from_snapshots() {
        let mut c = connected();
        c.accept_snapshot(3, 0);
        assert_eq!(c.latest_server_tick(), 3);
        c.accept_snapshot(8, 0);
        assert_eq!(c.latest_server_tick(), 8);
    }

    #[test]
    fn the_whole_flow_is_deterministic_when_replayed() {
        // Two independent clients fed identical inputs reach identical state.
        let drive = || {
            let mut c = ClientCoreApi::new();
            c.connect();
            c.accept_welcome(0);
            c.next_intent(1, 0, b"a");
            c.next_intent(2, 0, b"b");
            c.accept_snapshot(5, 1);
            c.accept_rejected_intent(2);
            (
                c.status_code(),
                c.next_client_sequence(),
                c.latest_server_tick(),
                c.pending_intent_count(),
                c.last_acked_client_sequence(),
            )
        };
        assert_eq!(drive(), drive());
    }
}
