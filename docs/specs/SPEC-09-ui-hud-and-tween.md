# SPEC-09 — UI/HUD overlay & tween/easing

> Status: Landed
> Landed (2026-06-28): `axiom-interface` gained the immediate-mode `UiSurface`; new module `axiom-tween` (`TweenApi` + `ease`); `@axiom/game` `Sim.tweens` (`makeTweens`/`EASES`) sampled by the `TickPump`. `solveLayout` projects the already-landed `axiom-layout::solve`. The §2 gaps below are now closed.
> Contract: §14, §12   Vocabulary: Overlay screens/modals, Responsive layout solver, Canvas-drawn HUD, Stat/leaderboard panels, Floating popups/toasts, Immediate-mode button, Tween/easing, Flip-book   Determinism: presentation

## 1. Summary

The screen-space layer drawn *after* the world: HUDs, menus, pause/result
screens, toasts, and the immediate-mode `button()` the author hit-tests against —
plus `tween`/`ease`, the generic display-value animation every presentation
surface wants. Both are **presentation** (§17.5): they draw and animate what is
*seen*, never what is *simulated*, and no value either produces may re-enter sim.

All 11 games need a HUD; the menu/pause/result loop is universal; tweens (n=5)
are how juice — a popping score, a sliding panel, a fading toast — is expressed
without hand-rolling per-frame interpolation. The engine supplies **drawing and
hit-testing, not a widget framework**: menus and screens are composed by the
author from these primitives plus an author-held screen `StateMachine` (SPEC-07).

## 2. Current state (verified)

- **`axiom-interface` (layer, `depends_on = ["kernel"]`) has retained panels, not
  a screen-space surface.** `InterfaceApi` owns panel visibility/pin/focus/drag, a
  console (history/command-table/recall), label/value rows, and action buttons. It
  emits a **neutral retained draw list** (`InterfaceDrawList` of `InterfaceDrawItem::{Panel,
  Header,Row,Button,ConsoleLine,ConsoleInput}`) in **integer** coordinates and owns
  no renderer. Its `Button { action: u32, label }` is *not* immediate-mode: it
  carries a consumer-defined id and a click is routed back by the consumer via a
  branchless dispatch table — the layer never reports "activated this frame."
- **`axiom-layout` (layer, `depends_on = ["kernel", "host"]`) already is the
  responsive solver.** `solve(viewport: &HostViewport, tree: &LayoutTree) ->
  LayoutResult` does single-pass, recursion-free, branchless flex placement with
  safe-area insets and orientation-adaptive stacking — exactly the contract's
  `solveLayout`.
- **Missing:** the screen-space `Ui` surface (float, top-left origin, `+y` down);
  `rect`/`text`/`sprite` screen variants; an immediate-mode `button(bounds,label,
  style) -> bool`; `viewport`; floating popups/toasts; and **all** of `tween`/`ease`
  — there is no easing or tween code anywhere in the tree. The TS `Ui`/`solveLayout`/
  `tween` surface is absent.

## 3. Architectural placement

Two pieces: **extend the `axiom-interface` layer** (the Ui surface) and a **new
isolated module `axiom-tween`**.

**(a) Ui surface → extend `axiom-interface`.** The interface layer's charter is
"renderer-/platform-neutral interface draw descriptions over kernel identity." A
screen-space immediate-mode HUD surface is the *same* responsibility in a second
mode, not a new one — so it extends the layer rather than forking a module. Add,
beside the retained `InterfaceState`, a per-frame `UiSurface`:

- screen-space draw variants `rect`/`text`/`sprite` accumulated into a new
  **float** `UiDrawList` (top-left origin, `+y` down — distinct from the retained
  integer `InterfaceDrawList`);
- an **immediate-mode `button(bounds, label, style) -> bool`** that draws the
  button *and* returns whether it was activated this frame, hit-testing `bounds`
  against a **per-frame pointer snapshot** the loop feeds in;
- `viewport` (logical width/height), fed in per frame.

The Ui surface stays a **kernel-only adapter**: its draw items carry *primitive*
style fields (an `Rgba` of four `f32`, stroke width `f32`), never the contract's
`FillStroke`/`Common`/`TextOpts` types — those are owned by the 2D surface
(SPEC-04, an `axiom-draw2d` module). A layer cannot import a module (Module Law
#1), so the runtime app translates the contract style records onto the surface's
primitive fields, exactly as today's `Button { action, label }` stays neutral.

`solveLayout` is **not** added to interface. It is the direct projection of the
already-landed `axiom-layout::solve`; the runtime app translates the contract
`LayoutNode` tree into a `LayoutTree`, calls `solve`, and returns the `Record<
string, Rect>`. This keeps interface from taking a `host` edge (via layout) it
does not need — placement is layout's job, drawing is interface's.

**(b) Tween/ease → new module `axiom-tween` (`TweenApi`, `kind = "engine-module"`,
`allowed_layers = ["kernel"]`, `allowed_modules = []`).** Tween is **generic
display-value animation** — a number `from → to` over a duration under an ease
curve — with **no UI in it**. It must not live in `axiom-interface`: a non-UI
consumer (the 2D surface animating a sprite's position/alpha, a 3D presentation
fading a value) would then have to drag in the entire interactive-panel layer to
tween, conflating "interactive windowing" with "value interpolation," two
unrelated responsibilities. The shared primitive gets its own isolated capability
(Module Law #2): each presentation surface is driven *alongside* tween by the
**app** (modules never import modules), so "reused by 2D and UI both" is an app-
tier composition, not a module dependency.

`axiom-tween` is **pure**: it owns the ease functions and a tween table keyed by
`TweenId`, advanced by an **elapsed presentation interval the app supplies**, and
yields the current sampled value + completion per tween. It needs **no platform
arm** — there is no Web API here (unlike audio), only arithmetic over a fed-in
clock — so it is a deterministic, fully-covered, branchless core with nothing
compiled out. The author's `onUpdate`/`onComplete` **closures are not native
data**: they live app-side keyed by `TweenId`; the native module hands back
samples and the app fires the closures.

Both are **presentation-class spine**: branchless, 100% covered, but `§17.5`-
excluded — every output is display-only and must never be read into a `sim` API.

## 4. API surface

### 4.1 Native

`axiom-interface` (extend, presentation):

```rust
impl UiSurface {                       // new, beside InterfaceState in InterfaceApi
    pub fn begin_frame(&mut self, viewport: UiViewport, pointer: PointerSnapshot);
    pub fn rect(&mut self, bounds: UiRect, style: UiFill);
    pub fn text(&mut self, value: &str, opts: UiTextOpts);
    pub fn sprite(&mut self, texture: HandleId, opts: UiSpriteOpts);
    pub fn button(&mut self, bounds: UiRect, label: &str, style: UiFill) -> bool; // activated this frame
    pub fn viewport(&self) -> UiViewport;
    pub fn draw_list(&self) -> &UiDrawList;   // this frame's accumulated screen-space items
}
```

`axiom-tween` (new module, presentation):

```rust
impl TweenApi {
    pub fn start(&mut self, spec: TweenSpec) -> TweenId;     // { from, to, duration_secs, ease }
    pub fn cancel(&mut self, id: TweenId);
    pub fn advance(&mut self, elapsed_secs: Ratio) -> Vec<TweenSample>; // { id, value, completed }
    pub fn value(&self, id: TweenId) -> Option<f32>;
}
pub fn ease(curve: Ease, t: Ratio) -> Ratio;   // t in [0,1] → eased [0,1]; branchless table over discriminant
```

### 4.2 TS authoring projection (the contract)

```ts
// §14 — screen-space overlay (origin top-left, +y down; camera-independent)
interface Ui {
  rect(r: Rect, style: FillStroke & Common): void;
  text(value: string, opts: TextOpts): void;
  sprite(texture: TextureId, opts: SpriteOpts): void;
  button(bounds: Rect, label: string, style?: FillStroke): boolean;  // immediate-mode
  readonly viewport: { width: number; height: number };
}
interface LayoutNode { id: string; direction?: "row" | "column"; grow?: number; basis?: number;
                       aspect?: number; children?: LayoutNode[] }
function solveLayout(root: LayoutNode, viewport: Rect): Record<string, Rect>;  // ← axiom-layout::solve

// §12 — tween / easing (presentation clock; display values only)
type Ease = "linear" | "quadIn" | "quadOut" | "quadInOut" | "cubicOut" | "expoOut" | "backOut";
type TweenId = Handle;
interface TweenSpec { from: number; to: number; duration: Seconds; ease?: Ease;
                      onUpdate: (value: number) => void; onComplete?: () => void }
function tween(spec: TweenSpec): TweenId;
function cancelTween(id: TweenId): void;
```

`FillStroke`/`Common`/`TextOpts`/`SpriteOpts`/`TextureId` are the §10 / SPEC-04
style records, reused unchanged; the app translates them onto `UiFill`/`UiTextOpts`.

## 5. Data contracts

- **`UiDrawList`** — ordered screen-space items `UiDrawItem::{Rect, Text, Sprite,
  Button}` in float coordinates with **primitive** style fields (`Rgba` as four
  `f32`, stroke width). Distinct from the retained integer `InterfaceDrawList`;
  rebuilt every frame (immediate-mode). The renderer (canvas/GPU) paints it after
  the world; the engine owns no UI renderer.
- **`PointerSnapshot`** `{ x, y, pressed_edge }` — the current-frame pointer state
  the loop feeds `begin_frame`. Presentation input; **not** the sim intent stream
  (SPEC-05). `button()` activation is `point_in(bounds, pointer) & pressed_edge`.
- **`UiViewport`** `{ width, height }` — logical screen size, fed per frame.
- **`Ease`** — 7-variant discriminant; `ease()` selects via a fn-pointer table
  indexed by `curve as usize` (the existing branchless-dispatch pattern).
- **`TweenSpec`** `{ from, to, duration_secs, ease }` (native; closures excluded),
  **`TweenSample`** `{ id, value, completed }`, **`TweenId`** = `HandleId` newtype.
- `onUpdate`/`onComplete` closures are **app-tier**, keyed by `TweenId` — they
  never cross into native, keeping the spine branchless and closure-free.

## 6. Determinism

- **Presentation-excluded (§17.5).** The Ui surface draws in screen space, is
  camera-independent, and reads a presentation pointer snapshot; tweens advance on
  the **presentation clock** (a fed-in elapsed interval). **No** value from either
  may be read back into a `sim`-class API — a HUD readout reflects sim state, it
  never sets it; a tweened display value never feeds a fixed update.
- **Spine discipline still holds.** Both cores are deterministic given their
  inputs (same pointer snapshot ⇒ same `button` result; same elapsed sequence ⇒
  same samples — and, like the SPEC-00 accumulator, sampling at total elapsed `T`
  is independent of how `T` was chunked across frames), branchless, and 100%
  covered. Presentation math is unconstrained by §17.6 (no cross-instance bit-
  exactness required), but the code is held to the spine's laws regardless.
- `button()` is immediate-mode: activation is a pure function of `(bounds,
  this-frame pointer)`, carrying no state between frames except the per-frame
  snapshot the loop installs.

## 7. Acceptance / proof

- **Ui surface (`axiom-interface`):** 100% covered, branchless. Golden tests for
  (a) `UiDrawList` item order/content across `rect`/`text`/`sprite`/`button`; (b)
  `button` hit-test truth table — pointer in/out of `bounds` × press-edge present/
  absent → activated `bool`, including boundary coordinates; (c) `begin_frame`
  resets the surface (immediate-mode: last frame's items do not leak).
- **`axiom-tween`:** 100% covered, branchless. Golden `ease(curve, t)` values at
  `t ∈ {0, 0.5, 1}` for all 7 curves (endpoints exact: `ease(_, 0) = 0`,
  `ease(_, 1) = 1`); `advance` chunk-invariance (one big step ≡ many small steps
  to the same total); `completed` fires once at/after `duration`; `cancel` removes
  a tween and stops further samples; `value` returns `None` for unknown/cancelled.
- **TS projection:** tsgo + Oxlint (branch ban) + 100% TS coverage. A headless
  test drives `tween` over a fixed elapsed sequence and asserts the `onUpdate`
  value series and a single `onComplete`; a `Ui` test asserts `button` returns
  `true` only on the activating frame. `solveLayout` is covered by SPEC-03/layout.
- **No replay/state-hash obligation** (presentation), but a presentation-leak test
  asserts no tween/Ui output is reachable from a `sim` accessor.

## 8. Dependencies & order

- **Depends on SPEC-04 (2D surface)** for the shared style records (`FillStroke`,
  `Common`, `TextOpts`, `SpriteOpts`, `TextureId`) the `Ui` variants reuse, and
  for the screen-space renderer that paints `UiDrawList`.
- **Depends on SPEC-00** for the loop: `onRender(frame, alpha)` is where the
  author calls `Ui` and where the runtime app calls `UiSurface::begin_frame` and
  `TweenApi::advance`.
- **`axiom-layout` already landed** — `solveLayout` is a free projection of
  `solve`, no new native work beyond the app's `LayoutNode ↔ LayoutTree`
  translation.
- **Composes with SPEC-07 (`StateMachine`)**: the author holds the menu/pause/
  result screen state in a state machine and draws the matching screen from these
  primitives — the engine ships no screen/menu framework.
- Lands in contract build-order slot 9 (§18), after audio (SPEC-08), before the
  physics extensions (SPEC-10).

## 9. Open questions

- **Immediate-mode inside a retained layer.** `axiom-interface` is retained today
  (panels persist; clicks route by id). The Ui surface is immediate-mode (rebuilt
  each frame; the only cross-call state is the per-frame `PointerSnapshot`
  installed by `begin_frame`). Housing both modes in one layer is justified — both
  are "neutral interface draw descriptions over kernel identity" — but if the two
  models prove to share nothing, the screen-space surface could fork into its own
  `axiom-overlay` module. Default: keep it in interface until a second
  divergence forces the split.
- **Tween closures vs. the branchless spine.** Native cannot hold or invoke JS
  `onUpdate`/`onComplete` closures branchlessly, so they live app-side keyed by
  `TweenId` and the native module only returns `TweenSample`s. This fits, but
  fixes the ordering: on the completing frame the app fires `onUpdate(to)` *then*
  `onComplete()` — the final value is delivered before completion. Confirm that is
  the contract's intent.
- **Toasts/floating popups.** The contract names no toast primitive; a toast is
  just an author-composed `Ui.rect` + `Ui.text` whose alpha/position is driven by
  a `tween` and whose lifetime is an author timer (SPEC-07). Confirm no dedicated
  toast API is wanted (keeping "primitives, not a widget framework").
- **Pointer snapshot source.** The Ui pointer state is presentation input, sampled
  from the same device stream SPEC-05 samples for sim — but on a *different* clock
  (per render frame, not per tick). Confirm the runtime app owns this split so no
  Ui pointer value can accidentally reach a fixed update.
- **Flip-book.** Frame-stepped sprite animation is SPEC-04's concern (the 2D
  surface), not this spec; a HUD that flip-books an icon composes SPEC-04's
  flip-book with `Ui.sprite`. Noted here only to fix the boundary.
