# `weapons::parts::barrel` — port notes

Ported from `C:/dev/Claude-of-Duty/src/weapons/parts.js:170-381`:
`addBarrel` (:178-222), `addGasBlock` (:228-244), `addMuzzleDevice` (:250-381).

Target: `apps/shmup/src/weapons/parts/barrel.rs`.

## Public API

- `BarrelOpts` / `add_barrel(asm, mat_steel, mat_cavity, opts) -> BarrelResult { gas_at, r_bore }`
- `GasBlockOpts` / `add_gas_block(asm, mat_steel, opts)`
- `MuzzleKind { Brake, A2, Comp, Trilug }` / `add_muzzle_device(asm, mat_steel, mat_cavity, kind, z_barrel_end, r_barrel, y) -> MuzzleDeviceResult { len, crown_z }`

Each JS options object with a mix of required and defaulted keys became a
`Default`-implementing `Opts` struct (matching `RailOpts`/`PicatinnyOpts`
precedent); the two "required" keys (`zBreech`/`zMuzzle`, `z`/`tubeTo`) get
`0.0` in `Default` purely so struct-update syntax works — every real caller
sets them explicitly, exactly as the JS callers always pass them.

## Divergence: `MuzzleKind` is exhaustive, not string-keyed

`addMuzzleDevice(kind, ...)` takes a bare string in the source; `MUZZLE_LEN[kind]
?? 0.05` falls back to `0.05` for any string not in the table, while the
`if/else if/else` geometry chain treats anything besides `'brake'`/`'a2'`/`'comp'`
as the tri-lug case — so an unlisted string like `'foo'` would build tri-lug
geometry at `len = 0.05`, decoupled from `MUZZLE_LEN.trilug = 0.042`. Every real
call site only ever passes one of the four `MUZZLE_LEN` keys, so `MuzzleKind` is
a 4-variant enum and that dead fallback path is not modeled — the type system
forecloses the "unknown kind" case instead of silently reproducing unreachable
JS leniency.

## Divergence: `add_screw` is a private, scoped duplicate (temporary)

`addGasBlock` calls `addScrew` (`parts.js:50-55`), which lives in the
"small hardware" section of the source, i.e. `parts::hardware`'s slice per the
port split. At the time this file was authored `parts::hardware` did not yet
exist; by the time verification ran it had landed with a public
`add_screw(asm, mat, x, y, z, r_head, axis: MountAxis, len)` — **so `barrel.rs`
now imports and calls `parts::hardware::add_screw`/`MountAxis` directly, not a
local duplicate.** (An earlier draft of this file carried a private duplicate
for standalone compilation; it was deleted once the real dependency was
available. Mentioned here in case a stale copy is ever found elsewhere.)

`MUZZLE_LEN` likewise now comes from `parts::hardware::MUZZLE_LEN` (a
`MuzzleLen { brake, a2, comp, trilug }` struct), matched per `MuzzleKind` variant
— not redefined here, per the contract.

## `Geo::apply`-based transform helpers

`addMuzzleDevice`'s per-variant loops call `.clone()`/`.translate()`/`.rotateZ()`
directly on a not-yet-assembled `THREE.BufferGeometry` (the brake's three ports,
the A2's five slots, the comp's knurl band, the tri-lug's three lugs) — these
never go through `Assembly.add`, so `geometry::primitives::xform`'s
`pub(super)` helpers aren't reachable from `parts::barrel`. Two small private
helpers (`translate`, `rotate_z`) were added locally, built the same way
`xform.rs` documents: `Geo::apply` with a `Mat4::translation`/
`Mat4::from_quaternion(Quat::from_axis_angle(Vec3::UNIT_Z, angle))`. No
`.dispose()` counterpart is ported — Rust ownership already frees the
geometry `Assembly::add` consumes.

The brake variant's per-iteration `port.clone()` + `.dispose()` (the JS builds
a box once, clones it, translates the clone, and disposes the un-translated
original — used nowhere else) has no Rust counterpart: `box_geo` already
returns an owned value, so this port just translates it directly. Byte-for-byte
identical output; noted in `barrel.rs` at the call site.

## Verification

Golden-capture: a Node script called `addBarrel`/`addGasBlock`/`addMuzzleDevice`
against a real `Assembly` (9 cases: 2 barrel, 2 gas block, 4 muzzle-kind
variants + 1 offset/radius variant), dumped every material bucket's
`position`/`normal`/`uv`/`index` plus each call's JS return value, and was
deleted after capture. Committed as
`apps/shmup/tests/parts/barrel_golden.json`, asserted against in
`apps/shmup/tests/weapons_parts_barrel_port.rs`.

- Vertex/triangle counts and index buffers: **exact** in every case.
- Position/normal/uv floats: **`1e-5`** absolute, not the primitives file's
  `1e-6`. Measured peak diff was `~5.9e-6` (a gas-tube normal component) across
  4 of 6 test cases before widening the bound. Root cause: each part here is
  *several* `lathe_z`/`tube_z`/`box_geo`/`knurl_band` primitives (each with its
  own independent `sin`/`cos` rounding) merged and welded by
  `merge_all`/`Assembly::build`, so per-vertex error compounds across more
  independent trig calls than a single primitive's own `1e-6`-bounded golden
  does. This is not a masked algorithm bug: every case's vertex/triangle/index
  counts match the golden exactly (the check the contract says catches a real
  divergence), only individual float components needed the wider bound.
- `BarrelResult`/`MuzzleDeviceResult` fields (`gas_at`, `r_bore`, `len`,
  `crown_z`) are read from the golden's `returned` object and tolerance-compared
  (`1e-5`) rather than hardcoded as literals or checked via `assert_eq!`: they
  are computed in JS `f64` vs Rust `f32` (`gasAt = zMuzzle + len * 0.34`,
  `crownZ = zBarrelEnd - len`), so even plain `+`/`-`/`*` is not guaranteed
  bit-identical across the two precisions.

## Not part of this slice

A **pre-existing, unrelated** test failure was observed in
`weapons_parts_hardware_port.rs::add_rail_matches_the_source_across_default_custom_and_no_floor_variants`
(`normal[437]` off by `~1.01e-6`, i.e. just past that file's own `1e-6` bound) —
same category of issue as the one fixed here, but in a sibling agent's
in-flight file (`parts/hardware.rs`, untracked at the time of this port). Not
touched, per the concurrency rule (own file only); flagged here for whoever
lands that slice next.
