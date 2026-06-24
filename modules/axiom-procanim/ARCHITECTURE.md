# axiom-procanim — architecture

`axiom-procanim` is an **engine module** (isolated; `allowed_modules = []`). It is
the *animation* half of proc-driven rendering: where the procedural-generation
substrate computes *what exists* in a world, `procanim` computes *how it moves*.

## Placement

`Module: procanim` (engine module). Depends on layers `kernel`, `space`,
`entropy` — no other module. It produces a neutral, integer value
(`AnimatedTransform`); composing it with a scene and a GPU backend is an **app's**
job (Phase 2 of proc-driven rendering wires it into `axiom-doom-browser`).

## The contract

```text
(seed, address, tick)  ──ProcAnimApi::animate──▶  AnimatedTransform
                                                    { offset, yaw, scale }
```

- **`seed`** — the world seed (all animation is reproducible from it).
- **`address`** — the entity's [`axiom_space::Address`]. The animation is *keyed*
  by it: an `axiom_entropy` stream over `(seed, address, version)` draws the
  entity's parameters (phase, bob amplitude, spin rate, scale pulse, period), so
  each entity moves in its own way and two entities never share motion by accident.
- **`tick`** — the frame counter. `animate` is a pure function of it: identical
  `(seed, address, tick)` always yields an identical transform.

`AnimatedTransform` is fixed-point integers — a position **offset** in milliunits,
a **yaw** in milliradians, a **scale** in per-mille. No naked floats cross the
boundary, so the value is byte-identical on every platform (the determinism a
proc-driven renderer and lockstep multiplayer both require). An app converts to
the engine's f32 `Transform` at the GPU edge, where floats are allowed.

## Why integer + branchless

A transcendental `sin` is neither branchless nor reliably bit-identical across
platforms, so oscillation reads a 16-entry fixed-point **sine table** (one cycle
per `period` ticks) and the yaw is a wrapped integer **ramp** — both pure
arithmetic + a table index, no control flow. This keeps `procanim` inside the
Branchless Law and makes its output a deterministic regression target.

## What it deliberately does not do

- It does not know about meshes, materials, the GPU, or the browser — it emits a
  transform, nothing more.
- It does not own *what* to animate (which entities exist, their base positions) —
  that is the scene/level's job; `procanim` only answers "given this entity's
  identity and the tick, what is its local animated transform?"
- It does not compute pixels. Rasterization stays the GPU's job; `procanim`
  feeds the render *input*.
