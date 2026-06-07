# `engine_no_large_enums`

A [dylint] lint that flags `enum` declarations in **non-test engine code** —
the layer crates under `crates/` and the module crates under `modules/` — that
have more than **24 variants** (the `MAX_VARIANTS` constant in `src/lib.rs`).

### Why

A very wide flat enum is usually a sign that the type is doing too many jobs.
Large discriminant spaces make `match` arms expensive to read, write, and
exhaustively test; every branch arm the optimizer sees is work, and every
`match` arm the CPU has to speculate through is heat. Wide enums also tend to
accumulate variants over time and become change-magnets — every new variant
touches every `match` in the codebase.

The usual fixes are:

- **Sub-enums** — group semantically related variants under a common tag
  (`InputEvent::Keyboard(KeyEvent)` + `InputEvent::Mouse(MouseEvent)` instead
  of 20 flat variants).
- **A struct + smaller tag** — if the discriminant is really a type family,
  replace the enum with a struct carrying a focused tag and a typed payload.

Both approaches reduce the per-`match` arm count, keep each type focused on
one concern, and make the hierarchy explicit in the type system rather than
implicit in naming conventions.

### The limit is tunable

`MAX_VARIANTS` in `src/lib.rs` is a named constant — edit it there if the
engine's architecture evolves and a different bound is warranted. The number is
24 today: a power-of-two-adjacent value large enough to comfortably accommodate
real discriminant families (keyboard keys, input events, error kinds) while
firmly rejecting "dump everything here" enums.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Tests may define arbitrarily
  large enums for fixture purposes.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Macro-generated enums** — spans from macro expansions are skipped
  (`span.from_expansion()`); generated code is not the author's fault.

The engine/app boundary is decided by the source file path (a `crates/` or
`modules/` path component, excluding `xtask` and `axiom-zones`); the
`ui/modules/` and `ui/apps/` fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
