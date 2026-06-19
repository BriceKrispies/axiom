# Growth-on-Axiom — port gap analysis

**Status:** Analysis only (no Axiom code changed). Written 2026-06-19.
**Author context:** Produced by auditing a separate, fully-specified target product (the *Growth* repo — a Godot/C++ procedural-planet survival game) against the current Axiom engine, to answer one question: **what is missing in Axiom to build that product here instead.**

> Axiom today is a deterministic, WASM-first 3D engine substrate with a strict layered spine, a working render path (rotating-cube / stress-cubes / a playable RetroFps-browser FPS), an ECS, scene graph, and deterministic signed-lockstep netcode. It has **no** world generation, terrain, chunk streaming, gameplay systems, or planetary-scale anything. None of that is a defect — those are app/feature concerns by design. This folder enumerates them so they can be built in the right place, in the right order, without violating Axiom's laws.

## The target in one paragraph

A deterministic **procedural-planet engine + survival game**: seed + planet preset → a whole-sphere "overworld" atlas (tectonics, elevation, erosion, moisture, rivers, climate) → a streamed, metre-scale, **player-editable** "game world" of chunks around the player → gameplay layered on top (walk, dig/terraform, survive, then spirit/possession, ecology, art pass). The substrate must be deterministic and moddable; the game is intentionally downstream of it. See [`target-product.md`](target-product.md).

## How to read this folder

| Doc | What it answers |
|-----|-----------------|
| [`target-product.md`](target-product.md) | What we are ultimately building (condensed from the Growth requirements audit), and what "done" means per subsystem. |
| [`axiom-baseline.md`](axiom-baseline.md) | What Axiom genuinely provides **today**, per crate/module, with real symbol names. The honest starting line. |
| [`gap-analysis.md`](gap-analysis.md) | The core deliverable: every missing subsystem, where it must live under the Layer/Module laws, and the hard architectural risks (branchless spine, 100% coverage, no threads, dynamic-mesh upload, wasm). |
| [`roadmap.md`](roadmap.md) | A phased build order anchored on a first **vertical slice**, mapped to Growth's milestones, with the "build-in-an-app-then-graduate" strategy Axiom's laws imply. |

## The one-line verdict

**Axiom is a sound foundation and the determinism alignment is a genuine asset — but ~80% of the target product does not exist in any form yet, and Axiom's spine laws (branchless + 100% coverage + no `thread::spawn`) make it *more* expensive, not less, to put that code into the engine spine.** The winning strategy is to build the world/gameplay as an **app** first (apps are exempt from the branchless and coverage gates), prove the primitives, and *graduate* only the stable, reusable ones down into modules and layers. Details in [`gap-analysis.md`](gap-analysis.md) and [`roadmap.md`](roadmap.md).
