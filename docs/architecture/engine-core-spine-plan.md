# Engine-Core Spine Plan

The reconciliation plan that accompanies
[`engine-core-spine-audit.md`](engine-core-spine-audit.md). Because the audit
found the spine already in place and enforced, this "plan" is the record of the
single smallest structurally-correct change made to close the one real gap —
skeletal animation — plus what was deliberately left alone.

## 1. Target structure

```
crates/   axiom-kernel  axiom-runtime  axiom-math  axiom-frame  axiom-host   (+ more layers)
modules/  axiom-scene  axiom-resources  axiom-render  axiom-animation
          axiom-physics  axiom-input  axiom-audio                            (+ more modules)
apps/     demos, harnesses, hosts (leaf composition roots)
games/    cartridges a host app loads
tools/    repo tooling, off the runtime graph
```

The mental model behind this layout is documented in
[`mechanism-vs-meaning.md`](mechanism-vs-meaning.md): kernel = deterministic
truth, layers = ordered engine spine, modules = reusable mechanisms, apps/games
= meaning.

## 2. Exact crates/modules added

- **`modules/axiom-animation`** — a new isolated engine module. Deterministic
  skeletal-animation mechanism: `AnimationApi` facade plus `SkeletonId` /
  `ClipId` / `BoneId`; behind the facade `Skeleton`/`Bone`, `Pose`/`ModelPose`,
  `AnimationClip`/`Keyframe`/`Track`, `JointLimit`, `ClipEvent`/`ClipPhase` —
  covering clip sampling, pose blending, anatomical joint-limit clamping,
  opaque-coded clip events/phases, and forward kinematics.
  `allowed_layers = ["kernel", "math"]`, `allowed_modules = []`. Branchless,
  100% covered, with `module.toml`, `ARCHITECTURE.md`, `TESTING.md`, and a
  `tests/architecture.rs` boundary scan. Added to the workspace `members`.

  This module is the reconciliation of two earlier drafts (a minimal
  handle-registry contract and a richer Euler-DOF toolkit from an unmerged
  branch): it keeps the strict facade shape, kernel-`Tick` timing, error codes,
  and pose blending, and grafts the joint-limit/event/phase/FK mechanisms — with
  all *content* (the humanoid, the kick) moved out to the app below.

- **`apps/axiom-animation-lab`** — a new app (composition leaf) that authors an
  18-bone humanoid and a right-foot soccer `kick_right` clip **entirely through
  `AnimationApi`**, maps opaque event/phase codes back to named concepts
  (`KickPhase`, `KickContact`), and renders the kick as side-view SVG stick
  figures. This is where the *meaning* lives; the module never names "kick".
  `allowed_layers = ["kernel", "math"]`, `allowed_modules = ["animation"]`.

## 3. Exact crates/modules modified

- **`crates/axiom-math`** — added `Quat::nlerp` (branchless shortest-arc
  normalized-lerp, for blending) and the `Quat::from_euler_xyz` /
  `Quat::to_euler_xyz` pair (for joint-limit clamping), plus their unit tests.
  These rotation primitives belong in the math *layer* (the owner of rotation
  maths), not in the module. Math keeps 100% coverage.
- **`Cargo.toml`** (workspace root) — one line adding
  `"modules/axiom-animation"` to `members`.

Nothing else in any existing crate, module, app, or tool was changed.

## 4. Exact crates/modules intentionally left alone

- **The five target layers** and the whole layer DAG — already correct.
- **The six pre-existing core modules** (`scene`, `resources`, `render`,
  `physics`, `input`, `audio`) — already isolated engine modules that own their
  mechanisms. Their facade/type names (`InputState`, `ResourceId`,
  `SceneNodeId`, the `Physics*` handle model) differ cosmetically from the
  target's suggested names; renaming them would ripple into every dependent app
  for **zero** structural benefit, so they were **not** touched.
- **The content-leaning modules** (`forest`, `biome`, `planetgen`, `levelgen`,
  `worldsave`, `world`) — left exactly as they are, by decision (see §5).
- **The architecture checker** (`crates/xtask`) — no code change needed; it
  already models the six-class taxonomy and auto-discovers the new module from
  its `module.toml`.

## 5. Game/domain modules needing future migration

By decision, this pass performs **no** relocation of the content-leaning
modules and writes **no** migration plan for them. They are legal today and are
merely noted in the audit (§6) as leaning app/content-ward. Whether any should
graduate toward `apps/`/`games/` is a separate future decision.

## 6. Architecture checker changes

None. The new module is enforced by the existing gates purely through its
manifests and source:

- classification + Module Law (isolation, single facade, allowed layers) via
  `cargo run -p xtask -- check-architecture`;
- source hygiene (no browser APIs, no console/placeholder macros, no junk
  drawers) via the same checker's `hygiene.rs`;
- branchless spine + genuine dependency + no-unwrap via the dylints;
- 100% coverage via `scripts/coverage.*`.

The module additionally self-checks the mechanism-vs-meaning boundary in its own
`tests/architecture.rs` (a gameplay/domain-noun denylist scoped to this module —
not a global scan, which would false-positive on the legitimately-content
`forest`/`biome` modules).

## 7. Test plan

- `cargo test -p axiom-animation` — 55 unit tests (facade, ordering/replay,
  every error arm, interpolation, joint limits, events/phases, FK) + 12 boundary
  tests.
- `cargo test -p axiom-animation-lab` — 8 app slice tests (rig authoring,
  per-frame FK, deterministic kick, contact-frame flag, SVG output).
- `cargo test -p axiom-math` — `Quat::nlerp` and the `from_euler_xyz` /
  `to_euler_xyz` round-trip tests.
- `cargo run -p xtask -- check-architecture` — classification + Module Law.
- `cargo dylint --all -- --all-targets` — branchless + genuine-dependency +
  no-unwrap (zero new warnings).
- `scripts/coverage.ps1` — 100% regions/lines/functions across the workspace,
  including the new module and the new math method.
- `cargo test --workspace` — nothing else regresses.

## 8. What this pass deliberately does not implement

- **A full animation engine.** No stateful `Animator` play-head and no
  `AnimationGraph`/`AnimationStateId` state machine — those are higher constructs
  that build on this contract (documented as deferred in the module's
  `ARCHITECTURE.md`).
- **Skeleton/animation-clip resource handles** in `axiom-resources` — animation
  is self-contained and does not need them; left as a future option.
- **Any reshaping** of the six existing core modules, or relocation of the
  content modules.
- **Anything in physics, rendering, assets, audio, or input** beyond what
  already exists — this pass is contracts and boundaries, not new subsystem
  implementation. None of those subsystems is claimed complete.
