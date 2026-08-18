//! Ported from Claude-of-Duty `src/world/builder.js` — the `Assembler`, the
//! central abstraction every other module in `src/world/` writes into
//! instead of touching a scene graph directly. Five verbs (`builder.js:24-31`):
//!
//! - `add(key, geo, matrix, opts)` — merge into a per-palette-key static batch
//! - `proto(id, spec)` / `place`/`put`/`putS` — declare an instanced
//!   prototype and place instances
//! - `box`/`collide_geo`/`slab_box` — author a *separate* cheap collision
//!   proxy
//! - `light()` — register a punctual light
//! - `finalize()` — one merged geometry per palette key, one instanced batch
//!   per prototype per 64 m chunk, with per-instance `[wear, grime, ao]`
//!
//! Three decisions the source documents in-line and this port preserves:
//!
//! 1. **Collision is authored, not derived** (`builder.js:16-22`) — proxies
//!    are boxes built from the same numbers as the visuals, so a doorway is a
//!    real hole and the collision hull stays small.
//! 2. **The level→world transform is baked into every vertex** (`setTransform`,
//!    `builder.js:90-98`), not applied to a parent node — so physics, culling
//!    and world-space triplanar materials all stay honest.
//! 3. **`put()` auto-jitters loose props and drops a contact fillet**
//!    (`builder.js:239-262`): when jitter is armed, any prototype with
//!    `tilt > 0` gets +/- yaw/tilt/scale and a sink, and any with `skirt > 0`
//!    gets a low mound of ground material so props meet the ground
//!    geometrically instead of on a razor edge.
//!
//! ## What this port deliberately does not carry
//!
//! - **Materials.** `mat(key)` (`builder.js:113-124`) resolves a palette key
//!   to a live `THREE.Material` via a `materials` facade. Nothing on the Rust
//!   side owns runtime material objects yet (`crate::materials` is bake
//!   *parameters* only — see its module doc), so this Assembler carries
//!   [`surface_of`] (the physics-relevant half of `mat`) and drops the
//!   material-resolution half entirely; a future rendering bridge re-adds it
//!   at the point it has somewhere to put a live material.
//! - **`render`.** The constructor's `render` argument
//!   (`this.render?.addLight?.(light, opts)`) is a live renderer hook with no
//!   Rust counterpart yet; [`Assembler::light`] stores what the Assembler
//!   itself is responsible for (the world-space position) and nothing more.
//! - **`updateLod`/bounding spheres** (`builder.js:428-438`). Distance LOD
//!   needs a live camera position sampled every frame — a different kind of
//!   system than a build-time assembler — so [`Assembler::finalize`] carries
//!   each instanced prototype's `max_dist` as plain data and stops there; a
//!   future per-frame LOD system reads it.
//! - **`dispose()`/`releaseCache()`.** Both are JS-side manual memory
//!   management (`builder.js:168-171, 440-454`) with no Rust counterpart —
//!   ownership drops the data when the `Assembler` does. [`Assembler::release_cache`]
//!   is kept anyway, as a plain "clear the cache" op, for call-site parity.

use axiom_math::{Mat4, Vec3};

use crate::rng::Rng;
use crate::weapons::geometry::primitives::box_geo;
use crate::world::accum::{Accum, AccumAddOpts};
use crate::world::geo::WorldGeo;
use crate::world::kit::trs;
use crate::world::palette::{Palette, Surface};

/// Spatial bucket size for chunked instance clouds (`builder.js:39`).
pub const CHUNK: f32 = 64.0;

/// `proto(id, spec)`'s `spec` (`builder.js:178-211`). Defaults match the
/// source: `tilt=0`, `sink=0`, `skirt=0`, `cast_shadow=true`,
/// `receive_shadow=true`, `chunk=true`, `max_dist=0`, `no_prepass=false`.
pub struct ProtoSpec {
    pub geo: WorldGeo,
    pub key: String,
    pub tilt: f32,
    pub sink: f32,
    pub skirt: f32,
    pub cast_shadow: bool,
    pub receive_shadow: bool,
    pub chunk: bool,
    pub max_dist: f32,
    pub no_prepass: bool,
}

struct Proto {
    id: String,
    geo: WorldGeo,
    key: String,
    tilt: f32,
    sink: f32,
    skirt: f32,
    cast_shadow: bool,
    receive_shadow: bool,
    chunk: bool,
    max_dist: f32,
    no_prepass: bool,
    matrices: Vec<Mat4>,
    masks: Vec<Option<[f32; 3]>>,
}

/// The `jitter = { rng, yaw, scale }` set-dressing state (`builder.js:69-72`).
pub struct Jitter {
    pub rng: Rng,
    pub yaw: f32,
    pub scale: f32,
}

/// A registered punctual light — the geometric half of `builder.js`'s
/// `{ light, opts }` pair. See the module doc's "What this port deliberately
/// does not carry" for why only the world-space position survives.
#[derive(Debug, Clone, Copy)]
pub struct LightRegistration {
    pub position: Vec3,
}

/// `this.stats` (`builder.js:79`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub static_tris: usize,
    pub inst_tris: usize,
    pub instances: usize,
    pub draw_calls: usize,
    pub collide_tris: usize,
}

/// One merged static-batch mesh (`builder.js:320-335`).
pub struct StaticMesh {
    pub key: String,
    pub surface: Surface,
    pub geo: WorldGeo,
}

/// One instanced-prototype draw, bucketed by 64 m chunk (`builder.js:337-398`).
pub struct InstancedDraw {
    pub proto_id: String,
    pub key: String,
    pub surface: Surface,
    pub cast_shadow: bool,
    pub receive_shadow: bool,
    pub no_prepass: bool,
    pub max_dist: f32,
    pub matrices: Vec<Mat4>,
    /// Per-instance `[wear, grime, ao]`. Empty when no instance in this
    /// bucket carried an explicit mask (`needColor` stays `false`,
    /// `builder.js:369-380`) — otherwise every entry is present, defaulting
    /// an unmasked instance to `[1, 1, 1]` (`mk ?? [1, 1, 1]`, `builder.js:374`).
    pub masks: Vec<[f32; 3]>,
}

/// One merged collision-proxy mesh (`builder.js:400-417`, minus the
/// `physics.addStatic`/`rebuildStatic` bridge — see `crate::physics`'s module
/// doc: baking a live mesh into `bvh::StaticWorld` is an explicitly
/// un-landed future arm, not something this port can wire up yet).
pub struct CollisionMesh {
    pub surface: Surface,
    pub geo: WorldGeo,
}

/// `finalize()`'s full output (`builder.js:318-426`).
pub struct FinalizeResult {
    pub statics: Vec<StaticMesh>,
    pub instanced: Vec<InstancedDraw>,
    pub collision: Vec<CollisionMesh>,
    pub lights: Vec<LightRegistration>,
    pub stats: Stats,
}

/// `class Assembler` (`builder.js:41-455`).
pub struct Assembler {
    pub rng: Rng,
    static_batches: Vec<(String, Accum)>,
    protos: Vec<Proto>,
    collide: Vec<(Surface, Accum)>,
    geo_cache: Vec<(String, WorldGeo)>,
    lights: Vec<LightRegistration>,
    /// Filled by a future interiors pass: where a bare bulb wants a point
    /// light. Position-only, for the same reason as [`LightRegistration`].
    pub interior_lights: Vec<Vec3>,
    /// Filled by a future dressing pass: where a street lamp wants a point
    /// light.
    pub lamp_anchors: Vec<Vec3>,
    pub jitter: Option<Jitter>,
    pub skirts: bool,
    xform: Mat4,
    identity: bool,
    ry: f32,
    stats: Stats,
}

impl Assembler {
    /// `constructor({ materials, rng, render })` (`builder.js:42-80`), minus
    /// `materials`/`render` — see the module doc.
    pub fn new(rng: Rng) -> Self {
        Assembler {
            rng,
            static_batches: Vec::new(),
            protos: Vec::new(),
            collide: Vec::new(),
            geo_cache: Vec::new(),
            lights: Vec::new(),
            interior_lights: Vec::new(),
            lamp_anchors: Vec::new(),
            jitter: None,
            skirts: true,
            xform: Mat4::IDENTITY,
            identity: true,
            ry: 0.0,
            stats: Stats::default(),
        }
    }

    // -------------------------------------------------------- transform --
    /// `setTransform(ry, tx = 0, tz = 0)` (`builder.js:90-98`).
    pub fn set_transform(&mut self, ry: f32, tx: f32, tz: f32) -> &mut Self {
        self.xform = trs(tx, 0.0, tz, ry, 1.0, 1.0, 1.0, 0.0, 0.0);
        self.identity = ry == 0.0 && tx == 0.0 && tz == 0.0;
        self.ry = ry;
        self
    }

    /// `toWorld(x, y, z, out)` (`builder.js:100-103`).
    pub fn to_world(&self, x: f32, y: f32, z: f32) -> Vec3 {
        self.xform.transform_point(Vec3::new(x, y, z))
    }

    /// `_x(matrix)` (`builder.js:106-110`): compose the level transform onto
    /// a level-space matrix.
    fn compose(&self, matrix: Option<&Mat4>) -> Option<Mat4> {
        if self.identity {
            return matrix.copied();
        }
        match matrix {
            None => Some(self.xform),
            Some(m) => Some(self.xform.multiply(*m)),
        }
    }

    // ------------------------------------------------------------- materials --
    /// `surfaceOf(key)` (`builder.js:126-128`): the physics-relevant half of
    /// `mat(key)` — see the module doc.
    pub fn surface_of(&self, key: &str) -> Surface {
        Palette::ALL.iter().find(|(name, _)| *name == key).map_or(Surface::Concrete, |(_, entry)| entry.surface)
    }

    // --------------------------------------------------------- static batch --
    /// `add(key, geo, matrix = null, opts = null)` (`builder.js:132-140`).
    pub fn add(&mut self, key: &str, geo: &WorldGeo, matrix: Option<&Mat4>, opts: Option<AccumAddOpts>) -> &mut Self {
        let world_matrix = self.compose(matrix);
        self.static_entry(key).add(geo, world_matrix.as_ref(), opts);
        self
    }

    /// `addOnce(key, geo, matrix = null, opts = null)` (`builder.js:161-166`).
    /// The source's `geo.dispose()` has no Rust counterpart (see module
    /// doc); this is otherwise identical to [`Assembler::add`].
    pub fn add_once(&mut self, key: &str, geo: &WorldGeo, matrix: Option<&Mat4>, opts: Option<AccumAddOpts>) -> &mut Self {
        self.add(key, geo, matrix, opts)
    }

    fn static_entry(&mut self, key: &str) -> &mut Accum {
        if let Some(pos) = self.static_batches.iter().position(|(k, _)| k == key) {
            return &mut self.static_batches[pos].1;
        }
        self.static_batches.push((key.to_string(), Accum::new(&format!("world:{key}"))));
        let last = self.static_batches.len() - 1;
        &mut self.static_batches[last].1
    }

    /// `cache(key, factory)` (`builder.js:152-159`).
    ///
    /// The source caches and returns the **same** `THREE.BufferGeometry`
    /// reference every call, relying on `Accum.add` to copy out of it. A
    /// Rust `Vec`/map cannot hand back an aliased `&mut` alongside further
    /// mutation of `self`, so this clones the cached [`WorldGeo`] on every
    /// hit instead — correct (the source's own copy-out-on-add makes the two
    /// behaviourally identical) and cheap enough for the kit-piece sizes this
    /// is ever called with (window frames, sills, steps — at most a few
    /// hundred vertices).
    pub fn cache<F: FnOnce() -> WorldGeo>(&mut self, key: &str, factory: F) -> WorldGeo {
        if let Some((_, g)) = self.geo_cache.iter().find(|(k, _)| k == key) {
            return g.clone();
        }
        let g = factory();
        self.geo_cache.push((key.to_string(), g.clone()));
        g
    }

    /// `releaseCache()` (`builder.js:168-171`) — see module doc.
    pub fn release_cache(&mut self) {
        self.geo_cache.clear();
    }

    // ------------------------------------------------------------ instanced --
    /// `proto(id, spec)` (`builder.js:178-211`). Idempotent: a repeated `id`
    /// is a no-op, matching `if (this._protos.has(id)) return id;`.
    pub fn proto(&mut self, id: &str, spec: ProtoSpec) -> String {
        if self.protos.iter().any(|p| p.id == id) {
            return id.to_string();
        }
        self.protos.push(Proto {
            id: id.to_string(),
            geo: spec.geo,
            key: spec.key,
            tilt: spec.tilt,
            sink: spec.sink,
            skirt: spec.skirt,
            cast_shadow: spec.cast_shadow,
            receive_shadow: spec.receive_shadow,
            chunk: spec.chunk,
            max_dist: spec.max_dist,
            no_prepass: spec.no_prepass,
            matrices: Vec::new(),
            masks: Vec::new(),
        });
        id.to_string()
    }

    /// `has(id)` (`builder.js:213-215`).
    pub fn has(&self, id: &str) -> bool {
        self.protos.iter().any(|p| p.id == id)
    }

    /// `place(id, matrix, masks = null)` (`builder.js:217-227`).
    pub fn place(&mut self, id: &str, matrix: &Mat4, masks: Option<[f32; 3]>) -> &mut Self {
        let world_matrix = self.compose(Some(matrix)).expect("compose(Some(_)) always returns Some");
        match self.protos.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                p.matrices.push(world_matrix);
                p.masks.push(masks);
            }
            None => eprintln!("[world] no prop prototype \"{id}\""),
        }
        self
    }

    /// `put(id, x, y, z, ry=0, s=1, masks=null, rx=0, rz=0)` (`builder.js:239-262`).
    #[allow(clippy::too_many_arguments)]
    pub fn put(&mut self, id: &str, x: f32, y: f32, z: f32, ry: f32, s: f32, masks: Option<[f32; 3]>, rx: f32, rz: f32) -> &mut Self {
        let proto_shape = self.protos.iter().find(|p| p.id == id).map(|p| (p.tilt, p.sink, p.skirt));
        let mut ry = ry;
        let mut rx = rx;
        let mut rz = rz;
        let mut s = s;
        let mut y = y;
        if let (Some(jitter), Some((tilt, sink, _))) = (self.jitter.as_mut(), proto_shape) {
            if tilt > 0.0 {
                ry += jitter.rng.range(f64::from(-jitter.yaw), f64::from(jitter.yaw)) as f32;
                rx += jitter.rng.range(f64::from(-tilt), f64::from(tilt)) as f32;
                rz += jitter.rng.range(f64::from(-tilt), f64::from(tilt)) as f32;
                s *= 1.0 + jitter.rng.range(f64::from(-jitter.scale), f64::from(jitter.scale)) as f32;
                y -= sink;
            }
        }
        let m = trs(x, y, z, ry, s, s, s, rx, rz);
        self.place(id, &m, masks);
        if self.skirts {
            if let Some((_, _, skirt)) = proto_shape {
                if skirt > 0.0 && self.has("dust_skirt") {
                    let rr = skirt * s;
                    let angle = (x * 2.7 + z * 1.9) % 6.283;
                    let m2 = trs(x, y + 0.004, z, angle, rr, 1.0, rr, 0.0, 0.0);
                    self.place("dust_skirt", &m2, None);
                }
            }
        }
        self
    }

    /// `putS(id, x, y, z, ry, sx, sy, sz, masks = null, rx = 0, rz = 0)`
    /// (`builder.js:264-267`).
    #[allow(clippy::too_many_arguments)]
    pub fn put_s(&mut self, id: &str, x: f32, y: f32, z: f32, ry: f32, sx: f32, sy: f32, sz: f32, masks: Option<[f32; 3]>, rx: f32, rz: f32) -> &mut Self {
        let m = trs(x, y, z, ry, sx, sy, sz, rx, rz);
        self.place(id, &m, masks)
    }

    /// `count(id)` (`builder.js:269-271`).
    pub fn count(&self, id: &str) -> usize {
        self.protos.iter().find(|p| p.id == id).map_or(0, |p| p.matrices.len())
    }

    // ------------------------------------------------------------ collision --
    /// `box(surface, cx, cy, cz, sx, sy, sz, ry = 0)` (`builder.js:275-283`).
    #[allow(clippy::too_many_arguments)]
    pub fn collide_box(&mut self, surface: Surface, cx: f32, cy: f32, cz: f32, sx: f32, sy: f32, sz: f32, ry: f32) -> &mut Self {
        let local = trs(cx, cy, cz, ry, sx, sy, sz, 0.0, 0.0);
        let world = self.compose(Some(&local));
        self.collide_entry(surface).add(&unit_box(), world.as_ref(), None);
        self
    }

    /// `collideGeo(surface, geo, matrix = null)` (`builder.js:286-293`).
    pub fn collide_geo(&mut self, surface: Surface, geo: &WorldGeo, matrix: Option<&Mat4>) -> &mut Self {
        let world = self.compose(matrix);
        self.collide_entry(surface).add(geo, world.as_ref(), None);
        self
    }

    /// `slabBox(surface, panelMatrix, x, y, w, h, t)` (`builder.js:296-307`).
    #[allow(clippy::too_many_arguments)]
    pub fn slab_box(&mut self, surface: Surface, panel_matrix: &Mat4, x: f32, y: f32, w: f32, h: f32, t: f32) -> &mut Self {
        let local = trs(x, y, t * 0.5, 0.0, w, h, t, 0.0, 0.0);
        let premultiplied = panel_matrix.multiply(local);
        let world = self.compose(Some(&premultiplied));
        self.collide_entry(surface).add(&unit_box(), world.as_ref(), None);
        self
    }

    fn collide_entry(&mut self, surface: Surface) -> &mut Accum {
        if let Some(pos) = self.collide.iter().position(|(s, _)| *s == surface) {
            return &mut self.collide[pos].1;
        }
        self.collide.push((surface, Accum::new(&format!("collide:{}", surface.name()))));
        let last = self.collide.len() - 1;
        &mut self.collide[last].1
    }

    // ------------------------------------------------------------------- light --
    /// `light(light, opts)` (`builder.js:310-314`) — see module doc for why
    /// only the position survives. `position` is in LEVEL space, exactly as
    /// the source's `light.position` is expected to be before this call.
    pub fn light(&mut self, position: Vec3, _opts: ()) -> &mut Self {
        let world_position = if self.identity { position } else { self.xform.transform_point(position) };
        self.lights.push(LightRegistration { position: world_position });
        self
    }

    // ------------------------------------------------------------------ finalize --
    /// `finalize(root, physics)` (`builder.js:318-426`), minus the
    /// `root`/`physics` side effects — see the module doc.
    pub fn finalize(&mut self) -> FinalizeResult {
        let mut statics = Vec::new();
        for (key, acc) in std::mem::take(&mut self.static_batches) {
            if acc.empty() {
                continue;
            }
            let surface = self.surface_of(&key);
            let geo = acc.build();
            self.stats.static_tris += geo.tri_count();
            self.stats.draw_calls += 1;
            statics.push(StaticMesh { key, surface, geo });
        }

        let mut instanced = Vec::new();
        for proto in std::mem::take(&mut self.protos) {
            let n = proto.matrices.len();
            if n == 0 {
                continue;
            }
            let mut buckets: Vec<(i64, Vec<usize>)> = Vec::new();
            if proto.chunk && n > 24 {
                for i in 0..n {
                    let c = proto.matrices[i].as_cols_array();
                    let gx = (c[12] / CHUNK).floor() as i64;
                    let gz = (c[14] / CHUNK).floor() as i64;
                    let k = gx * 97 + gz;
                    match buckets.iter_mut().find(|(bk, _)| *bk == k) {
                        Some((_, list)) => list.push(i),
                        None => buckets.push((k, vec![i])),
                    }
                }
            } else {
                buckets.push((0, (0..n).collect()));
            }

            let surface = self.surface_of(&proto.key);
            let tri = proto.geo.tri_count();
            for (_bucket_key, list) in buckets {
                let need_color = list.iter().any(|&i| proto.masks[i].is_some());
                let matrices: Vec<Mat4> = list.iter().map(|&i| proto.matrices[i]).collect();
                let masks: Vec<[f32; 3]> = if need_color {
                    list.iter().map(|&i| proto.masks[i].unwrap_or([1.0, 1.0, 1.0])).collect()
                } else {
                    Vec::new()
                };
                self.stats.draw_calls += 1;
                self.stats.instances += list.len();
                self.stats.inst_tris += tri * list.len();
                instanced.push(InstancedDraw {
                    proto_id: proto.id.clone(),
                    key: proto.key.clone(),
                    surface,
                    cast_shadow: proto.cast_shadow,
                    receive_shadow: proto.receive_shadow,
                    no_prepass: proto.no_prepass,
                    max_dist: proto.max_dist,
                    matrices,
                    masks,
                });
            }
        }

        let mut collision = Vec::new();
        for (surface, acc) in std::mem::take(&mut self.collide) {
            if acc.empty() {
                continue;
            }
            let geo = acc.build();
            self.stats.collide_tris += geo.tri_count();
            collision.push(CollisionMesh { surface, geo });
        }

        FinalizeResult {
            statics,
            instanced,
            collision,
            lights: self.lights.clone(),
            stats: self.stats,
        }
    }
}

/// `UNIT_BOX` (`builder.js:24`): a bare, un-mask-attributed unit box —
/// reuses `box_geo`'s unchamfered branch exactly as `kit::plain_box` does,
/// but *without* `plain_box`'s zeroed `color` column, since the source's
/// `UNIT_BOX` never carries one either (collision proxies are never
/// rendered, so `Accum.add`'s "missing color defaults to 0" fallback is all
/// they ever need).
fn unit_box() -> WorldGeo {
    let g = box_geo(1.0, 1.0, 1.0, 0.0, 1);
    WorldGeo {
        pos: g.pos,
        normal: g.normal,
        uv: g.uv,
        color: Vec::new(),
        index: g.index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::kit::chamfer_box;

    fn assembler() -> Assembler {
        Assembler::new(Rng::new(1))
    }

    #[test]
    fn new_assembler_is_identity_transformed() {
        let a = assembler();
        assert!(a.identity);
        let p = a.to_world(1.0, 2.0, 3.0);
        assert_eq!((p.x, p.y, p.z), (1.0, 2.0, 3.0));
    }

    #[test]
    fn set_transform_marks_non_identity_and_moves_points() {
        let mut a = assembler();
        a.set_transform(std::f32::consts::FRAC_PI_2, 5.0, 0.0);
        assert!(!a.identity);
        let p = a.to_world(0.0, 0.0, 1.0);
        // ry = 90deg about Y: (0,0,1) -> (1,0,0), then +5 on X.
        assert!((p.x - 6.0).abs() < 1e-5);
        assert!(p.z.abs() < 1e-5);
    }

    #[test]
    fn add_then_finalize_produces_one_static_mesh_per_key() {
        let mut a = assembler();
        a.add("concrete", &chamfer_box(1.0, 1.0, 1.0, 0.012), None, None);
        a.add("sand", &chamfer_box(1.0, 1.0, 1.0, 0.012), None, None);
        let result = a.finalize();
        assert_eq!(result.statics.len(), 2);
        assert_eq!(result.stats.draw_calls, 2);
        assert_eq!(result.stats.static_tris, 88);
    }

    #[test]
    fn empty_static_batch_never_reaches_finalize_output() {
        let mut a = assembler();
        // add() with an empty geometry never makes the batch non-empty.
        a.add("concrete", &WorldGeo::default(), None, None);
        let result = a.finalize();
        assert!(result.statics.is_empty());
    }

    #[test]
    fn surface_of_resolves_a_known_key_and_defaults_unknown_to_concrete() {
        let a = assembler();
        assert_eq!(a.surface_of("sand"), Surface::Sand);
        assert_eq!(a.surface_of("nonexistent"), Surface::Concrete);
    }

    #[test]
    fn proto_is_idempotent() {
        let mut a = assembler();
        let id1 = a.proto("barrel", ProtoSpec { geo: chamfer_box(1.0, 1.0, 1.0, 0.01), key: "metal_rust".into(), tilt: 0.0, sink: 0.0, skirt: 0.0, cast_shadow: true, receive_shadow: true, chunk: true, max_dist: 0.0, no_prepass: false });
        let id2 = a.proto("barrel", ProtoSpec { geo: chamfer_box(2.0, 2.0, 2.0, 0.01), key: "wood".into(), tilt: 0.0, sink: 0.0, skirt: 0.0, cast_shadow: true, receive_shadow: true, chunk: true, max_dist: 0.0, no_prepass: false });
        assert_eq!(id1, id2);
        // The second spec (different geo/key) never took effect.
        a.put_s("barrel", 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, None, 0.0, 0.0);
        let result = a.finalize();
        assert_eq!(result.instanced[0].key, "metal_rust");
    }

    #[test]
    fn placing_an_unknown_prototype_does_not_panic() {
        let mut a = assembler();
        a.place("nope", &Mat4::IDENTITY, None);
        assert_eq!(a.count("nope"), 0);
    }

    #[test]
    fn put_and_finalize_round_trips_instance_count_and_triangles() {
        let mut a = assembler();
        a.proto(
            "crate_",
            ProtoSpec {
                geo: chamfer_box(1.0, 1.0, 1.0, 0.01),
                key: "wood".into(),
                tilt: 0.0,
                sink: 0.0,
                skirt: 0.0,
                cast_shadow: true,
                receive_shadow: true,
                chunk: true,
                max_dist: 0.0,
                no_prepass: false,
            },
        );
        for i in 0..3 {
            a.put("crate_", i as f32, 0.0, 0.0, 0.0, 1.0, None, 0.0, 0.0);
        }
        let result = a.finalize();
        assert_eq!(result.instanced.len(), 1);
        assert_eq!(result.instanced[0].matrices.len(), 3);
        assert_eq!(result.stats.instances, 3);
        assert_eq!(result.stats.inst_tris, 44 * 3);
        assert!(result.instanced[0].masks.is_empty());
    }

    #[test]
    fn put_with_masks_populates_the_instance_color_buffer_for_every_instance_in_the_bucket() {
        let mut a = assembler();
        a.proto(
            "sign",
            ProtoSpec {
                geo: chamfer_box(1.0, 1.0, 1.0, 0.01),
                key: "metal_rust".into(),
                tilt: 0.0,
                sink: 0.0,
                skirt: 0.0,
                cast_shadow: true,
                receive_shadow: true,
                chunk: true,
                max_dist: 0.0,
                no_prepass: false,
            },
        );
        a.put("sign", 0.0, 0.0, 0.0, 0.0, 1.0, Some([0.5, 0.2, 0.1]), 0.0, 0.0);
        a.put("sign", 1.0, 0.0, 0.0, 0.0, 1.0, None, 0.0, 0.0);
        let result = a.finalize();
        assert_eq!(result.instanced[0].masks.len(), 2);
        assert_eq!(result.instanced[0].masks[0], [0.5, 0.2, 0.1]);
        assert_eq!(result.instanced[0].masks[1], [1.0, 1.0, 1.0]);
    }

    #[test]
    fn instances_beyond_24_in_a_chunked_prototype_split_into_buckets_by_position() {
        let mut a = assembler();
        a.proto(
            "post",
            ProtoSpec {
                geo: chamfer_box(0.1, 0.1, 0.1, 0.01),
                key: "wood".into(),
                tilt: 0.0,
                sink: 0.0,
                skirt: 0.0,
                cast_shadow: true,
                receive_shadow: true,
                chunk: true,
                max_dist: 0.0,
                no_prepass: false,
            },
        );
        // 30 instances clustered near x=0, 1 far away at x=1000 -> a second chunk.
        for i in 0..30 {
            a.put("post", i as f32 * 0.1, 0.0, 0.0, 0.0, 1.0, None, 0.0, 0.0);
        }
        a.put("post", 1000.0, 0.0, 0.0, 0.0, 1.0, None, 0.0, 0.0);
        let result = a.finalize();
        assert_eq!(result.instanced.len(), 2);
        let total: usize = result.instanced.iter().map(|d| d.matrices.len()).sum();
        assert_eq!(total, 31);
    }

    #[test]
    fn skirts_are_dropped_under_a_skirted_prototype_when_dust_skirt_exists() {
        let mut a = assembler();
        a.proto("dust_skirt", ProtoSpec { geo: chamfer_box(1.0, 0.02, 1.0, 0.005), key: "dust_skirt".into(), tilt: 0.0, sink: 0.0, skirt: 0.0, cast_shadow: false, receive_shadow: true, chunk: false, max_dist: 0.0, no_prepass: false });
        a.proto("box_prop", ProtoSpec { geo: chamfer_box(1.0, 1.0, 1.0, 0.01), key: "wood".into(), tilt: 0.0, sink: 0.0, skirt: 0.3, cast_shadow: true, receive_shadow: true, chunk: false, max_dist: 0.0, no_prepass: false });
        a.put("box_prop", 0.0, 0.0, 0.0, 0.0, 1.0, None, 0.0, 0.0);
        assert_eq!(a.count("dust_skirt"), 1);
    }

    #[test]
    fn jitter_perturbs_tilted_prototypes_deterministically() {
        let mut a = assembler();
        a.proto(
            "barrel",
            ProtoSpec {
                geo: chamfer_box(1.0, 1.0, 1.0, 0.01),
                key: "metal_rust".into(),
                tilt: 0.2,
                sink: 0.05,
                skirt: 0.0,
                cast_shadow: true,
                receive_shadow: true,
                chunk: false,
                max_dist: 0.0,
                no_prepass: false,
            },
        );
        a.jitter = Some(Jitter { rng: Rng::new(7), yaw: 0.3, scale: 0.1 });
        a.put("barrel", 0.0, 1.0, 0.0, 0.0, 1.0, None, 0.0, 0.0);
        let result = a.finalize();
        let m = result.instanced[0].matrices[0];
        // Under jitter with tilt>0, y is sunk by `sink` (0.05), so the
        // translation's Y is strictly less than the input 1.0.
        let c = m.as_cols_array();
        assert!(c[13] < 1.0);
    }

    #[test]
    fn collide_box_and_slab_box_merge_into_the_surface_bucket() {
        let mut a = assembler();
        a.collide_box(Surface::Dirt, 0.0, 0.0, 0.0, 2.0, 1.0, 2.0, 0.0);
        a.slab_box(Surface::Dirt, &Mat4::IDENTITY, 0.0, 0.0, 1.0, 1.0, 0.2);
        let result = a.finalize();
        assert_eq!(result.collision.len(), 1);
        assert_eq!(result.collision[0].surface, Surface::Dirt);
        assert_eq!(result.stats.collide_tris, 24);
    }

    #[test]
    fn light_transforms_position_by_the_level_to_world_transform() {
        let mut a = assembler();
        a.set_transform(0.0, 10.0, 0.0);
        a.light(Vec3::new(0.0, 2.0, 0.0), ());
        let result = a.finalize();
        assert_eq!(result.lights.len(), 1);
        assert!((result.lights[0].position.x - 10.0).abs() < 1e-6);
    }

    #[test]
    fn cache_returns_equal_geometry_on_repeated_calls_without_rerunning_the_factory() {
        let mut a = assembler();
        let mut calls = 0;
        let g1 = a.cache("box", || {
            calls += 1;
            chamfer_box(1.0, 1.0, 1.0, 0.01)
        });
        let g2 = a.cache("box", || {
            calls += 1;
            chamfer_box(1.0, 1.0, 1.0, 0.01)
        });
        assert_eq!(calls, 1);
        assert_eq!(g1, g2);
    }
}
