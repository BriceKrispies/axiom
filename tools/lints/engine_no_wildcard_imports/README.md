# `engine_no_wildcard_imports`

A [dylint] lint that bans glob (`use foo::*`) imports in **non-test engine
code** — the layer crates under `crates/` (except `xtask` and `axiom-zones`)
and the modules under `modules/`. Apps, tooling, and all test code are exempt.

### Why

Wildcard imports hide which symbols a module actually uses. In an agentic
codebase where dozens of agents read and write engine code cold, a `use foo::*`
forces every reader to mentally expand the glob to know what names are in scope.
Specific imports (`use foo::{A, B}`) make the symbol set greppable: you can
search for `A` or `B` and find every use site. Glob imports also make
unintended symbol capture invisible — a new item added to `foo` silently enters
scope everywhere the glob appears.

Axiom's engine is designed to survive agent-driven development. That requires
every name in scope to be explicitly visible, not hidden behind a glob.

This includes `pub use foo::*` re-exports: a wildcard re-export is just a
hidden glob in public clothing.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Tests may use glob imports
  freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Compiler-synthesised globs** — items whose span is `from_expansion()` (e.g.
  the implicit `std` prelude) are silently skipped.

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component, minus `xtask` and `axiom-zones`); the
`ui/modules/` and `ui/apps/` fixture directories exercise both sides.

### Example

```rust
// Banned — which types are in scope?
use std::collections::*;

// Correct — explicit, greppable
use std::collections::{BTreeMap, BTreeSet};
```

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
