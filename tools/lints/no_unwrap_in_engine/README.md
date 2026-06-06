# `no_unwrap_in_engine`

A [dylint] lint that bans `.unwrap()` (and `unwrap_err` / `unwrap_unchecked`) in
**non-test engine code** — the layer crates under `crates/` (except the `xtask`
tool) and the modules under `modules/`.

### Why

`.unwrap()` is an *undocumented* panic. Axiom's engine handles failure
explicitly through its kernel result/error types; an unannounced panic on the
hot path is a determinism and robustness hazard, and it hides which invariants
the code depends on. `.expect("<why it can't fail>")` is the sanctioned escape
hatch for a genuinely-impossible case — it documents the invariant at the call
site (the same spirit as a deliberate `unreachable!`).

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Tests may unwrap freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Non-panicking combinators** — `unwrap_or`, `unwrap_or_else`,
  `unwrap_or_default` are fine (they don't panic).
- **`.expect("…")`** — the documented escape hatch.

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component, minus `xtask`); the `ui/modules/` and `ui/apps/`
fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

When this lint was added the engine had **zero** non-test `.unwrap()`, so it is
pure regression prevention — it keeps the clean state from drifting.

[dylint]: https://github.com/trailofbits/dylint
