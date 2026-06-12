# Axiom Netcode — Module Architecture

`axiom-netcode` is **an isolated engine module**, not a layer. It owns the
deterministic-lockstep **session**: over a deterministic, replayable-by-tick
simulation, multiplayer means only *inputs* cross the wire (never state), and a
tick advances once every peer's input for it is present. Server and client run
identical code; the server is just the peer that also referees desync.

The module is **transport-free**. The boundary is plain bytes: the app owns the
socket loop (the nondeterministic edge) and calls `submit_local` / `ingest` with
byte messages, exactly as a wire would — the same way `axiom-windowing` isolates
the live presentation arm behind a deterministic core. A real `wasm32` transport
is a later, separate platform-facing slice.

One `NetcodeApi` is one participant (the facade is the stateful per-peer handle,
like `WindowingApi`).

## What this module owns

- `NetcodeApi` — the single public facade; one peer's session.
- An input timeline keyed by `(tick, peer)` in stable (`BTreeMap`) order.
- The lockstep readiness gate and the confirmed-tick cursor.
- A versioned, length-checked **wire codec** (`SchemaVersion` header + a one-byte
  message discriminant), decoded with the kernel's bounds-checked reader.
- State-hash **reconciliation**: a per-peer hash table and a desync verdict.
- A deterministic 256-bit state digest (a desync fingerprint, not a crypto hash).

## What this module is not allowed to know

- Sockets / network I/O of any kind (`std::net`, `TcpStream`, `UdpSocket`,
  `WebSocket`, `tokio`, …). The app owns the transport.
- Any other module (`axiom-scene`, `axiom-render`, the `axiom` umbrella, …) or
  any `App` / `RunningApp` — an engine module may not depend on a module.
- Rendering, the scene/world, assets, GPU, browser/DOM APIs.
- Wall-clock time, nondeterministic randomness, global mutable state.

`tests/architecture.rs` scans the source tree for every one of these.

## How it consumes the kernel

Netcode depends on **only** the kernel (`allowed_layers = ["kernel"]`):

- `HandleId` backs `PeerId` (stable, ordered, serializable identity).
- `BinaryWriter` / `BinaryReader` + `SchemaVersion` are the wire codec; decode
  failures surface as `KernelResult` / `KernelError` (`Binary` scope:
  `OutOfBounds`, `TruncatedData`, `SchemaVersionMismatch`, `InvalidDiscriminant`).
- `DeterministicRng` (seeded) drives the adversarial-network model in the
  convergence proof under `tests/`.

## Why the boundary is plain bytes / primitives

A module exposes exactly one facade. So the session cannot hand the app a
nameable `Session` or `SyncStatus` type — everything crosses as `u64` / `u32` /
`Vec<u8>` / `Option<bool>` / `[u8; 32]`. That is also exactly a wire shape, which
is why the app can own the socket without the module naming one. `reconcile`
returns `Option<bool>`: `None` = waiting on a peer, `Some(true)` = in sync,
`Some(false)` = desync at the queried tick.

## Public surface

`lib.rs` exposes **exactly one** facade: `NetcodeApi`. Every other type
(`Session`, the wire frame, the timeline, `PeerId`) is internal and reached only
through the facade's plain-data methods.
