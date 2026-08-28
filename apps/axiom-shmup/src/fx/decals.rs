//! Ported from Claude-of-Duty `src/fx/decals.js:1-420` — the whole file.
//!
//! Projected decals: a decal is a box projector. The static world's triangle
//! soup is pulled out of the physics BVH, any triangle whose normal deviates
//! too far from the impact normal is rejected, what is left is clipped
//! against the six planes of the projector box (Sutherland–Hodgman) and the
//! resulting polygons are fanned into a preallocated vertex ring.
//!
//! That gives what a screen-space decal cannot: the decal wraps around
//! corners and window reveals, and it cannot smear across a perpendicular
//! face, because the perpendicular face is discarded before clipping. The
//! box's thickness also depth-clips the projection.
//!
//! ## The physics seam
//!
//! [`DecalWorld`] mirrors [`crate::weapons::ballistics::RaycastWorld`]'s
//! established pattern — see that trait's doc for why a seam and not a
//! concrete type. The source reads a static world's triangle soup directly
//! (`world.pos`/`world.nrm`/`world.candidates`, `world.queryAabb(...)`,
//! `atlas.js:220-236`); the landed `crate::physics::bvh::StaticWorld`
//! exposes `query_aabb` and per-triangle accessors for its *bounds* and
//! *normal* (`node_bounds`, `normal_of`, `surface_of`) but not yet the raw
//! triangle *vertex positions* this clipper needs — a small, additive,
//! narrow accessor a future integration pass can add without touching
//! anything below it. [`DecalWorld`] names exactly the shape that accessor
//! would satisfy.

use crate::fx::atlas::ATLAS_COLS;

/// A physics static-world seam this port needs and `crate::physics::bvh::
/// StaticWorld` does not expose yet — see the module doc.
pub trait DecalWorld {
    fn tri_count(&self) -> usize;
    /// `world.queryAabb(minx,miny,minz,maxx,maxy,maxz,mask)` —
    /// `atlas.js:220-225`, candidate triangle indices.
    fn query_aabb(&self, min: [f64; 3], max: [f64; 3], mask: u16) -> Vec<u32>;
    /// The three world-space vertices and the face normal of triangle `tri`.
    fn triangle(&self, tri: u32) -> ([[f64; 3]; 3], [f64; 3]);
}

const MAX_POLY: usize = 24;
const VERTS_PER_DECAL: usize = 36;

/// Where one decal was placed, kept alongside the clipped vertex soup so a
/// renderer that cannot upload per-frame geometry can still draw the decal.
///
/// **This is not in the source.** The source's decal *is* the clipped triangle
/// soup: it wraps corners and window reveals, which is the whole reason
/// [`DecalSystem::add`] exists. Axiom uploads mesh geometry once, at
/// registration, and has no per-frame geometry update — so the soup can never
/// reach a pixel in this engine, and a renderer's only option is the
/// **projector's own face quad**, which is exactly the fallback the source
/// itself lays down when the BVH has no triangles under the impact
/// (`decals.js:334-357`, the `wrote == 0` arm).
///
/// So this records the frame `add` already computed and threw away — the
/// orthonormal `(tangent, bitangent, normal)` basis after the roll, the
/// half-size, and the four life terms `dec` packs per vertex — rather than
/// making a consumer re-derive a basis that would not match. Nothing in the
/// port reads it except `crate::scene::wiring::fx_draw`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecalPlacement {
    /// Whether this slot holds a decal at all. `false` for a ring slot nothing
    /// has written yet.
    pub occupied: bool,
    /// `o.point` — the impact point the projector box is centred on.
    pub point: [f64; 3],
    /// The **normalised** surface normal (`nx, ny, nzz` in `add`).
    pub normal: [f64; 3],
    /// The projector's U axis, after `o.roll` (`tx, ty, tz` in `add`).
    pub tangent: [f64; 3],
    /// The projector's V axis (`bx, by, bz` in `add`) — `normal x tangent`.
    pub bitangent: [f64; 3],
    /// `o.size * 0.5`, the half-extent the quad spans along U and V.
    pub half_size: f64,
    /// `o.tile` — the decal-atlas tile index the recipe chose.
    pub tile: usize,
    /// `o.now`, the frame it was laid.
    pub birth: f64,
    /// The resolved life in seconds (`o.life ?? 30`, floored at `0.2`).
    pub life: f64,
    /// `o.fade` — the source's third `dec` lane.
    pub fade: f64,
    /// `o.opacity` — the decal's authored peak opacity, the fourth `dec` lane.
    pub opacity: f64,
    /// `add`'s own return: whether any geometry was written. A decal that wrote
    /// nothing is one the source draws nothing for.
    pub wrote: bool,
}

impl Default for DecalPlacement {
    fn default() -> Self {
        DecalPlacement {
            occupied: false,
            point: [0.0; 3],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
            half_size: 0.0,
            tile: 0,
            birth: 0.0,
            life: 1.0,
            fade: 0.72,
            opacity: 1.0,
            wrote: false,
        }
    }
}

/// `DecalSystem.add(o)`'s parameters, `decals.js:154-172`. Every field here
/// mirrors an already-resolved value — the two-layer default nesting the
/// source has (`FxSystem.addDecal` resolves its own `??` defaults before
/// calling `DecalSystem.add`, which then applies a second layer of `??`
/// defaults of its own) is preserved as `Option` fields resolved inside
/// [`DecalSystem::add`], exactly at the source's own default sites.
pub struct DecalAdd<'a> {
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub size: f64,
    pub tile: usize,
    pub roll: Option<f64>,
    pub life: Option<f64>,
    pub fade: Option<f64>,
    pub opacity: Option<f64>,
    pub max_angle: Option<f64>,
    pub depth: Option<f64>,
    pub flip: bool,
    pub mask: u16,
    pub now: f64,
    pub world: Option<&'a dyn DecalWorld>,
}

/// `class DecalSystem`, `decals.js:26-420`.
pub struct DecalSystem {
    pub capacity: usize,
    cols: u32,
    cursor: usize,
    high_water: usize,
    expire_at: f64,
    pub count: u64,

    pos: Vec<f32>,
    nrm: Vec<f32>,
    uvs: Vec<f32>,
    dec: Vec<f32>,

    dirty_lo: usize,
    dirty_hi: Option<usize>,
    wrapped: bool,

    /// One [`DecalPlacement`] per ring slot — the projector frame `add`
    /// resolves and the vertex soup does not carry. See that type's doc for why
    /// this exists and why it is not in the source.
    placements: Vec<DecalPlacement>,
}

/// [`DecalSystem::flush`]'s per-frame result — see [`crate::fx::particles::
/// FlushResult`] for why this is returned rather than mutated into a
/// `THREE.Mesh`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecalFlushResult {
    pub dirty_range: Option<(usize, usize)>,
    pub vertex_draw_count: usize,
    pub visible: bool,
}

impl DecalSystem {
    /// `constructor(o)`, `decals.js:33-124` — minus the `THREE.*` buffer/
    /// material/mesh construction (the GPU-upload seam; see [`crate::fx::
    /// atlas`]'s module doc for the same split). `capacity` is clamped to a
    /// minimum of 8, matching `Math.max(8, o.capacity | 0)`.
    pub fn new(capacity: usize, cols: u32) -> Self {
        let capacity = capacity.max(8);
        let max_verts = capacity * VERTS_PER_DECAL;
        DecalSystem {
            capacity,
            cols,
            cursor: 0,
            high_water: 0,
            expire_at: -1.0,
            count: 0,
            pos: vec![0.0; max_verts * 3],
            nrm: vec![0.0; max_verts * 3],
            uvs: vec![0.0; max_verts * 2],
            dec: vec![0.0; max_verts * 4],
            dirty_lo: usize::MAX,
            dirty_hi: None,
            wrapped: false,
            placements: vec![DecalPlacement::default(); capacity],
        }
    }

    /// One entry per ring slot, in slot order — see [`DecalPlacement`].
    ///
    /// Slots are recycled by [`DecalSystem::add`]'s wrapping cursor, so an entry
    /// with `occupied == true` may still be *expired*: the reader decides, from
    /// `birth`/`life`, exactly as the source's decal shader does per vertex.
    pub fn placements(&self) -> &[DecalPlacement] {
        &self.placements
    }

    pub fn raw_positions(&self) -> &[f32] {
        &self.pos
    }

    pub fn raw_normals(&self) -> &[f32] {
        &self.nrm
    }

    pub fn raw_uvs(&self) -> &[f32] {
        &self.uvs
    }

    /// Per-vertex decal metadata: `birth, 1/life, fade, opacity` — the
    /// source's `this.dec` (`decals.js:60`).
    pub fn raw_decal_meta(&self) -> &[f32] {
        &self.dec
    }

    /// The ring cursor — `this.cursor` (`decals.js:39`). Exposed for tests
    /// pinning the eviction order against the JavaScript.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether the ring has wrapped at least once — `this._wrapped`
    /// (`decals.js:124`).
    pub fn wrapped(&self) -> bool {
        self.wrapped
    }

    /// Clip polygon `src`/`n` against plane `axis` (0=U,1=V,2=depth), `sign`
    /// (+1/-1), `limit`. `_clip`, `decals.js:132-160`.
    fn clip(src: &[[f64; 3]], n: usize, axis: usize, sign: f64, limit: f64) -> Vec<[f64; 3]> {
        let mut dst = Vec::with_capacity(n + 2);
        for i in 0..n {
            let j = (i + 1) % n;
            let av = src[i][axis] * sign;
            let bv = src[j][axis] * sign;
            let ain = av <= limit;
            let bin = bv <= limit;
            if ain && dst.len() < MAX_POLY {
                dst.push(src[i]);
            }
            if ain != bin && dst.len() < MAX_POLY {
                let t = (limit - av) / (bv - av);
                dst.push([
                    src[i][0] + (src[j][0] - src[i][0]) * t,
                    src[i][1] + (src[j][1] - src[i][1]) * t,
                    src[i][2] + (src[j][2] - src[i][2]) * t,
                ]);
            }
        }
        dst
    }

    fn write_uv(&mut self, w: usize, lx: f64, ly: f64, hs: f64, tile: usize, flip: bool) {
        let cols = self.cols as usize;
        let tx = (tile % cols) as f64;
        let ty = (tile / cols) as f64;
        let mut u = lx / (2.0 * hs) + 0.5;
        let v = ly / (2.0 * hs) + 0.5;
        if flip {
            u = 1.0 - u;
        }
        self.uvs[w * 2] = ((u + tx) / cols as f64) as f32;
        self.uvs[w * 2 + 1] = ((v + ty) / cols as f64) as f32;
    }

    /// `add(o)`, `decals.js:174-357`. Returns whether any geometry was
    /// written (`decals.js:355`, `return wrote > 0`).
    pub fn add(&mut self, o: &DecalAdd) -> bool {
        let size = o.size;
        let hs = size * 0.5;
        let hd = o.depth.unwrap_or_else(|| (0.045f64).max(size * 0.35));
        let p = o.point;
        let nz_in = o.normal;

        let nl = {
            let h = (nz_in[0] * nz_in[0] + nz_in[1] * nz_in[1] + nz_in[2] * nz_in[2]).sqrt();
            if h == 0.0 {
                1.0
            } else {
                h
            }
        };
        let (nx, ny, nzz) = (nz_in[0] / nl, nz_in[1] / nl, nz_in[2] / nl);

        // Roll reference: gravity-aligned on walls, arbitrary on floors/ceilings.
        let (mut ux, mut uy, mut uz) = (0.0, 1.0, 0.0);
        if ny.abs() > 0.94 {
            ux = 1.0;
            uy = 0.0;
            uz = 0.0;
        }
        let d = ux * nx + uy * ny + uz * nzz;
        let (mut tx, mut ty, mut tz) = (ux - nx * d, uy - ny * d, uz - nzz * d);
        let mut tl = (tx * tx + ty * ty + tz * tz).sqrt();
        if tl < 1e-4 {
            tx = 1.0;
            ty = 0.0;
            tz = 0.0;
            tl = 1.0;
        }
        tx /= tl;
        ty /= tl;
        tz /= tl;
        let roll = o.roll.unwrap_or(0.0);
        if roll != 0.0 {
            let c = roll.cos();
            let s = roll.sin();
            let cx = ny * tz - nzz * ty;
            let cy = nzz * tx - nx * tz;
            let cz = nx * ty - ny * tx;
            let rx = tx * c + cx * s;
            let ry = ty * c + cy * s;
            let rz = tz * c + cz * s;
            tx = rx;
            ty = ry;
            tz = rz;
        }
        let bx = ny * tz - nzz * ty;
        let by = nzz * tx - nx * tz;
        let bz = nx * ty - ny * tx;

        let slot = self.cursor;
        self.cursor = slot + 1;
        if self.cursor >= self.capacity {
            self.cursor = 0;
            self.wrapped = true;
        }
        let mut w = slot * VERTS_PER_DECAL;
        let limit = w + VERTS_PER_DECAL;

        let cos_limit = (o.max_angle.unwrap_or(62.0) * std::f64::consts::PI / 180.0).cos();

        let mut wrote = 0usize;
        if let Some(world) = o.world {
            if world.tri_count() > 0 {
                let rad = hs.max(hd) * 1.5;
                let cand = world.query_aabb(
                    [p[0] - rad, p[1] - rad, p[2] - rad],
                    [p[0] + rad, p[1] + rad, p[2] + rad],
                    o.mask,
                );
                for &tri in &cand {
                    if w + 3 > limit {
                        break;
                    }
                    let (verts, fn3) = world.triangle(tri);
                    let (fnx, fny, fnz) = (fn3[0], fn3[1], fn3[2]);
                    if fnx * nx + fny * ny + fnz * nzz < cos_limit {
                        continue;
                    }

                    let mut poly: Vec<[f64; 3]> = (0..3)
                        .map(|v| {
                            let dx = verts[v][0] - p[0];
                            let dy = verts[v][1] - p[1];
                            let dz = verts[v][2] - p[2];
                            [dx * tx + dy * ty + dz * tz, dx * bx + dy * by + dz * bz, dx * nx + dy * ny + dz * nzz]
                        })
                        .collect();

                    poly = Self::clip(&poly, poly.len(), 0, 1.0, hs);
                    if poly.len() < 3 {
                        continue;
                    }
                    poly = Self::clip(&poly, poly.len(), 0, -1.0, hs);
                    if poly.len() < 3 {
                        continue;
                    }
                    poly = Self::clip(&poly, poly.len(), 1, 1.0, hs);
                    if poly.len() < 3 {
                        continue;
                    }
                    poly = Self::clip(&poly, poly.len(), 1, -1.0, hs);
                    if poly.len() < 3 {
                        continue;
                    }
                    poly = Self::clip(&poly, poly.len(), 2, 1.0, hd);
                    if poly.len() < 3 {
                        continue;
                    }
                    poly = Self::clip(&poly, poly.len(), 2, -1.0, hd);
                    if poly.len() < 3 {
                        continue;
                    }

                    let lift = 0.0016 + size * 0.004;
                    let m = poly.len();
                    let mut v = 1usize;
                    while v + 1 < m && w + 3 <= limit {
                        for &vi in &[0usize, v, v + 1] {
                            let (lx, ly, lz) = (poly[vi][0], poly[vi][1], poly[vi][2]);
                            let wx = p[0] + tx * lx + bx * ly + nx * lz + fnx * lift;
                            let wy = p[1] + ty * lx + by * ly + ny * lz + fny * lift;
                            let wz = p[2] + tz * lx + bz * ly + nzz * lz + fnz * lift;
                            self.pos[w * 3] = wx as f32;
                            self.pos[w * 3 + 1] = wy as f32;
                            self.pos[w * 3 + 2] = wz as f32;
                            self.nrm[w * 3] = fnx as f32;
                            self.nrm[w * 3 + 1] = fny as f32;
                            self.nrm[w * 3 + 2] = fnz as f32;
                            self.write_uv(w, lx, ly, hs, o.tile, o.flip);
                            w += 1;
                            wrote += 1;
                        }
                        v += 1;
                    }
                }
            }
        }

        // Fallback: nothing in the BVH here — lay a single quad on the plane.
        if wrote == 0 && w + 6 <= limit {
            let lift = 0.004 + size * 0.01;
            let quad: [[f64; 2]; 6] = [
                [-hs, -hs],
                [hs, -hs],
                [hs, hs],
                [-hs, -hs],
                [hs, hs],
                [-hs, hs],
            ];
            for q in quad {
                let (lx, ly) = (q[0], q[1]);
                self.pos[w * 3] = (p[0] + tx * lx + bx * ly + nx * lift) as f32;
                self.pos[w * 3 + 1] = (p[1] + ty * lx + by * ly + ny * lift) as f32;
                self.pos[w * 3 + 2] = (p[2] + tz * lx + bz * ly + nzz * lift) as f32;
                self.nrm[w * 3] = nx as f32;
                self.nrm[w * 3 + 1] = ny as f32;
                self.nrm[w * 3 + 2] = nzz as f32;
                self.write_uv(w, lx, ly, hs, o.tile, o.flip);
                w += 1;
                wrote += 1;
            }
        }

        // Degenerate the rest of the slot so a shorter decal does not leave
        // the previous occupant's triangles behind.
        let life = o.life.unwrap_or(30.0).max(0.2);
        let birth = o.now;
        let fade = o.fade.unwrap_or(0.72);
        let opacity = o.opacity.unwrap_or(1.0);
        let base = slot * VERTS_PER_DECAL;
        for v in base..limit {
            if v >= base + wrote {
                self.pos[v * 3] = 0.0;
                self.pos[v * 3 + 1] = 0.0;
                self.pos[v * 3 + 2] = 0.0;
            }
            self.dec[v * 4] = birth as f32;
            self.dec[v * 4 + 1] = (1.0 / life) as f32;
            self.dec[v * 4 + 2] = fade as f32;
            self.dec[v * 4 + 3] = opacity as f32;
        }

        // The projector frame, recorded for a renderer that cannot upload the
        // soup above. Written from the SAME locals the soup was built from, so
        // the quad and the clipped triangles can never disagree about where the
        // decal is or which way it faces. See [`DecalPlacement`].
        self.placements[slot] = DecalPlacement {
            occupied: true,
            point: p,
            normal: [nx, ny, nzz],
            tangent: [tx, ty, tz],
            bitangent: [bx, by, bz],
            half_size: hs,
            tile: o.tile,
            birth,
            life,
            fade,
            opacity,
            wrote: wrote > 0,
        };

        if slot < self.dirty_lo {
            self.dirty_lo = slot;
        }
        self.dirty_hi = Some(self.dirty_hi.map_or(slot, |hi| hi.max(slot)));
        if slot + 1 > self.high_water {
            self.high_water = slot + 1;
        }
        if birth + life > self.expire_at {
            self.expire_at = birth + life;
        }
        self.count += 1;
        wrote > 0
    }

    /// `flush(now)`, `decals.js:396-413`.
    pub fn flush(&mut self, now: f64) -> DecalFlushResult {
        let dirty_range = self.dirty_hi.map(|hi| {
            let vpd = VERTS_PER_DECAL;
            (self.dirty_lo * vpd, (hi - self.dirty_lo + 1) * vpd)
        });
        self.dirty_lo = usize::MAX;
        self.dirty_hi = None;

        let verts = (if self.wrapped { self.capacity } else { self.high_water }) * VERTS_PER_DECAL;
        DecalFlushResult {
            dirty_range,
            vertex_draw_count: verts,
            visible: verts > 0 && now < self.expire_at,
        }
    }
}

/// Convenience: the source's default atlas column count for a decal system
/// constructed against [`crate::fx::atlas::bake_decal_atlas`]'s output.
pub const DEFAULT_COLS: u32 = ATLAS_COLS;

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyWorld;
    impl DecalWorld for EmptyWorld {
        fn tri_count(&self) -> usize {
            0
        }
        fn query_aabb(&self, _min: [f64; 3], _max: [f64; 3], _mask: u16) -> Vec<u32> {
            vec![]
        }
        fn triangle(&self, _tri: u32) -> ([[f64; 3]; 3], [f64; 3]) {
            ([[0.0; 3]; 3], [0.0, 1.0, 0.0])
        }
    }

    /// A single ground-plane quad, big enough to always cover the projector.
    struct FlatWorld;
    impl DecalWorld for FlatWorld {
        fn tri_count(&self) -> usize {
            2
        }
        fn query_aabb(&self, _min: [f64; 3], _max: [f64; 3], _mask: u16) -> Vec<u32> {
            vec![0, 1]
        }
        fn triangle(&self, tri: u32) -> ([[f64; 3]; 3], [f64; 3]) {
            if tri == 0 {
                ([[-10.0, 0.0, -10.0], [10.0, 0.0, -10.0], [10.0, 0.0, 10.0]], [0.0, 1.0, 0.0])
            } else {
                ([[-10.0, 0.0, -10.0], [10.0, 0.0, 10.0], [-10.0, 0.0, 10.0]], [0.0, 1.0, 0.0])
            }
        }
    }

    fn opts<'a>(point: [f64; 3], normal: [f64; 3], world: Option<&'a dyn DecalWorld>, now: f64) -> DecalAdd<'a> {
        DecalAdd {
            point,
            normal,
            size: 0.2,
            tile: 0,
            roll: None,
            life: None,
            fade: None,
            opacity: None,
            max_angle: None,
            depth: None,
            flip: false,
            mask: 0xffff,
            now,
            world,
        }
    }

    #[test]
    fn no_world_falls_back_to_a_single_quad() {
        let mut sys = DecalSystem::new(8, 4);
        let ok = sys.add(&opts([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], None, 0.0));
        assert!(ok);
    }

    #[test]
    fn empty_world_falls_back_the_same_as_no_world() {
        let mut sys = DecalSystem::new(8, 4);
        let w = EmptyWorld;
        let ok = sys.add(&opts([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], Some(&w), 0.0));
        assert!(ok);
    }

    #[test]
    fn a_flat_world_produces_real_clipped_geometry() {
        let mut sys = DecalSystem::new(8, 4);
        let w = FlatWorld;
        let ok = sys.add(&opts([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], Some(&w), 0.0));
        assert!(ok);
        // every written vertex should sit essentially on y=0 (lifted slightly).
        let pos = sys.raw_positions();
        for chunk in pos.chunks_exact(3).take(VERTS_PER_DECAL) {
            assert!(chunk[1].abs() < 0.01);
        }
    }

    #[test]
    fn ring_buffer_evicts_oldest_slot_first_at_budget() {
        let mut sys = DecalSystem::new(8, 4);
        // `cursor = slot + 1; if cursor >= capacity { cursor = 0; wrapped = true; }`
        // (`decals.js:186-190`) fires on the write that fills the *last*
        // slot, not on the write past it — so `wrapped` is already true
        // after exactly `capacity` adds, one call earlier than a "the ring
        // is full" reading might expect.
        for i in 0..7 {
            sys.add(&opts([i as f64, 0.0, 0.0], [0.0, 1.0, 0.0], None, i as f64));
        }
        assert_eq!(sys.cursor, 7);
        assert!(!sys.wrapped);
        sys.add(&opts([7.0, 0.0, 0.0], [0.0, 1.0, 0.0], None, 7.0));
        assert!(sys.wrapped);
        assert_eq!(sys.cursor, 0);
        // The ninth decal must land back in slot 0, overwriting the oldest.
        sys.add(&opts([99.0, 0.0, 0.0], [0.0, 1.0, 0.0], None, 8.0));
        assert_eq!(sys.cursor, 1);
        let pos = sys.raw_positions();
        // slot 0's first vertex should now be near x=99 (the fallback quad's
        // first corner is at local (-hs,-hs) around the new point).
        assert!((pos[0] as f64 - 99.0).abs() < 1.0);
    }

    #[test]
    fn pool_never_exceeds_its_capacity() {
        let mut sys = DecalSystem::new(8, 4);
        for i in 0..100 {
            sys.add(&opts([i as f64, 0.0, 0.0], [0.0, 1.0, 0.0], None, i as f64));
        }
        let f = sys.flush(100.0);
        assert!(f.vertex_draw_count <= sys.capacity * VERTS_PER_DECAL);
    }

    #[test]
    fn flush_clears_the_dirty_range() {
        let mut sys = DecalSystem::new(8, 4);
        sys.add(&opts([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], None, 0.0));
        let f1 = sys.flush(0.0);
        assert!(f1.dirty_range.is_some());
        let f2 = sys.flush(0.0);
        assert_eq!(f2.dirty_range, None);
    }
}
