# Axiom Net Protocol — Module Architecture

`axiom-net-protocol` is **an isolated engine module**, not a layer. It owns the
stable multiplayer **wire/message contract**: the exact, deterministic bytes
clients and servers exchange. It is *protocol data only* — constructors,
validators, and encode/decode helpers. It holds no session state, opens no
socket, and knows nothing about the browser.

## The authority model this contract encodes

- **The server is authoritative.** Clients send **intents** (what they want to
  do), never state. The server replies with authoritative **snapshots** and
  **events**. Snapshots — not client opinion — are the source of truth; the
  client applies them as data.
- The two message families make that asymmetry explicit on the wire:

  | Direction        | Messages                                            |
  |------------------|-----------------------------------------------------|
  | client → server  | `JoinRoom`, `LeaveRoom`, `ClientIntent`             |
  | server → client  | `Welcome`, `ServerSnapshot`, `ServerEvent`, `RejectedIntent` |

The portable client state machine (connection state, sequence numbers, pending
intents) is **not** here — it lives in `axiom-client-core`. The transport (a
socket / WebSocket) is **not** here — it lives at the app/package edge.

## What this module owns

- `NetProtocolApi` — the single public facade; a stateless codec namespace.
- The seven message bodies and their field validators.
- A versioned frame envelope (`SchemaVersion` header + a one-byte message-kind
  discriminant), read with the kernel's bounds-checked reader.
- Two documented size bounds: `MAX_ROOM_ID_LEN` (64 B) and `MAX_PAYLOAD_LEN`
  (64 KiB).

## Required primitive fields

| Field                 | Representation                | Validation                       |
|-----------------------|-------------------------------|----------------------------------|
| `ProtocolVersion`     | `u32`                         | must be nonzero                  |
| `RoomId`              | length-prefixed bytes         | non-empty, `<= MAX_ROOM_ID_LEN`  |
| `ClientId`            | kernel `HandleId` (`u64`)     | must be nonzero                  |
| `ClientSequence`      | `u64` field                   | any value                        |
| `ServerTick`          | `u64` field                   | any value (a kernel-`Tick` count)|
| `IntentPayload` / `SnapshotPayload` / `EventPayload` | bounded opaque bytes (`OpaquePayload`) | `<= MAX_PAYLOAD_LEN` |
| `RejectReasonCode`    | `u32`                         | machine-readable; any value      |
| `fixed_step_ns` (Welcome) | kernel `FixedStep` (`u64`)| must be nonzero                  |

`IntentPayload`, `SnapshotPayload`, and `EventPayload` are all the same opaque,
bounded byte buffer (`OpaquePayload`) — they differ only in role, not shape.
This keeps the contract free of game-specific schema; a future schema layer will
give the payloads structure.

## Message contents

- `JoinRoom { protocol_version, room_id, token }` — `token` is an optional
  opaque payload (empty when absent).
- `LeaveRoom { room_id }`.
- `ClientIntent { client_sequence, predicted_client_tick, last_seen_server_tick, payload }`.
- `Welcome { protocol_version, client_id, server_tick, fixed_step_ns }`.
- `ServerSnapshot { server_tick, last_accepted_client_sequence, payload }`.
- `ServerEvent { server_tick, payload }`.
- `RejectedIntent { client_sequence, reason_code }`.

## Encoding rules (deterministic)

- **Little-endian binary**, via the kernel `BinaryWriter`/`BinaryReader`, so the
  bytes are identical on every platform (native and wasm).
- Every frame is `SchemaVersion (2× u16)` then a **stable one-byte kind**
  (`0..=6`, never renumbered), then a **fixed field order**.
- No hash-map iteration, no random ids, no wall-clock timestamps anywhere.
- Round-trip is proven for every message type (`decode(encode(m)) == m`).

## Decoding rules (all rejections are precise kernel errors)

| Failure                        | Scope    | Code                    |
|--------------------------------|----------|-------------------------|
| incompatible wire major        | `Binary` | `SchemaVersionMismatch` |
| unknown / unexpected kind byte | `Binary` | `InvalidDiscriminant`   |
| truncated body                 | `Binary` | `OutOfBounds` / `TruncatedData` |
| protocol version / client id zero | `Message` | `InvalidId`          |
| room id empty / over-long; payload over max | `Message` | `OutOfBounds` |
| zero `fixed_step_ns`           | `Time`   | `InvalidFixedStep` (kernel `FixedStep`) |

Validation lives in one place per field (the field newtype's constructor) and is
re-run on decode, so a malformed value can never enter the system whether it
came from a local caller or off the wire.

## What this module is not allowed to know

- Sockets / network I/O (`std::net`, `WebSocket`, `tokio`, …) — the app/package
  owns the transport.
- Browser / DOM / WebGPU / JS APIs.
- Any other module (`axiom-client-core`, `axiom-scene`, …) or any `App`.
- Game-specific schema: movement, health, inventory, combat, etc. Payloads stay
  opaque until a future schema layer exists.
- Wall-clock time, nondeterministic randomness, global mutable state.

## How it consumes the kernel

`allowed_layers = ["kernel"]`. It uses `BinaryWriter`/`BinaryReader` +
`SchemaVersion` for the codec, `KernelResult`/`KernelError` for validation
failures, `HandleId` to back `ClientId`, and `FixedStep` for the nonzero
`fixed_step_ns` field.

## Public surface

`lib.rs` exposes **exactly one** facade: `NetProtocolApi`. Because a module
exposes one nameable type, messages cross the boundary as plain primitives
(`u32` / `u64` / `&[u8]` / `Vec<u8>`) — which is also exactly a wire shape, so an
app or the TypeScript `axiom-client` package can own the socket without naming a
protocol type.

## Deferred (intentionally not in this first version)

Prediction, rollback, reconnect, delta/compression of payloads, matchmaking, and
any game-specific message schema. The contract is deliberately the smallest
structurally-correct substrate.
