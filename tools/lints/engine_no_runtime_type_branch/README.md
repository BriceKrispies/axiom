# `engine_no_runtime_type_branch`

A [dylint] lint that bans runtime type reflection — `downcast_ref`,
`downcast_mut`, `downcast`, `type_id` (method calls), and `TypeId::of`
(path calls) — in **non-test engine code**: the layer crates under `crates/`
(except the `xtask` tool and the `axiom-zones` support crate) and the modules
under `modules/`.

### Why

Axiom's engine is built on a **static, deterministic data model**: every datum
has a concrete, statically-known type at the call site, and every dispatch path
is decided at compile time. `Any`/`TypeId`/`downcast` punches a hole in that
model:

- It makes control flow depend on the *runtime identity* of a type rather than
  its statically declared structure — which is exactly the hidden branching that
  makes engine code hard to reason about, test, and replay.
- A `downcast_ref` that silently returns `None` for an unexpected type can
  swallow bugs rather than surfacing them as compile-time or invariant errors.
- `TypeId` values are not stable across compiler versions or build sessions, so
  any serialisation / replay path that leaks them is non-deterministic by
  construction.

The fix is never "call the downcast more carefully" — it is to restructure so
the compiler knows all cases statically, typically via an explicit enum or a
typed dispatch table.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Tests may use reflection
  freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine. The `axiom-zones` support crate is
  also excluded.
- **Macro-generated call sites** — if the `downcast_ref` originates inside a
  macro expansion (`expr.span.from_expansion()`), it is exempt. Macro authors
  own that decision.

### Out of scope (Tier-1 limitation)

Detection of **bare `dyn Any` trait bounds** is intentionally out of scope for
this rule. This lint catches the *call sites* where the runtime type branch
actually executes. Auditing trait bounds for `Any` supertraits is a harder,
separate analysis that requires type-level reasoning.

### Running it

```sh
cargo dylint --all -- --all-targets
```

### Example — flagged

```rust
use std::any::Any;

// In a layer or module crate:
fn handle(value: &dyn Any) {
    if let Some(n) = value.downcast_ref::<u32>() { /* runtime branch */ }
}
```

```rust
use std::any::TypeId;

fn is_integer() -> bool {
    TypeId::of::<u32>() == TypeId::of::<i32>() // TypeId::of is banned
}
```

### Example — use instead

```rust
// Explicit enum: the compiler knows all variants, exhaustiveness is checked.
enum Value { Int(u32), Float(f32) }

fn handle(value: &Value) {
    match value {
        Value::Int(n)   => { /* … */ }
        Value::Float(f) => { /* … */ }
    }
}
```

[dylint]: https://github.com/trailofbits/dylint
