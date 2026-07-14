# prologue_postcard_001 — Pixel-Accountability Audit

**No features added.** This pass proves, by controlled toggles against a byte-deterministic
GPU render, which claimed renderer features actually change the final screenshot, which are
too weak, and which are no-ops — then names the real bottleneck to the reference.

Foreground subject note: the reference includes a hand + compass. **The target intentionally
excludes it** (per direction). The champion frames the same forest without the hand; the
foreground subject is treated as **masked/ignored** and is NOT scored — `subject_fidelity`
is not penalized for the missing compass.

## Harness / screenshot command

Real app through the existing GPU offscreen harness (same manifest, camera, tick, viewport,
target dir). Render is byte-deterministic (rendered twice → identical md5), so a toggle that
changes pixels is real and a no-op toggle is a true no-op.

```
cargo run --features visual-target --bin visual-target -- \
  render apps/axiom-gallery/visual_targets/prologue_postcard_001/manifest.toml \
  --backend gpu --out .../diagnostics/audit/<name>.png
```

Manifest-only toggles (fog, post-process) used a temp copy of `manifest.toml`; shader/
material/normal/lighting toggles were temporary one-line edits to
`modules/axiom-gpu-backend/src/scene_renderer.rs` (SCENE_WGSL) or
`apps/axiom-gallery/src/growth/visual_target/build.rs`, each reverted with `git checkout`
immediately after rendering. The working tree is unchanged by this audit.

## Screenshots

- Baseline: `diagnostics/audit/01_baseline.png`
- Toggles: `diagnostics/audit/{02_shadows_off, 03_shadows_exag, 04_fog_off, 05_fog_exag,
  06_flat_albedo, 07_normals_off, 08_postproc_off, 09_fullbright, 10_depth, 11_normals_viz,
  12_objclass}.png`
- Montage of all 12: `diagnostics/audit/_montage.png`

## Diff vs baseline (mean / max abs, % pixels changed >3, per sky·mid·ground band)

| # | Toggle | mean | max | %chg | sky·mid·ground | Verdict |
|---|--------|-----:|----:|-----:|----------------|---------|
| 02 | shadows OFF | 10.2 | 159 | 37% | 7.7·13.4·9.6 | material — shadows real |
| 03 | shadows EXAGGERATED | 8.6 | 132 | 29% | 9.2·12.4·4.1 | headroom exists (can go much darker) |
| 04 | fog OFF | 16.9 | 139 | 78% | **30.4**·17.5·2.9 | material — fog = the bright background |
| 05 | fog EXAGGERATED | 14.2 | 83 | 95% | 14·15·13 | fog strong (whole frame) |
| 06 | flat albedo (white RGB) | 15.5 | 100 | 67% | 6.3·11.9·**28.2** | material — bark+ground textures real |
| 07 | normal maps OFF | **2.0** | 96 | 21% | 0.8·1.3·4.1 | **NEAR NO-OP** (only faint on ground) |
| 08 | post-process OFF | 20.0 | 49 | 100% | 15·21·**24** | material — grade drives contrast/sat |
| 09 | fullbright (no lighting) | 27.5 | 142 | 94% | 31·**38**·14 | lighting does most of the tonal work |
| 10 | depth viz | — | — | — | near bright→far dark | depth buffer works (occlusion correct) |
| 11 | normal viz | — | — | — | trunks cyan, **ground ~flat** | normal map ≈ geometric only |
| 12 | object-class viz | — | — | — | terrain/trunk/foliage/tuft/litter | all classes render |

## Per-feature accountability (code exists ≠ feature counts)

- **Directional shadow map** — implemented ✅, wired ✅, **visible ✅** (37% of pixels; the
  ground shadow bands; exaggerate shows large headroom). Correctly contributes. Not weak
  (strengthened last iteration).
- **Distance fog / aerial haze** — implemented ✅, wired ✅, **visible ✅ (strong)**. It *is*
  the bright warm background (sky band moves 30 when removed). If anything it is slightly
  **too strong** — it blows the upper background toward white, which reads brighter/flatter
  than the reference's controlled mist.
- **Post-process color grade (exposure/contrast/saturation)** — implemented ✅, wired ✅,
  **visible ✅ (strong)**, 100% of pixels. Correctly carries the tonal look.
- **Material albedo textures (procedural bark + ground)** — implemented ✅, wired ✅,
  **visible ✅** (67%; concentrated on ground + trunks). Correctly contributes.
- **Normal maps (bark + ground, screen-space TBN)** — implemented ✅, wired ✅, but
  **effectively a NO-OP in the final image**: turning them off changes mean **2.0** and only
  the ground band (4.1); the normal-viz shows the ground normals are essentially flat (the
  perturbation is dominated by the geometric normal). **Blunt: this claimed feature does not
  materially change the screenshot.** It is technically live but visually negligible at this
  camera/scale — a real cost paid for ~nothing on screen.
- **Lighting (hemisphere ambient + directional diffuse)** — implemented ✅, wired ✅,
  **visible ✅ (strong)**; fullbright is pale and flat, so ambient+diffuse+shadow do the
  shaping.
- **Depth buffer** — works (correct occlusion, shadow depth-test, depth-viz gradient).
- **Object classes** — terrain, trunk, canopy/foliage cards, ground-cover tufts, litter all
  render and are separable.

## The biggest actual bottleneck

**Not a missing or broken renderer feature — it is scene data.** Every renderer feature
except normal maps materially affects the frame; normal maps are a no-op. The dominant gap
to the reference is the **foliage/canopy**: the reference is a dense, layered canopy of
**branch-attached, individually-lit leaves**; the champion is **sparse floating alpha-card
sprays** with visible gaps and edge clumps that read as debris. Secondary scene-data gaps:
smooth **cylinder trunks with no branches/taper**, and a flat heightfield ground of sprite
tufts/litter. The renderer is already lighting, shadowing, fogging, grading, and texturing
this low-poly card scene about as far as it can go — **more renderer features will not close
the gap; the card-foliage + low-poly geometry is the ceiling.**

## Where the next correct attack lives

**Scene-data-side (foliage generation/geometry).** Renderer-side is largely done (features
wired; the only renderer defect is that normal maps are a no-op — low value to fix).
Material-data-side is minor (textures already contribute). Camera/composition is acceptable
(same framing; the champion camera sits a touch lower/more ground-dominant, but that is not
the bottleneck).

## Harsh scorecard recalibration (fixed axes, identity bar, hand masked)

Calibrated to *identity with a photoreal reference*, so proxy/stylized geometry scores low.
This is a recalibration **down** from earlier optimistic scores (recorded as such).

| Axis | Score | Why |
|------|:----:|-----|
| terrain_silhouette | 3 | tree-line reads, but stylized against dense reference |
| foreground_material_detail | 3 | ground textured + litter/tufts, but sprite-ish/low-res |
| **vegetation_density** | **2** | sparse floating cards vs dense branch-leaves — worst gap |
| vegetation_clumping | 3 | clumped into masses, but the clumps are card sprays |
| depth_separation | 3 | fog+shadow give depth; background over-bright |
| fog_and_haze | 3 | warm haze present but blows the background |
| lighting_directionality | 3 | shadows directional, but band-like vs organic dapple (was 4 → recalibrated down) |
| color_palette | 3 | warm autumn, but more uniformly orange than the varied reference |
| contrast_and_exposure | 3 | grade helps, highlights still blown |
| object_scale | 4 | trunk/tree scale is believable |
| horizon_composition | 3 | framing similar, camera a touch low |
| **artifact_level** | **2** | floating card-spray foliage + blown sky read as artifacts |

`final_score = lowest(2)·0.7 + avg(2.92)·0.3 = 2.28`

## Next single axis to attack

**`vegetation_density`** (score 2). Tied at 2 with `artifact_level`, broken by axis order —
and they share one root cause (the sparse card foliage), so fixing density fixes both.

## The exact next structural pass (blunt)

**A foliage/canopy scene-data overhaul — not a renderer feature.** Replace the sparse
floating leaf-card sprays with a **branch-scaffold + dense leaf placement** generator: a few
branch strokes per tree carrying many small leaf quads clustered along them, at much higher
density, so the canopy reads as attached, layered foliage instead of debris. This is
generation/geometry in `growth/visual_target` (`build.rs` foliage + a branch primitive +
`scene.rs`/manifest density knobs), rendered by the existing pipeline. It directly attacks
`vegetation_density` (2→) and `artifact_level` (2→) at once.

Two honest side-cleanups this audit surfaced (do not add features, just correct existing
ones): (a) **normal maps are a no-op** — either make them actually read (they don't) or stop
paying for them; and (b) **fog/exposure blow the background** — the upper frame clips toward
white, flatter than the reference's controlled mist. Neither is the bottleneck; the foliage
is.
