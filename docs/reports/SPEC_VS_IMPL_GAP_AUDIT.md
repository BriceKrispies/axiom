# Spec-vs-Implementation Gap Audit

**Date:** 2026-06-29
**Method:** A team of adversarial agents (one auditor per spec, each charged with
*disproving* its "Landed" status; every undocumented gap then independently
re-checked by a skeptic who tried to refute it). 68 agents, 829 tool calls.
**Scope:** `docs/specs/SPEC-00 … SPEC-14` vs the real tree.
**Counting rule:** a gap counts only if the spec/README presents the capability
as **done** and the code does **not** back it, **and** no footnote/deferral note
discloses it. Documented deferrals (README ¹–⁵ and per-spec notes) are *not*
counted as gaps. Four auditor over-reports were refuted and dropped.

## Remediation status (2026-06-30)

**The gaps below have since been closed.** A re-verification against the
advanced tree found most were already addressed by in-flight TS-projection work;
the genuine remainder was then implemented:

- **SPEC-02** — full 12-method `World` (+ `Transform` value type) and the §7
  hierarchy/lifecycle proof: **closed**.
- **SPEC-03** — `v2` namespace, the pure predicates, `overlapBox`/`overlapCircle`/
  `raycast`, and `lerp` routed to native f32 (determinism smell fixed): **closed**.
- **SPEC-04** — full `Frame` 2D projection + §10.2 `sampleAnimation` +
  `measureText`/`loadFont`/`loadTexture`; GPU `present_draw2d` now rasterizes
  rect+sprite at proven parity with software (§7 both-backends proof); the §7
  marshalled-list proof landed. Residual deferrals: GPU circle/ellipse/line/
  particle, and both-backends path/gradient/text-glyph raster.
- **SPEC-05** — input crosses the wasm boundary (injection + reads), host
  `bindAction` wired, DOM edge feeding `sample`; §7 cross-chunk + snapshot-stream
  replay proofs landed: **closed**.
- **SPEC-09** — TS `Ui` overlay + `solveLayout`/`LayoutNode` + the §7 button
  truth-table and presentation-leak proofs: **closed**.
- **SPEC-11** — §7 render-one-frame slice ("nova-roll") + GPU↔canvas2d parity
  (cylinder+emissive) via `axiom-shot`: **closed**. Residual deferral: 3D
  translucency blend + `MeshData`.
- **SPEC-13** — the whole TS authoring surface (`onSnapshot`/`onRestore`, intent
  codec, per-player twin, `hostRoom`/`matchmake`, `NetParticipants`), the
  cross-instance determinism golden, and the authored-callback netplay-server:
  **closed**. Deferrals by decision: physics net-prediction OFF, delta/JWT/
  unreliable transport.

The medium/low proof-rigor items and the documented deferrals (§"Real but
already-disclosed") that were not in scope remain as noted. The original audit
findings below are preserved as the historical record.

## Headline

The **native Rust sim spine is solid** — nearly every spec's §4.1 facade matches
the code exactly, the accumulator/entropy/ecs/grid/tick/physics cores are real,
branchless, and covered. The gaps are concentrated in three places:

1. **The TypeScript `@axiom/game` projection is the single biggest hole.** Six
   specs claim a TS authoring surface is "Landed" that **does not exist** in
   `packages/`. The README's own rule — *"a native facade with no TS projection
   is half-built"* — is violated repeatedly. This is the dominant theme of the
   audit.
2. **The wasm boundary does not carry input.** `WasmGame` (the real seam the SDK
   binds) exposes no input method at all, so `Sim.input` — which SPEC-05 and
   SPEC-00 present as working — cannot function across the live bridge.
3. **Many promised §7 proof tests do not exist** — golden/replay/parity/property
   tests the specs list as the definition of done are simply absent.

Plus one **determinism smell**: SPEC-03's TS `lerp` is re-implemented in JS
(f64) instead of routing to the authoritative native `MathApi::lerp` (f32) — a
sim-class value computed off the authoritative path.

### Per-spec verdict

| Spec | Subsystem | Verdict | Confirmed gaps (H/M/L) |
|------|-----------|---------|------------------------|
| 00 | Authoring boundary / frame model | minor-drift | 0/0/1 |
| 01 | Deterministic randomness | minor-drift | 0/1/1 |
| 02 | Entities / components / hierarchy | **major-gap** | 2/2/0 |
| 03 | Math & spatial queries | **overclaimed** | 3/2/1 |
| 04 | 2D surface | **overclaimed** | 1/2/0 (+3 documented) |
| 05 | Input | **overclaimed** | 1/2/1 |
| 06 | Grid / pathfinding | minor-drift | 0/0/2 (+1 documented) |
| 07 | Timers & state machines | minor-drift | 0/1/2 |
| 08 | Audio | minor-drift | 0/1/0 (+2 documented) |
| 09 | UI/HUD & tween | **overclaimed** | 2/2/1 (+3 documented) |
| 10 | Physics extensions | minor-drift | 0/1/0 |
| 11 | 3D scene surface | **major-gap** | 1/2/1 |
| 12 | Host bridge & persistence | overclaimed | 0/0/0 (+1 documented) |
| 13 | Multiplayer & netcode | **overclaimed** | 5/1/0 (+3 documented) |
| 14 | TypeScript authoring SDK | minor-drift | 0/0/0 (+1 documented) |

**Cleanest:** 10, 06, 14, 00, 08. **Worst offenders:** 13, 03, 09, 02, 11.

---

## HIGH-severity gaps (claimed landed, genuinely missing, undisclosed)

### Theme A — TypeScript projection claimed "Landed" but absent

**SPEC-02 — World hierarchy/lifecycle TS surface (6 methods missing).**
§4.2 lists a 12-method `World`; the projected `World` exposes only 7
(spawn/despawn/despawnSubtree/get/set/query/childrenOf). Absent from both the TS
interface **and** the `NativeBridge` seam: `alive`, `has`, `remove`, `setParent`,
`parentOf`, `worldTransform`.
*Evidence:* `packages/axiom-game/src/world.ts` vs SPEC-02 §4.2 (L138–155). The §7
TS proof (parent a child, assert `worldTransform` composition, despawn parent,
assert child gone + `alive` false) **cannot run** and does not exist —
`world.test.ts` only touches the 7 built methods.

**SPEC-03 — entire spatial/vector TS projection absent.** The status line claims
`v2` helpers + predicates routed to native math are exported. Reality: only
`clamp/lerp/normalizeAngle/overlapCircle` exist. Missing: the `v2` namespace
(8 ops), the pure predicates `aabbOverlap`/`pointInRect`/`circleOverlap`, and the
scene queries `overlapBox`/`raycast`/`RayHit` — **even though the native
`SceneApi::overlap_box`/`raycast` are built**. No README footnote discloses any
of this.
*Evidence:* `packages/axiom-game/src/index.ts:89`, `math.ts` (whole file);
`host-binding.ts:89–94`.

**SPEC-04 — entire 2D Frame TS projection absent.** §4.2 projects the full
contract 2D surface; §7 promises a headless test asserting the marshalled command
stream matches the native `Draw2dList`. The TS `Frame` is literally
`{ readonly tick: number }` — none of camera2D/rect/circle/ellipse/line/path/
sprite/text/measureText/gradients/loadTexture/loadFont exist, and the headless
test does not exist.
*Evidence:* `packages/axiom-game/src/sim.ts:54–57`; no draw2d projection file.

**SPEC-09 — Ui overlay TS surface absent; `solveLayout` is fiction.** The header
says "the §2 gaps below are now closed" and that `solveLayout` projects the
landed `axiom-layout::solve`. Reality: no TS `Ui` surface
(rect/text/sprite/button/viewport) anywhere, **and `solveLayout`/`LayoutNode`
exist only in docs** — zero matches in any TS *or* Rust source.
*Evidence:* `packages/axiom-game/` (no `ui.ts`); grep `solveLayout` → docs only.

**SPEC-13 — multiplayer authoring TS surface largely absent.** Four high gaps:
- `onSnapshot`/`onRestore` author hooks (§16.5) — not projected in `@axiom/game`
  (the `axiom-client` `onSnapshot` is a different inbound-frame handler).
- No `Intent`-derived TS wire codec (§3.3/§16.2 "one record, engine derives the
  codec"); `sendIntent` forwards a raw object, no serializer.
- The new per-player Rust messages (`ClientIntentFor`/`ServerSnapshotFor`) have
  **no TS twin** in `packages/axiom-client/src/codec.ts`, so the §7 byte-parity
  golden cannot exist.
- `net.test.ts` never hosts a room / derives a codec / joins two clients /
  exchanges intents / tests oversize rejection — it asserts only seam forwarding.

### Theme B — boundary & proof gaps

**SPEC-05 — the wasm boundary exposes no input.** `WasmGame`
(`apps/axiom-game-runtime/src/wasm.rs:92–204`) exports only
new/seed/report_outcome/advance/current_tick/snapshot + rng*. There is **no**
`inputIsDown`/`inputPressed`/`inputPointer`/… method, so the `Sim.input` surface
SPEC-00/05 present as working has nothing to bind to on the real bridge. (The
native `InputState` module itself is fine; the boundary that exposes it is the
gap.)

**SPEC-11 — no render-one-frame slice proof; `nova-roll` does not exist.** §7
promises a slice test in `apps/axiom-game-runtime` authoring cube+cylinder+camera+
light and rendering one frame (the "nova-roll smoke path"). That app has no
`tests/` dir, zero references to Cylinder/createMesh/setCamera3D/addLight, and
`nova-roll` appears only in docs. The end-to-end 3D render proof is absent.

**SPEC-13 — no cross-instance determinism golden; netplay-server runs fake
movement.** The load-bearing §17.6 golden replay (authored `onFixedUpdate`, fixed
per-player intent stream, bit-identical authority/predicted) is not implemented.
`tools/axiom-netplay-server` runs hard-coded two-cube movement
(`main.rs:133–135`), decodes the *anonymous* `ClientIntent` 4-tuple (not the new
per-player `decode_client_intent_for`), and has no predicted-client reconcile.

---

## MEDIUM-severity gaps

- **SPEC-01** — no pinned first-value goldens for `unit`/`int`/`weighted_index`/
  `shuffle` (only same-process reproducibility asserted); the only pinned literal
  is on `next_u64`.
- **SPEC-02** — §4.2 `Transform` value type (Vec3 position/scale) not exported;
  `despawn` is split into `despawn`+`despawnSubtree`, so the contract's
  "despawn cascades to subtree" semantics aren't what projected `despawn` does.
- **SPEC-03 — determinism smell.** TS `lerp` is local JS f64
  (`start+(end-start)*fraction`), explicitly "stays in the TS layer rather than
  paying a bridge crossing" — **not** routed to native f32 `MathApi::lerp`. A
  sim-class value computed off the authoritative path can diverge. The promised
  TS↔native cross-check test also doesn't exist (TS math tests use a `FakeHost`).
- **SPEC-04** — §10.2 flip-book sampler (`sampleAnimation`) is presented as
  "PURE … trivially covered" but exists **nowhere** (native or TS) and is not in
  the README's disclosed deferrals. GPU backend `present_draw2d` is a permanent
  `false` no-op — only the software backend rasterizes a `Draw2dList`, so the §7
  "both backends" alpha-blend proof is half-met.
- **SPEC-05** — missing cross-chunk-invariance replay test (same events grouped
  into ticks differently → same snapshots) and snapshot-stream-alone replay.
- **SPEC-07** — missing reentrancy test (a timer whose dispatch schedules another
  timer; new timer due ≥ now+1, never same pass).
- **SPEC-08** — in the wasm arm only `PlayTone` makes sound;
  `Load`/`PlaySample`/`PlayMusic`/`Stop` collapse to `=> {}` no-ops
  (`audio_api/web.rs:67–73`). "Live playback browser-proven" overstates — only
  the tone path is.
- **SPEC-09** — no TS `Ui` button truth-table proof (native one exists); no
  signature on tween (`advance` takes integer nanos + `TweenValue`, not
  `Ratio`/f32/secs per §4.1 — nominal-only but real drift).
- **SPEC-10** — no TS physics collider-attach/material/friction projection;
  `NativeBridge` exposes only `physicsAddBody(entity, kind)`. §4.2 says friction
  is "already projected on collider attach; no new verb" — there is no collider
  attach in TS.
- **SPEC-11** — no GPU↔canvas2d backend-parity test for cylinder+emissive (§7);
  the math3d TS cross-check forwards to an *invented* `FakeHost`, never the real
  `MathApi`.
- **SPEC-13** — `matchmake`/`Match` not projected in `@axiom/game`.

## LOW-severity gaps (drift / proof-rigor)

- **SPEC-00** — chunk-invariance is example-based (3 hand-picked partitions), not
  the generative property test §7 names.
- **SPEC-01** — §4.1 doc says `unit` is `next_u64 >> 11 over 2^53`; code is
  `>> 40 over 2^24` as f32 (Ratio is f32; spec's derivation is wrong).
- **SPEC-03** — `overlap_circle` test isn't the promised parity-vs-`overlap_box`
  + before/after-advance propagation test; no SPEC-03 replay golden.
- **SPEC-05** — TS edge-read/`pressedAtTick` reproduce-on-replay proof exercises
  only `axis`, not the edge reads.
- **SPEC-06** — `gridPath`/`stepToward` collapse endpoints into a `CellPair`
  (≤3-param law) and return `Result<Cell[]>` not `Cell[] | null`; distance-field
  hash golden missing (`state_hash` only impl'd for `Grid<u32>`, not `Grid<Dist>`).
- **SPEC-07** — TS surface is `Time { after; every; cancel; createMachine }` on
  `Sim.time`, not the §4.2 `Timers` interface + top-level `createStateMachine`;
  `createMachine` takes an ordered array, not a `Record<S, StateDef>`.
- **SPEC-09** — no presentation-leak test (no tween/Ui output reachable from a
  `sim` accessor).
- **SPEC-11** — §4.1 `new_lit`/`roughness`/`opacity` written as `f32`; code uses
  `Ratio` (correct per no-naked-f32 — the **spec text** is stale, not the code).

---

## Real but already-disclosed (not counted as surprises)

These are genuine gaps the spec/README **does** disclose — listed for honesty:
SPEC-04 circle/line/path/gradient/stroke/text raster + particles + render-targets
(README ¹); SPEC-08 live playback/analyser browser-only (²); SPEC-10
cross-platform determinism + inertia tensor (³); SPEC-11 3D translucency blend +
`MeshData` (⁴); SPEC-13 `hostRoom`/`RoomConfig`/`matchmake`/networked
`onFixedUpdate` overload + cross-instance determinism + delta/JWT/unreliable
transport (⁵, §2 notes); SPEC-12 live postMessage/localStorage browser-only.

## What the auditors got wrong (refuted, dropped)

SPEC-00 `Sim.tick: Ticks` vs `number` (transparent alias — identical type);
SPEC-05 keymap first-match (the binding *does* resolve identically); SPEC-09
`ease` free fn + `UiSurface` placement (both present as specified).

---

## Recommended remediation order

1. **Fix the README's status ledger first** — several specs are marked "Landed"
   whose TS half is unbuilt. Either build the projection or re-mark them
   "Landed (native); TS projection deferred" with a footnote, so the index stops
   over-claiming. Per the README's own rule, those are "half-built."
2. **Close the TS projection gaps** (SPEC-02/03/04/09/13) — the largest, most
   user-visible hole; the native facades already exist to bind to.
3. **Carry input across the wasm boundary** (SPEC-05) — without it `Sim.input`
   is non-functional in the browser, blocking every interactive authored game.
4. **Route TS `lerp` (and any other sim-class math) to native** (SPEC-03) — a
   real determinism risk, not just a missing test.
5. **Land the promised proof tests** (SPEC-00/01/05/07/11/13) — the definition of
   done these specs set for themselves.
6. **Make `tools/axiom-netplay-server` run the authored callback** and the
   per-player codec (SPEC-13), then the cross-instance determinism golden.
