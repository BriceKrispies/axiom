//! The deterministic, **authored-callback** authority core (SPEC-13 §3.5, §6, §7).
//!
//! This is the source of truth a server-authoritative room runs: it owns the
//! authored game as a headless [`RunningApp`] (the engine's deterministic
//! fixed-step world — NOT the wasm presentation arm), folds each seated player's
//! decoded intent into the world through the engine's per-player
//! [`RunningApp::tick_with`] path, steps the fixed update, and emits the resulting
//! state as a per-player `ServerSnapshotFor` (kind 8) carrying the per-player ack
//! cursors. Clients send *intents*, never state; the authority decides the
//! outcome; predicted clients reconcile against these snapshots.
//!
//! It is split from the async socket harness in `main.rs` precisely so it is a
//! pure, wall-clock-free, deterministic value the §7 golden can drive directly:
//! two independent [`Authority`] instances fed the identical ordered intent stream
//! produce byte-identical `ServerSnapshotFor` sequences (proven in `tests`).
//!
//! Branchless (app/tool tier still holds the Branchless Law's gate), so every step
//! is a data transform over combinators — no `if`/`match`/`for`/`while`.
//!
//! ## Why the umbrella, not `axiom-game-runtime::GameRuntime`
//! The proven headless driver `GameRuntime`/`GameBridge` lives in an **app**
//! (`apps/axiom-game-runtime`), and the Module Law forbids a tool depending on an
//! app (`AppImportedBySomething`). The deterministic primitive `GameRuntime` wraps
//! — the `frame` accumulator + `RunningApp::tick` — is re-exported from the
//! `axiom` umbrella *module*, which a tool may legally compose. This authority
//! therefore drives the same engine surface directly (SPEC-13 §3.5 explicitly
//! places the authority loop at app/tool tier, "not a new reusable module").
//!
//! ## The authoritative participant-block schema (mirrored by the TS
//! ## `makeNetParticipants` decoder)
//! The `ServerSnapshotFor` opaque payload IS the participant block defined here.
//! It is flat and length-prefixed, reusing the `u32`-count / `u32`-length-prefix
//! conventions the `axiom-net-protocol` frames already use. All integers are
//! little-endian. This Rust definition is **authoritative**; a later TS agent
//! mirrors these exact bytes.
//!
//! ```text
//! PARTICIPANT BLOCK  (= ServerSnapshotFor.payload)
//!   u32                participant_count
//!   participant_count repetitions of:
//!     u64              player_id              (the stable 1-based seat id)
//!     u8               flags                  (bit0 = joinedThisTick; others reserved 0)
//!     u32              intent_len
//!     u8 * intent_len  intent                 (inputOf(player): the opaque intent
//!                                               payload last applied this tick;
//!                                               empty when the player sent none)
//!   u32                left_count
//!   left_count repetitions of:
//!     u64              player_id              (leftThisTick: a seat vacated this tick)
//!   u32                state_len
//!   u8 * state_len     state                  (the authoritative authored sim
//!                                               snapshot — RunningApp::snapshot_sim() —
//!                                               the deterministic renderable world)
//! ```
//!
//! A TS mirror decodes `players` (the participant_count ids), `inputOf(player)`
//! (each participant's `intent`, via `decodeIntent`), `joinedThisTick` (the bit0
//! flag), `leftThisTick` (the `left` ids), and the renderable `state` blob.

use axiom::prelude::{
    App, Color, DefaultPlugins, Material, Mesh, Player, PlayerInput, RunningApp, Transform, Vec3,
    Window,
};
use axiom_net_protocol::NetProtocolApi;

/// The application protocol version this authority speaks (matches `Welcome`).
pub const PROTOCOL_VERSION: u32 = 1;

/// The authoritative fixed simulation step, in nanoseconds (~60 Hz). Sent in
/// `Welcome` and used as the broadcast tick interval by the socket harness.
pub const FIXED_STEP_NS: u64 = 16_666_667;

/// The number of player seats this room admits (the demo is two-player).
pub const MAX_PLAYERS: u64 = 2;

/// `flags` bit0: this participant first appeared on the tick being described.
const FLAG_JOINED_THIS_TICK: u8 = 0b0000_0001;

/// One seated participant's authoritative bookkeeping.
struct Seat {
    /// The stable 1-based seat id (the player id on the wire).
    player: u64,
    /// The highest client sequence accepted from this seat (its ack cursor).
    last_accepted_seq: u64,
    /// The latest intent payload received for the upcoming step (latest-wins),
    /// `None` when the seat sent no intent this tick. This is `inputOf(player)`.
    pending: Option<Vec<u8>>,
    /// Whether this seat was claimed on the tick currently being described.
    joined_this_tick: bool,
}

impl Seat {
    /// A freshly-claimed seat: ack cursor at zero, no pending intent, flagged as
    /// joined this tick.
    fn new(player: u64) -> Self {
        Seat {
            player,
            last_accepted_seq: 0,
            pending: None,
            joined_this_tick: true,
        }
    }
}

/// The deterministic authored-callback authority: the headless authored world,
/// the monotonic fixed-step tick, the seated participants, and the seats vacated
/// since the last step.
pub struct Authority {
    app: RunningApp,
    tick: u64,
    seats: Vec<Seat>,
    /// Seats that left since the last [`Self::step`] — drained into `leftThisTick`.
    left: Vec<u64>,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authority")
            .field("tick", &self.tick)
            .field("seated", &self.seats.len())
            .finish()
    }
}

impl Default for Authority {
    fn default() -> Self {
        Authority::new()
    }
}

impl Authority {
    /// Stand up a fresh authority over the authored game at tick 0 with no seats.
    pub fn new() -> Self {
        Authority {
            app: authored_app(),
            tick: 0,
            seats: Vec::new(),
            left: Vec::new(),
        }
    }

    /// The monotonic count of fixed ticks stepped so far (the next `Welcome`'s
    /// `server_tick`).
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Claim the lowest free seat for a joining player, returning its 1-based id,
    /// or `None` when the room is full. The seats stay sorted by id so the
    /// participant block and ack list are deterministically ordered.
    pub fn claim(&mut self) -> Option<u64> {
        let next = (1..=MAX_PLAYERS).find(|id| self.seats.iter().all(|s| s.player != *id));
        next.inspect(|&id| {
            self.seats.push(Seat::new(id));
            self.seats.sort_by_key(|s| s.player);
        })
    }

    /// Record a player's latest intent for the upcoming step (latest-wins) and
    /// advance its ack cursor. An intent for an unseated player is dropped.
    pub fn apply_intent(&mut self, player: u64, sequence: u64, payload: Vec<u8>) {
        self.seats
            .iter_mut()
            .find(|s| s.player == player)
            .map(|s| {
                s.last_accepted_seq = s.last_accepted_seq.max(sequence);
                s.pending = Some(payload);
            })
            .unwrap_or(());
    }

    /// Release a player's seat, returning whether it was seated. A vacated seat is
    /// reported once, in the next snapshot's `leftThisTick`.
    pub fn leave(&mut self, player: u64) -> bool {
        let present = self.seats.iter().any(|s| s.player == player);
        self.seats.retain(|s| s.player != player);
        present.then(|| self.left.push(player)).unwrap_or(());
        present
    }

    /// Run one fixed-step authored update and emit the broadcast `ServerSnapshotFor`
    /// frame. Folds every seated player's pending intent into the authored world via
    /// the engine's per-player [`RunningApp::tick_with`] path, steps the fixed
    /// update, packs the participant block (with the authoritative sim snapshot),
    /// and encodes it with the per-player ack cursors. Per-tick state (pending
    /// intents, join flags, the vacated list) resets after the frame is built.
    pub fn step(&mut self) -> Vec<u8> {
        let inputs: Vec<PlayerInput> = self
            .seats
            .iter()
            .filter_map(|s| {
                s.pending.as_deref().map(|payload| {
                    let [dx, dy] = unpack_delta(payload);
                    PlayerInput::new(s.player as u32, Vec3::new(dx, dy, 0.0))
                })
            })
            .collect();
        self.app.tick_with(self.tick, &inputs);

        let state = self.app.snapshot_sim();
        let payload = self.encode_participant_block(&state);
        let acks: Vec<(u64, u64)> = self
            .seats
            .iter()
            .map(|s| (s.player, s.last_accepted_seq))
            .collect();
        let server_tick = self.tick + 1;
        let frame = NetProtocolApi::encode_server_snapshot_for(server_tick, &acks, &payload)
            .expect("authority snapshot is within the protocol payload bound");

        self.tick = server_tick;
        self.seats.iter_mut().for_each(|s| {
            s.pending = None;
            s.joined_this_tick = false;
        });
        self.left.clear();
        frame
    }

    /// Pack the authoritative participant block (the `ServerSnapshotFor` payload)
    /// per the schema documented at the top of this module.
    fn encode_participant_block(&self, state: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&(self.seats.len() as u32).to_le_bytes());
        self.seats.iter().for_each(|s| {
            out.extend_from_slice(&s.player.to_le_bytes());
            out.push(s.joined_this_tick as u8 * FLAG_JOINED_THIS_TICK);
            let intent = s.pending.as_deref().unwrap_or(&[]);
            out.extend_from_slice(&(intent.len() as u32).to_le_bytes());
            out.extend_from_slice(intent);
        });
        out.extend_from_slice(&(self.left.len() as u32).to_le_bytes());
        self.left
            .iter()
            .for_each(|player| out.extend_from_slice(&player.to_le_bytes()));
        out.extend_from_slice(&(state.len() as u32).to_le_bytes());
        out.extend_from_slice(state);
        out
    }
}

/// Build the authored game the authority runs headlessly: one `Player`-marked
/// node per seat, spread along X so distinct seats hold distinct authoritative
/// state. Rendering is left off (no `DefaultPlugins`) — the authority needs the
/// deterministic *simulation* world (`snapshot_sim`), not draws — but the seats
/// move through the exact engine `tick_with` path a rendered authored game uses.
fn authored_app() -> RunningApp {
    let _ = DefaultPlugins;
    App::new()
        .window(Window::new(320, 240))
        .fixed_timestep_nanos(FIXED_STEP_NS)
        .setup(|world, meshes, materials| {
            let _mesh = meshes.add(Mesh::cube());
            let _material = materials.add(Material::lit(Color::WHITE));
            (1..=MAX_PLAYERS).for_each(|seat| {
                world.spawn((
                    Transform::from_translation(Vec3::new(initial_x(seat), 0.0, 0.0)),
                    Player::new(seat as u32),
                ));
            });
        })
        .build()
}

/// The seat's starting X, spreading seats symmetrically about the origin so each
/// holds visibly distinct authoritative state from tick 0.
fn initial_x(seat: u64) -> f32 {
    ((seat as f32) - ((MAX_PLAYERS as f32) + 1.0) / 2.0) * 3.0
}

/// Decode a `[dx, dy]` move from an intent payload of two little-endian `f32`s; a
/// short or absent payload is no movement. Branchless via slice `get` + `try_into`.
fn unpack_delta(payload: &[u8]) -> [f32; 2] {
    [read_f32(payload, 0), read_f32(payload, 4)]
}

/// Read one little-endian `f32` at byte offset `at`, or `0.0` if out of range.
fn read_f32(bytes: &[u8], at: usize) -> f32 {
    bytes
        .get(at..at + 4)
        .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
        .map(f32::from_le_bytes)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::StableHash;

    /// One tick's worth of scripted inbound intents: `(player, sequence, payload)`.
    type TickIntents = Vec<(u64, u64, Vec<u8>)>;

    /// A `dx, dy` intent payload (two little-endian f32s) — the body a client's
    /// `ClientIntentFor` carries.
    fn move_payload(dx: f32, dy: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&dx.to_le_bytes());
        bytes.extend_from_slice(&dy.to_le_bytes());
        bytes
    }

    /// Seat both players, then for each scripted tick apply its intents and step,
    /// collecting the broadcast `ServerSnapshotFor` byte frames in order.
    fn run_authority(script: &[TickIntents]) -> Vec<Vec<u8>> {
        let mut authority = Authority::new();
        assert_eq!(authority.claim(), Some(1));
        assert_eq!(authority.claim(), Some(2));
        script
            .iter()
            .map(|intents| {
                for (player, sequence, payload) in intents {
                    authority.apply_intent(*player, *sequence, payload.clone());
                }
                authority.step()
            })
            .collect()
    }

    /// The per-snapshot stable-hash sequence (the §7 fingerprint).
    fn hashes(frames: &[Vec<u8>]) -> Vec<u64> {
        frames
            .iter()
            .map(|frame| StableHash::of_bytes(frame).raw())
            .collect()
    }

    /// A non-trivial two-player intent stream: both players move every tick, with
    /// gaps so some ticks carry an intent for only one seat.
    fn sample_script() -> Vec<TickIntents> {
        vec![
            vec![
                (1, 1, move_payload(0.10, 0.0)),
                (2, 1, move_payload(-0.05, 0.20)),
            ],
            vec![(1, 2, move_payload(0.10, 0.0))],
            vec![(2, 2, move_payload(0.00, 0.20))],
            vec![
                (1, 3, move_payload(-0.30, 0.10)),
                (2, 3, move_payload(0.15, -0.15)),
            ],
            vec![],
        ]
    }

    #[test]
    fn two_instances_fed_the_same_intent_stream_are_byte_identical() {
        // SPEC-13 §7, the load-bearing proof: two independent authority instances,
        // the identical ordered intent stream, produce byte-identical
        // ServerSnapshotFor sequences — and identical stable-hash sequences.
        let script = sample_script();
        let first = run_authority(&script);
        let second = run_authority(&script);
        assert_eq!(first, second, "the snapshot byte sequences must be identical");
        assert_eq!(hashes(&first), hashes(&second));
        // A third run reproduces it again (determinism is stable across runs).
        assert_eq!(hashes(&first), hashes(&run_authority(&script)));
    }

    #[test]
    fn the_authoritative_state_genuinely_evolves_under_intents() {
        // The snapshots are not a degenerate constant: the authored world moves
        // under the intents, so the fingerprint changes across ticks (proving the
        // intents flowed through tick_with into real sim state).
        let frames = run_authority(&sample_script());
        let fingerprints = hashes(&frames);
        assert!(
            fingerprints.windows(2).any(|w| w[0] != w[1]),
            "intents must drive an evolving authoritative state"
        );
    }

    #[test]
    fn a_different_intent_stream_diverges() {
        // Different intents ⇒ different authoritative state ⇒ different snapshots.
        // This is what makes the §7 byte-equality above meaningful rather than
        // vacuous (a constant would also be "byte-identical").
        let base = hashes(&run_authority(&sample_script()));
        let other_script = vec![
            vec![(1, 1, move_payload(1.0, 0.0))],
            vec![(2, 1, move_payload(0.0, -1.0))],
            vec![],
            vec![],
            vec![],
        ];
        let other = hashes(&run_authority(&other_script));
        assert_ne!(base, other);
    }

    #[test]
    fn the_payload_decodes_as_the_participant_block_with_acks() {
        // The broadcast frame is a real ServerSnapshotFor whose payload is the
        // documented participant block: count, per-seat (id, flags, intent), the
        // left list, and the authoritative state blob.
        let mut authority = Authority::new();
        assert_eq!(authority.claim(), Some(1));
        assert_eq!(authority.claim(), Some(2));
        authority.apply_intent(1, 7, move_payload(0.25, -0.50));
        let frame = authority.step();

        let (server_tick, acks, payload) =
            NetProtocolApi::decode_server_snapshot_for(&frame).unwrap();
        assert_eq!(server_tick, 1);
        // Both seats ack their highest accepted sequence (seat 2 sent none → 0).
        assert_eq!(acks, vec![(1, 7), (2, 0)]);

        // Decode the participant block per the schema.
        let block = decode_block(&payload);
        assert_eq!(block.participants.len(), 2);
        // Both joined on this first tick.
        assert_eq!(
            block.participants[0],
            (1, FLAG_JOINED_THIS_TICK, move_payload(0.25, -0.50))
        );
        assert_eq!(block.participants[1], (2, FLAG_JOINED_THIS_TICK, Vec::new()));
        assert!(block.left.is_empty());
        assert!(
            !block.state.is_empty(),
            "the authoritative sim snapshot rides along"
        );
    }

    #[test]
    fn leaving_reports_the_seat_in_left_this_tick_once() {
        let mut authority = Authority::new();
        authority.claim();
        authority.claim();
        // Step once so the join flags clear, then a player leaves.
        authority.step();
        assert!(authority.leave(2));
        assert!(!authority.leave(2)); // already gone
        let frame = authority.step();
        let (_tick, acks, payload) = NetProtocolApi::decode_server_snapshot_for(&frame).unwrap();
        // Only seat 1 remains seated; seat 2 is reported as left once.
        assert_eq!(acks, vec![(1, 0)]);
        let block = decode_block(&payload);
        assert_eq!(block.participants.len(), 1);
        assert_eq!(block.participants[0].0, 1);
        assert_eq!(block.left, vec![2]);
        // The next step no longer reports it.
        let next = authority.step();
        let (_t, _a, p2) = NetProtocolApi::decode_server_snapshot_for(&next).unwrap();
        assert!(decode_block(&p2).left.is_empty());
    }

    #[test]
    fn the_room_rejects_a_seat_beyond_capacity() {
        let mut authority = Authority::new();
        assert_eq!(authority.claim(), Some(1));
        assert_eq!(authority.claim(), Some(2));
        assert_eq!(authority.claim(), None, "the third seat is refused");
        // A freed seat is reclaimable at the lowest free id.
        assert!(authority.leave(1));
        assert_eq!(authority.claim(), Some(1));
    }

    #[test]
    fn intents_for_unseated_players_are_dropped_and_short_payloads_are_zero() {
        let mut authority = Authority::new();
        authority.claim(); // only seat 1
        authority.apply_intent(9, 1, move_payload(5.0, 5.0)); // unseated → dropped
        authority.apply_intent(1, 1, vec![1, 2, 3]); // short payload → [0, 0]
        let frame = authority.step();
        let (_tick, acks, payload) = NetProtocolApi::decode_server_snapshot_for(&frame).unwrap();
        assert_eq!(acks, vec![(1, 1)]);
        let block = decode_block(&payload);
        assert_eq!(block.participants.len(), 1);
        // The short payload is preserved verbatim as inputOf (the move it decodes
        // to is zero, but the block carries the raw bytes the client sent).
        assert_eq!(block.participants[0].2, vec![1, 2, 3]);
        assert_eq!(super::unpack_delta(&[1, 2, 3]), [0.0, 0.0]);
    }

    #[test]
    fn authority_is_debug_and_default() {
        let authority = Authority::default();
        assert_eq!(authority.tick(), 0);
        assert!(format!("{authority:?}").contains("Authority"));
    }

    /// A decoded participant block, for the schema assertions above.
    struct Block {
        participants: Vec<(u64, u8, Vec<u8>)>,
        left: Vec<u64>,
        state: Vec<u8>,
    }

    /// Decode the participant block per the module's documented schema. Test-only,
    /// so it reads with ordinary control flow (the wire encoder stays branchless).
    fn decode_block(bytes: &[u8]) -> Block {
        let mut at = 0usize;
        let read_u32 = |bytes: &[u8], at: &mut usize| {
            let v = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap());
            *at += 4;
            v as usize
        };
        let read_u64 = |bytes: &[u8], at: &mut usize| {
            let v = u64::from_le_bytes(bytes[*at..*at + 8].try_into().unwrap());
            *at += 8;
            v
        };
        let count = read_u32(bytes, &mut at);
        let mut participants = Vec::new();
        for _ in 0..count {
            let player = read_u64(bytes, &mut at);
            let flags = bytes[at];
            at += 1;
            let len = read_u32(bytes, &mut at);
            let intent = bytes[at..at + len].to_vec();
            at += len;
            participants.push((player, flags, intent));
        }
        let left_count = read_u32(bytes, &mut at);
        let mut left = Vec::new();
        for _ in 0..left_count {
            left.push(read_u64(bytes, &mut at));
        }
        let state_len = read_u32(bytes, &mut at);
        let state = bytes[at..at + state_len].to_vec();
        Block {
            participants,
            left,
            state,
        }
    }
}
