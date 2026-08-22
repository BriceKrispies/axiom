# `ai/weapon.js` → `apps/shmup/src/ai/weapon.rs`

Source: `C:/dev/Claude-of-Duty/src/ai/weapon.js:1-291` (291 lines, ported in
full — every statement, including the dead ones).

Files written:

| path | what |
|---|---|
| `apps/shmup/src/ai/weapon.rs` | the port |
| `apps/shmup/tests/ai_weapon_port.rs` | the Rust test (reads the golden) |
| `apps/shmup/tests/ai_weapon/capture.mjs` | the Node capture, runs the original |
| `apps/shmup/tests/ai_weapon/golden.json` | 2.6 MB, byte-reproducible |

---

## First: the slice brief describes a different file

The fan-out brief for this slice called `ai/weapon.js` "the AI-carried weapon
facade: the muzzle-flash / tracer / shell-eject events an agent fires through,
and the carried weapon model", and pointed at `weapons/defs.rs`,
`weapons/ballistics.rs`, `weapons/clips.rs`, `weapons/models/`, `fx/muzzle.rs`,
`fx/tracers.rs`, `fx/shells.rs` and `events.rs` as things to compose.

**`weapon.js` uses none of that, and the port therefore uses none of it
either.** The whole file is one exported function, `buildWeapon(nz, style,
rng)`, which is a pure procedural *geometry* builder. It has:

- no events, no `EventBus`, no `weapon:fire`;
- no ballistics, ammunition, fire rate, spread or damage;
- no FX of any kind;
- no reference to `src/weapons/` (the *player's* viewmodel family, which is a
  separate model set in a separate frame — the two share no code in the
  source);
- exactly two imports: `./geo.js` (the AI geometry toolkit) and `./rig.js`
  (two bind-pose constants).

The description in `ai/agent.rs`'s module doc ("muzzle-flash/tracer/shell
events fired through `weapon.js`'s facade") is loose in the same way. What
`agent.js`'s `_fireRound` actually does is read the *anchor points* this
builder returns — `W.muzzle`, `W.ejection` — and hand them to `fx/`. The
events live in `agent.js:734-787` and `ai/index.js`, both still deferred.
`weapon.js` supplies the six anchors and nothing else.

So the correct reading of this slice is: **the enemy's rifle mesh, and the six
bind-space attachment points the (unported) firing code will need.** Composing
`fx/` or `weapons/` here would have invented a seam the source does not have.

---

## What the port is

One function, transcribed statement for statement, plus two local helpers:

- `box_at(...)` = the source's local `box()` (`weapon.js:20-33`). Renamed only
  because `box` is a reserved word in Rust.
- `cyl(...)` (`weapon.js:36-50`).
- `bind_matrix(rng)` = `weapon.js:258-267`, factored out of `build_weapon`
  (see "One deliberate factoring" below).
- `build_weapon(nz, style, rng)` = `buildWeapon`.

Types:

- `WeaponStyle::{Carbine, Ak}`. The source takes a string and derives exactly
  one boolean from it (`const long = style === 'ak'`), so anything that is not
  `'ak'` is a carbine. Two variants say the same thing without an
  unrepresentable third state.
- `Weapon { steel, polymer, rubber, glass, matrix, muzzle, bore_origin,
  ejection, stock_top, foregrip, mag_bottom }` = the returned object literal
  at `weapon.js:277-290`, field for field (`poly` is returned as `polymer` in
  the source too).
- `rng: Option<&mut Rng>` — `weapon.js:263`'s `rng ? … : -0.06` is a real
  branch, not defensive padding, and `selftest.mjs` exercises it.

### One deliberate factoring

`bind_matrix` is the only structural divergence, and it is a pure extraction:
`weapon.js:258-267` inlined in `buildWeapon`. It is pulled out because it is
the one part of this slice that touches **no** `ai/geo` symbol at all — it is
`ai/rig` constants plus the single `rng` draw — so it can be pinned against the
golden while `ai/geo.rs` is still landing. `build_weapon` calls it at exactly
the point the source runs those lines (after all the geometry), which is also
the only point at which the RNG stream is touched, so the draw order is
unchanged.

---

## Determinism

One RNG draw in the whole file: `rng.range(-0.10, -0.03)` at `weapon.js:263`
(the cant angle), or the literal `-0.06` when `rng` is absent. **No
`rng.fork()` anywhere in `weapon.js`** — the forking happens in the callers
(`selftest.mjs:73` `buildWeapon(nz, style, rng.fork())`, `soldier.js:704`
passes the soldier's own stream straight through), and `build_weapon` takes an
already-forked `Rng` so the caller keeps control of the order, matching the
convention `ai/mod.rs` already documents for `Agent::new`/`Squad::new`.

The surface variation is *not* random: it comes from the caller's shared
`Noise` field (`nz`), which is state-free after construction, so building
`carbine` then `ak` then the two no-rng cases in one process is order-
independent for everything except the cant.

---

## The golden

`tests/ai_weapon/capture.mjs` runs the original under Node 24 with the
source's own headless driver, `src/ai/selftest.mjs:19-20,72-73`:

```js
const rng = new Rng(1234);
const nz  = new Noise(rng.fork());
for (const style of ['carbine', 'ak']) buildWeapon(nz, style, rng.fork());
```

Four cases: `carbine`, `ak` (in that fork order — it is part of the contract),
then `carbine_no_rng` and `ak_no_rng` *last*, because they consume nothing and
so cannot shift the two above.

Per case it dumps `matrix` (16, `Matrix4.elements`, column-major), the six
anchors, and `{v, t, p, n, uv, i}` for each of `steel`/`polymer`/`rubber`/
`glass`. Plus two diagnostics that are **not** asserted, because they belong
to other slices: `noisePerm` (`ai/geo.js`'s `Noise` permutation) and `rig`
(`GRIP_R`/`GRIP_L`/`BORE_ORIGIN`/`BORE_DIR`). They are committed so that when
a mesh comparison fails at integration, "is this `weapon.rs`, `geo.rs` or
`rig.rs`?" is answerable by reading the file.

`node capture.mjs` twice produced an identical MD5 (`fcf27f4d…`). 2,656,880
bytes.

### Counts the original produces

| case | steel v/t | polymer v/t | rubber v/t | glass v/t |
|---|---|---|---|---|
| carbine | 3227 / 4766 | 827 / 1278 | 119 / 192 | 238 / 320 |
| ak | 2136 / 3192 | 860 / 1320 | 119 / 192 | **0 / 0** |

### Tolerances

Set at integration, after the first run showed 8/9 passing and one miss. The
figures below are **derived from the algorithm**, not fitted to the miss — the
derivation is reproduced in full in the constants' doc comments in
`tests/ai_weapon_port.rs`.

| what | bound |
|---|---|
| triangle indices, vertex/triangle counts | **exact** (integers) |
| UVs, bake matrix, the six anchors | `1e-12` |
| `polymer` / `rubber` / `glass` positions and normals | `1e-12` |
| `steel` positions | `2e-11` |
| `steel` normals | `2e-10` |

Comparison is **index-aligned**, which is valid here: `appendMesh`
concatenates, `loft` emits rings in order, and nothing in `weapon.js` welds or
re-sorts. Vertex *k* of the port is vertex *k* of the original. (Contrast
`props_port.rs`, which needs a weld-invariant comparator.)

---

## The one divergence, and why it is not a defect

First integration run: **8 pass, 1 fail.**

```
carbine.steel.p: worst |delta| 1.1916e-11 > 1e-12 at [801]
got -0.11641279236933363, want -0.1164127923812501
```

Neither the port nor the golden is wrong. The storage-width trap was checked
first (it was the natural suspect, having been found three times in this wave)
and **ruled out**: this port is `f64` end to end — `M4` is `[f64; 16]`, `Q`/`V3`
are `f64`, every Three.js operation (`compose`, `makeBasis`, `setPosition`,
`applyMatrix4`, `setFromAxisAngle`, `setFromEuler('YXZ')`, `applyQuaternion`)
is transcribed locally in `weapon.rs` rather than routed through
`axiom_math` (f32) or the shared `weapons/geometry` kit. And the magnitude is
wrong for `f32`: an `f32` leak shows up at `~1e-7` relative, this is `~1e-10`.

### The mechanism

`superEllipse` (`geo.js:117-129`) evaluates `r * sign(c) * |c|^(2/n)`.

At any ring index where `4i/seg` is an integer, the cosine (or sine) of
`2*PI*i/seg` is **mathematically exactly zero**. What the expression actually
receives is a rounding residue of the libm's argument reduction, `~1e-16` —
and then takes its `(2/n)`-th root. `d(|c|^p)/dc` is *infinite* at `c = 0`, so
the residue is amplified by `|c|^(p-1)`. For the rail slot's `n = 8`
(`weapon.js:143`, `p = 0.25`) that factor is `6.35e11`.

Measured at `t = (12/16)*PI*2` (i.e. `3*PI/2`):

| | `cos t` |
|---|---|
| V8 (Node 24) | `-1.8369701987210297e-16` |
| Rust, `x86_64-pc-windows-msvc` | `-1.836909530733566e-16` |

`6.07e-21` apart absolutely — far below one ULP of the argument, and far
better than either implementation promises — but **`3.30e-5` apart
relatively**, and it is the relative difference the root propagates. `powf`
agrees to the last bit when given the same input; the entire disagreement is
the cosine. V8 ships its own fdlibm-derived `sin`/`cos` precisely so results
are platform-independent; the MSVC CRT does not use the same reduction.

Closing the loop analytically:

```
Δx = r · p · |c|^(p-1) · Δcos
   = 0.0125 × 0.25 × 6.35e11 × 6.0665e-21
   = 1.204e-11          vs   1.1916e-11 observed
```

So the vertex at that column is, in *both* implementations, `0.0125 ×
(rounding noise)^0.25 ≈ 1.5e-6` where the true value is `0`. It is numerically
meaningless on both sides, and the port lands on the other side of a point of
infinite sensitivity in the source's own algorithm. Faithfully reproduced.

### Deriving the bound

Not from the observed miss. Every distinct `(rx, rz, n, seg)` profile the
builder constructs — 32 of them — was evaluated in Node and in Rust at every
ring index, and the worst local-coordinate difference taken:

| profile | `n` | worst \|Δ\| |
|---|---|---|
| rail slot (`weapon.js:143`) | 8 | **1.2015e-11** |
| top rail (`weapon.js:139`) | 6 | 5.4669e-12 |
| every other `boxRound`/`loft` profile | ≤ 5 | ≤ 1.7440e-13 |
| every `cyl` (`ellipseProfile`) | 2 | ≤ 1.7347e-18 |

The ordering is monotone in `n`, exactly as `|c|^(2/n - 1)` predicts, and
`n = 2` — where the power is linear and there is no amplification at all — sits
at pure `f64` noise. The bake matrix is orthonormal, so it transports this
without growth. Ceiling `1.2015e-11` → **`2e-11`**, and the observed worst
across all four cases (`1.1916e-11`) sits under it.

Normals then carry a *geometric* gain: at the quadrant column the ring has
collapsed to `|x| ~ 1.5e-6` while its neighbour sits at `|x| ~ 9.8e-3`, so the
incident triangles are extremely thin and `|Δn̂| ≤ 2·Σ|C-B|·|δ| / |ΣN|` is
large. Measured across the model it is `9.55`, giving `1.148e-10` → **`2e-10`**.
That gain is a property of the mesh, identical on both sides — not a second
unexplained divergence.

### Why the bound is per-mesh

`steel` is the only mesh carrying an `n > 5` profile (the top rail and its 19
slots). `polymer`, `rubber` and `glass` top out at `n = 4.4` and hold `1e-12`
on their own. Relaxing all four would have thrown away the strict bar on three
meshes to accommodate a mechanism that cannot reach them.
`source_algorithm_superellipse_quadrant_points_amplify_libm_noise` pins both
halves of that claim — the other three meshes stay strict, and `steel`'s
excursions stay under 5% of its vertices — so the justification cannot rot into
"the number we needed".

### What was considered and rejected

Matching V8 bit-for-bit would mean porting fdlibm's `__kernel_cos` plus a
Payne–Hanek reduction into `ai/geo.rs` — a second libm in the app — to recover
**12 picometres** on a 12.5 mm rail slot. Not worth it, and it is not this
slice's file. Rewriting `superEllipse` to return exact `0`/`±1` at the quadrant
points would be *more* accurate than the source and therefore a different
program.

---


## The traps, checked by name

- **`Float32Array` storage width** — grepped `weapon.js`: none. The `ai/geo.js`
  path it drives is all `Float64Array` / plain JS arrays (`loft`'s `uArr` and
  ring `arr`, `computeNormals`' in-place normals). The only `Float32Array` in
  `geo.js` is inside `CharacterBuilder.build()`, which is a *later* stage and
  not on this path. So the port is `f64` throughout, correctly.
- **`sign` is not `signum`** — no `Math.sign` in `weapon.js`. It *is* in
  `geo.js`'s `superEllipse` (`rx * Math.sign(c) * Math.abs(c) ** e`) — flagged
  for the `ai/geo.rs` agent below, not fixable from here.
- **Euler order is a convention** — `box()` builds its quaternion with
  `new THREE.Euler(rx, ry, rz, 'YXZ')`. Ported as `quat_from_euler_yxz`, taken
  from Three r180's `Quaternion.js:337-342`, which differs from the `'XYZ'`
  branch in the **sign of the `z` and `w` terms**. `axiom_math::Quat::
  from_euler_xyz` would have been wrong twice over (wrong order *and* f32).
  In practice every call site leaves the angles at `?? 0`, so this always
  returns identity — see "source quirks".
- **Matrix storage order** — `Matrix4.makeBasis` puts the three axes in the
  **columns** (`Matrix4.js:253-264`); written as rows it would flip every
  off-diagonal sign. `mat4_make_basis`/`mat4_compose`/`apply_matrix4` are
  transcribed straight from Three r180's `Matrix4.js:253-264`, `1001-1035`,
  `688-706` and `Vector3.applyMatrix4`, keeping the `e[col*4 + row]` layout.
- **Float arithmetic is not associative** — transcribed literally at every
  site. The ones that most invited tidying, and were not tidied:
  - `weapon.js:95` `-0.028 - Math.sin(rake) * -t * 0.105 * 0.55 - t * 0.030` —
    the double negation is left as `- f64::sin(rake) * -t * …`, not folded
    into an addition.
  - `weapon.js:80` `-0.028 + Math.cos(a) * -0.024 + 0.024` — the `-0.024` and
    `+0.024` are *not* cancelled; they are two separate roundings.
  - `weapon.js:39` `z0 + ((z1 - z0) * i) / (n - 1)` — multiply then divide, in
    that order.
  - `weapon.js:143` `railZ0 + 0.004 + i * 0.0102` — left-to-right.
  - `Vector3.applyQuaternion`'s `vx + qw*tx + qy*tz - qz*ty` is kept
    left-to-right (see the note on `weapons::rig_math` below, which does *not*).
- **An enum used as a table index** — no lookup tables here; `WeaponStyle` is
  only ever tested for equality (`style == Ak`), never indexed.
- **`Math.hypot` is not `sqrt(x*x+y*y+z*z)`** — none in `weapon.js`. It *is*
  all over `geo.js` (`loft`'s arc lengths, `computeNormals`' normalisation) —
  flagged below.
- **A matching count is not proof** — so the test compares full `p`/`n`/`uv`
  arrays index-aligned, not just counts.
- **Your comparator can be the bug** — the comparator here is a flat
  index-aligned scan with no spatial keying and no sorting, precisely so it
  cannot mispair anything. It reports the worst `|delta|` and its index rather
  than the first failure, so a real divergence is distinguishable from a
  last-bit one at a glance.
- **Dead computation in the source is still part of the source** — three cases,
  all ported rather than dropped; see below.

---

## Source quirks found (ported faithfully, pinned where observable)

1. **`weapon.js:237` has an unreachable ternary.** It sits inside the
   `if (long)` arm and reads `long ? 0.33 : 0.29`, so the false arm can never
   be taken. Ported as written, with a comment. Not separately testable — the
   value is identical either way — which is exactly why it is worth a comment
   rather than a silent fold.
2. **`box()`'s Euler options are never used.** `opts.rx`/`ry`/`rz` default to
   `0` and no call site in the file sets them, so the composed quaternion is
   always exactly identity and every `box()` is a pure translation. The `'YXZ'`
   conversion is ported anyway.
3. **`revolve` is imported and never called** (`weapon.js:12`). Not imported in
   the Rust port — an unused Rust import is a warning, and there is nothing
   behavioural to preserve.
4. **The `ak` returns an empty `glass` mesh** (0 vertices, 0 triangles), because
   the AK arm builds iron sights and no optic. That emptiness is load-bearing,
   not incidental: `soldier.js:718` and `selftest.mjs:77` both guard on
   `W.glass.p.length` before registering a glass part, and `soldier.js:726-731`
   warns if the resulting material-slot order changes. A port that emitted a
   degenerate lens instead would silently reorder the opaque draws. Pinned by
   `source_quirk_ak_glass_mesh_is_empty`.
5. **The absent-`rng` cant is style-independent.** With `rng` omitted the cant
   is the literal `-0.06` and the style never enters the basis, so
   `carbine_no_rng` and `ak_no_rng` have *byte-identical* bake matrices. The
   golden confirms it; pinned by
   `source_quirk_absent_rng_cants_by_a_literal_and_is_style_independent`.
6. **The handguard is the only part whose material depends on the style** —
   `appendMesh(long ? poly : steel, hg)` (`weapon.js:200`). It is why the AK's
   polymer mesh is the *larger* of the two while its steel mesh is the smaller.
   Pinned by `source_quirk_handguard_switches_material_with_the_style`.
7. **`x` and `y` are normalised before the cant and not renormalised after**
   (`weapon.js:260-265`). Harmless (rotating a unit vector by a unit quaternion
   keeps it unit to within rounding) but it is the source's order and the port
   keeps it.
8. **Every per-part `computeNormals` is thrown away.** `weapon.js:269-272` runs
   `computeNormals` over each *merged* material mesh before the bake, wiping
   the per-part normals set at `weapon.js:30/48/83/103/125/187`. Those earlier
   normals still matter — they are the directions `displace` pushes along — so
   none of them can be dropped. Noted because it looks redundant and is not.

---

## Assumptions about slices written in parallel — and how they resolved

`ai/geo.rs` and `ai/rig.rs` did not exist when this was written (no file, no
git history on any branch), so the table below is what was **assumed**, chosen
as the most direct Rust translation of the JavaScript.

**All of it held, with four shape differences the orchestrator reconciled at
integration** — every one of them in geo's favour, and correctly so:

1. **The options bags nest rather than flatten.** `TubeOpts { up, frames,
   loft: LoftOpts { … } }`, `RibbonOpts { upright, seg, tube: TubeOpts { … } }`,
   `BoxRoundOpts { …, loft }`. This is the faithful shape: `geo.js`'s `tube`
   forwards its whole opts bag to `loft` and `ribbon` forwards to `tube`, so
   the nesting *is* the `{ ...opts }` spread. The four `up` vectors were
   re-derived from the source during that rewrite and are verified here against
   `weapon.js:46 [0,1,0]`, `:82 [1,0,0]`, `:101 [0,0,1]`, `:212 [0,1,0]` — all
   four correct.
2. **`Ring`'s `o`/`s`/`y` are `Option`**, modelling `ring.o ?? [0,0,0]`.
   Semantically identical to the bare values assumed here.
3. **`loft` takes `LoftOpts` by value.**
4. **`BORE_DIR`/`GRIP_R` are `LazyLock`**, so the by-value use in
   `bind_matrix` reads `*BORE_DIR`; indexed uses go through `Deref` unchanged.

The orchestrator also added `WeaponStyle::from_name` for `soldier.rs`
(`"ak"` → `Ak`, everything else → `Carbine`) — checked against `weapon.js:67`'s
`style === 'ak'` and correct, including the "unknown string is not an error"
behaviour.

### From `super::rig` (`src/ai/rig.js:54-69`)

| assumed | source |
|---|---|
| `pub const BORE_DIR: [f64; 3]` | `rig.js:55-58`, `= [0.11368738290391958, -0.0988585938294953, 0.9885859382949529]` |
| `pub const GRIP_R: [f64; 3]` | `rig.js:69`, `= [-0.1184412804449809, 1.2772967656043313, 0.17903234395668777]` |

Both values are in `tests/ai_weapon/golden.json` under `rig`, captured from the
original, so the rig port can be checked against them directly.

### From `super::geo` (`src/ai/geo.js`)

| assumed Rust | source |
|---|---|
| `struct Mesh { p: Vec<f64>, n: Vec<f64>, uv: Vec<f64>, i: Vec<u32> }` | `emptyMesh` (`geo.js:106-108`) |
| `fn empty_mesh() -> Mesh` | `emptyMesh` |
| `fn append_mesh(dst: &mut Mesh, src: &Mesh)` | `appendMesh` (`geo.js:452-459`) |
| `fn compute_normals(m: &mut Mesh)` | `computeNormals` (`geo.js:358-379`), `from = 0` |
| `fn displace(m: &mut Mesh, f: impl FnMut(f64,f64,f64,f64,f64,f64,usize) -> f64)` | `displace` (`geo.js:410-423`) — `(x, y, z, nx, ny, nz, i)` |
| `fn warp(m: &mut Mesh, f: impl FnMut(&mut V3, usize))` | `warp` (`geo.js:426-436`) |
| `fn transform_mesh(m: &mut Mesh, mat: &M4)` | `transformMesh` (`geo.js:438-450`) |
| `fn super_ellipse(rx: f64, rz: f64, n: f64, seg: usize, rot: f64) -> Vec<[f64;2]>` | `superEllipse` (`geo.js:117-129`) |
| `fn ellipse_profile(rx: f64, rz: f64, seg: usize, rot: f64) -> Vec<[f64;2]>` | `ellipseProfile` (`geo.js:131-133`) |
| `struct Ring { pts: Vec<[f64;2]>, o: [f64;3], q: Option<Q>, s: [f64;2], y: f64 }` | `loft`'s ring record (`geo.js:139`, `162-177`) |
| `struct LoftOpts { closed: bool, cap_start: bool, cap_end: bool }` | `loft` opts (`geo.js:145-147`); `closed` defaults **true** (`opts.closed !== false`) |
| `fn loft(rings: &[Ring], opts: &LoftOpts) -> Mesh` | `loft` (`geo.js:145-244`) |
| `struct TubeOpts { up: [f64;3], cap_start: bool, cap_end: bool, closed: bool }` | `tube` (`geo.js:274-282`); `up` defaults `[0,0,1]` |
| `fn tube(points: &[[f64;3]], profile: impl Fn(f64, usize) -> Vec<[f64;2]>, opts: &TubeOpts) -> Mesh` | `tube` |
| `struct RibbonOpts { seg: usize, up: [f64;3], upright: bool }` | `ribbon` (`geo.js:346-352`); it forces `capStart`/`capEnd` true itself, so they are not in the struct |
| `fn ribbon(points: &[[f64;3]], width: f64, thick: f64, opts: &RibbonOpts) -> Mesh` | `ribbon` |
| `struct BoxRoundOpts { n: f64, seg: usize, rows: usize, round_y: f64, ny: f64 }` | `boxRound` (`geo.js:299-318`) |
| `fn box_round(hx: f64, hy: f64, hz: f64, opts: BoxRoundOpts) -> Mesh` | `boxRound` |
| `struct Noise` with `fn new(rng: &mut Rng) -> Noise` and `fn fbm3(&self, x: f64, y: f64, z: f64, oct: u32) -> f64` | `Noise` (`geo.js:31-99`) |
| `struct V3 { x: f64, y: f64, z: f64 }` | `THREE.Vector3` (f64) |
| `struct Q { x: f64, y: f64, z: f64, w: f64 }` | `THREE.Quaternion` (f64) |
| `struct M4 { e: [f64; 16] }` | `THREE.Matrix4.elements`, column-major |

`V3`/`Q`/`M4` must be **f64**. `axiom_math::{Vec3, Quat, Mat4}` are f32 and
would lose ~7 decimal digits against a golden captured from JS `Number`s;
`weapons::rig_math::{V3, Q}` are f64 and the right *shape*, but they live under
`src/weapons/` (a different subsystem, and off-limits to this slice) — and see
the divergence note below before reusing them.

Only the *types* are borrowed from `geo`; `weapon.rs` implements the four
Three.js matrix/quaternion operations it needs (`compose`, `makeBasis`,
`setPosition`, `applyMatrix4`, `setFromAxisAngle`, `setFromEuler('YXZ')`,
`applyQuaternion`) locally, so the exact float grouping is under this file's
control rather than inherited.

---

## For the `ai/geo.rs` agent — four things on this path

Not fixable from here; all four are in `geo.js` and all four are on the code
path `buildWeapon` drives. Items 1-3 were written before `geo.rs` landed and
all three are handled correctly in it (`jsmath::sign`, a real `hypot`, and the
clamp kept). Item 4 is the finding from integration.

1. **`superEllipse` (`geo.js:124-125`) uses `Math.sign`.** `Math.sign(0)` is
   `0`; `f64::signum(0.0)` is `1.0` and `(-0.0f64).signum()` is `-1.0`. With
   `seg` a multiple of 4 the cosine/sine at the quadrant points are ~`6.1e-17`
   rather than exactly `0`, so the sign is usually well-defined — but `rot`
   is a caller parameter and the zero case is one argument away. Hand-roll a
   three-valued sign.
2. **`Math.hypot` (`geo.js:181, 193, 375, 400, 751`).** It scales by the
   largest magnitude first and rounds differently from
   `sqrt(x*x + y*y + z*z)`. It sets `loft`'s arc-length UVs and
   `computeNormals`' normalisation — i.e. every `uv` and every `n` this
   slice's golden pins.
3. **`boxRound`'s envelope (`geo.js:311-313`) clamps *before* the fractional
   power** — `Math.max(0, 1 - k ** ny) ** (1 / ny)` — with the source's own
   comment saying `1 - k^ny` can land at `-1e-16`. Keep the clamp.
4. **`superEllipse`'s fractional power amplifies libm noise at the quadrant
   points, and every consumer inherits it.** See "The one divergence" above for
   the full derivation. The bound scales as `r · (2/n) · |c|^(2/n - 1)`, so it
   grows sharply with `n`. Concretely, for anyone writing a golden test against
   a `geo.js` consumer:

   | `n` | worst \|Δ\| at `r ~ 0.02` |
   |---|---|
   | 8 | `1.2e-11` |
   | 6 | `5.5e-12` |
   | 5.5 | `~4e-13` |
   | ≤ 5 | `≤ 1.7e-13` |
   | 2 | `≤ 1.7e-18` |

   **`ai/parts.rs` is the one to watch**: `parts.js` tops out at `n: 5.5` (two
   calls) which lands at roughly `4e-13` — inside `1e-12`, but only by a factor
   of 2.5, and the bound is linear in the radius. If either of those parts has
   a radius much above 5 cm its golden will need the same treatment. Everything
   else in `parts.js`/`soldier.js` is `n ≤ 4.5` and comfortably strict.

## For whoever owns `src/weapons/rig_math.rs`

`V3::apply_quat` (`src/weapons/rig_math.rs:110-120`) groups Three's
`Vector3.applyQuaternion` as `vx + qw*tx + (qy*tz - qz*ty)`. Three r180
(`Vector3.js:479-481`) evaluates it left-to-right, i.e.
`((vx + qw*tx) + qy*tz) - qz*ty`. Those differ in the last bits — exactly the
"float arithmetic is not associative" trap. Found while checking whether that
type could be reused here (it was not, for this reason plus the subsystem
boundary). Not touched: `src/weapons/` is another agent's tree this session.

---

## Wiring

```text
apps/shmup/src/ai/mod.rs:  pub mod weapon;
```

Done at integration; nothing else was needed. No `Cargo.toml` change
(`serde_json` with `arbitrary_precision` was already a `dev-dependency`) and no
`lib.rs` change (`pub mod ai;` already existed).

**Status: 10/10 green** —
`cargo test -p axiom-shmup --test ai_weapon_port`.

## Not ported

Nothing. All 291 lines are represented, including the dead ternary, the never-
used Euler arguments, and the `!long` / `long` arms of every branch.
