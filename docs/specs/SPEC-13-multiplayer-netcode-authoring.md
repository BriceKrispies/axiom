# SPEC-13 — Multiplayer & netcode authoring

> Status: Draft
> Contract: §16(.1–.6)   Vocabulary: WebSocket realtime snapshot/delta/ack, client prediction/reconciliation, per-player intent stream, rooms/authority, JWT handshake, matchmaking   Determinism: sim

## 1. Summary

Server-authoritative realtime multiplayer with **one** authored simulation
deployed three ways — `local`, `authority`, `predicted` — where the engine, not
the author, performs snapshotting, delta replication, prediction, and
reconciliation. This is the contract's central bet: because the fixed update is
deterministic (§17), prediction is just "re-run unacked local intents on top of
the latest authoritative snapshot," and the author never hand-writes a netcode
twin.

**Axiom has two multiplayer stacks; this spec targets exactly one.** Contract §16
is the **server-authoritative intent/snapshot** stack (`axiom-net-protocol` +
`axiom-client-core` + the `@axiom/client` TS SDK; neon-clash is its template).
The other stack — deterministic-lockstep (`axiom-netcode`: inputs cross the wire,
never state; per-tick state-hash reconciliation) — is a **different** networking
model and is **out of scope here**. Do not conflate them: lockstep gates a shared
tick on all peers' inputs; §16 runs an authority ahead of optimistic predicted
clients. This spec extends the §16 stack only.

Of the 11 reference games, the competitive/co-op ones (neon-clash and its kin)
demand §16; single-player games need none of it and pay nothing (`local` mode is
the default and is just SPEC-00's loop).

## 2. Current state (verified)

The **substrate is real**; the **high-level authoring API does not exist.**

- **Wire codec — `axiom-net-protocol` (engine module, `allowed_layers=["kernel"]`).**
  Facade `NetProtocolApi`; a stateless 7-message codec: `JoinRoom`, `LeaveRoom`,
  `ClientIntent`, `Welcome`, `ServerSnapshot`, `ServerEvent`, `RejectedIntent`,
  with `encode_*`/`decode_*` per message. `ServerSnapshot` carries
  `{ server_tick: u64, last_accepted_client_sequence: u64, payload: OpaquePayload }`.
  Capabilities: `multiplayer-wire-contract`, `client-to-server-messages`,
  `server-to-client-messages`, `bounded-opaque-payloads`.
- **Client state machine — `axiom-client-core` (engine module, `["kernel"]`).**
  Facade `ClientCoreApi`: `Disconnected→Connecting→Connected`, with
  `connect`/`accept_welcome`/`next_intent` (monotonic client-sequence +
  pending-intent tracking)/`accept_snapshot` (acks up to a sequence)/
  `accept_rejected_intent` (drops one pending)/`status_code`/`latest_server_tick`/
  `pending_intent_count`/`last_acked_client_sequence`. It tracks a
  predicted-client-tick. Capabilities: `client-connection-state-machine`,
  `monotonic-client-intent-sequence`, `pending-intent-tracking`,
  `authoritative-server-tick-tracking`.
- **TS SDK — `packages/axiom-client` (`@axiom/client`).** Exports `AxiomClient`
  (`connect`/`disconnect`/`sendIntent`/`onSnapshot`/`onEvent`/`onStatus`/getters),
  three transports (`WebSocketTransport`, `WebTransportTransport`,
  `WebRtcTransport`), and the TS twin of the 7-message codec
  (`encode*`/`decode*` + `peekKind`/`decodeFrame`). Held to the TS spine laws.
- **Server harness — `tools/axiom-netplay-server` (tool tier).** A `tokio` +
  `tokio-tungstenite` bin: fixed ~60 Hz `tick_loop`, claims player slots on
  `JoinRoom`, integrates each client's pending intent, broadcasts
  `ServerSnapshot`. Reuses `axiom-net-protocol` for byte-identical wire framing.

**Missing — the entire §16 authoring surface.** No `NetSim`/`inputOf(player)`,
no `Intent`-derived codec generation, no `RoomConfig`/`hostRoom`/`Room`, no
`NetClient`/`JoinConfig`/`joinRoom`, no `onSnapshot`/`onRestore`,
no `configureNet`/`NetConfig`, no `matchmake`/`reportOutcomes`. Every
neon-clash-style backend to date was built at app/example tier (the
netplay-server scaleout work) **with zero spine change** — proving the substrate,
not the authoring contract. JWT handshake is **partial** (`token` is forwarded as
opaque bytes through `JoinRoom`; no verification path is engine-owned).
Matchmaking is **partial** (HTTP matchmaking exists at app tier; no `matchmake`
projection).

## 3. Placement

No new spine module. The §16 surface is **projection over existing substrate plus
two narrow spine extensions**, justified under the Module Law:

1. **Per-player intent fan-out — extend `axiom-net-protocol`.** The codec today
   addresses one anonymous client. `NetSim.inputOf(player)` and the authority's
   per-player accept/reject loop need the wire to carry a stable `PlayerId` and
   the snapshot to carry per-player acknowledgement. This is the wire contract's
   job — it lives at the **lowest correct layer** (the module that owns the bytes),
   not invented above. Stays `allowed_layers=["kernel"]`, engine module,
   `allowed_modules=[]`.
2. **Prediction/reconciliation bookkeeping — extend `axiom-client-core`.**
   `ClientCoreApi` already tracks pending intents, the acked sequence, and the
   predicted-client-tick — exactly the state reconciliation needs. Extend it with
   the **resimulation cursor** (which intents to replay after a snapshot snap) and
   the **interpolation delay** bookkeeping. This is deterministic client-side
   state with no platform surface: engine module, `["kernel"]`.
3. **`Intent` codec generation — `@axiom/game` projection.** The author defines
   one `Intent` record; the engine derives the wire codec from that single
   definition (no hand-written twin, §16.2). The derivation is a TS-side
   structural serializer over the contract's value types projected onto
   `axiom-net-protocol`'s opaque payload. Bounds/oversize validation happens **at
   the edge** (the codec rejects, surfacing `RejectedIntent`).
4. **High-level authoring shapes — `@axiom/game` (`@axiom/client` underneath).**
   `NetSim`, `NetClient`/`joinRoom`, `onSnapshot`/`onRestore`, `configureNet`,
   `matchmake`/`reportOutcomes` are projected in `@axiom/game`, which already
   depends on `@axiom/client` (SPEC-00 §3); `joinRoom` wraps `AxiomClient` +
   `ClientCoreApi`. `NetSim` is the SPEC-00 `Sim` widened with player addressing —
   same `world`/`rng`, player-indexed `input`.
5. **`hostRoom`/authority loop — app/tool tier, NOT a spine module.** The
   authoritative deployment is `tools/axiom-netplay-server` (and app-tier
   backends). `hostRoom(config) → Room` is the author-facing handle to *stand up*
   that authority; standing up a server, owning the socket accept loop, and
   driving wall-clock ticks is leaf composition + platform edge — the app/tool
   role (Module Law). The room harness composes `axiom-net-protocol` +
   the author's `onFixedUpdate` + SPEC-00's accumulator; it is **not** a new
   reusable module. The same `onFixedUpdate` the author wrote for `local` runs
   here as `authority`.

Legality: the two spine extensions only widen modules that already own this
exact concern (wire bytes; client reconciliation state), keep `["kernel"]`, stay
`allowed_modules=[]`, and add no platform API. All composition (room harness,
transport selection, JWT verification) stays at app/tool tier where cross-module
and platform wiring legally lives.

## 4. API surface

### 4.1 Native (Rust spine extensions)

`axiom-net-protocol` — per-player addressing on the existing 7 messages:

```rust
// Intent now carries the originating player and a bounded, validated opaque body.
// Snapshot acks per player so a client knows which of *its* intents landed.
impl NetProtocolApi {
    pub fn encode_client_intent_for(player: u64, sequence: u64, predicted_tick: u64,
                                    last_seen_server_tick: u64, payload: &[u8]) -> Vec<u8>;
    pub fn encode_server_snapshot_for(server_tick: u64,
                                      acks: &[(u64 /*player*/, u64 /*seq*/)],
                                      payload: &[u8]) -> Vec<u8>;
    // decode twins; oversize/malformed -> Err (rejection is a value, never a panic).
}
```

`axiom-client-core` — the resimulation cursor reconciliation needs:

```rust
impl ClientCoreApi {
    // After a snapshot snap, the ordered unacked intents to replay on top of it.
    pub fn unacked_intents(&self) -> &[(u64 /*seq*/, Vec<u8>)];
    // Render-time interpolation cursor: server_tick - interpolation_delay_ticks (saturating).
    pub fn interpolation_tick(&self, delay_ticks: u64) -> u64;
}
```

`hostRoom`/the authority loop is **not** a spine facade — it is the
`tools/axiom-netplay-server` harness (extended for per-player slots + the
author's `onFixedUpdate`), driven by SPEC-00's `FrameAccumulator`.

### 4.2 TS authoring projection (the contract, §16)

```ts
// §16.1 — networked sim is the single-player Sim widened by player addressing.
type PlayerId = number;
interface NetSim extends Sim {
  players(): PlayerId[];                  // seated players, stable order
  inputOf(player: PlayerId): Input;       // that player's per-tick intents (§8)
  joinedThisTick(): PlayerId[];
  leftThisTick(): PlayerId[];
}
function onFixedUpdate(cb: (sim: NetSim) => void): void;   // networked overload of SPEC-00

// §16.2 — author defines ONE shape; engine owns the wire codec from it (no twin).
type Intent = Record<string, number | string | boolean>;

// §16.3 — a room hosts one authoritative sim; hostRoom stands up the authority (app/tool tier).
type RoomId = string;
interface RoomConfig { maxPlayers: number; seed: bigint; fixedHz: number; botFill?: { afterTicks: Ticks } }
interface Room { readonly id: RoomId; players(): PlayerId[]; close(): void }
function hostRoom(config: RoomConfig): Room;

// §16.4 — client connection; the authority (not the socket) decides membership.
type ConnStatus = "connecting" | "connected" | "disconnected";
interface NetClient {
  status(): ConnStatus;
  localPlayer(): Result<PlayerId>;          // null until admitted
  sendIntent(intent: Intent): void;
  onStatus(cb: (s: ConnStatus) => void): void;
  onRejected(cb: (reason: string) => void): void;
  leave(): void;
}
interface JoinConfig { url: string; roomId: RoomId; token?: string }   // token opaque (JWT)
function joinRoom(config: JoinConfig): NetClient;

// §16.5 — extra authoritative state hooks; prediction/interp are CONFIGURED, not written.
function onSnapshot(cb: () => Uint8Array): void;
function onRestore(cb: (bytes: Uint8Array) => void): void;
interface NetConfig { predictLocalPlayer: boolean; interpolateRemote: boolean; interpolationDelayTicks?: number }
function configureNet(cfg: NetConfig): void;

// §16.6 — matchmaking delegated to host; outcomes are per-player.
interface Match { roomId: RoomId; url: string }
function matchmake(opts?: { mode?: string }): Promise<Match>;
function reportOutcomes(results: Record<PlayerId, Outcome>): void;   // Outcome from §15 / SPEC-12
```

`Sim.input` (single-player accessor) equals `inputOf(localPlayer)` in `local`
mode — the three deployments share the authored callback verbatim.

## 5. Data contracts

Neutral types crossing the boundary (all already byte-shaped on the wire):

- **`ClientIntent`** — `{ player, sequence, predicted_tick, last_seen_server_tick,
  payload }`. `payload` is the author's `Intent` serialized by the engine-derived
  codec; bounded and validated at encode/decode (oversize ⇒ `RejectedIntent`).
- **`ServerSnapshot`** — `{ server_tick, acks: [(player, sequence)], payload }`.
  `payload` is the engine-snapshotted component store + sim tick + RNG state
  (§16.5), plus any author `onSnapshot` bytes appended. Opaque to the transport.
- **`RejectedIntent`** — `{ player, sequence, reason }`; `reason` surfaces to
  `NetClient.onRejected`. Rejection is a normal value, never a crash (§16.2).
- **`PlayerId`** — opaque seat index, stable within a room; never serialized into
  authored sim state that a replay must reproduce (it is re-bound on join, like
  SPEC-00 handles).
- **`StepBudget`** — reused from SPEC-00; the authority and every predicted client
  advance through the same accumulator.

## 6. Determinism

This spec's core determinism requirement is **§17.6, cross-instance bit-identical
state** — it is what makes prediction and reconciliation *automatic* rather than
hand-tuned:

1. **Identical per-player intent stream ⇒ identical state, across machines.** The
   authority and every predicted client run the same authored `onFixedUpdate`
   over the same `(seed, config, per-player intent stream)` and **must** reach
   byte-identical state on different hardware. Reconciliation is then trivial:
   snap to the latest authoritative snapshot, replay the `unacked_intents`
   (§4.1) — no fuzzy correction, because a correct replay cannot drift.
2. **This depends on SPEC-10.** Cross-machine bit-identity requires deterministic
   `f32` arithmetic in all sim code; if the game uses physics, that determinism is
   **SPEC-10's** cross-platform deterministic physics. Without it, predicted
   clients desync and §16 is unsound. SPEC-13 inherits, does not re-solve, the
   f32 determinism obligation (shared §9 item).
3. **Single clock / single randomness / tick-indexed input** (§17.1–.3) are
   inherited from SPEC-00/01/05 unchanged: the authority's wall-clock tick rate
   sets *when* a step runs, never *what* it computes; per-player input is sampled
   to the per-tick intent snapshot before the sim sees it.
4. **Presentation excluded** (§17.5): `interpolateRemote`/`interpolationDelayTicks`
   smooth *rendering* of remote entities between snapshots; that interpolated
   value is presentation-only and never re-enters a fixed update.
5. **The codec is canonical and stateless.** Identical messages encode to
   identical bytes on the Rust and TS sides (the existing twin codec invariant);
   no machine-dependent ordering or float formatting enters the wire.

The spine extensions (per-player addressing, resim cursor) are `sim`-class:
branchless, 100% covered.

## 7. Acceptance / proof

- **`axiom-net-protocol` extension:** 100% covered, branchless. Round-trip
  property test: `decode(encode(m)) == m` for per-player intent and multi-ack
  snapshot, including oversize/malformed ⇒ rejection-value (no panic). Byte-parity
  golden against the `@axiom/client` TS codec (the existing cross-language fixture
  extended for the new fields).
- **`axiom-client-core` extension:** 100% covered, branchless. Test: after a
  snapshot acking sequence `k`, `unacked_intents()` is exactly the intents `>k` in
  order; `interpolation_tick(d)` saturates at 0.
- **Cross-instance determinism (§17.6) — the load-bearing proof.** A golden
  replay: one authored `onFixedUpdate`, a fixed per-player intent stream, run as
  (a) a single `authority`, (b) a `predicted` client that snaps + replays. Assert
  the per-tick state-hash sequence (SPEC-00 §17.4) is **byte-identical** between
  the two deployments, and identical across a second run. Reconciliation drift = 0.
- **`@axiom/game` projection:** tsgo + Oxlint (branch ban) + 100% TS coverage. A
  test game derives an `Intent` codec from one record, hosts a room, joins two
  `NetClient`s, exchanges intents headless, and asserts both observe the same
  authoritative snapshots. `onRejected` fires for an oversize intent.
- **Slice test:** `tools/axiom-netplay-server` runs the authored callback as
  `authority` for a 2-player room over a real socket; a `predicted` client
  reconciles to bit-identity. (Tool/app tier — outside the coverage gate, ships
  with its own slice test.)

## 8. Dependencies & order

- **Depends on:** SPEC-00 (the `Sim`/loop/accumulator `NetSim` widens), SPEC-01
  (seeded `Rng` in the snapshotted state), SPEC-05 (the per-tick `Input` that
  `inputOf` returns), and — when physics is used — **SPEC-10 (cross-platform
  deterministic physics)**, the hard prerequisite for §17.6. Contract §18 places
  this at item 11, after determinism is fully nailed.
- **Lands after** the single-player surface is real: there is no point predicting
  a sim the author cannot yet write.
- **Nothing in the spine depends on this**; it is a leaf capability consumed only
  by competitive/co-op apps. `local` mode is free and ships with SPEC-00.

## 9. Open questions

- **JWT handshake ownership.** `token` is opaque to the engine (§16.4). Where does
  *verification* live? It is not engine spine (auth is policy). Lean: the
  authority harness (`tools/axiom-netplay-server`) verifies before admitting, and
  the engine only forwards the bytes — but the admit/reject result must surface as
  a `ConnStatus`/`onRejected`, so the *seam* is contract, the *policy* is app.
- **Intent codec for non-flat records.** §16.2's `Intent` is
  `Record<string, number|string|boolean>` (flat). Does the engine-derived codec
  ever need nested/array intents, or is flat the permanent floor? Flat keeps the
  derivation branchless and the wire bounded; widen only on a real game's demand.
- **Snapshot vs. delta default.** §16.5 promises automatic deltas, but the
  current `ServerSnapshot.payload` is a full opaque blob. Delta encoding (vs. the
  last acked snapshot) is a spine optimization in `axiom-net-protocol`; is it
  required for the first competitive game, or does full-snapshot suffice until a
  bandwidth proof forces deltas? Default: full snapshot first, delta when measured.
- **`botFill` author seam.** A `botFill` slot is an ordinary `PlayerId` whose
  intents the author supplies from game code (§16.3). Confirm that seam is just
  "call `sendIntent` for that player id inside `onFixedUpdate`" with no special
  engine path — keeping bot behaviour game logic, not engine.
- **Transport selection.** Which of the three transports (`WebSocket` reliable /
  `WebTransport` / `WebRtc` unreliable) does `joinRoom` pick, and is it author-
  configurable via `JoinConfig`? Reliable WebSocket is the safe default; unreliable
  transports interact with snapshot/ack semantics and need their own proof.
</content>
</invoke>
