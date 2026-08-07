# Burnt Rubber — Courses

How a Burnt Rubber course is authored, compiled, validated and driven.

Everything described here lives in `src/course/` and is **app-local**. A course
motif, a racing encounter, a near-miss opportunity and a boost budget are this
game's opinions, not engine capabilities, and nothing in this system was added to
a layer or a module to support it.

---

## 1. What replaced what

The course used to be a procedure. `track/generate.rs` walked a list of control
points, drawing a heading step and a grade per point from a per-section envelope
baked into an enum; `sim/traffic.rs` placed cars at `k · 85 m` with a lane and a
speed that were an arithmetic function of `(seed, k)`.

Both were deterministic, and neither could be **authored**. There was nowhere to
say "put a zipper here", nowhere to ask "is this passable at all", and no way to
write a course down and read it back. The pacing plan was nine enum variants; a
tenth section meant editing the generator.

What replaced it is a compiler. `track/generate.rs`, `track/section.rs` and
`track/spline.rs` are gone; `track/mod.rs` is now only the sample table and the
questions everything asks of it.

---

## 2. The pipeline

```text
   courses/*.brc  (text)                 procedural::shipping_spec  (Rust)
            │                                        │
   authoring::parse                        CourseBuilder
            └──────────────┬─────────────────────────┘
                           ▼
                      CourseSpec                          specification/
                           │
                   compiler::expand      motifs and groups become sections
                           ▼
                     ExpandedCourse                       compiler/
                    ┌──────┴───────┐
          geometry::compile     traffic::flow / traffic::encounters
        Track + CompiledSection    TrafficPlan / CompiledEncounter /
                    │              NearMissWindow
                    └──────┬───────┘
                           ▼
                 validation::validate                     validation/
              grid + budget (+ ghost, offline)
                           ▼
                      CoursePlan                          runtime/
                 immutable, indexed, shared by Arc
                           │
                    RaceSim::from_plan
                           ▼
                    the running game
```

Three rules hold the shape together:

* **Distance is the coordinate.** Everything authored is stated in metres along
  the course, and everything compiled is addressed by them. There is no
  authoring interface in world space at all.
* **Compilation happens once**, at `RaceSim` construction. The runtime reads a
  sorted array through a bucket index. It never parses, never re-expands and
  never re-validates — by the time it holds a `CoursePlan` the spec is gone.
* **A restart reuses the plan.** `RaceSim::restart` clones the `Arc`; it does not
  recompile. Two things that must agree cannot disagree if there is only one of
  them.

---

## 3. The distance coordinate, `s`

`s` is arc length in metres from the start line. The compiled `Track` is
**arc-length uniform** at 2 m spacing, so sample `i` is at exactly `i · 2 m` and
"4 200 m along" is a distance rather than a parameter.

At any `s` the compiled course resolves:

| Question | Where |
|---|---|
| world position, tangent, right, up | `TrackSample` |
| curvature, grade, bank | `TrackSample` |
| road half-width | `TrackSample::half_width` |
| lane count, lane centre, lane at an offset | `Track::lane_count` / `lane_lateral` / `lane_at_lateral` |
| shoulder, verge, barrier line | `Track::shoulder` / `verge` / `barrier_offset` |
| section identity | `TrackSample::section_index` → `CoursePlan::sections()` |
| expected player speed | `TrackSample::expected_speed` |
| environment / scenery profile | `TrackSample::section` (`SectionKind`) |
| traffic ahead | `CoursePlan::first_vehicle_at` |
| the encounter here | `CoursePlan::encounter_at` |
| near-miss opportunities ahead | `CoursePlan::windows_ahead` |

Lane width is **constant for the whole course** (`CourseDefaults::lane_width_m`),
because `Track::lane_lateral` puts lane `n` at `n · lane_width` everywhere and
that is what makes a lane a durable identity rather than an ordinal into a list
that keeps changing length.

---

## 4. The unit convention

Simulation is SI throughout, and the specification adds one rule: **every scalar
field names its unit in its own name** — `_m`, `_km`, `_mps`, `_rad`, `_s`, or a
bare name for a count, weight or probability.

There is no dimensioned-quantity framework. The one place a unit is genuinely
ambiguous is a DSL literal, and there a unit suffix is **mandatory**: `700m`,
`18deg`, `0.75s`, `180mph`. An unknown suffix is `invalid-unit`, not a silently
accepted bare number.

---

## 5. Road primitives

Each compiles into a signal — heading rate, grade, bank, width — over its own
length. The compiler lays every section's signals end to end and integrates them
**once**, so position, tangent and heading are continuous by construction rather
than by agreement; nothing restarts at a section boundary and no primitive ever
writes a position.

| Primitive | Fields | What it does |
|---|---|---|
| `straight` | `length` | level, straight |
| `turn` | `length`, `radius`, `direction` | constant radius across the middle, eased at both ends |
| `s_bend` | `length`, `radius`, `first` | curvature is a full sine, so it reverses exactly at the midpoint |
| `crest` | `length`, `height` | `h(t) = height·(1−cos 2πt)/2` — zero slope at both ends, returns to level |
| `dip` | `length`, `depth` | a crest, mirrored |
| `bank_transition` | `length`, `from`, `to` | smoothstep between two bank angles |
| `lane_transition` | `length`, `from`, `to` | the *width* ramps; the lane count follows from it |
| `width_transition` | `length`, `from`, `to` | tarmac widens or narrows without changing lanes |

**Not implemented, and why.** There is no jump primitive: the car model has no
jump case — gravity always applies and the road is a floor — so a crest steep
enough to unload the car *is* the jump, and a separate primitive would be a
second name for the same thing. There is no bridge: the renderer has no way to
draw one. A tunnel or a walled corridor is expressible because the road mesh and
the scenery pool already know how to draw an enclosed section, and that is what
`environment = tunnel` / `canyon` selects.

### Enforced

Clamped and rate-limited in `geometry::correct`, then re-checked in
`validation::check_geometry`:

* position continuity (each sample exactly one spacing from the last)
* tangent continuity (`cos 2.6°` between adjacent tangents)
* curvature magnitude (`1 / min_turn_radius_m`) and step (`max_curvature_step`)
* grade magnitude (`CourseTuning::max_grade`) and step
* bank magnitude and step
* half-width inside the course's legal band, and its rate of change
* lane counts odd and ≥ 3, and a width the tarmac can actually carry

Where a clamp actually bit, the count comes back in `GeometryClamps` and the
validator raises a **warning naming the section**: the compiled road is not the
road that was authored, and saying so is more useful than either silently
changing it or refusing to build it.

---

## 6. Modifiers

Layered on a section's base primitive, in order.

| Modifier | Fields |
|---|---|
| `lateral_wave` | `amplitude`, `wavelength`, `phase` |
| `elevation_wave` | `amplitude`, `wavelength`, `phase` |
| `grade_profile` | `drop` — a sustained change of elevation |
| `banking` | `mode` (`follow_curvature`/`fixed`/`flat`), `strength`, `maximum` |
| `width_profile` | `from`, `to` (half-widths) |
| `lane_profile` | `from`, `to` (lane counts) |

`grade_profile` is **the only way to author a net elevation change**, and it is a
modifier rather than a primitive because elevation is orthogonal to what the road
does in plan — a descending turn is a turn with a drop on it. `crest` and `dip`
both return to the level they started at by construction and an elevation wave is
periodic, so before it existed the only way to end lower than you began was to
ride a quarter of a wave whose wavelength you had worked out by hand.

Its grade is **constant** across the section rather than eased at the ends. That
is what lets a figure cut into several sections descend continuously *through*
the joins instead of levelling off at each one; the compiler's rate limiter
smooths the ends of the whole figure, so a section falls a little short of the
drop it asked for wherever it meets level road.

A lateral wave is realised as **curvature** (`y'' = −A k² sin(ks + φ)`), not as a
displacement added to finished positions. That matters: displacing a centreline
after the fact leaves every tangent — and therefore every lane, every barrier and
every prop — pointing where it was before the wave existed.

---

## 7. Motifs

A motif is shorthand for road an author would otherwise write out, and it stops
existing the moment it is expanded. `motif high_speed_sweeps { count = 4 }`
becomes eight ordinary `SectionSpec`s named `<id>/bend0`, `<id>/link0`, …, and
nothing downstream records that a motif was involved.

| Motif | Expands into |
|---|---|
| `high_speed_sweeps` | `count` alternating banked turns, each with a link straight |
| `alternating_slalom` | `count` S-bends butted together, alternating which way they open |
| `rolling_freeway` | `count` straights carrying a lateral and an elevation wave, phase-continuous across the joins |
| `tunnel_squeeze` | collapse → corridor → release, at `narrow_lanes` |
| `blind_crest` | approach → crest → a turn you cannot see |
| `lane_collapse` | staged lane loss with no recovery |
| `corkscrew` | one continuous banked turn, descending far enough to pass under itself |

The corkscrew is worth a note because it is the one motif that **derives** its
geometry rather than taking it: it is told how much road it has and how many
revolutions to spend it on, and the radius falls out (`count` is revolutions for
this motif, not repetitions). That is the right way round — "one turn down a
ridge in twelve hundred metres" is the design and the radius is its consequence —
and if the consequence is tighter than `min_turn_radius` the compiler rejects it
by name rather than quietly opening the figure out.

It is also deliberately a **single** turn section rather than a string of them: a
`turn` eases its curvature in and out at each end, so a helix built from several
would relax to straight between every coil and be a sequence of corners rather
than a screw.

Every motif draws from `SeedDomain::Motif` salted by its **own stable id**, so
re-tuning one motif cannot re-roll another. `count` is bounded by
`MAX_MOTIF_COUNT` (64).

---

## 8. Traffic

### Ambient flow

A **distance-based density description** — never a spawn timer. Traffic
generated on a timer puts more cars in front of a slow player and fewer in front
of a fast one, so the road a player meets depends on how well they are driving
and a course cannot be authored at all.

| Field | Meaning |
|---|---|
| `vehicles_per_km` | target density |
| `min_headway` / `preferred_headway` / `max_headway` | the gap band; draws are two-sided about `preferred` |
| `speed` | the cruising band |
| `speed_relative_to_expected` | blend toward the section's expected player speed |
| `lane -1 = w` … | lane occupancy weights |
| `platoon_probability`, `platoon_size`, `platoon_gap` | knots of cars, followed by the widest legal gap |
| `burst_length`, `recovery_length` | a dense-then-relaxed cycle; burst traffic is also slower |
| `open_corridor_every`, `open_corridor_length` | deliberately empty road |
| `archetype van = w` … | cosmetic shape weights |

The generator is one bounded walk along the zone, placing a vehicle and stepping
forward by a drawn headway. Everything above modifies that step, which is why
they compose: there is one cursor and one rule for moving it. Bounded by
`MAX_VEHICLES_PER_ZONE` (512).

### Encounters

| Encounter | Shape | Configurable |
|---|---|---|
| `zipper` | every lane but one is blocked; the opening alternates | `at`, `length`, `spacing`, `speed`, `first_open_lane`, `alternation`, `minimum_clearance`, `target_near_misses`, `minimum_reaction_time`, `require_continuous_route` |
| `rolling_wall` | a block of cars around one opening; the opening walks each phase | `at`, `wall_width`, `open_lane`, `opening_step`, `phase_length`, `phases`, `speed`, `group_spacing`, `reaction_distance` |
| `slalom` | single blockers on alternating sides | `at`, `blockers`, `spacing`, `lane_sequence`, `speed`, `clearance`, `recovery_gap` |

All three compile into ordinary `TrafficPlan`s before the runtime sees anything.

### The compiled plan

```rust
TrafficPlan {
    id, spawn_m, despawn_m, lane, speed_mps, archetype,
    lane_changes, speed_changes, encounter, section, variation_seed,
}
```

The runtime's whole job is to notice which plans have entered the forward horizon
and copy them into a bounded pool (`sim/traffic.rs`). Recycling a pool entry
cannot change what a plan contains for the simplest possible reason: the pool
holds copies and the plan is immutable.

---

## 9. Near-miss opportunities

Compilation **never awards a near miss**. It compiles *windows*:

```rust
NearMissWindow { start_m, end_m, vehicles, clearance_m, side,
                 minimum_relative_speed_mps, intended_opportunities,
                 difficulty_weight, encounter, section }
```

A window says "a skilled player can earn a near miss here". Whether they did is
`sim::collision::is_near_miss`'s business, and nothing in the course system can
reach it. The scoring rule is unchanged.

An ambient window is placed at the **meeting point**, not at the spawn point: a
car placed 620 m ahead at 30 m/s is level with a player doing 78 m/s some 388 m
further on. `traffic::meeting_distance` is that projection, and the traversability
grid uses the same one — there is one model of where the player meets a car.

---

## 10. Deterministic seed partitioning

One course seed, six independent streams, and a per-section stream inside each:

```text
course_seed ─fork(domain salt)─▶ Geometry / Motif / TrafficFlow /
                                 TrafficEncounter / Scenery / Cosmetic
                    └─fork(StableHash(section id))─▶ this section's stream
```

Two consequences the tests pin directly:

* changing the scenery seed cannot move the road or the traffic;
* adding a vehicle in one section cannot change any earlier section.

Section streams are keyed on the **stable name**, never on an index — inserting a
section ahead of another must not re-roll the second one.

---

## 11. Traversability validation

A bounded distance–lane occupancy grid and a forward reachability sweep.

```text
 lane +2 │ · · ▓ ▓ · · · · ·
 lane +1 │ · · · ▓ ▓ · ▓ ▓ ·
 lane  0 │ ▓ ▓ · · ▓ ▓ ▓ · ·      ▓ blocked   · free
 lane −1 │ · · · · · ▓ · · ·
 lane −2 │ · ▓ ▓ · · · · ▓ ▓
         └────────────────────▶  course distance
```

* one column per `traversal_step_m` (30 m), one row per lane;
* a cell is blocked where the road has no lane, or where a projected vehicle
  expanded by the player's half-width plus `lateral_margin_m` sits;
* a transition between adjacent columns is legal only if the player could cross
  that many lanes in the time the column takes at `lateral_speed_mps`;
* when the reachable set empties, the blockage is recorded and the sweep
  **restarts** from the next column's free lanes, so one pass lists every blocked
  stretch rather than the first.

It is deliberately not a driving model. It does not model braking, racing lines
or the player choosing to slow down — all of which only ever *add* routes — so a
course it passes may still be hard, and a course it fails is genuinely impossible
for a player holding the expected speed.

Alongside it, `validation::validate` checks: geometry continuity, lane widths and
counts, traffic on the road, traffic not overlapped at spawn, encounter row
headway, encounter reaction time, encounter lateral clearance, encounter lanes
that exist, near-miss windows pointing at real vehicles, and near-miss clearances
the road can actually offer.

The report is a `ValidationReport` — errors, warnings **and measurements** — in a
total order (severity, distance, error code, rendered line), so two runs of the
same compilation produce byte-identical reports.

---

## 11a. What a course authors about its own envelope

`ValidationThresholds` is not only what the validator *judges* against — it is
also the envelope the geometry compiler clamps to, and both are per-course:

| Threshold | What it bounds |
|---|---|
| `min_turn_radius` | the tightest turn any primitive may author |
| `max_grade` | the steepest the compiled road may get |
| `max_bank` | the hardest the compiled road may lean |
| `max_curvature_step`, `max_grade_step`, `max_bank_step` | how fast each may change between adjacent samples |
| `traversal_step`, `lateral_speed`, `lateral_margin`, `min_reaction_time` | the traversability model |
| `near_miss_conversion`, `target_boost_duty`, `high_speed_share`, `starved_ratio`, `excellent_ratio`, `excellent_route_width` | the boost budget |

`max_grade` and `max_bank` live here rather than in `CourseTuning` because how
steep and how banked a road may get is a property of *a course*, not of the game:
a rolling motorway and a road that screws its way down a ridge want different
answers, and the author of each is the one who knows which. What stays in
`CourseTuning` is `bank_per_curvature` — how hard the road leans *per unit of
corner* — because that is the game's road-building style rather than one course's
limit.

---

## 12. Boost-sustain analysis

```text
earned = Σ chances · near_miss_boost · near_miss_conversion · difficulty
spent  = (section_length / expected_speed) · boost_drain_rate · target_boost_duty
ratio  = earned / spent
```

| Status | When |
|---|---|
| `invalid` | no traversable route through the section |
| `starved` | `ratio < starved_ratio` |
| `acceptable` | between the two |
| `excellent` | `ratio ≥ excellent_ratio` **and** ≥ `excellent_route_width` lanes stay reachable |

It is a **reproducible approximation and says so**. It uses the game's own
numbers (`RaceTuning::near_miss_boost`, `boost_drain_rate`) rather than invented
ones, and every threshold is authored in `ValidationThresholds` — a course that
wants a harsher economy says so in its own source, rather than the number being
buried in the analysis.

Where a ghost run is available its measured boost duty is folded in
(`boost::fold_ghost`), which turns the estimate into a measurement for the one
route the ghost actually drove. The fold can only ever **demote**.

---

## 13. Ghost validation

`validation::ghost::run` puts the app's real `axiom-agent` driver — the same one
the player races in the browser — on a compiled plan and measures:

completed · elapsed · collisions · near misses · boost steps · longest continuous
boost · sections where boost was lost · minimum clearance · average speed ·
encounter failures.

It is a generation-time tool. It never runs during play and cannot reach into a
running race: **the live game never quietly alters traffic to help the player**,
and the plan the ghost was validated against is the plan the player gets.

---

## 14. Runtime activation

`CoursePlan` holds the traffic sorted by spawn distance plus a `DistanceIndex` —
a flat `Vec<u32>`, one entry per 100 m, holding the first list entry at or past
that bucket. Every per-frame question ("which section am I in", "what has entered
the horizon", "what opportunities are ahead") is an array read plus a walk of the
few entries sharing a bucket, instead of a scan of the whole course.

Per fixed step, `Traffic::step`:

1. retires cars the player has left behind or that are past their plan's end;
2. drives every live car along its plan (reading its scheduled lane and speed
   changes at the car's own distance, so a replay is exact);
3. activates every plan that has entered the forward horizon into a free pool
   entry, skipping any that would land inside the player's safety region.

The cursor is monotone while the player moves forward, so a plan activates
exactly once; a jump (`place_at`, a capture, the finish teleport) clears the pool
and recomputes the cursor from the index in one move.

No allocation, no parsing and no recompilation happen on this path.

---

## 15. Debugging and inspection

* `course::runtime::inspect::rows(plan, distance, ahead)` — the live authoring
  rows the debug overlay shows: seed, distance, section id and primitive,
  curvature/grade/bank, lanes and expected speed, the section's traffic, the
  active encounter, upcoming plans, nearest headway, traversability
  classification, near-miss chances ahead, the boost verdict, and the error and
  warning counts.
* `CoursePlan::dump()` — the deterministic textual form of a whole compiled
  course: report, then every vehicle, encounter and window. Same plan in,
  byte-identical string out. This is what a test diffs and what an agent reads to
  answer questions about a course without running it.

---

## 16. The DSL

A small declarative language. It has **no** variables, callbacks, imports,
reflection, runtime evaluation or expressions beyond a literal and a range of two
literals. The only repetition is `repeat N { … }` and `alternate N { … }`, both
bounded at parse time by `MAX_REPEAT` (32). A course source is data.

```text
course "<name>" {
    seed = <int>
    defaults   { lanes lane_width shoulder_width expected_speed environment }
    thresholds { min_turn_radius max_grade max_bank
                 traversal_step lateral_speed lateral_margin
                 min_reaction_time near_miss_conversion target_boost_duty
                 starved_ratio excellent_ratio excellent_route_width }

    <primitive> { id length … <modifier blocks> traffic { … } }
    # modifier blocks: lateral_wave, elevation_wave, grade_profile { drop = 40m },
    #                  banking, width_profile, lane_profile

    section "<name>" { lanes environment expected_speed
                       <primitive blocks> traffic { … } }

    motif <kind> { id count length radius bank elevation_amplitude
                   lateral_amplitude wavelength height lanes narrow_lanes
                   environment expected_speed traffic { … } }

    repeat <n>    { <items> }
    alternate <n> { <items> }        # every other copy is mirrored
}

traffic {
    flow { vehicles_per_km headway=A..B min_headway preferred_headway
           max_headway speed=A..B speed_relative_to_expected
           platoon_probability platoon_size=A..B platoon_gap
           burst_length recovery_length
           open_corridor_every=A..B open_corridor_length
           lane <i> = <w>          archetype <name> = <w> }

    encounter zipper       { … }
    encounter rolling_wall { … }
    encounter slalom       { … lane_sequence = [ -1, 1 ] }

    near_miss { at length clearance=A..B side minimum_relative_speed
                opportunities difficulty }
}
```

Comments are `#` or `//` to end of line. Every dimensioned literal carries a
unit. Every field name is matched against a closed set — an unrecognised one is
`unknown-field` with a line and a column.

Diagnostics carry file, line, column, an error code and the section or field:

```text
burning_coast.brc:12:5: unknown-field: `wobbliness` is not a field of a `straight` block [field wobbliness]
```

### Creating and running a course

1. Write `courses/<name>.brc`.
2. `authoring::parse("<name>.brc", source)` → `CourseSpec`.
3. `compiler::compile(&spec, &Tuning::DEFAULT)` → `CoursePlan` (or
   `compile_valid`, which refuses a plan whose report has errors and lists every
   one of them).
4. `RaceSim::from_plan(Arc::new(plan), tuning, profile)` → a playable race.

The demo course is `courses/burning_coast.brc`, compiled into the binary with
`include_str!` (the app runs in a browser, where there is no filesystem, and a
course the game ships with is part of the game). It is parsed, compiled and
validated by the real pipeline in the test suite.

---

## 17. Extending it

* **A new primitive** — add a variant to `RoadPrimitiveSpec`, give it
  `heading_rate`/`grade`/`bank_rad`/`lanes`/`half_width_m` and a `validate`, add
  a keyword arm in `parser::primitive_section`. The compiler needs no change: it
  integrates signals and does not know what produced them.
* **A new motif** — add a variant to `MotifKind` and a function in
  `motifs`. It cannot introduce a new concept into the compiler, because its
  whole output is sections that already existed.
* **A new encounter** — add a variant to `EncounterSpec` with its own spec
  struct and `validate`, a `*_rows` function in `traffic::encounters`, and a
  keyword arm in `parser::encounter`. It compiles into ordinary `TrafficPlan`s
  like the others.

---

## 18. Current limitations

* **A corkscrew's radius is derived, so asking for more revolutions in the same
  road makes it tighter until `min_turn_radius` refuses.** There is no way to say
  "keep this radius and take as much road as you need".
* **Lane width is per-course, not per-section.** `Track::lane_lateral` puts lane
  `n` at `n · lane_width` for the whole course, and that is what makes a lane a
  durable identity. A per-section width would break it, so the specification does
  not offer one.
* **The road tops out at seven lanes** (`MAX_LANE_REACH = 3`), and the shipping
  course's tarmac band (`min_half_width`/`max_half_width`) only reaches five. An
  authored count the tarmac cannot carry is rejected rather than clamped.
* **The grade and bank envelope is per *course*, not per *section*.** A course
  that wants one dramatic figure has to raise the ceiling for the whole road; the
  shipping course works around it by keeping its ordinary sweepers to an authored
  lean of their own rather than to the ceiling. Per-section envelopes are the
  natural next step and are not built.
* **Speed changes are compiled but the shipping course authors none.** The field
  exists and the runtime honours it; the ambient generator uses a per-burst speed
  scale rather than mid-life changes.
* **The traversability grid assumes the expected speed.** It does not model a
  player who brakes, which only ever adds routes — so it is conservative in the
  right direction, but it cannot tell you a course is *easy*.
* **Ghost validation is one route.** It is a measurement of the agent's line, not
  a proof about every skilled line.
* **The traversal step must be coarse enough to contain a lane change.** At 30 m
  and 78 m/s that is one lane per column; a finer grid reports a zero shift and
  the validator raises it as a configuration error rather than silently deciding
  nothing is passable.
* **No editor.** Courses are text and Rust; there is no authoring UI.
