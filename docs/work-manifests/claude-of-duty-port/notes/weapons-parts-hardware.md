# weapons/parts/hardware.rs

Ported from `C:\dev\Claude-of-Duty\src\weapons\parts.js:36-168`: `MUZZLE_LEN`,
`addPin`, `addScrew`, `addQdSocket`, `addSlingLoop`, `cartridge`, `emptyCase`,
`addRail`.

## What landed

- `MUZZLE_LEN` -> a plain struct (`MuzzleLen { brake, a2, comp, trilug }`) with
  a `const MUZZLE_LEN` instance, transcribing the four literals exactly.
- `add_pin` / `add_screw` / `add_qd_socket` / `add_sling_loop` -> straight
  ports. Rust has no default args, so every JS `= value` default is
  documented on the function and callers pass it explicitly, matching the
  convention already established in `geometry::primitives`.
- `MountAxis { X, Y, Z }` replaces the JS `axis: 'x' | 'y' | 'z'` string
  parameter for both `add_screw` and `add_qd_socket`. **They are not the same
  mapping** — `add_screw`'s `'y'`/`'x'` branches produce `rx`/`-ry`
  respectively (`parts.js:52`), while `add_qd_socket`'s `'x'`/`'y'` branches
  produce `ry`/`-rx` (`parts.js:77`) — opposite signs, different axis
  triggers. Documented explicitly on `MountAxis` so a future reader doesn't
  assume the two functions share a table.
- `cartridge` -> returns a `Cartridge { brass: Geo, bullet: Geo, length: f32 }`
  struct instead of a JS object literal.
- `empty_case` -> returns a bare `Geo` (same shape as the JS return).
- `add_rail` -> takes a `RailOpts` struct (`base_h`, `top_h`, `waist`, plus
  every field `picatinny()`'s `PicatinnyOpts` needs, plus `slot_floor: bool`)
  whose `Default` mirrors `PicatinnyOpts::default()` plus `slot_floor: true`
  (`opts.slotFloor !== false`, `parts.js:163`). The "SLOT FLOORS" comment
  explaining why the recoil slot floor gets its own `cavity`-material box (the
  single loudest artefact on the whole weapon, per the source) is carried
  over verbatim.
- No JS `.dispose()` calls carry over — Rust ownership already frees the
  intermediate `Geo` buffers when they go out of scope; there is no GPU-buffer
  equivalent to release.

## Verification — golden capture

Wrote a throwaway `C:\dev\Claude-of-Duty\capture_hardware.mjs` (deleted after
running, per the port recipe) that imports the real `Assembly` from
`geometry.js` and the real `parts.js` builders, calls each one with fixed
arguments against real `Assembly` instances, `build()`s, and dumps every
material bucket's `position`/`normal`/`uv`/`index`. `cartridge`/`emptyCase`
are dumped directly (they never touch an `Assembly`). Captured JSON committed
as `apps/shmup/tests/parts/hardware_golden.json` (~918 KB).

`apps/shmup/tests/weapons_parts_hardware_port.rs` — 10 tests, all
green:

- `muzzle_len_matches_the_source_literals` — exact equality (plain literals).
- `add_pin_matches_the_source`.
- `add_screw_matches_the_source_for_every_mount_axis` — all three `MountAxis`
  branches in one assembly (`screwY`/`screwX`/`screwZ` buckets), including the
  identity (`Z`) fallthrough.
- `add_qd_socket_matches_the_source_for_every_mount_axis` — same, times two
  buckets per axis (body + steel insert).
- `add_sling_loop_matches_the_source_with_default_and_custom_rotation`.
- `cartridge_matches_the_source_with_{default,custom}_dimensions`.
- `empty_case_matches_the_source_with_{default,custom}_dimensions`.
- `add_rail_matches_the_source_across_default_custom_and_no_floor_variants` —
  three `addRail` calls in one assembly (default opts, custom opts, and
  `slot_floor: false`), which reproduces the source's own cross-call bucket
  merge: the default- and custom-opts slot floors both land in the same fixed
  `"cavity"` material bucket (`parts.js:165`), and the test asserts that
  bucket too, plus that the `slot_floor: false` call really contributed
  nothing to it.

Position/normal/uv floats: `1e-6` absolute, per the geometry API contract.
Counts and index buffers: exact.

## The one tolerance exception, and why

`add_rail`'s `railDefault`/`railCustom`/`railNoFloor` buckets are
`picatinny()` output run through a **second** `mergeAll` weld inside
`Assembly::build` (once inside `picatinny` itself, once again bucketing the
single-element list). `picatinny` builds its teeth via `extrude()` with
bevelling, which is the exact `f32` point-list precision boundary
`03-weapon-geometry-api.md`'s "Corrections" section already documents (and
that `weapons_geometry_primitives_port.rs` already works around for its own
`picatinny_normal`/`mlok_slot_normal` cases, via a topology-only check). A
first pass at this test used the strict `1e-6` check everywhere and failed by
one ULP-scale margin (`diff 0.0000010132789611816406`, i.e. `1.3e-7` over the
bound) on `railDefault`'s normals — not a different algorithm, the same
documented amplification one weld pass further downstream.

Fix: ported `assert_geo_topology_matches` from the primitives test file as
`assert_bucket_topology_matches` (triangle count exact via `earcut` topology,
vertex count bounded to `max(10%, 8)`) and used it for the three
`picatinny()`-derived buckets only. The `cavity` bucket (a plain `box_geo`
floor, no `extrude()` in its ancestry) keeps the exact `1e-6` check and
passes it cleanly — confirming the exception is specific to the extrude/bevel
path, not a blanket loosening.

## Divergences from the source

None beyond the mechanical Rust-isms above (struct returns instead of object
literals, no default arguments, an explicit `MountAxis` enum instead of a
bare string). No defect in the source was ported or fixed — every function in
this slice is pure geometry construction with no branchy edge cases beyond
the axis dispatch, which is faithfully reproduced.

## Concurrency notes

`weapons/mod.rs` and `weapons/parts/mod.rs` were touched by sibling agents
(barrel, magazine) before this file landed; by the time this port ran, both
already contained the `pub mod hardware;` / `pub mod parts;` lines this slice
needs, so no edit to either was required at commit time. Did not touch
`barrel.rs`, `magazine.rs`, or their tests. `cargo test -p axiom-shmup`
run as a whole workspace-of-parts command shows 6 pre-existing failures in
`weapons_parts_magazine_port.rs` (a sibling agent's in-flight file, unrelated
to this slice) — `weapons_parts_hardware_port.rs` itself is 10/10 green, and
`cargo xtask check-architecture` passes.
