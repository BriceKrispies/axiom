# `axiom-interface` — architecture

The deterministic, renderer-neutral, platform-neutral **interface-surface layer**.

## What it is

The neutral substrate behind engine-facing, UI-like surfaces. It owns:

- interface state / tree (panels), panel **identity**
- integer **layout rectangles** + drag-to-move (begin/update/clamp/end)
- **visibility** and **pin** state
- **focus** ownership (single owner, transfers between panels)
- **keyboard/text input events as data** (`InterfaceInputEvent`) + console-key
  classification
- a **command-console model**: history, navigation, result log; a parsed-command
  + command-outcome value model; and a generic `CommandTable<C>` dispatch shape
- **neutral interface draw descriptions** (`InterfaceDrawList` / `InterfaceDrawItem`)

It is small and boring by design — not a UI framework. No retained widget tree,
no docking, no themes, no layout engine beyond integer rects, no event bubbling.

## Why it is a layer (not a module)

The debug overlay had grown its own panel/window/console/focus system. The moment
a second UI-like surface (an editor panel, a menu, a settings screen) needs the
same primitives, an engine **module cannot supply them** — modules may not depend
on other modules. The Module Law's own rule settles it: *"a primitive many modules
need belongs in a lower layer, not a third module."* So the shared interface
substrate is a layer, and the debug overlay (and future UI modules) compose it.

## Why it depends only on `kernel`

Axiom layers form a **DAG** keyed on `depends_on` (there is no linear index and no
"previous layer"). An interface surface genuinely needs exactly one lower-layer
primitive: **stable identity**. That is the kernel's `HandleId`. It does **not**
need runtime (stepping), math (`f32` geometry — UI layout is integer pixels), host
(the platform boundary), frame, ecs, or introspect; declaring any of them would be
a ceremonial dependency the `engine_genuine_dependency` dylint bans. So
`depends_on = ["kernel"]`, making it root-adjacent (a peer of crypto/runtime),
**not** a layer stacked above `introspect`.

## How it adapts `HandleId`

`PanelId` is a newtype over `axiom_kernel::HandleId`; `InterfaceApi::add_panel`
mints one (`HandleId::from_raw` over a monotonic counter starting at 1, so handles
are valid/non-null). Panels and the focus owner are keyed by `PanelId`. This is
the genuine, proven kernel use (`[[proof_exports]]` for `InterfaceApi` and
`PanelId` both `must_reference = ["HandleId"]`). The kernel's `KernelResult`/
`KernelError` are available for a future fallible operation; the current surface is
infallible (lookups degrade to no-ops), so it does not yet use them.

## What does **not** belong here

DOM, browser APIs, WebGPU/WebGL/Canvas2D, native OS windows, renderer submission,
font rasterization, editor docking, a full UI framework, gameplay menu logic,
settings persistence, debug metrics, profiler logic, or scene-inspection
semantics. The hygiene gate enforces the browser/render ban: this layer is **not**
on the platform allowlist, so any `web_sys`/`wgpu`/`canvas`/`document.` reference
fails the architecture checker.

## How the debug overlay uses it

`modules/axiom-debug-overlay` (a platform-facing engine module) declares
`allowed_layers = ["interface"]` and composes `InterfaceApi`:

1. It creates one panel and drives its visibility / pin / focus / drag through
   `InterfaceApi`.
2. It keeps the **debug-specific** data and commands — `Diagnostics`,
   `OverlayDensity`, and its command set (`help`, `overlay.*`,
   `diagnostics.snapshot`, `backend.report`, `replay.mark`, `perf.mark`) — and
   feeds neutral `(label, value)` rows + header text into the panel.
3. Its command dispatch is a `CommandTable<OverlayState>` (this layer's generic
   shape) over the overlay's own command specs.
4. Each repaint it reads back the panel's `InterfaceDrawList`.

## Why browser rendering stays outside the layer

This layer emits **neutral draw descriptions** and accepts **neutral input
events**. Turning a draw list into pixels and lifting real `KeyboardEvent`/
`PointerEvent`s into neutral events is platform work — it lives in the debug
overlay's `#[cfg(target_arch = "wasm32")]` arm (the allowlisted platform-facing
module), or a future native/canvas backend. Keeping it out preserves the layer's
determinism and lets any backend render the same interface.
