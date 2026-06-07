# `engine_no_large_functions`

A [dylint] lint that flags engine functions whose source span exceeds
**120 lines** (the line-count budget).

### The limit

```rust
/// Maximum source lines for one engine function body.
const MAX_LINES: usize = 120;
```

This constant is **tunable** — it lives in `src/lib.rs` as a named `const` so
the orchestrator can verify the real engine spine stays under it and a future
policy change is a single-character edit.

The measurement is the function's full source span (opening `fn` through
closing `}`), counted in source lines via `SourceMap::lookup_char_pos`. A
120-line function is on the edge; 121 is flagged.

### Why

An engine function that runs for more than 120 lines is doing too many things.
It hides its responsibilities from the next reader — human or AI agent — behind
a wall of sequential logic that is hard to test in isolation, hard to reason
about, and nearly impossible to reuse. Axiom's Coverage Law requires every
branch to be reachable through a public API; a 200-line function with eight
nested conditions is a design smell, not a test problem. The fix is structural:
extract helper functions, name each responsibility, and let the compiler confirm
that every piece is exercised.

### Scope (what is exempt)

- **Closures** — measured as part of their enclosing named function; checked
  there, not separately.
- **Macro-generated code** — spans that originate from a macro expansion are
  skipped; blame the macro, not the call site.
- **Test functions** — `#[test]` functions and anything `is_in_test` recognises
  (`#[cfg(test)]` context) may be long. Setting up elaborate state for a single
  assertion is normal; the lint does not police test scaffolding.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine. Only `crates/<layer>/src/` and
  `modules/<module>/src/` files are in scope.

The engine/app boundary is decided by the source file path via
`engine_lint_helpers::is_engine_file`; the `ui/modules/` and `ui/apps/` fixture
directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

### Fixtures

| File | What it proves |
|---|---|
| `ui/modules/m/src/big.rs` | Engine fn > 120 lines → **flagged**; short fn → silent; over-limit `#[test]` fn → silent |
| `ui/apps/a/src/app.rs` | Over-limit fn in an app → **silent** (not engine spine) |

[dylint]: https://github.com/trailofbits/dylint
