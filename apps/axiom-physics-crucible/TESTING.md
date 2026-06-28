# Axiom Physics Crucible — Testing

The crucible is an app (a composition leaf), so it is **exempt from the 100%
spine-coverage gate and the branchless law** — but it ships with the tests its
behaviour warrants. Every station is covered, every facade method the app calls is
exercised through a behavioural assertion, and determinism is proven, not assumed.

## Principles

- **No "does not panic" tests.** Every test asserts a concrete, observable physics
  outcome (a body settled at a height, a velocity reversed, a ray returned a
  specific handle, two runs produced equal state).
- **Drive only `PhysicsApi`.** Tests build a `CrucibleWorld` and step it; they never
  reach into physics internals (they cannot — the types are unnameable) and never
  add a backdoor.
- **Deterministic, scripted scenarios.** No randomness, no wall-clock time. Bodies
  are placed at fixed positions and driven by the fixed `RuntimeStep` sequence.
- **Replay is mandatory.** The determinism claim is tested directly: identical
  worlds stay byte-equal, and a deliberate perturbation is detected.

## What each station's tests prove

**Body Bay** (`body_bay.rs`)
- populates the full body-kind catalogue (5 bodies);
- the dynamic sphere falls and settles near its radius on the floor;
- the static box never moves; the kinematic box ignores gravity (stays exactly put);
- the disabled body holds its position despite gravity — *and* the same body, left
  enabled, falls (proving the disable is load-bearing, not a no-op);
- a capsule body can be created through the facade.

**Contact Bay** (`contact_bay.rs`)
- a settled world reports resolved contacts;
- the step record counts broad-phase and narrow-phase work, with broad ⊇ contacts;
- approaching contacts are actually solved (`solved_contact_count > 0`);
- a dropped sphere and a dropped box each *rest* on the plane instead of tunnelling;
- at least one contact normal is roughly vertical (faces off the floor).

**Material Bay** (`material_bay.rs`)
- an elastic sphere rebounds upward and an inelastic one does not;
- rebound speed orders with restitution (`0.9 > 0.5 > 0.0`);
- a heavier body gains less speed from the same impulse (`Δv ∝ 1/mass`, ~8× ratio);
- material validation rejects restitution `> 1`.

**Query Bay** (`query_bay.rs`)
- a raycast hits the target box; a raycast over the box misses;
- overlap-sphere finds the two bodies in range and excludes a distant one;
- a raycast passes *through* a trigger to the solid body behind it.

**Stress Bay** (`stress_bay.rs`)
- creates the pile + floor (17 bodies);
- the broad phase generates candidate pairs and the solver resolves contacts;
- no body tunnels the floor and all state stays finite;
- the same drop run twice is byte-identical (determinism under load).

**Replay Bay** (`replay_bay.rs`)
- the scripted sphere moves sideways (impulse) and falls (gravity);
- two identically-driven worlds stay in perfect sync;
- a replay perturbation is detected (the worlds diverge);
- the report reflects the replay match, and its digest is stable across two runs.

## Crate-level tests (`lib.rs`)

- all six stations are present in canonical order;
- `run_report()` is byte-reproducible and reports a replay match;
- the full crucible (all stations together) keeps both worlds in sync.

## Running

```sh
cargo test -p axiom-physics-crucible      # 64 tests
cargo run  -p axiom-physics-crucible      # the deterministic headless report
```

The rendered frame is validated separately via the `axiom-shot` screenshot tool
(`--app physics-crucible`), which proves the pre-simulated room actually paints
through the real GPU / Canvas 2D backends.
