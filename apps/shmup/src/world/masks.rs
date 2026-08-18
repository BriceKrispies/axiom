//! Ported from Claude-of-Duty `src/world/util.js:9-16, 203-240`
//! (the mask convention doc comment, `paintMasks`, `fillMasks`).
//!
//! ## The vertex-mask convention
//!
//! Every geometry in the level carries a `color` vertex attribute used as a
//! *mask*, matching the materials contract:
//!
//! - **r = edge wear** — convex chamfers, corners, anything that would catch a
//!   scrape or a boot.
//! - **g = grime** — undersides, reveals, anywhere dust and rain-runoff
//!   collect.
//! - **b = extra AO** — additional ambient occlusion baked in beyond whatever
//!   the lighting model derives from geometry alone.
//!
//! This is the cheapest high-value idea in the source (`util.js:12-16`):
//! builders author these masks **analytically**, not by detecting curvature at
//! runtime. Curvature detection cannot know that the bottom of a wall is where
//! the wind piles dust, or that a chamfer strip (rather than a flat face) is
//! where a hand would wear the paint off — those are facts about what the
//! shape *is for*, not about its local geometry, so the builder that knows the
//! shape's purpose paints the mask itself.
//!
//! ## Scope of this port
//!
//! Only the two mask *operations* — [`paint_masks`] and [`fill_masks`] — and
//! the [`MaskGeometry`] carrier they operate on are ported here. Every actual
//! *user* of them in the source (`weatherProp`, `chamferBox`, `wallPanel`,
//! `runoffStreak`, `clothGeometry`, …) builds real `THREE.BufferGeometry` —
//! position/normal/uv/index buffers, `Accum` merging, `THREE.Shape`/
//! `ExtrudeGeometry` — which is the geometry back end and belongs with the
//! Assembler port, not this one. [`MaskGeometry`] is deliberately minimal: it
//! carries exactly what these two functions need (position, normal, mask, all
//! per-vertex and index-aligned) and nothing else — no indices, no UVs, no
//! `build()`. When the Assembler port lands the real geometry type, these two
//! functions are expected to be re-pointed at it rather than at
//! `MaskGeometry`.

/// A minimal per-vertex carrier for the mask helpers to operate on.
///
/// Stand-in for the position/normal/color attributes of a `THREE.
/// BufferGeometry`, restricted to what [`paint_masks`] and [`fill_masks`]
/// touch. `positions`, `normals` and `masks` are index-aligned: vertex `i`'s
/// data lives at index `i` of each. The real geometry type (indices, UVs,
/// merging into one draw call) arrives with the Assembler port — see this
/// module's doc comment.
#[derive(Debug, Clone, Default)]
pub struct MaskGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// `[r = edge wear, g = grime, b = extra AO]` per vertex.
    pub masks: Vec<[f32; 3]>,
}

impl MaskGeometry {
    /// Build from position/normal columns, with an all-zero mask column — the
    /// source's `paintMasks` allocates exactly this (a zeroed `color`
    /// attribute) when the geometry doesn't already have one (`util.js:212-216`).
    ///
    /// Panics if `positions.len() != normals.len()`: the source's `pa.count`/
    /// `na.count` are always equal because `computeVertexNormals` derives one
    /// normal per position, a precondition this constructor makes explicit
    /// rather than silently indexing past a short array.
    pub fn new(positions: Vec<[f32; 3]>, normals: Vec<[f32; 3]>) -> Self {
        assert_eq!(
            positions.len(),
            normals.len(),
            "MaskGeometry: positions and normals must be the same length"
        );
        let masks = vec![[0.0, 0.0, 0.0]; positions.len()];
        Self {
            positions,
            normals,
            masks,
        }
    }
}

/// Rewrite a geometry's mask attribute from a per-vertex callback. Local space.
///
/// Ported from `util.js:205-227` (`paintMasks`). The source lazily computes
/// normals and lazily allocates a zeroed `color` attribute if the geometry
/// doesn't have them yet; both are geometry-construction concerns owned by
/// [`MaskGeometry::new`] here (see the module doc comment for why), so this
/// function is exactly the source's per-vertex loop: read the vertex's current
/// mask into `out`, hand the caller `(x, y, z, nx, ny, nz, out, i)`, write
/// `out` back. Passing `&mut geo.masks[i]` directly as `out` is the same
/// read-then-mutate-in-place shape as the source's `out[0] = ca.getX(i); …
/// fn(...); ca.setXYZ(i, out[0], out[1], out[2])` — there is no separate
/// scratch array to shuttle through because Rust can mutate the slot in place.
pub fn paint_masks<F>(geo: &mut MaskGeometry, mut paint: F)
where
    F: FnMut(f32, f32, f32, f32, f32, f32, &mut [f32; 3], usize),
{
    for i in 0..geo.positions.len() {
        let [px, py, pz] = geo.positions[i];
        let [nx, ny, nz] = geo.normals[i];
        paint(px, py, pz, nx, ny, nz, &mut geo.masks[i], i);
    }
}

/// The source's defaulted `fillMasks(geo, w = 0, g = 0, a = 0)` (`util.js:230`).
/// Rust has no default arguments; call sites that want the source's default
/// pass `(0.0, 0.0, 0.0)` explicitly.
pub const FILL_MASKS_DEFAULT: [f32; 3] = [0.0, 0.0, 0.0];

/// Uniform mask fill — the cheap path for props that don't need spatial
/// variation. Ported from `util.js:230-240` (`fillMasks`).
pub fn fill_masks(geo: &mut MaskGeometry, w: f32, g: f32, a: f32) {
    geo.masks.iter_mut().for_each(|m| *m = [w, g, a]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo3() -> MaskGeometry {
        MaskGeometry::new(
            vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [-1.0, 0.5, 0.25]],
            vec![[0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0]],
        )
    }

    #[test]
    fn new_zeroes_the_mask_column_and_matches_position_count() {
        let geo = geo3();
        assert_eq!(geo.masks, vec![[0.0, 0.0, 0.0]; 3]);
        assert_eq!(geo.positions.len(), geo.normals.len());
    }

    #[test]
    #[should_panic(expected = "same length")]
    fn new_panics_on_mismatched_lengths() {
        MaskGeometry::new(vec![[0.0, 0.0, 0.0]], vec![]);
    }

    #[test]
    fn fill_masks_writes_the_same_triple_into_every_vertex() {
        let mut geo = geo3();
        fill_masks(&mut geo, 0.2, 0.5, 0.9);
        assert_eq!(geo.masks, vec![[0.2, 0.5, 0.9]; 3]);

        // The source's defaulted (0, 0, 0) call, named explicitly.
        fill_masks(
            &mut geo,
            FILL_MASKS_DEFAULT[0],
            FILL_MASKS_DEFAULT[1],
            FILL_MASKS_DEFAULT[2],
        );
        assert_eq!(geo.masks, vec![[0.0, 0.0, 0.0]; 3]);
    }

    #[test]
    fn paint_masks_reads_the_current_mask_and_writes_the_callback_result() {
        let mut geo = geo3();
        fill_masks(&mut geo, 0.1, 0.2, 0.3);

        // The classic weathering shape from the source's callers: read `out`
        // (the current mask, seeded by fill_masks above), combine it with
        // position/normal, write back — proves paint_masks hands the callback
        // the *existing* mask value rather than a fresh zeroed one.
        paint_masks(&mut geo, |_x, y, _z, _nx, ny, _nz, out, _i| {
            out[0] += 0.05;
            out[1] = (y * ny).max(out[1]);
        });

        assert_eq!(geo.masks[0], [0.15, 0.2, 0.3]); // y=0 * ny=1 = 0 -> max(0, 0.2) = 0.2
        assert_eq!(geo.masks[1], [0.15, 2.0, 0.3]); // y=2 * ny=1 = 2 -> max(2, 0.2) = 2
        assert_eq!(geo.masks[2], [0.15, 0.2, 0.3]); // y=0.5 * ny=-1 = -0.5 -> max(-0.5, 0.2) = 0.2
    }

    #[test]
    fn paint_masks_receives_position_normal_and_index_in_the_documented_order() {
        let mut geo = geo3();
        let mut seen = Vec::new();
        paint_masks(&mut geo, |x, y, z, nx, ny, nz, _out, i| {
            seen.push((i, x, y, z, nx, ny, nz));
        });
        assert_eq!(
            seen,
            vec![
                (0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0),
                (1, 1.0, 2.0, 3.0, 0.0, 1.0, 0.0),
                (2, -1.0, 0.5, 0.25, 0.0, -1.0, 0.0),
            ]
        );
    }

    #[test]
    fn paint_masks_on_an_empty_geometry_is_a_no_op() {
        let mut geo = MaskGeometry::default();
        let mut calls = 0;
        paint_masks(&mut geo, |_, _, _, _, _, _, _, _| calls += 1);
        assert_eq!(calls, 0);
    }
}
