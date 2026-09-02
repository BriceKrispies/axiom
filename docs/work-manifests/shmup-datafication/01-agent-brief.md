# Datafication fan-out brief

*Hand this verbatim to every W2a agent. It is the datafication analogue of
`shmup-port/07-fanout-brief.md`.*

You are converting one file's near-duplicate code into a table plus a driver, in a
repo where ~34 other agents are doing the same thing at the same time.

**Read first:** `apps/axiom-shmup/src/fx/tracers.rs` — it is the whole pattern in
259 lines. Then `apps/axiom-shmup/src/audio/foley.rs:123` and `:427` for the same
pattern at a larger N.

## Overrides

1. **Do not build. Do not test.** No `cargo check`, `build`, `test`, `clippy`,
   `fmt`, `xtask`, `dylint` — no gate of any kind. Builds serialise on one target
   directory and are the thing limiting how many of you run at once. The
   orchestrator compiles everything in one integration pass.
2. **Do not run any mutating git command** — no `add`, `commit`, `reset`,
   `checkout`, `stash`, `clean`, `pull`, `merge`, `rebase`. `.git/index.lock` is
   one shared resource and several of you are live. Read-only git (`log`, `show`,
   `diff`, `status`) is fine.
3. **Do not touch `mod.rs`, `lib.rs`, `Cargo.toml`, `app.toml`, `app.json`, or
   `.gitattributes`.** Every agent would collide. If a line must be added, end
   your report with the exact line, e.g.
   `apps/axiom-shmup/src/fx/mod.rs: pub mod bursts;`.
4. **Do not write, regenerate, or edit anything under
   `apps/axiom-shmup/tests/golden/` or `apps/axiom-shmup/src/characterize/`.** The
   ledger is the oracle. An agent that regenerates the oracle it is checked
   against has proved nothing. If you believe the ledger is wrong, say so in your
   report and stop.
5. **Write only your assigned paths**: your `.rs` file(s), and your notes file
   `docs/work-manifests/shmup-datafication/notes/<slice>.md`.
6. **Your test goes *inside* your `.rs` file** in `#[cfg(test)] mod tests`, not in
   `apps/axiom-shmup/tests/`. `FxSystem::test_instance` is `#[cfg(test)]`
   (`src/fx/system.rs:732`) and is unreachable from an integration test crate,
   which is a separate crate. The repo already has 104 in-file test modules
   against 12 integration test files. An in-file test collides with nothing.

## The scope rule

> **You may only convert a recipe the frozen ledger already covers.**

Check `apps/axiom-shmup/src/characterize/probes.rs` for a probe case naming your
recipe. If there is none, **do not convert it** — leave it, name it in your
report, and it lands in W4.

This is the honest replacement for "there is no external oracle". The port's
agents could always generate a fresh oracle by running Node. You cannot generate
anything without building. So the oracle is enumerated and frozen up front, and
the scope of this wave is defined *by* the oracle rather than the other way round.
A conversion with no oracle is indistinguishable from a wrong one.

## The slot type — get this right or nothing else matters

A field in a row is one of exactly three things, and they are **not**
interchangeable:

```rust
enum Slot {
    Fixed(f64),        // a constant.        Consumes NO draw.
    Draw(f64, f64),    // one rng.range.     Consumes EXACTLY ONE draw.
    Absent,            // field not written. Consumes NO draw.
}
```

**Do not encode a constant as `Draw(v, v)`.** The driver would take a draw the
hand-written code never took, and every later effect in the frame shifts.
`shmup-promotion/00-manifest.md:466` proposes exactly that and it is wrong.

Some slots need more than three cases. `impacts.rs:284` is

```rust
s.delay = if band == 0 { 0.0 } else { fx.rng.range(0.02, if band == 1 { 0.09 } else { 0.2 }) };
```

— a skipped draw whose upper bound is band-dependent. That is three rows
(`Fixed(0.0)`, `Draw(0.02, 0.09)`, `Draw(0.02, 0.2)`), one per band. **A band is a
full row.** If your rows cannot express it, say so and leave it; do not invent a
fourth slot kind to make one file fit.

## The traps that compile and are wrong — check each BY NAME

- **RNG draw order.** The stream is shared across every subsystem in the frame.
  One extra, missing, or reordered draw shifts every later effect, silently, and
  the frame still looks plausible. Before writing a row, write out the
  hand-written block's draw sequence and check the driver reproduces it exactly.
- **An eager default consumes a draw a lazy one skipped.**
  `o.size.unwrap_or_else(|| rng.range(0.007, 0.016))` draws only when `o.size` is
  `None`. `o.size.unwrap_or(rng.range(0.007, 0.016))` draws **always**.
  Idiomatic, compiles, wrong. `impacts.rs:57, 60, 63` are all this shape, and
  `:61`/`:64` are `unwrap_or` with a *constant*, which is correct — so the two
  spellings sit on adjacent lines.
- **Per-index banding.** `for i in 0..n { let band = i % 3; … }` is not one row,
  it is three, and the bands differ in which draws they take.
- **Emit kind.** `emit_add`, `emit_lit`, `emit_mote`, `emit_view_add`,
  `emit_view_lit` are five different pools. A row that emits into the wrong one
  passes any test that only inspects `add.raw()`. The ledger digests all five; do
  not weaken your assertion to one.
- **Float arithmetic is not associative.** Moving a multiply into the table
  (`size0: 0.045 * e`) changes the last bits versus `rng.range(0.045, 0.1) * e`.
  **Do not tidy or reorder any expression while moving it.** Move the literals;
  leave the arithmetic where it stands.
- **Euler order is a convention, not a spelling.** `YXZ` vs `XYZ` vs
  `axiom_math::Quat::from_euler_xyz` compose differently. `src/world/kit/mod.rs`
  has the app's `euler_yxz_quat` for that reason. Four port bugs traced to this.
- **An enum used as a table index is order-dependent**, even when the lookup looks
  like a search. `Surface::ALL` (`src/world/palette.rs:32`) is the index basis for
  `foley::IMPACT`, `foley::STEP` and the physics per-triangle surface byte. Do not
  reorder it; state in a doc comment that your new table is indexed by it.
- **A passing test that pins the wrong thing.** A test asserting only
  `spawned() == 3` passes for a table that emits the right *count* with every
  field wrong. The ledger case is the assertion — do not substitute a weaker one,
  and **never assert a value you reasoned out**.
- **Storage width is part of the algorithm.** `ParticleSpawn` fields are `f64`;
  the raw buffer is `f32` (`src/fx/particles.rs:263`). A table field typed `f32`
  where the code computed `f64` changes results by ~1e-8, and only where that
  value is later used.
- **`sign` is not `signum`.** JS `sign(0)` is `0`; `f64::signum(0.0)` is `1.0`.
  Use `crate::jsmath`.
- **Dead computation in the source is still part of the source.** If a field is
  computed and overwritten, keep it — it may have taken a draw.
- **`Assembler::muted` consumes draws on purpose** (`src/world/assembler.rs:364`).
  A conversion that skips a muted set-piece "because it emits nothing" removes its
  draws and re-rolls the street.

## What you produce

- The `const` table. One row per variant, rows in **emission order** — the order
  fixes the RNG draw order and therefore every seed.
- The driver: one loop over the table, supplying only what the call site knows.
- A doc comment on the table naming what it is indexed by (`Surface::ALL`,
  emission order, band index) and why the order is not a preference.
- A test in the same file calling
  `probe::<area>::<case>().assert_matches(&Ledger::area("<area>"))`.
- A test for **every** conditional field the table now expresses — the
  `warmth_tints_the_streak_but_not_the_head` shape at `tracers.rs:238`.
- The source citation header (`//! Ported from Claude-of-Duty …`) preserved
  unchanged. It is attribution, not decoration.

## Tooling

- **`ax` is the front door.** Raw `grep`/`rg`/`find` is banned by the Atlas Rule.
  Use `scripts/ax q`, `ax def`, `ax refs`, `ax read`, `ax owns`.
- **`ax shape` is only on the built binary**: `target/release/ax shape <path>`
  (add `--vocab` to name the closed vocabulary). `~/.cargo/bin/ax` is stale and
  reports `unknown command 'shape'`. Do **not** rebuild `ax` — that is a build,
  and `tools/axiom-atlas/src/` has another session's live WIP in it.
- When `ax` falls short, `scripts/ax friction "<tried>" --want "<needed>"
  --verdict tool|repo` and say so in your report. Do not route around it.

## If you cannot convert it

Say so. `N = 1` is not a conversion — `docs/engine-datafication.md` §2: at `N = 1`
datafication *adds* code. If your file's variants turn out to be three genuinely
different algorithms rather than three fillings of one shape, report that and
leave it. **A wrong conversion costs far more than a skipped one.**

## Report — at most 10 lines

- files written;
- what you pinned, against which ledger case;
- the `mod.rs`/`lib.rs` lines the orchestrator must add;
- any recipe in your file with **no ledger case** (deferred to W4);
- anything genuinely surprising, and anything you could not convert.

Everything else goes in your notes file.
