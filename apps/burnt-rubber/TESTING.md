# Burnt Rubber — Testing

380 tests, all co-located with the code they cover. Everything below runs under
plain `cargo test` on the native target; the `wasm32` browser edge
(`src/web.rs`) is the only file with no native coverage, by construction — see
§6.

```sh
cargo test -p axiom-burnt-rubber           # this app
cargo test --workspace                     # everything
cargo run -p xtask -- check-architecture   # Layer + Module Law
bash scripts/coverage.sh                   # the 100% engine-spine gate
cargo dylint --all -- --all-targets        # the lint rulebook
bash scripts/ts-gate.sh                    # the TypeScript SDK gate
```

---

## 1. Deterministic replay

The headline guarantee: **same seed + same ordered commands ⇒ same everything.**

| Test | What it pins |
|---|---|
| `sim::tests::an_identical_command_sequence_replays_identically` | Car state, boost meter, traffic, camera pose, near-miss and impact counts, over 2 400 mixed steps |
| `script::tests::the_canned_run_replays_identically` | The full seven-stage scripted run |
| `sim::traffic::tests::traffic_placement_is_deterministic_across_two_identical_runs` | Traffic across 3 000 steps |
| `draw::tests::the_same_seed_draws_the_same_sequence` | The seeded source itself |
| `draw::tests::forks_depend_only_on_seed_and_salt` | A fork ignores how far the parent advanced — the property that makes chunk recycling safe |
| `app::tests::stepping_deterministically_ignores_the_clock_entirely` | The step path reads no clock |
| `app::tests::real_time_is_banked_into_whole_fixed_steps` | Elapsed time becomes an integer step count and nothing else |
| `app::tests::edge_triggered_input_fires_once_per_frame_not_once_per_step` | One key press is one action however many steps a frame banks |
| `app::tests::a_stall_does_not_leave_the_simulation_running_fast` | A hitch is dropped, not replayed — twenty ordinary frames after a two-second stall run twenty steps, not a hundred |
| `app::tests::acceleration_after_a_stall_matches_acceleration_without_one` | The same thing where the player feels it: a second of throttle reaches the same speed either way |

Replay is also what the capture slices rest on
(`capture::tests::every_slice_renders_identically_twice`), which compares the
**instance floats** — not a summary — between two independent builds of the same
slice.

---

## 2. Track generation invariants

Asserted across **seven distinct seeds**, not just the shipping one, so a
constraint that happens to hold for the demo course but not in general fails.

`track::generate::tests` and `track::tests`:

* identical seed ⇒ identical control points and identical sampled centreline;
* different seeds ⇒ courses that diverge by >100 m;
* every value finite;
* heading step ≤ `max_yaw_step`; heading-step *change* ≤ `max_yaw_step_delta`
  (the no-instant-reversal bound, tested independently of the magnitude bound);
* grade ≤ `max_grade`, grade change ≤ `max_grade_delta`;
* banking ≤ `max_bank`, and it leans **into** the corner (the outside edge is
  the raised one — asserted geometrically, not by inspecting the sign);
* half-width inside `[min_half_width, max_half_width]`;
* minimum turn radius > 90 m;
* the road is never inverted (`up.y > 0.5` on every sample);
* every sample's frame is orthonormal;
* distance is monotone and evenly spaced at exactly 2 m;
* the start line and the finish are straight and level;
* the nine sections appear in the authored order and cover every sample.

The relaxation pass is also tested **directly** on a signal engineered to
violate both bounds (`relaxation_enforces_both_bounds_on_a_hostile_signal`), and
on degenerate inputs, so the safety net is proven rather than assumed.

### Geometry

`render::road_mesh::tests`:

* adjacent chunks share their boundary sample **identically** (the same table
  entry, asserted by equality), and their generated surfaces meet at the seam;
* every chunk on the course is finite, non-degenerate, and index-valid;
* every road triangle winds outward, and the stored normals agree with the
  winding — the check that catches the failure mode where a surface is
  invisible on the GPU and lit from underneath on Canvas2D;
* the road surface stays within the road's own width;
* tunnels get walls and a roof, open straights get no guardrail;
* lane dashes are spaced by **distance**, not by sample index.

`render::surface_builder::tests` proves the winding guarantee at the source: a
quad faces the direction it was asked to regardless of the corner order it was
given, and every face of a box points outward.

---

## 3. Car, camera, traffic, boost

Behavioural, not structural — each asserts the *design intent*:

* throttle accelerates; the car passes 90 km/h in one second and 250 km/h in
  five (`the_car_is_genuinely_quick_off_the_line`);
* braking beats coasting, and is forceful;
* zero input coasts down and never rolls backwards on its own;
* reverse engages from rest and is capped;
* steering turns the commanded way, and the **authority curve** is pinned
  directly (full at rest, half at `steer_falloff_speed`, monotone, never below
  the floor) *and* through the integrator (a gentle input turns the car more at
  low speed);
* a parked car cannot pirouette;
* the handbrake genuinely slides the car and starts a drift; an ordinary turn
  does not;
* a drift **converges** rather than spinning once the input is released;
* drift state has hysteresis and does not chatter;
* boost pulls harder and exceeds the natural top speed; an empty meter refuses;
* boost cannot re-engage under a held key after running dry;
* off-road costs speed;
* barrier impacts cost speed without stopping the demo, and the car can be
  **driven out of a wall** it has been ground against at full lock for four
  seconds — the scrape-alignment property;
* a shallow contact classifies as `Scrape` however fast it was, and a gentle one
  does too; an ordinary rear-end or side impact is a `Bump`; a fast square hit,
  or ploughing into something barely moving, is a `MajorCrash`;
* each severity retains at least its floor (95% / 85% / 65%) of the pre-impact
  forward speed, measured at closing speeds well past the loss reference so every
  band takes its full cut;
* a guardrail and a tunnel wall classify differently under the *identical* hit —
  a rail gives, rock does not;
* a rear-ender never leaves the player slower than the car in front;
* near misses need closeness *and* closing speed *and* no contact;
* the field of view rises with speed, widens further on boost, stays inside its
  band, and **never snaps** (asserted step-by-step through 900 steps of slamming
  between full boost and full braking);
* the chase distance holds at racing speed — the regression test for the
  velocity feed-forward, without which the camera settles 16 m too far back;
* the position spring converges without overshooting;
* the camera follows travel rather than the nose in a drift, isolated by running
  the *same* drift through two cameras differing only in the blend;
* camera roll stays inside its limit at full lock;
* the camera never drops below the road, over 4 000 steps;
* traffic stays on its lane path over 9 000 steps;
* a traffic slot regenerates identically after the pool recycled through >100
  slots.

### Input

`controls::tests` and `touch::tests` cover the whole input path natively:

* both the letter keys and the arrow keys drive; opposite steering keys cancel;
* edge-triggered actions (reset, pause, restart, debug) fire once per *press*,
  not once per frame;
* a gamepad trigger drives with no keys held, and where a key and a stick
  disagree the one asking for more wins;
* analogue input is clamped and never non-finite;
* the fold is a pure function of the key sequence;
* **a key released while Shift is held does not stay stuck** — the regression
  test for the bug where `code` and `key` were both recorded and only one of
  them ever matched its release, leaving the car steering forever with no reset
  able to clear it;
* a key is tracked by its physical `code`, not the character it produced;
* a synthetic `key`-only event (an on-screen keypad) still presses and releases;
* auto-repeat cannot double-register a key;
* clearing (focus loss) releases everything and the car is asked for nothing;
* the on-screen pad lays out inside any viewport from 320×240 up, with every
  button clearing a 44 px thumb target and no two buttons overlapping;
* the accelerator is the largest button;
* several pad buttons can be held at once, and a second finger on an already-held
  button does not release it when it lifts;
* the joystick appears where the finger lands, steers both ways, clamps to its
  ring rather than running away, has a deadzone that does not cost full lock,
  and recentres when released;
* only one joystick exists at a time, and a press on a button never starts one;
* rotating the device re-lays out and drops everything held.

**`steering_turns_the_car_in_the_direction_the_player_sees`** is the important
one. It derives screen-right the way `Mat4::look_at` does (`forward × up`) and
asserts the car moves that way — because the engine puts world `+X` on the
*left* of the screen, so a test written against world `+X` passes happily with
the controls inverted.

---

## 4. Long-running stability

Three separate long runs, each asserting **state**, not merely absence of a
panic:

| Test | Length | Asserts every step |
|---|---|---|
| `sim::tests::a_long_scripted_run_stays_finite_and_inside_the_world` | 36 000 steps (10 min) through 7 rotating behaviours | finite car state; distance inside the course; lateral inside the barriers; speed ≤ 200 m/s; boost in `0..1`; camera pose finite |
| `script::tests::the_canned_run_is_stable_over_several_minutes` | 14 400 steps (4 min) | finite; on-course; boost in range; *and* that the run actually drifted, actually hit something, and actually went fast |
| `sim::collision::tests::repeated_wall_contact_stays_stable_over_a_long_run` | 6 000 steps of deliberate wall-grinding | finite every step; ends inside the barriers |

The scripted run (`script::Stage`) is the "representative sequence" of the
brief: acceleration → cruise → braking → drift → boost → impact → reset, on
repeat. It asserts the stages *had their effect*, so a stage that silently
stopped working fails the test rather than passing quietly.

`script::tests::the_autopilot_holds_the_road_for_the_whole_course` drives the
entire nine kilometres to the finish line and asserts it arrived, stayed finite,
and threaded traffic on the way.

### Deliberate mistakes

`script::deliberate_excursion` drives the car off the road **on purpose** at
racing speed, then hands it back to the autopilot and measures what happened:
how far off it got, whether it reached the barrier, the lowest speed it fell to,
how long the recovery took, and whether the stuck detector ever offered a reset.

Three tests use it: an ordinary excursion both ways, a badly botched one (full
lock held for four seconds, which buries the car against the barrier), and a
replay check. They assert the car **always comes back without a reset**, in under
twelve seconds, still moving — and, separately, that the mistake genuinely *cost*
something, so a future tuning pass that makes running wide free fails here.

Measured on the shipping course, entering at 91 m/s:

| Mistake | Worst | Slowest | Recovered in | Speed after |
|---|---|---|---|---|
| Full lock, 1.5 s | 10.0 m past the edge, barrier | 35.7 m/s | 2.05 s | 47.8 m/s |
| Full lock, 4.0 s | 10.0 m past the edge, barrier | 26.0 m/s | 2.08 s | 39.5 m/s |
| Full lock left, 2 s | 10.0 m past the edge, barrier | 29.5 m/s | 2.15 s | 42.5 m/s |

`script::deliberate_collision` does the same for traffic: it chases whichever car
is next ahead and drives into the back of it — a pursuit, not a teleport into an
overlap, so it measures the game rather than the collision resolver. It reports
the closing speed, the impact strength, the speed either side of the hit, how
far the shunt swung the nose, and the recovery.

Three tests use it, asserting that contact is never free, never costs more than
the reported severity's retained-momentum floor, never spins the car, never needs
a reset, and always ends with the car driving again — plus that a shunt can never
leave the player slower than the car it hit.

Measured on the shipping course, entering at ~90 m/s:

| Severity | Closing | Strength | Speed | Lost | Yaw kick | Recovered |
|---|---|---|---|---|---|---|
| `Scrape` | 64.8 m/s | 0.05 | 91.4 → 90.9 | 1% | 0.03 rad | 0.50 s |
| `Scrape` | 64.1 m/s | 0.00 | 88.4 → 88.6 | 0% | 0.06 rad | 0.50 s |
| `MajorCrash` | 54.2 m/s | 0.59 | 91.4 → 59.6 | 35% | 0.04 rad | 0.50 s |
| `MajorCrash` | 65.0 m/s | 0.72 | 89.8 → 58.9 | 34% | 0.02 rad | 0.50 s |
| `MajorCrash` | 62.3 m/s | 0.71 | 87.2 → 56.9 | 35% | 0.08 rad | 0.50 s |

The bands are deliberately far apart, and each is pinned at its floor: brushing
past costs a percent or nothing at all, while squaring up the back of a much
slower car at 60 m/s of closing speed costs exactly the 35% a `MajorCrash` is
capped at and not a point more. Neither ever spins the car (the yaw kick is
two orders of magnitude below the 1.0 rad spin threshold) or ends the run.

A pursuit at full speed produces only these two outcomes, which is correct: a
`Bump` is what an *ordinary* closing speed gives, and the harness deliberately
never brakes. The `Bump` band is exercised directly by the staged scenarios in
`sim::tests`.

### Contact episodes — the regression that motivated all of it

A collision used to be a **state** rather than an event: the full response fired
once per traffic car per fixed step for as long as the boxes overlapped. Three
groups of tests pin the fix, at three different altitudes:

| Level | Test | Proves |
|---|---|---|
| Resolver | `sim::contact::tests::a_sustained_overlap_does_not_compound_the_speed_loss` | Half a second of continuous contact takes momentum once |
| Resolver | `sim::contact::tests::the_same_vehicle_cannot_trigger_a_second_full_impact_during_the_cooldown` | The cooldown holds for its whole length, then releases |
| Resolver | `sim::contact::tests::a_different_vehicle_can_still_be_hit_during_the_cooldown` | And never makes the player intangible |
| Resolver | `sim::contact::tests::several_contacts_in_one_step_clamp_against_a_single_baseline` | Four cars hit at once still leave 85% of the speed |
| Pipeline | `sim::tests::sustained_side_by_side_contact_costs_its_momentum_once` | Two seconds of leaning on a car, measured against a coasting control, stays inside the scrape floor and never escalates past `Scrape` |
| Pipeline | `sim::tests::a_grind_is_rate_limited_in_sound_and_kicks_the_camera_once_per_episode` | A grind is audible and continuous but far from one cue per step, and the camera is armed strictly more rarely still |
| Pipeline | `sim::collision::tests::grinding_a_barrier_does_not_take_speed_every_step` | The barrier half of the same bug, through the real sub-move loop |
| Camera | `camera::tests::one_impulse_decays_and_is_never_re_armed_by_a_lingering_impact_state` | 120 steps of held impact state produce a monotonically decaying kick |

Separation, yielding and recovery each have their own group:

| Test | Proves |
|---|---|
| `sim::collision::tests::separation_reduces_penetration_step_after_step_until_the_pair_is_clear` | Penetration falls strictly monotonically and the pair genuinely comes apart |
| `sim::collision::tests::separation_never_teleports_either_body_or_lifts_them_off_the_road` | Every move is inside `separation_step`, no vertical impulse, both yields bounded — under a pathologically deep overlap |
| `sim::collision::tests::a_rear_end_biases_the_player_sideways_as_well_as_back` | A shunt pushes the player *round* the obstacle, not only back from it |
| `sim::traffic::tests::a_traffic_car_yields_sideways_but_only_within_its_budget` | Fifty shoves stop at the budget, and the car returns to its lane exactly |
| `sim::controller::tests::recovery_never_overrides_the_players_steering` | Full lock still points the car both ways under the assist, with most of its authority intact |
| `sim::controller::tests::recovery_acceleration_helps_under_throttle_and_fades_away` | The assist is a bounded fraction of the throttle and fades monotonically to nothing |
| `sim::contact::tests::stabilisation_stops_early_when_steady_but_the_throttle_help_does_not` | The two halves of recovery have different lifetimes, on purpose |
| `sim::tests::a_collision_neither_awards_nor_consumes_boost` | Measured against a control, because the meter is always moving |
| `sim::tests::the_player_keeps_every_control_through_every_severity` | Throttle, steering, brake, handbrake and boost all bite on the next step, after all three severities |

And traffic fairness:

| Test | Proves |
|---|---|
| `sim::traffic::tests::recycled_traffic_never_spawns_inside_the_player_safety_region` | Swept across a whole slot pitch, so every phase relationship is exercised |
| `sim::traffic::tests::traffic_never_appears_inside_the_safety_region_across_repeated_jumps` | Forty teleports, checking only the *first sighting* of each slot — a car may drive into the region, it may never be created there |
| `sim::traffic::tests::traffic_never_blocks_the_road_across_the_whole_generation_range` | Over 10 000 cross-sections along the entire nine kilometres, some lane centre is always clear |

---

## 5. Scripted capture

`capture.rs` registers eight deterministic slices in `tools/axiom-shot`:

```sh
cargo run -p axiom-shot -- --app burnt-rubber
cargo run -p axiom-shot -- --app burnt-rubber-straight
cargo run -p axiom-shot -- --app burnt-rubber-sweeping-turn
cargo run -p axiom-shot -- --app burnt-rubber-drift
cargo run -p axiom-shot -- --app burnt-rubber-tunnel
cargo run -p axiom-shot -- --app burnt-rubber-traffic
cargo run -p axiom-shot -- --app burnt-rubber-boost
cargo run -p axiom-shot -- --app burnt-rubber-start-line
```

Each slice is built by placing the car at a known point on the course, launching
it at a known speed, and running a known number of fixed steps under a known
command — no browser, no clock, no input.

`every_slice_renders_identically_twice` builds each slice **twice from scratch**
and compares the draw count, the camera matrix, the clear colour and the full
instance-float buffer. That is byte-identity on the deterministic path; the GPU
path is compared under the repository's established image tolerance.

Two slices additionally assert they show what they claim:
`the_drift_slice_is_actually_drifting` and `the_boost_slice_is_actually_boosting`.

---

## 6. Browser playtesting

```sh
uv run scripts/localhost_servers.py start-app burnt-rubber --port 8085
uv run scripts/localhost_servers.py logs burnt-rubber -n 20    # confirm it compiled
uv run scripts/playwright_controller.py goto http://localhost:8086/
uv run scripts/playwright_controller.py wait 2500
uv run scripts/playwright_controller.py console                 # must be error-free
uv run scripts/playwright_controller.py screenshot burnt-rubber
```

To drive it from the controller (the page listens on `window`):

```sh
uv run scripts/playwright_controller.py eval "window.dispatchEvent(new KeyboardEvent('keydown',{code:'KeyW',key:'w'}))"
uv run scripts/playwright_controller.py eval "window.dispatchEvent(new KeyboardEvent('keydown',{code:'ShiftLeft',key:'Shift'}))"
uv run scripts/playwright_controller.py eval "window.dispatchEvent(new KeyboardEvent('keyup',{code:'KeyW',key:'w'}))"
```

`src/web.rs` is the only file the native suite does not reach. That is the point
of how it is built: it captures keys, reads one clock, drives the windowing
loop, realizes the audio batch and writes the DOM HUD — and every *decision* it
makes it delegates to `Controls`, `BurntRubber` and `HudModel`, all of which are
covered natively. Nothing there is allowed to be interesting.

---

## 7. Performance diagnostics

Turn on the in-game overlay with **F1**, or read the counters directly:

```rust
let d = app.diagnostics();
d.scene.active_chunks;        // ≤ CHUNKS_AHEAD + CHUNKS_BEHIND + 1 = 17
d.scene.road_triangles;       // whole-course total
d.scene.scenery_instances;    // drawn this frame, bounded by the pools
d.scene.effect_instances;
d.active_traffic;
d.simulation_steps;
```

`Diagnostics::rows()` is the ordered `(label, value)` list the overlay renders;
the order is asserted stable so the overlay never reflows.

Two tests hold the performance shape:

* `render::tests::the_drawn_set_stays_bounded_across_the_whole_course` drives
  the autopilot for 6 000 steps and asserts the active chunk count never exceeds
  the window and the scenery instance count never exceeds 1 400;
* `render::scenery::tests::the_pool_capacities_cover_what_the_active_range_generates`
  walks **every** window of consecutive chunks on the course and asserts the
  worst-case per-kind instance count fits its pool — so the pool ceilings are
  proven sufficient rather than hoped to be.

`diagnostics::tests::observing_does_not_disturb_the_simulation` pins the rule
that collecting telemetry cannot change the frame.

---

## 8. Tuning values

Every authored number lives in `src/tuning.rs`, in four records:
`VehicleTuning`, `CameraTuning`, `CourseTuning`, `RaceTuning`. A tuning pass is
an edit to that one file.

The relationships between them are themselves tested
(`tuning::tests`): braking is more forceful than the throttle, reverse is
limited, the handbrake and the dirt both reduce grip, drift state has
hysteresis, the field-of-view band is ordered and bounded, the course
constraints are self-consistent, and boost drains faster than it passively
trickles in — so a tuning edit that breaks the *design* fails the build, not
just one that breaks the code.

A handful of constants live next to the code they belong to rather than in
`tuning.rs` — the road's paint geometry (`render/road_mesh.rs`), the prop
capacities and draw distances (`render/scenery.rs`), the effect pool sizes
(`render/effects.rs`), the audio grain rate (`audio_cues.rs`) and the autopilot's
gains (`script.rs`). Those are *structural* numbers rather than feel numbers:
changing them changes what exists, not how it drives.
