# Arena Forge — Mobile UI

Arena Forge is designed **mobile-first, landscape, from an 844×390 viewport**,
and scales up to larger mobile (932×430) and desktop (1440×900). Everything is
drawn on a **single gameplay canvas** — there is no DOM dashboard around it. The
whole interface is composed from Canvas2D primitives each frame (immediate mode).

## Files

| File | Responsibility |
|---|---|
| `src/ui/layout.ts` | The responsive layout solver. `computeLayout(w, h, combat, shopSize, handCount)` returns hit-testable rects for the HUD, seven warband slots, enemy row, hand, shop cards, action buttons, and the sell zone. `inspectRects(w, h)` returns the inspect overlay + action + close rects. |
| `src/ui/draw.ts` | Canvas2D primitives: rounded panels, text, buttons, rivets, the `Rect` type, and `inRect` hit-testing. |
| `src/ui/theme.ts` | The "arcane industrial" palette and the per-arena-stage treatment (floor, glow, machinery, particle scale). |
| `src/ui/render.ts` | `renderFrame(input)` — draws the whole frame from `(view, layout, ui, combat frame)`. Pure presentation: it never decides state. |
| `src/ui/interaction.ts` | The pointer controller — turns taps/drags/holds into authoritative commands. |
| `src/game.ts` | Wires host + interaction + audio + renderer and paces combat playback. |
| `src/harness.ts` | Browser boot: canvas sizing (DPR + resize), pointer wiring, `startLoop`. |
| `index.html` | The single-canvas page with the mobile viewport + no-scroll/no-zoom CSS. |

## Mobile-first rules honored

- **Landscape 844×390 baseline**, usable up through desktop — the layout is a
  pure function of the live canvas size, so it reflows at any resolution.
- **Touch/pointer is the primary model.** Down on the canvas, move/up on the
  window, so a drag can leave the canvas and still resolve. Mouse uses the exact
  same path (`onPointerDown/Move/Up`).
- **No hover, right-click, keyboard, tiny text, or precision dragging required.**
- **Touch targets ≥ 44 CSS px** in their smallest dimension: `layout.ts` clamps
  button heights to `>= 44` and keeps warband slots / shop cards well above it.
- **Safe-area insets** via `env(safe-area-inset-*)` on the canvas; the page is
  `position: fixed` with `overflow: hidden`, `overscroll-behavior: none`,
  `touch-action: none`, and `user-select: none` — so no scrolling, text
  selection, accidental zoom, or overscroll during play.
- **Everything readable without a secondary panel**: shop, hand, seven-slot
  warband, timer, gold, health, rank, and current opponent are all on screen at
  once (see any `browser/screenshots/*-01-*.png`).

## Gestures (all implemented in `interaction.ts`)

| Gesture | Result |
|---|---|
| Tap a shop card | Inspect (opens the rules overlay with a **BUY** action). |
| Tap BUY in the overlay | Buy the inspected card (to the first empty warband slot, else hand). |
| Drag a shop card → warband slot | Buy-and-place in one gesture. |
| Drag a shop card → elsewhere | Buy to hand. |
| Tap / drag a hand card → warband slot | Play it. |
| Drag a unit between warband slots | Reorder (swap). |
| Drag a unit → SELL zone (top-right) | Sell. |
| Tap REROLL / FREEZE / FORGE UP | Reroll / toggle freeze / upgrade forge rank. |
| Tap-hold a card or unit | Full rules view. |
| Tap outside the overlay | Close it. |

A cancelled or invalid drag (dropped on an occupied slot, off a target, or
without the gold) **submits nothing** — the transactional command layer means no
gold or card is ever lost to a fumbled gesture.

## The presentation boundary

The UI is strictly downstream of the simulation. It reads the authoritative
`MatchState` view and the `SimEvent` stream and paints them; it never mutates
state (every action is a command through `MatchApi`). Combat is the clearest
case: the simulation computes the whole combat deterministically at
`combat_prepare`, and `src/presentation/combat-playback.ts` reconstructs each
animation frame by **replaying the event stream** from the two warband snapshots
up to a time cursor. Fast-forwarding or dropping frames changes nothing about the
result. Audio (`src/audio/cues.ts`) is likewise event-driven and throttled, and
muting it cannot affect the sim.

## Visual progression

The arena has four data-derived stages — `workshop → kindled → tempered →
masterwork` — computed by the simulation from forge rank, forged-unit count, and
warband power (`src/sim/stage.ts`), and mapped to increasing floor warmth, forge
glow, and machinery/particle richness in `theme.ts`. Card frames carry their
group's accent; forged units get a brass border, a ✦ mark, and a stronger
silhouette. A **reduced-effects** quality profile lowers effect intensity and is
presentation-only — it never changes a simulation result.

Canvas2D is the required baseline: Arena Forge is fully playable with no WebGL or
WebGPU. An accelerated backend could later enhance the presentation through
Axiom's existing capabilities, but is never required to play.
