# `engine_require_module_docs`

A [dylint] lint that requires every `pub mod` declaration in **engine source**
(layer crates under `crates/` and modules under `modules/`) to carry a doc
comment explaining the module's role and its allowed dependencies.

### Why

Axiom is designed for agentic development: an AI agent — or a human — must be
able to read the repository cold and immediately understand what each module
owns and which layers it is allowed to depend on. A `pub mod foo {}` with no
documentation is an unannounced black box. It forces every reader to spelunk
the contents to discover what the module is for, breaking the self-describing
architecture the engine depends on.

The Module Law and the Layer Law (documented in `CLAUDE.md`) require explicit
ownership and dependency declarations. This lint mechanically enforces the
documentation half of that contract: every public engine module must carry at
least a one-liner stating its responsibility and its allowed dependencies.

### Scope (what is exempt)

- **Private modules** — `mod foo {}` without `pub`. Implementation detail;
  the caller does not expose them as part of the module's interface.
- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Test helpers don't need
  architectural documentation.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are
  composition leaves / tooling outside the engine spine.
- **Macro-generated modules** — spans from macro expansion are skipped.

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component, minus `xtask`); the `ui/modules/` and `ui/apps/`
fixture directories exercise both sides.

### What counts as a doc comment

Both placement styles are accepted:

```rust
/// Outer doc comment — placed before the `mod` keyword.
pub mod scene {}

pub mod scene {
    //! Inner doc comment — placed inside the module body.
}
```

Either a `///` outer doc comment or a `//!` inner doc comment is sufficient.
The compiler represents both as `Attribute::Parsed(AttributeKind::DocComment)`
in HIR, so both are detected by the same pattern.

### What to write

A minimal useful comment answers two questions:

1. **What does this module own?** (its responsibility)
2. **What can it import?** (its allowed layers / modules)

Example:

```rust
/// Scene graph and transform hierarchy. Depends only on the kernel and the
/// math layer; imports nothing from other modules.
pub mod scene {}
```

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
