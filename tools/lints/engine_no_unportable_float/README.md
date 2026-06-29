# `engine_no_unportable_float`

A [dylint] lint that locks Axiom's cross-target float-determinism invariant
(SPEC-10 §17.6): inside the **deterministic step path** the only float ops
allowed are `{+, -, *, /, sqrt, min, max}` — the subset that wasm32 and the
native SSE2/NEON backends round identically and that Rust/LLVM never fuses
without fast-math.

It flags, in non-test step-path code:

- **Fused multiply-add** — `f32::mul_add` / `f64::mul_add` and the
  `core::intrinsics` FMA intrinsics (`fmaf32`, `fmuladdf64`, ...). `a.mul_add(b, c)`
  rounds *once* and lowers to a hardware FMA on some targets and a software
  polyfill on others — a different bit-pattern from `a * b + c`.
- **Fast-math / algebraic intrinsics** — `fadd_fast`, `fmul_fast`,
  `fdiv_algebraic`, ... — which LLVM is licensed to reassociate or contract.
- **Transcendentals** — `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`,
  `sin_cos`, the hyperbolics, `exp`, `exp2`, `exp_m1`, `ln`, `ln_1p`, `log`,
  `log2`, `log10`, `cbrt`, `hypot`, `powf`, `powi` — and their
  `core::intrinsics` equivalents. Each platform serves these from its own
  `libm`, which is not bit-identical.

`sqrt` is **allowed**: IEEE-754 mandates a correctly-rounded result, so it is
bit-identical everywhere.

### Why

A recorded simulation tick must replay byte-for-byte on a server, a desktop, and
a browser. FMA fusion and `libm` transcendentals are the two places ordinary
float code silently diverges across targets. This lint is the float-determinism
sibling of [`engine_no_time_in_sim`]: the same step path that may not read the
wall clock may not perform an unportable float op.

### Scope — the deterministic step path

The lint fires only where the determinism invariant must hold, so authoring-time
math is untouched:

1. **The `axiom-physics` module.** Its entire non-test source is the per-step
   spine — it carries no authoring-time trig — so the whole crate is held to the
   invariant, even without a marker. (`STEP_PATH_CRATES` in `src/lib.rs`.)
2. **Any `#[sim]` zone**, detected via the `axiom_zones`-injected
   `const __engine_zone_sim` marker, exactly like `engine_no_time_in_sim`. This
   is the generalizable form: when other simulation code adopts `#[sim]`, it
   inherits the float-determinism ban for free.

Everything else is out of scope by design: `Quat::from_axis_angle`'s `sin`/`cos`
in `axiom-math`, mesh generation in `axiom-resources`, easing curves in
`axiom-tween`, and view-cone trig in `axiom-perception` all run once at setup,
not per replayed step.

### How float method calls are disambiguated

`.log()` is also a logging method (`KernelApi::log`), and a user type may define
its own `.sin()`. To avoid false positives, a method call is flagged only when
its **receiver type is `f32`/`f64`** (read from `typeck_results`); a free-function
or UFCS call (`f32::sin(x)`, `core::intrinsics::fmaf32(..)`) is matched on its
resolved def-path. Test code, non-engine paths (`apps/`, `xtask`, `axiom-zones`),
and macro expansions are exempt — the same scoping as the rest of the rulebook.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
[`engine_no_time_in_sim`]: ../engine_no_time_in_sim/README.md
