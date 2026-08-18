//! Ported from Claude-of-Duty `src/world/util.js:98-201` — the `Accum` class:
//! merges any number of transformed geometries into one indexed
//! [`WorldGeo`].
//!
//! Every module in `src/world/` writes into an `Accum` per palette key
//! (`Assembler.add`) or per collision surface (`Assembler.box`/`collideGeo`/
//! `slabBox`) rather than building its own draw call, which is how a map with
//! hundreds of thousands of triangles comes out as roughly a hundred merged
//! meshes. Unlike `weapons::geometry::merge_all`, `Accum` does **not** weld
//! coincident vertices across adds (`util.js` never calls `mergeVertices`
//! here) — it simply re-indexes every input's own vertex stream onto the
//! growing output, one input at a time.

use axiom_math::Mat4;

use super::geo::WorldGeo;

/// `Accum.add`'s `opts` (`util.js:119-123`): `{ masks:[w,g,ao],
/// paint(x,y,z,nx,ny,nz,out) }`. `mulMasks` is named in the source's doc
/// comment but never read by the implementation (`util.js:124-181` never
/// touches `opts.mulMasks`) — not carried here for that reason.
pub struct AccumAddOpts<'a> {
    pub masks: Option<[f32; 3]>,
    /// `paint(x, y, z, nx, ny, nz, out)`, run in WORLD space (after `matrix`
    /// has already moved `x,y,z,nx,ny,nz` — `util.js:142-146` transforms
    /// first, `:159-167` paints after), given the pre-seeded `[r, g, b]` (the
    /// geometry's own color, or `masks`-overridden, whichever ran first).
    pub paint: Option<&'a mut dyn FnMut(f32, f32, f32, f32, f32, f32, &mut [f32; 3])>,
}

impl Default for AccumAddOpts<'_> {
    fn default() -> Self {
        AccumAddOpts { masks: None, paint: None }
    }
}

/// `class Accum` (`util.js:103-201`).
#[derive(Debug, Clone, Default)]
pub struct Accum {
    pub name: String,
    pos: Vec<f32>,
    nrm: Vec<f32>,
    uv: Vec<f32>,
    col: Vec<f32>,
    idx: Vec<u32>,
    verts: usize,
    tris: usize,
}

impl Accum {
    /// `constructor(name = 'merged')` (`util.js:104-113`).
    pub fn new(name: &str) -> Self {
        Accum {
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// `get empty()` (`util.js:115-117`).
    pub fn empty(&self) -> bool {
        self.tris == 0
    }

    /// `add(geo, matrix = null, opts = null)` (`util.js:124-181`).
    ///
    /// `geo` is transformed by `matrix` (when given) exactly as
    /// [`WorldGeo::apply`] does — position as a point, normal by the normal
    /// matrix — then its `position`/`normal`/`uv`/`color` (color defaulting
    /// to `[0,0,0]` per vertex when the input carries none) are appended to
    /// this accumulator's own flat buffers, remapping `geo`'s index (or, for
    /// a non-indexed `geo`, an implicit identity index) onto the running
    /// vertex base. `masks` and `paint`, when given, are applied in that
    /// order — `masks` widens each channel via `max`, `paint` then runs on
    /// top with the (possibly `masks`-widened) triple seeded into `out`.
    pub fn add(&mut self, geo: &WorldGeo, matrix: Option<&Mat4>, mut opts: Option<AccumAddOpts>) -> &mut Self {
        if geo.pos.is_empty() {
            return self;
        }
        let mut g = geo.clone();
        if g.normal.is_empty() {
            g.compute_vertex_normals();
        }
        if let Some(m) = matrix {
            g.apply(m);
        }

        let base = self.verts as u32;
        let vert_count = g.vert_count();
        for i in 0..vert_count {
            let px = g.pos[i * 3];
            let py = g.pos[i * 3 + 1];
            let pz = g.pos[i * 3 + 2];
            let nx = g.normal[i * 3];
            let ny = g.normal[i * 3 + 1];
            let nz = g.normal[i * 3 + 2];
            self.pos.extend_from_slice(&[px, py, pz]);
            self.nrm.extend_from_slice(&[nx, ny, nz]);
            self.uv.push(if g.uv.is_empty() { 0.0 } else { g.uv[i * 2] });
            self.uv.push(if g.uv.is_empty() { 0.0 } else { g.uv[i * 2 + 1] });

            let mut r = if g.color.is_empty() { 0.0 } else { g.color[i * 3] };
            let mut gr = if g.color.is_empty() { 0.0 } else { g.color[i * 3 + 1] };
            let mut b = if g.color.is_empty() { 0.0 } else { g.color[i * 3 + 2] };
            if let Some(opts) = opts.as_ref() {
                if let Some(masks) = opts.masks {
                    r = r.max(masks[0]);
                    gr = gr.max(masks[1]);
                    b = b.max(masks[2]);
                }
            }
            if let Some(opts) = opts.as_mut() {
                if let Some(paint) = opts.paint.as_mut() {
                    let mut out = [r, gr, b];
                    paint(px, py, pz, nx, ny, nz, &mut out);
                    r = out[0];
                    gr = out[1];
                    b = out[2];
                }
            }
            self.col.extend_from_slice(&[r, gr, b]);
            self.verts += 1;
        }

        if g.index.is_empty() {
            for i in 0..vert_count as u32 {
                self.idx.push(base + i);
            }
            self.tris += vert_count / 3;
        } else {
            for &i in &g.index {
                self.idx.push(base + i);
            }
            self.tris += g.index.len() / 3;
        }
        self
    }

    /// `build()` (`util.js:183-200`): package the accumulated buffers into a
    /// [`WorldGeo`]. The source additionally picks a `Uint16`/`Uint32` index
    /// width and frees its scratch arrays — both pure JS-side memory
    /// concerns with no Rust counterpart (a `Vec<u32>` already holds the
    /// index either way, and there is no separate "free the scratch" step to
    /// mirror once ownership moves out of `self`).
    pub fn build(self) -> WorldGeo {
        WorldGeo {
            pos: self.pos,
            normal: self.nrm,
            uv: self.uv,
            color: self.col,
            index: self.idx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    fn triangle_with_color() -> WorldGeo {
        WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uv: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            color: vec![0.1, 0.2, 0.3, 0.1, 0.2, 0.3, 0.1, 0.2, 0.3],
            index: Vec::new(),
        }
    }

    fn triangle_no_color() -> WorldGeo {
        WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uv: Vec::new(),
            color: Vec::new(),
            index: vec![0, 1, 2],
        }
    }

    #[test]
    fn new_accum_is_empty() {
        let a = Accum::new("test");
        assert!(a.empty());
    }

    #[test]
    fn add_a_triangle_makes_it_non_empty_and_counts_one_tri() {
        let mut a = Accum::new("t");
        a.add(&triangle_with_color(), None, None);
        assert!(!a.empty());
        let g = a.build();
        assert_eq!(g.vert_count(), 3);
        assert_eq!(g.tri_count(), 1);
        assert_eq!(g.index, vec![0, 1, 2]);
    }

    #[test]
    fn missing_color_defaults_to_zero() {
        let mut a = Accum::new("t");
        a.add(&triangle_no_color(), None, None);
        let g = a.build();
        assert_eq!(g.color, vec![0.0; 9]);
    }

    #[test]
    fn missing_uv_defaults_to_zero() {
        let mut a = Accum::new("t");
        a.add(&triangle_no_color(), None, None);
        let g = a.build();
        assert_eq!(g.uv, vec![0.0; 6]);
    }

    #[test]
    fn masks_widen_via_max_not_overwrite() {
        let mut a = Accum::new("t");
        let opts = AccumAddOpts {
            masks: Some([0.5, 0.0, 0.9]),
            paint: None,
        };
        a.add(&triangle_with_color(), None, Some(opts));
        let g = a.build();
        // channel 0: max(0.1, 0.5) = 0.5; channel 1: max(0.2, 0.0) = 0.2; channel 2: max(0.3, 0.9) = 0.9
        assert_eq!(&g.color[0..3], &[0.5, 0.2, 0.9]);
    }

    #[test]
    fn paint_runs_after_masks_and_sees_the_widened_triple() {
        let mut a = Accum::new("t");
        let mut paint = |_x: f32, _y: f32, _z: f32, _nx: f32, _ny: f32, _nz: f32, out: &mut [f32; 3]| {
            out[0] += 1.0;
        };
        let opts = AccumAddOpts {
            masks: Some([0.5, 0.0, 0.0]),
            paint: Some(&mut paint),
        };
        a.add(&triangle_with_color(), None, Some(opts));
        let g = a.build();
        assert_eq!(g.color[0], 1.5);
    }

    #[test]
    fn second_add_offsets_indices_by_the_running_vertex_base() {
        let mut a = Accum::new("t");
        a.add(&triangle_no_color(), None, None);
        a.add(&triangle_no_color(), None, None);
        let g = a.build();
        assert_eq!(g.index, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(g.vert_count(), 6);
        assert_eq!(g.tri_count(), 2);
    }

    #[test]
    fn add_transforms_position_and_normal_by_the_matrix() {
        let mut a = Accum::new("t");
        let m = Mat4::translation(Vec3::new(10.0, 0.0, 0.0));
        a.add(&triangle_no_color(), Some(&m), None);
        let g = a.build();
        assert!((g.pos[0] - 10.0).abs() < 1e-6);
        assert!((g.normal[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn add_on_a_geometry_with_no_positions_is_a_no_op() {
        let mut a = Accum::new("t");
        let empty = WorldGeo::default();
        a.add(&empty, None, None);
        assert!(a.empty());
    }

    #[test]
    fn add_computes_normals_when_the_input_has_none() {
        let mut a = Accum::new("t");
        let g = WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: Vec::new(),
            uv: Vec::new(),
            color: Vec::new(),
            index: Vec::new(),
        };
        a.add(&g, None, None);
        let out = a.build();
        assert!((out.normal[2] - 1.0).abs() < 1e-6);
    }
}
