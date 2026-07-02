# Pass 2 — Deterministic Depth Ordering & Net Layering

Pass 1 built a static diorama with a coarse 10-bucket painter order. Pass 2
replaces that with an explicit, testable **render-ordering model** so the scene
reads as a real 3D diorama — most visibly, so the goal net has depth: rear net
behind the goalie/ball/kicker, front net in front of them.

Nothing about gameplay changes. This is still a static, fixed-camera visual
pass.

## What Pass 2 adds

- `PenaltyDrawLayer` — 14 explicit, fixed, back-to-front draw buckets.
- `PenaltySortKey` — a total sort key of `(layer, coarse depth bucket, stable
  object ordinal)`.
- `PenaltyRenderContent` / `PenaltyRenderItem` — a render item is either a world
  primitive or a HUD element, carrying its sort key and a greppable label.
- `PenaltyRenderPlan` — the world objects **and** the HUD, collected into one
  explicit `Vec` and sorted by the total key. It also exposes a deterministic
  `debug_lines()` view of the final order (used by tests and the
  `print_stage1` example; never printed by production code).
- Net split into two roles/layers (`RearNet` before actors, `FrontNet` after).

The object model (`penalty_scene`) no longer stores a draw layer. Each object
carries only its **semantic role** and geometry; the render plan is the single
place that maps role → layer and assigns depth. "What a thing is" and "when it
draws" are kept separate on purpose.

## The draw layers (back to front)

```
0  Background          (reserved, empty in Pass 2 — sky / far backdrop)
1  Crowd               fake crowd cards
2  StadiumWall         stadium wall + ad boards (incl. "AXIOM")
3  RearField           green pitch plane + grass bands
4  FieldLines          painted lines + penalty spot
5  RearNet             rear net panel  ── behind the actors
6  GoalFrame           posts + crossbar
7  ActorShadow         blob shadows on the ground
8  Goalie              goalie puppet
9  Ball                ball on the spot
10 Kicker              foreground kicker puppet
11 FrontNet            front net panel ── in front of the actors
12 ForegroundEffects   (reserved, empty in Pass 2 — impacts / particles)
13 Hud                 arcade HUD, always last
```

`Background` and `ForegroundEffects` are intentionally reserved and empty: they
exist so a later stage can slot a sky backdrop or impact particles into the
order without renumbering anything.

## Why render ordering is explicit

A painter-style renderer (and the Canvas2D fallback in particular) has no
hardware depth buffer, so *the order it receives items in is the depth*. Leaving
that order implicit — dependent on hash-map iteration, allocation order, or
pointer identity — would make the picture non-deterministic and impossible to
snapshot-test. So the order is a first-class, explicit artifact: a fixed layer
enum, a coarse depth bucket, and a stable per-object ordinal, sorted into one
vector. Two builds are byte-for-byte identical.

The sort key is compared field-by-field:

1. **layer** — the dominant, authored back-to-front bucket;
2. **coarse depth bucket** — a quantized world depth so that, *within* a layer,
   farther primitives draw first. The bucket is taken from each primitive's
   *farthest* edge (`center.z − size.z/2`). Because the pitch's grass bands lie
   inside the base plane's extent, the big plane's farthest edge is at least as
   far as any band's, so the plane always sorts behind the bands — no z-fighting
   guesswork. Buckets are coarse (a few meters each) so near-coplanar items fall
   together and the ordinal — not float noise — orders them;
3. **stable object ordinal** — the object's build-order id, the final total
   tie-breaker for equal layer + depth.

No part of this reads the wall clock, uses randomness, or depends on unordered
iteration.

## Why the net is split into rear and front

A real goal net wraps from the crossbar down behind the goal and forms a volume
the ball and goalie sit inside. Simulating that (or even modeling it as one mesh
with correct per-pixel depth) is out of scope for a static retro 32-bit-style pass. So we
fake it:

- the **rear net** panel is a grid of lines at the back of the goal, placed in
  layer `RearNet` (5) — *before* the goalie/ball/kicker;
- the **front net** panel is a grid of lines at the goal mouth, placed in layer
  `FrontNet` (11) — *after* the goalie/ball/kicker.

The result: the actors read as standing *inside* the goal, with net visibly
behind them and net lines visibly crossing in front of them. This is a
deliberate, documented fake-depth trick, exactly the kind retro 32-bit-era games used. The
net is still just line geometry — no motion, no collision, no simulation.

## How a Canvas2D-style renderer should use the plan

Iterate `PenaltyRenderPlan::items` in order and draw each one; the order already
encodes depth, so **do not** sort again and **do not** need a depth buffer.
Later items paint over earlier ones. That is the whole contract.

## How a hardware-depth (WebGPU/WebGL) renderer can consume the same plan

The same ordered list works unchanged: submit items in plan order. A
depth-buffered backend can additionally use each item's world position/size (on
`PenaltyRenderContent::World`) to set true per-fragment depth, treating the layer
order as the *coarse* bucket and the depth buffer as the fine resolver. Either
way, both backends consume one high-level ordered render plan — the point of
keeping ordering backend-neutral.

## Still not implemented (later stages)

Pass 2 is ordering only. It deliberately does **not** add:

- shooting or shot input;
- a ball trajectory / arc;
- goalie animation or dive poses;
- collision volumes of any kind;
- save / goal / miss / post resolution;
- net wobble or impact effects;
- scoring, rounds, or replay logic.

The net does not move and has no collision. The camera is still fixed. See
`STAGE_1.md` for the full future-stage roadmap.
