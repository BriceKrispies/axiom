# Claude of Duty

Get updates [here](https://shumer.dev/newsletter).

A first-person shooter built in the browser with Three.js r180 and WebGL2. Roughly
55k lines across 11 subsystems, written by a fleet of AI agents under orchestration.

**There are no art assets.** Every texture, mesh, animation and sound is generated
procedurally at load time from code. No models, no HDRIs, no image files, no audio
files. The only runtime dependency is `three`.

```bash
npm install
npm run dev          # http://127.0.0.1:5173
```

Click the canvas to lock the cursor. WASD move, mouse aim, LMB fire, RMB ADS,
R reload, Shift sprint, Ctrl crouch, Space jump, Q/E lean, Esc release.

## What's in it

| subsystem | what it does |
|---|---|
| `render` | HDR pipeline, cascaded shadow maps in a `sampler2DArray` with texel snapping and PCSS contact hardening, MRT depth/normal/velocity prepass, GTAO, TAA with YCoCg variance clipping, tile-dilated motion blur, Karis bloom pyramid, GPU EV100 metering, procedural 33³ grade LUT, AgX composite |
| `materials` | GPU texture forge: 19 procedural surfaces (concrete, brick, plaster, asphalt, sand, rusted/painted/brushed metal, wood, fabric, burlap, glass…), periodic noise so everything tiles seamlessly, Sobel height→normal, parallax occlusion mapping, triplanar projection, curvature-driven edge wear |
| `sky` | Atmospheric scattering, time of day, PMREM environment generation, volumetric fog and light shafts |
| `world` | ~120×120 m market street: modular building kit with real wall thickness, enterable interiors, several hundred instanced props |
| `physics` | Written from scratch, no library. Binned-SAH BVH (29k tris → 14k nodes in 22 ms, 0.25 µs/raycast), swept-capsule character controller with a 5-plane crease stack, impulse rigid bodies with CCD, PBD ragdolls, multi-layer bullet penetration |
| `player` | Movement state machine, slide/mantle/lean, camera feel |
| `weapons` | Procedural weapon geometry, viewmodel rig, ADS, spring recoil, procedural reloads, ballistics with travel time and drop |
| `fx` | GPU particles, decals, tracers, muzzle flash, explosions |
| `ai` | Skinned soldiers, navmesh pathing, perception, cover behaviour, ragdoll death |
| `ui` | DOM/CSS HUD: crosshair, hitmarkers, minimap, compass, killfeed |
| `audio` | Web Audio synthesis — no sound files. Layered weapon fire, convolution reverb, HRTF spatialisation, occlusion |

`ARCHITECTURE.md` is the contract the agents worked against: subsystem interface,
directory ownership, the cross-subsystem event vocabulary, and shared surface types.

## Tooling

The interesting part of this repo is arguably the harness, not the game.

| tool | purpose |
|---|---|
| `tools/capture.mjs` | Screenshot one named shot via GPU-backed headless Chromium |
| `tools/shotset.mjs` | All 11 shots in one session — fast review set |
| `tools/baseline.mjs` | **Reproducible** capture: each shot in an isolated page, fixed frame budget. Bit-identical across runs |
| `tools/imagediff.mjs` | Per-pixel gate. Exits non-zero if any pixel moved |
| `tools/profile.mjs` | Gameplay profiler at real device pixel ratio. Frame-time *distribution* and hitch attribution via per-frame WebGL program counts |
| `tools/playtest.mjs` | Scripted movement/fire smoke test |
| `tools/bootprofile.mjs` | **Boot** profiler. Span tree of everything before the first frame, crossed with V8's CPU profiler and a WebGL call probe, so each phase reports JS vs blocked-in-WebGL vs idle. `--programs` explains the shader permutation population |
| `tools/rngprobe.mjs` | Per-subsystem RNG stream gate. Catches a reseed in under a minute, where the pixel gate takes six and only says "everything changed". `--trace` names the line that moved a fork |

Two findings worth recording, because both invalidated earlier measurements:

**Median frame time hides the actual problem.** A static-camera benchmark reported
94 fps while the game was unplayable. Real gameplay at Retina DPR (internal 3.34 MP,
not 2.07) ran 12–17 fps with **728–1236 ms stalls** caused by 34+ WebGL programs
compiling lazily mid-frame. `profile.mjs` reports p50/p95/p99 and attributes each
hitch, which is what surfaced it.

**Boot is two unrelated problems, and one of them is not slow code.** `bootprofile.mjs`
splits every phase into JS / blocked-in-WebGL / idle main thread, which separates them:

* ~9 s of genuine main-thread CPU work — procedural texture bakes (`ai/textures.js`
  alone is 2.6 s), mesh building, the nav grid. Optimisable, cacheable, or movable
  to a worker.
* ~1–30 s of **idle**. The pre-warm is 95 % a parked main thread polling
  `KHR_parallel_shader_compile` every 10 ms while the GPU driver links ~220
  programs in the background. No code in that window is slow; the app simply
  declines to start the frame loop while it waits.

The spread on the second number is not noise, it is **which shader cache was hot**,
and the one that matters is not the browser's. Measured on the same machine:
driver cache emptied 54 s · fresh browser profile 11 s · full reload 10 s. The GPU
driver's on-disk program cache is per *machine* and keyed on shader source, so
editing a shader evicts exactly what you touched — which is why a slow boot is
reproducible for whoever is editing shaders and for nobody else. `--icy` empties it
so a first-visit boot can actually be measured.

**The first frame and the finished load are two different numbers.** Boot used
to build everything, pre-warm every shader, and only then start the loop. Now
`init()` builds what frame 1 needs and each subsystem declares the rest as a
`stream()` generator that the engine drains a few ms per frame with the game
already on screen — the weapons the player is not holding, the navigation grid
for enemies that have not engaged, the shader pre-warm. Measured on a production
build: first painted frame 2.9 s on a reload, fully loaded 5.4 s. `?capture=1`
drains it all before `__READY__`, so the pixel gate still compares finished
worlds.

Two things had to be true first, and both were found by measuring:

* **Pre-warm had to stop moving the camera.** Its four warm-up poses existed
  only because the visible light count is part of three's program cache key and
  the count depends on the camera's distance cull. Pinning the count before
  anything compiles made the poses dead weight — 108 programs with them, 108
  without — and a pre-warm that never touches the camera is one that can run
  while the game is being played.
* **A subsystem that retries in `update()` must not race its own `stream()`.**
  `ai` did, and built the grid and the garrison twice: 12 enemies in 4 squads.

**Rust/wasm for the bakes was tried and rejected — 1.18-1.44x.** Worth recording
because the case looked strong and someone will propose it again. `bakeprofile.mjs`
put 54% of the ~4 s of worker bake CPU in `fbm`/`ridge`, pure f64 arithmetic
behind a `seed -> typed arrays` interface crossed once per bake, with no
transcendentals in the hot path. The port (`bake-rs/`, ~200 lines) came out
**bit-identical** to the JavaScript and barely faster, because the noise is
gather-bound rather than ALU-bound: each `n2()` does four random-access lookups
into a 4096-entry table and `fbm` does sixteen, so the limit is memory latency,
which wasm does not change and SIMD cannot help with (wasm128 has no gather).
`node tools/wasmbench.mjs` reproduces it in seconds. The crate is kept as
evidence and is not in the build.

What worked instead, for a fraction of the effort: the wall time of a parallel
bake is its LARGEST SHARD, and the shards were far too coarse — three groups of
~950 ms each. Split to one per texture set (eleven jobs) with a wider pool, the
wait disappeared: `fx:atlases.await` went from ~390 ms to below measurement.

**The loading bar is generated from the profiler.** Most loading bars count
steps, which is a lie whenever the steps differ in cost — here they differ by two
orders of magnitude (`world:gate` is 18 ms, `world:buildings` 562 ms). This one
weights every phase by measured time, and the table is emitted by the same
instrument that measures the boot:

```sh
node tools/bootprofile.mjs --emit-weights   # writes src/core/bootweights.js
```

It rides on the profiler's spans rather than a second set of hand-placed calls,
so a weight table generated from those spans cannot drift out of sync with them.
Four things make it accurate rather than merely weighted: sub-phase motion
through the long phases (one span per building, or the bar stops for 562 ms);
EXACT progress where it exists — the shader link is a count of finished programs
via `isReady()`, not a timer; calibration to the machine as phases complete; and
a monotonic clamp, so re-pricing shows up as the bar slowing rather than
retreating. The one phase that varies by an order of magnitude between a first
visit and a reload re-prices itself from its own first few completions.

The overlay is inline in `index.html`, above the module script, so it is on
screen before the bundle has downloaded — and it comes down at the FIRST PAINTED
FRAME, not at "fully loaded", because the game is playable while the rest streams
in. What is left goes to a corner indicator. `?capture=1` removes it outright:
an overlay in a reference image would make the pixel gate meaningless.

**A cold GPU driver, not the code, is what makes a first visit slow.** With an
empty driver shader cache the first frame took 15.5 s, and `bootprofile.mjs`
attributed 14 435 ms of it to 7054 calls to `getUniformLocation` /
`getActiveUniform`. The driver defers each program link past `linkProgram` and
past `LINK_STATUS`, until something asks for the program's interface — which is
the reflection three does the first time it draws with a program. So the first
frame paid all 109 links serially, on the main thread, and every instrument that
only watched `linkProgram` reported 1 ms.

`compileAsync` before the first draw inverts that: the links run on the driver's
own threads and three polls a completion flag, so the same work happens off the
main thread and in parallel with itself — 14 435 ms of blocking reflection
becomes 2001 ms, and cold first paint drops to 10.3 s. What is left is 6.8 s of
a 95%-idle main thread waiting on the driver, which is irreducible in WebGL2:
there is no program-binary API, so nothing can be cached between visits. Below
that the only lever is fewer or smaller programs.

**Captures were not reproducible.** `shotset.mjs` reuses one page across all 11
shots, so particle age, decal buffers and exposure state leak forward — two identical
runs differed on 10 of 11 shots. `baseline.mjs` isolates each shot in a fresh page,
which is bit-identical and is what makes `imagediff.mjs` a usable gate.

## Performance

Measured on an Apple silicon laptop at 1512×982, DPR 2 (3.34 MP internal), `ultra` preset,
3 runs, gameplay in motion with AI and firing active:

| | before optimization | after |
|---|---|---|
| fps p50 | 12–17 | **28–30** |
| fps p99 | 4–9 | **14–17** |
| worst frame | 728–1236 ms | **66–82 ms** |
| shader compiles during play | 34–35 | **0** |
| boot | ~9–12 s | **3.7–4.6 s** |

The optimization pass was constrained to produce **zero visual change**, enforced by
`imagediff.mjs` rather than by assertion — the shipped build is bit-identical to its
pre-optimization reference across all 11 shots.

Shader pre-warm (`src/core/prewarm.js`) is what removed the stalls. Making it
*provably* pixel-neutral required first fixing subsystems that animated off
`performance.now()` instead of the engine clock, since any change to boot duration
otherwise shifted output.

## Honest assessment

The goal was to match a modern Call of Duty. **It does not.**

Eleven independent adversarial critics scored the frames against that bar. Scores
went 3.59 → 4.14 → 4.05 → **5.05** out of 10. Two shots reached "CLOSE"; the rest
remain "AMATEUR". In a blind A/B, **every critic in every round picked the real Call
of Duty frame.**

Where it falls short, specifically:

- **Hands.** Blocky finger slabs that don't convincingly grip the weapon.
- **Material richness.** Surfaces read as procedural noise rather than photographed
  reality at close range — the ceiling of generating texture from code.
- **Characters.** Enemies read as mannequins at distance.
- **Indirect light.** An approximation, not real GI.
- **Frame rate.** 28–30 fps at Retina. The art passes tripled geometry cost
  (5.9M → 11.3M triangles) and optimization recovered about half.

A known root cause remains unfixed: the viewmodel light rig in `render/index.js`
delivers roughly 20× the irradiance per unit albedo that the world does — a plain
*black* material in the view scene renders at L=110 against a background of 91,
purely from F0=0.04. Every weapon albedo is cheated to a third of physical to
compensate, which caps material separation on the most-looked-at object in the game.

## Process note

Sequential single-owner passes beat parallel fan-out decisively. Three rounds of six
agents each owning one directory moved the score +0.46 and left frame-ruining defects
*higher* than they started (60 → 47 → 66), because tonemapping, sky and indirect light
are one coupled system and isolated agents kept breaking each other's assumptions.
One sequential pass with a single owner per coupled concern moved it +1.00 and cut
defects 66 → 26.

The most valuable single result came from an agent contradicting its own brief. Every
critic for three rounds reported the weapon as "untextured". It wasn't — it was
specular-dominated, with the diffuse term measured at L=26 against a shipped L=67.
Prior rounds had been crushing albedos to fight bright-part complaints, which killed
diffuse and made it worse. The fix was the opposite of what was asked for.
