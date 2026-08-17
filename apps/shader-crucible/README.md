# The Shader Crucible

Ten labelled stations demonstrating Axiom's procedural appearance system — and,
beside the stations they affect, the four things it does not do.

**There is no WGSL in this app, and no Rust in it that computes a colour.** Every
pattern, mask, blend, displacement and density below is a `FieldGraph` over the
27-operator algebra; `src/authoring.rs` is one-line spellings of single
operators and nothing else. `tests/no_wgsl.rs` is a grep test that proves it.

## Why it exists

`apps/burnt-rubber` authors a field graph and **bakes** it, through
`TextureOp::Field`, into an ordinary texture. That was the right call there —
asphalt is the largest surface in any frame — but it means the other half of the
system had no shipping consumer: **graph → `Surface` → `surface_program` → WGSL
→ pixels**. The crucible authors live `Surface`s and takes that path.

## Running it

```sh
# The live browser arm — the only place the GPU renders an authored surface.
uv run scripts/localhost_servers.py start-app shader-crucible --port 8086

# The software arm's pixels, headless, plus the control image with the surface
# set withheld. The difference between the two files IS the surface system.
cargo run -p axiom-shader-crucible --bin crucible_shot -- \
    --out screenshots/crucible-c2d.png --tick 0

# The axiom-shot capture. NOTE: neither of its arms carries a surface (see
# below), so this is the constant-fallback control image. `--release` is
# required on the windows-gnu toolchain: a debug cdylib linking real wgpu
# overflows mingw's export table ("export ordinal too large").
cargo run --release -p axiom-shot --features offscreen -- \
    --app shader-crucible --backend gpu --tick 0 --out screenshots/crucible-gpu.png
```

## The ten stations

| # | Station | Proves | Measured |
|---|---------|--------|----------|
| 1 | Layered material | metal + paint + scratch + dirt, each masked; mask-driven layering flattening to `Mix` | 71 authored nodes; flattened **58 / 50 / 39 / 39 / 39 / 38 / 39** per channel |
| 2 | Live procedural surface | graph → `Surface` → `surface_program` → WGSL, per pixel | 32-node colour, 33-node roughness |
| 3 | Baked texture | the *same* graph through `TextureOp::Field` | agrees with the live evaluation to **0.5 byte levels** worst texel |
| 4 | Parameter retune | nine tunings → one digest → **one program** | 3 slots, 22 nodes |
| 5 | Time-varying displacement | vertex-stage fields on deterministic engine time | 16-node wind, 20-node ripple |
| 6 | Three lighting models | a closed discriminant, zero extra pipelines | 3 digests, 2 pipeline markers, 1 lit program |
| 7 | Implicit surface | a `FieldGraph` as a `ScalarField`, marched | 23-node density → 40³ lattice → **1,410 vertices / 2,816 triangles** |
| 8 | Transcendental patterns | marble and wood over `Sin` and `Pow` | 22 / 26 nodes, 3 slots each |
| 9 | Both backends | per-pixel vs per-triangle-centroid, as a *reported substitute* | `supported_by` is `true` for all 11 surfaces on both real profiles |
| 10 | Introspection | `explain` / `digest` / `diff` | retune diff **+0 −0 ~0**; marble-vs-wood **+3 −0 ~20** |

Eleven surfaces, twelve bodies: station 5 authors two (wind and ripple), station
6 authors three (one per lighting model), station 8 authors two (marble and
wood), and station 3's body carries a *texture*, not a surface. The preparation
barrier compiles **11 programs from 11 surfaces** — one each, no variant
explosion.

## The node budget, measured first

`MAX_LAYERS` is 4, `axiom_field::MAX_NODES` is 256, and a layered surface
flattens into **one graph per channel**. Station 1 was built first for exactly
that reason. It fits, comfortably:

```
BaseColor    58 nodes      Normal       39 nodes
Roughness    50 nodes      Emission     39 nodes
Metallic     39 nodes      Opacity      38 nodes
                           Displacement 39 nodes
```

**The interesting number is not the 58; it is the 39.** Five of the seven
channels are bound to plain constants in every surface of the tree, and they
flatten to ~39-node graphs anyway — because the three layer masks are *fields*,
and `Mix(const, const, mask_field)` has a non-constant input, so it cannot fold
back to a constant. Roughly 195 of station 1's ~302 flattened nodes compute
values that never change. That is the real shape of the `MAX_LAYERS` ×
`MAX_NODES` tension: it is not the number of layers, it is that a field mask
promotes **every** channel to a graph at once. A fourth layer would add ~13
nodes per channel; the ceiling is still far away, and neither cap needs raising.

## What this does NOT do

### 1. A displaced vertex casts an undisplaced shadow

The shadow depth pre-pass is a separate WGSL module and runs no
`axiom_displace`, so the depth it writes is the depth of the **undeformed** mesh.
Station 5's wind body leans while its shadow stays standing, and the gap opens
with height because the wind's amplitude is weighted by object-space `y`. The
station is lit at a low angle onto a visible ground for exactly this reason.
Fixing it means teaching the shadow pass to run the vertex program — an engine
change, not an app one.

### 2. Skinned geometry always gets the default program

`SkinnedGpuDraw` carries no `surface_program` lane at all, and the skinned vertex
stage binds all 16 vertex attributes a WebGL2 downlevel target guarantees — the
ceiling that already costs a skinned material its emissive and its specular — so
it runs no displacement program either.

**The crucible therefore shows no skinned body.** There is no such thing as a
surfaced one, and standing an unsurfaced figure beside eleven surfaced ones would
read as a bug rather than as a limitation. What it shows instead is the backend's
own answer: the barrier calls `skinned_surface_degradations` on its own surface
set and records the result, and the report prints
`skinned degradations: [ProceduralSurface]`.

### 3. Canvas2D shades one sample per triangle

The software rasterizer executes no shader, but a surface's channels are fields
with a reference evaluator, so it evaluates each channel **once per triangle**, at
that triangle's object-space centroid. `RenderCapability::ProceduralSurface` is
therefore on in its profile too: the substitution is the *sampling rate*, not the
appearance.

A mask finer than a triangle is not sampled at all. Station 1's scratch lines are
a fraction of a body wide and can miss every centroid, so they **vanish** on the
software arm. **The meshes are deliberately not tessellated to hide it** —
subdividing until the software arm resolved them would be measuring a mesh
instead of a backend.

Two further gaps the software capture makes visible, neither of them about
surfaces and both worth knowing before reading that image:

* **The software 3D path samples no albedo texture at all.**
  `Canvas2dBackendApi::load_textures` feeds the *2D* `Draw2dList` sprite path;
  the mesh rasterizer has no albedo sampling and the frame reports
  `FrameFeature::AlbedoSampling` as a drop. So **station 3's baked tile renders
  untextured** there — the one station whose subject is a texture is the one the
  software arm cannot show.
* **There is no point light and no tone mapping.** Its whole lighting is a
  hemisphere ambient (weighted 0.6) plus one directional term (weighted 0.5),
  applied linearly. The same authored rig is therefore markedly darker on the
  software arm, and pushing the ambient far enough to match it blows the GPU arm
  out. The rig is tuned for the GPU and the gap is reported rather than papered
  over.

### 4. `metallic` changes no pixel

`SurfaceChannel::Metallic` is a channel, not a BRDF: carried, digested, reported,
and read by no lighting model (SPEC-11's "resist PBR scope creep"). Station 1's
base binds it to `1.0` and its paint and dirt bind it to `0.0`, and moving any of
them moves nothing on screen. It is labelled rather than omitted.

## Two more things this app found, which are not in the manifest's list of four

### The engine's own presentation stack cannot carry a surface

The only public entries that take an authored `Surface` set are
`GpuBackendApi::present_packet_with_surfaces`,
`Canvas2dBackendApi::present_packet_with_surfaces` and
`Canvas2dBackendApi::render_offscreen_rgba_with_surfaces`. **Before this app,
none of the three had a caller anywhere in the repository outside tests.**

Everything an app would normally present through takes explicit instance batches
and passes an empty program slice:

| Route | Surfaces? |
|---|---|
| `axiom-windowing`'s live loop (`App::run`) → `GpuBackendApi::present_frame_result` | **no** — passes `&[]` programs and `0.0` surface time, and never calls `prepare_surfaces` |
| `axiom-shot --backend gpu` → `GpuBackendApi::render_offscreen_rgba` | **no** — there is no surface lane on it |
| `axiom-shot --backend canvas2d` → `Canvas2dBackendApi::render_offscreen_rgba_skinned` | **no** |
| `GpuBackendApi::present_packet_with_surfaces` | yes — wasm only; a no-op on native |
| `Canvas2dBackendApi::render_offscreen_rgba_with_surfaces` | yes — the only public *native* path |

So this app owns `src/frame.rs` (its own `FrameOutcome` → `FramePacket`
translation, carrying each draw's `surface_program` and the frame's engine time)
and `src/web.rs` (its own canvas, device handshake and `requestAnimationFrame`
loop). That is a lot of app code for something an engine loop should do, and the
imbalance is the finding: the live path works, and nothing in the engine's own
presentation stack walks it.

`axiom-shot`'s capture of this app is therefore the **constant-fallback control
image** — twelve white bodies and one correctly-textured baked tile. It is
genuinely useful (it is what these frames look like when the surface lane is
dropped) and it is registered for exactly that reason, but it is not the
demonstration.

### `TextureOp::Field` writes linear bytes into an sRGB upload path

The bake writes `clamp(v, 0, 1) * 255`, rounded — **linear**. The material-texture
upload path binds an app-supplied albedo as `Rgba8UnormSrgb`, so the sampler
*decodes* the byte through the sRGB curve before it multiplies. The same graph,
baked and then sampled, therefore comes back darker than the same graph evaluated
live, by exactly the transfer function. The graphs agree — station 3's test pins
them to 0.5 of one byte level — the two upload conventions do not.
`apps/burnt-rubber` works around this by fitting a cubic sRGB encode into its own
graph; the crucible does not, because hiding the seam would hide the finding.

## What the four captures actually show

| File | What is in it |
|---|---|
| the live browser page | **the demonstration.** Twelve bodies, every one of them shaded by its own compiled program. Console: `the barrier bound 11 surface programs to the device`, then `first frame drew=true, degraded=[]`. |
| `screenshots/crucible-c2d.png` | the software arm with the surfaces **evaluated** — bodies tinted by their own field values, at one sample per triangle, and much darker than the GPU arm for the reasons above. |
| `screenshots/crucible-c2d-fallback.png` | the identical frame with the surface set **withheld** — the same bodies, uniformly grey. The bin prints the difference: the surface set changes **3.2%** of the frame's bytes. |
| `screenshots/crucible-gpu.png` | `axiom-shot`'s GPU capture: twelve **blank white** bodies and one correctly-textured baked tile. The wind cube is un-leaned and the ripple body is a plain sphere, because that path drops the fragment *and* the vertex programs. The control image. |

## The levers under the canvas

A row of buttons sits between the canvas and the diagnostics panel. Every one of
them removes exactly one thing from the frame and holds the rest, so the
difference between two panel readings is attributable to it. They were query
parameters until this row existed, which in practice meant they were never run
on a phone — and the phone is the only device whose numbers here are real,
because Chrome's device emulation does not emulate a phone GPU.

| Button | What it removes | Reload |
|---|---|---|
| `CAPTIONS` | the twelve caption meshes — 12 of the frame's 25 draws |  |
| `SHADOWS` | the shadow pass's draws, and the PCF result | |
| `SURFACES n/11` | the generated program on the bodies past *n*, which then take the constant fallback pipeline | |
| `SOLO` | every body but one, at a **fixed framing** shared by all twelve | |
| `HALF RES` | three quarters of the fragments (the render-scale ladder's floor) | |
| `ADAPTIVE` | nothing — it hands the resolution to `RenderScaleController` | |
| `DEVICE PX` | the backbuffer↔screen match | yes |
| `RESET` | every lever, back to the shipping configuration | if needed |
| `COPY DIAGNOSTICS` | — | |

`SOLO` frames every body from the same offset, so two solo readings differ only
in which shader is on the pixels. That is what makes it a measurement rather
than a viewer: on a desktop WebGPU adapter, solo'ing body 1 (station 1's layered
metal + paint) measures **2.43 ms** in the main GPU pass against **0.05 ms** for
body 7 at identical screen coverage — a factor of ~49 for one material, which is
the whole answer to "why does station 1 halve the frame rate when it fills the
screen".

`COPY DIAGNOSTICS` puts the entire panel — the frame distribution, the CPU
spans, the workload, the per-pass GPU times, the capability profile, which levers
are pulled, and every station's flattened node count per channel — on the
clipboard **and** in the console as one JSON object. It is how a phone's numbers
reach somebody who cannot see the phone. The clipboard path falls back to a
`textarea` copy, because `navigator.clipboard` does not exist in a non-secure
context and a phone on `http://192.168.x.x` is exactly that.

Each button is also a query parameter, so a configuration can be handed over as a
link: `?captions=0`, `?shadows=0`, `?surfaces=3`, `?solo=1`, `?half=1`,
`?adapt=1`, `?dpr=0`, `?back=WxH`.

### Two things the levers measured that nobody had measured before

* **The `surfaces` lever used to be inert.** It narrowed the `Surface` slice
  handed to `present_packet_with_surfaces`, on the belief that a draw whose
  surface was missing would take the fallback. It does not: the startup barrier
  has already bound all eleven programs to the device, and a draw finds its
  program by digest whatever slice the present is given. At `SURFACES 0/11` every
  body still wore its own shader — visible on screen, and measurable as no
  change at all. The lever now cuts the packet's own `surface_program` lane,
  which is what the backend really keys on, and the eleven generated shaders
  then account for roughly **85–90% of the main pass's GPU time**.
* **Clearing `RenderCapability::Shadows` does not make the frame faster.** Across
  five A/B pairs the main pass was never quicker with the bit cleared and was
  usually a little slower. The shader computes `shadow_factor` and then
  `select`s on the result, and `select` evaluates both arms — so the 25
  `textureSampleCompare` taps run either way. See "What this cannot do from the
  app" below.

### What this cannot do from the app

Two shadow costs are unreachable from app code and are named here rather than
worked around:

1. **The shadow depth pre-pass always runs.** `scene_renderer.rs` begins
   `"axiom-shadow-pass"` unconditionally — there is no `caps & Shadows` filter
   around it, unlike the SDF and sky passes in the same function — so a
   full-size depth clear happens on every frame whatever the profile says. What
   the app *can* do, and does, is hand the packet an identity `light_view_proj`,
   which makes the light's culling frustum the world cube `[-1, 1]³`; every body
   on this stand is outside it, so the pass submits no draws.
2. **The 25 PCF taps always run.** `shadow_factor` is called outside the
   capability test because `textureSampleCompare` needs uniform control flow.
   Making the bit a real saving means restructuring the fragment stage, not
   flipping a flag.

The render scale has a smaller one: `RenderScale` exposes no constructor but
`FULL`, so `HALF RES` reaches the ladder's floor by driving a throwaway
`RenderScaleController` down it. A `RenderScale::of(Ratio)` would retire that.
And `GpuBackendApi::render_width()` is the device *tier's* size, not the live
one, so the panel prints the scale it **asked** for and lets the GPU pass times
say whether it took.

## Determinism

Fixed seeds; no wall clock anywhere. Station 5's displacement reads
`EvalContext::time`, which the frame supplies from the engine's own tick count
(`frame::time_at`, `tick / 60`). Tick *N* replayed twice is byte-identical; tick
*N* and *N + 60* differ exactly where a station is time-varying.

## Tests worth having

Apps are outside the 100% coverage gate. These earn their place:

* every station's graph `validate()`s, and every surface's digest is pinned
  (`COMMITTED_DIGESTS`);
* **station 4: retuning every knob leaves `Surface::digest()` identical** — and
  the sharper form, nine tunings handed to the barrier compiling one program;
* the other side of that line: changing a *constant* channel does move the
  digest;
* a node-count assertion per station, so a future edit that blows the budget
  fails a test rather than a frame;
* station 2 vs station 3 agree within one byte level at every texel;
* `supported_by` reports the truth for both profiles before anything renders;
* the barrier's degradation report is non-empty on the skinned path and empty on
  the rigid one;
* the no-WGSL grep test, and its sibling that bans Rust shading maths.
