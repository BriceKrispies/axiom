# `engine_no_transmute`

A [dylint] lint that bans `std::mem::transmute` and `std::mem::transmute_copy`
in **non-test engine code** — the layer crates under `crates/` (except the
`xtask` tool and `axiom-zones`) and the modules under `modules/`.

### Why

`mem::transmute` is a raw memory reinterpretation with no type-system safety.
It bypasses Rust's ownership, alignment, and validity guarantees, making it
trivially easy to produce undefined behaviour. In an engine that must be
deterministic and correct across WASM targets, transmutes are a reliability and
portability hazard — endianness, padding, and ABI differences can all produce
silent corruption between native and WASM builds.

Safe alternatives exist for every common use-case:

- **Bit-level reinterpretation**: `f32::from_bits` / `f32::to_bits`,
  `u32::from_le_bytes` / `to_le_bytes`, and similar intrinsics that express
  intent precisely and remain portable.
- **Numeric widening/narrowing**: `as` casts.
- **Pod reinterpretation across types**: a reviewed `bytemuck`-style wrapper
  that upholds alignment and size contracts at the type level rather than
  silently hoping they happen to hold.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under a `#[cfg(test)]`
  module (detected via `clippy_utils::is_in_test`). Tests may transmute freely.
- **Apps and tooling** — `apps/`, `tools/`, and `crates/xtask` are composition
  leaves / tooling outside the engine spine.

There is **no zone gate**: unlike `engine_no_time_in_sim`, transmute is banned
everywhere in engine source regardless of the `axiom_zones` annotation — there
is no zone where raw reinterpretation becomes acceptable.

The engine/app boundary is decided by the source file path (a `crates` or
`modules` path component, plus a `src` component, minus `xtask` and
`axiom-zones`); the `ui/modules/` and `ui/apps/` fixture directories exercise
both sides.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
