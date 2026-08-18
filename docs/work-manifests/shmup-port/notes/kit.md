# The modular building kit (`kit.js`)

Ported from `C:/dev/Claude-of-Duty/src/world/kit.js` (1113 lines): the
per-element builders every facade/street composition in the level draws
on — `facadeWall`, `windowState`/`windowUnit`, `doorUnit`, `shopfront`,
`balcony`, `parapet`, `stairRun`, `stripedCloth`, `awning`, `drainpipe`,
`pockGeometry`, `spallPatch`, `rubbleMound` — plus the sub-primitives it
leans on from `util.js` that had not yet been ported when `world-assembler.md`
landed: `solidSlabs`, `clothGeometry`, `tubeY`, `polyPrism`, `rockGeometry`,
and `kit.js`'s own `mergeSimple`, `sashLeaf`, `shutterLeaf`, `doorLeaf`,
`rollerShutter`, `worldOf`, `ryOf`.

## Why `world/kit.rs` became `world/kit/`

`apps/shmup/src/world/kit.rs` already existed before this session,
holding the **`util.js` geometry-toolkit** port (`trs`, `chamfer_box`,
`weather_prop`, `patch_geometry`, `plane_geometry`/`quad`, `wall_panel`) —
`world-assembler.md`'s own notes call this out as deliberately named ahead of
the real `kit.js` port landing here. Porting the actual `kit.js` into that
one file would have made it enormous, so it is now a directory:

- `kit/mod.rs` — unchanged `util.js` content, plus the new `ll`/`world_of`/
  `ry_of` composition helpers (`kit.js:33-52,1097-1111`), the cached
  `box_kit`/`box_fine_kit`/`box_soft_kit`/`box_thin_kit`/`pane_kit`
  (`BOX`/`BOX_FINE`/`BOX_SOFT`/`BOX_THIN`/`PANE`, `kit.js:54-60`), and `slab`
  (`kit.js:63-65`), plus flat `pub use` re-exports of every submodule below
  so `crate::world::kit::facade_wall` etc. reads exactly like the source's
  one-file namespace.
- `kit/primitives.rs` — `solid_slabs`, `cloth_geometry`, `tube_y` (+ the
  promoted `cylinder_geometry`, see below), `poly_prism`, `rock_geometry`,
  `merge_simple`.
- `kit/facade.rs`, `kit/window.rs`, `kit/door.rs`, `kit/shopfront.rs`,
  `kit/balcony.rs`, `kit/parapet.rs`, `kit/stairs.rs`, `kit/canopy.rs`,
  `kit/pipework.rs`, `kit/damage.rs` — one file per element family, matching
  the task's own grouping.

`ground.rs`'s own doc had a standing note: *"`CylinderGeometry` is ported
here, not in `crate::world::kit`, because it has exactly one caller in this
whole port; if a second caller arrives, promote it there."* `tubeY` is that
second caller, so `cylinder_geometry`/`cylinder_cap` moved from `ground.rs`
into `kit/primitives.rs`, gaining a real `open_ended` parameter (the manhole
ring never opted out of caps, so the ground-only copy never modelled the gate
at all — `ground.rs`'s call site now passes `false` explicitly, unchanged
behaviour). `geo.rs` also gained `WorldGeo::rotate_y` (`rollerShutter` needs
it; only `rotate_x` existed before).

## Panel-space composition

Every element takes a `pm: &Mat4` (panel→level) and composes its own parts
onto it through `kit::ll` — the one function `L`/`LL` (`kit.js:33-52`)
collapse into, since the source's split only exists to dodge a JS GC
allocation that a by-value `Mat4` return doesn't have. `ll(pm, x, y, z, ry,
sx, sy, sz, rx, rz) = pm.multiply(trs(x, y, z, ry, sx, sy, sz, rx, rz))`,
reusing `kit::trs`'s already-verified `'YXZ'` Euler order.

## RNG draw order — the thing that actually needed care

Every element with a stochastic branch was walked draw-by-draw against the
source to preserve exact call order, especially where JS's `&&`/`||`/`?:`
short-circuits skip a draw:

- `windowUnit`: `openR = state === 'open' || rng.float() < 0.4` only draws
  when `state != Open`; the broken-pane roll (`broken && rng.float() < 0.55`)
  only draws per-pane when `broken` and the pane wasn't already skipped by
  the open-leaf check; the shutter swing rolls (`shut ? false :
  rng.float() < 0.45`) draw per leaf only when not `Shuttered`.
- `shopfront`: cloth-bolt block draws in the exact source order (guard,
  width, height, cloth's own internal seed draw, `pick`, sign ternary,
  offset) — verified by hand against `kit.js:638-655`'s literal argument
  evaluation order.
- `stripedCloth`: `seed` is drawn **once**, before the loop, regardless of
  how many bands are skipped — `cloth_geometry` is then always called with
  that `seed` pre-resolved (`ClothOpts::seed: Some(_)`), never drawing its
  own.
- `pockGeometry`: exactly 16 draws (8 + 8), matching the source's own
  comment that `registerProps` shares one RNG stream with the whole level
  build.
- `doorUnit`, `balcony`, `stairRun`, `spallPatch`: draw **nothing** from
  `rng` (accepted but unused in the source too) — each pinned by a
  same-seed-untouched-vs-passed-through `Rng::state()` comparison test.

## Cross-field JS defaults that Rust can't express as struct defaults

Several source functions default one option off another already-defaulted
option (`windowUnit`'s `broken = opts.broken ?? state === 'open'`;
`stripedCloth`'s `bands ?? max(3, round(w/0.38))` then `segX ?? max(2,
round(24/bands))`). Rust structs have no cross-field defaults, so every such
struct's doc comment states the source formula and callers resolve it
themselves — `striped_cloth_default_bands`/`striped_cloth_default_seg_x` are
exposed as real functions (not just prose) so `awning` and any future caller
share the one formula rather than re-deriving it.

## `rockGeometry` — only `detail = 0` is implemented

`rockGeometry(rng, size, detail=1, squash=0.7)` builds on
`THREE.IcosahedronGeometry(size*0.5, detail)`. Grepping every `rockGeometry(`
call site across `src/world/*.js` (`kit.js`, `dressing.js`, `props.js`) shows
**every one passes `detail = 0`**. At `detail = 0`,
`PolyhedronGeometry.subdivideFace`'s barycentric subdivision algebraically
collapses to "emit the 20 base icosahedron faces, vertices in order `(b, c,
a)` for base face `(a, b, c)`" (worked by hand from the general algorithm),
which is what `rock_geometry` builds directly — no general subdivision code,
no `generateUVs`/`correctUVs`/`correctSeam` azimuth-wrap port, since nothing
in this port would ever exercise them and `rockGeometry`'s own callers never
read its `uv` attribute. `rock_geometry` panics if ever called with
`detail != 0`, so a future caller that actually needs subdivision fails
loudly instead of silently getting an unsubdivided rock.

## `polyPrism`/`spallPatch` — the same `extrude`-reuse trade as `wall_panel`

`poly_prism` reuses `weapons::geometry::primitives::extrude` exactly as
`wall_panel` already does (see `world-assembler.md`), for the same reason:
one already-verified bevelled-extrude-with-holes engine instead of a second
hand copy of `THREE.ExtrudeGeometry`. Two corrections applied at the call
site, both documented in `poly_prism`'s own doc: undo `extrude`'s
`-depth/2 + bevel` translate (the source's raw `ExtrudeGeometry` is never
translated), and reproduce the source's `rotateX(-PI/2)` +
`computeVertexNormals()` tail exactly (mathematically idempotent for a pure
rotation, kept anyway per the port recipe's "port the behaviour, don't
simplify it away" rule). `spallPatch` then applies a **second**,
opposite `rotateX(+PI/2)` on top — preserved verbatim rather than
"optimized" into a no-net-rotation version, since the recipe explicitly
warns against silently tidying up a source quirk.

## Golden capture

`apps/shmup/tests/kit/capture.mjs` → `golden.json`, read by
`apps/shmup/tests/kit_port.rs` (15 tests, all passing). Pins:

- `solidSlabs` — four opening layouts (no holes, one centred hole, two
  holes, a hole flush with the panel edge), exact `f32`-tolerance rectangle
  equality.
- `windowState`'s selection distribution — every combination of
  `floor ∈ {-1,0,1,2} × damage ∈ {0,0.2,0.5,0.8} × allowLit ∈ {true,false}`
  (32 combinations), 500 draws each from a fixed seed, exact per-state
  counts. This is the "distribution matters" check the recipe asked for: a
  threshold typo anywhere in `window_state` would shift a count here even if
  every individual call still returned *some* valid state.
- Every element's per-palette-key vertex/triangle counts (`facadeWall`,
  `windowUnit` at all seven states, `doorUnit`, `shopfront`, `balcony` at
  both railing kinds, `parapet`, `stairRun`, `stripedCloth`, `awning`,
  `drainpipe`, `rubbleMound`), for one fixed non-trivial panel matrix
  (`trs(1.2, 0.4, 3.4, 0.3, 1,1,1,0,0)`) and fixed per-call seeds — exact
  `(key, verts, tris)` tuples, **except** `facadeWall`, which (via
  `wall_panel`'s own already-documented `extrude`-welding divergence) only
  matches on triangle count, not vertex count — the same accepted trade
  `world_port.rs` already documents for `wall_panel_arch_hole`.
- `pockGeometry` — full position/normal/color arrays, exact `1e-6`
  tolerance: it is a hand-rolled indexed mesh with an identical, deterministic
  vertex order on both sides (no extrude/weld anywhere in its construction),
  so a direct array comparison is meaningful.
- `spallPatch` — triangle-soup (weld-invariant) position/normal comparison
  via the shared `tests/geometry_assert` comparator, since it goes through
  `poly_prism` → `extrude` and can be welded/reordered relative to the raw
  JS.
- `parapet`'s/`stairRun`'s/`awning`'s return values (`top`, `{top, endZ}`,
  `{x,y,w,d}`).

## Deliberate divergences (documented at the site too)

1. **`poly_prism`/`wall_panel` both reuse `extrude`** — vertex welding can
   change vertex count/order but never triangle count; only `facadeWall`'s
   golden test needed the triangle-count-only relaxation in practice (every
   other element never touches `wall_panel`/`poly_prism`, so their bucket
   comparisons are vertex-exact).
2. **`rock_geometry` only implements `detail = 0`** (see above) — a
   deliberate scope boundary, not a silent gap: every real caller in the
   whole source passes `detail = 0`.
3. **`rockGeometry`'s `uv` attribute is left empty** rather than porting
   `generateUVs`/`correctUVs`/`correctSeam` — never read by any caller.
4. **`merge_simple` zero-fills a missing attribute** (matching the source's
   pre-sized, zero-initialized `Float32Array`s) rather than computing real
   vertex normals the way `Accum::add` does for a normal-less input — inert
   in practice since every real caller only ever merges `plainBox()`-derived
   parts, which always carry all four attributes.
5. **Cross-field JS defaults are not modelled as Rust struct defaults** — see
   above; every affected struct's doc names the source formula.

## Not ported (out of scope per the task)

- `L` (`kit.js:33-38`) as a separate function — collapses into `ll`, see
  above.
- `runoffStreak`, `driftBerm`, `catenaryTube`, `sackGeometry`,
  `disposeAll`, `warpGeometry` (`util.js`) — not used by `kit.js`, belong to
  future prop/dressing passes per `world-assembler.md`'s own "not ported"
  list, which this port does not widen.

## Verification

`cargo test -p axiom-shmup --lib world::kit::` — 67/67 passing.
`cargo test -p axiom-shmup --test kit_port` — 15/15 passing.
`cargo xtask check-architecture` — OK.

A full-crate `cargo test -p axiom-shmup` run intermittently showed
compile errors and (once compiling) 3 test failures entirely inside
`world::props::*` — an **untracked, uncommitted directory from a different,
concurrently-running agent session** (confirmed via `git status`:
`apps/shmup/src/world/props/` and `apps/shmup/tests/props/`
are both `??`, and `src/world/mod.rs`'s `pub mod props;` plus visibility
widenings in `weapons/geometry/primitives/{lathe,sphere,mod}.rs` are
modified-but-uncommitted, from before and after this session's own edits).
Per the port recipe's concurrency note, none of those paths are staged or
touched by this commit. Every failure this session ever saw in `world::kit::`
or `kit_port` was zero, at every point checked.
