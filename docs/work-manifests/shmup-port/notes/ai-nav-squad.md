# `ai/nav.js` + `ai/squad.js` — audit and goldens

**Slice:** `apps/shmup/src/ai/nav.rs` (861 lines, from `src/ai/nav.js`, 510) and
`apps/shmup/src/ai/squad.rs` (227 lines, from `src/ai/squad.js`, 113).

Both were already ported and wired in, with **zero tests**. This pass audited
them function by function against the source, fixed what was wrong, and pinned
both with a golden captured by running the original JavaScript under Node 24.

## Files

| | |
|---|---|
| `apps/shmup/src/ai/nav.rs` | edited — three defects, below |
| `apps/shmup/src/ai/squad.rs` | edited — two fields made `pub`, one doc addition |
| `apps/shmup/tests/ai_nav_port.rs` | new — 16 tests |
| `apps/shmup/tests/ai_nav/capture.mjs` | new — the Node capture |
| `apps/shmup/tests/ai_nav/golden.json` | new — 37 KB, byte-reproducible |

Nothing else was touched. `ai::nav` and `ai::squad` are already declared in
`apps/shmup/src/ai/mod.rs`, and `serde_json` is already a dev-dependency, so
this slice adds no wiring line of its own — but it now depends on the
concurrent `jsmath` slice's `pub mod jsmath;` in `lib.rs` (see defect 1 below).

## Audit result

**The port was structurally complete.** Every function in both source files has
a counterpart, and the transcription of the A* loop, the string pull, the cover
scoring and the whole of `Squad` is faithful — constants, call order and
names all line up, including the `f32` storage discipline for `floor`,
`gScore` and the heap key (which is exactly right and load-bearing: A*'s
`g >= this.gScore[ni]` tie-break and the heap's pop order both compare
`f32`-truncated values in the source).

Three real defects were found and fixed, all in `nav.rs`:

### 1. `Math.hypot` transcribed as `sqrt(x*x + y*y + z*z)` — the named trap

`nav::line_of_sight` (the port's reimplementation of
`src/physics/index.js:616-623`) computed `(dx*dx + dy*dy + dz*dz).sqrt()` where
the source calls `Math.hypot(dx, dy, dz)`. The other four `Math.hypot` sites in
`nav.js` were transcribed as Rust's `f64::hypot`, which is a *third* algorithm —
closer, but still not V8's.

V8 implements `Math.hypot` (`math.tq`'s `MathHypot`) by dividing every argument
through by the largest magnitude, Kahan-summing the squares of the scaled
values, taking the root and rescaling.

This slice first added a private `hypot2`/`hypot3` to `nav.rs`. A concurrent
slice then landed `apps/shmup/src/jsmath.rs` — one V8 transcription for the
whole crate, with its own golden (`tests/jsmath_port.rs`) — whose doc names
`ai/nav.rs` as one of the six duplicates to fold in. So `nav.rs` now imports
`crate::jsmath::{hypot2, hypot3}` and carries no copy.

> **Cross-slice dependency, flagged for the integration pass:** `ai/nav.rs`
> will not compile until `apps/shmup/src/lib.rs` declares `pub mod jsmath;`.
> That line is the `jsmath` slice's own reported wiring, not new work here.

This is not pedantry about the last bit. Those distances feed decisions:
`ceil(dist / (cell * 0.65))` picks `lineOfWalk`'s step count; `d_t` is gated
against hard `2.5` / `40` thresholds and then *divides* into the protection dot
product; and `score > bestScore` picks between cover points that are frequently
tied by symmetry in a grid world.

### 2. An `if`-expression directly multiplied

`let mut cost = if dx != 0 && dz != 0 { SQRT2 } else { 1.0 } * cell;` — the
source's `(dx && dz ? SQRT2 : 1) * cell`. It does parse the way the port
intended, but it is a parse hazard in a line whose meaning must be obvious;
parenthesised.

### 3. `squad.rs`: `peek_timer` / `peek_holders` were private

Both are ordinary public fields in the source (`this.peekTimer`,
`this.peekHolders`). Making them `pub` matches the source and lets the golden
pin the peek-rotation timer and holder set, which are the whole point of the
class. This is not API-widening-for-a-test: it restores the source's surface.

## Divergences carried forward, and why

* **`find_path` clears `last_raw`; the source does not.** `findPath` rewrites
  `this._raw` only on a successful search, so after a failed or
  short-circuited call it still holds the *previous* call's parent chain. The
  capture records that (path 3, the sealed pocket, reports path 2's chain) and
  `the_raw_path_buffer_is_stale_after_a_failed_search` pins both facts. The
  port clears instead, because `last_raw_path()` is an accessor this port
  *invented* for testing and a stale value read through it would be a trap.
  Nothing in the source ever reads `_raw` outside the one function that writes
  it, so no behaviour depends on the difference.
* **`Squad::request_peek` drops the source's `dt` parameter.** `requestPeek(agent, dt)`
  never reads `dt`. Kept dropped rather than carrying a dead argument; noted
  here.
* **`Squad` has no `_pending`.** `this._pending = []` (`squad.js:28`) is never
  read or written anywhere in the source — not by `squad.js`, `agent.js` or
  `ai/index.js` — so it has no element type to port. Recorded in the struct's
  doc comment instead of inventing a `Vec<?>`.
* **`Squad::update`'s flanker check needs the flanker in the snapshot list.**
  The source holds a live object reference, so `this.flanker.alive` always
  resolves; the port looks the id up in the caller's `&[MemberSnapshot]` and
  drops the flanker if it is absent. Identical whenever the caller passes the
  squad's own members, which is the only sane call.
* **`CoverPoint::score` is dead in the source** (written `0` at construction,
  never again — `pick` keeps its running score in a local). Kept, per the
  recipe, and `cover_point_score_is_dead_in_the_source` proves every captured
  point still reads `0`.

## The golden: a synthetic world, with the instrument pinned first

`nav.js` reads the level through three physics calls (`raycast`, `raycastAny`,
`lineOfSight`) plus `phys.MASK`. Standing up the real `PhysicsSystem` would have
meant generating a whole level to get some rays, and the resulting grid would be
far too large to read.

So both sides drive a **synthetic world**: an ordered list of axis-aligned boxes
with a slab ray test. That stub necessarily exists twice — in `capture.mjs` and
in `ai_nav_port.rs`'s `BoxWorld` — which is exactly the transcription risk the
recipe warns about for GLSL held in JS strings. Two things contain it:

1. **The scene comes from the golden.** `BoxWorld` is built by reading
   `scenes.A` / `scenes.B` out of `golden.json`, so the box list is never
   hand-copied; only the ~25-line intersection routine is duplicated.
2. **`the_probe_stub_matches_the_capture` runs before anything else.** It
   replays 16 fixed rays — axis-aligned, diagonal, grazing, origin-inside,
   out-of-range, both masks — and demands bit-exact `t` / `point` / `normal`. If
   the two stubs drift, that test fails first and names the real cause instead
   of letting every nav assertion blame the port.

`MASK` / `LAYER` are imported from the real `src/physics/surfaces.js`, not
hand-typed, and `lineOfSight` is transcribed line-for-line from
`src/physics/index.js:616-623` (including its `Math.hypot(dx, dy, dz)` and its
`d - 1e-3` shortening).

`golden.json` is byte-identical on re-run (verified).

### Scenario A — a street-shaped world

Ground, two long walls that force a detour, a low platform (step 0.4, inside
`maxStep`) and a high one (step 0.9, outside it, so its top is an isolated
component), a free-standing pillar, a low crate (a standing shot clears it →
`high: false`) and a tall one, a three-sided niche that trips `build()`'s
`blocked >= 3` rejection, and a sealed pocket in the far corner that A* can
never reach. 30x30 cells, 899 walkable, 106 cover points (96 high, 10 low).

One deliberate detail: **the walls straddle the cell grid rather than sitting on
it.** A wall face exactly on a cell boundary is 0.8 m from the nearest cell
centre and `build()`'s shoulder probe only reaches `radius + 0.06` = 0.42 m, so
a grid-aligned wall leaves `enclosure` at 0 on both sides and `CoverMap.build`
skips it entirely — the world generates *no cover along its walls at all*. Worth
knowing when the real level generator is wired up.

### Scenario B — the two `build()` arms a well-formed world cannot reach

**Finding.** `build()`'s crouch-only arm (`flags = 2`) and its blocked-ceiling
`else continue` arm are **unreachable in any world whose bounds contain its
geometry.** The down-ray starts at `topY = bounds.max.y + 4` and takes the
nearest hit below it; the up-ray reaches at most `floor + 1.83`. So any ceiling
the up-ray could find was already found by the down-ray and *became* the floor.
There is no backface culling to escape through — `bvh.js`'s raycast tests both
faces deliberately, for bullet exit hits.

The arms only open when the bounds **under-cover** the geometry, i.e. when
`floor > bounds.max.y + 2.17`. Scenario B is exactly that world:
`bounds.max.y = -3` over a floor at `y = 0`, with overhangs at 1.05 (blocked)
and 1.30 (crouch-only) that sit above `topY = 1`. The capture produces 4
crouch-only cells and 6 blocked ones, and
`scenario_b_reaches_the_crouch_only_and_blocked_ceiling_arms` names the finding
so a future reader does not delete those arms as dead code.

### Also pinned as source behaviour, not port bugs

* **The default 6000-node budget can starve a reachable path.** On this 30x30
  grid, `[20,0,1] -> [1,0,1]` exhausts it and returns nothing, while the exact
  mirror-image query succeeds in about half the pops. The capture runs the same
  pair again with `maxNodes: 20000` to prove the goal really is reachable.
  (`the_default_node_budget_can_starve_a_reachable_path`.)
* **Wall tops are walkable.** The down-ray finds a flat roof, its normal passes
  the slope test and nothing is above it, so every wall/pillar top is
  `flags = 1`. They are harmless — the 3 m step blocks every transition on and
  off them — but they are why walls generate no `adj`-based cover.

## What is pinned, and at what tolerance

**Almost everything is exact.** `flags` / `enclosure` are integers; `floor` is
`Float32Array` in the source and `f32` here, compared bit-for-bit; cell
coordinates are `min_x + ix * cell`; A* chains are integer indices; cover `dist`
is a slab `t` built only from `- * /`; waypoints are those same coordinates.

| | |
|---|---|
| `NavGrid::new` | `min_x`/`min_z`/`nx`/`nz`/`top_y`/`max_slope`, both scenes |
| `NavGrid::build` | every cell's `flags`, `floor`, `enclosure`; `walkable_count`; **the total ray count** (5 690 for A, 360 for B) — a direct check that the port makes the same probes in the same order |
| `NavGrid::nearest` | 13 queries: direct hit, ring search, `y` tie-break, `y_tol` rejection, ring exhaustion, off-grid |
| `NavGrid::find_path` | 13 pairs incl. `start == goal`, adjacent cells, unreachable pocket, off an isolated platform, `max_nodes` starvation, default-budget exhaustion |
| the raw A* chain | `last_raw_path()` after every successful search |
| `NavGrid::line_of_walk` | 9 segments incl. degenerate and a step-limit rejection |
| `CoverMap::build` | all 106 points: position, facing, `high`, `dist`, `claimed`, `score` |
| `CoverMap::pick` | 11 picks: claiming, re-claiming, squad bunching, range/travel/`y_ref` filters, no-candidate |
| `CoverMap::release` | the post-release claim table |
| `CoverMap::peek_offset` | 9 peeks, covering all three `s` arms (`1`, `-1`, `0`) |
| `line_of_sight` | 8 segments incl. both edge arms |
| `Squad` | 14 snapshots over a scripted frame sequence |

Two things carry a stated tolerance, `1e-12` (the established figure in this
port):

* **`max_slope`** = `cos(46 * PI / 180)`, one libm `cos`. It is only ever
  compared against normals of exactly `0` / `±1`, so the tolerance cannot
  change a decision.
* **The squad's RNG-derived timers** (`peek_timer`, `grenade_cooldown`,
  `last_known_age`). `Rng::float` is bit-exact (`tests/core_port.rs`) and so is
  `0.9 + f * 0.8`; the tolerance covers only the long `-= dt` accumulations
  that both sides perform in the same order — the source's own captured value
  already shows the drift (`7.499999999999989`).

Distances are **not** given a tolerance, because `hypot2`/`hypot3` now
reproduce `Math.hypot` exactly.

## Determinism: the fork is pinned

`ai/index.js:541` builds a squad as `new Squad(this.rng.fork())`, so the squad
draws from the *child* stream and the root advances by exactly one `u32()`. The
capture does the same (`new Rng(20260821)`, then `.fork()`) and records **both**
the root's four state words after the fork and every value the child stream then
produces:

* the peek-rotation timer (`1.1 + float() * 1.2`), redrawn every time it
  expires — the scripted sequence expires it many times;
* the three call-out ages (`0.9 + float() * 0.8`), drawn **in squad-member
  order**, only for members whose `lastKnownAge > 1.5` — the capture includes a
  frame where the same contact re-broadcasts and draws *nothing*, which is the
  case an extra draw would silently break;
* the grenade re-arm (`14 + float() * 12`).

The state words are emitted `>>> 0` because JS `^` yields a signed int32, so
the source's state reads negative after the first draw. Same bits.

The squad replay drives a mock member with exactly the field set `squad.js`
touches, and applies the port's returned `ContactBroadcast`s the way
`squad.js:64-68` writes them — deliberately **not** through
`ai::agent::Agent::receive_squad_contact`, so this suite does not couple to a
file another slice is editing.

## Not done here

* `Squad`'s `_nextSquad` module counter — the port takes the id from the
  caller, matching the choice already made for `Agent`'s `_nextId`. Wiring it
  is `ai/index.js`'s job.
* `NavGrid.buildMs` / `CoverMap.buildMs` (`performance.now()` timing) are not
  ported. They are telemetry, they are wall-clock, and the Determinism rules
  put wall-clock behind an explicit boundary.
* Nothing in `nav.js` is unported.
