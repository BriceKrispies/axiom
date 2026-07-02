---
name: visual-convergence
description: Converge any Axiom app/game's rendered output toward a reference screenshot via a disciplined champion/candidate loop. Use when the user gives you a reference image and wants a real app's render to match it. The skill harnesses the actual app (axiom-shot / agent bin / visual-target / Playwright) to capture a real screenshot, then iterates one bounded, scored nudge at a time.
---

# visual-convergence

Drive an Axiom app/game's **real rendered output** toward a **reference image**, one
disciplined, scored change at a time. This is not "make it look better" — it is an
axis-by-axis, keep-if-better, fully-audited convergence loop with an abstraction gate.

The novel part is the **harness**: you must run the *actual* app (not a mock) and capture a
deterministic screenshot of it, then compare that real render against the reference. Most of
the work is figuring out how to put the target app into a harness.

## Inputs

- **A reference image** (the user provides it, or points at a file).
- **A target app/game** — a name (`retro_fps`, `growth`, `forest_walk`, `soccer-penalty`, …) or a
  path. If the user only gives a screenshot, ask which app it targets and what framing/moment
  it should represent (camera pose, tick, game state).

Work in a git repo (or worktree). Commit per kept iteration. Never claim reference parity —
see Scoring.

## Step 1 — Harness the app (capture a real screenshot deterministically)

Pick the **cheapest capture path that runs the real app**. Decision order:

1. **`tools/axiom-shot` — native, offscreen, deterministic (preferred).** Works if the app
   exposes `pub fn build_<name>() -> RunningApp`. The registry is a match in
   `tools/axiom-shot/src/main.rs` (~L259); wired today: `retro_fps`, `showcase`, `nova-roll`,
   `physics-crucible`. axiom-shot is **excluded from the workspace** — build it via
   `--manifest-path`:
   ```sh
   cargo run --manifest-path tools/axiom-shot/Cargo.toml --release -- \
     --app <name> --backend gpu|canvas2d --out <dir>/champion.png \
     [--tick N] [--script "ticks:forward=1;yaw=0.02"] [--pose "x,z,yaw,pitch"] [--quality 0..3]
   ```
   Camera is first-person (`--script` phases, `--pose` teleport = retro FPS-style). Prefer the
   backend the reference implies (GPU for lit/textured hero shots; canvas2d for the legible
   flat proxy).

2. **A native capture agent bin.** Some apps expose their own headless render:
   `growth-agent` (`shots gpu`, `summit gpu`, `run <script.toml> gpu`, `portrait`), the retro FPS
   agent, etc.:
   ```sh
   cargo run --manifest-path apps/axiom-gallery/Cargo.toml \
     --bin growth-agent --features growth-agent -- shots gpu
   ```

3. **A manifest-driven `visual-target` scene** (static diorama). If the target *is* a
   `visual_targets/<name>/manifest.toml`, use the built-in automation — it renders, scores,
   decides, and appends the ledger for you:
   ```sh
   cargo run --features visual-target --bin visual-target -- \
     render <scene.toml> --backend gpu|canvas2d --out <dir>/candidate.png
   cargo run --features visual-target --bin visual-target -- status <target-dir>   # scores + next flaw
   cargo run --features visual-target --bin visual-target -- attack <target-dir>   # names the axis
   cargo run --features visual-target --bin visual-target -- review <target-dir>   # decide + ledger + promote
   ```

4. **Playwright — live browser (wasm-only apps).** For apps that only render live via
   `axiom-windowing` (`forest_walk`, `zanzoban`, `quintet`, `stress_cubes`, `rotating_cube`,
   live `growth`):
   ```sh
   make gallery-fast                                             # build + serve at :8000
   uv run scripts/playwright_controller.py goto http://localhost:8000/<demo>/
   uv run scripts/playwright_controller.py wait 2000
   uv run scripts/playwright_controller.py console               # check for errors
   uv run scripts/playwright_controller.py screenshot <name>     # → prints a PNG path to Read
   ```
   (`AXIOM_PW_VIEWPORT="WxH"` fixes the viewport; `AXIOM_PW_HEADLESS=0` shows the window.)

5. **Not yet harnessable → wire the cheapest harness (this is real work, do it).** In order of
   preference:
   - Implement `pub fn build_<name>() -> RunningApp` in the app's module (see
     `apps/axiom-gallery/src/retro_fps/mod.rs`, `physics_crucible_app.rs` for the shape) and add a
     `"<name>" => axiom_gallery::<name>::build_<name>()` case to the axiom-shot registry. This
     gives a deterministic native screenshot — the best harness.
   - Or add a capture agent bin (feature-gated, offscreen GPU/canvas2d, PNG out), mirroring
     `growth/bin/agent.rs`.
   - Or, if it is fundamentally live/wasm, use Playwright and accept it is not byte-deterministic.
   Match the reference's **camera/framing/moment** (pose, tick, game state) — a convergence is
   meaningless if the two images frame different things.

**Verify determinism before trusting a screenshot:** render twice and diff. canvas2d must be
byte-identical; GPU must be within tolerance (mean ≤2, max ≤40). If a render is
non-deterministic where it shouldn't be, fix that first.

## Step 2 — Set up the convergence directory

Mirror the `visual_targets/<name>/` layout, app-agnostic:

```
<target-dir>/
  reference.png                # the user's target image
  champion.png                 # current best real screenshot (+ champion.gpu.png if two backends)
  candidate.png                # latest candidate real screenshot
  scorecard.champion.toml      # champion's axis scores (hand-authored)
  scorecard.candidate.toml     # candidate's axis scores
  ledger.toml                  # append-only [[iteration]] log (schema below)
  abstractions/NNNN.toml       # justified structural changes (abstraction gate)
  diagnostics/                 # per-iteration diff/compare artifacts
  manifest.toml                # ONLY for visual-target manifest scenes
```

The first real screenshot from Step 1 is the initial **champion**.

## Step 3 — Choose the axes (once per campaign, then keep them fixed)

Score on **8–12 axes** that capture what matters for this reference. General starter rubric —
adapt to the app (a 3D scene weights lighting/materials; a UI/game weights layout/readability):

`composition_and_framing`, `subject_fidelity`, `silhouette_readability`,
`material_and_texture_detail`, `lighting_and_shadow`, `color_palette`,
`contrast_and_exposure`, `depth_and_separation`, `atmosphere`, `scale_and_proportion`,
`detail_density`, `artifact_level`.

(The `prologue_postcard_001` forest target's 12 axes — `terrain_silhouette`,
`foreground_material_detail`, `vegetation_density`, `vegetation_clumping`, `depth_separation`,
`fog_and_haze`, `lighting_directionality`, `color_palette`, `contrast_and_exposure`,
`object_scale`, `horizon_composition`, `artifact_level` — are one instantiation.) Fix the axis
list + order for the whole campaign (order is the tie-break for "lowest axis").

## Step 4 — The convergence loop

Repeat until every axis ≥ 4, or the user accepts the champion:

1. **Score** the champion against the reference, `0..5` per axis, by eye.
   - **5 = indistinguishable from the reference to a human reviewer.** A stylized/low-poly
     render is *not* a 5. Do **not** claim parity until the user explicitly accepts. Be honest
     — an inflated scorecard corrupts the whole loop.
   - `final_score = lowest_axis * 0.7 + average_axis * 0.3`. The **lowest axis is the next flaw
     to attack** (ties broken by fixed axis order).

2. **Attack the lowest axis with ONE bounded nudge**, smallest-first up this ladder:
   `config/manifest → generation/data → backend/shader → new code/primitive`. Before editing,
   write the rationale:
   - *Attacked mismatch* — what about this axis differs from the reference.
   - *Why it's the most important flaw* — it's the lowest / dominates final_score.
   - *The smallest nudge* — the one change you'll make.
   - *Why it's smaller than a new primitive* — you're staying low on the ladder.
   - *What would justify a primitive later* — the abstraction-gate trigger.

3. **Re-render the candidate** through the *same harness* (Step 1) and **re-score it against
   the reference** (candidate vs reference, not vs champion).

4. **Decide** (significant drop = a non-attacked axis falling ≥ 2):
   | Decision | When | Champion |
   |---|---|---|
   | `keep_candidate` | attacked axis improved, no non-attacked drop | replaced |
   | `keep_candidate_mark_regression` | attacked axis improved, a non-attacked axis slipped 1 | replaced, flagged |
   | `reject_candidate` | attacked axis improved but a non-attacked axis dropped ≥ 2 | kept |
   | `start_new_candidate_branch` | attacked axis did **not** improve | kept; abandon this line |

5. **Ledger** — append one `[[iteration]]`; promote candidate→champion on a keep (overwrite
   `champion.*` + `scorecard.champion.toml`); **commit** the kept iteration.

6. **Stop after one candidate for review by default** — report the scorecard, decision, and
   reason, and wait — unless the user said to keep pushing.

### Ledger schema (`ledger.toml`)
```toml
[[iteration]]
iteration = 12
attacked_axis = "material_and_texture_detail"
changed_files = ["<dir>/manifest.toml"]        # or the app source files touched
champion_screenshot = "champion.png"
candidate_screenshot = "candidate.png"
decision = "keep_candidate"                      # one of the four above
reason = "material_and_texture_detail 2->3 (+1); no non-attacked axis dropped; promoted"
next_attacked_axis = "lighting_and_shadow"
abstraction_introduced = false

[iteration.scorecard_before]   # all axes, 0..5
material_and_texture_detail = 2
# ...
[iteration.scorecard_after]
material_and_texture_detail = 3
# ...
```
(For a `visual-target` manifest scene, `visual-target review` writes this for you.)

## Abstraction gate

A **new primitive / structural change** (a new manifest field, a new engine capability, a
shader/material feature) is allowed **only** after either:
- the same axis has failed **≥ 3** bounded (config/data) attempts, recorded in the ledger, or
- the current implementation genuinely **cannot express** the needed change.

Justify it in `abstractions/NNNN.toml`:
```toml
axis = "material_and_texture_detail"
failed_attempts = [7, 9, 10]        # ledger iterations, or [] if inexpressible
inexpressible = false
smallest_api = "the minimal new surface (a manifest field / a value type / one backend hook)"
screenshot_command = "the exact command that renders it"
screenshot_proof = "rendered twice byte-identical: md5 <hash>"
```
Fix problems at the **lowest correct layer** and keep the new surface minimal — this is the
same No-Shortcuts discipline as the engine itself.

## Gates (when a change touches engine spine)

Most nudges are app-tier (manifest/generation) — no gate. But if the smallest correct fix is
in a **layer or module** (`crates/*`, `modules/*`), it must ship green:
- `bash scripts/coverage.sh` = **100%** (note: the wgpu render files —
  `scene_renderer.rs`/`offscreen.rs`/`live_gpu_binding.rs` — are *not* coverage-instrumented;
  they are GPU-verified by rendering, like the Playwright path).
- `cargo run -p xtask -- check-architecture` clean.
- `cargo dylint --all -- --all-targets` — engine-lint counts at/under the
  `.git/hooks/dylint-baseline.txt` baseline. Watch the hard caps: `engine_no_large_files=0`
  (1000 lines/file), `engine_no_large_functions=2` (120 lines/fn) — split files / extract
  helpers rather than tripping them. Don't rely on `--no-verify`; it hides these.

## Notes / gotchas

- **Canvas 2D is flat-shaded and ignores textures/normal maps** — texture + normal-map
  richness is GPU-only; canvas2d keeps a legible flat proxy (per the capability system). Score
  the backend the reference implies.
- **Match the moment.** For a game, the screenshot must capture the same camera/tick/state the
  reference shows, or the axes are comparing different things.
- **Reference-derived composites** (side-by-side comparison images you build for review) are
  scratch — send them to the user, don't commit them.
- Related memory: `gpu-fidelity-and-capability-system`, `visual-convergence-comparator`.
