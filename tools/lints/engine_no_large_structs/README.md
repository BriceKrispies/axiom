# `engine_no_large_structs`

A [dylint] lint that flags structs in **engine code** (layer crates under
`crates/` and modules under `modules/`) that exceed the field limit of
**24 fields** (the constant `MAX_FIELDS` in `src/lib.rs`).

### Why

A struct with dozens of fields is a design smell. It is doing too much, knows
too much, or has not been divided into focused sub-types. In Axiom's strict
layered architecture, god-structs leak responsibilities across boundaries, make
the data model opaque to future agents and readers, and are a reliable sign
that the surrounding abstraction needs to be decomposed. The field limit is a
forcing function: if you think you need more fields, restructure the data model
first.

The limit is intentionally **tunable** — the named constant `MAX_FIELDS` can
be raised if the engine genuinely requires a larger default, but prefer
sub-struct decomposition before reaching for that option.

### Scope (what is exempt)

- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves and repo tooling outside the engine spine; they are never flagged.
- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`) are exempt; test helpers
  may define whatever data shapes they need.
- **Macro-generated code** — structs whose span originates inside a macro
  expansion are skipped (`from_expansion()` check).
- **Unit structs** and structs with zero fields have 0 fields and trivially
  pass (the check is `n > MAX_FIELDS`).

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component, minus `xtask` and `axiom-zones`); the `ui/modules/`
and `ui/apps/` fixture directories exercise both sides.

### Tuning the limit

`MAX_FIELDS` lives at the top of `src/lib.rs`:

```rust
const MAX_FIELDS: usize = 24;
```

Change the value there and update the `ui/modules/m/src/big.rs` fixture
(the flagged struct must have `MAX_FIELDS + 2` fields) and its `.stderr`
golden file to keep the UI tests green.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
