---
name: visual-convergence
description: Converge any Axiom app/game's rendered output toward a reference screenshot via a disciplined champion/candidate loop. Use when the user gives you a reference image and wants a real app's render to match it. The skill harnesses the actual app (axiom-shot / agent bin / visual-target / Playwright) to capture a real screenshot, then iterates one bounded, scored nudge at a time.
---

# visual-convergence

Drive an Axiom app/game's **real rendered output** toward a **reference image**, one
disciplined, scored change at a time. This is not "make it look better" — it is an
axis-by-axis, keep-if-better, fully-audited convergence loop with an abstraction gate.

**The target is parity: a render *identical* to the reference.** Not "close," not "reads
as the same thing," not "a nice stylized take." That goal governs two things this skill is
strict about — **score harshly** against identity (§Step 4.1), and **attack structurally**
(§Step 4.2). Convergence toward parity is won with structural changes — new geometry, a
real mesh, a material/shader feature, a re-cut data contract — far more often than with
cosmetic config tweaks. A config nudge that cannot in principle reach the reference (a boxy
proxy will never become a modeled subject by moving a number) is a shortcut; take the
structural fix at the lowest correct layer instead.

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
  campaign.toml                # app, substrate, judged arm, guarded arms, capture recipes, axis order
  reference.png                # the user's target image
  champion.png                 # current best real screenshot — always the JUDGED arm
  candidate.png                # latest candidate real screenshot
  <arm>-guard.png              # one per guarded arm (e.g. canvas-guard.png) — captured, never scored
  scorecard.champion.toml      # champion's axis scores (hand-authored)
  scorecard.candidate.toml     # candidate's axis scores
  ledger.toml                  # append-only [[iteration]] log (schema below)
  champions/                   # every champion that has LANDED on main (see Champion archive)
  abstractions/NNNN.toml       # justified structural changes (abstraction gate)
  diagnostics/                 # per-iteration diff/compare artifacts
  manifest.toml                # ONLY for visual-target manifest scenes
```

The first real screenshot from Step 1 is the initial **champion**.

`campaign.toml` is the machine-readable half of the campaign — what the foreman
reads in Step 1 instead of re-deriving the recipe from ledger prose. See
`visual_targets/burnt-rubber/campaign.toml` for a filled-in one.

## Judged renderer arm

Axiom drives every backend from **one full-richness frame** and lets each backend degrade
what it cannot do (`crates/axiom-host/src/frame_capability.rs`: the GPU arms take
`BackendCapabilityProfile::all()`, the Canvas 2D software rasterizer takes `canvas2d()`,
which drops `Textures`, `AlphaMask`, `NormalMapping`, `Sky`, `Specular`, `Bloom` and
substitutes a planar contact shadow for the PCF one). So the same app has more than one
render, and **a reference was shot on exactly one of them.**

A campaign therefore declares, in `campaign.toml`:

```toml
[arms]
judged = "gpu"                        # the arm every score, lens and decision is aimed at
guard  = ["canvas2d"]                 # captured every pass, shown to the human, NEVER scored
guard_rule = "legibility, not parity"
```

- **The judged arm is the only arm that is scored.** `champion.png`,
  `scorecard.*.toml` and every ledger score describe it. Pick the arm the reference
  was actually shot on; if you don't know, ask.
- **A guarded arm is captured, shown, and left alone.** It is not a target: a lens must
  never spend its one change making the guarded arm better, and must never trade
  judged-arm parity away to keep the guarded arm identical. It is allowed to drift.
  What it may not do is **break** — go black, error out, lose the subject, or stop being
  legible. That is a regression the foreman reports and the human decides on.
- **Most richness is arm-scoped for free.** A texture, a normal map, a sky gradient, a
  specular term or a bloom is capability-gated, so it lands on the GPU arm and the
  software arm degrades it as declared. Prefer those. What is *not* free is neutral
  scene data — geometry, base colours, light rigs, camera pose — which reaches every
  arm. That is fine (the guarded arm may drift); it is simply not a way to hide a
  change from the gate.
- **Never lower the judged arm to keep a guarded arm happy.** If the reference needs
  something the guarded arm cannot express, that is exactly what the capability system
  is for. The fix is a declared degradation, never a lesser scene.

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

1. **Score** the champion against the reference, `0..5` per axis, by eye. **Calibrate to
   identity and score harshly.** The bar is "indistinguishable from the reference," so a
   render that merely *reads as the same subject* is low, not middling. Explicit anchors:
   - **0** — absent or wrong (the axis's subject isn't there, or is plainly incorrect).
   - **1** — present but crude/proxy: you can tell *what it is meant to be*, but nobody would
     mistake it for the reference (a boxy cube-puppet standing in for a modeled player; a
     flat colour band standing in for a photographic crowd; a plain sphere for a panelled
     ball). **Most axes start here against a polished reference — that is correct, not
     pessimistic.**
   - **2** — the right structure is emerging but is obviously stylized/simplified next to the
     reference.
   - **3** — clearly on-model; a viewer sees the same intent, but side-by-side the gap is
     still obvious.
   - **4** — near-parity; you have to look closely to tell them apart.
   - **5** — indistinguishable from the reference to a human reviewer. A stylized/low-poly
     render is **not** a 5. Never claim parity until the user explicitly accepts it.
   - An inflated scorecard corrupts the whole loop: it hides the real flaw, picks the wrong
     axis to attack, and fakes progress. When unsure between two scores, take the **lower**.
     If you catch your scores drifting up without the render approaching *identity*,
     recalibrate down and record it as a scoring-recalibration iteration in the ledger.
   - `final_score = lowest_axis * 0.7 + average_axis * 0.3`. The **lowest axis is the next flaw
     to attack** (ties broken by fixed axis order).

2. **Attack the lowest axis with ONE bounded change — the smallest change that actually
   closes the gap to the reference.** "Smallest" is measured toward *parity*, not toward
   *least code*. There is a ladder — `config/manifest → generation/data → backend/shader →
   new geometry/primitive` — and you still start as low as a change that can *reach the
   reference* allows. But **do not spend the iteration on a cosmetic tweak you already know
   cannot close the gap.** Against a polished reference the honest smallest-correct change is
   usually **structural**: real geometry where there were proxy boxes, a panelled mesh where
   there was a bare sphere, mow-stripe/field-marking geometry, a material/shader feature, a
   re-shaped data contract. Reach for the structural fix directly when config demonstrably
   can't express parity (see the abstraction gate's "cannot express" branch — for a parity
   target it is the common case, not the exception). Keep it **one axis, one coherent change,
   fully scored** — structural does not mean unbounded or multi-axis. Before editing, write
   the rationale:
   - *Attacked mismatch* — what about this axis differs from the reference, concretely.
   - *Why it's the most important flaw* — it's the lowest / dominates final_score.
   - *The change* — the one structural (or config) change you'll make, and the layer it lives at.
   - *Why this is the smallest change that can reach parity on this axis* — and, if you're
     staying at config/data, why that genuinely *can* close the gap rather than just nudge it.
   - *What deeper structural move is queued next* if this one only gets partway.

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

## Champion archive — keep every landed champion

`champion.png` is **overwritten** every time a candidate is promoted, so by design the
campaign directory only ever holds the newest render. The history of how the app actually
looked is then recoverable only by digging shas out of git, which nobody does. So:

> **Every champion that lands on `main` is copied into `<target-dir>/champions/` and kept
> forever.** Archive on landing — not on promotion. A champion that only ever lived on a
> convergence branch is not history; a champion that is on `main` is what the app looks
> like, and that is the progress worth seeing.

```
<target-dir>/champions/
  0000-<yyyy-mm-dd>-<sha>.png   # one per landing, oldest first, never edited or deleted
  0001-<yyyy-mm-dd>-<sha>.png
  INDEX.md                      # one row per landing + the reference-era key
  contact-sheet.png             # reference first, then every champion, left to right
```

Do this as the **last step of landing** (after the champion line is merged to `main`, in
the same commit as the merge or immediately after it):

1. `cp <target-dir>/champion.png <target-dir>/champions/NNNN-<date>-<sha>.png` — `NNNN`
   is the next landing index, `<sha>` the landed commit.
2. Add one row to `champions/INDEX.md`: index, date, commit, reference era, what landed
   (which lenses, in one line), and the lowest axis afterwards.
3. Regenerate `contact-sheet.png`:
   ```python
   from PIL import Image, ImageDraw; import glob, os
   T = '<target-dir>'
   tiles = [(T+'/reference.png','REFERENCE')] + [(p, os.path.basename(p)[:-4])
            for p in sorted(glob.glob(T+'/champions/0*.png'))]
   H, BAR, PAD = 520, 26, 8
   ims = [(Image.open(p).convert('RGB'), lab) for p, lab in tiles]
   ims = [(im.resize((max(1,int(im.width*H/im.height)), H), Image.LANCZOS), lab) for im, lab in ims]
   W = sum(i.width for i,_ in ims) + PAD*(len(ims)+1)
   sheet = Image.new('RGB', (W, H+BAR+PAD*2), (16,16,18)); d = ImageDraw.Draw(sheet); x = PAD
   for im, lab in ims:
       sheet.paste(im, (x, PAD+BAR)); d.text((x+3, PAD+7), lab, fill=(225,225,230)); x += im.width + PAD
   sheet.save(T+'/champions/contact-sheet.png')
   ```
4. Commit the archive with the landing.

**Reference eras.** A champion is only comparable to the reference it was scored against.
When the user supplies a **new** reference, the campaign enters a new era: bump
`reference_era` in `campaign.toml`, record the era key in `champions/INDEX.md` (what
changed, and the sha the old `reference.png` is recoverable from), re-score the champion
from scratch, and say plainly in the ledger that no score movement across the era
boundary is progress — it is a different exam. Do **not** carry scores across an era, and
do not delete the older champions: the contact sheet spanning both eras is the record of
what the app has actually looked like.

## Abstraction gate

A **new primitive / structural change** (a new manifest field, a new engine capability, a
shader/material feature) is allowed **only** after either:
- the same axis has failed **≥ 3** bounded (config/data) attempts, recorded in the ledger, or
- the current implementation genuinely **cannot express** the needed change.

For a **parity target** the second branch is the *norm*, not a rare escape: when the axis
gap is structural (proxy geometry vs a modeled subject, a bare sphere vs a panelled ball,
no field markings vs painted lines), the config layer *cannot express* the reference, so
you go structural on the first attempt — do **not** burn three ceremonial config nudges you
already know will fail just to "unlock" the gate. Still record the `abstractions/NNNN.toml`
justification with `inexpressible = true` and `failed_attempts = []`. The gate exists to
stop *gratuitous* new surface, not to force cosmetic theater before an obviously-needed
structural fix. Keep the new surface minimal and at the lowest correct layer.

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

- **Canvas 2D is flat-shaded and ignores textures/normal maps** — texture, normal-map, sky,
  specular and bloom richness is GPU-only; canvas2d keeps a legible flat proxy (per the
  capability system). Score the arm the reference was shot on and guard the other — see
  §Judged renderer arm. Never score two arms on one scorecard.
- **Match the moment.** For a game, the screenshot must capture the same camera/tick/state the
  reference shows, or the axes are comparing different things.
- **Reference-derived composites** (side-by-side comparison images you build for review) are
  scratch — send them to the user, don't commit them.
- Related memory: `gpu-fidelity-and-capability-system`, `visual-convergence-comparator`.
