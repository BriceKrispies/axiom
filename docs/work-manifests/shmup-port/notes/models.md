# Port notes — `weapons/models/{rifle,smg,pistol}.rs`

Source: `C:\dev\Claude-of-Duty\src\weapons\models\rifle.js` (468 lines),
`smg.js` (~330 lines), `pistol.js` (~230 lines) — `buildRifle()`,
`buildSmg()`, `buildPistol()`.

Target: `apps/shmup/src/weapons/models/{mod,rifle,smg,pistol}.rs`.
Tests: `apps/shmup/tests/weapons_models_port.rs`, goldens at
`apps/shmup/tests/models/{rifle,smg,pistol}_golden.json`.

## What was ported

All three `build*()` functions, faithfully, laying the already-ported part
builders (`weapons::parts::*`) out against each weapon's dimension sheet.
Every source comment documenting *why* a number is what it is (the rail-top
offset, the aperture budget on the optic, the hand-target derivations) was
carried over verbatim, converted from JS block comments to `//` prose (never
`/** */` inside a function body — that gets parsed as a stray doc comment
attached to the next statement and triggers `unused_doc_comment`; no other
ported file in this crate does that either).

Preserved source quirks, named as such per the port recipe:
- `rifle.js`'s `const lower = addLowerReceiver(...)` is captured and never
  read again in the source; ported as `let _lower = add_lower_receiver(...)`.
- `rifle.js`'s `const barrel = addBarrel(...)` is likewise unused; ported as
  `let _barrel = add_barrel(...)`.
- Every JS `Geo.clone()`-then-reuse (`stopLever` added twice in `pistol.js`,
  the mirrored stipple geometry, the two selector halves) is an explicit
  `.clone()` at the second use site, since `Assembly::add` takes `Geo` by
  value (JS's own `Assembly.add` clones internally, so this is the same
  behavior, not a shortcut).

Return-value shape (`{ id, label, fxClass, body, moving, nodes, shell,
magSize }`) is a per-weapon Rust struct: `RifleModel`/`SmgModel`/
`PistolModel`, each with its own `*Moving` and `*Nodes` struct. Three shared
node-shape types live in `models/mod.rs`: `PosRot` (`{pos,rot}` attachment
points), `GripTarget` (`{pos,finger,back}`), `HandguardProfile` (the rifle's
collision-cylinder node). All fields are `f32`, matching `Assembly::Node`'s
own shape and the rest of a model's authoring math — deliberately **not**
`weapons::clips::AttachNodes`'s `f64` fields, which serve a different,
not-yet-wired consumer per that module's own doc ("a placeholder for the
whole rig" it explicitly is not).

`SmgNodes` has no `handguard` field: `smg.js` calls `addHandguard` but never
adds a `handguard` node to its returned `nodes` object (the SMG's support
hand targets the foregrip, not a cylinder-solved handguard grip) — confirmed
by reading `smg.js`'s full `nodes: {...}` literal, not assumed.
`PistolNodes` has no `chargeRest`/`boltRest`/`selectorPivot` (no charging
handle or selector on a striker-fired pistol) but does have
`slideRest`/`slideTravel`/`slideGeom`, none of which the rifle/smg have.

## The decisive whole-weapon verification, and the controls-residual verdict

**Verdict: the 0.0057-0.071 m position deviations
`weapons_parts_controls_port.rs` measured on isolated `pistol_grip`/
`carbine_stock`/`trigger`/`charging_handle` buckets were a comparison
artifact, not a geometry bug.** The assembled rifle (which contains every one
of those parts, laid out at real dimensions, merged into whole-weapon
buckets) reproduces the golden JS build's material-bucket set and exact
per-bucket triangle counts, and every triangle's position/normal is correct
to float noise once compared correctly. Full chain of evidence:

1. **Triangle counts match exactly**, per bucket and in total, for all three
   weapons, against both this port's own golden capture AND
   `03-weapon-geometry-api.md`'s independently-measured reference numbers
   (rifle: 11 buckets / 60,125 verts / 53,692 tris — reproduced bit-for-bit
   by my own Node capture before any Rust code existed, confirming the
   capture methodology itself is sound). Triangle count is fixed by
   triangulation and untouched by welding, so an exact match here is a
   strong, weld-independent signal that the algorithm is the same.
2. **`geometry_assert::assert_triangle_soup_matches` (the existing shared
   helper) reports large false failures on whole-model buckets** —
   `rifle.alu`, `smg.alu`, `pistol.polymer` came back with "worst deviation"
   up to 2.0 in a *unit normal component* (2.0 being the maximum possible,
   i.e. two nearly-opposite-facing triangles paired together). That is not a
   plausible geometry bug for meshes whose triangle counts already matched
   exactly built from the same primitives.
3. **Diagnosed the cause directly, not assumed**: `geometry_assert`'s
   canonicalization sorts triangle corners on a `POS_GRID = 5e-3` (5 mm)
   per-field grid, deliberately coarse so noisy duplicates of ONE feature's
   own vertices fall through to a real tie-break rather than scattering —
   correct at single-part scale. A whole assembled weapon is not one
   feature: it merges 15-40 of them into one bucket, and several repeat at a
   pitch under 5 mm (2.6 mm pistol-grip stipple pyramids, ~9 mm Picatinny
   teeth whose corners cluster well inside 5 mm of each other, M-LOK
   pockets, knurl bands). At that density the coarse grid buckets corners
   from *physically different* triangles together and its float tie-break
   pairs them arbitrarily.
4. **Proved it with a finer, whole-assembly-appropriate comparison**: wrote
   `weapons_models_port.rs`'s own `assert_bucket_matches`, keyed on each
   triangle's own centroid rounded to `1e-5` m (~0.01 mm — far below any
   repeated feature's pitch, so corners from distinct triangles cannot
   collide), trying all three cyclic corner-rotations of a match candidate
   (a triangle's `canonicalize`-chosen starting corner can itself differ
   between two independently-welded meshes at a near-tied corner, without
   that being a winding bug). Result, across `9532 + 10584 + 13260 = 33376`
   triangles in the three previously-failing buckets: **every** triangle
   pairs to a golden triangle with position/normal agreeing to `1e-9`-`1e-7`
   (literal `f32` rounding-noise floor). The handful (16-17 per bucket, well
   under 1%) that needed the nearest-centroid fallback (i.e. didn't land in
   the *exact* same `1e-5` cell) are all at that same noise floor too — a
   centroid nudged across a quantization boundary by the last bit of an
   `f32`, not a displaced triangle.
5. `uv` gets its own, much wider tolerance (`0.3`) for the same
   already-documented reason `weapons_parts_controls_port.rs` and
   `weapons_parts_magazine_port.rs` establish: `extrude()`'s projection-axis
   choice is a discrete `<` comparison between two side-length magnitudes,
   so a sub-tolerance position difference can flip it and produce a `uv`
   difference far bigger than any float-noise budget on an otherwise
   perfectly correct triangle. Measured up to `0.21` at whole-model scale
   here (more axis-tie opportunities than a single part) — not a new defect,
   the same one, just more chances to trigger it.

So the isolated-part test's own diagnosis was directionally right (a
libm/weld-tie residual, not a bug) but the actual mechanism at whole-model
scale needed its own, finer comparator to prove rather than assume — which is
exactly why this test does not reuse `geometry_assert::assert_triangle_soup_matches`
directly; see the long module doc in `weapons_models_port.rs` for the full
writeup and the `assert_bucket_matches` implementation.

## Verification

- `cargo test -p axiom-shmup` — pass (all test binaries green,
  including the new `weapons_models_port` with 6 tests).
- `cargo xtask check-architecture` — pass, exit 0.

## Nothing was left unported

All three `build*()` functions are fully ported: every part call, every
direct geometry op (`smg.js`'s raw charging-handle `translate`/`rotateY`,
`pistol.js`'s stippling-loop `translate`), every node, with no gaps and no
dropped comments.
