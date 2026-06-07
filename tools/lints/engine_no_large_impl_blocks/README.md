# `engine_no_large_impl_blocks`

A [dylint] lint that flags `impl` blocks in **engine code** (layers under
`crates/` and modules under `modules/`) that have more than `MAX_ITEMS` (30)
associated items — methods, associated constants, and associated types combined.

### Why

An `impl` block with dozens of methods is a god-object smell. It means one type
is carrying too many responsibilities, making the code hard to reason about,
hard to test in isolation, and hard for future agents to navigate. In Axiom's
strict layered architecture, each type should own a focused, well-bounded
capability. When an impl block exceeds the limit, the right answer is to split
the behavior into focused traits or break the type into smaller, more purposeful
types — not to raise the limit.

### The limit is tunable

`MAX_ITEMS` is declared as a named `const` in `src/lib.rs`. The engine has
API-facade impls with many methods, so headroom is left at 30. If a genuinely
large facade is needed, raise the constant with a comment explaining why — but
prefer trait decomposition first.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything reachable under a
  `#[cfg(test)]` module (detected via `clippy_utils::is_in_test`). Test helpers
  may define large impl blocks freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Macro-generated impls** — impl blocks whose span comes from a macro
  expansion are exempt (`item.span.from_expansion()`).

The engine/app boundary is determined by the source file path; the `ui/modules/`
and `ui/apps/` fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

### Example

```rust
// BAD — one impl block carrying 32 unrelated methods
struct Engine;
impl Engine {
    fn init(&self) {}
    fn update(&self) {}
    fn render(&self) {}
    // ... 29 more methods ...
}
```

Use instead:

```rust
// GOOD — behavior split into focused traits
trait Lifecycle { fn init(&self); fn update(&self); }
trait Renderer  { fn render(&self); }

struct Engine;
impl Lifecycle for Engine { fn init(&self) {} fn update(&self) {} }
impl Renderer  for Engine { fn render(&self) {} }
```

[dylint]: https://github.com/trailofbits/dylint
