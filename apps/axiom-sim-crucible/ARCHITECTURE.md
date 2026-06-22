# axiom-sim-crucible — Architecture

## What the sim crucible is

A tiny, deterministic, **headless** proof app. It composes the Axiom ECS layer
and the `axiom-sim-core` module into one explainable, replayable causal chain and
nothing more. It is not a game, a colony sim, or a tavern simulation — it is a
*substrate test that happens to be runnable*.

## What it proves

That the generic substrate can express a Dwarf-Fortress-like emergent
interaction with **no special-case domain code**:

1. A creature (`cat`) is instantiated with a body (core + `paw` extremity +
   `mouth`).
2. A transferable, intoxicant substance (`beer`) sits as a residue in a
   non-body location (`tavern-cell`).
3. A **touch** interaction + a generic **transfer rule** move beer onto the paw
   surface.
4. A scheduled generic **process** (grooming-like) wakes; it *produces effects*
   (it adds a "groomed" fact through the effect boundary — it never mutates
   stores directly).
5. Its consequence — an **ingestion-entry** interaction + a transfer rule — moves
   beer from the paw surface to the mouth surface.
6. A generic **material effect rule**, matched by the `intoxicant` tag + the
   ingestion route (never by the names `cat`/`beer`), updates an intoxication
   **fact** on the creature.
7. The **causal journal** explains every step; replaying the same inputs yields
   the same final state and causal-event order.

Quantity is conserved: source 10 → 4 onto the paw → 3 from paw to mouth, leaving
source 6, paw 1, mouth 3.

## Why it is an app, not a layer or module

It is a **composition root** (a leaf): nothing depends on it. It wires two
substrate crates together for one scenario. That is exactly what apps are for —
and apps are exempt from the branchless and 100%-coverage spine gates, so this
scenario code can be idiomatic. A layer or module, by contrast, must be reusable
and domain-free; this code is neither (it knows about cats and beer).

## How it composes ECS and sim-core

- `axiom-ecs` provides the `EntityRegistry` and the creature's generational
  `EntityHandle`.
- `axiom-sim-core`'s `SimCoreApi` provides everything else: tissue/body-plan/body
  construction, substance definitions, residues, interaction routes, transfer
  rules, material effect rules, the process scheduler, effect batches, and the
  causal journal — plus its published **identity vocabulary** (`ResidueId`,
  `BodySurfaceId`, `ProcessId`, `TransferRuleId`, `FactId`, …).

`scenario::build` runs once at tick 0 and returns a `ScenarioRefs` of durable
typed handles — the creature, its body, the paw/mouth surfaces, the source
residue, the two transfer rules, the intoxication fact, the substance, and the
grooming process. The driver captures these once and reuses them for the whole
run; it never re-queries the world for "the paw" or "the grooming process".

### The action model (why the tick loop is boring)

A scenario is data: `action::schedule(&refs)` builds a deterministic, ordered
`Vec<ScenarioAction>`, each tagged with the tick it runs on and a data-shaped
`ScenarioActionKind`:

- `SurfaceTransferAction` — record a surface interaction on a route, then apply a
  transfer rule (drives both the contact source→paw and the grooming paw→mouth).
- `ProcessAction` — schedule a registered process to wake at a tick.
- `EffectApplicationAction` — apply the generic material-effect rules to the
  pending interaction.

The tick loop (`Crucible::tick`) is then uniform and carries no scenario
knowledge: step the scheduler (running due process handlers), note any grooming
wake, apply the effect boundary, and run the actions due this tick through one
generic executor (`Crucible::execute`). There is no `if tick == CONTACT` /
`if tick == GROOM` special-casing — the architecture test
`tick_loop_has_no_hardcoded_consequence_branches` enforces this.

The grooming process is *registered* in `build` and *scheduled* by a tick-0
`ProcessAction`; its consequence (the ingestion transfer + effect) is expressed
as further actions caused by that process — never inlined into the driver. Its
handler adds the "groomed" fact through the effect boundary, proving the
process/effect substrate. (The effect system has no residue-transfer effect kind,
so the residue movement itself is an app-orchestrated action *caused by* the
process, not a process effect.)

### Why sim-core publishes its id vocabulary

A substrate whose purpose is to let a composition root build entities and
re-address them over many ticks must let that root *name* the entities. sim-core
therefore exports its pure value-type id newtypes alongside the one `SimCoreApi`
facade — the same reason the `ecs` layer exports `EntityHandle`. The Module Law's
single-facade rule was refined to permit exactly this (one behavioral facade plus
its identity vocabulary, a `pub use ids::{…}` re-export) and nothing more; all
behavioral/contract types still live behind the facade. Only one runtime lookup
remains (`ResidueSource::OnSurface`): the paw residue is *created at run time* by
the contact transfer, so it has no setup-time handle and is addressed by its
durable surface instead.

## Why domain names are allowed here but forbidden in the substrate

`cat`, `beer`, `paw`, `mouth`, `tavern-cell` are **scenario data**, not engine
concepts. An app is the only place a concrete scenario lives. The reusable
substrate must stay domain-agnostic so every future scenario can compose it; if
`beer` or a `cat`-special-case appeared in `axiom-sim-core`, the substrate would
no longer be generic. An architecture test (`architecture.rs`) enforces that the
scenario tokens do not leak into `axiom-sim-core`/`axiom-ecs`, and that the final
rule matches the generic `intoxicant` tag + route — never the names.

## The causal chain demonstrated

```
command ─▶ contact-interaction (paw)        ─▶ contact-transfer  (source→paw)
process ─▶ process-scheduled/woke/started
        ─▶ process-produced-effects/applied/completed   (adds "groomed" fact)
        ─▶ ingestion-interaction (mouth)    ─▶ groom-transfer   (paw→mouth)
        ─▶ intoxication-effect              (updates the creature fact)
```

The journal answers: *why* the paw has residue (the contact transfer, caused by
the command); *why* the mouth received residue and *why* ingestion happened (the
grooming process); *why* the creature fact changed (the effect rule on the
ingestion interaction). Every event carries a parent cause, so the chain is
walkable.

## Replay / determinism expectations

All state is `BTreeMap`-ordered, ticks are integer logical ticks, and there is no
randomness or wall-clock time. `replay::verify()` runs the scenario twice from the
same initial state and asserts an identical structural digest, identical
causal-event order, and identical fact/residue state. sim-core's full
byte-snapshot/replay seam is deferred (see sim-core's deferred-features note), so
replay is proven by deterministic re-run comparison of actual state — not by a
byte snapshot.

## Explicit non-goals

No game, colony sim, full tavern, graphics, WebGPU, browser UI, scene graph,
physics, pathfinding, AI planning, jobs, needs, mood, social memory, combat,
wound processing, toxicology model, death model, fluids, temperature, world-gen,
or editor. This is a tiny deterministic proof app; later expansions (spatial
cells, needs, jobs, history, inspector UI, rendering) build on the proven
substrate.
