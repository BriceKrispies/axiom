# Mobile-first Axiom

This document records the structural pass that made Axiom mobile-first, and what
that actually means for a WASM-first 3D engine. The short version: the render and
host **spine was already built to the mobile floor** (it requests
`Features::empty()` + `Limits::downlevel_webgl2_defaults()`, uses `f32`
throughout, has no MSAA / compute / bindless), so "mobile-first" here is not a
rendering rewrite. The genuine desktop assumptions were concentrated at the
**edges**, in four places. Each is fixed at the lowest correct layer below.

> **Update — responsive layout is now an engine layer.** Per-app CSS broke on
> phones (roomed-puzzle's board ran off-screen behind a fixed side panel). The fix
> is the new **`axiom-layout` layer** (`crates/axiom-layout`, `depends_on =
> ["kernel", "host"]`): a deterministic, branchless, recursion-free flex/constraint
> solver. Given the host viewport facts (logical size, `Orientation`, safe-area
> insets) and a flat node tree, it computes one placed `LayoutRect` per node —
> mobile-first by default: the root insets by the safe area, a `Direction::Adaptive`
> node flips a row to a stacked column in portrait, and free space is distributed by
> grow weight. It is the engine's single home for *how on-screen regions are ordered
> and placed*; apps declare intent (roles, grow, min sizes, aspect) and the engine
> decides placement. The solver mirrors the scene layer's transform-propagation
> idiom (a single index-order pass over a parent-before-child array; each node lays
> out its children), so it is recursion-free and branchless yet handles nesting,
> wrap, justify/align, gap/padding, min/max, and aspect-letterbox — at **100%
> coverage**. **roomed-puzzle** is the proving consumer: its `web.rs` builds a
> `HostViewport` from the live window/DPR/safe-area, declares a board+panel tree,
> and applies the solved rects to absolutely-positioned DOM — verified in-browser at
> both a wide viewport (board + panel side-by-side) and a phone-portrait viewport
> (board full-width on top, panel stacked below), the board fully visible in both.
> Fast-follows: retro_fps (canvas/HUD/touch-zones from solved rects) and the `interface`
> layer adopting it for responsive default panel placement.

The guiding principle: **you opt *out* of the mobile budget, never silently into
a desktop one.** Touch is the primary input; a mouse is one more pointer. The
default device tier targets the constrained device; capable platforms opt up.
Host facts grow to cover the mobile envelope as *data on existing boundaries*,
never as `#[cfg]` platform branches in the spine.

## Gap 1 — Input is device-agnostic, touch-first

The engine-side input contract (`FirstPersonInput { move_local, yaw, pitch }`)
was already device-neutral; the desktop assumption lived entirely *above* it, in
per-app `requestPointerLock` + `movementX/Y` + WASD decode, plus the gallery's
`keypad.js` shim that faked `KeyboardEvent`s from on-screen buttons. Pointer Lock
does not exist on a touchscreen.

The fix splits cleanly by layer:

- **Synthesis** — `modules/axiom-input` (a new isolated engine module). Its one
  facade, `TouchControls`, turns a frame's neutral pointer samples `(position,
  is_down)` plus a virtual on-screen layout into a normalized movement vector
  (left-thumb joystick) and look deltas (right-thumb drag). A "pointer" is a
  mouse, finger, or pen — one code path. Pure, deterministic, branchless, 100%
  covered on native.
- **Capture** — the platform edge. `WindowingApi::install_pointer_capture`
  (`modules/axiom-windowing`, wasm32) installs unified **PointerEvent** listeners
  (the one browser API that reports mouse + touch + pen in a single shape) and
  exposes the down pointers as neutral samples in physical pixels.

An app composes the two: read `capture.samples()` each frame, feed
`TouchControls::update`, map the resulting `ControlFrame` onto its
`FirstPersonInput`. This retires both the per-app pointer-lock decode and the
`keypad.js` mustache.

**Status:** synthesis + capture landed and gated. App adoption and `keypad.js`
retirement are the remaining integration step — they change a live browser
surface and so must be **browser-verified** (Playwright + ideally a real device)
before they land, per No-Shortcuts (don't ship an unverified change to a live
demo).

## Gap 2 — Display facts: orientation + safe-area insets

`HostViewport` was DPI-correct (logical + physical + `scale_factor`) but had no
way to express a phone's orientation or its notch/rounded-corner/home-indicator
insets, so a HUD would draw under the system UI.

- `Orientation` (`crates/axiom-host`) — `Portrait | Landscape | Square`, **derived**
  branchlessly from the viewport's physical extents (`HostViewport::orientation`),
  so it can never disagree with the size the engine renders into.
- `HostSafeAreaInsets` — the four non-negative edge insets (logical pixels),
  attached to a viewport via `with_safe_area_insets` (default `none()`), exactly
  as `HostFrameInput::with_presentation_nanos` layers on its optional fact. A
  browser adapter fills it from the CSS `env(safe-area-inset-*)` values; a
  headless harness leaves it `none()`.

Both are host *data*, validated like the scale factor. A surface resize/rotation
is handled by re-calling `WindowingApi::configure_surface` (already idempotent).

**Status:** landed and gated (100% covered, branchless).

## Gap 3 — The device tier is load-bearing

`HostDeviceProfile { Baseline, ExtendedLimits }` existed but was inert. It is now
the mobile-first render-parameter lever, with `Baseline` (the default every
caller already picks) tuned for the constrained device:

- `shadow_map_size()` — `Baseline` 1024², `ExtendedLimits` 2048². A quarter the
  shadow-atlas VRAM and a quarter the pre-pass fragments, for a barely-perceptible
  change at demo scale. **Wired** through `GpuBackendApi` (which reads it from the
  presentation request at construction — native-tested) into the renderer's
  shadow texture.
- `max_render_dimension()` / `render_size(w, h)` — an aspect-preserving cap on the
  rendered resolution (`Baseline` 1600, `ExtendedLimits` 4096). The single biggest
  GPU saving on a high-DPR phone, where the physical surface can be 3× the CSS
  size. The cap is high enough that ordinary desktop-sized (and the 960×600 demo)
  surfaces render 1:1, so nothing is degraded.

**Status:** the tier policy and `shadow_map_size` wiring landed and gated
(`shadow_map_size` is native-tested via `GpuBackendApi::shadow_size`).
`render_size` is delivered as gated host policy; consuming it in the **live** arm
needs a render-to-smaller-target + upscale-on-present pipeline, which is wasm-only
and must be browser-verified before it lands.

## Gap 4 — GPU context-loss recovery

A backgrounded mobile browser drops the WebGPU/WebGL drawing context; the next
frame the surface reports an error instead of a texture, and the old code just
propagated it.

- `surface_recovery` (`modules/axiom-gpu-backend`) — a pure
  `SurfaceStatus -> RecoveryAction` decision (`Present` / `SkipFrame` /
  `Reconfigure` / `Reinitialize`), branchless, fully tested. Compiled with the GPU
  arm it serves (wasm32 / `offscreen`), exactly like `scene_renderer`.
- The live binding (`live_gpu_binding`, wasm32) now retains its surface config and,
  on a `Lost`/`Outdated` acquisition, **reconfigures and re-acquires** the context
  then retries once; a `Timeout` skips the frame; `OutOfMemory` signals a rebuild.

**Status:** the recovery decision + wasm reconfigure-retry landed and compile on
both targets; the recovery tests run under `--features offscreen` and wasm. The
full device-loss *reinitialize* path (run loop rebuilds the binding) and on-device
behavior need browser verification.

## Integration pass (the four follow-ups), browser-verified

The four follow-ups have now landed, with **roomed-puzzle as the first app to
adopt touch input** and **retro_fps-browser as the GPU verification target**:

1. **Touch input adopted (swipe).** `axiom-input` gained a second method on its
   facade, `TouchControls::swipe(surface, pointers) -> Option<Vec2>` (the discrete
   counterpart of `update`, branchless + 100% covered): one flick → one cardinal
   direction. roomed-puzzle (a 2D grid puzzle, previously keyboard-only and
   unplayable on touch) consumes it — gathering pointer samples app-side and
   mapping the swipe to `PuzzleCommand::Move` — plus on-screen Freeze/Restart
   buttons. **Verified in-browser:** swipe steps the player, both buttons work.
   (The analog FPS apps are the natural next adopters of `TouchControls::update`
   + `install_pointer_capture`, retiring `keypad.js`.)
2. **`render_size` consumed in the live arm.** The wasm `LiveGpuBinding` now
   renders the scene into an intermediate target sized by
   `HostDeviceProfile::render_size` (native-tested via `GpuBackendApi::render_width
   /height`) and upscales it to the swapchain with a new fullscreen-triangle blit
   (`modules/axiom-gpu-backend/src/upscale.rs`, wasm32-only). **Verified on retro_fps**
   (renders correctly through the intermediate→blit path). The visible downscale
   needs a >`max_render_dimension` surface (a real high-DPR phone); the cap
   arithmetic is unit-tested.
3. **Safe-area + orientation.** retro_fps and roomed-puzzle use `viewport-fit=cover` +
   `env(safe-area-inset-*)` so their layout clears a notch (the visible win,
   verified: falls back cleanly to the default padding where insets are 0). A
   reusable `WindowingApi::read_safe_area_insets()` bridges the browser `env()`
   values to the engine's `HostSafeAreaInsets` contract. *Follow-up:* a
   live-render resize-reconfigure on `orientationchange` needs a responsive canvas
   (the demos use a fixed backing store), and an engine-side consumer of the
   insets — neither exists to verify against yet, so neither was landed blind.
4. **Device-loss reinitialize.** `present` now surfaces an unrecoverable GPU-loss
   error instead of swallowing it; the `drive_web_multi` run loop rebuilds the
   backend off-loop (re-probing WebGPU → WebGL2 → Canvas2D) with a one-at-a-time
   guard. The recovery decision (`surface_recovery`, failure-only) is 100%
   covered. **Verified on retro_fps** (no regression to the refactored loop). The true
   device-loss rebuild fires only when a real GPU drops its context.
