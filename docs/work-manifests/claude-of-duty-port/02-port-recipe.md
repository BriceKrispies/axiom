# Port recipe — follow this exactly

You are porting one file from `C:\dev\Claude-of-Duty` (Three.js browser FPS, ISC
licensed) into `apps/shmup` in this worktree. This file is the whole
procedure; your prompt names only the source file and the target module.

## Rules

1. **Work only in `apps/shmup/`.** Never touch `crates/` or `modules/` —
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
2. Have that script **write a JSON file** the test reads — see "Emit goldens to a file" below. Paste values inline only when there are few enough to check by eye.
3. Assert:
   - **exact equality** for anything integer-derived, or built only from
     `+ - * /` and comparisons;
   - **a stated tolerance** (`1e-12` is the established figure) where `sin`,
     `cos`, `ln`, `exp`, `pow` or `sqrt` are involved — those are not
     bit-guaranteed across libm implementations. State the tolerance and why in
     the test.
4. Commit the golden file. Commit the capture script too when it is small enough to be worth rereading.

Precedent: `apps/shmup/tests/core_port.rs` (commit `16fbf5d4`) pins the
RNG this way — the `u32` stream from three seeds, exact `f64` equality on
`float`/`range`/`signed`/`int`/`pick`, `1e-12` on the Box–Muller pairs.

## Determinism — do not break this

The source is fully seed-driven and that property is being preserved. Every
subsystem takes `rng.fork()` at init so its draws never perturb another
subsystem's sequence. Some streams use deliberately pinned fixed seeds so that
editing them cannot reshuffle the level. **Preserve every `fork()` call and every
literal seed exactly, in the same order.** Draw order is part of the contract: an
extra or missing draw shifts every subsequent value.

`apps/shmup/src/rng.rs` is the ported generator (xoshiro128\*\* with a
SplitMix32 expander). Use it. Do **not** substitute the kernel's
`DeterministicRng` — it is splitmix64 and produces a different sequence.

## Style

Match `apps/shmup/src/`. One concept per module. Data tables as `const`
arrays or a `struct` per entry. Prefer plain `f32`/`f64` matching the source's
precision; reach for the kernel's `Seconds`/`Meters`/`Ratio` only at boundaries
where they read naturally.

## Verify before committing

Run these **sequentially, never at the same time** — two gates at once makes
dylint report a fake `cargo metadata` error and mask the real finding:

1. `cargo test -p axiom-shmup`
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

## Emit goldens to a file — do not hand-copy them

Learned in `c2f3fbb5`: an agent hand-copied a captured array into a Rust test and
mis-grouped two values by eye. The port was correct; the *golden* was wrong. A
hand-transcribed golden is another transcription step with its own error rate, and
it fails in the worst direction — it makes correct code look broken, or (worse)
broken code look correct.

So, for anything beyond a handful of scalars:

1. Have the Node capture script **write a JSON file** next to the test
   (`tests/<slice>/golden.json`) and commit it.
2. Have the Rust test **read that file** and compare.
3. Make the capture reproducible — re-running it must produce a byte-identical
   file. Commit the script when it is small enough to be worth rereading.

Precedent: `tests/audio/capture.mjs` → `tests/audio/golden.json` (703 KB,
byte-reproducible), read by `tests/audio_port.rs`.

Pasting values inline is fine only when there are few enough to check by reading,
and even then prefer naming them so a wrong grouping is visible.

**And when a golden disagrees with the port, do not assume the port is wrong.**
Work out what the value *should* be from the algorithm before changing either side.

## Language traps that compile and are wrong

Each of these has already cost real time on this port. Check for them by name.

- **`sign` is not `signum`.** GLSL/JS `sign(0)` returns `0`; Rust `f32::signum(0.0)`
  returns `1.0` (and `-1.0` for `-0.0`). Wherever the source relies on a zero sign
  contributing nothing, hand-roll a three-valued sign. Hit twice: `physics`'s
  `box_box` SAT axis selection, and `sky`'s shader bodies.
- **Euler order is a convention, not a spelling.** Three's `'XYZ'` composes
  `qx*qy*qz`; `axiom_math::Quat::from_euler_xyz` composes `qz*qy*qx`. Different
  rotations. `Assembly::add` builds its own composition for this reason.
- **Compute in `f64`, store `f32`.** JS numbers are `f64` and Three computes in
  `f64` while storing `Float32Array`. Truncating an *angle* before `sin`/`cos` is
  worse than computing in `f64` and rounding the result. But note the inverse also
  bites: the physics BVH stores node bounds as `f32` with a `1e-5` pad, so an
  all-`f64` port diverged from the source's real bounds.
- **An enum used as a table index is order-dependent even when the lookup looks like
  a search.** Consolidating two enums with different variant orders silently
  reindexed every per-surface audio recipe. Compare orders before merging, and run
  the goldens after.
- **A matching count is not proof.** A differing vertex/triangle count is definitely
  a different algorithm — but an equal count can still hide a different weld that
  traded one merge for another.
- **Your comparator can be the bug.** A triangle-sort keyed on a 5 mm grid mispairs
  sub-5 mm repeated features (knurling, rail teeth, stipple) and reports the gap
  between neighbours as error. Before widening a tolerance, check the instrument.

When the source is GLSL held in JS strings there is no native oracle to call, so the
capture script has to re-implement it. That transcription is itself a risk — say so
in the notes, and keep the re-implementation line-by-line faithful rather than tidy.

- **Float arithmetic is not associative — do not tidy an expression.** `(a + b) + c`
  differs from `a + (b + c)` in the last bits. Folding two sequential adds, hoisting
  a common factor, or reordering a sum to read better all change the result and
  silently break bit-exactness. Transcribe the source's grouping and left-to-right
  order literally, however clumsy it looks. Caught mid-draft in `b893880d`.
- **Dead computation in the source is still part of the source.** `foliage`'s
  generator computes a value it never uses. Port it with a comment rather than
  dropping it — the judgement that it is dead can be wrong, and preserving it costs
  nothing.

## Committing when other agents are live

"Never `git add -A`" is not enough. `git commit -m "..."` with **no pathspec**
commits whatever is already in the index — including files a sibling agent staged
seconds earlier. That happened in `78e06ea3`: a metal-surfaces commit swept up
another agent's half-finished module split and put a **non-building tree on the
branch**.

So, every time:

1. `git add <your explicit paths>`
2. `git status --porcelain` — read it, and confirm nothing staged is yours-adjacent
   but not yours.
3. `git commit -m "..." -- <your explicit paths>` — pass the pathspec to `commit`
   too, so a stray staged file cannot ride along.
4. `cargo check -p axiom-shmup` **after** committing, so you notice
   immediately if your commit does not build.

If you find someone else's work already staged, **do not commit it and do not
revert it** — they are probably mid-edit. Commit only your paths and say so in your
report.
