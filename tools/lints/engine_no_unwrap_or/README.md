# `engine_no_unwrap_or`

A [dylint] lint that bans `.unwrap_or(..)` in **non-test engine code** — the
layer crates under `crates/` (except the `xtask` tool and the `axiom-zones`
support crate) and the modules under `modules/`.

### Why

`.unwrap_or(b)` is a **value-level branch wearing a combinator's clothes**. It
selects between the carried value and a fallback on the `Option`/`Result`
discriminant, but there is no `if`/`match` in the HIR, so `engine_no_branching`
cannot see it. That makes it precisely the place the Branchless Law's pressure
escapes to: `docs/unbranching.md` recommends `cond.then_some(a).unwrap_or(b)` as
a de-branching recipe, so the spine's branches did not disappear — a large share
of them turned into `unwrap_or` call sites.

The second problem is semantic, and it is the more expensive one. The fallback is
an *eagerly evaluated* default chosen at the **use** site, which is where absence
gets papered over: a missing table entry, an out-of-range index, or a failed
lookup silently becomes `0`, `Default::default()`, or an identity value, and the
surrounding code can no longer distinguish "absent" from "genuinely this value".
A determinism bug that begins as a silent `unwrap_or(0.0)` is invisible in the
data and shows up frames later as motion.

### What to do instead

In rough order of preference — each removes the optionality rather than
defaulting it:

1. **Make the producer total.** If the lookup cannot actually fail, return the
   value, not `Option<value>`. Most spine `unwrap_or`s sit on an infallible
   lookup that returns `Option` only out of container habit.
2. **Carry the discriminant in the data contract.** If absence is meaningful,
   the consumer should be reading a field that says so — not re-deriving it from
   an `Option` and then discarding it.
3. **Push the named default down.** Where a fallback genuinely is part of the
   contract, it belongs in the layer that *defines* it, behind a method whose
   signature says what the default means (`speed_or_rest(id)`), not as an
   anonymous `0.0` at a call site in a higher layer.

### Scope (what is exempt)

- **Test code** — `#[test]` functions and anything under `#[cfg(test)]`
  (via `clippy_utils::is_in_test`), plus whole `tests/`, `benches/`, and
  `examples/` files (no `src` path component).
- **Apps and tooling** — `apps/`, `tools/`, `crates/xtask`, and the
  `axiom-zones` support crate are outside the engine spine.
- **Macro expansions** — an `unwrap_or` a macro expanded into a call site is not
  the call site's to fix.
- **The lazy siblings** — `unwrap_or_else` and `unwrap_or_default` are **not**
  flagged. They are the same shape and arguably the same problem, but banning
  them is a separate decision; this lint bans exactly what it says.

The engine/app boundary is `engine_lint_helpers::is_engine_file`, shared with
`no_unwrap_in_engine`; the `ui/` fixture directories exercise both sides.

### Why this is its own lint and not part of `engine_no_branching`

`engine_no_branching` is a **hard ban at a baseline of zero** — the Branchless
Law is *at* zero on the spine and any new finding fails the gate immediately.
Folding a rule with a large existing backlog into it would force
`engine_no_branching=<N>` into `tools/lints/dylint-baseline.txt`, which would
retire the Branchless Law's zero gate to make an unrelated rule land. A separate
lint gets its own ratchet line and can be driven down independently, leaving the
branch gate at zero. See `tools/lints/dylint-baseline.txt` for the current
allowance; regenerate the work list at any time with:

```sh
cargo dylint --lib engine_no_unwrap_or -- --all-targets
```

### Running it

```sh
cargo dylint --all -- --all-targets     # the whole rulebook
bash scripts/dylint-gate.sh             # the ratcheted gate
```

[dylint]: https://github.com/trailofbits/dylint
