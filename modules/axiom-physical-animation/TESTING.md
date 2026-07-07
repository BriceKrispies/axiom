# `axiom-physical-animation` — Testing

The bridge is held to the spine bar: **100% region/line/function coverage**, all
tests deterministic. Two layers of tests.

## Unit tests (per file, `#[cfg(test)]`)

- `humanoid_binding.rs` — the standard binding builds deterministically and
  completely (13 bodies; pelvis dynamic, limbs kinematic); the foot effectors
  address their foot bodies; a **too-small physics world** funnels a capacity
  failure into `PhysicsFailed`; a **missing plan** funnels into `AuthoringFailed`;
  the ball body is a dynamic sphere.
- `physical_animation_api.rs` — advancing before binding fails `NotBound`, before a
  ball fails `NoBall`; two identical runs produce byte-identical frames; the approach
  drives the pelvis `+Z` under physics; the plant holds the left-foot body at the
  plant spot; the strike applies a real ball impulse toward the net and drives harder
  than the backswing; a frame exposes gaze / effectors / contacts / events / step
  index; the recover drive is weaker than the strike; a missing plan on `attach_ball`
  fails through authoring.
- `physical_error{,_code}.rs` — stable numeric codes; error identity is the code
  alone (message excluded).

## Integration slice (`tests/penalty_physics_slice.rs`)

Drives only the public `PhysicalAnimationApi` + `AnimationAuthoringApi` facades
end-to-end over the authored penalty kick:

- the pose (kinematic) path and the physics path coexist;
- two identical penalty simulations yield the **same ball velocity after strike**
  (same-binary determinism);
- the strike drives the ball toward the net with a **real impulse** (velocity dot
  `net − ball` > 0, real speed — not a teleport);
- **stronger power → faster ball**;
- the plant phase holds the left-foot body at `left_plant_spot`;
- the follow-through swings the right foot past the ball;
- strike drive exceeds both the backswing and the recover drive.

`PhysicalAnimationFrame` is intentionally not nameable outside the crate (one
published facade), so every frame is held by inference from `advance`.

## Running

```sh
cargo test -p axiom-physical-animation                               # unit + slice
cargo xtask check-architecture                                       # Layer + Module Law
bash scripts/dylint-gate.sh                                          # Branchless Law (0 findings here)
bash scripts/coverage.sh                                             # 100% coverage gate
# Windows: the profiler runtime lives on the msvc toolchain, e.g.
#   cargo +nightly-...-msvc llvm-cov --branch -p axiom-physical-animation --summary-only
```

Coverage note: the enforced trio (regions/lines/functions) is **100%**. A handful
of llvm-cov `--branch` sub-conditions inside `&`-combined booleans read below 100%
in the branch column — regions already cover each arm, which is the gate's
documented branch-level proxy (llvm-cov has no branch threshold).
EOF
echo written