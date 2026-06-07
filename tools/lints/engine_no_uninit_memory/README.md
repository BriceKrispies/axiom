# `engine_no_uninit_memory`

A [dylint] lint that bans uninitialized and zero-initialized memory APIs in
**non-test engine code** — the layer crates under `crates/` (except the `xtask`
tool and `axiom-zones`) and the modules under `modules/`.

### What it flags

| Call form | Example |
|---|---|
| `std::mem::zeroed()` | `let s: Foo = unsafe { std::mem::zeroed() };` |
| `std::mem::uninitialized()` | `let s: Foo = unsafe { std::mem::uninitialized() };` |
| `MaybeUninit::uninit()` | `MaybeUninit::<Foo>::uninit()` |
| `MaybeUninit::zeroed()` | `MaybeUninit::<Foo>::zeroed()` |
| `MaybeUninit::uninit_array()` | `MaybeUninit::<Foo>::uninit_array()` |
| `.assume_init()` | `mu.assume_init()` |

### Why

Uninitialized memory is one of the most dangerous primitives in unsafe Rust:

- **`mem::uninitialized`** was deprecated and removed from stable because it
  trivially produces undefined behavior for most types.
- **`mem::zeroed`** is only sound when the type's bit pattern of all zeros is
  a valid, meaningful value — a constraint that is invisible at the call site
  and easy to violate silently during refactoring.
- **Direct `MaybeUninit` usage** scatters unreviewed unsafe initialization
  logic across the engine codebase, making safety audits impractical and
  fragile.

In Axiom's engine the rule is: **values are fully initialized before use**.
If a storage primitive genuinely needs uninit memory for performance (e.g. a
fixed-capacity arena), that need is encapsulated in one reviewed type — it is
not spread across engine call sites.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything detected as test context by
  `clippy_utils::is_in_test`. Tests may use unsafe memory APIs freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.
- **Macro-expanded code** — calls introduced by a macro expansion are skipped
  (`expr.span.from_expansion()`).

The engine/app boundary is decided by the source file path (a `crates` or
`modules` component with a `src` component, minus `xtask` and `axiom-zones`);
the `ui/modules/` and `ui/apps/` fixture directories exercise both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
