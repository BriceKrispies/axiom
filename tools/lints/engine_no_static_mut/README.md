# `engine_no_static_mut`

A [dylint] lint that bans `static mut` declarations in **non-test engine code**
— the layer crates under `crates/` (except the `xtask` tool) and the modules
under `modules/`.

### Why

`static mut` is process-global mutable state. It breaks determinism (two ticks
can observe different values), breaks reentrancy (concurrent or reentrant access
is instant UB in Rust), and hides state that should flow explicitly through the
runtime. Axiom's engine is built around explicit ownership and typed handles —
state belongs in a data structure threaded through the call graph, not in a
global slot.

A plain immutable `static` is fine: it carries no mutation hazard.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Tests may use `static mut`
  freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Macro-expanded items** — a `static mut` produced by a macro expansion is
  not flagged (`item.span.from_expansion()`).
- **Immutable statics** — `static FOO: u32 = 0;` is not flagged; only the `mut`
  qualifier triggers the lint.

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component, minus `xtask`); the `ui/modules/` and `ui/apps/`
fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

### What to do instead

Thread owned state through the runtime via a typed handle or data structure:

```rust
// Instead of:
static mut COUNTER: u32 = 0;

// Use:
struct EngineState { counter: u32 }
// and pass `EngineState` explicitly through your call graph,
// or expose it via the layer API as a typed handle.
```

[dylint]: https://github.com/trailofbits/dylint
