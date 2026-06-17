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

Dominant class: **exhaustive `match` on a multi-variant enum** (variant→code,
variant→serialization, variant extraction). These have no Option/Result
combinator form and are the expected residue.

- `modules/axiom-render/src/render_command.rs` — `kind_code` + 6 `as_*` extractors — exhaustive `RenderCommand` enum match (centralized so callers stay branchless).
- `modules/axiom-render/src/render_receipt.rs:72` — `write_command` — per-variant serialization, exhaustive enum match.
- `modules/axiom-webgpu/src/gpu_command.rs` — `kind_code` — exhaustive `GpuCommand` match.
- `modules/axiom-webgpu/src/webgpu_backend_state.rs` — `kind`/`submission_status`/`presentation_request` — exhaustive enum matches (one is a `const fn`, so no closures).
- `modules/axiom-webgpu/src/gpu_submission_status.rs` + `webgpu_api.rs` + `gpu_submission_report.rs` — `matches!` single-arm enum predicates.
- `crates/axiom-introspect/src/frame_report.rs` — `lifecycle_to_u8`/`lifecycle_from_u8` (enum↔code) + `read_from` `?`-dense sequential decode.
- `crates/axiom-introspect/src/metric_report.rs:80` — `match read_u8` value-tag dispatch selecting a u64 vs f32 read.
- `crates/xtask/src/class_check.rs`, `violation.rs`, `cargo_metadata.rs` — exhaustive `PackageClass`/`ViolationKind`/`DepValue` enum matches (tooling).

### VERDICT (after re-attacking the punted sites in wave 3)

Most originally-logged "irreducibles" were **not** — they were agent conservatism.
Reduced via: fieldless enum→code (`self as u8` / `const TABLE[self as usize]`),
int→fieldless enum (`const VARIANTS` table + `.get().ok_or()`), `matches!`/predicate
(discriminant `==`), `?`-chains (`and_then`), value-tag dispatch
(`(tag==A).then(||..).or_else(..)`), async loops (`tokio-stream` `for_each`).

The **genuine** floor is ONE class: **destructuring a data-carrying enum variant**
(reading variant X's payload) and **reading a data-carrying enum's integer
discriminant**. Safe Rust has no combinator for these — it's exactly what
`match`/`if let` exist for; only `unsafe` or a type-representation change removes
them. Genuine sites (~30): `render_command.rs` (kind_code + as_* extractors),
`render_receipt.rs` write_command, `gpu_command.rs` kind_code,
`webgpu_backend_state.rs`, `net_message.rs` peer/signature/signed_bytes,
`session.rs` admit, kernel `log_field.rs`/`metric_value.rs` accessors,
`metric_report.rs` write_to, `axiom/scene_commands.rs`, `axiom-zones/lib.rs`
`syn::Item` destructure, netcode-sim `cheat.rs`/`lib.rs` enum dispatch.

To reach literal 0 either: (a) **escape-hatch** these (reverse "zero exemptions"
for data-carrying destructures), or (b) **reshape the data contracts** — hoist
common fields out of `NetMessage`, store command kind as a field, replace
command/message enums with tagged structs. (b) is a real, sizable contract
change needing sign-off.

## Progress log

- 2026-06-16: lint updated to exempt test code; baseline non-test count ≈1195.
- 2026-06-16: lint also exempts `tests/`/`examples/`/`benches/` files; true target ≈924.
- 2026-06-16: `axiom-windowing` done (count→ 924 region of the curve).
- 2026-06-17: **wave 1** — swarm rewrote axiom-math, axiom-ecs, axiom-host,
  axiom-introspect, axiom-frame, axiom-runtime, axiom-render, axiom-resources,
  axiom-webgpu, xtask. **924 → 393** (531 removed). Workspace green, all tests
  pass (none modified), coverage 100%, no other lint above baseline. Commit
  `807682c` (--no-verify). Remaining 393 = not-yet-done crates (scene, netcode,
  kernel, render-pipeline, axiom umbrella, the apps, relay) + the irreducible
  enum matches above.
- Note: an unrelated, pre-existing uncommitted retro_fps "agent-bridge" feature
  (`apps/axiom-retro-fps-browser/src/agent.rs`, `bin/`, Cargo.toml/lock) sits in the
  tree; left untouched, not part of any unbranching commit.
