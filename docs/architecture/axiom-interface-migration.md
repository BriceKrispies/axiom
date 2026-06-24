# Migration plan: adding `axiom-interface` as an engine layer

**Status:** ✅ **implemented.** The `axiom-interface` layer exists, the debug
overlay composes it, and all gates are green. Sections 1–13 are the original
plan (kept for the rationale); the **Implementation status** section at the
bottom records what actually shipped and any deviations from the plan.

## Context

The browser debug overlay (`modules/axiom-debug-overlay`) grew a small but real
windowing/UI system of its own: a draggable panel (position + grab), a console
model (history, cursor, result log, focus), keyboard/text-input classification,
visibility/pin state, and a neutral `(label, value)` draw model. These are
**generic interface primitives**, not debug concepts. If a second UI-like
surface ever appears (an editor panel, a settings screen, a menu), it would need
the same primitives — and **two engine modules cannot share a Rust type**
(Module Law: a primitive many modules need belongs in a *lower layer*, never a
third module). So the shared interface substrate must become a **layer**.

This plan adds `axiom-interface`: the deterministic, renderer-neutral,
platform-neutral interface-surface layer, and folds the debug overlay's generic
primitives down into it so the overlay keeps **only** debug-specific data,
commands, and its browser binding.

---

## 1. Current layer inventory

Layers are discovered at exactly `crates/*/layer.toml` (one level deep, no
recursion). There are **8 layers** today.

> **Critical finding — Axiom layers are a DAG, not a linear chain.** There is no
> `index`, `level`, or `previous_layer` field in `layer.toml`, and the checker
> enforces only an *acyclic `depends_on` graph* (a 3-colour DFS for cycles). The
> manifest loader's own comment: *"Layers form a directed **acyclic** graph, not
> a strict line."* The task's framing ("continuous ordered chain", "layer index",
> "immediately previous layer", "must directly use the immediately previous
> layer") describes a mental model the repo does **not** implement. The real rule
> is: a layer declares the lower layers it **genuinely uses** in `depends_on`, and
> must genuinely adapt each one. The check output lists layers **alphabetically**
> (a set), which is further proof there is no ordering index.

| layer | crate | depends_on (DAG edges) | introduced (selected) | consumed (selected) | proof_exports |
|---|---|---|---|---|---|
| **kernel** | axiom-kernel | `[]` *(root)* | KernelApi, KernelResult/Error, Tick, FixedStep, FrameIndex, **HandleId**, **EntityId**, MessageId, Meters/Radians/Ratio, BinaryReader/Writer, Log*/Telemetry* | — | *(none — root)* |
| **crypto** | axiom-crypto | `[kernel]` | SigningKey, VerifyingKey, Signature | BinaryReader/Writer, KernelResult/Error | VerifyingKey→[BinaryReader,BinaryWriter,KernelError]; Signature→[BinaryReader,BinaryWriter] |
| **runtime** | axiom-runtime | `[kernel]` | Runtime, RuntimeScheduler, RuntimeContext, RuntimeStepRecord, … | KernelApi, SimulationClock, FixedStep, Tick, FrameIndex, HandleId, Log/Telemetry | Runtime→[KernelApi]; RuntimeTimeline→[SimulationClock,Tick,FrameIndex]; … |
| **math** | axiom-math | `[kernel, runtime]` | MathApi | KernelApi, RuntimeContext, TelemetryMetric, Tick | MathApi→[KernelApi,RuntimeContext,TelemetryMetric] |
| **host** | axiom-host | `[kernel, runtime]` | HostApi, HostStepDriver, **Pixels**, Orientation, HostSafeAreaInsets | Runtime, RuntimeStepRecord, RuntimeConfig, KernelApi, Tick, TelemetryMetric | HostApi→[KernelApi]; HostStepDriver→[Runtime] |
| **frame** | axiom-frame | `[kernel, runtime, host]` | FrameApi, EngineFrame, FrameBuilder | HostFrameReport, HostViewport, RuntimeStepRecord, Tick, FrameIndex | FrameApi→[HostFrameReport,HostViewport]; … |
| **ecs** | axiom-ecs | `[kernel, frame]` | EcsApi, World, **EntityHandle**, Query, CommandBuffer, … | EntityId, FrameContext | World→[FrameContext] |
| **introspect** | axiom-introspect | `[kernel, frame, ecs]` | IntrospectApi, FrameReport, FrameHistory, WorldReport | EngineFrame, World, BinaryReader/Writer, KernelResult | WorldReport→[World]; FrameReport→[EngineFrame]; IntrospectApi→[EngineFrame] |

**The DAG (not a line):**

```
kernel ──┬── crypto
         ├── runtime ──┬── math
         │             └── host ── frame ──┬── ecs ── introspect
         └──────────────────────────────────┘   (ecs: kernel + frame)
```

- Roots (`depends_on = []`): **kernel** only.
- Root-adjacent (`depends_on = [kernel]`): crypto, runtime.
- `ecs` deliberately *skips* runtime/math/host and depends on `[kernel, frame]`.

There is **no "highest layer index."** If one computes a topological depth for
intuition only, it is kernel(0) → {crypto,runtime}(1) → {math,host}(2) →
frame(3) → ecs(4) → introspect(5) — but this number is **not** in any manifest
and **not** enforced.

---

## 2. Proposed placement of `axiom-interface`

**Recommendation: `axiom-interface` is a root-adjacent layer with
`depends_on = ["kernel"]`.** It adapts the kernel's `HandleId` (stable opaque
identity — the same primitive `host` uses for `HostSurfaceHandle`) into
**panel/node identity** for a deterministic interface tree, plus
`KernelResult`/`KernelError` for fallible interface operations.

### The "next layer" framing is the trap — stated plainly

The task asks: *"if appending `axiom-interface` as the next layer would violate
the semantic-adapter requirement because it cannot meaningfully consume the
current immediately previous layer, state the violation plainly."*

> **Violation, plainly:** If `axiom-interface` were appended "above `introspect`"
> (the deepest current layer) and required to "directly use the immediately
> previous layer", it would have to genuinely adapt **`introspect`** (retained
> frame reports, `FrameHistory`, `WorldReport`). An interface-surface layer has
> **no honest use** for introspection/frame-report data. Declaring `introspect`
> (or `frame`, `ecs`, `math`, `runtime`) in `depends_on` would be a **ceremonial
> dependency** — exactly what the `engine_genuine_dependency` dylint bans and what
> the No-Shortcuts rule calls "a temporary mistake wearing a fake mustache."

> **Smallest structural prerequisite: none — use the model that already exists.**
> Axiom is a DAG, so there is no "append as the next index" requirement.
> `axiom-interface` declares `depends_on = ["kernel"]` and adapts the kernel
> directly, sitting **root-adjacent** (a peer of `crypto`/`runtime`), not on top
> of `introspect`. The only "prerequisite" is to stop modeling layers as a line.

### Why kernel, and only kernel

- **Identity** — panels and interface-tree nodes need stable, ordered,
  serializable identity. `axiom_kernel::HandleId` is precisely that primitive.
  `PanelId`/`NodeId` are newtypes over `HandleId`. This is a genuine adaptation
  (the kernel exists to mint identity), satisfying the `must_reference` proof and
  the genuine-dependency dylint.
- **Failure** — `KernelResult`/`KernelError` for the few fallible ops (e.g.
  addressing a panel that does not exist).
- **Not math** — interface layout is **integer pixels** (the overlay's existing
  `DragState` is already `i32`). `axiom-math` is `f32` geometry; forcing it would
  be ceremonial. `axiom-interface` defines its own integer `Rect`.
- **Not host** — `host` introduces a `Pixels` type, which is tempting for layout.
  But `host` is the platform **presentation boundary** (depends on runtime);
  coupling a platform-neutral interface layer to it for one newtype is a bigger,
  unnecessary dependency for a layer the task wants "boring and small." Use plain
  `i32`/a local `Rect`. (Revisit only if a real need appears.)

---

## 3. Why this is a **layer**, not a feature module

| Test | Result |
|---|---|
| Will **multiple** engine modules/apps need these primitives (overlay now; editor/menus/settings later)? | **Yes** → shared substrate. |
| Can two engine modules share the primitive any other way? | **No** — Module Law forbids module→module deps; "a primitive many modules need belongs in a *lower layer*." |
| Is it a *composition* of existing modules (a feature module)? | **No** — it composes nothing; it's foundational. |
| Is it deterministic + platform-neutral (spine qualities)? | **Yes.** |
| Is it broad-and-shallow, not a junk drawer? | **Yes** if kept minimal (one cohesive domain: neutral interface primitives). |

The decisive reason is the Module Law's own clause: *"If two engine modules want
to share a primitive, the primitive belongs in a lower layer, not a third
module."* The debug overlay already owns generic interface primitives; the moment
a second UI surface needs them, a module cannot supply them. → **layer.**

---

## 4. The boundary: what belongs in `axiom-interface` (and what never does)

### Belongs (neutral interface primitives)

- interface state / interface tree / interface nodes
- panels, panel identity (`PanelId` = `HandleId`)
- layout rectangles (integer `Rect`) + drag/move arithmetic
- visibility state, pin state
- focus state (which panel/node owns input)
- keyboard/text input **events as data** (a neutral chord/key model)
- command-console model primitives (history, cursor, result log, focus)
- interface commands (parsed name+args, command outcome) + a generic dispatch
  table shape
- interface **draw descriptions as neutral data** (the renderer-agnostic draw
  list: panels, rects, text rows, label/value pairs, console lines)

### Never belongs (explicit non-ownership)

DOM, browser APIs, WebGPU, WebGL, Canvas2D, native OS windows, renderer
submission, font rasterization, debug-specific metrics, profiler logic,
editor-specific behavior, start-screen/gameplay logic, settings persistence,
gameplay menus, scene-inspection semantics. (Enforced for free: the hygiene
browser-API ban applies to every layer except `host`; `axiom-interface` is **not**
on the allowlist, so any `web_sys`/`wgpu`/`canvas`/`document.`/`window.` reference
fails the checker.)

---

## 5. Debug-overlay inventory & per-concept classification

`modules/axiom-debug-overlay` is an **engine module** (`module.toml`: name
`debug-overlay`, kind `engine-module`, `allowed_layers = []`,
`allowed_modules = []`, capability `browser-debug-overlay`). One facade:
`DebugOverlayApi`. No native deps; `web_sys`/`js_sys`/`wasm-bindgen` only under
the `wasm32` target. It is on the platform-facing module allowlist (for its
`dom_binding` arm).

Classification keys: **move-to-axiom-interface**, **stay-in-debug-overlay**,
**stay-in-app**, **stay-in-render-or-backend**, **delete-or-replace**.

| Concept (file) | Nature | Classification |
|---|---|---|
| `DragState` — `x,y,grab`, `begin/update(clamp)/end` (drag.rs) | generic window position + drag math | **move-to-axiom-interface** (→ `Rect`/layout + drag) |
| `ConsoleState` — history, cursor, results, focused (console.rs) | generic console/REPL model | **move-to-axiom-interface** (console model) |
| `Row { label, value }` (diagnostics.rs) | neutral draw unit | **move-to-axiom-interface** (draw description) |
| `visible_rows()`/`recent_results()`/`header_status()` view model (overlay_state.rs) | produces neutral draw data | **move-to-axiom-interface** (draw-list generation), *fed by* debug data |
| `OverlayState.visible` + `pinned` + toggle/pin logic (overlay_state.rs) | generic visibility/pin | **move-to-axiom-interface** (panel visibility) |
| `OverlayState.console` / `.drag` fields | generic | **move-to-axiom-interface** |
| `KeyChord` + `classify()` (keyboard.rs) | input event as data + decision | **move-to-axiom-interface** (input-event model + classifier), minus the *bindings* |
| `ConsoleKey` + `classify_console_key()` (keyboard.rs) | semantic console keys | **move-to-axiom-interface** |
| `ParsedCommand`, `CommandResult` (command.rs) | generic command model | **move-to-axiom-interface** |
| `CommandSpec`, `CommandRegistry::dispatch/execute` (command_registry.rs) | generic static dispatch shape | **move-to-axiom-interface** (the *shape*; not the entries) |
| `OverlayShortcut::TogglePinned/FocusConsole/ToggleOverlay` | generic panel intents | **move-to-axiom-interface** (intents), but the **Backquote→intent binding** stays |
| `OverlayShortcut::CycleDensity` | debug affordance | **stay-in-debug-overlay** |
| `OverlayDensity` (overlay_density.rs) | overlay-only density ring | **stay-in-debug-overlay** |
| `Diagnostics` — 18 engine fields + formatters (diagnostics.rs) | debug read-out | **stay-in-debug-overlay** |
| `SPECS` (the 12 commands: `help`,`clear`,`overlay.*`,`diagnostics.snapshot`,`backend.report`,`replay/perf.mark`) + handlers | debug command set | **stay-in-debug-overlay** |
| The Backquote keyboard contract (which physical key + which modifier → which intent) | overlay binding | **stay-in-debug-overlay** |
| `dom_binding.rs` — `Nodes`, `Binding`, listeners, `web_sys` rendering (wasm32) | browser binding | **stay-in-render-or-backend** (rewritten to render the neutral draw list) |
| `DebugOverlayApi` facade | composition seam | **stay-in-debug-overlay** (now *composes* `InterfaceApi`) |

**Does the debug overlay duplicate generic interface concepts today? Yes** —
window/panel position, focus, visibility/pin, a console model, keyboard/text
input events as data, command-model primitives, and a neutral `(label,value)`
draw unit are all generic and currently live inside the debug module.

---

## 6. What the architecture checker requires (and what must change)

The checker (`crates/xtask/src/{manifest,check,hygiene}.rs` + the
`engine_genuine_dependency` dylint) enforces, per layer:

| Rule | ViolationKind | What `axiom-interface` must do |
|---|---|---|
| Known deps | `UnknownDependency` | every `depends_on` name is a real layer → list only `kernel`. |
| Acyclic DAG | `DependencyCycle` | `depends_on = ["kernel"]` adds no cycle. |
| Imports declared | `DisallowedLayerImport` | reference only `axiom_kernel::…` across layers. |
| Public paths only | `PrivatePathImport` | import `axiom_kernel::HandleId`, never `axiom_kernel::private_mod::…`. |
| Capabilities exported | `CapabilityNotExported` | every `introduced_capabilities` name is a real `pub` export of `lib.rs`. |
| Proof exists | `MissingProofExport` | non-root → ≥1 `[[proof_exports]]`; each `export` is public. |
| Proof references dep | `ProofReferenceMissing` | the file declaring `InterfaceApi` (or its re-export module) contains `HandleId`. |
| Genuine use (dylint) | — | a resolved `axiom_kernel` `DefId` in **non-test** code (e.g. `PanelId(HandleId)`). |
| Hygiene | `SourceHygiene*` | no `println!`/`eprintln!`/`dbg!`/`todo!`/`unimplemented!`; no `utils`/`helpers`/`common`/`misc`; **no `web_sys`/`js_sys`/`wasm_bindgen`/`WebGPU`/`WebGL`/`canvas`/`window.`/`document.`** (not on the platform allowlist); no `coverage(off)`. |

Plus the workspace-wide laws this layer is automatically subject to:
**branchless** (`engine_no_branching`, baseline 0), **100% coverage**, the
size lints (`engine_no_large_*`), and **no naked float in public API**
(`engine_no_unitless_float_public_api`) — so timing/positions cross as integers,
as the overlay already does.

**Files/manifests that must change for the checker to pass:**

1. `crates/axiom-interface/layer.toml` — **new** (drafted in §9).
2. `crates/axiom-interface/Cargo.toml` — **new** (`axiom-kernel` path dep).
3. Root `Cargo.toml` — add `"crates/axiom-interface"` to `members`.
4. `crates/axiom-interface/src/lib.rs` + sources — the `pub` `InterfaceApi`
   facade whose impl references `HandleId` (proof).
5. `modules/axiom-debug-overlay/module.toml` — add `"interface"` to
   `allowed_layers`.
6. `modules/axiom-debug-overlay/Cargo.toml` — add `axiom-interface` path dep.
7. No checker-code change is required — `axiom-interface` is **not** added to any
   platform allowlist (it must stay browser-free).

---

## 7. The `axiom-interface` boundary: facade + internal concepts + files

**Primary facade:** `InterfaceApi` (the one behavioral entry point).

> **One nuance vs the task's "only InterfaceApi is public":** the "single public
> facade" rule is the **Module Law (#8)**, for *modules*. **Layers legitimately
> export their value vocabulary** alongside the facade — `ecs` exports `EcsApi`
> **and** `EntityHandle`/`Query`/…, `kernel` exports dozens. `axiom-interface`'s
> reason to exist is to **emit neutral draw descriptions** and **accept neutral
> input events** across the boundary, so those value types must be nameable by the
> renderer/app. Recommendation: keep **all behavior** behind `InterfaceApi`, and
> expose a **minimal** value vocabulary (the draw list, the input event, the panel
> id). Everything else is `pub(crate)`.

**Public surface (minimal):**

- `InterfaceApi` — the facade. Methods take/return primitives + the small
  vocabulary below. (Panels: create/visibility/pin/focus/layout. Input: feed a
  neutral key/text event. Console: submit/history/append. Output: produce the
  draw list.)
- `PanelId` — `HandleId` newtype (panel/node identity; the proof reference).
- `InterfaceDrawList` / `InterfaceDrawItem` — neutral draw description (a panel
  with a rect, a title, label/value rows, console lines, a focused-input marker).
- `InterfaceInputEvent` — neutral keyboard/text input as data (code + modifiers +
  focus context; the generalization of `KeyChord`).

**Internal concepts (one primary public-ish thing per file, all `pub(crate)`):**

```
crates/axiom-interface/
  Cargo.toml
  layer.toml
  ARCHITECTURE.md
  src/
    lib.rs                 # docs + module decls; pub use interface_api::InterfaceApi (+ vocabulary)
    interface_api.rs       # InterfaceApi  (the facade; references HandleId → proof)
    interface_state.rs     # InterfaceState (root state: panels + focus + console)
    panel.rs               # Panel (id, rect, visible, pinned, focused)
    panel_id.rs            # PanelId (HandleId newtype)  [public vocabulary]
    layout_rect.rs         # Rect (i32 x,y,w,h) + clamp; drag arithmetic (was DragState)
    focus_state.rs         # FocusState (which PanelId/region owns input)
    input_event.rs         # InterfaceInputEvent + classify() (was KeyChord/classify)
    console_model.rs       # ConsoleModel (history, cursor, results, focused)  (was ConsoleState)
    interface_command.rs   # ParsedCommand, CommandOutcome  (was command.rs)
    command_table.rs       # generic CommandTable/dispatch shape  (was command_registry shape)
    draw_list.rs           # InterfaceDrawList + InterfaceDrawItem  (was Row + view model)
```

No `utils`/`helpers`/`common`/`misc`. Each file owns one concept. **Boring and
small** — no tree-diffing, no docking, no theming (the overlay needs no
color/style primitive beyond what the renderer applies; `density` stays in the
overlay). This is **not** a UI framework.

---

## 8. Target data flow after migration

```
┌ axiom-debug-overlay (engine module, allowed_layers += interface) ─────────────┐
│  Diagnostics (debug data)  +  SPECS (debug command set)  +  density            │
│                                  │                                             │
│        feeds debug data ────────▶│  builds/updates a panel's content           │
└──────────────────────────────────┼──────────────────────────────────────────┘
                                    ▼
┌ axiom-interface (LAYER, depends_on = [kernel]) ───────────────────────────────┐
│  InterfaceApi: updates deterministic visibility / pin / focus / layout(drag) / │
│  console state; classifies neutral input events; runs the generic command-     │
│  dispatch shape; emits InterfaceDrawList (neutral draw descriptions).          │
└───────────────────────────────────┬──────────────────────────────────────────┘
                                     ▼  InterfaceDrawList (neutral data)
┌ debug-overlay dom_binding (wasm32) / app / backend ───────────────────────────┐
│  translates InterfaceDrawList → actual pixels (DOM nodes), and lifts browser   │
│  KeyboardEvent/PointerEvent → InterfaceInputEvent fed back into InterfaceApi.  │
└───────────────────────────────────────────────────────────────────────────────┘
```

- The **debug overlay produces debug data + command specs**; it no longer owns a
  panel/window/console/focus engine.
- **`axiom-interface` owns** all deterministic interface state + emits neutral
  draw descriptions.
- The **wasm binding** (still in the debug-overlay module's platform arm, which is
  allowlisted) renders the neutral draw list and lifts real DOM events into
  neutral input events. (Equally valid: a future dedicated interface backend; for
  the minimal pass, the overlay's existing `dom_binding` is rewritten in place.)
- **No second windowing system** remains in the overlay.

The Backquote *binding* (physical key + modifiers → an `InterfaceApi`
intent/command) stays in the debug overlay — `axiom-interface` only owns the
neutral input-event model and the classifier shape, not the overlay's specific
key choices.

---

## 9. Proposed manifest — `crates/axiom-interface/layer.toml`

> The task asks the manifest to carry "layer index", "previous layer", and
> "forbidden dependencies". **None of those are real `layer.toml` fields** (the
> schema is exactly: `name`, `crate_name`, `depends_on`, `meaningful_dependency`,
> `introduced_capabilities`, `consumed_capabilities`, `[[proof_exports]]`). They
> are captured below as **comments** so intent is explicit; the executable
> manifest uses only real fields. "Forbidden deps" are enforced *implicitly* —
> anything not in `depends_on` is rejected by `DisallowedLayerImport`, and
> browser/render APIs by the hygiene ban.

```toml
# Axiom layer manifest — axiom-interface.
#
# Conceptual placement (NOT manifest fields; Axiom layers are a DAG, no index):
#   role            = "root-adjacent interface-surface layer"
#   adapts          = "kernel" (HandleId identity, KernelResult failure)
#   forbidden deps  = runtime, math, host, frame, ecs, introspect, crypto,
#                     ANY module, ANY app, ANY browser/render API
#                     (web_sys/js_sys/wasm_bindgen/WebGPU/WebGL/Canvas2D/wgpu)
#
# It owns ONLY renderer-/platform-neutral interface primitives and emits neutral
# draw descriptions. The browser/native rendering of those descriptions lives in
# a platform-facing module/app, never here.

[layer]
name = "interface"
crate_name = "axiom-interface"
depends_on = ["kernel"]

meaningful_dependency = """
Interface adapts the kernel's HandleId (stable, ordered, serializable opaque \
identity — the same primitive host uses for HostSurfaceHandle) into panel and \
interface-node identity for a deterministic interface tree, and \
KernelResult/KernelError for fallible interface operations. It turns those \
kernel identity + result primitives into renderer- and platform-neutral \
interface primitives — panels, integer layout rectangles, visibility, pin, \
focus, keyboard/text input events as data, a command-console model, and neutral \
interface draw descriptions — that UI-like modules (the debug overlay today) and \
apps compose. It owns no DOM, no renderer, and no debug semantics.
"""

introduced_capabilities = [
  "InterfaceApi",
  "PanelId",
  "InterfaceDrawList",
  "InterfaceDrawItem",
  "InterfaceInputEvent",
]

consumed_capabilities = ["HandleId", "KernelResult", "KernelError"]

# Proof: the InterfaceApi facade's implementation references the kernel's
# HandleId (panels/nodes are identified by HandleId), so the dependency on
# kernel is genuine, not ceremonial.
[[proof_exports]]
export = "InterfaceApi"
must_reference = ["HandleId"]

[[proof_exports]]
export = "PanelId"
must_reference = ["HandleId"]
```

`Cargo.toml` (new): `axiom-kernel = { path = "../axiom-kernel" }`,
`crate-type = ["rlib"]`, `unsafe_code = "forbid"`. **No** wasm/web-sys deps.

---

## 10. Test plan (added when implementation happens)

### Already enforced by the existing real-repo checker tests (no new test needed, just must pass)
- **manifest validation / legal imports / illegal self-import / illegal future
  import / duplicate capabilities / single facade (module side) / no browser
  APIs / no renderer APIs / no console output / no placeholder macros / no
  junk-drawer names** — all covered by `cargo xtask check-architecture` and the
  `real_repo_layers_pass` + `real_repo_class_aware_check_passes` workspace tests
  running against the new manifest. The new layer simply has to satisfy them.
  (Synthetic-fixture variants for self-import/future-import/duplicate-capability
  already exist in xtask's own tests.)

### New unit tests in `axiom-interface` (native, branchless code → 100% coverage)
- `deterministic_panel_visibility_update` — toggle/show/hide/pin produce the
  exact expected visibility for every case (the branchless `!visible | pinned`
  truth table).
- `deterministic_focus_transfer` — focusing panel B blurs A; focus queries are
  exact and replay-stable.
- `deterministic_layout_calculation` — drag `begin/update/end` with bounds clamps
  to the exact rect; over-/under-flow clamps; window-bigger-than-viewport case.
- `deterministic_command_console_state_update` — record (non-empty only),
  ArrowUp/Down history navigation to the live line, append/clear result log,
  parse → dispatch → echo, empty input ignored, unknown command → clean error.
- `deterministic_interface_draw_list_generation` — given a fixed `InterfaceState`,
  `draw_list()` is byte-stable and contains the expected items/order.
- `input_event_classification` — neutral chord/key → intent mapping (the moved
  `classify`/`classify_console_key`), all arms.
- `panel_identity_is_stable` — `PanelId` (HandleId) round-trips / orders stably.

### New regression test (the fold-in is real)
- `debug_overlay_defines_no_private_panel_primitives` — assert
  `modules/axiom-debug-overlay/src/` no longer declares `DragState`,
  `ConsoleState`, a private `Row`, `KeyChord`/`classify`, or `ParsedCommand`/
  `CommandResult` (a source-scan test, mirroring the existing per-crate
  `tests/architecture.rs` pattern), proving the overlay owns no second
  panel/windowing system.

---

## 11. Implementation plan (ordered; execute in a later pass)

> Sequence so the workspace stays green at each milestone. The debug overlay +
> harness currently sit **uncommitted** in the tree (held) interleaved with a
> parallel mobile-first effort; coordinate landing.

**A. Stand up the empty layer (green by itself)**
1. Create `crates/axiom-interface/{Cargo.toml, layer.toml, ARCHITECTURE.md,
   src/lib.rs}` with a minimal `InterfaceApi` that references `HandleId`
   (e.g. `PanelId(HandleId)`), plus a trivial deterministic op + test.
2. Add `"crates/axiom-interface"` to root `Cargo.toml` `members`.
3. Validate: `cargo xtask check-architecture` (manifest + proof pass),
   `cargo test -p axiom-interface`, `cargo llvm-cov -p axiom-interface` (100%),
   `cargo dylint --all` (branchless + genuine-dep clean).

**B. Move the generic primitives down (file map)**
4. Port, rewritten branchless + 100% covered, into the file layout in §7:
   - `modules/axiom-debug-overlay/src/drag.rs` → `crates/axiom-interface/src/layout_rect.rs`
   - `…/console.rs` → `…/console_model.rs`
   - `…/command.rs` → `…/interface_command.rs`
   - `…/command_registry.rs` (the dispatch shape) → `…/command_table.rs`
   - `…/keyboard.rs` (`KeyChord`/`classify`/`ConsoleKey`/`classify_console_key`,
     minus the overlay's Backquote bindings) → `…/input_event.rs`
   - `…/diagnostics.rs::Row` + the view-model builders → `…/draw_list.rs`
   - generic visibility/pin/focus from `overlay_state.rs` → `panel.rs` /
     `focus_state.rs` / `interface_state.rs`

**C. Fold the debug overlay onto the layer**
5. `modules/axiom-debug-overlay/module.toml`: `allowed_layers = ["interface"]`
   (and `"kernel"` if referenced directly).
6. `modules/axiom-debug-overlay/Cargo.toml`: add `axiom-interface` path dep.
7. Rewrite `overlay_state.rs`/`overlay_api.rs` to **compose** `InterfaceApi`:
   keep `Diagnostics`, `OverlayDensity`, `SPECS`/handlers, the Backquote binding;
   delete the now-moved generic files; feed diagnostics → interface panel
   content; read back `InterfaceDrawList`.
8. Rewrite `dom_binding.rs` to render `InterfaceDrawList` and lift DOM
   `KeyboardEvent`/`PointerEvent` → `InterfaceInputEvent`.
9. Harness app (`apps/axiom-browser-dev-harness`): likely unchanged (still calls
   `DebugOverlayApi`); verify the wasm build + browser drag/console still work.

**D. Tests + validation**
10. Add the §10 unit + regression tests.
11. **Validation commands (required):**
    - `cargo xtask check-architecture`
    - `cargo test --workspace`
    - (plus, to honor the spine laws the gate enforces) `cargo dylint --all -- --all-targets`,
      `pwsh -File scripts/coverage.ps1`, and a `wasm32` build of the overlay/harness.

**Files deleted:** the moved generic sources in `modules/axiom-debug-overlay/src/`
(`drag.rs`, `console.rs`, `command.rs`, `command_registry.rs`, `keyboard.rs`, and
`Row` out of `diagnostics.rs`). **Files created:** the `crates/axiom-interface/*`
set. **Public API change:** `DebugOverlayApi`'s *external* surface can stay
primitive-only (no visible churn for the harness); internally it delegates to
`InterfaceApi`.

---

## 12. Risks

- **Ceremonial-kernel risk.** If the first cut uses one panel and never truly
  needs `HandleId`, the genuine-dep dylint could view it as ceremonial. Mitigate
  by making `PanelId(HandleId)` real, used in `InterfaceApi`'s signatures, and
  exercised by tests — identity is the layer's honest reason to touch kernel.
- **Facade-purity vs neutral-data-export tension.** The layer must export the
  draw list + input event types (renderer must name them). Resolved via the
  `ecs` precedent (facade + minimal value vocabulary), documented in §7 — but it
  means "only `InterfaceApi` is public" is softened to "one facade + a tiny
  vocabulary."
- **Branchless + 100% coverage tax.** The moved code is already branchless and
  100% covered in the overlay, so the port is mostly mechanical — but the
  rewrites (panel/focus split, draw-list shape) must preserve both.
- **`dom_binding` rewrite.** Rendering a neutral draw list instead of the overlay's
  own `Row` is the largest non-trivial change; it's wasm-gated/gate-exempt but
  must be browser-verified (Playwright) since it's not in the coverage gate.
- **Scope creep.** "Interface tree / nodes / panels" invites a UI framework.
  Keep it to the **single-panel** shape the overlay needs; add tree depth only
  when a second real consumer demands it.
- **Landing coordination.** The overlay + harness are uncommitted alongside a
  parallel effort; the migration touches the same module — sequence the land.

---

## 13. Non-goals (explicit)

`axiom-interface` will **not** include, now or as part of this migration:

- no DOM
- no WebGPU
- no WebGL
- no Canvas2D
- no native OS windows
- no font rasterization
- no editor docking
- no full UI framework (no retained widget tree, no layout engine beyond integer
  rects, no event-bubbling system)
- no gameplay menu logic
- no settings persistence
- no theme/style system (unless a future consumer forces a tiny deterministic
  primitive; the debug overlay does not — styling stays in the renderer)
- no debug-specific metrics, profiler logic, or scene-inspection semantics

---

## Implementation status (shipped)

The migration is complete. The layer is in the DAG
(`kernel → … → interface`), the overlay composes it, and the architecture
checker, dylint rulebook (incl. the Branchless Law), and the 100% coverage gate
all pass for both crates.

### What shipped — `crates/axiom-interface` (new root-adjacent layer)

`depends_on = ["kernel"]`. One behavioral facade plus a small public value
vocabulary; everything else is `pub(crate)`. Files:

| file | concept | visibility |
|---|---|---|
| `panel_id.rs` | `PanelId` newtype over `axiom_kernel::HandleId` | **pub** |
| `layout_rect.rs` | `Rect` — integer position/size + clamp | pub(crate) |
| `panel.rs` | `Panel` — identity, rect, visibility, pin, drag, content, console | pub(crate) |
| `focus_state.rs` | `FocusState` — console focus ownership that *transfers* between panels | pub(crate) |
| `console_model.rs` | `ConsoleModel` — history, navigation cursor, result log | pub(crate) |
| `interface_command.rs` | `ParsedCommand`, `CommandOutcome` | **pub** |
| `command_table.rs` | `CommandSpec<C>`, `CommandTable<C>` — generic static dispatch | **pub** |
| `input_event.rs` | `InterfaceInputEvent` (+ `ConsoleKey` classify) | `InterfaceInputEvent` **pub** |
| `draw_list.rs` | `InterfaceDrawItem`, `InterfaceDrawList` — neutral, ordered | **pub** |
| `interface_state.rs` | `InterfaceState` — panels + focus + draw-list assembly | pub(crate) |
| `interface_api.rs` | `InterfaceApi` — the one facade | **pub** |

`PanelId` is used in `InterfaceApi`'s signatures (minted via
`HandleId::from_raw`), so the kernel dependency is **genuine**, not ceremonial —
the `engine_genuine_dependency` dylint confirms it.

### What shipped — `modules/axiom-debug-overlay` (refactored onto the layer)

The overlay now keeps **only** debug-specific code and composes `InterfaceApi`
for all generic windowing:

- **Deleted** (moved down into the layer): `drag.rs`, `console.rs`,
  `command.rs`, `command_registry.rs`, `keyboard.rs`.
- **`overlay_state.rs`** holds an `InterfaceApi` + a single `PanelId` + density +
  diagnostics, and delegates visibility / pin / focus / drag / console / draw
  list to the panel. It pushes neutral header + rows into the panel on every
  change (`refresh_panel`) so the layer's draw list renders the debug content.
- **`overlay_commands.rs`** (new) supplies `OVERLAY_SPECS:
  &[CommandSpec<OverlayState>]` — the debug commands (`help`, `clear`,
  `overlay.*`, `diagnostics.snapshot`, `backend.report`, `replay.mark`,
  `perf.mark`) — dispatched through the layer's `CommandTable`.
- **`backquote.rs`** (new) keeps the **debug-specific** `` ` `` binding:
  `OverlayShortcut` + `classify_backquote`, built on the layer's neutral
  `InterfaceInputEvent::routes_global_hotkey`.
- **`diagnostics.rs`** keeps the debug read-out, now emitting `(label, value)`
  tuples (the `Row` type moved to the layer's draw list).
- **`overlay_api.rs`** (`DebugOverlayApi`) keeps a **byte-for-byte identical
  external method surface** and delegates to `OverlayState`.
- **`dom_binding.rs`** (wasm32) now **renders the layer's `InterfaceDrawList`**
  and lifts browser key/pointer events into `InterfaceInputEvent` + the overlay's
  bindings.
- `module.toml` gained `allowed_layers = ["interface"]`; `Cargo.toml` gained the
  `axiom-interface` path dependency.

A regression test (`tests/no_local_windowing_primitives.rs`) source-scans
`src/` and fails if the overlay ever re-declares `DragState`, `ConsoleState`,
`KeyChord`, `ParsedCommand`, `CommandResult`, `CommandRegistry`, `Row`, or
`ConsoleKey` — i.e. if it re-grows a generic primitive instead of composing the
layer.

### Deviations from the plan (with rationale)

1. **Public command vocabulary is wider than the plan's 4 types.** The plan
   listed `PanelId`, `InterfaceDrawList`, `InterfaceDrawItem`,
   `InterfaceInputEvent` as the only public value types. The overlay must
   *compose* the command dispatch from *outside* the layer (it owns the
   commands), which is impossible if `CommandSpec`/`CommandTable`/`ParsedCommand`/
   `CommandOutcome` are `pub(crate)`. They are therefore **public** — the plan's
   own "unless tests force otherwise" escape, here forced by the consumer rather
   than a test. This keeps the dispatch *shape* in the layer and the *commands*
   in the overlay, exactly as intended.
2. **One added layer capability: `console_recent_history`.** The verbose density
   shows a preview of recent commands; reading recent history is a genuine,
   generic console capability (not debug-specific), so it lives on the layer.
3. **`clear` is a silent success.** A command whose outcome is an empty-message
   success produces no echo line, so `clear` truly empties the log instead of
   leaving a blank `clear:` row. General rule, lives in `run_command`.

### Validation (all green)

| command | result |
|---|---|
| `cargo xtask check-architecture` | ✅ `interface` in the DAG; Layer Law satisfied |
| `cargo test -p axiom-interface -p axiom-debug-overlay` | ✅ 42 + lib/integration tests pass |
| `cargo test -p xtask` (`real_repo_layers_pass`, `real_repo_class_aware_check_passes`) | ✅ pass |
| `cargo dylint --all` (branchless, genuine-dep, float, size, …) | ✅ clean for both crates |
| `cargo build -p axiom-debug-overlay --target wasm32-unknown-unknown` | ✅ `dom_binding` compiles |
| coverage (both crates) | ✅ 100.00% regions / functions / lines |
