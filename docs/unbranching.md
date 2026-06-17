# Unbranching the engine — execution plan & tracking

Goal: drive `engine_no_branching` to **0 findings** across all non-test code, so
the hard-ban dylint gate (baseline 0) can go green. Tests are exempt from the
lint and are **never modified**; coverage stays at **100%** throughout.

This is a long, iterative grind (run it as a `/loop`, one crate per wave). This
doc is the resumable state: the roadmap, the rewrite recipes, the per-wave gate,
and the irreducible log.

## Ground rules (every edit, every subagent)

1. **Never touch tests.** No `#[test]` fn, no `#[cfg(test)]` module. They are
   lint-exempt and keep their branches.
2. **Behavior-identical.** The rewrite must be observably the same; all existing
   tests pass unchanged.
3. **No new lint violations.** No `.unwrap()` (`no_unwrap_in_engine`), no
   recursion (`engine_no_recursion`), no new branches, etc. Use `.expect("why")`
   only where an invariant already justified it.
4. **Coverage stays 100%.** Don't introduce an arm a test can't reach (e.g. a
   `try_into` whose Err is impossible). Prefer constructs with no dead arm.
5. **Lowest correct layer.** If a branch exists because a lower layer only
   offers a branchy API (e.g. per-byte reads), add a *branchless* primitive
   there and rewrite callers against it — fixes the whole class at once.

## Rewrite recipes (the catalog)

| Construct | Branchless rewrite |
|---|---|
| `for x in it { body }` (no break/continue/?/return) | `it.for_each(\|x\| { body })` / iterator adapters (`map`/`fold`/`sum`/`collect`) |
| `for` accumulating with early-exit | `find`/`position`/`any`/`all`/`try_fold` (mind that `?` is also banned) |
| `if c { a } else { b }` (expr) | `c.then_some(a).unwrap_or(b)`, `[b, a][usize::from(c)]`, or arithmetic |
| `if c { stmt }` (side-effect, no else) | `c.then(\|\| stmt);` |
| `if let Some(x)=o {a} else {b}` | `o.map_or(b, \|x\| a)` / `o.map(\|x\| a).unwrap_or(b)` |
| `if let Ok(x)=r {..}` | `r.map(..).unwrap_or(..)` / `.ok().map_or(..)` |
| `expr?` (in `Result`/`Option` fn) | `.map(..)` / `.and_then(..)` / `.map_err(..)` chain returning the same type |
| `a && b` (b pure & always safe) | `a & b` |
| `a \|\| b` (b pure & always safe) | `a \| b` |
| per-byte `for … read_u8()?` | add `BinaryReader::read_bytes::<N>()` (branchless) in the kernel; caller becomes `reader.read_bytes::<N>().map(..)` |

**Caution on `&&`/`||`→`&`/`|`:** only when the RHS has no side effects and is
always safe to evaluate. `i < len && buf[i] == 0` must NOT become `&` (would
index out of bounds). Such guards stay as a combinator (`buf.get(i)…`).

**Irreducible (log, don't force):** exhaustive `match` on a multi-variant enum,
genuinely-unbounded `loop`/`while` with complex exit, and `?`-dense flows where
a combinator chain would violate another gate. Recursion is banned, so a loop
with no iterator form is irreducible. Record these in the log below; do not
contort the code past readability or other gates.

## Per-wave gate (orchestrator, after each crate)

```sh
cargo test -p <crate>                         # all green
cargo dylint --all -- --all-targets           # <crate> count strictly DOWN; no OTHER engine lint UP
pwsh -File scripts/coverage.ps1               # still 100% (run before committing a wave)
```
Any file that can't pass → **revert that file, log its sites as irreducible, move
on.** Commit each green wave with `git commit --no-verify` (the branching gate is
red by design until the end). Never block the loop.

## Swarm protocol

- One subagent per **file** (files are disjoint → parallel-safe edits);
  verification is serialized on the shared build.
- Hand each agent: the file path, its exact finding list (`file:line` +
  construct), this recipe table, and the ground rules. Forbid touching tests and
  any other file.
- Orchestrator runs the per-wave gate, reverts failures, commits the wave.

## Roadmap (non-test findings, target-duplicated counts — unique sites fewer)

Order: easy → hard. `xtask`/`axiom-math` last (largest + most `match`/loop math).

| Crate / module | ~count | status |
|---|---|---|
| modules/axiom-windowing | 1 | **done** (→0) |
| apps/* (rotating-cube-browser, netplay, stress-cubes) | 1–2 each | pending (apps are outside coverage gate) |
| crates/axiom-crypto | 6 | pending (needs kernel `read_bytes::<N>` primitive) |
| crates/axiom-zones | 6 | pending |
| apps/axiom-netcode-demo | 6 | pending |
| modules/axiom | 15 | pending |
| modules/axiom-render-pipeline | 16 | pending |
| crates/axiom-runtime | 18 | pending |
| modules/axiom-resources | 26 | pending |
| modules/axiom-webgpu | 26 | pending |
| tools/axiom-netcode-relay | 28 | pending |
| crates/axiom-frame | 30 | pending |
| modules/axiom-render | 39 | pending |
| apps/axiom-retro-fps-browser | 40 | pending |
| crates/axiom-ecs | 41 | pending |
| crates/axiom-kernel | 49 | pending (rewrite `BinaryReader`/`take` branchless; add bulk primitive) |
| crates/axiom-host | 57 | pending |
| crates/axiom-introspect | 59 | pending |
| apps/axiom-netcode-sim | 61 | pending |
| apps/axiom-demo-rotating-cube | 63 | pending |
| modules/axiom-netcode | 77 | pending |
| modules/axiom-scene | 86 | pending |
| crates/axiom-math | 191 | pending (heavy `for`/`match`; many → `for_each`/iterators) |
| crates/xtask | 253 | pending (tooling; lots of parsing `match`/`?`) |

## Irreducible log (append as encountered)

_Format: `path:line` — construct — why no clean branchless form._

(none yet)

## Progress log

- 2026-06-16: lint updated to exempt test code; baseline non-test count ≈1195.
- 2026-06-16: `axiom-windowing` done — `configure_surface`'s `?` → `map_err().map()` chain; tests green, coverage 100%, count 1196→1195.
