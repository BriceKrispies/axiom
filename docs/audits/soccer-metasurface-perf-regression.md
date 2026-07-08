# Soccer-Penalty Performance Regression — Root Cause Analysis

**Date:** 2026-07-05 · **Introduced by:** `30725a2` · **Live fix:** `37fa520`

## Summary
The live soccer-penalty game fell to ~7 FPS with the kicker and goalie invisible.
Cause: a per-frame marching-cubes mesh bake added when the athletes were skinned
as continuous "MetaSurface" bodies. Offscreen screenshots hid it because they
render a single frame.

## Symptoms
- Frame rate ~7 FPS (from 60).
- Kicker + goalie not drawn at all.
- Static screenshots (`axiom-shot`, the convergence champion) looked correct.

## Root cause
`30725a2` skinned each athlete as a continuous body via `MeshOp::MetaSurface`
(a metaball field polygonised by marching cubes). The soccer renderer
**re-authors the whole scene every frame** (`web.rs` frame closure →
`PenaltyMeshedScene::author`), and the new `author_bodies` baked **one
marching-cubes surface per athlete kit-material group inside that per-frame
path**:

- **13 bakes/frame ≈ 136 ms/frame → ~7 FPS** (native release; wasm worse).
  A single keeper-jersey group at grid res 46 alone costs ~30 ms.
- Each bake registered a **brand-new mesh** via `add_mesh_data`, which only
  appends — **+13 meshes/frame, unbounded** — and the per-frame GPU upload
  re-clones the whole growing set (cost grows with playtime).
- The live `run_web_multi` loop **uploads meshes once at bind and never
  re-uploads**, so the per-frame body-mesh ids never reached the GPU → the
  athletes drew nothing.

Two failures, one source: **expensive geometry (re)generated in the frame loop**,
plus **mesh registration the live upload path was never designed to receive**.

## Why it wasn't caught
- The offscreen path renders **one frame** — one bake is cheap, and the
  single bind-time upload includes it, so screenshots were perfect.
- No test exercised the **live re-author loop**; unit tests only checked the
  one-shot draw count, not per-frame mesh-store growth.

## The lesson
Marching cubes is a **one-shot/static** technique. Posed, animated figures can't
be re-polygonised at 60 FPS. Geometry that changes per frame needs a **bounded,
reuse-based** path — pre-baked parts posed by transform — never per-frame
(re)baking + registration. The offscreen-looks-fine / live-broken split is the
tell: always exercise the *live* loop, not just a single frame.

## Fix
The live loop now draws athletes as **pre-baked articulated parts posed by
transform** (the pre-`30725a2` path): no per-frame baking (full frame rate), no
mesh-store growth, and every mesh id is in the bind-time upload (figures render).

> **Status note:** `37fa520` gated the MetaSurface bodies behind a `smooth_bodies`
> flag (live = off, offscreen champion = on) — a *runtime gate*, not a removal;
> the expensive path still exists for the one-shot render. If the smooth bodies
> are not wanted in the shipped game, the honest follow-up is to **remove the
> branch entirely** (box-man everywhere) rather than keep a conditional that is
> only ever true offscreen.

## Prevention
Added a regression guard (`live_loop_does_not_rebake_meshes_per_frame`): the live
author must register **zero new meshes per frame**. Any future re-introduction of
per-frame baking fails it.
