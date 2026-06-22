# axiom-sim-crucible — Testing

Run with `cargo test -p axiom-sim-crucible`. Tests split into behavioral
(`tests/crucible.rs`) and architecture (`tests/architecture.rs`).

## Deterministic run expectations

Fixed logical ticks: **contact at tick 2, grooming at tick 5, final at tick 8.**
Fixed amounts: source starts at **10**; contact moves **4** onto the paw; grooming
moves **3** from paw to mouth. Final state: source **6**, paw **1**, mouth **3**
(quantity conserved), intoxication fact **set**, "groomed" fact **set**. The
causal journal holds **11** events in a fixed order.

## Scenario tests (`tests/crucible.rs`)

- `scenario_initializes_deterministically` — two fresh worlds agree (all zero).
- `body_plan_and_surfaces_exist` — the captured extremity and mouth surfaces
  resolve (build would have panicked otherwise).
- `source_residue_exists_at_initial_location` — the tavern-cell holds 10.
- `scenario_actions_run_at_deterministic_ticks` — the schedule is
  `[Process@0, SurfaceTransfer@2, SurfaceTransfer@5, EffectApplication@5]`.
- `scenario_schedule_is_deterministic` — two builds produce equal action lists.
- `contact_transfers_residue_to_the_extremity_surface` — at tick 2: paw 4,
  mouth 0, source 6, grooming not yet woken.

## Causal-chain tests

- `grooming_process_wakes_at_the_expected_tick` — woke tick is `None` before
  tick 5, `Some(5)` after.
- `grooming_produces_effects_rather_than_mutating_directly` — the "groomed" fact
  appears, and `process-produced-effects` + `effects-applied` events prove the
  grooming went through the effect boundary.
- `residue_transfers_from_extremity_to_mouth` — mouth 3, paw 1, conserved.
- `ingestion_entry_interaction_is_recorded` — an ingestion event on the ingestion
  route at tick 5.
- `final_creature_fact_changes_through_a_generic_effect_rule` — intoxication
  active; the effect event carries the substance + ingestion route.
- `causal_journal_contains_the_full_parent_child_chain` — contact is
  command-caused; grooming-transfer/ingestion/effect are process-caused; every
  event is attributed (no `Unknown` parents).
- `causal_event_order_is_the_expected_canonical_chain` — pins the exact ordered
  11-event sequence (scheduled → contact ×2 → woke/started/produced-effects/
  effects-applied/completed → ingestion → groom-transfer → intoxication-effect).
  Any intentional change to the chain must update this list.

## Replay / digest tests

- `repeated_run_produces_identical_digest_and_causal_order` — `replay::verify()`
  runs the scenario twice and asserts identical structural digest, identical
  causal-event order (and identical rows), and identical fact/residue state.
- `same_scenario_actions_replay_to_the_same_digest` — two runs share the same
  action list, final structural digest, and causal-chain digest.
- `app_runs_headlessly_and_reports` — `run_report()` yields the scenario name,
  the `intoxicated=true` outcome, the replay `PASS`, and the digest line.

## Architecture tests (`tests/architecture.rs`)

- `app_toml_exists_and_lists_only_consumed_layers_and_modules` — lists `ecs` +
  `sim-core`, and no browser/render/scene modules.
- `no_browser_gpu_or_dom`, `no_wall_clock_or_randomness`, `no_placeholder_macros`,
  `no_junk_drawer_modules` — app source hygiene.
- `no_illegal_substrate_imports` — the app imports only `axiom-ecs` /
  `axiom-sim-core`.
- `no_phase_milestone_naming_in_structure` — no `phaseN` file or identifier names.
- `tick_loop_has_no_hardcoded_consequence_branches` — the driver's `tick` fn does
  not branch on a scenario tick or inline the contact/grooming consequence; it
  dispatches due actions from the schedule.
- `scenario_domain_names_do_not_leak_into_reusable_substrate` — `beer`/`tavern`
  are absent from `axiom-sim-core`/`axiom-ecs` source (proving no special-case was
  baked into the substrate).
- `no_layer_or_module_depends_on_this_app` — nothing imports the crucible.

## Replay/snapshot expectations

sim-core's full byte-snapshot/replay seam is deferred (sim-core's deferred-features
note); the crucible proves determinism by deterministic re-run comparison of actual
state, not by a byte snapshot.

## What failure would mean for the substrate

- A behavioral failure means a substrate primitive (residue/transfer/route/effect/
  scheduler/causal) does not compose as documented — the generic mechanisms cannot
  express the chain.
- A digest/order failure means the substrate is **non-deterministic** (unordered
  iteration, hidden state, or wall-clock/randomness) — a violation of the core
  determinism invariant.
- A leak failure means domain logic crept into the reusable substrate — the
  substrate is no longer generic.
