# Axiom Simulation Worker

This note records the architecture of the Axiom **simulation worker** and how the
.NET 10 authoritative server embeds it. It is a practical reference, not a spec.

## Selected architecture

**In-process native Rust `cdylib`, embedded by the .NET server via P/Invoke.**

- The worker is the app `apps/axiom-netplay-ffi` (`crate-type = ["cdylib","rlib"]`).
  It embeds the real headless Axiom engine (`RunningApp`) and exposes a stable,
  versioned, panic-safe C ABI.
- The .NET server (`examples/axiom-netplay-dotnet`) loads the native library and
  drives it: create a sim, submit validated intents, advance ticks, read back
  authoritative snapshots and per-tick state hashes, export/verify replays.
- WASM is only the browser shipping format. On the server the *same* engine is
  compiled native and driven headlessly as the authority.

## Why the worker is an app

Apps are the only workspace tier where the things the worker needs are legal:

- `unsafe` / `extern "C"` (the FFI boundary),
- composing several modules (`axiom` engine umbrella + `axiom-net-protocol`),
- game-specific schema and branchy validation logic.

Layers and modules forbid all of the above (no `unsafe` FFI, no module→module
deps for engine modules, branchless + 100% coverage laws). Apps are composition
leaves outside the branchless/coverage gates, so the worker lives there.

## Why NOT elsewhere (v1)

- **Not the kernel / a layer**: no networking, no sessions, no game rules ever
  go in the spine. The worker is a leaf, not part of the engine layer DAG.
- **Not a module**: engine modules cannot hold sockets, cannot do `unsafe` FFI,
  cannot depend on other modules, and must be branchless + 100% covered. Game
  schema (the intent payload meaning) must not enter `axiom-net-protocol` or any
  layer.
- **Not a browser feature**: rendering/input-capture/prediction stay in the
  browser client; they never enter the worker.
- **Not a separate process / WASI / scaleout (v1)**: an out-of-process worker,
  sandboxing, or multi-room scaleout adds IPC, supervision, and routing before a
  single authoritative room even works. Those are deliberately deferred. The
  worker-control contract is shaped so they remain possible later without a
  rewrite.

## Tier A vs Tier B boundary

Two protocols, kept strictly separate. This separation is what makes
browser-driven state mutation impossible.

- **Tier A — browser wire protocol** (`axiom-net-protocol`, browser-reachable):
  the browser can encode only `JoinRoom`, `LeaveRoom`, `ClientIntent`. The server
  replies `Welcome`, `ServerSnapshot`, `ServerEvent`, `RejectedIntent`. There is
  no client message that decodes into "set state". The worker re-exports this
  codec across the C ABI (`apps/axiom-netplay-ffi/src/codec.rs`) so .NET has one
  source of truth — no hand-written C# codec twin.
- **Tier B — worker-control protocol** (.NET ↔ worker, **server-only**, never on
  a socket): create/destroy/load/submit/advance/snapshot/hash/export/verify, plus
  version handshake. The browser cannot name, encode, or reach any Tier-B call.
  Defined in `apps/axiom-netplay-ffi/src/ffi.rs` and friends.

## Authority and ownership

- **Browser clients only send input intent.** Never state, positions, hashes,
  scores, rewards, or authoritative events.
- **Authoritative state comes from the worker only.** It is the engine's durable
  scene state via `RunningApp::snapshot_sim()` / `restore_sim()`. There is no
  parallel position/state mirror — that would be a second source of truth.
- **.NET owns** sockets, transports, auth/session ownership, room ownership,
  player-id assignment, tenant isolation, persistence hooks, and process
  lifecycle. The worker trusts only the `player_id` .NET assigns it.
- **Rust/Axiom owns** deterministic headless simulation, intent validation,
  authoritative state, snapshots, per-tick state hashes, and replay
  records/verification.

## State hashes and replay

- Per-tick `state_hash = StableHash::of_bytes(snapshot_sim())` (kernel FNV-1a).
  The hash is a diagnostic locator; **byte-equality of canonical snapshot bytes
  is the determinism proof**.
- Replay records `(seed, max_players, fixed_step_ns)` plus, per tick, the ordered
  accepted intents and the prev/new state hashes. Verification re-runs from tick
  zero in a fresh worker and compares per-tick hashes, reporting `matched`, the
  first divergence tick, and the final hash.

## Browser client (Tier A consumption)

The authoritative `ServerSnapshot` payload the .NET host broadcasts is the
worker's `snapshot_sim()` bytes (the engine's durable scene state) plus the
per-client `last_accepted_client_sequence`. The browser's role:

- Send only `ClientIntent` (monotonic `client_sequence`), predict locally, buffer
  unacknowledged intents.
- On a `ServerSnapshot`, restore the authoritative state (the browser runs Axiom
  too: `restore_sim(payload)`), drop acknowledged intents (`<= last_accepted`),
  and replay the still-unacknowledged ones onto the restored state.
- Treat every correction as server truth; browser state is disposable.

The reconciliation state machine already exists and is 100%-covered in the engine
module `axiom-client-core` (`accept_welcome` / `next_intent` / `accept_snapshot` /
`accept_rejected_intent` / `pending_intent_count` / `last_acked_client_sequence`).
The `@axiom/client` TypeScript SDK wraps it. **Wiring the browser app to consume
`snapshot_sim` bytes via `restore_sim` and the Playwright end-to-end verification
are not implemented in this pass** — they require a wasm rebuild, a running server,
and a headless browser. They are the next step, on top of the now-authoritative
worker + .NET loop.

## Validation commands

- `cargo test --workspace`
- `cargo xtask check-architecture`
- `cargo dylint --all -- --all-targets`
- `dotnet test examples/axiom-netplay-dotnet` (and the worker test project)

The worker app is exempt from the branchless and 100%-coverage gates (apps are
out of scope), but is held to `cargo fmt`, `cargo clippy -D warnings`, and the
architecture checker like everything else.
