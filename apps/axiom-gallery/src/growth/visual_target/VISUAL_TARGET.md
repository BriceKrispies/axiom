# Axiom Visual Target — format spec & convergence comparator

A **visual target** is one deterministic screenshot we are trying to make look
expensive, and a disciplined loop for getting there. It has two halves:

1. **The shot** — a fixed, versioned scene manifest that renders to one
   byte-reproducible PNG (`scene.rs` → `build.rs` → an off-screen backend).
2. **The comparator** — a review loop that scores each candidate on twelve visual
   axes, decides keep/reject from the scorecards, and records every iteration in a
   ledger. This is the machinery that replaces *"looks better"* with a defensible,
   axis-by-axis verdict.

This document is the spec for both. It is **app-tier tooling** in the gallery, not
engine spine: it reuses growth's headless render backends and the
`axiom-terrain-mesh` mesher, and it deliberately does **not** build an editor, a
biome system, a node graph, or a generalized world generator. The first
implementation is boring and local, on purpose.

Everything is gated behind the native-only `visual-target` cargo feature:

```sh
cargo run --features visual-target --bin visual-target -- <command> …
```

---

## 1. The deterministic shot

A scene is one TOML manifest (`scene::Manifest`, schema `version = 1`): a camera, a
directional sun, distance fog, a sloped terrain patch with height-banded ground
colour, and vegetation instances — a few authored `[[tree]]` entries plus an
optional deterministic, seeded `[scatter]` expansion. The whole diorama is a pure
function of that file. See `scenes/visual_target_001_autumn_forest.toml` for a
worked manifest and `scene.rs` for the field-by-field schema.

`build::build` turns a manifest into **neutral render data** (meshes, instanced
batches, lights, matrices — no GPU types), which either backend consumes:

- **Canvas 2D** (`--backend canvas2d`) — a pure software rasterizer. **Byte-exact**:
  the same manifest yields a bit-identical PNG on every machine. This is the backend
  the comparator uses, so scorecards always describe the same pixels.
- **GPU** (`--backend gpu`) — the engine's off-screen `wgpu` path (the shadowed, lit,
  instanced path the browser runs). Reproducible on the *same adapter*, compared
  within a tolerance across drivers.

The determinism boundary is honest: the mesh/instance/matrix data is bit-exact at
the `build` boundary; the Canvas 2D PNG is bit-exact; the GPU PNG is same-adapter
reproducible. `compare.rs` does the low-level pixel diff (mean/max/changed-fraction
against a tolerance) that underpins the reference blessing and the diff heatmaps.

Shot commands:

```sh
visual-target render  <scene.toml> [--backend gpu|canvas2d] [--out PATH]
visual-target bless    <scene.toml> [--backend …] [--out PATH]   # save as reference
visual-target compare  <scene.toml> <reference.png> [--tol MEAN,MAX] [--diff PATH]
```

---

## 2. The convergence comparator

### 2.1 The twelve axes

Every candidate is scored `0..=5` on each of these axes (`axes::Axis`, in this
canonical order — the order is the deterministic tie-break for "lowest axis"):

| # | axis | what it reads |
|---|------|---------------|
| 1 | `terrain_silhouette` | the terrain/canopy outline against sky and far fog |
| 2 | `foreground_material_detail` | close-up material richness of the foreground ground |
| 3 | `vegetation_density` | how much vegetation there is (sparse ↔ full) |
| 4 | `vegetation_clumping` | believable clumps vs. an even sprinkle |
| 5 | `depth_separation` | readable near / mid / far depth planes |
| 6 | `fog_and_haze` | atmospheric fog quality and distance falloff |
| 7 | `lighting_directionality` | whether the light reads as coming from one direction |
| 8 | `color_palette` | cohesion and appeal of the palette |
| 9 | `contrast_and_exposure` | tonal spread — neither crushed nor blown out |
| 10 | `object_scale` | objects at a believable real-world size |
| 11 | `horizon_composition` | horizon placement and overall framing |
| 12 | `artifact_level` | freedom from z-fighting, shadow acne, seams |

The scores are a **human/agent judgement** — the "visual scorecard" artifact,
authored as a flat TOML table keyed by the snake_case axis name. The comparator owns
only the arithmetic and the decisions over those scores; assigning them is the one
irreducibly human act in the loop.

### 2.2 The final score is lowest-dominated, not an average

```
final_score = lowest_axis_score * 0.7 + average_axis_score * 0.3
```

A plain average lets eleven good axes hide one broken one. This formula is dominated
by the *weakest* axis, so a scene cannot score well until its worst flaw is fixed.
The **lowest-scoring axis is the next flaw to attack** (ties broken by the canonical
axis order), unless a human verdict overrides it.

### 2.3 One attacked axis per iteration

Each iteration attacks **exactly one** axis. The agent makes **one bounded change**
(edit `manifest.candidate.toml`) aimed at that axis, re-renders the same
deterministic shot, authors the candidate scorecard, and the comparator compares
champion vs candidate.

### 2.4 The decision (`review::decide`)

> A candidate may replace the champion **only if** it improves the attacked axis
> **and** does not significantly damage any other axis. A regression is
> *significant* if any non-attacked axis drops by **2 or more** points.

That yields exactly four outcomes:

| decision | when | champion |
|----------|------|----------|
| `keep_candidate` | attacked axis improved, no non-attacked axis dropped | **replaced** |
| `keep_candidate_mark_regression` | attacked axis improved, a non-attacked axis slipped by 1 (minor) | **replaced**, flagged |
| `reject_candidate` | attacked axis improved but a non-attacked axis dropped ≥ 2 (significant) | kept |
| `start_new_candidate_branch` | attacked axis did **not** improve | kept; abandon the line |

### 2.5 Human verdict override

An optional `verdict.toml` (`review::HumanVerdict`) may, for one iteration:

- force the `decision`,
- force the next `attacked_axis`,
- `accept_champion` (the human half of completion),
- attach a `note` recorded in the ledger.

A consumed verdict is archived to `diagnostics/verdict.iterNNNN.toml` so it never
silently re-applies.

### 2.6 The iteration ledger

Every iteration appends one entry to `ledger.toml` (`ledger::LedgerEntry`) with:
iteration number · attacked axis · changed files · champion & candidate screenshot
paths · scorecard before · scorecard after · decision · reason · next attacked axis ·
whether an abstraction was introduced · optional human note.

### 2.7 Abstractions are forbidden by default

The loop is meant to be boring and local — one bounded change against the existing
implementation. A **new abstraction** (a new API / generalization) may be introduced
**only** when (`abstraction::permit`):

1. the same axis has failed to improve after **≥ 3** candidate attempts, **or**
2. the current implementation genuinely cannot express the needed visual change.

When introduced, every `abstraction::AbstractionRecord` must state: the visual axis
it unlocks · the specific failed attempts that justified it · the smallest API the
next candidate needs · proof that the deterministic screenshot command still works.
The record type **cannot be constructed** from a forbidden permission, so an
unjustified abstraction cannot even be represented.

### 2.8 Completion

The target is **not** complete until **every axis scores ≥ 4**, or a human explicitly
accepts the champion (`accept`). The agent may not declare it done otherwise.

---

## 3. The target directory

```text
manifest.toml              the CHAMPION scene (source of champion.png)
manifest.candidate.toml    the CANDIDATE scene (one bounded edit of the champion)
reference.manifest.toml    provenance for reference.png (rendered once)
reference.png              the target reference image (the goal)
champion.png               the current champion screenshot
candidate.png              the latest candidate screenshot
scorecard.champion.toml    the champion's twelve-axis scores
scorecard.candidate.toml   the candidate's twelve-axis scores
verdict.toml               optional human override for this iteration
ledger.toml                the append-only iteration ledger
diagnostics/               diff heatmaps + consumed verdicts, per iteration
abstractions/              justified abstraction records (0001.toml, …)
```

### The loop, command by command

```sh
D=visual_targets/prologue_postcard_001

visual-target status  $D          # champion scores, final score, next flaw
visual-target attack  $D          # names the one axis to target this iteration
#   … copy manifest.toml -> manifest.candidate.toml, make ONE bounded change …
visual-target render  $D/manifest.candidate.toml --backend canvas2d --out $D/candidate.png
#   … author scorecard.candidate.toml by eye (candidate vs reference) …
visual-target review  $D [--changed a,b] [--abstraction-introduced]
#   … repeat. When stuck on an axis after 3 tries: …
visual-target abstraction $D --api … --command … --proof … [--inexpressible]
visual-target accept  $D [--note …]   # human accepts the champion as complete
```

`review` decides, appends the ledger entry, promotes the candidate to champion on a
win (its manifest/PNG/scorecard overwrite the champion's), and writes
candidate-vs-champion and candidate-vs-reference diff heatmaps into `diagnostics/`.

---

## 4. `prologue_postcard_001` — the worked target

The seeded target is an autumn-forest prologue postcard. Its `reference.png` is a
dense, warm, raked-light slope; its iteration-0 champion is deliberately rough — a
sparse, gray, flatly-lit slope (`final_score 1.275`). The live ledger records the
convergence:

- **iter 1** — attacked `vegetation_density` (1→4) by raising the scatter from 30 to
  150 trees. Clean win, `keep_candidate`. Side benefit: `terrain_silhouette` 2→3.
- **iter 2** — attacked `lighting_directionality` (1→2) by swapping the dim overhead
  sun for a low raking warm sun. Modest win, `keep_candidate`. `final_score` → 1.400.

The next flaw is `color_palette` (still 1 — the canopy tints are muted gray). The
champion is mid-convergence and honestly **not** complete; the loop continues until
every axis clears 4 or a human accepts it.

The comparator's decision/regression/abstraction logic is proven exhaustively by the
unit tests in `axes.rs`, `review.rs`, `ledger.rs`, `abstraction.rs`, and `target.rs`
(`cargo test -p axiom-gallery --features visual-target growth::visual_target`).
