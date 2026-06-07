# `engine_no_thread_spawn`

A [dylint] lint that bans direct OS thread spawning (`std::thread::spawn` and
`std::thread::Builder::spawn`) in **non-test engine code** — the layer crates
under `crates/` (except `xtask` and `axiom-zones`) and the modules under
`modules/`.

### Why

Axiom is a **WASM-first, deterministic** engine. Raw OS thread spawning breaks
both of those properties simultaneously:

- **WASM**: `std::thread::spawn` does not exist in the standard
  `wasm32-unknown-unknown` target. Engine code that calls it cannot compile for
  the primary deployment target. WASM threading requires a separate, explicit
  integration (shared memory, Atomics), which is a deliberate, scoped decision —
  not something that falls out of scattering `thread::spawn` calls.
- **Determinism**: OS thread scheduling is non-deterministic. Any engine code
  that spawns threads cannot be replayed, fuzz-tested, or simulated repeatably
  under the same seed. Work that must run concurrently must flow through the
  engine's runtime/scheduler, which owns ordering, priority, and testability.

No scheduler/platform layer exists yet, which is exactly why the ban is
unconditional for now: calling into the raw OS thread API before the engine's
own scheduling abstraction exists means there is nothing to migrate away from
later. The ban keeps the engine portable and deterministic by default; lifting
it for a specific layer is a deliberate, explicit amendment.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]` module
  (detected via `clippy_utils::is_in_test`). Tests may spawn threads freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Macro-expanded call sites** — calls that a macro expanded into the engine
  are not blamed.

The engine/app boundary is decided by the source file path (a `crates/` or
`modules/` component, excluding `xtask` and `axiom-zones`); the `ui/modules/`
and `ui/apps/` fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
