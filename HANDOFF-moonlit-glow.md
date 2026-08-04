# Handoff — burnt-rubber moonlit glow (GPU shader layer)

Written 2026-08-02. Everything below is verified unless it explicitly says
otherwise. Where I am uncertain, I say so.

## The ask

> "the webgpu version reads dark. i want gentle moonlit glow vibe and with a real
> light source and shaders please. hopefully more in the engine. skip it for the
> canvas2d version"

Two scope decisions the user made when asked, and they are **binding**:

1. **Full engine scope** — bloom **and** a specular term in the shared shader
   **and** "sky as a light" (a gradient sky + moon disc as engine frame data),
   not just a post pass.
2. **The moon is visible, low on the horizon** — a moon you can see down the road
   ahead, which is what makes long raking shadows and a specular streak on the
   tarmac possible.

"Skip it for the canvas2d version" is to be honoured **through the capability
system**, not an app-level `if`: canvas2d must *declare* the drops.

## Where things stand

`main` is at **`89560b17`** (pushed, clean). On top of it there is
**uncommitted work in the working tree**. It compiles, `cargo check --workspace`
is clean, and the tests are green:

| Suite | Result |
|---|---|
| `cargo test -p axiom-host` | 326 unit + 17 architecture, all pass |
| `cargo test -p axiom` | 136 pass |
| `cargo test -p axiom-burnt-rubber` | 503 pass |

**Nothing is committed.** That is deliberate: `RenderCapability::Sky` and
`::Specular` exist but no backend either implements or reports dropping them,
and the Capability Law says a capability is never silently no-op'd. Landing it
as-is would put an incomplete contract on `main`.

Uncommitted files:

```
new:  crates/axiom-host/src/frame_sky.rs
new:  crates/axiom-host/src/frame_bloom.rs
new:  modules/axiom-gpu-backend/src/post_chain.rs
mod:  crates/axiom-host/src/{lib.rs,frame_packet.rs,frame_capability.rs}
mod:  crates/axiom-host/tests/architecture.rs      (curated export list)
mod:  modules/axiom/src/{app.rs,prelude.rs,frame_outcome.rs,app_tests.rs}
mod:  modules/axiom/src/app/{render_look.rs,frame.rs}
mod:  modules/axiom-gpu-backend/src/{lib.rs,offscreen.rs,gpu_backend_api.rs}
mod:  tools/axiom-shot/src/capture.rs
mod:  apps/burnt-rubber/src/render/mod.rs          (authors FrameBloom::moonlit())
```

## The diagnosis — why it read dark

This part is settled and cost real investigation. "Dark" is really **flat**, and
there were three independent causes:

1. **There is no GPU post-process at all on the live path.**
   `FramePostProcess` and `FrameVolumetrics` are *CPU* loops over an RGBA8
   buffer (`crates/axiom-host/src/frame_postprocess.rs:112`,
   `frame_volumetrics.rs:155`). They are applied by the Canvas 2D rasteriser
   (`software_rasterizer.rs:310,315`) and by the **offscreen** GPU capture
   (`offscreen.rs:246,249`) — which reads pixels back anyway. The **live**
   swap-chain frame (`gpu_backend_api::present_frame` → `live.render_frame`)
   never touches either. `upscale.rs`'s `BLIT_WGSL` was a bare `textureSample`:
   the only post slot on the GPU, doing nothing.
2. **Nothing could glow.** Emissive *does* reach the fragment shader on rigid
   meshes (commit `045cbbdf`, `scene_renderer.rs:292` `let emitted = lit +
   in.emissive;`), but the target is 8-bit sRGB with no tonemap, so anything
   above `1.0` hard-clips to white. burnt-rubber authors emissive above 1.0 all
   over `render/palette.rs` — every bit of that surplus was being thrown away.
3. **No specular anywhere in the Rust shader.** Lambert only
   (`scene_renderer.rs:270-292`). `grep specular|Blinn|Phong|roughness` over the
   Rust GPU path returns nothing. No surface can catch a highlight, so no light
   value tuning makes the road read as lit *by* something.

Related engine facts worth not rediscovering:

- **16 lights max**, directional + point only, **no spot**, no light radius.
  Point attenuation `1/(1 + 0.09d + 0.032d²)` is hardcoded.
- **One** directional PCF shadow map (5×5, 25 taps), and its ortho box is fixed
  around the **world origin** with `SHADOW_EXTENT = 20.0`
  (`modules/axiom-render-pipeline/src/shadow_view.rs`). Any action far from the
  origin is unshadowed. burnt-rubber drives 9 km, so **shadows are effectively
  absent for most of the course** — relevant if you plan to lean on moon shadows.
- **Ambient and depth fog are captured at BIND time on the GPU arm**, not per
  frame (`scene_renderer.rs:1071`, set at `live_gpu_binding.rs:110-111`). Sky
  and bloom should follow the same pattern. Changing them mid-run will not move
  GPU pixels.
- `apps/burnt-rubber/ARCHITECTURE.md:587-594` still claims "`with_emissive`
  never reaches the GPU". **That is stale** — `palette.rs:16-45` and its tests
  are the current truth. Worth fixing while you are in there.

## What is implemented

### 1. `crates/axiom-host` — neutral frame data (complete, fully tested)

- **`frame_sky.rs`** — `FrameSky::gradient(zenith, horizon)` +
  `.with_body(direction, angular_radius, color, halo_falloff, halo_strength)`.
  The load-bearing part is **`FrameSky::radiance(view) -> [f32;3]`**: the
  reference arithmetic (gradient + disc + halo) that the GPU sky shader is meant
  to mirror. Branchless (this is a layer). 9 tests.
- **`frame_bloom.rs`** — `FrameBloom::moonlit()` / `::highlights()`, plus
  **`contribution(luma)`** (the quadratic bright-pass knee) and **`tonemap(ch)`**
  (reciprocal highlight rolloff), again as the reference the WGSL mirrors. Also
  `luminance(rgb)` (Rec.709) and `ROLLOFF_KNEE`. 7 tests.
- `RenderCapability::Sky = 1 << 8`, `::Specular = 1 << 9`.
- `FramePacket::with_sky`/`sky()`/`with_bloom`/`bloom()`.

Two real bugs were found writing this — both are the kind that survive review:

- **`lerp` must be endpoint-exact.** `a + (b - a) * t` at `t = 1` gave
  `0.099999994` instead of `0.1`, so looking straight up did not return the
  zenith colour. Now `a * (1 - t) + b * t`.
- **`NaN * 0.0` is still `NaN`.** The obvious branchless fallback
  (`v * usable + fallback * (1 - usable)`) let a poisoned direction sail through
  the guard meant to catch it. `normalize_or` now selects by **table index**
  (`[fallback[c], scaled[c]][usize::from(usable)]`), which never touches the bad
  value.

### 2. `modules/axiom` — authoring surface (complete, tested)

`RunningApp::set_sky`/`sky()`, `set_bloom`/`bloom()` in `app/render_look.rs`,
carried on `FrameOutcome` (`with_sky`/`with_bloom`), captured in `app/frame.rs`
before the render closure borrows `self` (same shape as `depth_fog`).
`FrameBloom`/`FrameSky` added to `axiom::prelude`. Two flow tests in
`app_tests.rs`.

### 3. `modules/axiom-gpu-backend/src/post_chain.rs` — **the bloom chain (WORKS)**

Bright-pass → separable blur (H then V, 9-tap Gaussian, half-res) → composite
with the rolloff. Replaces `UpscaleBlit`'s job: the composite is already a
fullscreen triangle sampling the intermediate with a linear filter, so it
upscales for free.

**Verified rendering.** Wired into the offscreen path only, and the stills prove
it: `screenshots/moon-bloom-tunnel.png` and `moon-bloom-straight.png` vs the
baselines `moon-base-tunnel.png` / `moon-base-straight.png`. Tunnel lamps,
reflector posts, tail lights and lane paint all gained real halos.

Two decisions in there that must not be quietly undone:

- **Two uniform buffers, one per blur axis.** My first draft rewrote *one*
  buffer between passes. `queue.write_buffer` is ordered against the encoder's
  **submission**, not against the passes inside it, so both blur passes read the
  last write — blurring horizontally twice and never vertically. This is
  invisible in a still of a symmetric highlight.
- **The offscreen path skips the chain entirely when a frame authors no bloom**,
  rather than running it at zero intensity. A no-op composite is still a
  sample-and-write round trip through an 8-bit sRGB texture and is not
  guaranteed bit-exact — and every existing capture in the repo is compared
  byte-for-byte (`tools/axiom-shot/tests/*_parity.rs`).

**The 8-bit ceiling.** The intermediate target is 8-bit sRGB, so a fragment
emitting `4.0` is already clamped to white before the chain samples it —
everything over white blooms equally. The fuller fix is an `Rgba16Float`
intermediate, deliberately **not** taken: half-float *render targets* are not
guaranteed under `downlevel_webgl2_defaults`, which `live_gpu_binding.rs:488`
requests on **both** browser arms (WebGPU included) to keep them in parity. This
is documented at the top of `post_chain.rs`. If you revisit it, that parity
decision is the thing to weigh.

## What is NOT done — in dependency order

1. **Wire the chain into the live path.** *Highest value: without this the
   browser shows none of it.* Mirror how ambient/fog reach the GPU:
   `axiom-windowing` gains `set_bloom` → passed at `LiveGpuBinding::new` →
   build a `PostChain` instead of / alongside `UpscaleBlit` → call
   `chain.record(...)` in `render_frame` where the blit is today
   (`live_gpu_binding.rs:388-400`). The intermediate target is **unconditional**
   there (`live_gpu_binding.rs:291-330`), so the seam is clean. Then forward
   `running.bloom()` from `apps/burnt-rubber/src/web.rs:90-98`, next to the
   existing ambient/fog forwarding.
2. **canvas2d declares the drops.** `Sky`, `Specular`, `PostProcess`(bloom) →
   `CapabilityDegradation::Drop`, reported in the submission report's
   degraded-features list. This *is* "skip it for canvas2d", done legally.
3. **The sky pass.** Fullscreen pass behind the scene, mirroring
   `FrameSky::radiance` in WGSL. Needs an **inverse view-projection** uniform to
   turn a pixel into a world ray — that is the only new plumbing. Draw it first
   in the main pass with depth-write off.
4. **Specular.** Blinn-Phong in `SCENE_WGSL`. Needs (a) camera position in the
   `Lights` uniform — there is spare room, `fog_range.zw` is unused — and (b) a
   per-material strength. **Pack the strength into the existing instance
   `emissive` `vec4`'s pad float** (`frame_packet_adapter.rs:18`, layout is
   `mvp(16)|world(16)|colour(4)|emissive(3)+pad(1)`). Do **not** add a vertex
   attribute: rigid uses 14 of WebGL2's 16, and the skinned pipeline is already
   at 16, which is exactly why emissive was omitted there
   (`scene_renderer.rs:180-190`).
5. **burnt-rubber's moonlight rig.** Currently the key light is *warm* white
   `(1.0, 0.94, 0.84)` at `render/mod.rs:317-329` — that is sunlight. Wants a
   cool blue-white from the moon's direction, low elevation, plus `set_sky`,
   retuned `FrameAmbient` (`mod.rs:93-96`), and fog whose target matches the
   sky's horizon colour (`mod.rs:106-111`).
6. **Gates + commit.** None of the four gates have been run on this work yet.

## Gotchas that will cost you time

- **WebGPU cannot be tested live on this machine.** Device creation fails on
  `dxil.dll` (`Windows Error: 87`) and the cascade falls back to WebGL2 every
  time. Both arms run the **same** WGSL through the same wgpu device, and the
  native offscreen path is a real GPU, so `axiom-shot --features offscreen` is
  the trustworthy shader loop. You will not be able to show a true live-WebGPU
  frame.
- **`cargo check -p axiom-shot --features offscreen --all-targets` fails with 9
  errors on a CLEAN tree.** Pre-existing (verified by stashing). The test
  `common/mod.rs` and `render_parity.rs` are stale against
  `capture::render_gpu`'s signature. The normal gate does not use that feature
  combo, which is why CI is green. Do not chase it thinking you broke it — but
  do check `cargo check -p axiom-shot --all-targets` (no feature) stays at 0.
- **Coverage.** `modules/axiom-gpu-backend`'s wgpu/WGSL code is behind
  `#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]` and
  `scripts/coverage.sh` does **not** pass `--features offscreen`, so it is not
  compiled or measured. That is why the pure logic lives in `axiom-host`, where
  it *is* measured and must be 100%. Keep new shader-adjacent logic on that side
  of the line.
- **The Branchless Law applies to `crates/` and `modules/` but NOT `apps/`.**
  `frame_sky.rs` / `frame_bloom.rs` are branchless because they are a layer. In
  `apps/burnt-rubber` write plain `if`/`else` — clippy's `obfuscated_if_else`
  fires on `then_some().unwrap_or()` there and it is right.
- `crates/axiom-host/tests/architecture.rs` pins the **exact** `pub use` list in
  `lib.rs`. Any new export must be added in both places, sorted.

## Commands

```sh
# Deterministic GPU stills — the real shader loop.
cargo run -q -p axiom-shot --features offscreen -- \
  --app burnt-rubber-tunnel --backend gpu --width 960 --height 540 \
  --out screenshots/moon-x.png
# slices: burnt-rubber{,-start-line,-straight,-sweeping-turn,-drift,-tunnel,-traffic,-boost}

# Live app (already running on :8085 as of writing).
uv run scripts/localhost_servers.py start-app burnt-rubber --port 8085
uv run scripts/localhost_servers.py logs burnt-rubber -n 20
uv run scripts/playwright_controller.py goto "http://localhost:8085/?backend=gpu"
uv run scripts/playwright_controller.py console
uv run scripts/playwright_controller.py screenshot name
# For controlled driving, use the phone/rails profile — the car stays on the road
# by itself, which makes it far easier to steer at a target:
#   AXIOM_PW_VIEWPORT=390x844 uv run scripts/playwright_controller.py goto ...

# Gates (none run on this work yet).
cargo test --workspace
cargo run -p xtask -- check-architecture
bash scripts/coverage.sh
bash scripts/dylint-gate.sh
bash scripts/ts-gate.sh
```

## Reference screenshots

In `screenshots/`: `moon-base-straight.png`, `moon-base-tunnel.png` (before) and
`moon-bloom-straight.png`, `moon-bloom-tunnel.png` (after the bloom chain). The
tunnel pair is the clearest evidence the chain works.
