# Weapon geometry: the Rust API contract

Ported from Claude-of-Duty `src/weapons/geometry.js` (447 lines). Every builder in
`parts.js` and `models/*.js` calls only this surface, so it is fixed here **before**
the parts are split, and agents write against it in parallel.

Target module: `apps/claude-of-duty/src/weapons/geometry/`.

## Space convention (from geometry.js:15-18)

Metres at real scale. `+X` right, `+Y` up, `-Z` toward the muzzle. Origin at the
shooting hand's thumb web.

**The rule the whole kit exists to enforce (geometry.js:8-13): there is no such
thing as a 90-degree edge on a real firearm.** Every box is chamfered 0.3–1.5 mm,
every extrusion bevelled, every tube end crowned. Do not "simplify" a chamfer away.

## The geometry buffer

```rust
pub struct Geo {
    pub pos: Vec<f32>,      // xyz triples
    pub normal: Vec<f32>,   // xyz triples
    pub uv: Vec<f32>,       // uv pairs
    pub index: Vec<u32>,
}

impl Geo {
    pub fn vert_count(&self) -> usize;
    pub fn tri_count(&self) -> usize;
    pub fn apply(&mut self, m: &Mat4);        // transform pos + normal
    pub fn flip_winding(&mut self);           // negative-scale mirror
    pub fn normalize_attributes(&mut self);   // ensure normal+uv present
}
```

`normalize_attributes` matters: `mergeAll` requires every input to carry the same
attribute set or the merge silently drops channels.

## Primitives — signatures mirror geometry.js exactly

```rust
pub fn box_geo(w: f32, h: f32, d: f32, chamfer: f32, seg: u32) -> Geo;   // default chamfer 0.0012, seg 1
pub fn blob(w: f32, h: f32, d: f32, radius: f32, seg: u32) -> Geo;       // default radius 0.006, seg 3
pub fn lathe_z(profile: &[[f32; 2]], seg: u32, phi_start: f32, phi_length: f32) -> Geo;
pub fn tube_z(r_outer: f32, r_inner: f32, len: f32, seg: u32, crown: f32) -> Geo;
pub fn rod_z(r0: f32, r1: f32, len: f32, seg: u32, chamfer: f32) -> Geo;
pub fn dome(r: f32, seg: u32, cut: f32) -> Geo;
pub fn extrude(pts: &[[f32; 2]], depth: f32, opts: ExtrudeOpts) -> Geo;
pub fn round_rect(w: f32, h: f32, r: f32, seg: u32) -> Geo;
pub fn ring(radius: f32, thickness: f32, seg: u32, rings: u32, arc: f32) -> Geo;
pub fn screw(r_head: f32, r_shank: f32, head_h: f32, shank_l: f32, seg: u32) -> Geo;
pub fn knurl_band(radius: f32, len: f32, count: u32, depth: f32, rows: u32) -> Geo;
pub fn serrations(w: f32, h: f32, len: f32, count: u32, depth: f32, axis: Axis) -> Geo;
pub fn picatinny(len: f32, opts: PicatinnyOpts) -> Geo;
pub fn mlok_slot(len: f32, wide: f32, depth: f32) -> Geo;
pub fn merge_all(list: Vec<Geo>) -> Option<Geo>;
```

Rust has no default arguments; give each a `*_with` variant or an `Opts` struct
carrying the JS defaults, and keep the JS default values in the doc comment.

## The Assembly

```rust
pub struct Xform { pub x: f32, pub y: f32, pub z: f32,
                   pub rx: f32, pub ry: f32, pub rz: f32,
                   pub sx: f32, pub sy: f32, pub sz: f32 }  // Default = identity

pub struct Assembly { /* buckets: BTreeMap<String, Vec<Geo>>, nodes: BTreeMap<String, Node> */ }

impl Assembly {
    pub fn new(name: &str) -> Self;
    pub fn add(&mut self, geo: Geo, mat: &str, t: Option<Xform>) -> &mut Self;
    pub fn add_mirrored(&mut self, geo: Geo, mat: &str, t: Xform) -> &mut Self;
    pub fn node(&mut self, name: &str, x: f32, y: f32, z: f32, rx: f32, ry: f32, rz: f32) -> &mut Self;
    pub fn build(&mut self) -> BTreeMap<String, Geo>;   // consumes buckets
}
```

**Use `BTreeMap`, not `HashMap`.** JS `Map` iterates in insertion order; a Rust
`HashMap` is randomised, which would make the merged output — and therefore its
hash — differ between runs. Sorted order is deterministic and comparable.

Euler order is **XYZ** (geometry.js `_e.set(..., 'XYZ')`). `add` composes
translate × rotate × scale, applies it, and **flips winding when the scale
determinant is negative** (`sx*sy*sz < 0`) — that is what `add_mirrored` relies on.

## Three.js algorithms that must be ported

`geometry.js` leans on Three (MIT, port with attribution):

- `RoundedBoxGeometry` — `three/examples/jsm/geometries/RoundedBoxGeometry.js`
- `LatheGeometry`, `ExtrudeGeometry` (bevel + holes), `TorusGeometry`,
  `CylinderGeometry`, `SphereGeometry` — `three/src/geometries/`
- `mergeGeometries`, `mergeVertices` — `three/examples/jsm/utils/BufferGeometryUtils.js`.
  `mergeVertices` welds at tolerance **1e-6**; `mergeAll` converts every input to
  non-indexed first, merges, then welds.

Port the algorithm, not a lookalike — vertex order and seam handling decide whether
the hashes match.

## Verification — mesh goldens

`buildRifle()` runs headless in Node and is **deterministic**: measured 716 ms,
11 material buckets, 60,125 verts, 53,692 tris, FNV-1a hash `343e3ffa` identical
across runs.

So every port here is checkable against real geometry:

1. Write a Node script importing the JS primitive/part, dump `position`, `normal`,
   `uv` and `index` as JSON.
2. Assert the Rust output matches — **vertex count and triangle count exactly**,
   and each float within `1e-6`.
3. For a whole part builder, compare per-material-bucket counts and the hash.

Floats may differ in the last bits (different libm, different fma), so compare
positions with a tolerance; compare **counts and indices exactly** — a different
count means a different algorithm, not a rounding difference.

---

## Corrections to this contract (learned in `e91a5eda`)

**1. Compute in `f64`, store `f32`.** JS numbers are `f64`, and Three computes in
`f64` while storing into `Float32Array`. The original version of this contract said
`f32` everywhere, which collapsed that distinction: `get_bevel_vec` is provably
bit-exact against the source when fed full-precision corners, but an `f32` point-list
boundary loses enough precision — amplified through a division — to occasionally flip
the `1e-6` weld quantization and change the hash.

So: **point lists, profiles and intermediate math are `f64`. Only the final `Geo`
buffers are `f32`.** Do not widen a golden's tolerance to paper over a precision
boundary — narrow the boundary.

**2. `round_rect` returns a point list, not geometry.**
```rust
pub fn round_rect(w: f64, h: f64, r: f64, seg: u32) -> Vec<[f64; 2]>;
```
Its only caller feeds it to `extrude`. Same for any other helper whose JS result is
consumed as a contour rather than drawn.

**3. Euler composition is NOT `axiom_math::Quat::from_euler_xyz`.** That helper
composes `qz*qy*qx`; Three's `'XYZ'` order is `qx*qy*qz` — a different rotation, not
the same one rewritten. `Assembly::add` builds its own composition. Never substitute
the math-layer helper here (verified against a Three golden, pinned by a test in
`5c504d5b`).
