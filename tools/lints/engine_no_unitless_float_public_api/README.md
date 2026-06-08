# `engine_no_unitless_float_public_api`

A [dylint] lint that bans a naked `f32` / `f64` on the **public surface** of
engine code — the layer crates under `crates/` (except the `xtask` tool) and the
modules under `modules/`.

Three surfaces are checked:

1. the parameter and return types of a `pub fn` (free function),
2. the same for a `pub` method in an **inherent** `impl` block (constructors and
   accessors like `Vec3::new` / `fn distance(self) -> f32`), and
3. a `pub` field of a `pub` struct.

A single layer of reference (`&f32`, `&mut f64`) is peeled before the check.
Methods of a **trait** `impl` are skipped — their signature is the trait's
contract, not a free choice of this crate.

### Why

A bare float carries no unit. `set_speed(speed: f32)` does not say whether
`speed` is meters per second, units per tick, or degrees per frame — the caller
has to guess, and a wrong guess compiles cleanly and produces a silent physics
bug. Axiom prefers quantity newtypes — the kernel's `Meters`, `Radians`, `Ratio`
(and domain types like a light's `Intensity` at their owning layer) — so the
unit is part of the type: the compiler rejects mismatched units, the public API
documents itself, and a future agent reading the signature cannot misread the
contract.

```rust
pub fn set_speed(speed: f32) {}        // unitless — what unit is `speed`?
pub fn area() -> f64 { 0.0 }           // unitless return
pub struct Body { pub mass: f32 }      // unitless public field
```

Use instead:

```rust
pub fn set_speed(speed: MetersPerSecond) {}
pub fn area() -> SquareMeters { SquareMeters(0.0) }
pub struct Body { pub mass: Kilograms }
```

### Scope (what is exempt)

- **Private items** — a private `fn`, or a private field of a public struct, may
  use bare floats freely. Only the public surface other crates and apps build
  against is constrained (visibility via `cx.tcx.visibility(..).is_public()`).
- **Function bodies and local variables** — out of scope; this lint only sees
  signatures and struct fields, not internals.
- **Non-float types** — `u32`, `i64`, etc. are untouched.
- **Test code** — `#[test]` functions and `#[cfg(test)]` modules
  (`clippy_utils::is_in_test`).
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **A quantity newtype's own boundary** — the inherent methods of a struct that
  is itself a single `f32`/`f64` field (e.g. `Pixels(f32)`, `Angle`) are skipped.
  That type's `new(f32)` / `get() -> f32` are where a raw scalar enters and leaves
  the quantity — the boundary, not a unitless leak (the same shape as the kernel's
  `Ratio::new`/`get`). A *multi*-field struct is not a newtype, so a float method
  on it is still flagged.
- **The scalar-floor crates** — `axiom-kernel` and `axiom-math` are skipped
  entirely. The kernel owns the dimensioned-scalar primitives themselves (their
  constructors take a raw `f32` by definition) plus serialization (`write_f32`)
  and telemetry; `axiom-math` is the dimensionless linear-algebra layer
  (`Vec3::new`, `dot`, `length` are dimensionless by construction). A raw `f32`
  is the *correct* type there, not a missing unit. This is the rule being
  precise about where raw scalars belong, not an exemption to dodge it.

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component with a `src`, minus `xtask`); the floor is decided by an
`axiom-kernel` / `axiom-math` crate-dir component. The `ui/modules/`, `ui/apps/`,
and `ui/crates/axiom-{kernel,math}/` fixture directories exercise every side.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
