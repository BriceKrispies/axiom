//! The safe authoritative simulation session.
//!
//! [`Session`] embeds a headless [`RunningApp`] and is the single source of
//! authoritative truth: its state **is** the engine's durable scene state
//! (`snapshot_sim()` / `restore_sim()`), never a parallel mirror. It validates
//! player intents, queues the accepted ones, applies them in a deterministic
//! order each tick, and captures a replay record. The `extern "C"` layer in
//! `ffi.rs` is a thin panic-guarded wrapper over this type, which has ordinary
//! unit tests.

use axiom::prelude::{App, Player, PlayerInput, RunningApp, Transform, Vec3};
use axiom_kernel::{DeterministicRng, StableHash};
use axiom_net_protocol::NetProtocolApi;

use crate::replay::{AcceptedIntent, ReplayRecord, TickRecord};
use crate::ruleset;
use crate::status::{
    REASON_DUPLICATE_SEQUENCE, REASON_INVALID_PLAYER, REASON_NONE, REASON_OUT_OF_ORDER,
    REASON_PAYLOAD_TOO_LARGE, REASON_RATE_LIMITED,
};

/// The largest `max_players` a sim may be created with — a sanity bound so a
/// bogus host argument cannot request an enormous allocation.
pub const MAX_PLAYERS_CAP: u32 = 256;

/// The most accepted intents the worker queues for one player in a single tick.
/// Beyond this, further intents are rate-limited (rejected), bounding per-tick
/// work and blunting action-spam.
const MAX_INTENTS_PER_PLAYER_PER_TICK: usize = 8;

/// The authoritative session: the engine, the per-player sequence cursors, the
/// pending accepted-intent queue, the replay history, and a last-error slot.
#[derive(Debug)]
pub struct Session {
    app: RunningApp,
    seed: u64,
    max_players: u32,
    fixed_step_ns: u64,
    tick: u64,
    /// The last accepted client sequence per player slot (`None` = none yet).
    last_accepted_seq: Vec<Option<u64>>,
    /// Accepted intents awaiting the next [`Self::advance`].
    pending: Vec<AcceptedIntent>,
    /// Intents rejected since the last advance (recorded with the next tick).
    rejected_since_advance: u32,
    /// The per-tick replay history.
    history: Vec<TickRecord>,
    /// The initial state hash (captured at construction), for replay identity.
    initial_hash: u64,
    /// The host's deterministic random source, seeded from `seed`. Carried inside
    /// the full session snapshot ([`Self::snapshot_session`]) so a restored or
    /// recovered worker continues the identical random sequence rather than
    /// diverging. (The cube ruleset draws none today; the field future-proofs an
    /// rng-using ruleset and proves the snapshot path carries it.)
    rng: DeterministicRng,
    /// The most recent error `(code, message)`, surfaced via the C ABI.
    last_error: Option<(u32, String)>,
}

impl Session {
    /// Build a fresh authoritative session: a headless engine with `max_players`
    /// player nodes laid out deterministically (centered on the origin).
    pub fn new(seed: u64, max_players: u32, fixed_step_ns: u64) -> Self {
        let app = build_headless(max_players, fixed_step_ns);
        let initial_hash = StableHash::of_bytes(&app.snapshot_sim()).raw();
        Session {
            app,
            seed,
            max_players,
            fixed_step_ns,
            tick: 0,
            last_accepted_seq: vec![None; max_players as usize],
            pending: Vec::new(),
            rejected_since_advance: 0,
            history: Vec::new(),
            initial_hash,
            rng: DeterministicRng::seeded(seed),
            last_error: None,
        }
    }

    /// The number of ticks advanced so far (the authoritative tick count).
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The authoritative snapshot bytes — the engine's durable scene state.
    pub fn snapshot(&self) -> Vec<u8> {
        self.app.snapshot_sim()
    }

    /// The per-tick state hash over canonical snapshot bytes. A diagnostic
    /// locator; byte-equality of [`Self::snapshot`] is the determinism proof.
    pub fn state_hash(&self) -> u64 {
        StableHash::of_bytes(&self.snapshot()).raw()
    }

    /// The authoritative renderable view: each player's `(x, y)` world position,
    /// read directly from the engine — a read-only projection of authoritative
    /// state, never a parallel mirror. `2 * max_players` floats laid out
    /// `[p0x, p0y, p1x, p1y, …]`, which an authoritative host broadcasts so a
    /// browser client can render and reconcile against authoritative positions.
    pub fn render_view(&self) -> Vec<f32> {
        (0..self.max_players)
            .flat_map(|index| {
                let t = self.app.player_translation(index).unwrap_or(Vec3::ZERO);
                [t.x, t.y]
            })
            .collect()
    }

    /// Restore authoritative state from snapshot bytes. Returns `false` and
    /// records the error on a truncated / incompatible buffer (no panic).
    pub fn restore(&mut self, bytes: &[u8]) -> bool {
        self.app
            .restore_sim(bytes)
            .map(|()| true)
            .unwrap_or_else(|_| {
                self.record_error(
                    crate::status::STATUS_ERR_DESERIALIZE as u32,
                    "restore_sim rejected the snapshot bytes",
                );
                false
            })
    }

    /// Restore authoritative state AND re-establish the tick counter — used for
    /// crash recovery, where the host respawns a worker and resumes it at the tick
    /// it had reached. The engine snapshot carries only scene state, not the
    /// worker's tick (which the host owns), so the host supplies it here; without
    /// it a respawned worker would tick from zero and diverge. Only sets the tick
    /// when the restore succeeds.
    pub fn restore_at(&mut self, tick: u64, bytes: &[u8]) -> bool {
        let ok = self.restore(bytes);
        self.tick = [self.tick, tick][usize::from(ok)];
        ok
    }

    /// The **full session** snapshot — the durable sim state AND the host RNG —
    /// as one opaque, versioned blob the embedding host stores verbatim for
    /// persistence, rewind, or crash recovery. The RNG lives inside the blob, so a
    /// restored worker continues the identical random sequence. Distinct from
    /// [`Self::snapshot`] (the scene-only bytes the per-tick replay/hash machinery
    /// uses).
    pub fn snapshot_session(&self) -> Vec<u8> {
        self.app.snapshot_session(&self.rng)
    }

    /// Restore a full session from [`Self::snapshot_session`] bytes — forking the
    /// sim AND resuming the captured RNG. Returns `false` and records the error on
    /// a truncated / incompatible buffer (no panic).
    pub fn restore_session(&mut self, bytes: &[u8]) -> bool {
        self.app
            .restore_session(bytes)
            .map(|rng| {
                self.rng = rng;
                true
            })
            .unwrap_or_else(|_| {
                self.record_error(
                    crate::status::STATUS_ERR_DESERIALIZE as u32,
                    "restore_session rejected the snapshot bytes",
                );
                false
            })
    }

    /// Validate and (if accepted) queue a player intent for the next tick.
    /// Returns [`REASON_NONE`] on accept, otherwise the rejection reason. The
    /// `player_id` is the slot the **.NET host assigned** — it is never read from
    /// the client wire, so a client cannot address another player.
    pub fn submit_intent(
        &mut self,
        player_id: u32,
        client_sequence: u64,
        predicted_client_tick: u64,
        payload: &[u8],
    ) -> u32 {
        let reason = self.classify_intent(player_id, client_sequence, payload);
        if reason != REASON_NONE {
            self.rejected_since_advance += 1;
            return reason;
        }
        // Accept: record the per-player cursor and queue the intent for the next tick.
        self.last_accepted_seq[player_id as usize] = Some(client_sequence);
        self.pending.push(AcceptedIntent {
            player_id,
            client_sequence,
            predicted_client_tick,
            payload: payload.to_vec(),
        });
        REASON_NONE
    }

    /// Pure validation: decide the reason an intent would be rejected, or
    /// [`REASON_NONE`] if it is acceptable. No state change.
    fn classify_intent(&self, player_id: u32, client_sequence: u64, payload: &[u8]) -> u32 {
        // 1. The slot must be in range (host-assigned, but defend anyway).
        let in_range = player_id < self.max_players;
        // 2. Bound the opaque payload to the protocol's maximum.
        let size_ok = payload.len() <= NetProtocolApi::MAX_PAYLOAD_LEN;
        // 3. Per-player sequence monotonicity (only meaningful for an in-range slot).
        let seq_reason = match in_range
            .then(|| self.last_accepted_seq[player_id as usize])
            .flatten()
        {
            Some(last) if client_sequence == last => REASON_DUPLICATE_SEQUENCE,
            Some(last) if client_sequence < last => REASON_OUT_OF_ORDER,
            _ => REASON_NONE,
        };
        // 4. Ruleset: payload decodes to a legal move.
        let rule_reason = ruleset::decode_move(payload).err().unwrap_or(REASON_NONE);
        // 5. Per-player per-tick rate limit.
        let rate_ok = self
            .pending
            .iter()
            .filter(|a| a.player_id == player_id)
            .count()
            < MAX_INTENTS_PER_PLAYER_PER_TICK;

        // Resolve in priority order: structural problems first, then sequence,
        // then ruleset, then rate. A simple precedence chain (an app, so a plain
        // match is fine here).
        match (in_range, size_ok, seq_reason, rule_reason, rate_ok) {
            (false, _, _, _, _) => REASON_INVALID_PLAYER,
            (_, false, _, _, _) => REASON_PAYLOAD_TOO_LARGE,
            (_, _, r, _, _) if r != REASON_NONE => r,
            (_, _, _, r, _) if r != REASON_NONE => r,
            (_, _, _, _, false) => REASON_RATE_LIMITED,
            _ => REASON_NONE,
        }
    }

    /// Apply the pending accepted intents in deterministic order and advance one
    /// fixed tick. Captures a [`TickRecord`]. Returns `(new tick count, new state
    /// hash)`.
    pub fn advance(&mut self) -> (u64, u64) {
        let prev_hash = self.state_hash();

        // Deterministic application order: by player id, then client sequence.
        // Never hash-map order.
        let mut batch = std::mem::take(&mut self.pending);
        batch.sort_by_key(|a| (a.player_id, a.client_sequence));

        // Collapse each player's accepted intents this tick into one net delta
        // (summed in the deterministic sorted order above), then clamp the
        // *resulting* position to the authoritative field bound. The bound is a
        // server-side rule: it is enforced against the engine's current
        // authoritative position (read back, never mirrored), so a client cannot
        // cross the wall by holding a key — the worker trims the applied delta.
        let mut net: std::collections::BTreeMap<u32, (f32, f32)> =
            std::collections::BTreeMap::new();
        for a in &batch {
            if let Ok((dx, dy)) = ruleset::decode_move(&a.payload) {
                let acc = net.entry(a.player_id).or_insert((0.0, 0.0));
                acc.0 += dx;
                acc.1 += dy;
            }
        }
        let inputs: Vec<PlayerInput> = net
            .iter()
            .map(|(&player_id, &(dx, dy))| {
                let pos = self.app.player_translation(player_id).unwrap_or(Vec3::ZERO);
                let effective_dx = ruleset::clamp_axis(pos.x, dx);
                let effective_dy = ruleset::clamp_axis(pos.y, dy);
                ruleset::player_move(player_id, effective_dx, effective_dy)
            })
            .collect();

        self.app.tick_with(self.tick, &inputs);
        let new_hash = self.state_hash();

        self.history.push(TickRecord {
            tick: self.tick,
            prev_hash,
            new_hash,
            rejected_count: self.rejected_since_advance,
            accepted: batch,
        });
        self.tick += 1;
        self.rejected_since_advance = 0;
        (self.tick, new_hash)
    }

    /// The replay record for the run so far.
    pub fn replay_record(&self) -> ReplayRecord {
        ReplayRecord {
            seed: self.seed,
            max_players: self.max_players,
            fixed_step_ns: self.fixed_step_ns,
            initial_hash: self.initial_hash,
            ticks: self.history.clone(),
        }
    }

    /// The replay record's canonical bytes.
    pub fn export_replay(&self) -> Vec<u8> {
        self.replay_record().encode()
    }

    /// Record an error to be surfaced via the C ABI's last-error functions.
    pub fn record_error(&mut self, code: u32, message: &str) {
        self.last_error = Some((code, message.to_string()));
    }

    /// The last error code (`0` if none).
    pub fn last_error_code(&self) -> u32 {
        self.last_error.as_ref().map(|(c, _)| *c).unwrap_or(0)
    }

    /// The last error message (`""` if none).
    pub fn last_error_message(&self) -> &str {
        self.last_error
            .as_ref()
            .map(|(_, m)| m.as_str())
            .unwrap_or("")
    }
}

/// Build the headless authoritative engine: no rendering, no window — just the
/// scene with `max_players` player nodes. Movement and snapshotting are scene
/// concerns, so this needs no GPU.
fn build_headless(max_players: u32, fixed_step_ns: u64) -> RunningApp {
    App::new()
        .fixed_timestep_nanos(fixed_step_ns.max(1))
        .setup(move |world, _meshes, _materials| {
            (0..max_players).for_each(|index| {
                world.spawn((
                    Transform::from_translation(spawn_pos(index, max_players)),
                    Player::new(index),
                ));
            });
        })
        .build()
}

/// A deterministic, finite initial position: players sit on a line centered on
/// the origin, 3.0 units apart (player 0 left, player 1 right). This matches the
/// browser client's initial render positions so client prediction lines up with
/// the authoritative spawn.
fn spawn_pos(index: u32, max_players: u32) -> Vec3 {
    let centered = index as f32 - (max_players.saturating_sub(1) as f32) / 2.0;
    Vec3::new(centered * 3.0, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{REASON_IMPOSSIBLE_MOVEMENT, REASON_MALFORMED};

    fn session() -> Session {
        Session::new(7, 2, 16_666_667)
    }

    fn move_payload(dx: f32, dy: f32) -> Vec<u8> {
        ruleset::encode_move(dx, dy)
    }

    #[test]
    fn a_fresh_session_starts_at_tick_zero() {
        let s = session();
        assert_eq!(s.tick(), 0);
        assert!(!s.snapshot().is_empty());
    }

    #[test]
    fn render_view_is_the_authoritative_player_positions() {
        let mut s = session();
        // Players spawn centered: p0 left at x=-1.5, p1 right at x=+1.5.
        assert_eq!(s.render_view(), vec![-1.5, 0.0, 1.5, 0.0]);
        s.submit_intent(0, 1, 0, &move_payload(0.5, 0.0));
        s.advance();
        // The authoritative view tracks the engine's moved player.
        assert_eq!(s.render_view(), vec![-1.0, 0.0, 1.5, 0.0]);
    }

    #[test]
    fn authoritative_position_is_clamped_to_the_field_bound() {
        let mut s = session();
        // Player 0 spawns at x=-1.5; shove right at the max delta far past the
        // wall over many ticks. The authoritative position must rest *at* the
        // field bound, never beyond it — the server is the wall.
        (1..=20u64).for_each(|seq| {
            s.submit_intent(0, seq, 0, &move_payload(1.0, 0.0));
            s.advance();
        });
        let x = s.render_view()[0];
        assert!(x <= ruleset::FIELD_BOUND, "x={x} escaped the field bound");
        assert!(
            (x - ruleset::FIELD_BOUND).abs() < 1e-3,
            "x={x} should rest against the wall"
        );
        // Player 1 (untouched) is still at its spawn — the clamp is per-player.
        assert_eq!(s.render_view()[2], 1.5);
    }

    #[test]
    fn same_seed_same_inputs_same_hashes() {
        let mut a = session();
        let mut b = session();
        assert_eq!(a.state_hash(), b.state_hash());
        a.submit_intent(0, 1, 0, &move_payload(0.5, 0.0));
        b.submit_intent(0, 1, 0, &move_payload(0.5, 0.0));
        assert_eq!(a.advance(), b.advance());
    }

    #[test]
    fn different_inputs_different_hashes_when_game_state_changes() {
        let mut moved = session();
        let mut still = session();
        moved.submit_intent(0, 1, 0, &move_payload(0.5, 0.0));
        let (_, moved_hash) = moved.advance();
        let (_, still_hash) = still.advance();
        assert_ne!(moved_hash, still_hash);
    }

    #[test]
    fn advancing_an_empty_tick_preserves_the_hash_deterministically() {
        let mut s = session();
        // The first advance runs the (empty) startup phase, which flips the
        // world's `startup_done` bit — genuine state, now carried in the snapshot
        // and therefore the hash. Measure across a *post-startup* empty tick, where
        // nothing changes.
        s.advance();
        let before = s.state_hash();
        let (_, after) = s.advance();
        assert_eq!(before, after);
    }

    #[test]
    fn snapshot_restore_round_trips() {
        let mut s = session();
        s.submit_intent(1, 1, 0, &move_payload(0.0, 0.5));
        s.advance();
        let bytes = s.snapshot();
        let hash = s.state_hash();

        let mut fresh = session();
        assert!(fresh.restore(&bytes));
        assert_eq!(fresh.snapshot(), bytes);
        assert_eq!(fresh.state_hash(), hash);
    }

    #[test]
    fn session_snapshot_carries_and_resumes_the_rng() {
        let mut s = session();
        s.submit_intent(1, 1, 0, &move_payload(0.0, 0.5));
        s.advance();
        // Advance the host RNG, then bundle the full session (sim + rng).
        (0..5).for_each(|_| {
            s.rng.next_u64();
        });
        let blob = s.snapshot_session();

        // Restore into a session built with a DIFFERENT seed: if the blob did not
        // carry the rng, the continuation would diverge — it does not.
        let mut fresh = Session::new(0x9999, 2, 16_666_667);
        assert!(fresh.restore_session(&blob));
        let original: Vec<u64> = (0..8).map(|_| s.rng.next_u64()).collect();
        let replayed: Vec<u64> = (0..8).map(|_| fresh.rng.next_u64()).collect();
        assert_eq!(original, replayed, "the session blob carries and resumes the rng");

        // A bad buffer is rejected (recorded), not a panic.
        assert!(!fresh.restore_session(&[1, 2, 3]));
    }

    #[test]
    fn restore_then_advance_matches_the_original_path() {
        let mut original = session();
        original.submit_intent(0, 1, 0, &move_payload(0.3, 0.0));
        original.advance();
        let mid = original.snapshot();
        original.submit_intent(0, 2, 0, &move_payload(0.2, 0.0));
        let (_, original_end) = original.advance();

        let mut forked = session();
        assert!(forked.restore(&mid));
        forked.submit_intent(0, 2, 0, &move_payload(0.2, 0.0));
        let (_, forked_end) = forked.advance();
        assert_eq!(original_end, forked_end);
    }

    #[test]
    fn first_sequence_is_accepted() {
        let mut s = session();
        assert_eq!(
            s.submit_intent(0, 1, 0, &move_payload(0.1, 0.0)),
            REASON_NONE
        );
    }

    #[test]
    fn duplicate_sequence_is_rejected_and_state_unchanged() {
        let mut s = session();
        s.submit_intent(0, 5, 0, &move_payload(0.1, 0.0));
        s.advance();
        let before = s.state_hash();
        assert_eq!(
            s.submit_intent(0, 5, 0, &move_payload(0.1, 0.0)),
            REASON_DUPLICATE_SEQUENCE
        );
        let (_, after) = s.advance();
        assert_eq!(before, after);
    }

    #[test]
    fn older_sequence_is_rejected_and_state_unchanged() {
        let mut s = session();
        s.submit_intent(0, 5, 0, &move_payload(0.1, 0.0));
        s.advance();
        let before = s.state_hash();
        assert_eq!(
            s.submit_intent(0, 4, 0, &move_payload(0.1, 0.0)),
            REASON_OUT_OF_ORDER
        );
        let (_, after) = s.advance();
        assert_eq!(before, after);
    }

    #[test]
    fn malformed_payload_is_rejected() {
        let mut s = session();
        assert_eq!(s.submit_intent(0, 1, 0, &[1, 2, 3]), REASON_MALFORMED);
    }

    #[test]
    fn impossible_movement_is_rejected() {
        let mut s = session();
        assert_eq!(
            s.submit_intent(0, 1, 0, &move_payload(99.0, 0.0)),
            REASON_IMPOSSIBLE_MOVEMENT
        );
    }

    #[test]
    fn invalid_player_is_rejected() {
        let mut s = session();
        assert_eq!(
            s.submit_intent(99, 1, 0, &move_payload(0.1, 0.0)),
            REASON_INVALID_PLAYER
        );
    }

    #[test]
    fn too_many_intents_per_tick_is_rate_limited() {
        let mut s = session();
        // The first MAX_INTENTS_PER_PLAYER_PER_TICK accept; the next is limited.
        (1..=MAX_INTENTS_PER_PLAYER_PER_TICK as u64).for_each(|seq| {
            assert_eq!(
                s.submit_intent(0, seq, 0, &move_payload(0.01, 0.0)),
                REASON_NONE
            );
        });
        let next = MAX_INTENTS_PER_PLAYER_PER_TICK as u64 + 1;
        assert_eq!(
            s.submit_intent(0, next, 0, &move_payload(0.01, 0.0)),
            REASON_RATE_LIMITED
        );
    }

    #[test]
    fn oversized_payload_is_rejected() {
        let mut s = session();
        let big = vec![0u8; NetProtocolApi::MAX_PAYLOAD_LEN + 1];
        assert_eq!(s.submit_intent(0, 1, 0, &big), REASON_PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn accepted_inputs_apply_in_deterministic_order() {
        // Submit with players interleaved (each player's sequences stay
        // monotonic, so none is rejected); the applied result must be independent
        // of arrival order, matching a session that received them already sorted.
        let mut shuffled = session();
        shuffled.submit_intent(1, 1, 0, &move_payload(0.1, 0.0));
        shuffled.submit_intent(0, 1, 0, &move_payload(0.2, 0.0));
        shuffled.submit_intent(1, 2, 0, &move_payload(0.1, 0.0));
        let (_, shuffled_hash) = shuffled.advance();

        let mut ordered = session();
        ordered.submit_intent(0, 1, 0, &move_payload(0.2, 0.0));
        ordered.submit_intent(1, 1, 0, &move_payload(0.1, 0.0));
        ordered.submit_intent(1, 2, 0, &move_payload(0.1, 0.0));
        let (_, ordered_hash) = ordered.advance();

        assert_eq!(shuffled_hash, ordered_hash);
    }

    #[test]
    fn repeated_same_input_stream_produces_same_hashes() {
        let run = || {
            let mut s = session();
            let mut hashes = Vec::new();
            (1..=5u64).for_each(|seq| {
                s.submit_intent(0, seq, 0, &move_payload(0.05, 0.0));
                hashes.push(s.advance().1);
            });
            hashes
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn last_error_is_recorded_on_bad_restore() {
        let mut s = session();
        assert!(!s.restore(&[9, 9, 9]));
        assert_ne!(s.last_error_code(), 0);
        assert!(!s.last_error_message().is_empty());
    }
}
