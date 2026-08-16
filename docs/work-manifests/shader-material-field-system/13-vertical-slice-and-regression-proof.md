# 13 — The vertical slice and the regression proof

## Objective

Prove the whole architecture end to end on real, tuned, shipped content by
replacing **one** of `apps/burnt-rubber`'s three hand-written CPU texture
generators with an authored field graph — and prove, on `apps/axiom-rotating-cube`,
that the system did not make trivial rendering absurd.

## Why burnt-rubber, and why the asphalt generator specifically

`apps/burnt-rubber` is the only app in the repo whose appearance is *already*
three procedural fields, hand-written in Rust with roughly forty derived tuning
constants each carrying a paragraph of justification:

| Generator | File | Size | Method |
|---|---|---|---|
| asphalt | `apps/burnt-rubber/src/render/asphalt_texture.rs` | 759 lines, 128² | three weighted octaves — `smooth_octave` (`:354`), `cross_octave` (`:341`, **x-only**), per-texel `hash_unit` (`:385`) — mixed `SMOOTH_SHARE 0.25 / CROSS_SHARE 0.55`, contrast-expanded ×1.6 about the midpoint (`grain()` `:318-326`), emitted as a neutral grey multiplier (`asphalt_albedo()` `:300`) |
| verge | `.../verge_texture.rs` | 441 lines, 64² | 8² toroidal clump field + 22% fine hash, per-channel `COVER→DUST` lerp |
| foliage | `.../foliage_texture.rs` | 541 lines, 64² | leaflet comb; divides out the pattern's ~0.56 mean so texturing is not also a grade |

`hash_unit`, `smoothstep` and `lerp` are **byte-identical copies** across all
three files, and `byte_for_multiplier` (a hand-rolled sRGB encode) appears three
times with an inverse in the fourth. This is the duplication the whole design
exists to remove, in the one app where the content is real enough that a
regression is visible.

It also already exercises the full chain: `RecipeGraph` → geometry
(`src/render/rock_mesh.rs`, a 3-node `Sphere → Displace → Transform` graph whose
own test asserts `bytes.len() < 256`), CPU pixels → `add_texture_data` →
`with_custom_texture` → `TextureSampling::Anisotropic` → mip chain → `SCENE_WGSL`.

And its `app.toml` **already declares `recipe` and `proc-mesh`** in
`allowed_layers` — so adding `field`, `surface` and `proc-texture` is an
extension of an existing, precedented arrangement rather than a new one.

**Asphalt, not verge or foliage**, because it is the largest, the most tuned, the
one sampled at `Anisotropic` with a real mip chain, and the one whose
`cross_octave` immediately exposes a genuine vocabulary question (below).

`apps/axiom-proc-player` is the wrong choice — it already passes and would prove
nothing new.

## What the slice must prove

The brief's nine requirements, each mapped to a concrete artifact:

| # | Requirement | How it is proven |
|---|---|---|
| 1 | an app constructs a structured field graph | `asphalt_field()` in the app returns a `FieldGraph` |
| 2 | that graph drives a material property | it is bound to `SurfaceChannel::BaseColor` (and, in the stretch case, `Roughness`) |
| 3 | the representation crosses the proper boundaries | `Surface` → `Material` → `MaterialData` → `RenderMaterial` → `MaterialAsset` → `FrameDrawItem.surface_program` |
| 4 | the backend lowers it to a real shader | `08`'s emitter produces WGSL compiled at the barrier (`09`) |
| 5 | the real app renders it | `axiom-shot` PNG + a live browser capture |
| 6 | changing graph data changes the output | a second graph with one constant altered produces a different `agent_*_resources.bin` digest **and** a visibly different PNG |
| 7 | no raw WGSL is authored by the app | `apps/burnt-rubber` contains no shader text — assert by a grep test |
| 8 | deterministic/canonical representation is testable | the graph's `digest()` is asserted; two authoring orders canonicalise equal |
| 9 | the result is captured by the existing harness | the 15 committed goldens + the convergence campaign |

**The composition requirement — "a spatial gradient mixed with procedural
variation driving colour or roughness" — is the asphalt generator's actual
structure**, not a contrivance: a smooth octave, a directional octave and a fine
hash, weighted and remapped. That is what makes this slice meaningful rather than
a hard-coded red shader.

## Architectural placement

**App** (`apps/burnt-rubber`, `apps/axiom-rotating-cube`) plus **Tooling**
(goldens, `slice.toml`). No engine change should be needed; if one is, that is a
defect in an earlier manifest and must be reported rather than patched in the app
— `xtask`'s `SlicePlacementEngineLogicInApp` check exists to catch exactly that.

## Existing code involved

| Path | Role |
|---|---|
| `apps/burnt-rubber/app.toml` | `allowed_layers` gains `field`, `surface`, `proc-texture` |
| `apps/burnt-rubber/src/render/asphalt_texture.rs` | the generator being replaced |
| `apps/burnt-rubber/src/preparation/textures.rs` | `PreparedTextures::generate()` — where baking happens today, and where program preparation joins it |
| `apps/burnt-rubber/src/render/palette.rs:546-552` | **texture ids are `len + 1` and are baked into the goldens** — registration order is load-bearing |
| `apps/burnt-rubber/src/golden.rs` | the run definition: seed, `PlayProfile::Wheel`, `Tuning::DEFAULT`, 3,800 agent-driven steps |
| `apps/burnt-rubber/tests/agent_golden.rs` | the driver |
| `apps/burnt-rubber/tests/golden/` | 15 artifacts — `{grid,opening,esses,canyon,finish}` × `{render,resources,state}` |
| `apps/burnt-rubber/slice.toml` | SHA-256 pins, checked by `cargo run -p xtask -- check-slices` |
| `tools/axiom-shot/src/registry.rs:113-180` | 13 registered burnt-rubber slices |
| `visual_targets/burnt-rubber/campaign.toml` | the live-browser convergence recipe |

## The harness — exact commands

**A. Native headless PNG.**
```sh
cargo run -p axiom-shot --features offscreen -- \
  --app burnt-rubber-straight --backend gpu --tick 0 --out screenshots/br-straight.png
cargo run -p axiom-shot -- \
  --app burnt-rubber-straight --backend canvas2d --tick 0 --out screenshots/br-c2d.png
```
**Without `--features offscreen` the GPU arm silently falls back** — a capture
that does not assert its backend is worthless.

**B. Byte goldens — the artifact that actually catches this change.**
`apps/burnt-rubber/tests/golden/agent_opening_resources.bin` is a content
fingerprint of every uploaded mesh **and every uploaded texture**.
`slice.toml` states the reason verbatim:

> *"`FrameOutcome` carries a mesh id, never its vertices … a moved constant inside
> `asphalt_albedo` would render a visibly different game while leaving the draw
> list byte-identical."*

That sentence was written about this exact hazard. **The resources golden is the
primary acceptance artifact of this manifest.**

**C. Live-browser convergence.** `visual_targets/burnt-rubber/campaign.toml` —
serve on 8085, viewport 470×836 @ dpr 2, then **freeze first**, place, step:
```
window.__probe.burnt_rubber_probe_pause(true)
window.__probe.burnt_rubber_probe_place(1900, 45)
window.__probe.burnt_rubber_probe_step(90)
expect_distance_m = 1966.118
```
A capture reporting any other distance is a failed capture, not a changed render.
`judged = "gpu"`, `guard = ["canvas2d"]`, `guard_rule = "legibility, not parity"`.

## Dependencies on earlier manifests

**All of them.** This is the last manifest.

## The work

### Step 1 — author the graph

Add `apps/burnt-rubber/src/render/asphalt_field.rs`: a function returning a
`FieldGraph` reproducing `asphalt_albedo()`'s three-octave structure with the
existing constants (`SMOOTH_SHARE 0.25`, `CROSS_SHARE 0.55`, `CONTRAST 1.6`,
`MIN_MULTIPLIER 0.86`) as `Const` nodes or, better, as **named parameters** — so
the slice also demonstrates that retuning is a uniform write, not a recompile.

**Two known vocabulary questions, and they are the point of the slice:**

* **The directional (`cross_octave`) term.** Anisotropic, x-only. Expressible:
  sample `Fbm` at `Compose(Mul(Component(Uv, 0), k), Const(0), Const(0))`. Verify
  it, because this octave is the one that survives 16× anisotropic filtering at
  depth and is therefore the visually load-bearing one.
* **Guaranteed toroidal wrap.** The existing octaves wrap by construction on a
  32²/8² lattice. `axiom_noise::value_noise` does **not** wrap. If the seam is
  visible, the honest options are (a) accept it — a 128² asphalt tile tiled across
  a road may not show it; (b) express the wrap as a `Mix` of two samples at
  offset domains; (c) conclude the algebra needs a domain-wrap facility and
  **report it rather than adding an operator locally.** Decide by looking at the
  render, not by argument.

### Step 2 — bind it

Bind the graph to a `Surface`, bind the `Surface` to the tarmac `Material`,
register the surface program in the existing `PreparedTextures` preparation task.

**Keep the texture registration order identical** — ids are `len + 1` and are
baked into the goldens (`palette.rs:546-552`). If the asphalt texture stops being
a texture, its id disappears and every later id shifts. **Preferred: keep the
other two textures exactly as they are and let asphalt become a surface**, then
regold once, deliberately, with the shift documented in the commit message.

### Step 3 — prove the change is a change

A second graph identical but for one constant must produce a different resources
digest and a visibly different PNG. This is requirement #6 and it is what
separates "the system runs" from "the system works".

### Step 4 — the regression control

`apps/axiom-rotating-cube` authors a complete scene in 237 lines, of which the
material is one:
```rust
let material = materials.add(Material::lit(color).with_texture(Texture::Checker));
```
**This must still be one line.** If a field system makes `Texture::Checker` cost a
graph, the design has failed and that is a reportable defect, not something to
work around in the app.

The app currently has **no goldens at all** — its two `.bin` artifacts were
deleted 2026-08-11 as drifted, and `tests/render_determinism.rs:92-102` now only
asserts self-equality and non-vacuity. **Nothing in the repo pins what trivial
rendering looks like.** Land a real pinned artifact for it in this manifest; that
gap is a genuine hole this work is obliged to close, because it is the only
control against the whole system quietly degrading the simple case.

## Explicitly excluded

* **Do not port verge or foliage.** One generator is the slice. Porting all three
  is follow-on work, and doing it here would make a regression impossible to
  attribute.
* **Do not delete `asphalt_texture.rs` in this manifest.** Keep it, keep its
  tests, and add an equivalence test comparing the two paths within tolerance.
  Delete it in a follow-up once the render has been judged by a human.
* Do not change the golden run definition (seed, profile, tuning, 3,800 steps).
* Do not touch the convergence campaign's champion or scorecard — a capture for
  this slice is a *candidate*, and scoring it is the `/visual-convergence`
  workflow's job, not this manifest's.
* No new engine capability. If one seems needed, stop and report.

## Determinism requirements

* The field-baked asphalt must be byte-identical across runs and targets.
* The 3,800-step agent run must still be byte-identical — the sim is untouched, so
  `agent_*_state.bin` must **not** change. If it does, something has leaked from
  rendering into simulation and that is a serious defect.
* Tick N and tick N+60 differ; tick N replayed twice is identical.

## Serialization requirements

Regold `agent_*_render.bin` (the `surface_program` lane from `06`) and
`agent_*_resources.bin` (the texture change). **`agent_*_state.bin` must be
unchanged.** Update the SHA-256 pins in `slice.toml` in the same commit.

## Testing requirements

Apps are outside the 100% coverage gate but ship the tests their behaviour
warrants:

* The field graph's `digest()` is asserted against a committed value.
* Two authoring orders of the same graph canonicalise to equal digests.
* CPU equivalence: the field-evaluated asphalt matches `asphalt_albedo()` within a
  stated per-channel tolerance, over the full 128² tile.
* A grep test asserting `apps/burnt-rubber` contains **no** WGSL/shader text
  (requirement #7).
* Changing one constant changes the resources digest.
* `agent_*_state.bin` is unchanged.
* `apps/axiom-rotating-cube` gains a real pinned render artifact.

## Architecture tests

```sh
cargo xtask check-architecture          # app.toml gains three layers
cargo run -p xtask -- check-slices      # the SHA-256 pins
cargo run -p xtask -- check-slice-placement
```
The last one is not run by CI and not by `check-architecture`; run it explicitly.
`SlicePlacementEngineLogicInApp` is a heuristic that flags a large `apps/` file
with public geometry-producing functions and no `*Api::` call — exactly the shape
of an app-local field implementation. If it fires, the logic belongs in the
engine.

## Performance risks

* **Preparation time.** `PreparedTextures::generate()` runs in the startup
  barrier; a 128² field bake is 16,384 evaluations of a ~15-node graph. Measure
  against the current generator and report the delta. A barrier has no frame
  budget, but a startup that doubles is still a regression.
* **Fragment cost.** If asphalt becomes a *live* shader rather than a baked
  texture, its cost moves from startup to per-pixel on the largest surface in the
  frame. **Prefer baking it to a texture through `TextureOp::Field` (`05`) for
  the first slice**, and treat live evaluation as the stretch goal — that keeps
  the slice honest about what it proves and defers the fill-rate question.
* Program count after preparation must be asserted (`09`), so a variant explosion
  shows up as a failing test.
* Draw-call and GL-call counts must be unchanged for the parts of the scene that
  use no surface.

## Migration considerations

15 goldens + `slice.toml` pins + possible texture-id shift. Do it once,
deliberately, in one commit, with the reason in the message.

## Completion criteria

1. `apps/burnt-rubber` authors asphalt as a `FieldGraph`, with no WGSL anywhere in
   the app.
2. The graph reaches the backend and the app renders — confirmed by a **read**
   screenshot, not by a green build.
3. Changing one constant changes both the resources digest and the pixels.
4. `agent_*_state.bin` unchanged; render/resources goldens regolded and pinned.
5. `apps/axiom-rotating-cube` still authors a material in one line and now has a
   pinned render artifact.
6. `cargo test --workspace`, `check-architecture`, `check-slices`,
   `check-slice-placement` all pass; coverage 100/100/100 on the spine; no dylint
   count rises.
7. A live browser capture at the campaign's frozen state is error-free in the
   console and is filed as a convergence *candidate*.

## Validation commands

```sh
cargo test -p axiom-burnt-rubber
cargo test --workspace
cargo xtask check-architecture
cargo run -p xtask -- check-slices
cargo run -p xtask -- check-slice-placement
bash scripts/coverage.sh
bash scripts/dylint-gate.sh

cargo run -p axiom-shot --features offscreen -- \
  --app burnt-rubber-straight --backend gpu --tick 0 --out screenshots/br-field.png

uv run scripts/localhost_servers.py start-app burnt-rubber --port 8085
uv run scripts/localhost_servers.py logs burnt-rubber -n 20
uv run scripts/playwright_controller.py goto http://localhost:8085/
uv run scripts/playwright_controller.py wait 2000
uv run scripts/playwright_controller.py console
uv run scripts/playwright_controller.py screenshot burnt-rubber-field
```

**Read the screenshot.** A green build and a painted page are different facts.

## Parallel safety

**Wave 10, width 1.** Nothing may run concurrently — it regolds shared artifacts.
