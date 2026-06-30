# Spec-vs-Implementation Gap Audit — 2026-06-30 (second pass)

**Date:** 2026-06-30
**Method:** Adversarial agent team via the spec-vs-impl-adversarial-audit workflow — one
`Explore` auditor per spec (SPEC-00…14), each charged with *disproving* its `Landed`
status; every finding then independently re-checked by a skeptic who tried to *refute*
it (defaulting to REFUTED unless the gap re-confirmed with fresh file:line evidence).
**Scale:** 42 agents, 1494 tool calls. (Two verifier agents errored without emitting a
verdict; their parent findings were dropped — minor under-report, not over-report.)
**Counting rule:** a gap counts only if the spec/README presents the capability as
**done** and the code does **not** back it, **and** no footnote/deferral discloses it.
Documented deferrals (README ¹–⁵, per-spec open questions) are not counted.
**Result:** 13 confirmed findings across 8 specs (1 critical, 9 major, 3 minor).
**Predecessor:** see `SPEC_VS_IMPL_GAP_AUDIT.md` (2026-06-29 + remediation).

---

# Spec-vs-Implementation Gap Audit

## 1. Verdict

Of 15 specs audited, **7 are genuinely fully Landed with no confirmed gap** (SPEC-03, 05, 08, 10, 11, 12, 13), and **8 specs (SPEC-00, 01, 02, 04, 06, 07, 09, 14) carry 13 confirmed, undisclosed gaps** despite every one being marked "Landed" (or, for SPEC-14, freshly mis-marked "Partial"). The dominant failure mode is **TS-authoring signature drift** — the shipped `@axiom/game` surface silently diverges from the published contract (rect `w/h`→`width/height`, `setParent` null arg, `gridReachable` `CellPair`, `Timers`→`Time`, `Ui.sprite`/`viewport`/`text`), so author code written to the contract will not compile. None of these are disclosed via footnote, deferral note, or README status; the disclosures that exist live only in buried code comments.

## 2. Severity-Ranked Findings

| Spec | Severity | Kind | Gap |
|------|----------|------|-----|
| SPEC-04 | critical | signature-mismatch | `Frame` TS interface missing `path`/`linearGradient`/`radialGradient`; "full projection" claim misleading |
| SPEC-00 | major | signature-mismatch | `Rect` ships `width`/`height`, contract says `w`/`h` |
| SPEC-01 | major | missing-proof | No pinned golden for `unit`/`int`/`weighted_index`/`shuffle` |
| SPEC-02 | major | signature-mismatch | `setParent` lacks `parent: Entity \| null` (no detach-to-root) |
| SPEC-06 | major | signature-mismatch | `gridReachable` takes `CellPair` not `start`/`goal` |
| SPEC-07 | major | signature-mismatch | `Timers` interface renamed `Time`, on `Sim.time` not `Sim.timers` |
| SPEC-07 | major | coverage-gap | Missing reentrancy test (timer-dispatch-schedules-timer) |
| SPEC-09 | major | signature-mismatch | `Ui.sprite` takes `bounds`, not `SpriteOpts` |
| SPEC-09 | major | behavioral-bug | `Ui.text`/`sprite` drop full SPEC-04 `TextOpts`/`SpriteOpts` styling |
| SPEC-14 | major | missing-proof | Spec header reverted to false "Partial"; factories filled & lifecycle driven |
| SPEC-00 | minor | missing-proof | Spec claims `Rect` absent; it ships and is exported |
| SPEC-07 | minor | signature-mismatch | State-machine `states` is `StateNode[]` not `Record<S,StateDef>` |
| SPEC-09 | minor | signature-mismatch | `Ui.viewport` is a method, contract says property |

## 3. Per-Finding Detail

### SPEC-00 — Authoring boundary & frame model

**[major] `Rect` property names diverge: `width`/`height` vs `w`/`h`**
- **Claim:** Contract §0.2 specifies `interface Rect { x; y; w; h }`; authors read `rect.w`/`rect.h`.
- **Reality:** Ships `interface Rect { readonly x; readonly y; readonly width; readonly height }`; all TS uses `width`/`height`. Author code written to contract fails.
- **Evidence:** `docs/game-api-contract.md:85` vs `packages/axiom-game/src/vocabulary.ts:72-78`.
- **Fix (lowest correct layer):** This is a value-vocabulary contract, not glue — reconcile at `vocabulary.ts`. Either rename the shipped fields to `w`/`h` (and update all consumers: `math.ts`, `draw2d.ts`, `ui-layout.ts`) or amend the contract §0.2 + spec §5 to `width`/`height`. Pick one and make `vocabulary.ts` the single source.

**[minor] Spec falsely claims `Rect` is absent**
- **Claim:** SPEC-00:37-38 "the `Rect` core value-type (§5) is still absent from the shipped value vocabulary."
- **Reality:** `Rect` is defined and exported; spec self-contradicts (also lists it at line 135).
- **Evidence:** `docs/specs/SPEC-00-authoring-boundary-and-frame-model.md:37-38` vs `vocabulary.ts:72-78`, `index.ts:260`.
- **Fix:** Documentation-layer correction — delete the "still absent" sentence in SPEC-00 §2; the gap is now solely the naming mismatch above.

### SPEC-01 — Deterministic randomness

**[major] Missing pinned literal goldens for `unit`/`int`/`weighted_index`/`shuffle`**
- **Claim:** §7:147-148 requires "a pinned first-value golden (mirroring `golden_first_value_is_stable`) for `unit`, `int`, `weighted_index`, and a shuffled ordering."
- **Reality:** Only `next_u64()` has a pinned golden. The four named methods have property/reproducibility tests only — no pinned literal assertions.
- **Evidence:** `crates/axiom-entropy/src/entropy_api.rs:74-77` (sole golden) vs `entropy_stream.rs:178-186, 189-193, 215-225, 235-248`.
- **Fix (lowest correct layer):** Test-layer only — add four pinned-literal assertions in `entropy_stream.rs` tests. No production change; the methods already produce deterministic streams.

### SPEC-02 — Entities, components, queries, hierarchy

**[major] `setParent` missing `null` to detach to root**
- **Claim:** §4.2:150 `setParent(child: Entity, parent: Entity | null): void` — "null detaches to the root."
- **Reality:** TS and wasm boundary both accept only `(child, parent: Entity)`; no null path, no `clearParent`. Authors cannot detach to root.
- **Evidence:** `docs/specs/SPEC-02-entities-components-queries.md:150` vs `packages/axiom-game/src/world.ts:42`, `apps/axiom-game-runtime/src/wasm.rs:516`.
- **Fix (lowest correct layer):** Root is the wasm/ECS boundary — `wasm.rs` `setParent` must accept an optional parent and route a sentinel/None to the hierarchy root; then widen `world.ts:42` to `Entity | null`. Fixing only TS would leave the boundary unable to carry the null.

### SPEC-04 — 2D surface

**[critical] `Frame` TS interface missing `path`/`linearGradient`/`radialGradient`**
- **Claim:** SPEC-04:5 claims the "full `@axiom/game` `Frame` 2D projection ... is landed"; contract §10:386-431 requires `path`, `linearGradient`, `radialGradient`.
- **Reality:** `Frame` (sim.ts:70-102) exposes none of the three; `Draw2dBridge` has no `draw2dPath`/`draw2dLinearGradient`/`draw2dRadialGradient`. Code comments admit the gap; the spec/README call the projection "full."
- **Evidence:** `game-api-contract.md:386-431`, `SPEC-04-2d-surface.md:5` vs `packages/axiom-game/src/sim.ts:70-102`, `draw2d-binding.ts:42-46, 191-226`.
- **Fix (lowest correct layer):** The raster backend already supports path/gradients (per README¹) — the gap is the missing draw2d exports + TS projection. Add `draw2dPath`/`draw2dLinearGradient`/`draw2dRadialGradient` at the wasm draw2d boundary, then surface them on `Frame`. Until then, correct SPEC-04:5/README¹ to stop claiming "full."

### SPEC-06 — Grid, pathfinding, tile space

**[major] `gridReachable` uses `CellPair` not separate `start`/`goal`**
- **Claim:** §4.2:141-142 / contract:306-307 `gridReachable(grid, start: Cell, goal: Cell, passable): boolean`.
- **Reality:** Ships `gridReachable(grid, ends: CellPair, passable)` with `CellPair = { start; goal }`; same drift on `gridPath` and `stepToward`.
- **Evidence:** `SPEC-06:141-142`, `docs/game-api-contract.md:306-307` vs `packages/axiom-game/src/grid.ts:206, 211-215, 233`.
- **Fix (lowest correct layer):** TS projection layer — flatten `grid.ts` signatures to `(grid, start, goal, passable)` to match the contract (or amend contract+spec to `CellPair` consistently across all three functions). Reconcile in `grid.ts`, the one owner.

### SPEC-07 — Timers & state machines

**[major] `Timers` interface renamed `Time`, placed at `Sim.time`**
- **Claim:** §4.2 defines `interface Timers { after, every, cancel }` at `Sim.timers`, plus a standalone `createStateMachine`.
- **Reality:** Ships `interface Time` at `Sim.time`, folding `createMachine` in as a method. Three-way drift (interface name, property name, function placement).
- **Evidence:** `docs/specs/SPEC-07-timers-and-state-machines.md:139-160` vs `packages/axiom-game/src/time.ts:21-32`, `sim.ts:51`.
- **Fix (lowest correct layer):** Decide the canonical shape and reconcile in `time.ts`/`sim.ts` + spec together. If `Time` (combining timers + machine) is the intended design, amend SPEC-07 §4.2; if the spec is canonical, split back to `Sim.timers` + top-level `createStateMachine`.

**[major] Missing reentrancy test**
- **Claim:** §7:223-227 requires a test that a timer whose dispatch schedules another timer yields the new timer "due no earlier than `now + 1`, never within the same due pass," deterministic on replay.
- **Reality:** No such test in `time.test.ts`, `pump.test.ts`, `tick_api.rs`, or `timers.rs`.
- **Evidence:** `SPEC-07:223-227`; absent across `packages/axiom-game/src/pump.test.ts`, `modules/axiom-tick/src/{tick_api.rs,timers.rs}`.
- **Fix (lowest correct layer):** Test-layer — add the reentrancy test at the level the spec names (TS pump test mirroring the native tick semantics). If the schedule-during-pump path isn't reachable through the public API, that's a design signal to expose it, not to skip the test.

**[minor] State-machine `states` is `StateNode[]` not `Record<S, StateDef>`**
- **Claim:** §4.2:150-156 `createStateMachine` takes `states: Record<S, StateDef<S>>`.
- **Reality:** Ships `states: readonly StateNode<State>[]` (named array); acknowledged in code comment but spec never updated.
- **Evidence:** `SPEC-07:150-156` vs `packages/axiom-game/src/state-machine.ts:10-14, 36`.
- **Fix:** Documentation-layer — update SPEC-07 §4.2 to the array shape the code intentionally chose (the comment already states the rationale), making spec match implementation.

### SPEC-09 — UI/HUD overlay & tween/easing

**[major] `Ui.sprite` takes `bounds`, not `SpriteOpts`**
- **Claim:** §4.2:135 / contract:595 `sprite(texture: TextureId, opts: SpriteOpts)` with rotation/scale/anchor/tint/flip/source.
- **Reality:** Ships `sprite(texture: Handle, bounds: Rect)`; `UiSpriteOpts` carries only `x,y,w,h`. No rotation/scale/tint/flip/source.
- **Evidence:** `SPEC-09-ui-hud-and-tween.md:135`, `game-api-contract.md:595` vs `packages/axiom-game/src/ui.ts:32`, `ui-binding.ts:71`, `ui_geometry.rs:120-129`.
- **Fix (lowest correct layer):** Root is the UI geometry/binding boundary — extend `UiSpriteOpts` (`ui_geometry.rs`) to carry the full opts, thread through `ui-binding.ts`, then widen `ui.ts:32`. TS-only widening would have nothing to bind to.

**[major] `Ui.text`/`sprite` drop full SPEC-04 styling**
- **Claim:** §4.2:152-153 "`TextOpts`/`SpriteOpts` are the §10 / SPEC-04 style records, reused unchanged."
- **Reality:** `Ui.text` accepts minimal `UiTextOpts` (`x,y,color,size`), dropping font/align; `sprite` drops rotation/scale/tint/source. Authors cannot use documented SPEC-04 styling on UI.
- **Evidence:** `SPEC-09:152-153` vs `packages/axiom-game/src/ui-binding.ts:50-60`, `ui.ts:30,32`.
- **Fix (lowest correct layer):** Same boundary as above — make the UI binding reuse the SPEC-04 `TextOpts`/`SpriteOpts` records end-to-end rather than minimal UI-local opts; this is one structural fix that subsumes the `Ui.sprite` finding.

**[minor] `Ui.viewport` is a method, contract says property**
- **Claim:** §4.2:137 `readonly viewport: { width; height }` (property access).
- **Reality:** Ships `readonly viewport: () => UiViewport` (method call). Contract-written author code breaks.
- **Evidence:** `SPEC-09:137` vs `packages/axiom-game/src/ui.ts:36`, `ui.test.ts:42`.
- **Fix:** TS projection layer — make `viewport` a property (snapshot per frame) in `ui.ts` to match the contract, or amend SPEC-09 §4.2 + contract to a method. Reconcile the two; do not leave them split.

### SPEC-14 — TypeScript authoring SDK

**[major] Spec header reverted to false "Partial"**
- **Claim:** SPEC-14:3 (commit eac58f8) "Status: Partial — the `Scene` factories are still M0 stubs and the `Scene` lifecycle is not driven by the loop."
- **Reality:** Factories are real per-tick projections (not `{ subsystem }` placeholders) and the loop drives the scene lifecycle. The eac58f8 revert made the header *less* accurate than the prior 82b25b0 ("§2 stubs are now filled"); README:111 lists SPEC-14 "Landed."
- **Evidence:** `git diff eac58f8^..eac58f8`; contradicting code at `packages/axiom-game/src/game-loop.ts:16, 50-56`, `scene.ts:7-14`, `scene-runtime.ts` (`mountScene`/`start()`/`tick()`); README:111.
- **Fix (lowest correct layer):** Documentation-layer — restore SPEC-14:3 to "Landed" with the accurate "§2 stubs filled, lifecycle driven" note (revert eac58f8's header change), aligning spec with README and code. The one genuinely-documented stub (cameras) stays noted.

## 4. Specs That Held Up

- **SPEC-03** — Math & spatial queries: no confirmed gap.
- **SPEC-05** — Input (keyboard, bindings, pointer, timing): no confirmed gap.
- **SPEC-08** — Audio (synthesis, playback, analysis): no confirmed gap.
- **SPEC-10** — Physics extensions (angular, friction): no confirmed gap.
- **SPEC-11** — 3D scene authoring surface: no confirmed gap.
- **SPEC-12** — Host bridge & persistence: no confirmed gap.
- **SPEC-13** — Multiplayer & netcode authoring: no confirmed gap.