# The parity campaign — ledger

Driving `apps/axiom-shmup` (the Rust/Axiom port, restored at `94f8890b`) to visual
parity with `apps/shmup` (the JS original, `?fidelity=full`).

Servers: original `http://localhost:8087/`, port `http://localhost:8088/`.

## Standing facts every agent must know

- **MSVC or nothing.** `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`. The
  default `windows-gnu` cannot link this crate's test binary at all, and its libm
  zeroes the low mantissa bits of `cos`/`sin` near axis angles (~3.3e-5 relative),
  which reads as a port bug and is not one. MSVC is bit-identical to V8.
- **The original is not deterministic by default.** It only uses the fixed seed
  `0x5eed1234` under `?capture=1` (`apps/shmup/src/core/engine.js:35`,
  `main.js:71`). Measured: three loads of `?fidelity=full` produced three
  different towns (580,661 / 567,197 / 585,630 static triangles). Every
  comparison must run the original in capture mode.
- **Never run two gates at once** on this machine — it OOMs, and dylint then
  fakes a `cargo metadata` error that masks the real finding. `link.exe`
  `0xc0000142` means out of RAM.
- `apps/` is outside the Branchless Law and the Coverage Law. `crates/` and
  `modules/` are not.

## THE DOMINANT DEFECT — the port cannot make anything dark

Established by independent audit at matched camera on two shots. **This outranks
everything else in this document.**

- **Interior/exterior is INVERTED.** Original room = **0.399x** the street outside
  (2.5x darker). Port room = **1.454x** (brighter). The `interior` shots entire
  subject, backwards, by 3.6x. Interior wall x2.69 too bright, exterior through
  the door x0.74 too dark.
- **The error is in the shadows and in RED.** Shadowed sidewalk on `hero`:
  original `0.097,0.127,0.159`, port `0.546,0.331,0.163`. **Blue matches to x1.02;
  red is inflated x5.63.** Split-tone: shadow warmth 0.80 -> 1.55 (x1.94),
  highlight warmth 1.12 -> 1.20 (x1.07). **The highlights already match.**
- **Coherent shadow area x0.34.** Road rect: original 2 dark blobs over 22.7%;
  port 10 fragments over 7.7%. Port p10 luminance 0.221 vs 0.109 — its darkest
  decile is twice as bright as the originals.

**Cause, structural and stated in the ports own source:** `look.rs:42-45` records
that the originals ambient comes from a **GPU env bake** the port does not have;
`look.rs:32` calls hemisphere ambient "partial". The substitute is a **fixed
two-band fill** (`look.rs:433-437`, `SKY_FILL 0.32`, `INTERIOR_INDIRECT 0.035`)
with **no occlusion or visibility term**. A constant fill is structurally
incapable of making an interior darker than a street. **No value of those
constants fixes it — do not tune them.**

Consequence for the cascade work: **even a perfect cascade will not read correctly
until the ambient stops flooding the shadows.**

## THE INSTRUMENTS WERE WRONG

- **Bimodality and p90:p10 do not measure cast shadows.** Both are histogram
  statistics; a histogram is spatially blind, so a shadow slab and salt-and-pepper
  noise score identically. **Bimodality has the SIGN BACKWARDS** on a road rect
  (orig 0.178, port 0.287). The 0.656/0.541 whole-frame reading was the
  sky/ground/viewmodel split. p90:p10 is unreproducible (3.28 vs 2.45 on clean
  road) and is an outlier detector.
  **Correct instrument: `scripts/shadow_structure.py`.** Target: top-3 blobs
  >= 22.7% in <= 2 components.
- **Every metric is blind to a dead frame.** Two black captures scored a PERFECT
  match (`maxdiff=0.0`). Run `scripts/frame_sanity.py` before scoring anything.
- **Means over mixed regions launder inversions.** `hero`s global tone reads SAME
  only because x2.69 and x0.74 errors cancel; `interior` reads +0.474 stop. **Never
  quote a global tone number again without a paired local ratio.**
- **The capture recipe fails silently.** `?capture=1` ALONE did not raise
  `__READY__` during the audit (engine never stepped, black page, live HUD).
  **`?capture=1&prewarm=0` works.** `lockstep=1` reaches ready but renders a wrong
  frame. Some existing numbers are of unknown provenance.

## CENSUS CONTRADICTION — SETTLED

Both agents were right and measuring different instruments. Claim Bs ratios
reproduce exactly by dividing port per-frame/inventory numbers by the originals
BUILD-TIME numbers: 140/62 = 2.26, 506/308 = 1.64, 686822/586000 = 1.17 — that
last dividing total mesh tris by a static-only subtotal, discarding the originals
own 115k instanced tris. **Like for like: x0.980.** Edge cross-correlation peaks
at exactly (0,0) on both shots. **The buildings match to sub-two-pixel accuracy;
the clutter does not.** The -1.80 stop `shade` residual re-attributes from town
difference to the ambient defect above.

## NEWLY FOUND (nobody had looked)

- **Minimap renders no world geometry** — empty grid vs the originals footprints
  and street network.
- **Compass heading is wrong and not by a constant offset** — `hero` orig NW, port
  SE (180deg); `interior` ~105deg apart. Geometric truth at `hero` is N33.7W; the
  original is correct.
- **Viewmodel is a far lower-detail mesh**, flat-shaded, offset ~60px left/20px up.
  ~15% of the frame.
- **Sky colour does NOT match** (earlier claim refuted): cloud-excluded zenith R
  0.765 -> 0.641, and the **vertical gradient is inverted** — original brighter at
  the horizon (correct air-mass scattering), port brighter at the zenith.

## EXECUTION FREEZE (in force)

Ten concurrent agents building at once thrashed the machine. **Agents are
read-and-write only**: no `cargo`, no `node`/`npm`/`uv run`, no servers, no
browser, no `apps/shmup/tools/*.mjs`, no gates. They may search with `ax`, read
files, read git history, and write their changes with `ax apply`/`ax edit`.

All compiling, testing and capturing is **centralised through the integrator**,
once agents have quiesced. Consequences to hold on to:

- Every agent report after this point is **UNVERIFIED by its author**. Each owes
  an explicit runbook — the command to run and what a pass looks like.
- Claims that were only going to be true *because* the agent intended to test
  them must be flagged as such. Reading proves different things than running.
- Nobody may fabricate a screenshot, a measured mean, or a parity score. A score
  with no pixel behind it would poison every later decision in the campaign.
- `METERING_FIT` cannot be re-fitted empirically under the freeze. It is to be
  **derived, labelled DERIVED-NOT-MEASURED**, and re-fitted by the integrator.

## Baseline captures

| what | path |
|---|---|
| original, `?fidelity=full` | `scripts/.playwright-controller/screenshots/js-original-full-040559.png` |
| port, restored | `scripts/.playwright-controller/screenshots/rust-port-first-040459.png` |

Both are 1280x720. The port's backbuffer is hardcoded to that at DPR 1, so the
original must be captured at `--w=1280 --h=720` or the comparison is between
resampled images.

## Wave 1 — diagnosis (complete)

| finding | verdict |
|---|---|
| **Double tone map.** `look.rs`'s `display()` is a Reinhard at exposure 1.0, upstream of the engine's real AgX composite. Authored exposure ~16.745 exceeds AgX's entire domain (16.29 linear). Key light is on the correct photometric scale while everything around it is not, destroying the sun/shadow ratio. Upstream of four consumers: `FrameSky` stops, `FrameSky` body, `FrameAmbient`, `FrameDepthFog`. | **Two independent agents converged on this by different evidence paths.** Highest leverage in the campaign. |
| **Materials seam is CLOSED, not open.** `05-port-status.md`'s "nothing samples it" is stale: the port resolves 46 palette keys through `runtime_material()` and binds albedo + normal + ORM. Flatness is `RUNTIME_BAKE_SIZE = 64` (source bakes 1024² on GPU; port 64² on CPU = 3.3 cm/texel), plus `detail`/`macro_field` baked-and-dropped, plus `parallax`/`detile` force-zeroed by expired deferrals. | Status doc corrected. |
| **Shadows are wired but coarse.** Every hop present. The frame contract carries ONE `light_view_proj`, so one cascade spans a ~205 m box in a 1024² atlas — a ~1.25 m penumbra plus ~34 cm peter-panning. The finished 4-cascade port sits in `cascade.rs` bound by nothing. | Engine-side; contract width. |
| **Sky is fully ported, not partial.** All ten source files exist (4,371 Rust lines). ~1,600 of them — dome shader, cloud fbm, stars, volumetrics — are correct, tested and *unreachable*, because `FrameSky` is the only seam and is far narrower than what was ported. | Status doc corrected. |
| **The two apps do not build the same town.** The port's world is RNG fork #1 where the original's is #3; the port never ported the clutter *suppression* policy (only the placers); road decal lifts are stale. Root cause is version skew — the port was baselined at `102852b7` and never re-synced past `dd67ed91`. | Blocks all scoring. |
| **The env-bake test is a wrong test.** `set_time_of_day` bakes eagerly, so the re-derive gate can never fire for a still sky. The original does the same. Fix the expectation, not the code. | Not a lighting bug. |

## Wave 2 — builders in flight

| system | files owned |
|---|---|
| determinism | `scene/game.rs`, `rng.rs`, `engine.rs`, `registry.rs` |
| world | `world/**` |
| sky + tone | `scene/wiring/look.rs`, `scene/wiring/sky_draw.rs`, `scene/boot.rs` |
| harness | `scene/console.rs`, `scripts/parity_shot.py` |
| materials | `scene/install.rs`, `materials/**` |
| weapons | `weapons/**`, `scene/wiring/{weapon_look,weapons}.rs` |
| fx | `fx/**`, `scene/wiring/{fx_draw,fx_audio}.rs` |
| ai | `ai/**`, `scene/wiring/ai.rs` |
| ui | `ui/**`, `scene/wiring/hud.rs` |
| shadows *(worktree)* | `crates/axiom-host`, `modules/axiom-render-pipeline`, `modules/axiom`, `modules/axiom-gpu-backend` |

## Landed, awaiting integration

- **`engine/shadow-bias-units-and-aspect-assertion`** (`7c70c33e`, worktree
  `.claude/worktrees/agent-addcc9429a0b73c80`). Shadow bias restated in shadow
  TEXELS rather than a constant in NDC — recovered from `light_vp` itself, so no
  new uniform and no contract change. The old constant measured 1.80 texels at a
  1024 atlas and 3.60 at 2048 (same NDC number, double the peter-panning, because
  an NDC constant cannot see the atlas); it is now 1.75 at both. Plus the
  penumbra table rewritten to carry both aspects and asserted from
  `shadow_volume` itself instead of being a comment.
  Verified before the freeze: architecture check exit 0; render-pipeline 18/18;
  gpu-backend 818/4 and offscreen 977/7 — **identical to the same suites with the
  changes stashed**, so a measured zero delta. Dylint NOT run.
  **Hold until `engine_no_branching` can be checked.** Reasoned zero (the Rust
  delta is doc comments, two `#[cfg(test)]` fns, and the body of a `&str`), but
  reasoned is not measured, and this is spine code.

## The cascade blocker — the brief was wrong, the agent was right

Widening `FramePacket` to four `light_view_proj` matrices is **illegal**, not
merely large. `cascade.rs` is in `axiom-gpu-backend`, an engine module with
`allowed_modules = []`; the fitting code is in `axiom-render-pipeline`, a feature
module. There is no legal edge, so render-pipeline cannot produce four matrices.
Widening would create four slots with no producer — the "one cascade in four
identical slots" outcome — and each new lane would still owe 100% coverage.

**The correct shape inverts the plumbing:** carry the camera intrinsics (fovy,
aspect, near, far, camera world) UP into the frame packet and let the backend fit
its own cascades. The sun is already in the packet. Every cascade matrix and split
lane stays backend-internal, so `FrameOutcome`, `RenderReport` and all app call
sites are untouched. Written up in `docs/work-manifests/shmup-port/notes/csm.md`
§6b. This is not the shortcut that section warned against — that was *inverting*
`camera_view_proj`; stating intrinsics as first-class frame facts is the correct
contract.

Second unbound port found: `frame_graph` already carries a per-tier `CsmConfig`
(3-4 cascades, clamped 2048) and `schedule::plan` already emits
`FramePass::Cascades` against a `StepTarget::ShadowAtlas` with
`layers: csm.cascades`. Like `cascade` itself, nothing references it.

Corrected caller counts: `FrameOutcome::light_view_proj` has **5** non-test
callers, `run_web_multi` has **6** — not the nine the brief assumed.

## SOLVED — the port now builds the original towns layout

The RNG fork order is fixed (`scene/game.rs`). The original runs one prepare pass
over `registry.resolve()` order (`apps/shmup/src/core/engine.js:143-145`), ten
forks: render, physics, **world**, player, weapons, weapons(viewmodel), fx, ai,
ui, audio. The port took only seven and made `build_level` first — so its world
drew from fork #1, which is the sources RENDER stream. Verified live with
`node tools/rngprobe.mjs --port=8087 --trace`, matching the committed golden.

Witness, same binary, same tree:

| root fork | staticTris | instances | drawCalls |
|---|---|---|---|
| #1 (old) | 584465 | 295 | 56 |
| #3 (correct) | 585336 | **308** | **62** |
| `rng-golden.json` | 585630 | **308** | **62** |

Instances and draw calls match the golden exactly. **This unblocks scoring.**

Residual **CLOSED**. `staticTris` lands on 585630 exactly. Two causes, both cited:
a hoisted `sr.int(1, 3)` loop bound in `buildGround`s seam pass (230 tris,
`ground.js:205` — the source puts the bound in the loop CONDITION, so it redraws
before every iteration including the one that fails) and an f32-narrowed width
flipping `Math.round(11.4 / 1.2)` from 10 to 9 on four buildings jagged parapets
(64 tris, `util.js:480`). Pinned by
`the_static_triangle_count_matches_the_golden`, un-ignored and passing.

Found by a **per-emit trace** — every `Assembler.add` with its palette key and
triangle delta, from BOTH sides, the originals taken by running its JS headless
under Node. 17,133 entries, matching entry for entry. That instrument also caught a
defect no count could ever find: two `rng.pick` calls drawn AFTER their transform,
so each cushion gets the colour that belonged to its neighbours x-offset — same
four draws, zero triangle delta, wrong only in the frame.

Correction to earlier notes: the "original world stream `2835107428,…`" is the
world forks INITIAL state; `rng-golden.json`s `systems.world` is the POST-BOOT
state. Both consistent.

## World skew — closed

The whole `102852b7..HEAD` world skew is **556 lines across 7 files**, in only
three of the nine commits (the rest is the boot/fidelity program). All ported:
`clutter.rs` (53 suppressed ids, the arena-floor policy, `?clutter=1` restore),
`road_y`/`camber` hoisted, `DECAL_LIFT = 0.002` at four sites, the manhole
re-seated to `road_y(x, -0.012)`, the eight `drop()`ed set-pieces, and the
vehicle-mark mutes.

Deliberately NOT ported, consistent with a pre-existing recorded decision:
`settleLights` and `?maxlights=N`, both of which hang off the Three
light-ballast machinery the port already dropped as a shader-permutation
workaround with no Axiom analogue.

**RNG neutrality is argued structurally, not just tested:** every mute gate sits
on a function (`add`, `add_once`, `collide_box`, `light`, `place`) whose signature
takes no `&mut Rng`, so an early return cannot skip a draw — enforced by the
parameter list, not by convention. `muted(f)` always calls `f`. Both arms of every
policy branch call the same body with the same arguments.

**The one place the stream legitimately moves:** suppressing the wreck skips
exactly one `rng.float()`, because `dressing.js:628` puts `driftBerm`s draw
INSIDE the suppression block. The agent initially hoisted it out "for determinism"
and reverted — hoisting would diverge from the original. **Any golden capturing
the world stream after `dressStreet` must be re-captured from `apps/shmup` at
HEAD, not at `102852b7`.**

Queued hand-off: `src/scene/furniture.rs` should be **deleted**. Its own module doc
says to delete it when `dressing.rs` lands; `dressing` has landed, `ax refs` shows
no production caller, and every prototype it places is in `GROUND_CLUTTER` so it
emits nothing under the shipping policy. Left in place only because it sits
outside the world agents subtree.

## The tone scale — solved, and the factor everyone missed

`display()` (Reinhard at exposure 1.0) is gone, replaced by a linear
`scene_radiance()`. The correct scale is **NOT** `1/KEY_INTENSITY_FULL_SCALE`:

> **`SCENE_RADIANCE_SCALE = pi / KEY_INTENSITY_FULL_SCALE = pi / (5.12 * 1.55) = 0.3958662`**, linear, UNCLAMPED.

Two independent factors force it:

- `1/KEY_INTENSITY_FULL_SCALE`, because `key_light()` divides the sun by it and the
  composite multiplies the whole target by `KEY_INTENSITY_FULL_SCALE * METERING_FIT`.
- **`pi`** — the one two separate diagnostic agents AND the integrator missed.
  Three Lambert BRDF carries the `1/pi` (a lit surface writes `b = I/pi`); the
  engine `LightingModel::LambertSpecular` does not, and its own doc says so ("a
  physical surface lit by the same light is ~PI times dimmer"). `scene_wgsl.rs:1012`
  confirms: `lit = base.rgb * lt.col.rgb * lt.col.w * diffuse`, no `1/pi`. So an
  engine-lit surface is pi x brighter for the same light, and every radiance beside
  it must be pi x brighter or it sinks **1.65 stops**.

**The clamp claim was wrong.** `Ratio::finite_or_zero` does NOT clamp — its doc:
"finite values (including HDR magnitudes above 1.0) pass through unchanged". The
0..1 ceiling was purely this apps own `ratio()` helper. So: no type change on
`SkyRadiance`, no `app.rs` hand-off, and **zero display-referred conversions left
in the path** — the correct end state for an HDR target.

Measured natively before the freeze (HOUR now 16.5, was 9.5):

| quantity | old | corrected |
|---|---|---|
| horizon, stops under a lit white surface | 1.763 | 2.709 |
| zenith | 3.538 | 4.765 |
| sun disc body colour | 0.997 (crushed) | 133.79 |

`METERING_FIT` 2.11 -> **0.14243** (exposure 16.745 -> 1.1301), **DERIVED NOT
MEASURED**, bracket **[0.142, 0.232]** (~0.7 stop). The grey-card anchor and the
sources real log-average meter are different instruments; the true fit should sit
in the upper half. Fit so `parity_shot`s `score.meanLuma.ratio` reads 1.0.

Also: `halo_fit` is now an exact dimensionless fraction-of-the-disc, because with a
linear scale `SCENE_RADIANCE_SCALE` cancels top and bottom. Magnitude changed by
orders of magnitude — arithmetically right, visually unseen. **Look at the sun.**

## THE ONE LINE THAT UNBLOCKS SCORING

`scene::console` already implements the camera pin (tests pass), but it is **not
called from `boot.rs`s frame closure**, so `parity_shot.py` will report
`camera: UNPINNED` and every number is taken across mismatched framings. In the
frame closure, before `write_camera(&mut scene.app, pose)`, override the pose with
the console scripted camera when one is in force. Owned by the console agent.

## BLOCKER — the original will not reach `__READY__` here

Under `?capture=1&lockstep=1` the original stalls at `[boot] prewarm.scene`
(`{ok:true, ms:2704, compiled:27}`) and never raises `__READY__`. `__ENGINE__` is
still `undefined`, which places the stall at `main.js:426`s `await startPrewarm()`,
BEFORE the `__PUMP__(3)` handshake. Ruled out: rAF is healthy (382 callbacks in
2 s) and a hand-called `__PUMP__(1)` returned `1`. **The shader pre-warm is the
stall.** `?prewarm=0` is the originals own lever; the one attempt hit
`ERR_INSUFFICIENT_RESOURCES` from machine load, so it is UNTESTED.

Until this clears there is no deterministic reference leg and therefore no score.
First thing to try after the freeze lifts.

Related trap (matches a known repo gotcha): when the wasm build fails,
**axiom-serve starts anyway and serves the LAST GOOD bundle**, so every later step
silently measures old code. Never proceed past a failed build in the log.

## The harness — written, unrun

`scripts/parity_shot.py` (PEP-723; playwright/pillow/numpy, owns its own Chromium,
`--use-angle=gl` never `metal`). Emits `.original.png`, `.port.png`, `.diff.png`,
`.report.json` with `score`, `pins` and `town`. **UNRUN, and not even
`py_compile`d.**

Console: `cam x y z yaw pitch [fov]` / `freeze on|off` / `dt <s>` / `stats`.
**20 native `scene::console` tests pass** — verified before the freeze, the one
piece of this that is.

Two properties worth preserving:
- **Pins report IN FORCE, not requested.** `applied=yes` only after a frame really
  went through `resolve_camera`; `dt_used=UNOBSERVED` means the loop never read it.
  The wiring is `wasm32`-only and untestable, so "hook exists" and "hook is wired"
  are different facts, and a harness that cannot tell them apart prints a confident
  number for an unpinned run.
- **The level fingerprint is free.** `install.rs` already tags every placement, so
  the census is an order-independent FNV-1a over the tag set. Cross-app comparison
  parses the originals own boot line: `586k static tris, 115k instanced in 308
  instances, 62 draw calls, 37.8k collision tris`.

Not pinnable: **time of day** (no console command; `look::HOUR` is a source
constant, now 16.5 — so `sunset`/`night` read DIVERGENT and their numbers are not
parity numbers) and **frame INDEX** (`dt` pins the step; the rAF cadence belongs to
`run_web_multi_skinned` in `axiom-windowing`, out of scope).

**Five `boot.rs` inserts (A-E) are the whole hand-off** — anchors verified verbatim,
in the agent report. Without them `stats` answers `UNOBSERVED`/`UNPINNED` and names
exactly which insert is missing.

## THE BIGGEST FINDING — the vertex-colour lane is an engine gap

`apps/shmup/src/materials/masks.js:11`: the mesh `color` attribute is
**r = wear, g = grime, b = extra AO, all channels default to 0**, which the
material shader treats as "no effect".

`modules/axiom/src/app/resources.rs:131` writes **`1.0, 1.0, 1.0, 1.0`**, and
`MeshGeometry` (`mesh_geometry.rs:20-27`) has **no colour stream at all**, so no
app can supply one. **42 of 46** shmup palette keys set `vertex_masks: Some(true)`.

With `vColor = (1,1,1)` the weathering stack saturates:
`weathering.rs:233` `stain_m = smoothstep(0.58, 0.98, vcolor.g)` = 1.0 -> `stained`
saturates on every near-vertical face -> `:254` replaces wall albedo wholesale with
`rusted`, built from `grime_col` `0x2A2620` = linear **(0.023, 0.019, 0.014)**,
effectively black. Plus `masks.rs:192` `ao * (1 - wear[2])` = **half the AO,
unconditionally**.

**This is the largest single thing between the port and the reference, and it is
not in the material maps at all.** The fix is a colour stream on `MeshGeometry`
plus `vColor` semantics selected by `vertex_masks` — an engine change. Making the
neutral `(0,0,0,1)` would be wrong for every other app, since white IS the correct
identity for a plain vertex-colour multiply.

## The neutrals are wrong (engine, one line + a moved test)

`modules/axiom-gpu-backend/src/scene_renderer.rs:1224` writes macro
`[128,128,128,255]` while its own comment two lines above says mid-grey is the
identity. Three consumers read 0.5 as the midpoint — `macro_variation.rs:221`
(permanent **+0.08 roughness**), `masks.rs:168` (`wearN` pinned at **1.0**),
`weathering.rs:229/347` (rain streak pinned at **1.0**, and `ow_runoff` gets a
constant +0.5 phase). So the street runs at maximum wear with no spatial
structure.

Fix: `-> vec![128, 128, 128, 128]`. This is a deliberate amendment to the
compatibility contract — it moves every un-mapped materials frame, and
`a_material_with_no_maps_matches_one_that_binds_the_neutrals_byte_for_byte`
writes the expected bytes longhand, so that test moves with it.

The detail neutrals `a = 0` is documented as safe because "`owDetailP.z` is 0 for
any material with no detail block" — **true of the engine, false of this app.**
42 of 46 keys author real detail blocks, so `micro = -1` is a permanent full
trough at every near-field pixel.

## Material packing — my hypothesis was WRONG

The ports packing is **correct on every channel** (detail `.xy` normal, `.b`
micro-albedo, `.a` micro-height; macro rgba). Colour space right too — bindings
4/5/6 upload `Rgba8Unorm`, never sRGB, which is what `linearAlbedo: true` requires.

**Measured** (native release, bake probe): detail mean `127.5,127.5,132.7,134.4`;
macro mean `127.5,126.0,127.5,127.7`; both span ~0.20-0.79. **Both centred on 0.5,
neither systematically dark.** "The port packs it wrongly" is disproved, not merely
unverified. No NaN/Inf path exists in any of the five layers for texels in [0,1].

## Bake size — one knob, two budgets

`RUNTIME_BAKE_SIZE = 64` caps **nineteen per-surface bakes** AND **two shared
bakes** through one number. The source authors the detail tile at 1K and says why
(`materials/index.js:198-199`): *"the micro tooth is 1.6-4 mm over a 0.25 m tile,
which needs ~6 texels per grain to survive mip 1 instead of averaging to flat
grey."* At 64 the tile is **3.9 mm/texel — 16x below that floor**, so the micro
tooth mips to flat grey exactly as predicted.

**Raise the SHARED cap first** — two bakes, ~an eighth the cost of taking the whole
library to 128. Needs a second parameter on `bake_library` (touches `scene/app.rs`,
`gpu_bake.rs`).

Native extrapolation from the docs own 512-squared measurement: 64 -> 3.6 s,
96 -> 8.2 s, 128 -> 14.5 s, 192 -> 32.6 s, 256 -> 58 s. **Extrapolated, native, not
wasm.** The number that decides it is the PAGEs boot to first painted frame.

## POM / detile verdict

- **detile: DO IT.** Its mask is sampled from `material_macro_tex`
  (`compose.rs:343`); with a 1x1 neutral that mask was constant, so the forcings
  reason was CORRECT and binding 6 is exactly what retires it. Cost: one extra
  pipeline permutation plus a second full sample set.
- **POM: HOLD.** The premise that no height is bound was **already false** — POM
  marches the albedo alpha (`compose.rs:266` + `pom.rs:137`) and `bake.rs:327`
  writes height there, so it was bound before the ORM upload existed. It will not
  step (anisotropic forces linear mag, so it marches a smooth ramp) but at
  **33 mm/texel** there are 2-6 texels across a whole brick. Downstream of bake
  resolution, not of the binding.

Also: **the ORM alpha is never read.** `compose.rs:283` samples binding 4 as `.rgb`.
The ORM binding never unlocked parallax and its absence was never what blocked it.

## Weapons — status doc stale, two real defects fixed

All four "Remaining #3" files (`viewmodel.js`, `hands.js`, `materials.js`,
`index.js`) are **ported, wired and faithful** — 15,694 lines across 24 files. The
lag springs, the 10% bone-length cheat, and the rig-space pole vector are all
constant-for-constant. The rifle was untextured because `install_rifle` bound a
9-entry debug palette, not because materials.js was missing; `WeaponLook` already
replaced that.

**The trigger test was a wrong expectation.** `system.rs:1666` runs `run_trigger`
BEFORE computing `trigger = input.fire() && can_fire()`, and `try_fire`s guard is
character-for-character identical to `can_fire`s. So after `run_trigger` returns,
`can_fire()` is false down every path (fired -> `fire_timer` set; dry -> `fire_timer
= 0.25`; blocked -> by the same condition). **In `auto`, `state.trigger` is
identically false in the port AND in the JS original.** The rifle starts in `auto`.
Assertion inverted (now pins the real invariant: `run_trigger` must precede the
computation, and the two guards must stay identical), plus a new `semi` test that
preserves the original intent.

**A colour was being deleted.** Copper F0 `(2.25, 1.4, 1.09)` — all three channels
above 1.0 — clamped channel-wise to **pure white**, neutralising the bullet jacket.
Brass `1:0.687:0.322` became `1:1:0.74`. Now scaled by peak channel: chromaticity
exact, magnitude sacrificed (it was never surviving an sRGB u32 anyway).

**Weapon bake resolution.** `detail[0]` (detail tiles per base tile) is 9-30 for
weapon entries. At 64 the finest gets `64/30 = 2.1` texels/cell — **at Nyquist, so
the detail layer cannot be represented**. `RUNTIME_BAKE_SIZE`s justification does
not transfer: it reasons in metres-per-texel at 5 m, not texels-per-detail-cell at
0.4 m, and it leans on weathering layers that `materials.js` **switches off** for
this table (`BASE.weather = [0,0,0,0.62]` — world-Y keyed, meaningless on a
camera-parented object). Raised to 128 above `Quality::Low`. Boot cost unmeasured;
dial-back is one line, and if cheap the table wants 256.

**Independent confirmation of the vertex-colour gap.** Every weapon material sets
`vertexMasks: true` and its edge wear is a per-vertex curvature mask — so **the gun
has no edge wear at all**, no bare metal on chamfers. Same root cause the materials
agent found darkening walls, reached from a different subsystem.

## CONFIRMED — the wasm build was already broken

`logs axiom-shmup`: `initial build failed … exit code 0xc000013a` — the
memory-pressure kill — **before this sessions edits**. axiom-serve then started
anyway. **Port 8088 has been serving a stale or error bundle**, so every frame
captured after that point was of old code. This explains the unattributable black
frames two agents reported. Rebuild before believing any capture.

## AI — ported, wired, and reaching a draw call

All **thirteen** "Remaining #5" files are ported (16,890 lines), plus `bake.js`
folded into `textures.rs`. Third subsystem where `05-port-status.md` is stale.
Wired at `game.rs:323`, `app.rs:252`, `draw.rs:78`, `boot.rs:265`.

**Soldiers reach a skinned draw call** — traced hop by hop, unit-pinned at every
step except `submit_skinned_draw` -> GPU. **But a fresh capture correctly shows
none:** `populate` does `retain(|e| e.2 > 18.0)`, keeping only spawn points MORE
than 18 m from the player. They are placed to be found, not photographed. Sources
own behaviour (`index.js:483-538`). Not a regression; do not chase it.

**`debugStage` was dead.** Body existed (`system.rs:2320`); the dispatcher did not.
`index.js:1107-1109` guards on the name and `prewarm.js:654` passes `none`
specifically to hit the no-op path. Now ported, with a test asserting six staged
men, one prone, all 2-60 m from camera, at least one surviving `lod_irrelevant`.
**Needs a `stage` arm in `console.rs` to be reachable** — highest-value remaining
change in this subsystem, and it must also push the returned 17.9 h into
`SkyDriver` (the source drops the sun so characters are lit, not silhouetted).

**Structural finding: `src/ai/` has ONE test in 16,890 lines.** No `tests/ai_port.rs`,
while core/materials/physics/player/render/weapons each have dedicated `*_port.rs`
goldens. Legal (apps are outside the Coverage Law) but every doc-comment claim is
unchecked.

Also carried across constants that were in NO Rust file: `grounding.js`s
contact-shadow material (tint `(0.045,0.05,0.062)`, opacities 0.62/0.85,
`renderOrder 6`, `depthWrite:false`, `DoubleSide`, `toneMapped:false`). Guessing
these fails specifically — additive blend paints grey discs, a tone-mapped colour
drifts with exposure, a depth-writing quad z-fights the road.

More baked-then-discarded work in `soldier_draw.rs`: the **rim term**
(`bake.js:507`, zero call sites — limbs wash out against bright sky) and **both
detail tiles** (`textures.rs:1486` never called — no 1.5 mm weave inside ~3 m,
~2 MB and a slice of boot bake spent on nothing).

`owNoShadow` survived as a VALUE but nothing consumes it — `submit_skinned_draw`
has no caster flag, so port soldiers cast uniformly. Note the port uses
`lod_irrelevant` to skip the whole draw where the source only drops shadows; this
is SAFE (`visible` is a superset of in-frustum) — **do not remove that filter
without adding a real frustum test first.**

## FX — fully ported; and there was no NaN

Fourth stale headline in `05-port-status.md`. `index.js` -> `system.rs` (2032) and
`ambience.js` -> `ambience.rs` (940) are both ported AND reach a pixel. Only
`haze.js`s screen-space refraction is genuinely unported — it needs a half-res
render target and a warp pass. **Haze is not a hero-shot factor** (heat shimmer off
explosions); the ambient FX that show on a quiet street are **motes and decals**.

**The failing test was NOT a NaN.** `is_finite()` is TRUE; the failing conjuncts
are `alpha > 0.0` and `size > 0.0` — an uninitialised spawn slot read as live.
`particle_points` walked `0..capacity` instead of `instance_count()`. A zero-filled
slot has birth 0 and `1/life = 0`, so `t = now`, `n = t*0 = 0`, and it passes BOTH
of `integrate`s early-outs (`particles.rs:403` = `particles.js:99`) reporting alive
with `size = 0`, `alpha = 0`.

The GPU never sees this because it never runs the vertex shader past
`geometry.instanceCount` (`particles.js:286,430`). **That bound is load-bearing and
the CPU readback dropped it.** Diagnosis proven exhaustive: all 60+ `size0`/`size1`
assignments across seven emitters are strictly positive, so `size == 0.0` can ONLY
come from a never-emitted slot.

Second trigger: the test sampled at the exact emit instant, and
`smoothstep(0.0, 0.045, n)` is exactly zero at `n = 0` — every zero-delay particle
is invisible on its own birth frame, in the source too.

Side effect, measurable: `FxDraw::frame`s particle loop drops from ~23,000
integrations per frame (5 layers x ~23k ring slots, each paying an `exp`, six
`sin`/`cos`, two `powf`) to roughly the live count. Not a visual change — the
phantoms were already alpha-rejected by `PARTICLE_ALPHA_FLOOR`.

**WATCH THIS ONE:** `scene/game.rs:1232` asserts `!particle_points().is_empty()`.
Before this change it passed **for a false reason** (phantoms guaranteed non-empty).
It should still hold — `ambience.rs:557` fills all 240 motes on the first two frames
with NEGATIVE delays so they are already mid-life — but it is the test to watch.
Its comment claiming "muzzle flash, at minimum" is wrong regardless:
`particle_points` deliberately skips the view layers.

## Three.js workarounds — verified against the ENGINE, do not port

- **`lights.js` intensity-parking.** `lights.js:6-9` names the reason: forward
  rendering recompiles every material when the visible light count changes. Axiom
  has no such key — `scene_renderer.rs:674-681` is a **fixed-size UBO with a runtime
  count** (`MAX_LIGHTS = 16`). Nothing recompiles. The ports own parking-at-y=-1000
  is a workaround for a DIFFERENT Axiom limit (`PointLight` is a `Bundle`, so
  intensity is frozen at spawn).
- **"fx self-warms on frame 2."** Same root cause; the key does not exist. The port
  computes `prewarm_due` and nothing consumes it. **That is correct — leave it.**
- **`ParticleSpawn::stretch`** — clip-space quad-corner smear; no clip-space corners
  in a pooled-node billboard path. Already documented as dropped.

Engine-tier gaps found: additive `BlendState` exists with **no call site**
(`fx_draw.rs:63-68`), so additive FX are faked as emissive alpha-blended quads; and
`SurfaceKind::code()` excludes `MaterialParams` from the program-cache digest, so
every runtime material in the process shares one parameter block.

## Open regressions

- **Binding `detail` + `macro_field` blacks out the entire 3D scene.** HUD
  survives; no console error, no panic; bind reports healthy
  (`BrowserWebGpu`, `hdr targets = true`, `scene target = Rgba16Float`).
  Reproduced and reverted by the integrator. No error plus a black frame means
  the maps are consumed wrongly, not missing. The two neutral fallbacks differ
  in alpha (`detail (128,128,128,0)`, `macro (128,128,128,255)`), which points at
  alpha being load-bearing. Handed to the materials agent.

## The three failing lib tests (of ~697)

All three were added by `cf56a515`, the port's last commit before deletion.

| test | symptom | owner |
|---|---|---|
| `look::…the_first_frame_derives_the_radiance…` | "the env bake gate fired" | sky+tone — wrong expectation |
| `weapons::…holding_the_trigger_drains_the_magazine…` | "the trigger bit never went true" | weapons |
| `fx_audio::…the_particle_readback_reports_live_world_particles` | non-finite particle position | fx — a real NaN |

## Scoring

Byte-equality is ruled out (different renderers —
`docs/work-manifests/shmup-port/10-convergence-plan.md`). The target is a scored
convergence: `meanDelta`, `changedPct`, `maxDelta`, per-side `meanLuma` and their
ratio, plus per-region means over `probe.mjs`'s `SHOT_REGIONS` so lighting and
grade separate from geometry. Shot vocabulary is `apps/shmup/src/dev/shots.js`
(11 named shots); only `hero, interior, detail, sunset, night` have port
equivalents.
