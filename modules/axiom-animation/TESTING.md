# axiom-animation — Testing

The module ships at **100% region/line/function coverage** (the Axiom Coverage
Law), fully **branchless** in non-test code (the Branchless Law), and with the
architecture-boundary tests every core module carries. Tests are co-located as
`#[cfg(test)]` modules next to the code they exercise, plus one integration test
file for the cross-cutting boundary scans.

Run them with:

```sh
cargo test -p axiom-animation
```

## Public-facade tests (`animation_api.rs`)

Drive the whole module through `AnimationApi` exactly as an app would:

- monotonic id allocation (`create_skeleton` / `create_clip`);
- authoring a skeleton (`add_root_bone` / `add_child_bone`) and a clip
  (`add_track`), then `sample` → `resolve_model` composing a parent/child rig;
- `blend` of two sampled poses, including out-of-range factor clamping;
- `rest_pose` reads the bind transforms;
- `new` and `default` produce equivalent empty registries.

## Deterministic-ordering / replay tests

- Bone ids are assigned in strict insertion order (`skeleton.rs`).
- Sampling the same clip at the same tick twice is byte-equal (`clip.rs`,
  `animation_api.rs`) — the core replay invariant.
- Blending the same inputs twice is byte-equal (`blend.rs`).
- Clip sampling holds the endpoints outside the key range and interpolates
  inside it, and is exact **at** a keyframe (`track.rs`).

## Invalid-reference / error tests

Every deterministic error arm is provoked and asserted by its stable
`AnimationErrorCode`:

- `SkeletonNotFound` / `ClipNotFound` — every facade method that takes an id is
  called with a non-existent id (`animation_api.rs`).
- `BoneNotFound` — an out-of-range parent in `add_child_bone` and an out-of-range
  track bone in `sample` (`skeleton.rs`, `clip.rs`).
- `EmptyTrack` / `NonMonotonicKeyframes` — malformed track keyframes (`track.rs`).
- `PoseLengthMismatch` — blending / resolving mismatched-length poses
  (`blend.rs`, `pose.rs`).
- `NonFiniteInterpolation` — a zero-length keyframe rotation makes the shortest-
  arc `nlerp` fail; the module surfaces the wrapped `MathError` rather than
  panicking (`interpolate.rs`, `clip.rs`).

## Joint-limit / event / phase / FK tests

- **Joint limits** (`joint_limit.rs`, `animation_api.rs`) — an inverted
  (`min > max`) limit is rejected; an out-of-range rotation clamps back to the
  bound; a rotation already inside is left unchanged and reads `is_pose_legal`;
  translation/scale survive the clamp; a limit for an absent bone is ignored.
- **Events & phases** (`clip.rs`, `clip_event.rs`, `clip_phase.rs`,
  `animation_api.rs`) — events fire at their exact tick and report their opaque
  codes in order; a half-open phase span reports its code for covered ticks only;
  the facade rejects a missing clip id on every event/phase method.
- **Forward kinematics** (`pose.rs`) — `ModelPose::position` returns the
  model-space joint location, `None` out of range.

## Interpolation / math tests

- Endpoints are exact and the midpoint averages translation/scale
  (`interpolate.rs`).
- The supporting `Quat::nlerp` primitive is tested in `axiom-math`
  (`crates/axiom-math/src/quat.rs`): exact endpoints, half-rotation midpoint,
  shortest-arc hemisphere flip, and deterministic failure on a degenerate input.
- The `Quat::from_euler_xyz` / `Quat::to_euler_xyz` pair (used by joint-limit
  clamping) is tested there too: single-axis equivalence, X-then-Y-then-Z
  composition, round-trip away from gimbal, and the pitch clamp at the ±90° pole.

## Architecture-boundary tests (`tests/architecture.rs`)

A source scan (comments/strings stripped) that fails at `cargo test` time if the
module regresses on isolation:

- `module.toml` declares `allowed_modules = []`;
- `lib.rs` exports exactly one facade plus one `pub use ids::{…}` line;
- imports are limited to `axiom-kernel` / `axiom-math`; no sibling module is
  referenced; no layer imports `axiom-animation`;
- no browser/GPU/platform APIs, no wall-clock time, no randomness, no threads,
  no console/placeholder macros, no global mutable state, no hash/BTree/linked
  collections;
- no foreign engine-subsystem or external-engine names, and **no gameplay/domain
  vocabulary** — the mechanism-vs-meaning boundary, enforced;
- no junk-drawer module names; every `src/*.rs` is wired into `lib.rs`.

## Coverage / branchless notes

These global gates also cover this crate:

```sh
scripts/coverage.ps1                      # 100% regions/lines/functions
cargo dylint --all -- --all-targets       # engine_no_branching + genuine-dep + no-unwrap
cargo run -p xtask -- check-architecture   # Module Law (classification, isolation, one facade)
```
