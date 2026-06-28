# `axiom-agent` — Architecture

`axiom-agent` is a **deterministic embodied-agent substrate**. It owns one neutral
loop and nothing else:

```text
observe -> decide -> emit player-equivalent intents -> record decision report
```

This document explains *what* the module is, *where* it sits, *why* its public
surface looks the way it does, and — just as importantly — the long list of
things it deliberately is **not**.

## It is an engine module, not a layer and not the kernel

`axiom-agent` is an **engine module** (`modules/axiom-agent`, `module.toml` with
`kind = "engine-module"`), not a layer in the ordered spine and not part of the
kernel. That classification is deliberate:

- **Not the kernel.** The kernel is the small, boring substrate every layer must
  always trust: time, identity, errors, math primitives. An agent loop is a
  *capability*, not a universal truth — it is exciting, domain-shaped, and
  optional. Exciting code does not belong in the kernel.
- **Not a layer.** A layer is a broad, ordered rung of the spine that everything
  above it builds on. `axiom-agent` is the opposite: an **isolated** capability
  that depends on a curated set of *layers* and on **no other module**
  (`allowed_modules = []`). Nothing in the spine builds on it; only an app (or a
  future feature module) composes it.

It depends on exactly two completed layers, and genuinely uses each:

| Layer           | What `axiom-agent` consumes from it                                  |
|-----------------|----------------------------------------------------------------------|
| `axiom-kernel`  | the `Tick` identity (stamps observations, reports, memory entries) and the `KernelResult` / `KernelError` model (bounded-overflow failures in the observation builder and action queue) |
| `axiom-runtime` | the deterministic `RuntimeStep` that drives one decision — `AgentRuntime::step` reads its `tick()` to stamp the decision, exactly as `axiom-physics` reads it to drive a physics step |

It depends on **nothing else** — no `axiom-math` (observation data is
integer/fixed-point, so no vector layer and no naked floats), no scene, render,
resources, physics, input, ECS, app, tool, browser API, GPU API, wall-clock, or
randomness. Those forbidden tokens are scanned for by `tests/architecture.rs` and
by the workspace `xtask check-architecture` gate.

## What `axiom-agent` owns

The **neutral contracts** of an embodied agent, and the deterministic machinery
that turns one into the next:

- `AgentId` / `AgentProfile` — stable identity and a fixed block of human-like
  control limits (integer/fixed-point parameters only; no noisy behavior yet).
  A profile's per-tick action budget can be overridden via the facade to
  throttle or, at `0`, freeze a *deciding* agent (the scripted brain); a replay
  brain reproduces its recording verbatim and is unaffected.
- `AgentMemory` — a bounded, insertion-ordered store of machine-readable
  `(tick, key_code, value_code)` records; oldest-dropped at capacity.
- `Observation` / `ObservationBuilder` / `ObservationChannel` / `ObservationFact`
  — a bounded, game-neutral perception packet and a bounds-enforcing builder.
- `ActionIntent` / `ActionQueue` — player-equivalent intents (low-level controls
  + high-level data-only intents) and a bounded FIFO of them.
- `ScriptedBrain` / `ReplayBrain` — two tiny deterministic decision-makers behind
  the internal `AgentBrain` contract. **Scripted** is a fixed ordered rule table
  (not a scripting language): the first rule whose fact-kind is present wins and
  emits that rule's intent + reason; no match emits `Noop` (`no_matching_rule`);
  a zero action budget emits nothing (`action_budget_zero`). **Replay** replays a
  fixed recorded intent sequence one-per-step, ignoring the observation; an empty
  recording (`replay_empty`) and a past-the-end cursor (`replay_complete`) are
  distinct exhaustion outcomes.
- `DecisionReport` — the replayable, all-numeric record of one decision.
- `AgentRuntime` — the stateless orchestrator that steps one agent once.

## What `axiom-agent` explicitly does NOT own

It is **not an AI framework.** There is no neural network, machine learning, LLM,
pathfinding, navmesh, behavior tree, utility-AI, or planner — and
`tests/architecture.rs::no_prohibited_ai_concepts` makes those words fail the
build. It is **not enemy AI** and carries no player or gameplay rules. It does
not render, does not touch a scene/physics/asset/input device, and does not run a
game loop. It performs no I/O, reads no clock, and uses no randomness.

## The observe → decide → intent → report loop

One step is a pure function of its inputs:

1. **Observe.** An app builds an `Observation` (through `ObservationBuilder`,
   under explicit channel/fact/legal-action bounds) from whatever it knows.
2. **Decide.** A brain maps `(agent id, profile, observation, memory)` to a
   `BrainDecision` — the intents it wants to emit plus a reason code (for a
   scripted match, the reason carried by the rule that fired) — with no hidden
   state beyond its own (a replay brain advances a cursor).
3. **Emit.** The scripted brain clamps its emissions to the profile's action
   budget (a zero budget emits nothing, reported as `action_budget_zero`); the
   replay brain reproduces its recording verbatim. `AgentRuntime::step` queues
   the result as an `ActionQueue` of player-equivalent intents.
4. **Report.** It records one deterministic memory entry and produces a
   `DecisionReport` (all codes and counts), stamped with the `RuntimeStep` tick.

Same inputs always produce the same report and the same emitted actions, so a
session is recordable and replayable.

## Why apps translate game state into observations

Two modules can never share a Rust type they each name. `axiom-agent` therefore
publishes **neutral contracts** (codes, counts, fixed-point coordinates) and
leaves both ends of the translation to the composition tier (an app or a feature
module):

- An app reads its own scene/sim/render/game state and **fills an `Observation`**
  — choosing the channel, the fact kind codes, and the subject codes.
- An app takes an emitted `ActionIntent` and **lowers it into concrete input** —
  the actual key/button/axis/pointer event its engine understands.

This keeps `axiom-agent` a black box with a stable shape that any future app
(headless test, native, WASM) can reuse without the agent module ever learning a
game noun.

## Why machine vision is only an observation channel here

A real agent might one day perceive a rendered frame. Modelling that *inside*
this module would drag in rendering, GPU, and platform concerns — exactly the
inward leakage the architecture forbids. So machine vision is represented only as
the declared `ObservationChannel::ScreenSample` **label**. An app/tool may sample
a frame and present the result as `screen_sample` facts; the agent module never
implements vision and never depends on a renderer.

## Why there is no pathfinding, behavior tree, planner, ML, renderer, input, or scene dependency

Each of those is either a **separate capability** that belongs in its own module
(pathfinding, planning) or a **subsystem this module must not absorb** (scene,
render, input). Folding any of them in would turn an isolated substrate into a
god-module and break the dependency direction. The agent substrate's job is only
to define the neutral *contracts and loop*; richer decision strategies compose on
top, they do not live inside.

## How future work can build on this without breaking boundaries

- **Richer brains** (a planning brain, a goal-driven brain) are added behind the
  same internal `AgentBrain` contract — `AgentRuntime::step` is generic over it,
  so a new brain needs no change to the runtime or the facade's shape.
- **Real perception** is added by an *app/tool* that fills `screen_sample` (or
  new) facts; the contract already carries the channel.
- **A genuinely shared lower primitive** (if two future modules need it) belongs
  in a **layer**, never in a second module wired sideways into this one.
- **Composition** (wiring an agent's intents into a real game) is always the job
  of an app or feature module, never of `axiom-agent`.
