# `ui/minimap.js` → `apps/shmup/src/ui/minimap.rs`

**Source:** `C:/dev/Claude-of-Duty/src/ui/minimap.js:1-603` (603 lines).
**Wrote:** `apps/shmup/src/ui/minimap.rs`, `apps/shmup/tests/ui_minimap_port.rs`,
`apps/shmup/tests/ui_minimap/{capture.mjs,golden.json}`.

This was the last unported file in `ui/`.

---

## The deferral had already expired, and in two ways

The port status recorded this file as *"blocked on the render work — needs an
orthographic depth bake read back once, then a Sobel pass for roof outlines"*.
Both halves of that are wrong, and neither is a close call.

### 1. There is no Sobel pass anywhere in the file

`minimap.js:10-23`'s module comment still says *"a Sobel pass for crisp roof
outlines"*, but the code it describes was replaced at some point.
`_buildBitmap` derives its rim from a blurred **coverage** field —
`rim = 4w(1-w)`, peaking on the coverage midline (`minimap.js:414-421`) — and
the comment immediately above that code (`minimap.js:290-305`) explains why:
the aliased 1px outlines *"it used to draw"* were replaced with a separable
tent blur. The deferral note was written from the stale header, not the body.

**Lesson for the remaining deferrals in this port:** a file's own doc comment
is not evidence about its code. This is the fifth deferral-that-became-a-defect
the port has turned up, and the first one caused purely by trusting prose.

### 2. The depth bake is the *fallback*. The real map is pure CPU.

`tryBake` (`minimap.js:71-164`) calls `_buildVectorMap` **first** and only falls
through to the GPU path *"for a scene that has no world subsystem in it"*
(`minimap.js:74-76`). `_buildVectorMap` needs exactly three things —
`world.buildings`, `world.levelToWorld`, `world.isOpen` — and
`crate::world::system::WorldSystem` has had all three as public queries since
the world slice landed (`level_to_world` at `src/world/system.rs:481`,
`is_open` at `:499`, `buildings` at `:230`).

The reference screenshot settles it. Crop the top-left inset of
`docs/work-manifests/shmup-port/reference/original-street.png`: the footprints
are **rotated**, all at the same angle. Only the `levelToWorld` affine (which
carries `LEVEL_YAW = 0.5877`) produces that. A top-down orthographic depth bake
is axis-aligned to its own camera and cannot.

So the primary map is ported in full, CPU-side, with **no engine capability
added and nothing invented**.

---

## What is ported

Everything except one byte fetch.

| `minimap.js` | Rust | Notes |
|---|---|---|
| `constructor` `25-56` | `Minimap::new` | DOM chrome (corner ticks, `N`, the `ZONE 07 / 60M` tag) is view work; `style.css.tpl` already carries every `.ow-minimap*` rule |
| `resize` `58-67` | `Minimap::resize` | |
| `tryBake` `71-164` | `Minimap::try_bake` → `BakeAttempt` | all six arms |
| `_buildVectorMap` `183-280` | `Minimap::build_vector_map` | the primary map |
| `_releaseGpu` `282-288` | `Minimap::release_gpu` | |
| `_buildBitmap` `306-433` | `Minimap::build_bitmap` | the whole fallback pipeline |
| grain `269-275`, `401,424-427` | `Minimap::apply_grain` | |
| `draw` `441-596` | `Minimap::draw` → `Vec<DrawOp>` | |
| `dispose` `598-602` | `Minimap::dispose` | |

### Output shape: a display list, not a numeric frame

Every other `ui/` widget computes a struct a DOM view writes onto nodes. This
one is a **canvas painter** — its behaviour *is* a call sequence — so `draw`
returns `Vec<DrawOp>`, one variant per canvas2d call, in source order. The
golden compares op-for-op against the calls the real browser context received,
which is a stronger pin than any struct of derived numbers would be, and
rasterisation stays where it belongs (the view).

`build_vector_map` returns the same vocabulary, authored in LEVEL metres with
the recovered level→canvas affine as its `SetTransform`.

---

## The seam that remains, and exactly what would satisfy it

```rust
pub trait DepthBakeSource {
    fn read_ortho_depth(&mut self, req: &DepthBakeRequest) -> Result<Vec<u8>, ()>;
    fn release(&mut self);
}
```

One call, same shape as `grounding::FootSource` / `agent::PathSource` /
`penetration::RayWorld`. It must return `512 * 512 * 4` bytes, **bottom-up**,
from a `MeshDepthMaterial` + `BasicDepthPacking` render of the scene through an
orthographic camera at `(centre.x, 26, centre.z)` looking down with
`up = (0, 0, -1)`, half-extent `95`, clip `0.1 .. 34`, cleared to opaque black.
Only the red channel is read; the value is `(1 - fragCoordZ)`, linear because
the camera is orthographic.

**What must change engine-side to make it live**, in dependency order:

1. `modules/axiom-gpu-backend/src/offscreen.rs` — `render_to_rgba` exists but
   renders the *lit* scene. This needs a **depth-only override material**: one
   pass that writes linear normalised depth to colour, with the scene's own
   materials bypassed. The nearest existing thing is
   `modules/axiom-gpu-backend/src/gbuffer.rs`'s `GBufferChannel::Depth`, which
   is **view-space linear depth**, not `1 - fragCoordZ`, and is written during
   the main pass rather than from an arbitrary camera. Either channel works —
   the port only needs a monotone height encoding — but the arithmetic in
   `build_bitmap` (`h = clamp(CAM_Y - NEAR - (1-d)*(FAR-NEAR), 0, CAM_Y)`) is
   written for `BasicDepthPacking` and would have to be re-derived, which means
   re-capturing the golden. Prefer adding the packing the source uses.
2. An **orthographic camera** with a settable `up` on the offscreen path.
   `up = (0, 0, -1)` is what puts north (-Z) at the top of the map; a default
   `up = +Y` camera looking straight down is degenerate.
3. The **oversize-object cull** (`minimap.js:110-127`: hide sprites, points,
   lines, and any mesh whose bounding-sphere radius × max world scale exceeds
   260 m). Without it a sky dome or a 1 km ground plane swallows the map. This
   is a per-object visibility override for one pass — the offscreen path has no
   such filter today.

**Until then the seam is honest:** with no `DepthBakeSource`, `try_bake`
returns `BakeAttempt::NoRenderer`, `baked` stays `None`, and `draw` paints the
`#2b333b` flat plate — which is exactly what the source does. **It never
invents a roof outline.** And because the vector map is the primary path and it
*is* ported, the fallback is only reached by a scene with no world subsystem —
i.e. never, in the shipped game.

### Expiry check

If `axiom-gpu-backend` gains a depth-only offscreen pass with a settable camera
`up`, this seam becomes implementable and `apps/shmup/src/ui/minimap.rs` needs
no change beyond an `impl DepthBakeSource`. The file that must change is
`modules/axiom-gpu-backend/src/offscreen.rs`.

---

## The source defect this found

**The street network west of level x = 0 is never drawn.**

`minimap.js:229-241` run-length-codes each row of open ground:

```js
let run = -1;
for (let lx = -44; lx <= 44 + STEP; lx += STEP) {
  const open = lx <= 44 && world.isOpen(cxw, czw, 0);
  if (open && run < 0) run = lx;
  else if (!open && run >= 0) { g.fillRect(run, lz, lx - run, STEP * 1.16); run = -1; }
}
```

`run = -1` is the "no run open" sentinel — but `run` holds an `lx`, and `lx`
ranges `[-44, 44]`. Every negative `lx` *is* the sentinel. So while a run's
start is negative, `open && run < 0` keeps re-firing and walks the start
forward one cell per iteration, and the close arm `!open && run >= 0` can never
fire. A run is only ever emitted from the first open cell at `lx >= 0`, and a
run that ends before `lx = 0` is dropped entirely.

Measured on the **real level** by the golden: 7 202 of the 41 772 queried cells
are open, and **213 rects** are emitted, **none** starting at a negative level
x. The whole point of the street layer, per the source's own comment
(`minimap.js:171-177`), was that *"the road reads as the negative space between
the blocks, which is how you recognise the alley you are standing in"* — and
half of it is missing.

This is visible in the reference screenshot: the minimap shows footprints on
flat ground with **no lighter street network at all**, which is what this
defect produces for a level whose streets straddle `lx = 0`.

Ported faithfully. Pinned by `on_the_real_level_no_street_run_starts_west_of_zero`
(integration, real level) and `negative_lx_street_runs_are_never_emitted` /
`a_straddling_street_run_loses_its_western_half` (unit, isolated).

---

## Traps checked by name

* **`Math.round` ties toward `+Infinity`** — `crate::jsmath::round`, at the
  three sites that use it: the canvas pixel size (`:61`), the grid lines
  (`:486`, `:493`) and the footprint colour channels (`:251`). Two of the
  golden's five resize cases land on an exact tie (`178 × 1.25 = 222.5 → 223`,
  `178 × 0.75 = 133.5 → 134`).
* **`(x * Math.PI) / 180` is not `to_radians()`** (`self * (PI / 180)` — a
  different grouping, and float multiplication is not associative). The source
  writes the former at all three angle sites (`:500`, `:501`, `:552`); so does
  the port. Same trap the `ui/system.rs` slice found at its bearing sites.
* **`Math.hypot`** — not called. The file has no distance computation at all;
  the blip cull is a per-axis box test.
* **`|0`** — not used.
* **`Float32Array` storage width** — `hgt`, `cov`, `cr`, `cg`, `cb` and the
  blur scratch are all f32 (`:310`, `:328-331`, `:350`) while every
  intermediate is f64. The port stores `Vec<f32>` and narrows on assignment.
  Missing this would drift the whole bitmap.
* **`Uint8ClampedArray` assignment** rounds **half to even** and clamps —
  `u8_clamped`. Both grain loops and the bitmap writer go through it; `as u8`
  truncates and would be wrong on every fractional channel.
* **`Number.prototype.toFixed(1)`** ties toward the **larger** integer where
  Rust's `{:.1}` ties to even (`9.25` → `"9.3"` vs `"9.2"`) — `to_fixed_1`.
  Reachable: the argument is `9.5 * u` and exact quarters are dyadic.
* **`Math.min(2, devicePixelRatio || 1)`** — JS `||` is falsy on `0`, `-0`
  **and** `NaN`; `crate::jsmath::or_one`.
* **`rng.fork()` order** — unchanged. The minimap's generator is the *second*
  fork in `UiCore::new` (`index.js:86`), already spent there as
  `_minimap_rng`. Both bake paths draw exactly `512 × 512` floats for the
  grain, in raster order; only one path runs per bake, so the streams never
  interleave. Pinned by generator state before and after.
* **Dead computation is still part of the source** — `HEIGHT_RANGE` is
  `CAM_Y` and both are used; `this.centre` is never written but is read four
  times, so it stays a field.
* **Float grouping preserved** everywhere: `((h - inX) + (h - inY)) * 0.22`,
  `(a + b + c) / 3`, `lerp(FR, cr[si] * iw, w)`, `4 * w * (1 - w)`,
  `rim * rim * 0.66` are transcribed from the JS text, not re-derived.
* **The probe aliasing** at `:193-201` — the source passes one scratch
  `Vector3` to all three `levelToWorld` calls, so each overwrites the last.
  `ox`/`oz` are read out as numbers before the second call, so it is harmless;
  noted so a future reader does not "fix" it.

---

## The golden

`tests/ui_minimap/capture.mjs` runs the **original, unmodified** `minimap.js`
in headless Chromium (real DOM, real canvas2d, real `three@0.180`, real
`core/rng.js`), the same way `tests/ui_system/capture.mjs` does. Canvas calls
are journaled by wrapping the live context and forwarding straight through, so
the browser still rasterises and `getImageData`/`putImageData` behave.

**The vector map is driven by the real level**, not an invented one: the world
seam is fed the genuine `src/world/layout.js` `BUILDINGS` array, the genuine
`isOpen` from `src/world/dressing.js`, and the real
`Assembler.setTransform(LEVEL_YAW, LEVEL_TX, LEVEL_TZ)` affine with the real
constants from `src/world/index.js:60-62`. The Rust test replays the three
recorded probe results and the complete ordered 41 772-answer `isOpen`
sequence (packed as a hex bitstring), asserting at 42 sampled points that it
queried the same world position — so a port that projects differently
desynchronises and fails loudly rather than quietly agreeing.

Pinned: 5 resize cases · the 378-op vector-map journal · 262 144 grain draws
and the resulting generator state · `build_bitmap` end to end (a byte checksum
over the whole 512² result plus 212 sampled pixels, read out of the `ImageData`
the source hands to `putImageData` so no canvas round-trip colours it) · the
too-sparse rejection · `tryBake`'s state machine · and a 9-frame `draw`
trajectory (both plates, four widget scales, friend/foe blips, all four cull
arms, 0/1/3 objectives).

**One input is synthetic and labelled as such:** the depth-bake pixel buffer.
There is no CPU oracle for `readRenderTargetPixels`, so both sides generate the
buffer from the same integer-only formula (`depthFormula` in the golden) and
the comparison is of everything `_buildBitmap` does *with* those bytes. The
capture script says so, the test's module doc says so, and the bytes
themselves are never claimed to be a GPU's.

**Tolerance: bit-exact expected**, asserted at 1e-15 relative (absolute floor
1e-12) so a failure reports a magnitude. Nothing in this widget calls a
transcendental — the affine arrives from the golden and everything downstream
is multiply/add/`floor`/`ceil`/`round`/f32-narrowing, all IEEE-exact.
**Unverified**: written in the no-build wave.

---

## Wiring the orchestrator must do

```
apps/shmup/src/ui/mod.rs:  pub mod minimap;
```

and in `apps/shmup/src/ui/system.rs`, one accessor so the bake gate can be
closed — `UiCore::minimap_bake_done` is private with no setter, so today the
gate re-fires forever (which is correct while nothing bakes, and wrong once
something does):

```rust
/// `this.minimap.bakeDone`, fed back by whoever owns the `Minimap`.
pub fn set_minimap_bake_done(&mut self, done: bool) {
    self.minimap_bake_done = done;
}
```

No other change to `system.rs`. Its `MinimapState`, `MinimapObjective`,
`UiEffect::MinimapTryBake` and `UiEffect::MinimapDraw` are already exactly the
shapes this module consumes, and `_minimap_rng` is already forked and spent in
the right order — the owner passes that fork to `Minimap::new`. The caller
handles `MinimapTryBake` with `minimap.try_bake(Some(world), depth)` and
`MinimapDraw` with `minimap.draw(&frame.minimap)`.

`src/ui/mod.rs`'s module doc says `minimap.js` is not ported (three places) and
`Blip`'s doc says `MAX_BLIPS` "has no home yet"; `system.rs`'s doc says the same
in four places and `resize` carries a `// the minimap is not ported` comment.
Those are now stale but they are shared files — flagged here rather than
edited.

## Not ported

Nothing, apart from the depth readback described above and the static DOM
chrome. `demo.js` remains another slice's problem.
