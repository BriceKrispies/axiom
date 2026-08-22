# `ai/soldier.js` → `apps/shmup/src/ai/soldier.rs`

Ported from Claude-of-Duty `src/ai/soldier.js:1-837` (837 lines, all of it).

| file | what |
|---|---|
| `apps/shmup/src/ai/soldier.rs` | the port |
| `apps/shmup/tests/ai_soldier_port.rs` | the golden test |
| `apps/shmup/tests/ai_soldier/capture.mjs` | the Node capture, run against the original |
| `apps/shmup/tests/ai_soldier/golden.json` | 303 KB, byte-reproducible |

**Status: integrated, 7/7 golden tests pass.** Originally written under
`07-fanout-brief.md`'s no-build rule; the golden was captured up front by
running the original JavaScript under Node 24, which is what made the
integration cheap. See "Integration reconciliation" at the bottom.

---

## SEAMS ASSUMED FROM SIBLING MODULES — read this first

`soldier.js` is pure composition: it calls `rig`, `geo`, `parts`, `weapon` and
`textures` and does no geometry of its own. None of those existed on disk when
this was written. Here is **every** symbol assumed and the signature assumed
for it. The integration pass reconciles this list; nothing else in the module
touches a sibling.

### `crate::ai::rig`

| symbol | assumed |
|---|---|
| `RIG` | a `static` (`LazyLock<Rig>` is fine — `&RIG` deref-coerces) |
| `Rig::index(&self, name: &str) -> usize` | panics on an unknown bone, as JS throws |
| `Rig::bind_pos` | a public field, indexable, yielding `[f64; 3]` (`Copy`) |
| `GRIP_R` | `[f64; 3]` |
| `BORE_DIR` | `[f64; 3]` |

`GRIP_L` is imported by `soldier.js` and **never used** — a dead import. Rust
cannot spell an unused import without a warning, so it is not imported; it is
recorded here rather than silently dropped.

### `crate::ai::geo`

| symbol | assumed |
|---|---|
| `Mesh` | `{ p: Vec<f64>, n: Vec<f64>, uv: Vec<f64>, i: Vec<u32> }`, `Clone` |
| `Noise::new(rng: Rng) -> Noise` | takes the forked `Rng` by value |
| `CharacterBuilder::new(rig: &Rig, noise: &Noise, materials: &[(&str, f64)])` | the JS `{ noise, materials }` options bag, with the `MATERIALS` table as `(name, tile)` pairs |
| `CharacterBuilder::occlude(&mut self, a: [f64;3], b: [f64;3], r: f64, k: f64)` | |
| `CharacterBuilder::add(&mut self, mesh: Mesh, o: PartOptions)` | |
| `CharacterBuilder::build(self) -> Built` | by value or `&self`, either compiles here |
| `Built` | `{ geometry: CharacterGeometry, material_names: Vec<String>, vertices: usize, triangles: usize, parts: Vec<PartRange> }` |
| `PartRange` | `{ name: String, material: String, start: usize, count: usize }` |
| `CharacterGeometry` | the interleaved buffers; the test reads `uv: Vec<f32>` and `colour: Vec<f32>` |
| `PartOptions` | `Default`, with `material: &'static str`, `bones: Option<Vec<String>>`, `bone: Option<String>`, `bias: Option<Vec<f64>>`, `colour: Option<[f64;3]>`, `grime/dirt/dust/wear: Option<f64>`, `name: String` |

`PartOptions`' `Option` fields are not decoration: `CharacterBuilder._shade`
applies `?? 0.5` / `?? 0.22` / `?? 0` defaults that differ per field, so
"absent" and "explicitly 0.5" must stay distinguishable.

### `crate::ai::parts` (imported as `p`)

Function names are the JS names snake-cased. Options bags become structs with
`Default` matching the JS `?? ` defaults.

`jacket_torso(&Noise, &JacketOpts)` · `pelvis(&Noise)` · `collar(&Noise)` ·
`limb_tube(&Noise, [f64;3], [f64;3], [f64;3], &[f64], &LimbOpts)` ·
`shoulder_cap(&Noise, [f64;3], f64)` · `pouch(&Noise, &PouchOpts)` ·
`knee_pad(&Noise, [f64;3], f64)` · `boot(&Noise, [f64;3], f64)` ·
`boot_sole([f64;3])` · `boot_laces([f64;3])` · `plate_carrier(&Noise)` ·
`carrier_webbing()` · `belt(&Noise)` · `hip_pouch(&Noise, f64)` ·
`head_mesh(&Noise, [f64;3], &HeadOpts)` · `nose(&Noise, [f64;3])` ·
`ear(&Noise, [f64;3], f64)` · `eyeball([f64;3], f64)` ·
`face_wrap(&Noise, [f64;3])` · `helmet(&Noise, [f64;3])` ·
`helmet_hardware(&Noise, [f64;3])` · `chin_strap([f64;3])` ·
`goggles([f64;3], bool) -> Goggles { frame, strap }` ·
`goggle_lens([f64;3], bool)` · `head_scarf(&Noise, [f64;3])` ·
`sunglasses([f64;3]) -> Sunglasses { frame, lens }` ·
`glove(&Noise, [f64;3], [f64;3], [f64;3], f64)` ·
`knuckle_guard([f64;3], [f64;3], [f64;3])` · `sling([f64;3], [f64;3])`

Opts structs assumed: `JacketOpts { flare, bulk }`, `HeadOpts { wide }`,
`LimbOpts { rings: usize, seg: usize, fold, crease, bend: [f64;3], flat,
cap_start, cap_end, up }`, `PouchOpts { hx, hy, hz, x, y, z, rx, ry, rz,
lid_tilt, bend }`. All `f64` unless noted.

**`face_wrap`, `helmet` and `plate_carrier` are called WITHOUT the variant
record.** The source passes `V` to all three and `parts.js` reads nothing out
of it — a dead argument in all three cases. If the ported `parts` keeps the
parameter, the three call sites need it back; the comment is at each site.

### `crate::ai::weapon`

`build_weapon(&Noise, &str, &mut Rng) -> Weapon`, where `Weapon` has
`steel`/`polymer`/`rubber`/`glass: Mesh` and
`muzzle`/`bore_origin`/`ejection`/`stock_top`/`foregrip`/`mag_bottom: [f64;3]`.
`build_weapon` **draws one `rng.range(-0.10, -0.03)`** (`weapon.js:263`, the
receiver cant) from the shared stream — that draw is the last one in the
assembly and is pinned by the golden's post-build RNG state.

### `crate::ai::textures`

`pub const CLOTH_TILE: f64 = 0.78;` — a `const`, because `MATERIALS` is a
`const` array that embeds it.

---

## What the golden pins, and at what tolerance

The capture patches `CharacterBuilder.prototype.{occlude,add,build}` **in the
capture script**, never in the read-only source repo, and wraps the `Rng` and
the material factory in recorders. It then builds all three variants plus one
unknown name with `new Rng(7).fork()` (the seed `selftest.mjs` uses).

| pinned | tolerance | why |
|---|---|---|
| `VARIANTS` (every field of all three), `MATERIAL_SLOTS`, the emitted material-group order per variant, vertex/triangle totals, every part's `start`/`count`, `resolve_materials`' every field | **exact** | integers, strings, or literals / a single IEEE division (`0.85 / 0.905`, `tile / DETAIL_TILE`) |
| the RNG state after each build | **exact** | one extra or missing draw moves it — the sharpest available check on draw order and count |
| the 28 occlusion proxies; per-`add` mesh bbox + centroid | `1e-9` | rig bind positions chain `sqrt`; `parts.js` lofts run `sin`/`cos`/`pow`, not bit-guaranteed across libm |
| per-part uv and vertex-colour min/max/mean | `1e-6` | read back off `Float32Array`s — f32 resolution |

### Why the golden is a recipe, not a triangle soup

`soldier.js` produces no geometry of its own; every triangle comes from
`parts.js` / `geo.js` / `weapon.js`, each a separate slice with its own
golden. Dumping ~16.7k vertices × 4 runs would mostly re-test those slices and
would fail for *their* bugs. So each `add` is fingerprinted by vertex count,
triangle count, position bounding box and position centroid. A wrong offset, a
swapped side, a mistyped radius or a reordered part all move that fingerprint;
a bug inside `parts.js` stays attributable to `parts.js`.

`tests/geometry_assert/mod.rs` was read and deliberately **not** used: it
compares two triangle soups, and there is no triangle soup here that this
slice authors. It was not edited.

The per-part **uv extent** is the one thing that pins `MATERIALS[...].tile` for
`skin`, `polymer`, `steel`, `rubber` and `glass` — `resolveMaterials` only
exposes the cloth/plate/gear/boot tiles (through `detail.scale`), and the other
five are otherwise invisible from outside `CharacterBuilder`. The per-part
**colour extent** pins this module's `colour`/`grime`/`dirt`/`dust`/`wear`
budget through the real vertex bake, which is what pins the `GEAR` table (whose
values the source does not export).

### Measured, from the capture

```
vanguard   16731 v / 25698 t   54 adds   28 occluders   8 rng draws   weapon glass 238 v
irregular  13808 v / 21374 t   47 adds   28 occluders   6 rng draws   weapon glass   0 v
breacher   16731 v / 25698 t   54 adds   28 occluders   8 rng draws   weapon glass 238 v
```

All four runs emit the material groups in exactly `MATERIAL_SLOTS` order, so
the source's `console.warn` prewarm guard never fires — the golden records
`warnings: []` and the test asserts that.

`irregular`'s AK produces **no glass**, so `if (W.glass.p.length)` skips the
`wpnGlass` part on that variant only — but it still has a `glass` group,
because `shadeLens` is glass. That is a real behavioural fork and it is pinned.

---

## Trap checklist

- **`Float32Array`.** Grepped. `soldier.js` has none, but `geo.js`'s
  `CharacterBuilder.build()` stores position/normal/uv/colour/skinWeight as
  `Float32Array` and the index buffer as `Uint16Array`/`Uint32Array` (switching
  at 65535 vertices — a soldier is 16.7k, so always `Uint16Array`). That is
  `geo`'s to get right; this slice's golden reads those buffers back and holds
  them to f32 tolerance, which is the only place the width shows up here.
- **Euler order.** No Euler angles in this file. The `rx`/`ry`/`rz` fields the
  pouch/cargo call sites set are `parts.js`'s `place()`, which builds a
  `'YXZ'` Euler — `parts`' problem, flagged for that slice.
- **Matrix storage order.** No matrices in this file.
- **Float arithmetic is not associative.** Every expression is transcribed with
  the source's grouping and left-to-right order —
  `el[0] * 0.98 + sh[0] * 0.02`, `(t - 0.5) * (nPouch > 2 ? 0.156 : 0.09)`,
  `GRIP_R[1] + BORE_DIR[1] * 0.4 + 0.09`. The mesh centroid in both the capture
  and `Assembly::add` sums in vertex order and divides once at the end, so the
  two agree bit-for-bit modulo libm.
- **An enum used as a table index is order-dependent.** This is *the* trap for
  this file. `MATERIAL_SLOTS` is a nine-entry order that decides three's opaque
  draw order within one soldier (see the constant's doc: prewarming in a
  hand-written order moved 2 pixels of the `combat` shot by 1/255). The emitted
  order is a function of the add order, so it is pinned per variant *and* the
  source's own guard is ported and asserted.
- **A matching count is not proof.** Counts are asserted, but so are the bbox,
  the centroid, the vertex ranges, the uv extents and the colour extents of
  every part — and the RNG state after the build.
- **`sign` is not `signum`.** No `Math.sign` here. `normalize3` spells out
  JavaScript's `length() || 1` zero case explicitly rather than reaching for
  `signum`, for the same family of reason.
- **`Math.hypot` is not `sqrt(x*x+y*y+z*z)`.** No `hypot` in `soldier.js`.
  `normalize3` deliberately uses Three's own `sqrt(x*x + y*y + z*z)`, **not**
  `hypot`, because Three does.
- **Dead computation in the source is still part of the source.** Three dead
  things preserved: the unused `GRIP_L` import (documented, not imported), the
  dead third argument to `faceWrap`/`helmet`/`plateCarrier` (documented at each
  call site), and the `nPouch === 1 ? 0.5 : …` arm that can never be taken
  (ported with the dead arm intact).
- **Your comparator can be the bug.** The mesh fingerprint is a bbox and a
  centroid, both computed identically on the two sides; there is no sorting or
  pairing step that could mispair anything.

## Divergences from the source, and why

1. **`resolve_materials` returns data, not `THREE.Material`s.** It yields
   `MaterialRequest` values carrying the exact set, cache key, tint, roughness,
   metalness, normal scale, ao and detail record the source hands
   `SoldierMaterials.get()`. Building a GPU material is the render tier's job.
   The `Option` fields are load-bearing: the source's material cache key is
   built from `opts.rough ?? ''`, so an absent option and an explicitly-default
   one are different cache entries. `resolve_materials` touches no RNG, which
   is the whole reason the source split it out of `buildSoldier` (prewarming
   must not move the shared stream).
2. **`occlusion_proxies()` is a function.** 28 consecutive `B.occlude(...)`
   calls become one list, consumed in order. It is a pure function of the rig,
   so this makes it directly testable without a `CharacterBuilder`; the order
   and the values are unchanged.
3. **The two magazine-pouch RNG draws are bound to locals** before the
   `PouchOpts` literal, because the Rust field order differs from the JS
   property order and JavaScript evaluates properties in source order (`y`
   first, then `rz`).
4. **`Assembly` wraps `CharacterBuilder`** to record the add sequence
   (`AddRecord`). The source already returns a per-part list for the same
   purpose ("so the albedo audit in `selftest.mjs` can report the effective
   value of every single piece of kit") — that one is material-sorted and
   carries only vertex ranges, so it cannot say which builder was called with
   what. Call sites still read `b.add(mesh, opts)`, identical to the source.
5. **The weapon meshes are cloned into the builder**, because the source hands
   the same JS objects both to `B.add` and to the returned `weapon` field.
6. **`console.warn` becomes `SoldierBuild::warnings`.** The prewarm guard's
   message is produced as data instead of printed, so the test can assert it is
   empty.

## Not ported / open

Nothing in `soldier.js` is unported. The single thing the port cannot express
today is the material *object* identity the source's ordering argument rests
on (`THREE.Material` ids incrementing in creation order): that is a render-tier
concern, and the ordering contract is preserved here as `MATERIAL_SLOTS` plus
the guard.

---

## Integration reconciliation

### The one failure, and which side was wrong: the comparator

`variants_table_matches_source` failed on `variant names / declaration order`.
Neither the port nor the golden's *content* was wrong — **the test's
comparator was**. It derived the expected order from
`golden()["tables"]["VARIANTS"].as_object().keys()`, and `serde_json::Map` is a
`BTreeMap` unless the `preserve_order` feature is enabled (it is not, in this
workspace). So the "golden order" it compared against was
`breacher, irregular, vanguard` — alphabetised by the JSON reader, with no
relationship to `soldier.js` at all.

A JSON object cannot carry order across that reader, so the fix is on the
capture side: `tables.VARIANT_ORDER` is now an ordered **array** of
`Object.keys(VARIANTS)`, and the test reads that, plus asserts it names every
key in the `VARIANTS` object so a name dropped from both sides cannot pass.
The Rust declaration order (`vanguard, irregular, breacher`) was correct all
along.

This is the recipe's **"your comparator can be the bug"** trap, in a new place:
not a mispairing tolerance, but a serialiser that silently reorders. Worth
remembering for any other slice pinning a key order out of a JSON object —
`MATERIAL_SLOTS` and `materialNames` were already captured as arrays and were
never at risk.

### Seams, as actually landed

The assumed-seam table above is kept as written (it is what the integration
reconciled against). What differed:

| assumed | landed |
|---|---|
| `textures::CLOTH_TILE` as `const` | `const` — held |
| `GRIP_R` / `BORE_DIR` as `[f64; 3]` values | `LazyLock`; the two by-value sites read `*GRIP_R` / `*BORE_DIR`. Correct — they are sqrt-derived and the rig rightly refused to hardcode them |
| `RIG.bind_pos[RIG.index(name)] -> [f64; 3]` | `RIG.bind_pos_of(name)`; the field is `V3` |
| `PartOptions.material: &'static str` | `String`; 45 call sites take `.to_string()`, `AddRecord.material` is `String` |
| `Noise::new(rng: Rng)` | `Noise::new(&mut Rng)` |
| `build_weapon(&Noise, &str, &mut Rng)` | `build_weapon(&Noise, WeaponStyle, Option<&mut Rng>)`. `WeaponStyle::from_name` maps `"ak"` -> `Ak`, everything else -> `Carbine`, which is exactly `weapon.js:67`'s `const long = style === 'ak'`; `Some(rng)` is the live-rng arm at `weapon.js:263`. Checked against the source — faithful, and the post-build RNG-state assertion would have caught it if it were not |
| `Built { geometry, .. }` | `build()` returns the `CharacterGeometry` directly and it carries `material_names`/`parts`/`vertices`/`triangles`; `SoldierBuild` clones those out before moving it, so the struct still mirrors the source's returned object |
| `CharacterGeometry.colour` | `color` — `geo.js:566` names the Three attribute `'color'` while the rest of that file spells it `colour`; `geo.rs` keeps both |
| everything else (the 30 `parts` builders, the four opts structs, `PartRange`, `Weapon`'s meshes and anchor points) | as assumed |

None of these moved a captured value, which the goldens then confirmed: the
per-`add` fingerprints, the per-part uv/colour extents and the post-build RNG
state all matched on the first run.
