# Axiom Client Core — Module Architecture

`axiom-client-core` is **an isolated engine module**, not a layer. It owns the
**portable client-side multiplayer state machine**: the bookkeeping a client
needs to participate honestly in an authoritative-server session. It is pure
deterministic logic — it opens no socket, knows nothing about the browser, and
does not speak the wire format.

## The authority model

- **The server is authoritative.** This module never holds "the truth." It tracks
  what the client *sent* (pending intents) and *last heard* (the latest
  authoritative server tick + the last acknowledged sequence). Clients send
  **intents**; the server's **snapshots** are authoritative and are applied as
  data.
- There is **no prediction and no rollback** in this first version. Tracking
  pending (unacknowledged) intents is the whole of the client logic.

## State machine

```text
Disconnected --connect()--> Connecting --accept_welcome()--> Connected
```

- **`Disconnected`** — no session. `next_intent` returns `None`.
- **`Connecting`** — establishing a session, not yet welcomed. `next_intent`
  still returns `None`; a snapshot or rejection arriving now is rejected.
- **`Connected`** — the server has welcomed the client; intents may flow.

Rules (all enforced and tested):

- A client **cannot send an intent** while `Disconnected` or `Connecting`
  (`next_intent` → `None`).
- `Welcome` transitions `Connecting → Connected`; a `Welcome` in any other state
  is ignored.
- A `ServerSnapshot` or `RejectedIntent` **before `Welcome` is rejected**.
- `client_sequence` **starts at 1** and **increments by exactly 1** per accepted
  outbound intent (saturating, so it can never wrap/panic).
- The **pending-intent queue preserves insertion order**.
- Accepting a snapshot **drops every pending intent with sequence `<=
  last_accepted_client_sequence`** and advances `latest_server_tick`.
- A snapshot **older** than `latest_server_tick` is **rejected**; an **equal**
  tick is **allowed** and idempotent.
- `RejectedIntent` **removes exactly** the named pending sequence (a no-op if it
  is not pending).

## Why the boundary is plain primitives

An engine module may never depend on another module, so this module does **not**
depend on `axiom-net-protocol`. It neither decodes nor encodes wire frames. Its
facade accepts and returns plain values (`u64` / `&[u8]` / `Vec<u8>` / `bool` /
`Option`). The composition flow lives in the app (or the TypeScript package):

```text
inbound  : socket bytes → axiom-net-protocol decode → values → ClientCoreApi::accept_*
outbound : ClientCoreApi::next_intent → values → axiom-net-protocol encode → socket bytes
```

State-machine rejections are modelled as `false` / `None` — an operation that
does not apply in the current state is a **normal, expected outcome**, not a
failure, so this module returns no kernel errors. (The *wire* validation that
does fail with kernel errors lives in `axiom-net-protocol`.)

## What this module is not allowed to know

- Sockets / network I/O of any kind — the app/package owns the transport.
- Browser / DOM / WebGPU / JS APIs.
- Any other module (including `axiom-net-protocol`) or any `App`.
- Game-specific concepts (movement, health, inventory, combat, …). Intent and
  snapshot payloads are opaque bytes the module never inspects.
- Wall-clock time, nondeterministic randomness, global mutable state.

## How it consumes the kernel

`allowed_layers = ["kernel"]`. It uses the deterministic kernel `Tick` to model
the authoritative server tick and to order snapshots — an out-of-order snapshot
is rejected by `Tick`'s ordering, and `Tick::ZERO` is the pre-session value.

## Public surface

`lib.rs` exposes **exactly one** facade: `ClientCoreApi`. The connection-state
enum and all bookkeeping are internal; status crosses the boundary as a stable
`u8` (`STATUS_*`) plus `is_*` predicates.

## Deferred (intentionally not in this first version)

Prediction, rollback, reconnect, interpolation, and any game-specific state. The
module is deliberately the smallest structurally-correct client substrate.
