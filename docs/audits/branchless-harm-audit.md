# Branchless Enforcement — Harm Audit

_Repo-wide architectural audit of Axiom's `engine_no_branching` enforcement
system: where the Branchless Law protects the engine, and where it is doing more
harm than good._

**Date:** 2026-07-04
**Method:** seven parallel subagent investigations (enforcement surface, overreach,
code-quality, safety/correctness, test-harm, performance-reality, architecture-
alignment), each reading real source, cross-checked and spot-verified by the lead.
**Deliverable:** this file. No engine code was changed by the audit.

---

## 1. Executive summary

Axiom's Branchless Law bans **all** control flow (`if`/`else`, `match`,
`for`/`while`/`loop`, `&&`/`||`, `?`, `if let`/`while let`) in non-test code across
every layer (`crates/*`) and module (`modules/*`). It is enforced by a single HIR
dylint, `engine_no_branching`, at baseline 0, and framed in CLAUDE.md as "an
invariant, on the same footing as the Layer Law, the Module Law, and the Coverage
Law."

The conversion was executed with genuine discipline. The dedicated
safety/correctness pass found **no Critical correctness, safety, or determinism
bug**: the pervasive `[a, b][cond as usize]` eager-both-arms idiom is, in every
traced instance, either provably discarding a panic-free arm or selecting a pure
value; validation still returns `Result`/`Option`; decode paths still return
`InvalidDiscriminant`/`OutOfBounds` on bad input; the physics world still commits
only finite state via atomic rollback. That is the headline good news, and it is
why nothing here is rated Critical.

The harm is **architectural, not correctness-level**, and it is systemic in one
specific form: to reach literal zero, the project **reshaped correctness-bearing
data contracts from Rust enums into type-unsafe tagged structs** — in the kernel
(`MetricValue`, `FieldValue`), the render module (`RenderCommand`, `GpuCommand`,
`WebGpuBackendState`), and the signed multiplayer wire frame (`NetMessage`). Each
trades away the three things a Rust enum buys — illegal states unrepresentable,
exhaustiveness-checked consumers, minimal size — to delete a `match`. This is the
inverted priority the audit charter warns about: a stylistic discipline (tier 4)
overriding correctness and ownership (tiers 1–2). It also opened one **latent
totality hole** (a write path that is no longer statically total) and forced a
likely **performance regression in the engine's hottest loop** (the per-pixel
rasterizer), both defended only by unmeasured comments.

Two structural weaknesses compound it: (a) the lint has **zero hot/cold
granularity** — a once-per-load manifest parser is held to the same bar as a
per-tick math kernel, even though the `#[hot_path]`/`#[sim]`/`#[strict]` zone
vocabulary that could scope it already exists and is wired into *other* lints; and
(b) the gate **runs only in a local, `--no-verify`-bypassable pre-commit hook and
is absent from CI**.

**Counts:** Critical 0 · High 5 · Medium 5 · Low 6.

The recommendation is not to weaken or delete branchless enforcement. It is to
**re-scope it to the zones where it earns its keep** (hot/deterministic inner
loops, where it is currently exemplary), **stop it from overriding correctness**
(revert the enum reshapes; allow a documented discriminant-read/variant-destructure
via the existing escape-hatch mechanism), and **wire the gate into CI**.

---

## 2. Exact definition: "branchless doing more harm than good"

Branchless enforcement is **valuable** when it protects deterministic behavior,
replayability, portable Rust/WASM execution, predictable per-tick math, stable
simulation logic, or agent-readable straight-line code. Where it does that — the
math crate's per-frame vector/quaternion/matrix work, the physics contact tables,
the per-pixel raster selects, the sim-core state-transition matrices — it should
**not** be weakened (see §6).

Branchless enforcement is **harmful**, for this audit, precisely when it causes any
of:

1. Code becomes harder to understand than a straightforward branch.
2. Clever boolean masks, arithmetic tricks, lookup tables, sentinel values, macro
   indirection, or bit manipulation stand in where normal control flow is safer.
3. Error handling is weakened to avoid a branch (Result/Option collapsed, finite/
   bounds/zero checks softened, sentinel returned instead of explicit failure).
4. Tests prove implementation *shape* (or merely "does not panic") instead of
   behavior.
5. The rule blocks harmless code in tests, tools, harnesses, docs, or non-hot
   orchestration.
6. The rule forces duplicate code, awkward APIs, leaky abstractions, needless
   public surface, or misplaced helpers.
7. Performance is worse because the branchless form defeats compiler optimization,
   adds memory traffic, or forces eager evaluation of work a branch would skip.
8. The code drifts away from idiomatic Rust in a way a future agent is more likely
   to break.
9. The enforcement is too broad — it treats kernel/math/sim hot paths the same as
   apps, tools, harnesses, docs, cold setup, and one-time validation.
10. The enforcement has loopholes that satisfy the checker without preserving its
    spirit.
11. The rule overrides a higher Axiom law: determinism, layer/module ownership,
    explicit data contracts, checked failure, tests proving behavior.

The correct priority hierarchy this audit measures against:
**correctness & determinism first → clear ownership second → tests proving behavior
third → performance-with-evidence fourth → branchlessness only where it supports
those goals.**

---

## 3. Inventory of branchless enforcement mechanisms

| # | Mechanism | File(s) | What it does | Where it runs | Scope |
|---|-----------|---------|--------------|---------------|-------|
| 1 | **`engine_no_branching` dylint** | `tools/lints/engine_no_branching/src/lib.rs` | HIR `LateLintPass`; `branch_message` (lib.rs:52–79) classifies `ExprKind::If`/`Loop{Loop,While,ForLoop}`/`Match{Normal,TryDesugar}`/`Binary{And,Or}`; skips `.await` desugaring, macro-internal expansions, and the synthetic `if` inside a `while`. | `cargo dylint --all` | `outside_branchless_spine` (lib.rs:89–103) exempts any path containing a `tests`/`examples`/`benches`/`apps`/`games`/`tools`/`xtask` segment; `is_in_test` exempts inline `#[test]`/`#[cfg(test)]`. **No zone gate, no escape hatch.** |
| 2 | **Pre-commit hook** | `.git/hooks/pre-commit:37–46` | Runs `cargo dylint --all`, parses `#[warn(engine_…)]` counts, fails if any lint exceeds its baseline. | Local `git commit` | Bypassable with `--no-verify` (which `docs/unbranching.md` explicitly sanctioned for the whole campaign). |
| 3 | **Dylint baseline** | `.git/hooks/dylint-baseline.txt` (`engine_no_branching=0`) | The ratchet threshold. | Read by hook #2 | **Untracked and per-worktree** — each `git worktree` has its own copy (verified: two stale worktree copies still read `15`/`17`). |
| 4 | **CLAUDE.md — Branchless Law** | `CLAUDE.md` ("The Axiom Branchless Law") | Prose statement; declares co-equal-invariant status and baseline 0; also contains the correct subordination clause ("a branch is often a symptom… raise it and reshape"). | Human/agent guidance | Whole spine. |
| 5 | **`docs/unbranching.md`** | `docs/unbranching.md` | Campaign plan, rewrite-recipe catalog, irreducible log, milestone record. Documents the enum→tagged-struct reshape decision (lines 126–164). | Human/agent guidance | Whole spine. |
| 6 | **TS analogue (branch ban)** | `packages/axiom-client/.oxlintrc.json:46–60` (+ axiom-game) | Oxlint `no-restricted-syntax` bans `if`/ternary/`switch`/`for`/`while`/`&&`/`||`/`??`/`?.` in non-test TS. | **Pre-commit AND CI** (`scripts/ts-gate.sh`) | SDK `src/`, with documented test + platform-edge carve-outs. |
| 7 | **Adjacent zone lints** | `engine_no_time_in_sim`, `engine_no_runtime_type_branch`, etc. | Zone-scoped structural lints; prove the `#[sim]`/`#[hot_path]` marker plumbing works and is used elsewhere. | `cargo dylint` | Zone-marked code — **the granularity `engine_no_branching` declines to use.** |

**Critical structural facts about the surface:**

- **The Rust branch gate is not in CI.** `.github/workflows/ci.yml` runs
  `cargo xtask check-architecture`, the coverage gate, and the TS gate — there is
  **no `cargo dylint` step.** The Branchless Law's only mechanical enforcement is
  the local hook (#2), which is `--no-verify`-bypassable. (The TS branch ban, #6,
  *is* in CI — the Rust one is not.)
- **The lint's only scoping axis is the directory.** It cannot distinguish a hot
  per-tick loop from cold one-shot code within the spine.
- `README.md` still describes the lint as firing inside apps/tools; the 2026-07-04
  baseline flip scoped those off. Doc is stale.

---

## 4. Findings by severity

### CRITICAL — 0

No finding rises to Critical. The dedicated safety/correctness pass, plus lead
verification of the two most-suspicious sites (`metric_report::write_to` and the
rasterizer inner loop), confirmed no live correctness, safety, or determinism bug:
decode paths still return checked errors, the physics world still gates commits on
`is_finite`, and every eager-both-arms select traced discards a panic-free arm.
**This is a genuine positive result and is recorded as such** — the conversion did
not, in the audited surface, trade correctness for branchlessness at runtime. The
harm is structural (High) and scope/robustness (Medium), below.

---

### HIGH

#### H1 — Data contracts distorted enum → type-unsafe tagged struct to dodge `match`
- **Files/symbols:**
  - `crates/axiom-kernel/src/metric_value.rs:15–20` — `MetricValue`
  - `crates/axiom-kernel/src/log_field.rs:11–18` — `FieldValue`
  - `modules/axiom-render/src/render_command.rs:18–31` — `RenderCommand`
  - plus (per `docs/unbranching.md:144–152`) `GpuCommand`, `WebGpuBackendState`,
    `NetMessage` (`modules/axiom-netcode/src/net_message.rs:52–69`), `NodeComponent`.
- **Current behavior:** each is a natural data-carrying enum re-expressed as a
  struct with a `kind` discriminant that carries **every variant's fields at once**,
  with inactive fields pinned to a `DEFAULT`/sentinel (`render_command.rs:45–57`;
  `metric_value.rs:27–42`; `log_field.rs:26–64` carries `i64v,u64v,boolv,strv`
  simultaneously; `net_message.rs:29–36` rides `absent_command()`/`ABSENT_HASH`
  placeholders). Payload extraction is a set of `kind`-gated `as_*` accessors
  instead of one `match`. Each file's own doc comment names the motive verbatim:
  *"Represented as a tagged struct rather than an enum so payload extraction is
  branchless."*
- **Why harmful:** (charter #2, #6, #11) A Rust enum is a self-describing,
  exhaustiveness-checked, minimally-sized contract — illegal states are
  unrepresentable, adding a variant breaks incomplete consumers at compile time,
  and each value costs only its own payload. The tagged struct trades **all three**
  away: any `kind` byte outside the constructor range is representable, consumers
  silently accept unknown kinds, and every `RenderCommand` (even a 16-byte
  `ClearFrame`) now carries ~200 bytes of dead sentinel fields (three `Mat4`s +
  five ids). Correctness of the derived `PartialEq` now depends on a
  **hand-maintained invariant** ("inactive fields equal their default") that the
  enum enforced for free. This lands **in the kernel** and **in the signed netcode
  wire frame** — the two contracts where type-safety matters most. It is the exact
  "distort a data contract to dodge a match" anti-pattern.
- **Rule disposition:** **narrow.** The Branchless Law should treat a documented
  data-carrying-enum discriminant-read / variant-destructure as an *allowed
  residue* (via the existing `#[escape_hatch(reason=…)]`/zone mechanism —
  `docs/unbranching.md:138` option (a), which the project rejected in favor of the
  reshape).
- **Smallest correct fix:** revert these six types to enums; grant their
  destructure/discriminant-read sites the escape hatch. The enums, not the gate,
  are the correct shape.
- **Belongs in:** Kernel + Module (the type reverts) and Tooling (teach the lint
  to accept the escape hatch on these sites).

#### H2 — Reshape opened a latent totality hole in kernel telemetry serialization
- **File/symbol:** `crates/axiom-introspect/src/metric_report.rs:62–74`,
  `MetricReport::write_to`.
- **Current behavior:** `self.value.as_integer().map(…).or_else(|| …as_float()…).
  unwrap_or(())`. When `MetricValue` was an enum, this was a compiler-guaranteed
  **total** `match` — every value produced a tag byte. After H1, `MetricValue.kind`
  is a `u8`; if it ever holds a value other than `KIND_INTEGER`/`KIND_FLOAT`, both
  accessors return `None` and `.unwrap_or(())` **writes no tag byte at all**,
  silently emitting a truncated/corrupt frame the reader (`:88–100`) then
  mis-parses.
- **Why harmful:** (charter #3, #11) a *statically total* operation became
  *silently lossy*. **Verified latent, not live:** `kind` is only ever set to 0/1
  by the two `pub const fn` constructors, and the decode side also routes through
  them, so the hole is unreachable today. But the compiler no longer prevents an
  incomplete writer — the safety net that made "add a third kind" a compile error
  is gone, replaced by a silent data-corruption path. That regression is the direct
  cost of H1.
- **Rule disposition:** resolved by reverting H1 (restores exhaustiveness). If the
  struct is kept, the write **must fail loudly**, not `.unwrap_or(())`.
- **Smallest correct fix:** revert `MetricValue` to an enum (H1) so `write_to`'s
  `match` is total again.
- **Belongs in:** Kernel.

#### H3 — Branchless idiom in the engine's hottest loop likely regresses performance, unmeasured
- **File/symbol:** `modules/axiom-canvas2d-backend/src/software_rasterizer.rs:357–393`,
  per-pixel inner `for_each`.
- **Current behavior:** for **every** candidate pixel (including depth-rejected
  ones): (a) `depth[idx] = [cur, dep][pass as usize]` — an **unconditional**
  depth-buffer store that writes the old value back on a failing test (extra write
  traffic per pixel); (b) the 4-channel src-over composite `blended[…]` — four
  float multiply-adds, rounding, `as u8` casts, **plus a read-back of
  `rgba[off..off+4]`** — is computed **eagerly** and then discarded via
  `[old, blended][wi]`. Under any overdraw, most fragments fail the depth test, so
  the composite + destination read are done for pixels a branchy rasterizer would
  skip entirely. The module header (`:9–20`) markets this: "a covered or rejected
  fragment costs the same."
- **Why harmful:** (charter #7, #9) making rejected fragments cost the *same* as
  covered ones is precisely the wrong property for a rasterizer — the point of an
  early depth test is to make rejected fragments **cheap**. There is **zero
  measurement**: no `criterion`, no `#[bench]`, no native rasterizer microbenchmark
  exists anywhere in the repo; `axiom-render-bench` reports whole-frame browser FPS
  and cannot isolate this. The engine's hottest loop was shaped by an unmeasured
  comment.
- **Rule disposition:** **narrow / measure.** This is the one place the law should
  yield to a benchmark: build an overdraw-heavy fixed scene, measure the current
  form against a branchy reference (skip composite + dst-read + depth store on
  `!pass`). If the branch wins (near-certain under overdraw), grant a
  `#[hot_path]`-scoped, measured escape.
- **Smallest correct fix:** add the native rasterizer microbenchmark; on the
  expected result, make the composite/depth-store conditional under a documented
  escape hatch.
- **Belongs in:** Test/Harness (the benchmark) then Module (the conditional write).

#### H4 — The Branchless Law is mis-positioned as a co-equal, zero-tolerance invariant
- **Files:** `CLAUDE.md` ("The Axiom Branchless Law": *"an invariant, on the same
  footing as the Layer Law, the Module Law, and the Coverage Law"* + baseline 0);
  `docs/unbranching.md:3–5` (operational goal = *"drive `engine_no_branching` to 0…
  so the hard-ban gate can go green"*).
- **Current behavior:** a stylistic/structural discipline is tiered equal to
  correctness, determinism, and ownership, and enforced at baseline 0 with **no
  escape hatch** (the lint doc: "no escape hatch"). When the prose subordination
  clause ("removing a branch that harms the design is a design signal — reshape")
  and the mechanical baseline-0 gate disagreed, the **gate won**: agents reshaped
  the *contracts* (H1) to keep the *gate* green, rather than relaxing the gate.
  Corroborating symptom: `#[escape_hatch]` has **zero uses** anywhere in the spine
  (grep: only its own definition, tests, and docs) — the sanctioned pressure valve
  was left unused *and* the correctness-bearing enums were sacrificed instead.
- **Why harmful:** (charter #11) inverts the correct hierarchy — branchlessness
  (tier 4) overrode correctness/ownership (tiers 1–2).
- **Rule disposition:** **narrow the positioning.** Branchlessness is a strong
  default that **auto-yields** whenever removing a branch would weaken a
  correctness, determinism, or ownership contract. It is not a co-equal invariant.
- **Smallest correct fix:** amend CLAUDE.md to demote the Branchless Law below the
  correctness/determinism/ownership laws and to state that a documented
  data-carrying-enum discriminant-read/destructure is an expected, escape-hatchable
  residue; make the lint honor `#[escape_hatch]`.
- **Belongs in:** Documentation + Tooling.

#### H5 — Systemic coverage theater: tests assert auto-derived `Debug` shape, not behavior
- **Files/symbols (representative):** `modules/axiom-agent/src/agent_api.rs:287`
  (`debug_derive_is_exercised` — whole body is
  `assert!(format!("{:?}", AgentApi).contains("AgentApi"));`);
  `modules/axiom-streaming/src/residency.rs:252`; plus the trailing
  `format!("{:?}",…).contains(TypeName)` line in ~100 mixed tests
  (`crates/axiom-host/src/frame_packet.rs:452–556`, `crates/axiom-ecs/src/ecs_api.rs:82`,
  `crates/axiom-kernel/src/tick_schedule.rs:197`, …). Scale: **35** `*derive*
  exercised` test fns and **109** `format!("{:?}",…).contains(…)` asserts across the
  spine.
- **Why harmful:** (charter #4) these assert the shape of a compiler-derived `Debug`
  impl to move llvm-cov to 100% — exactly the "tests that execute code without
  asserting on its behavior (coverage theater — they move the number, prove
  nothing, and rot)" that CLAUDE.md's Coverage Law explicitly bans. The reshape to
  derive-heavy tagged structs (H1) enlarged the derive-only surface these tests
  farm. (This is coverage-law-adjacent, not branchless-caused, but it is systemic
  and law-violating, so High.)
- **Rule disposition:** **remove the theater.** Where `Debug` content is a real
  contract, assert the formatted *content* (`assert_eq!(format!("{a:?}"), "…")`);
  otherwise delete the test.
- **Smallest correct fix:** delete the pure-theater fns; strip the trailing
  `contains(TypeName)` line from mixed tests (their Clone/Eq assertions stay).
- **Belongs in:** Test/Harness.

---

### MEDIUM

#### M1 — The lint has zero hot/cold granularity; the zone vocabulary that could fix it is unused by it
- **File/symbol:** `tools/lints/engine_no_branching/src/lib.rs:89–103`
  (`outside_branchless_spine` — directory-only).
- **Current behavior:** the only scope axis is `crates|modules` (banned) vs
  `apps|games|tools|xtask|tests|examples|benches` (exempt). A once-per-load manifest
  parser, a telemetry constructor, and a per-tick math kernel are all held to the
  same bar. `crates/axiom-zones` defines `#[hot_path]`/`#[sim]`/`#[strict]` HIR
  markers *precisely* so lints can be zone-scoped, and `engine_no_time_in_sim`
  already uses them — but `engine_no_branching` ignores them, and only 5 spine files
  even carry a zone marker.
- **Why harmful:** (charter #9) enforces the rule uniformly where its stated
  determinism/hot-path rationale is often absent, producing the cold-path overreach
  in M3.
- **Rule disposition:** **narrow to zones.** Enforce branchlessness inside
  `#[hot_path]`/`#[sim]`/`#[strict]`; allow normal control flow in unmarked cold
  spine code.
- **Smallest correct fix:** gate the lint on the zone markers (mirror
  `engine_no_time_in_sim`'s plumbing); annotate the genuinely-hot spine
  (math inner loops, per-tick runtime/frame step, raster) with `#[hot_path]`/`#[sim]`.
- **Belongs in:** Tooling (lint) + Layer/Module (zone annotations).

#### M2 — The Rust branch gate is absent from CI and only in a bypassable local hook
- **Files:** `.github/workflows/ci.yml` (no `cargo dylint` step);
  `.git/hooks/pre-commit:37–46`; `.git/hooks/dylint-baseline.txt` (untracked,
  per-worktree — two stale worktree copies read `15`/`17`).
- **Current behavior:** the "mechanically enforced hard gate" is a local hook that
  `--no-verify` skips (and `docs/unbranching.md` sanctioned skipping it throughout
  the campaign); CI never checks it. The baseline is not version-controlled.
- **Why harmful:** (charter #10) a loophole — the invariant CLAUDE.md calls
  mechanically enforced is not enforced on `main` by CI, and its threshold can drift
  per worktree with no history.
- **Rule disposition:** **keep the rule, fix the enforcement.** Either the gate is
  real (add it to CI, track the baseline) or CLAUDE.md should stop calling it
  mechanically enforced.
- **Smallest correct fix:** add a `cargo dylint --all` step to `ci.yml` (matching
  the TS gate, which *is* in CI); move `dylint-baseline.txt` into the tracked repo.
- **Belongs in:** Tooling.

#### M3 — Cold-path overreach: validation, serialization, and readers contorted for no determinism/perf gain
- **Files/symbols:**
  - `modules/axiom-assets/src/manifest.rs:106–122` — `validate` `&`-merges three
    distinct failure modes (duplicate id / null id / dangling dep) into one boolean
    and reports **one lumped error**, losing per-rule diagnostics in one-time
    manifest load.
  - `modules/axiom-render/src/render_receipt.rs:71–109` — `write_command` is a
    five-deep `.or_else` accessor chain (with a `unwrap_or(0)` fallback for a case
    that "is always `Some`") where a `match` is the textbook form; cold golden-image
    capture.
  - `crates/axiom-kernel/src/binary_reader.rs:38–50` — `take` computes the slice,
    then advances the cursor by `slice.map(len).unwrap_or(0)` (compute-both-arms) to
    avoid `?`; cold deserialization.
- **Why harmful:** (charter #1, #9) these are cold — load-time validation,
  serialization, one-shot byte reads — with no per-tick cost or determinism stake in
  being branchless. The reshape degraded diagnostics (manifest) and readability
  (serde) for nothing.
- **Rule disposition:** **narrow (follows M1).** Under zone-scoped enforcement these
  cold paths revert to guarded early-returns / `match` / `?`.
- **Smallest correct fix:** M1's zone scoping; then restore per-rule `Err` returns
  in `validate`, a `match` in `write_command`, and `?` in `take`.
- **Belongs in:** Module + Kernel.

#### M4 — `test_without_assertion` lint cannot catch trivial/tautological assertions (enables H5)
- **File/symbol:** `tools/lints/test_without_assertion/src/lib.rs:71–80,178–191`.
- **Current behavior:** any `assert!`-family macro anywhere in a test body satisfies
  the lint; it fires only on the strictly-empty case. So a test whose sole assertion
  is `assert!(format!("{:?}",x).contains("Name"))` — or `assert!(true)` — passes.
- **Why harmful:** (charter #4, #10) the structural blind spot that lets H5's ~35
  theater tests persist under a green gate; the lint's own doc claims to enforce the
  Coverage Law's "no execute-without-asserting" but enforces only emptiness.
- **Rule disposition:** **narrow the lint** — disqualify a `{:?}`-`contains`
  assertion as the *sole* assertion.
- **Smallest correct fix:** add that rule to `test_without_assertion`.
- **Belongs in:** Tooling.

#### M5 — Eager `[value, 1.0][ocean]` discards a full neighbour-fold in worldgen
- **File/symbol:** `modules/axiom-planetgen/src/stages/moisture_advection.rs:44`.
- **Current behavior:** `[advect_region(globe, cur, r), 1.0][usize::from(ocean)]`
  eagerly runs `advect_region` (a `fold` over every neighbour of region `r`:
  subtract, normalize, dot per neighbour) for **every ocean region on every pass**,
  then throws the result away. A branch short-circuits it.
- **Why harmful:** (charter #1, #7) both less readable and strictly more work
  (`O(regions × passes × neighbours)` of wasted computation). Cold worldgen, so not
  a hot regression, but the clearest "branchless does more work than a branch"
  instance in the spine.
- **Rule disposition:** **fix in place (branchless-preserving).**
- **Smallest correct fix:** `ocean.then_some(1.0).unwrap_or_else(|| advect_region(globe, cur, r))`
  — stays branchless, evaluates the fold only when needed.
- **Belongs in:** Module.

---

### LOW

#### L1 — `README.md` stale: still claims the branch lint fires in apps/tools
- **File:** `README.md` (branchless section). The 2026-07-04 baseline flip scoped
  apps/games/tools off; the README predates it. **Fix:** update the README.
  **Belongs in:** Documentation.

#### L2 — Nested table-selects that read worse than a small match (cold)
- **Files:** `modules/axiom-planetgen/src/stages/wind_field.rs:23`
  (`[[-0.6,0.9][temperate], -0.9][trade]` — a 3-band ladder flattened into nested
  2-entry tables read right-to-left); `modules/axiom-agent/src/replay_brain.rs:46–52`
  (nested 3-way reason table). Both cold (worldgen / once-per-tick agent decision).
  **Fix (under M1 scoping):** a `match` on the band/precedence index.
  **Belongs in:** Module.

#### L3 — ~40 spine comments assert a performance benefit with no benchmark behind them
- **Files:** `software_rasterizer.rs:9–20` ("costs the same", "lean") and ~40 other
  "branchless"/"branch-free"/"no division" comments across modules. The governing
  doc (`docs/unbranching.md`) never claims a perf benefit; these comments invent one.
  **Fix:** delete the perf language or back each with a number; add one
  branchless-vs-branchy A/B benchmark to `bench/` so the engine has *any* empirical
  basis. **Belongs in:** Documentation + Test/Harness.

#### L4 — Signing-payload duplication from a banned unifying branch
- **File:** `modules/axiom-netcode/src/net_message.rs:134–154` —
  `input_signing_payload` and `beacon_signing_payload` duplicate the frame-header
  serialization to avoid a single per-kind branch. Low (tails genuinely differ;
  drift contained by the shared `signed_bytes` dispatcher). **Fix (under escape-hatch
  policy):** one function with a guarded tail. **Belongs in:** Module.

#### L5 — `geo.rs` substitutes an arbitrary axis on normalize failure instead of signaling
- **Files:** `crates/axiom-math/src/geo.rs:56,59,69,70,82,93` — normalize failure
  yields `unwrap_or(Vec3::UNIT_X/UNIT_Z)`. Documented, total-by-contract, every
  consumer wants a total function; noted as the one spot a zero/NaN direction gets a
  plausible-but-arbitrary result rather than a signal. **No change required**; listed
  for completeness. **Belongs in:** (none — accepted).

#### L6 — The finite-guard idiom is duplicated ~20× with no shared primitive
- **Files:** `crates/axiom-kernel/{ratio,meters,seconds,radians}.rs`,
  `crates/axiom-noise/*`, etc. — `[0.0, value][value.is_finite() as usize]` inlined
  in every dimensioned-scalar validator. Safe and greppable, but the single
  most-duplicated branchless construct. **Fix:** extract a `finite_or_zero` (and
  `finite_or`) kernel primitive; callers reference it. **Belongs in:** Kernel.

---

## 5. Per-finding matrix (summary)

| ID | Sev | File:line / symbol | Rule → | Fix tier |
|----|-----|--------------------|--------|----------|
| H1 | High | `metric_value.rs:15`, `log_field.rs:11`, `render_command.rs:18`, `net_message.rs:52` (+GpuCommand, WebGpuBackendState) | **narrow** (allow enum destructure via escape hatch; revert types) | Kernel/Module + Tooling |
| H2 | High | `metric_report.rs:62` `write_to` | resolved by reverting H1 | Kernel |
| H3 | High | `software_rasterizer.rs:357–393` | **narrow/measure** (benchmark, then conditional write) | Test/Harness + Module |
| H4 | High | `CLAUDE.md` Branchless Law; `docs/unbranching.md:3` | **narrow positioning** (demote below correctness; honor escape hatch) | Documentation + Tooling |
| H5 | High | `agent_api.rs:287`, `residency.rs:252`, +~100 | **remove theater** | Test/Harness |
| M1 | Med | `engine_no_branching/src/lib.rs:89` | **narrow to zones** | Tooling + Layer/Module |
| M2 | Med | `ci.yml`; `pre-commit`; `dylint-baseline.txt` | **keep rule, fix enforcement** (add to CI, track baseline) | Tooling |
| M3 | Med | `manifest.rs:106`, `render_receipt.rs:71`, `binary_reader.rs:38` | **narrow** (follows M1) | Module + Kernel |
| M4 | Med | `test_without_assertion/src/lib.rs:71` | **narrow the lint** | Tooling |
| M5 | Med | `moisture_advection.rs:44` | **fix in place** (branchless-preserving) | Module |
| L1 | Low | `README.md` | update | Documentation |
| L2 | Low | `wind_field.rs:23`, `replay_brain.rs:46` | **narrow** (match under M1) | Module |
| L3 | Low | `software_rasterizer.rs:9`, ~40 comments | delete/measure | Documentation + Test/Harness |
| L4 | Low | `net_message.rs:134` | unify under escape hatch | Module |
| L5 | Low | `geo.rs:56` | accept (no change) | — |
| L6 | Low | kernel scalar validators (~20) | extract primitive | Kernel |

---

## 6. Keep branchless here — do not weaken

These are where the Branchless Law earns its keep. The safety and code-quality
passes verified them as correct, hot, deterministic, and *readable*. **None should
be relaxed.**

- **Math per-frame kernels** — `crates/axiom-math/src/{vec2,vec3,vec4,mat3,mat4,quat,geo}.rs`.
  Vector/matrix/quaternion arithmetic, `quat::select` (quat.rs:170), the `nlerp`/
  `slerp`/`tangent_basis` selects — genuinely per-frame, and each is backed by
  explicit both-arms equivalence tests that name the mutant they kill
  (`frustum.rs:324–386`, `geo.rs:190–245`). Exemplary.
- **Physics narrow-phase & solver** — `modules/axiom-physics/src/{contact_pair,contact_solver,integrator,mass_properties}.rs`.
  The `CONTACT_TABLE[kind_a*4+kind_b]` dispatch (contact_pair.rs:47), the
  `.max(f32::MIN_POSITIVE)` reciprocal guards with gated-out degenerate arms, and
  the `is_finite` atomic-rollback commit (physics_world.rs:410–431) are the correct
  branchless shape and are fully tested.
- **Per-pixel raster *selects*** — `software_rasterizer.rs` value selects (distinct
  from H3's eager composite), `sdf_raymarch.rs:122–126`, `canvas_depth_cue.rs`.
  Genuinely hot per-pixel; index-selects are right here. (H3 is specifically the
  *eager discarded composite + unconditional store*, not the selects.)
- **Sim-core & asset state-transition matrices** — `process_lifecycle.rs:96`
  (`LEGAL[from][to]`), `asset_state.rs:40` (`TRANSITIONS[state][outcome]`),
  `body_route.rs:129` (`TARGETS[kind][surface]`). 2D lookup matrices are clearer
  than nested matches and are deterministic.
- **Crypto constant-time compare** — `crates/axiom-crypto/src/jwt.rs:145–153`
  `constant_time_eq` (`fold(0, |a,(x,y)| a | (x^y))`). The non-short-circuit fold is
  a security requirement (timing-safe); keep it (it would exist with or without the
  law).
- **Fn-pointer dispatch tables** — Bayer dither (`frame_retro_32bit.rs:215`), ease
  curves (`axiom-tween/src/curve.rs`), wave labels (`axiom-audio/src/ids.rs:180`).
  Genuine tables, not branch-dodges.

---

## 7. Change the policy — proposed clean repo policy

Replace "branchless everywhere in the spine, baseline 0, no escape hatch" with a
zone-scoped, correctness-first policy:

1. **Required in deterministic hot primitives where semantics benefit.** Code marked
   `#[hot_path]`/`#[sim]`/`#[strict]` (math inner loops, per-tick runtime/frame step,
   per-pixel raster, sim transitions) **must** be branchless. This is where §6 lives.
2. **Strongly preferred in low-level math/sim inner loops — only with readability
   preserved.** A branchless rewrite that is *less* readable than the branch (nested
   table-selects, L2) is not an improvement; prefer a small `match` on an index.
3. **Not required in validation/error handling unless proven necessary.** Cold
   validation (M3 manifest), constructors, and error construction keep guarded
   early-returns and per-rule `Err`s. A branch here has no determinism or perf stake.
4. **Not required in tests, tools, docs, visual harnesses, app glue, or non-hot
   orchestration.** (Already true for apps/tools; extend "cold" to unmarked cold
   spine code via zone scoping.)
5. **Never allowed to replace explicit checked failure with a sentinel-state trick.**
   No `.unwrap_or(())` that drops a write (H2), no sentinel return where `Result`/
   `Option` belongs, no data-carrying enum flattened into a tagged struct with
   sentinel fields to dodge a `match` (H1). A data-carrying-enum discriminant-read or
   variant-destructure is an **expected, escape-hatchable residue**, not a reason to
   reshape the contract.

**Priority restatement for CLAUDE.md:** correctness & determinism → ownership →
tests-proving-behavior → performance-with-evidence → branchlessness. The Branchless
Law is demoted from co-equal invariant to a subordinate discipline that auto-yields
to any higher law.

---

## 8. Mechanical enforcement changes (precise)

1. **Zone-scope the lint (M1).** In `tools/lints/engine_no_branching/src/lib.rs`,
   replace the "fire on all spine code" policy with "fire only inside a
   `#[hot_path]`/`#[sim]`/`#[strict]` marker" — reuse the marker-detection plumbing
   `engine_no_time_in_sim` already uses (`__engine_zone_*` HIR markers). Unmarked
   cold spine code is no longer flagged. Keep the app/game/tool/test directory
   exemptions.
2. **Honor `#[escape_hatch]` (H1/H4).** Add an escape-hatch check: a branch inside an
   `#[escape_hatch(reason=…)]` item is allowed (the reason is already required
   non-empty by `engine_require_escape_hatch_reason`). This restores option (a) from
   `docs/unbranching.md:138` so a data-carrying-enum destructure can be kept as an
   enum.
3. **Add the gate to CI (M2).** Add a `cargo dylint --all` step to
   `.github/workflows/ci.yml`, failing on any engine lint above baseline (mirror the
   TS gate that is already in CI).
4. **Track the baseline (M2).** Move `dylint-baseline.txt` from the untracked
   per-worktree `.git/hooks/` into the tracked repo (e.g. `tools/lints/baseline.txt`),
   read by both hook and CI, so drift has history.
5. **Tighten `test_without_assertion` (M4/H5).** Disqualify a
   `format!("{:?}",_).contains(_)` assertion as a test's sole assertion.
6. **Un-stale the README (L1).**

---

## 9. Semantic test replacements — prove behavior, not branchlessness

1. **Rasterizer overdraw benchmark + equivalence (H3, new).** A native fixture with
   heavy overdraw asserting the branchless raster output is **byte-identical** to a
   branchy reference rasterizer, plus a `criterion`/timed A/B. Proves both
   correctness-equivalence and the perf claim (or refutes it). Today neither exists.
2. **`MetricValue`/`FieldValue` serialization round-trip totality (H2).** A test
   that every constructible value survives `write_to`→`read_from` byte-identically —
   and, once reverted to an enum, this is compiler-guaranteed total. Until reverted,
   assert `write_to` panics/errors rather than silently emitting no tag for an
   out-of-range `kind`.
3. **Replace Debug-shape theater with content assertions (H5).** Where a `Debug`
   impl is a real contract, `assert_eq!(format!("{x:?}"), "<exact expected>")`;
   otherwise delete. Kill all `format!("{:?}",_).contains(TypeName)`-as-sole-
   assertion tests.
4. **Branchless-vs-branchy A/B for the `[a,b][idx]` idiom (L3, new).** One `bench/`
   entry comparing the canonical select against `if`, giving the engine its first
   empirical datum on the idiom.
5. **Manifest per-rule validation tests (M3).** Once `validate` returns per-rule
   errors, assert each rule (duplicate id / null id / dangling dep) yields its own
   distinct error — currently impossible because all three are lumped.

---

## 10. Do not change — rules that must remain strict

These are correctness/determinism/ownership invariants the audit **explicitly
affirms**. None is weakened by anything above; several are *strengthened* by
demoting branchlessness beneath them.

- **No hidden nondeterminism.** Deterministic per-tick behavior and replay stay
  mandatory.
- **No ambient wall-clock time in the spine** (`engine_no_time_in_sim` — the
  correctly zone-scoped lint; the model M1 should copy).
- **No unseeded randomness.** Deterministic random sources only.
- **No unstable iteration order in deterministic artifacts.** Snapshots, receipts,
  and wire frames stay byte-stable.
- **No layer-direction violations** (Layer Law / `check-architecture`).
- **No browser/platform APIs leaking inward** (host/windowing allowlist only).
- **No junk-drawer modules** (`utils`/`helpers`/`common`/`misc` ban).
- **Checked failure over sentinels** — this audit *reinforces* it (H1/H2): the fix
  is to restore checked, exhaustive contracts, never to add more sentinel state.
- **100% spine coverage** — stays; the fix to H5 is honest behavior tests, not
  lowering the gate.

---

## Validation

Commands run after writing this report; results recorded in the final response
accompanying this file (`cargo test --workspace`, `cargo xtask check-architecture`).
The audit made **no code changes** other than creating this file, so it cannot have
introduced a new failure; any failure reported is pre-existing.
