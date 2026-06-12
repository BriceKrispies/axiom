# Axiom Netcode — Testing Discipline

The contract is determinism: given the same ordered input timeline, every peer
reaches byte-identical state at every confirmed tick. Every public concept
reached through `NetcodeApi` has a direct test, and the headline proof exercises
the whole session end-to-end with no sockets.

## What is tested

- `PeerId` — raw round-trip, ordering, binary serialization, truncation reject.
- `NetCommand` — accessors, serialization round-trip, empty payload, every
  truncated prefix rejected.
- `NetMessage` (wire codec) — `Input` and `HashBeacon` round-trips; an unknown
  tag → `InvalidDiscriminant`; an incompatible major → `SchemaVersionMismatch`;
  every truncated prefix of both variants rejected (walking each field's `?`).
- `InputTimeline` — empty start, `has_all` requires every peer, `ordered_at`
  sorts by peer and is scoped to the tick, insert is idempotent.
- `Session` / `NetcodeApi` — peer-set dedup/sort via confirm order, local-tick
  increment, the readiness gate (waits for all peers, confirms in order),
  out-of-order / incomplete confirm is a no-op, `ingest` rejects malformed bytes,
  `reconcile` maps Pending / In-Sync / Desync, `digest` determinism.

## Determinism / the convergence proof

`tests/lockstep_convergence.rs` is the multiplayer-correctness proof, in-process
and socket-free. N peers (each a real `NetcodeApi`) are joined by a deterministic
adversarial transport that reorders, delays, and drops messages, driven by the
kernel's seeded `DeterministicRng` (so a run is replayable). Each peer runs a
tiny deterministic mock sim standing in for a real `App`. It asserts:

- **Safety** — under 30% loss + reordering, every peer's state hash is identical
  at every commonly-confirmed tick.
- **Liveness** — retransmission carries confirmation to near-completion.
- **Replayability** — the same seed replays byte-identically.
- **Desync detection** — an injected divergence is caught by `reconcile`.
- **Clean-path agreement** — a lossless network reconciles In-Sync at every tick.

## Architecture / boundary

`tests/architecture.rs` enforces:

- `module.toml` exists and declares `allowed_modules = []`.
- `lib.rs` publicly exports exactly `pub use netcode_api::NetcodeApi;`.
- Source imports only `axiom_kernel` (the single allowed layer) and no other
  module; no lower layer imports `axiom-netcode`; an app importing it must list
  `"netcode"` in its `app.toml`.
- No sockets / network I/O, no browser/JS/DOM, no WebGPU/WebGL, no wall-clock
  time, no nondeterministic randomness, no console output, no placeholder macros,
  no global mutable state, no `utils`/`helpers`/`common`/`misc` modules.
