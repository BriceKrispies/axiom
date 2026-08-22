# Fan-out brief — read this, then `02-port-recipe.md`, then `06-parallel-port-plan.md`

This is the standing brief for a porting agent in the parallel fan-out. It
**overrides** the parts of `02-port-recipe.md` it names, and nothing else.

## Overrides

1. **Do not build. Do not test.** No `cargo check`, `cargo build`, `cargo test`,
   `cargo xtask`, `cargo clippy`, `cargo fmt`, no gate of any kind. Builds
   serialise on one target directory and are the thing that limits how many of
   you can run at once. The orchestrator compiles everything in one integration
   pass afterwards.
2. **Do not commit. Do not stage. Do not run any mutating git command**
   (`add`, `commit`, `reset`, `checkout`, `stash`, `clean`, `pull`, `merge`,
   `rebase`). Read-only git (`git log`, `git show`, `git diff`) is fine. The
   orchestrator commits each wave with an explicit pathspec. This replaces the
   recipe's commit section entirely — several of you are live at once and
   `.git/index.lock` is a single shared resource.
3. **Do not touch `mod.rs`, `lib.rs`, `Cargo.toml`, or `app.toml`.** They are
   shared and every agent would collide on them. Instead, end your report with
   the exact lines that need adding, e.g. `apps/shmup/src/ai/mod.rs: pub mod parts;`.
   The orchestrator wires them.
4. **Write only the paths you were assigned.** Those are: your `.rs` module
   file(s) under `apps/shmup/src/`, your test file `apps/shmup/tests/<slice>_port.rs`,
   your golden directory `apps/shmup/tests/<slice>/` (capture script + JSON), and
   your notes file `docs/work-manifests/shmup-port/notes/<slice>.md`. Nothing else.
   If you believe something outside that set must change, say so in your report
   instead of doing it.

## Everything else in `02-port-recipe.md` still binds

In particular, and non-negotiably:

- **Ship a golden captured by running the original JavaScript.** Node 24 is on
  PATH and `C:/dev/Claude-of-Duty` has its dependencies installed. Your capture
  script writes `apps/shmup/tests/<slice>/golden.json`; your Rust test reads that
  file. Never hand-copy captured values into Rust. Never assert a value you
  reasoned out yourself — every expected number comes from running the original.
- Where the source is GLSL held in a JS string there is no oracle, so the capture
  script must hand-transcribe the shader independently. Say so in the notes and
  keep the transcription line-by-line faithful. Precedent: `tests/sky/capture.mjs`.
- **Ship the Rust test even though it will not run until integration.** It will
  not get written later.
- Faithfulness over elegance; cite the source at the top of every module
  (`//! Ported from Claude-of-Duty \`src/<path>.js:<first>-<last>\`.`).
- Preserve every `rng.fork()` and every literal seed, in order.
- Port source defects faithfully and pin them with a test naming the quirk.

## The traps that compile and are wrong

Check each **by name** before you finish. Full text in
`06-parallel-port-plan.md`; the short list:

`Float32Array` storage width · `sign` is not `signum` · Euler order is a
convention (`'YXZ'` vs `'XYZ'` vs `Quat::from_euler_xyz`) · matrix storage order
(column-major) · float arithmetic is not associative, do not tidy an expression ·
an enum used as a table index is order-dependent · `Math.hypot` is not
`sqrt(x*x+y*y+z*z)` · a matching count is not proof · your comparator can be the
bug · dead computation in the source is still part of the source.

## `C:/dev/Claude-of-Duty` is read-only

Never modify it. Write your capture script under `apps/shmup/tests/<slice>/` and
import the source from there by relative path.

## Report

At most 10 lines to the caller:

- the files you wrote,
- what you pinned and at what tolerance,
- the `mod.rs`/`lib.rs`/`Cargo.toml` lines the orchestrator must add,
- anything genuinely surprising, and anything you could not port.

Everything else goes in your notes file.
