# `engine_no_recursion`

A [dylint] lint that flags **direct (self-) recursion** in **non-test engine
code** — the layer crates under `crates/` (except the `xtask` tool) and the
modules under `modules/`. A function is flagged when its own body contains a
call (a plain call *or* a method call) that resolves back to its own `DefId`.

### Why

Unbounded recursion risks stack overflow and is non-obvious about its bound —
the recursion depth is implicit, spread across call sites, and invisible at the
function signature. Axiom forbids that on the runtime path: every loop on the
engine spine must have an explicit, inspectable bound. Rewrite a recursive
function as an explicit bounded `loop`/`while`, or as an explicit worklist/stack
you push and pop, so the bound and the working-set size are first-class and
reviewable.

```rust
// flagged:
fn count(n: u32) -> u32 {
    if n == 0 { 0 } else { count(n - 1) }
}

// instead:
fn count(mut n: u32) -> u32 {
    let mut acc = 0;
    while n != 0 { acc += 1; n -= 1; }
    acc
}
```

### Scope (what is exempt)

- **Indirect / mutual recursion is NOT detected.** This is a Tier-1 lint: it
  only finds a function that calls *itself* by walking that single function's
  body. A cycle through other functions (`a` calls `b`, `b` calls `a`) requires
  whole-program call-graph analysis and is a possible **future enhancement** —
  it is deliberately out of scope today.
- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Tests may recurse freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Macro-expanded self-calls** — a self-call a macro expanded into the body is
  not blamed on the call site (`Span::from_expansion`).

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component, with a `src` component, minus `xtask`/`axiom-zones`);
the `ui/modules/` and `ui/apps/` fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
