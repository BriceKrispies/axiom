# Port recipe — follow this exactly

You are porting one file from `C:\dev\Claude-of-Duty` (Three.js browser FPS, ISC
licensed) into `apps/claude-of-duty` in this worktree. This file is the whole
procedure; your prompt names only the source file and the target module.

## Rules

1. **Work only in `apps/claude-of-duty/`.** Never touch `crates/` or `modules/` —
   other agents are live there. Never touch `C:\dev\axiom` itself.
2. **Never `git add -A`.** Stage only the paths you created or edited.
3. **Never** run `git reset`, `checkout`, `stash`, `clean`, `pull`, or `merge`.
4. Apps are outside the Branchless Law and the Coverage Law. Write normal,
   idiomatic Rust with real control flow. Do not contort it.
5. **Faithfulness over elegance.** Keep the source's constant values, names, and
   call order recognisable so the port can be diffed against the original. Where
   Rust forces a divergence, comment WHY at that site.
6. Put a source citation at the top of every ported module:
   `//! Ported from Claude-of-Duty `src/<path>.js:<first>-<last>`.`
7. If the source contains a defect, port the behaviour and pin it with a test
   naming it as a source quirk. Do not silently fix it. If fixing is clearly
   right, fix it, comment why, and cover it.

## Verifying a port — the golden-capture method

This is what makes a port checkable rather than merely plausible.

For any routine that is pure maths or pure data:

1. Write a tiny Node script that imports the original module, calls the routine
   over a fixed set of inputs, and prints the results as JSON.
   Run it: `node <script>.mjs` (Node 24 is on PATH; the source repo has its
   dependencies installed already).
2. Paste those captured values into a Rust test as expected constants.
3. Assert:
   - **exact equality** for anything integer-derived, or built only from
     `+ - * /` and comparisons;
   - **a stated tolerance** (`1e-12` is the established figure) where `sin`,
     `cos`, `ln`, `exp`, `pow` or `sqrt` are involved — those are not
     bit-guaranteed across libm implementations. State the tolerance and why in
     the test.
4. Delete the capture script. The committed goldens are the artifact.

Precedent: `apps/claude-of-duty/tests/core_port.rs` (commit `16fbf5d4`) pins the
RNG this way — the `u32` stream from three seeds, exact `f64` equality on
`float`/`range`/`signed`/`int`/`pick`, `1e-12` on the Box–Muller pairs.

## Determinism — do not break this

The source is fully seed-driven and that property is being preserved. Every
subsystem takes `rng.fork()` at init so its draws never perturb another
subsystem's sequence. Some streams use deliberately pinned fixed seeds so that
editing them cannot reshuffle the level. **Preserve every `fork()` call and every
literal seed exactly, in the same order.** Draw order is part of the contract: an
extra or missing draw shifts every subsequent value.

`apps/claude-of-duty/src/rng.rs` is the ported generator (xoshiro128\*\* with a
SplitMix32 expander). Use it. Do **not** substitute the kernel's
`DeterministicRng` — it is splitmix64 and produces a different sequence.

## Style

Match `apps/claude-of-duty/src/`. One concept per module. Data tables as `const`
arrays or a `struct` per entry. Prefer plain `f32`/`f64` matching the source's
precision; reach for the kernel's `Seconds`/`Meters`/`Ratio` only at boundaries
where they read naturally.

## Verify before committing

Run these **sequentially, never at the same time** — two gates at once makes
dylint report a fake `cargo metadata` error and mask the real finding:

1. `cargo test -p axiom-claude-of-duty`
2. `cargo xtask check-architecture`

Do **not** run the workspace coverage script; that runs centrally.

## Commit and report

Commit on the current branch in house style: a lowercase area prefix, then a
declarative sentence saying what is now true. Check `git log --oneline -20` for
the voice. Example: `weapons: the recoil pattern is a learnable snake, not noise`.

**Then write your detailed notes to
`docs/work-manifests/claude-of-duty-port/notes/<module>.md`** — what you ported,
what you pinned and at what tolerance, any divergence and why, anything you could
not port.

**Return at most 8 lines to the caller**: the commit hash, pass/fail for both
commands, and anything genuinely surprising. Nothing else. Detail goes in the
notes file, not the reply.
