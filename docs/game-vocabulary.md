# The Axiom Game Vocabulary

> The set of engine-provided **verbs and nouns** an author (human or agent)
> composes a whole game from — *without writing engine Rust and without
> reaching into engine internals.*

This document is the north star for "to author a game on Axiom, you should not
have to write any Rust." It is deliberately **discovered, not invented**: the
vocabulary grows only under proven pressure, and every entry must survive the
Vocabulary Law below. The goal is a surface small enough that an agent can read
it cold and reason about completeness, and orthogonal enough that there is one
obvious way to express each thing.

This is a *direction*, built one proven slice at a time. The first slice
(Category 2 — spatial reasoning) is implemented; see **Status** at the end.

## The Vocabulary Law

A capability is admitted to the game vocabulary only if it is **all five** of:

1. **Game-neutral** — its *name* presupposes no genre. `raycast` is in;
   `health` is out (it presupposes combat). Gameplay nouns — health, ammo,
   score, factions, recipes, dialogue — are never primitives. They are state
   the *author* holds; the engine never learns them.
2. **Orthogonal** — no existing primitive already expresses it. Two ways to do
   one thing is both soup and author-hostile.
3. **Pressure-justified** — admitted only when **≥2 unrelated games** provably
   cannot be expressed without it. Primitives are discovered by trying to build
   real games and hitting a wall, never by speculation.
4. **Maximally general** — added in the most general form that clears the wall,
   not the specific thing that revealed it. (A need for "gravity" is almost
   always a need for "integrate a velocity with vector math," not a `gravity`
   primitive.)
5. **Introspectable** — present in the engine's reflected capability surface
   (`TypeSchema` / the introspection layer), or it does not exist. The spec an
   author reads is *generated from the engine*, so it cannot drift.

This is the same kind of mechanically-defensible invariant as the Layer Law and
the Branchless Law. It does not automate *taste* — the "maximally general vs.
convenient" judgement is permanent and lives in review — but it bounds the
shape of the surface so it cannot become a junk drawer.

## The categories are closed; the entries grow

Reduce real games and the engine-universal capabilities collapse to seven
categories. The categories are the spec's table of contents and **do not grow**;
entries within them are admitted under the Law. Most already exist in Axiom,
scattered — the work is recognition and consolidation behind one introspectable
surface, plus filling the gaps.

| # | Category | What the engine owns | Axiom today |
|---|----------|----------------------|-------------|
| 1 | **Spatialized / presented state** | entities, transforms, renderables, cameras, lights, clear color | `axiom-scene` (`SceneApi`) |
| 2 | **Spatial reasoning** | raycast, overlap, nearest, containment | **the gap** — being built here |
| 3 | **Time & lifecycle** | deterministic tick, spawn/despawn, lifecycle hooks | runtime / frame / scene `advance` |
| 4 | **Input** | mapped actions, axes, pointer | `axiom-input` |
| 5 | **Variation** | seeded RNG, procedural generation | `axiom-proc` / `axiom-entropy` |
| 6 | **Persistence** | snapshot / restore of opaque state | scene `snapshot_state`, `axiom-worldsave` |
| 7 | **Composition** | spawn-from-prototype, parenting, static content | `App` / `SceneCommands` / level docs |

## The DOOM reduction (the worked proof)

DOOM (`apps/axiom-doom-browser`) is the proving game. Walking every distinct
thing `DoomGame::step` does and forcing each into **script** (author-held state /
logic the engine must never see) or **primitive** (a genuine engine call, named
in its most general form):

| What DOOM does (`lib.rs`) | Verdict | If primitive: general form |
|---|---|---|
| `health`, `score`, `ammo`, cooldowns, enemy `alive` | **script state** | — (genre nouns) |
| look from input | **input** | `axis`, `action` |
| build yaw/pitch rotation | **script** | over `set_transform` + quat math |
| move + per-axis wall slide (`move_player`) | **script + 1 primitive** | `overlap(box, at)`; slide policy is script |
| hitscan: cone + range + nearest + LOS (`fire_shot`, `line_clear`) | **script + 1 primitive** | `raycast(origin, dir, max) -> hit node` |
| enemy chase + wall slide (`chase`) | **script** | steering math; reuses `overlap` |
| park dead enemy (`update_enemies`) | **script choice** | real form is `despawn` |
| contact damage proximity (`any_enemy_in_contact`) | **script + 1 primitive** | `overlap(box, player)`; the rule is script |
| death → respawn-all (`respawn`) | **script** | reset script vars + `set_transform` + `spawn` |
| build walls/floor/enemies/camera/light (`level_setup`) | **composition** | `spawn`, `set_renderable`, `add_camera`, `add_light` |
| `write_state` / `read_state` | **persistence** | `snapshot` / `restore` |

### What the reduction proves

- The whole game is **~a dozen engine calls** across five of the seven
  categories. Everything genre-specific is author-held state the engine never
  learns — the engine stays fully game-neutral.
- **Spatial reasoning is load-bearing and orthogonal.** Three separately
  hand-coded subsystems — wall collision, hitscan + line-of-sight, contact
  proximity — **all collapse into `raycast` + `overlap`.** A `raycast` that
  returns the hit *entity* subsumes DOOM's cone-and-nearest search *and* the
  wall-blocking *and* the LOS march into a single call.
- DOOM uses **zero** randomness, so by the pressure rule it justifies **no**
  variation primitives. Pressure-justification, honored.
- The one genuinely new requirement: **bounding volumes as first-class,
  queryable engine state.** Today walls are renderable cubes and the *app*
  re-derives a tile grid to collide against. That is the concrete first build
  target. (It is deliberately a *spatial-query* bounding volume — picking /
  overlap / line-of-sight, which the math layer is built to serve — **not** a
  physics collider; the scene owns no rigid bodies or collision response, and an
  architecture guard keeps it that way.)

## Status

**Category 2 — spatial reasoning: first slice landed.**

- `axiom-math`: `Ray::intersect_aabb` factored onto a shared `slab_range`; new
  `Ray::intersect_aabb_entry` returns the entry distance (for nearest-hit).
- `axiom-scene`: a `Bounds` component (axis-aligned bounding box of half-extents,
  sized by the node's world scale), serialized in the scene snapshot and
  reflected in `component_schemas`, plus two queries on `SceneApi`:
  - `raycast(origin, direction, max_distance) -> Option<SceneNodeId>` — the
    nearest bounded node the ray enters within range.
  - `overlap_box(center, half_extents) -> Vec<SceneNodeId>` — every bounded node
    whose world box intersects the query box.

Queries read **world** transforms, so a node must be propagated (`advance` or
`update_world_transforms`) before it is queryable — bounds without a computed
world transform are skipped.

`Bounds` is a **spatial-query** bounding volume (picking / overlap / LOS), the
"scene bounding volumes" the math layer exists to serve — *not* a physics
`Collider`. Physics (rigid bodies, collision response) is a separate future
concern; a scene architecture guard bans physics nouns from this module.

### Deliberate v1 scope (pressure-bounded, not a shortcut)

- **Bounds are axis-aligned.** The box is centered at the node's world
  translation and sized by its world scale; **world rotation is not modeled**.
  DOOM (and any grid/box world) needs exactly this. Oriented boxes are the
  sanctioned extension the moment a second game presents rotated bounds.
- **`raycast` returns the node, not a distance or impact point.** DOOM needs the
  node; a richer `RayHit` is added under pressure.
- **`overlap` takes a box, not a sphere.** Reuses existing math; a sphere shape
  is added under pressure.

### DOOM now runs through the queries (proof, landed)

`apps/axiom-doom-browser` no longer re-derives a tile grid at runtime. The hand-
rolled `map_wall_at` / `is_wall` / `line_clear` / `map_wall` are deleted; walls
and enemies carry `Bounds` (the grid is authoring-only now), and `DoomGame::step`
takes a `&dyn DoomSpace` (implemented by `RunningApp`) and asks the engine for
every spatial answer:

- **movement & enemy collision** → `overlap_box`, ignoring live-enemy hits (walls
  block; player-marked actors are walked through);
- **hitscan + line-of-sight + target selection** → one `raycast`: the nearest
  hit is the enemy it kills or the wall that shadows it;
- **contact damage** → `overlap_box`, keeping live-enemy hits.

Queries return **first-class `Entity` handles** (the engine-standard name for
`axiom-scene`'s `SceneNodeId`, re-exported through the umbrella prelude), exactly
as a real ECS engine hands back entities from `spawn`/`query`. The app classifies
a returned `Entity` against its own tracked set — *is it one of my live enemies?*
— so "geometry" is simply an `Entity` that isn't. No raw node `u64` and no
engine-internal `SpatialHit` enum ever cross the boundary. The engine grew the
matching authoring + query + lifecycle surface: a `Bounds` bundle component,
`RunningApp::raycast` / `overlap_box` → `Entity`, `spawn(Spawn) -> Entity`,
`despawn(Entity)`, and `player_entity(index) -> Entity` (all 100% covered,
branchless). Determinism survives unchanged — `Entity` handles are never
serialized; a fork re-binds them after `restore_sim`, and the **golden captures
match without re-baselining** (replay/fork stay byte-identical).

### Typed component access by `Entity` — `get` / `set` / `query`

On top of the handle surface, `RunningApp` now offers the engine-standard ECS
read/write triad, addressed by `Entity` and parameterized by component type:

```rust
let t = app.get::<Transform>(entity);          // Option<Transform>
app.set::<Transform>(entity, moved);           // bool (false if not a live node)
for (e, b) in app.query::<Bounds>() { … }       // Vec<(Entity, Bounds)>, ascending
```

This is the Bevy/Godot-shaped generic syntax, built the **Axiom-idiomatic way**:
a closed compile-time `Component` trait (one impl per engine value type — today
`Transform` and `Bounds`) maps each type to explicit `SceneApi` accessors. There
is deliberately **no runtime `TypeId` registry or `Any` downcast** — Axiom's ECS
is named-column / explicit-selector by design, and a reflective type map would be
exactly the "clever abstraction" the engine distrusts (and a branch-and-unsafe
hazard). The trait dispatch is static, branchless, and fully covered; the scene
grew only value getters/enumerations (`node_transforms`, `bounds_half_extents`,
`bounded_nodes`). The surface is additive — no determinism impact — and is the
foundation the transform-mutation stage and the roomed-puzzle port build on.
(`get`/`set` of `Transform` are *local*-transform semantics, mirroring Bevy's
`Transform`; world transforms refresh on the next tick.)

### Category 3 — time & lifecycle: runtime despawn landed

The remaining categories were assessed against the Vocabulary Law (admit a
primitive only under a real game wall). Most already sit at the host surface:

| Category | Host-surface status |
|---|---|
| 1 spatialized state | authored via `App::setup`/bundles; mutated at runtime by despawn (below) |
| 2 spatial reasoning | `RunningApp::raycast` / `overlap_box` (landed) |
| 3 time & lifecycle | deterministic `tick`; **runtime `despawn` (landed)** |
| 4 input | deltas via `tick_with_controls`; *mapped bindings* are the scripting tier |
| 5 variation | `axiom-proc`/`axiom-entropy` exist; **deferred** — no in-repo game uses RNG, so wiring it now would be speculative surface the Law forbids |
| 6 persistence | `snapshot_sim` / `restore_sim` (already there) |
| 7 composition | `App::setup` / `reauthor` / prototypes (already there) |

The one gap with a concrete DOOM workaround to retire was **runtime lifecycle**:
DOOM faked despawn by *parking dead enemies below the floor*. Now the engine owns
object lifetime — `World::despawn` (cleans every column) is surfaced as
`SceneApi::despawn_player(index)` and `RunningApp::despawn(index)` (also clearing
the player/controller marker maps). DOOM's kill path returns the killed index in
`StepCommands.despawns`; the app removes the node for real. `park_y` is deleted.
Killed enemies are gone; on player death only *surviving* enemies reset to spawn.
Both 100% covered, branchless; goldens re-baselined, replay/fork still
byte-identical.

`Input` mapped-bindings and `Variation` RNG are intentionally **not** built yet:
the Law discovers them from a second game's wall, not a category checklist.

## Second game: the roomed-puzzle reduction

`apps/axiom-roomed-puzzle` is the second, deliberately different genre — a
deterministic top-down **grid puzzle** (a block walks a room cell-by-cell; `q`
freezes a *ghost* that replays its recorded path; `r` restarts; ghosts are solid
and stand on buttons that open doors). Reducing it against the vocabulary:

| Need | Verdict |
|---|---|
| ghosts created at `q`, cleared at `r` (`self.ghosts.push` / `.clear`) | **spawn** + **despawn** (lifecycle) |
| move cell-by-cell, blocked by walls/doors/actors (`can_enter`) | the game's **own discrete grid** — no engine query |
| arrows/WASD → Move, `q` → freeze, `r` → restart (`input_mapping.rs`) | **input action-bindings** (discrete) |
| ghost cadence + replay (`SimulationClock`/`TickDivider`/`ReplayTimeline`) | kernel **fixed-step time** ✓ |
| randomness | **none** |

### What it proves

- **Orthogonality holds.** A grid game takes *lifecycle + input + time* and
  **skips spatial reasoning entirely** — it tracks its own cell occupancy, so
  `raycast`/`overlap` are genre-specific (FPS/continuous), not universal. The
  vocabulary is "take what you need," exactly as intended — no game is forced
  through every primitive.
- **`spawn` and input-bindings are now pressure-justified** by a real
  second-game wall (ghosts spawn; keys map to discrete actions), so they graduate
  from "deferred" to "build it."
- **Variation stays deferred** — neither game uses randomness. Honest: still no
  wall for it.

### Category 3 — runtime spawn landed

`spawn` completes the lifecycle pair with `despawn`. `RunningApp::spawn(Spawn{…})`
creates a node at runtime from a renderable + optional player marker + optional
bounds (the scene methods that already exist, surfaced for runtime use). Its
first consumer retires a DOOM compromise: last turn, player-death respawn left
killed enemies dead (no spawn primitive); now respawn **re-spawns** the full
enemy set — `StepCommands.spawns` carries the revived enemies and the app calls
`spawn`. Branchless, 100% covered; DOOM goldens re-baselined.

### Next

- **Input action-bindings** — the other primitive the roomed-puzzle reduction
  surfaced (discrete `key → action` mapping owned by the engine).
- Port roomed-puzzle to render through the engine scene as the end-to-end
  consumer (today it draws to its own 2D canvas), once spawn + input-bindings are
  both in.
- The introspectable host surface (the agent-readable spec) and the scripting
  front-end on top of it.
