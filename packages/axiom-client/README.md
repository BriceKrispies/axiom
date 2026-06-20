# @axiom/client

The browser authoring SDK for Axiom multiplayer. A tiny, dependency-free
WebSocket client that speaks the `axiom-net-protocol` wire format.

> **This package is browser glue and author ergonomics — not engine truth.**
> The server is authoritative. Clients send *intents*; the server's *snapshots*
> are the source of truth. The SDK never hides server authority and invents no
> gameplay-specific API.

## What's here

- **`AxiomClient`** — the ergonomic client: open a socket, join a room, send
  intents, and receive authoritative snapshots/events through callbacks.
- **A protocol codec** (`encode*` / `decode*` / `peekKind`) that mirrors the
  Rust `axiom-net-protocol` module **byte-for-byte**. The Rust module owns the
  canonical contract; this is its browser twin so there is no build step. A
  golden-bytes test on both sides locks the two codecs to one wire format.

The portable client state machine itself is the Rust `axiom-client-core` module;
this SDK mirrors its concepts (connection state, a monotonic client sequence, a
pending-intent queue, the latest server tick) on the browser side, where the
WebSocket lives. The Rust modules never touch the browser; this package is the
only place WebSocket/DOM is used.

## Usage

```ts
import { AxiomClient } from "@axiom/client";

const client = new AxiomClient();

client.onStatus((status) => console.log("status:", status));
client.onSnapshot((snap) => applySnapshot(snap.serverTick, snap.payload));
client.onEvent((ev) => handleEvent(ev.serverTick, ev.payload));

client.connect({
  url: "wss://example.com/play",
  roomId: "lobby",        // string (UTF-8) or Uint8Array
  token: "optional-auth", // optional
  protocolVersion: 1,     // optional, default 1
});

// Later, when connected (the server has sent Welcome):
client.sendIntent(new Uint8Array([/* opaque, game-defined */]));

// Inspect mirrored state:
client.getStatus();              // "disconnected" | "connecting" | "connected"
client.getServerTick();          // latest authoritative tick applied
client.getLastAckedSequence();   // newest client sequence the server acked
client.getPendingIntentCount();  // sent-but-unacknowledged intents

client.disconnect();             // sends LeaveRoom (if connected), then closes
```

### `connect(config)`

| field             | type                          | notes                          |
|-------------------|-------------------------------|--------------------------------|
| `url`             | `string`                      | required                       |
| `roomId`          | `string \| Uint8Array`        | required                       |
| `token`           | `string \| Uint8Array`        | optional                       |
| `protocolVersion` | `number`                      | optional, default `1`          |
| `socketFactory`   | `(url) => WebSocketLike`      | optional; defaults to `WebSocket` (used by tests to inject a fake) |

### Behavior

- On socket open the client sends `JoinRoom`. **Status stays `connecting` until
  the server's `Welcome` arrives** — the server, not the socket, decides you are
  in.
- `sendIntent(payload)` only sends while `connected`; it assigns the next
  monotonic `client_sequence`, records it as pending, and returns `true`. While
  `disconnected`/`connecting` it returns `false` and sends nothing.
- A `ServerSnapshot` acknowledges pending intents up to its
  `last_accepted_client_sequence`, advances the server tick (an older snapshot is
  ignored, an equal tick is allowed), and fires `onSnapshot`.
- A `RejectedIntent` drops exactly the named pending intent.
- `disconnect()` sends `LeaveRoom` (when connected) and closes the socket.

## Testing

No dependencies are required to run the tests — they use Node's built-in test
runner and execute the TypeScript directly via Node's type stripping (Node ≥ 23.6,
developed on Node 24):

```sh
npm test
# → node --test "test/**/*.test.ts"
```

## Static analysis & gates

This package is held to TypeScript-native versions of the Axiom engine's laws:
maximum-strictness static analysis, a branch ban, and 100% coverage. After
`npm install`:

```sh
npm run typecheck   # tsgo — TypeScript 7.0 native (Go) compiler
npm run lint        # Oxlint — every category an error, plus the branch ban
npm run coverage    # node:test built-in coverage, fails under 100%
npm run gate        # all three in sequence (or: make ts-gate from the repo root)
```

See [`STATIC_ANALYSIS.md`](STATIC_ANALYSIS.md) for how each maps to the Rust
spine's gates, and [`../../docs/ts-sdk-hardening.md`](../../docs/ts-sdk-hardening.md)
for the in-progress remediation to a fully-green gate.

The tests prove: connect enters `connecting`; socket-open sends `JoinRoom`;
`sendIntent` encodes a `ClientIntent` only when connected and rejects while
disconnected/connecting; `Welcome` → connected; snapshots/events fire their
handlers; `RejectedIntent` updates pending state; `disconnect` closes and returns
to disconnected; the encoder/decoder round-trips **every** message type; a
truncated inbound payload and an unknown inbound kind are rejected; and the wire
bytes match the Rust module (golden-bytes test).

## Intentionally deferred

Prediction, rollback, reconnect, compression, and matchmaking are **not** in this
first version, and there is no game-specific schema — intent/snapshot/event
payloads are opaque bytes until a future schema layer exists.
