# 12 — Burnt Rubber Traffic Variation Preparation (OPTIONAL)

> **This manifest is optional and may be skipped without affecting any other.**
> It is the only manifest that touches simulation code, and its benefit is
> narrow: it removes the app's *last* runtime `Draw::seeded` call. Skip it if the
> programme is running long or if golden stability is proving fragile.

## Mission

Fold the two traffic *variation constants* — `wander_phase` and `wander_amount` —
onto `TrafficPlan` at course-compile time, so `Traffic::activate` reads them
instead of drawing them. This makes the gameplay path RNG-free without freezing
any dynamic behaviour.

## Architectural owner

- **Package:** `apps/burnt-rubber`
- **Classification:** App
- **Why here:** traffic is a racing concept.

## Depends on

**`11-burnt-rubber-wiring.md`** — the golden run must be green and stable before
anything touches the simulation.

## Parallel safety

None. Runs alone.

## Files owned

| Path | Action |
|---|---|
| `apps/burnt-rubber/src/course/traffic/mod.rs` | modify — **`TrafficPlan` is declared at `:44`** |
| `apps/burnt-rubber/src/course/traffic/flow.rs` | modify (648 lines) |
| `apps/burnt-rubber/src/course/traffic/encounters.rs` | modify (583 lines) |
| `apps/burnt-rubber/src/course/validation/mod.rs` | modify — 4 struct-literal sites (`:630, :757, :786, :813`) |
| `apps/burnt-rubber/src/course/validation/traversal.rs` | modify — 2 struct-literal sites (`:329, :480`) |
| `apps/burnt-rubber/src/sim/traffic.rs` | modify (904 lines) |

`TrafficPlan` has all-`pub` fields and is built by **struct literal at 8 sites
across 3 files** beyond `flow.rs`/`encounters.rs`. Adding two fields therefore
touches every one of them — an earlier draft owned only three files and was
unimplementable.

## Files allowed to modify

Only the six above.

## Files forbidden to modify

- `apps/burnt-rubber/src/preparation/**` — this is **not** a preparation task; it
  is a change to what the course compiler emits
- `apps/burnt-rubber/src/app.rs`, `render/**`, `sim/mod.rs` — `08`/`11`
- `apps/burnt-rubber/tests/golden/**`, `slice.toml`, `tests/agent_golden.rs` —
  **read-only**

> **If implementation appears to require a forbidden file, stop and report the
> dependency to the orchestrator rather than modifying it.**

## Existing code to study first

| Path | Why |
|---|---|
| `apps/burnt-rubber/src/sim/traffic.rs:339-346` | **The whole justification.** Documented as *"A pure function of the plan and nothing else — not of when it activated, not of which pool entry it landed in, not of how the player got here."* Then `Draw::seeded(plan.variation_seed)` draws `wander_phase` then `wander_amount` |
| `apps/burnt-rubber/src/sim/traffic.rs:266-308` | `Traffic::activate` — keyed on player distance and free slot. **This scheduling stays runtime** |
| `apps/burnt-rubber/src/course/traffic/flow.rs:243` | Where `variation_seed` is assigned at compile time |
| `apps/burnt-rubber/src/course/traffic/encounters.rs:110` | The same for authored encounters |

## Contract consumed

Nothing new. This manifest changes a data shape internal to the app.

## Contract produced

`TrafficPlan` gains two `f32` fields carrying the pre-drawn wander pair.
`Traffic::activate` reads them instead of drawing.

## Implementation instructions

1. At the two assignment sites (`flow.rs:243`, `encounters.rs:110`), after
   `variation_seed` is chosen, draw the pair **in exactly the same order** the
   runtime does today:

```rust
let mut draw = Draw::seeded(variation_seed);
let wander_phase  = draw.range(0.0, std::f32::consts::TAU);
let wander_amount = draw.range(0.1, 0.45);
```
   Store both on the plan.

2. In `sim/traffic.rs:339-346`, delete the `Draw::seeded` and read the two fields
   instead. **Change nothing else** in `activate`.

3. **Do not touch anything else in traffic.** Activation scheduling, wander
   integration, yielding and retirement are gameplay and stay in the frame loop.
   This manifest moves two *constants*, not behaviour.

## Required behavior

- Every traffic car receives byte-identical `wander_phase` and `wander_amount`
  to today.
- Activation timing, slot assignment and every downstream behaviour are
  unchanged.
- `sim/traffic.rs` contains **no** `Draw::seeded` afterwards — verify with
  `rg 'Draw::seeded' apps/burnt-rubber/src/sim/`.

## Error behavior

None. Both draws are infallible.

## Determinism requirements

**This is the crux.** The pair must be drawn from the same seed, in the same
order, with the same range arguments. `Draw::seeded(plan.variation_seed)` starts
a fresh stream, so drawing at compile time yields identical values — that is
exactly what makes this provably safe. If the values were to differ by one ULP
the golden state bytes would move, which is the detector.

## Tests

Inline in `sim/traffic.rs`:

- `a_prepared_wander_pair_matches_the_runtime_draw` — construct a plan, compare
  against a direct `Draw::seeded(variation_seed)` sequence. **The key test**
- `activation_no_longer_draws` — the whole point
- `traffic_placement_is_deterministic_across_two_identical_runs` — the existing
  test must still pass unchanged

## Architecture validation

`apps/` is outside the branchless, coverage and dylint gates.

## Performance considerations

Negligible per frame — two `f32` reads replace two RNG draws per activation.
The real value is architectural: the gameplay path becomes RNG-free.

## Documentation changes

A comment at the compile-time draw sites explaining that the pair is pre-drawn
precisely because `activate` is documented as a pure function of the plan.

## Completion criteria

- [ ] `TrafficPlan` carries both values
- [ ] `Traffic::activate` draws nothing
- [ ] `rg 'Draw::seeded' apps/burnt-rubber/src/sim/` returns **no** hits
- [ ] Activation scheduling untouched
- [ ] All 8 struct-literal sites updated
- [ ] `cargo test -p axiom-burnt-rubber` green
- [ ] **All 15 golden files byte-unchanged** — assert this explicitly; it is the
      one manifest that touches the simulation

## Validation commands

```sh
cargo test -p axiom-burnt-rubber
cargo test -p axiom-burnt-rubber --test agent_golden
rg 'Draw::seeded' apps/burnt-rubber/src/sim/
```

## Deliverable to orchestrator

Report: commit hash; six paths; **explicit confirmation that all 15 golden
artifacts are byte-unchanged**; the `rg` output proving no runtime draw remains;
deviations. If the goldens move by even one byte, **revert and report** — do not
re-bless.
