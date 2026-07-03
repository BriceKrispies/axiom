# Mechanism vs. Meaning

Axiom's spine is organised around one ownership rule:

> **The engine owns mechanisms. Games own meaning.**

A *mechanism* is a reusable, game-agnostic capability: a transform, a scene
snapshot, a resource handle, a render command list, a skeleton, a pose, an
animation clip, a rigid body, a collider, an input action, an audio command,
a deterministic replay boundary. Mechanisms are the same whether you are
building a soccer game or a forest walk — so they live in the engine, once.

*Meaning* is what a specific game makes of those mechanisms: a *kicker*, a
*goalie*, *shot power*, a *forest*, *enemy behaviour*, a *level goal*, a
*score*. Meaning is content and rules — it changes from game to game — so it
lives in an **app** or a **game cartridge**, never in the engine spine.

Keeping the two apart is what stops the engine turning into soup. When a game
concept leaks *inward* (a `kicker` field on a core animation type, a `soccer`
rule baked into a physics collider), the mechanism stops being reusable and the
next game has to fork it. When a mechanism leaks *outward* (an app re-deriving
"scatter → seated tree instances" by hand because no module owns it), every app
reinvents the same wheel and they drift apart. Both are architectural debt.

## The four tiers

Axiom is built in four tiers, from most-stable to most-specific:

| Tier | Location | Owns | Depends on |
|------|----------|------|------------|
| **Kernel** | `crates/axiom-kernel` | Deterministic *truth*: time/ticks, stable ids, result/error types, dimensioned scalars, deterministic RNG sources, telemetry. Small, boring, substrate-only. | nothing |
| **Layers** | `crates/*` + `layer.toml` | The ordered engine *spine*: `runtime`, `math`, `host`, `frame`, `ecs`, … Each builds a broad capability on the layers below it, forming a DAG rooted at the kernel. | lower layers only |
| **Modules** | `modules/*` + `module.toml` | Reusable *mechanisms*. **Engine modules** are isolated (`scene`, `resources`, `render`, `animation`, `physics`, `input`, `audio`); **feature modules** compose several modules into a pipeline. | allowed layers (+ allowed modules, for feature modules) |
| **Apps / Games** | `apps/*`, `games/*` | *Meaning*: content, rules, composition. Apps are leaves; game cartridges are content a host app can load. This is the only tier that translates between two module contracts. | layers + modules (+ games, for host apps) |

- **Kernel = deterministic truth.** If it is not always true and substrate-level,
  it does not belong here. No rendering, no physics, no gameplay.
- **Layers = ordered engine spine.** A layer must genuinely use the layers it
  declares and provide a real capability on top of them.
- **Modules = reusable mechanisms.** One facade each, isolated by default. Two
  engine modules never name each other's types; a lower **layer** carries any
  primitive they both need.
- **Apps & games = meaning.** All game vocabulary, content, and cross-module
  glue lives here.

## Why the kernel does not grow to fix this

When an app is forced to reinvent a mechanism, the fix is **never** to widen the
kernel — the kernel must stay small, boring, and substrate-only. The fix is to
put the mechanism in the correct **module** (or, if many modules need a shared
primitive, in the correct **layer**). Growing the kernel to hold a mechanism is
the same shortcut as leaking meaning inward, pointed the other way.

## Enforcement

This boundary is mechanically enforced, not just documented:

- `cargo run -p xtask -- check-architecture` classifies every package
  (Layer / Module / App / Game / Tool / Support) and enforces the Layer Law and
  Module Law: layers import only lower layers, engine modules import no modules,
  modules expose exactly one facade, apps are leaves, tools are off the runtime
  graph, and no junk-drawer modules exist.
- `hygiene.rs` bans browser/platform APIs outside the sanctioned host/platform-
  facing crates, plus console/placeholder macros in the spine.
- The `engine_no_branching`, `engine_genuine_dependency`, and
  `engine_no_unitless_float_public_api` dylints, and the 100% coverage gate,
  hold the spine to its invariants.

Each core mechanism module additionally carries a `tests/architecture.rs` scan
that fails if game/domain vocabulary (`soccer`, `forest`, `kicker`, `goalie`, …)
leaks into it — the mechanism-vs-meaning line, checked at `cargo test` time.
