# `engine_no_large_files`

A [dylint] lint that flags **engine source files** whose physical line count
exceeds the configured budget (`MAX_LINES = 1000`).

The limit is tunable — it lives as a named `const` at the top of `src/lib.rs`.
Adjust it there; the `ui/modules/m/src/big.rs` fixture (currently 1005 lines)
must track the change.

### Why

Axiom forbids junk-drawer files. A file over the line budget is a structural
smell: it is doing too many things, hiding relationships between items, and
making the engine harder for both humans and agents to understand. Axiom's
agentic-development rules require that every file's purpose is obvious from
its path — a large grab-bag violates that contract.

Large files also concentrate multiple responsibilities in one module, which
fights Axiom's explicit ownership and dependency-direction rules. The
kernel/layer/module structure exists precisely to give each concern its own
home. Split early.

### Scope (what is exempt)

- **Apps** (`apps/`) — composition leaves; the lint only covers the reusable
  engine spine.
- **Tooling** (`tools/`, `crates/xtask`, `crates/axiom-zones`) — outside the
  engine runtime graph.
- **Tests and integration fixtures** — the `src/` path component requirement in
  `is_engine_file` already excludes `tests/`, `benches/`, and `examples/`.

The engine/app boundary is decided by the source file path (a `crates/` or
`modules/` path component with a `src/` component, minus the tooling exclusions);
the `ui/modules/` and `ui/apps/` fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

### Fixtures

| File | Path pattern | Expected |
|------|-------------|---------|
| `ui/modules/m/src/big.rs` | engine spine, 1005 lines | **flagged** |
| `ui/modules/m/src/small.rs` | engine spine, 8 lines | silent |
| `ui/apps/a/src/app.rs` | app (non-spine), 1005 lines | silent |

[dylint]: https://github.com/trailofbits/dylint
