//! Which draws can actually land in the shadow map.
//!
//! The directional shadow pre-pass renders scene depth from the sun's point of
//! view into a fixed-size orthographic box that follows the camera
//! (`axiom_render_pipeline`'s `SHADOW_EXTENT`, a 40 m cube). The frame it is
//! rendering, meanwhile, can reach as far as its far plane — 1,650 m in a racing
//! course. Every draw beyond the box is clipped, but only *after* it has been
//! submitted and its vertices transformed: the pass was costing a full draw call
//! and a full vertex load per batch to produce nothing at all, and it ran over
//! **every** batch in the frame.
//!
//! On a browser that is the expensive kind of nothing. wgpu's WebGL2 path
//! re-specifies the whole vertex layout on every draw — measured at ~52 GL calls
//! each — so a shadow draw that contributes no texels still costs the same
//! submission as one that does. Roughly half of a frame's draw calls were this.
//!
//! So the pass asks first. Both functions here are pure geometry over plain
//! arrays: no GPU, no wgpu types, no frame state. That is deliberate — the rule
//! that decides whether an object is a shadow caster is exactly the kind of thing
//! that is impossible to debug once it is tangled into a render pass, and exactly
//! the kind of thing a native test can pin completely.

use axiom_math::{Aabb, Frustum, Mat4, Vec3};

/// The local-space bounds of an interleaved vertex stream whose first three
/// floats per vertex are the position, or `None` for an empty stream.
///
/// `stride` is the float count per vertex (the mesh streams here are 12: position,
/// normal, uv, colour). Computed once when a mesh is uploaded, because mesh
/// geometry never changes after that — the same reason the geometry itself is
/// uploaded once and never re-sent (see `axiom_render::RenderMesh`).
pub(crate) fn local_bounds(vertices: &[f32], stride: usize) -> Option<Aabb> {
    let stride = stride.max(1);
    let positions = vertices.chunks_exact(stride).map(|v| (v[0], v[1], v[2]));
    positions
        .fold(None, |acc: Option<([f32; 3], [f32; 3])>, (x, y, z)| {
            Some(acc.map_or(([x, y, z], [x, y, z]), |(lo, hi)| {
                (
                    [lo[0].min(x), lo[1].min(y), lo[2].min(z)],
                    [hi[0].max(x), hi[1].max(y), hi[2].max(z)],
                )
            }))
        })
        .and_then(|(lo, hi)| {
            Aabb::new(Vec3::new(lo[0], lo[1], lo[2]), Vec3::new(hi[0], hi[1], hi[2])).ok()
        })
}

/// Whether an instance of `bounds`, placed by the column-major `world` matrix,
/// can put anything inside the light's clip volume.
///
/// The world-space bounds are derived by the standard transformed-AABB identity
/// rather than by transforming eight corners: the centre maps through the matrix
/// as a point, and the extents map through the matrix's **absolute value**, which
/// costs one point transform and three dot products instead of eight transforms
/// and a min/max reduction. That matters because this runs per instance per
/// frame — hundreds of times — on a phone's main thread, and a cull that costs
/// more than the draw it removes is not a cull.
///
/// The result is conservative in the right direction: the transformed box is an
/// over-estimate for a rotated object, so this can keep a caster that would in
/// fact have been clipped. It can never *drop* one that would have contributed,
/// which is the only error that would be visible — a shadow disappearing.
pub(crate) fn casts_into(bounds: &Aabb, world: &[f32], light: &Frustum) -> bool {
    casts_into_with_margin(bounds, world, light, 0.0)
}

/// [`casts_into`], with the world-space bounds grown by `margin` first.
///
/// The margin exists because a caster does not have to be *inside* the light's
/// volume to darken a texel that is: the fragment stage samples outside a
/// receiver's own projected point by up to the sum of the whole-texel snap, the
/// normal offset, the PCSS blocker search and the PCF disc. A caster just outside
/// the box, whose shadow the filter would reach in for, is dropped by an
/// undilated test — and a dropped caster is a shadow that disappears, the one
/// error `casts_into` says is visible.
///
/// `crate::cascade::CascadeSet::cull_margin` is where the number comes from: 32
/// shadow texels, which is the source's own measured figure (it measured 2 texels
/// as *not* output-preserving). Growing the BOUNDS rather than the frustum is the
/// same test — a containment query does not care which side the slack is added
/// to — and it needs no dilation operation on `Frustum`, which has none.
///
/// The single-volume pass calls this with a zero margin, which adds an exact
/// `0.0` to each extent: the identity on every IEEE float, so that path's culling
/// decision is unchanged to the bit.
pub(crate) fn casts_into_with_margin(
    bounds: &Aabb,
    world: &[f32],
    light: &Frustum,
    margin: f32,
) -> bool {
    let m = |row: usize, col: usize| world[col * 4 + row];
    let centre = bounds.center();
    let extents = bounds.extents();
    let axis = |row: usize| {
        m(row, 0) * centre.x + m(row, 1) * centre.y + m(row, 2) * centre.z + m(row, 3)
    };
    let spread = |row: usize| {
        m(row, 0).abs() * extents.x + m(row, 1).abs() * extents.y + m(row, 2).abs() * extents.z
    };
    let world_centre = Vec3::new(axis(0), axis(1), axis(2));
    let world_extents = Vec3::new(
        spread(0) + margin,
        spread(1) + margin,
        spread(2) + margin,
    );
    Aabb::from_center_extents(world_centre, world_extents)
        .ok()
        .map_or(true, |b| light.intersects_aabb(&b))
}

/// The light's clip volume as a frustum, or `None` when the frame carries no
/// usable shadow camera (a degenerate or identity light matrix).
///
/// `None` is the signal to skip culling entirely and submit every batch, which is
/// exactly what the pass did before this existed: a frame whose shadow camera
/// cannot be inverted has no volume to test against, and guessing would drop
/// casters rather than keep them.
pub(crate) fn light_volume(light_view_proj: &[f32; 16]) -> Option<Frustum> {
    Frustum::from_view_projection(Mat4::from_cols_array(*light_view_proj)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 12-float interleaved vertex (position, normal, uv, colour) — only the
    /// first three floats are read.
    fn vertex(x: f32, y: f32, z: f32) -> [f32; 12] {
        [x, y, z, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]
    }

    fn stream(points: &[(f32, f32, f32)]) -> Vec<f32> {
        points
            .iter()
            .flat_map(|&(x, y, z)| vertex(x, y, z))
            .collect()
    }

    /// A light looking straight down at `focus` over a `half`-sized box, built
    /// the same way the render pipeline's shadow camera is.
    fn light_at(focus: Vec3, half: f32) -> Frustum {
        let eye = Vec3::new(focus.x, focus.y + 40.0, focus.z);
        let view = Mat4::look_at(eye, focus, Vec3::new(0.0, 0.0, 1.0)).expect("distinct eye/target");
        let proj = Mat4::orthographic(-half, half, -half, half, 0.1, 100.0).expect("valid box");
        Frustum::from_view_projection(proj.multiply(view)).expect("invertible")
    }

    /// A translation matrix, column-major, matching the layout `casts_into` reads.
    fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m[12] = x;
        m[13] = y;
        m[14] = z;
        m
    }

    #[test]
    fn local_bounds_spans_every_position_in_the_stream() {
        let b = local_bounds(&stream(&[(-1.0, -2.0, -3.0), (4.0, 5.0, 6.0), (0.0, 0.0, 0.0)]), 12)
            .expect("a non-empty stream has bounds");
        assert_eq!(b.center(), Vec3::new(1.5, 1.5, 1.5));
        assert_eq!(b.extents(), Vec3::new(2.5, 3.5, 4.5));
    }

    #[test]
    fn an_empty_or_degenerate_stream_has_no_bounds() {
        assert!(local_bounds(&[], 12).is_none());
        // Fewer floats than one whole vertex: `chunks_exact` yields nothing.
        assert!(local_bounds(&[0.0, 1.0, 2.0], 12).is_none());
        // A zero stride is floored to 1 rather than panicking on a zero-size chunk.
        assert!(local_bounds(&[], 0).is_none());
    }

    #[test]
    fn a_single_vertex_still_produces_usable_bounds() {
        let b = local_bounds(&stream(&[(2.0, 3.0, 4.0)]), 12).expect("one vertex is a point box");
        assert_eq!(b.center(), Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(b.extents(), Vec3::ZERO);
    }

    /// The case the whole module exists for: an object far down the course is not
    /// submitted to a shadow box that is following the camera.
    #[test]
    fn an_object_outside_the_light_box_is_culled() {
        let light = light_at(Vec3::ZERO, 20.0);
        let unit = local_bounds(&stream(&[(-0.5, -0.5, -0.5), (0.5, 0.5, 0.5)]), 12).unwrap();
        assert!(
            casts_into(&unit, &translation(0.0, 0.0, 0.0), &light),
            "an object at the focus casts"
        );
        assert!(
            !casts_into(&unit, &translation(0.0, 0.0, 1_600.0), &light),
            "a road chunk 1.6 km down the course cannot reach a 40 m box"
        );
    }

    #[test]
    fn an_object_straddling_the_boundary_is_kept() {
        let light = light_at(Vec3::ZERO, 20.0);
        // A long, wide slab centred well outside the box but reaching into it —
        // exactly a road chunk's shape. Culling on the centre alone would drop it.
        let slab = local_bounds(&stream(&[(-9.0, 0.0, -60.0), (9.0, 0.5, 60.0)]), 12).unwrap();
        assert!(
            casts_into(&slab, &translation(0.0, 0.0, 45.0), &light),
            "its near end is inside the box, so it must still be drawn"
        );
    }

    /// A rotated object's transformed bounds must not shrink below what it
    /// actually occupies, or a caster near the edge would vanish.
    #[test]
    fn rotation_grows_the_tested_bounds_rather_than_shrinking_them() {
        let light = light_at(Vec3::ZERO, 20.0);
        let unit = local_bounds(&stream(&[(-1.0, -1.0, -1.0), (1.0, 1.0, 1.0)]), 12).unwrap();
        // 45° about Y: the box's diagonal now reaches sqrt(2) along x/z.
        let c = std::f32::consts::FRAC_1_SQRT_2;
        let mut spun = [0.0f32; 16];
        spun[0] = c;
        spun[2] = -c;
        spun[5] = 1.0;
        spun[8] = c;
        spun[10] = c;
        spun[15] = 1.0;
        spun[12] = 21.2;
        // Placed just past the 20 m wall: an axis-aligned unit box would miss, but
        // the rotated one still reaches in, and the conservative test keeps it.
        assert!(casts_into(&unit, &spun, &light));
    }

    #[test]
    fn a_degenerate_light_matrix_yields_no_volume_so_nothing_is_culled() {
        // An all-zero matrix is not invertible: there is no volume to test, and
        // the caller's contract is to submit everything rather than guess.
        assert!(light_volume(&[0.0; 16]).is_none());
        // A real shadow camera does produce one.
        let view = Mat4::look_at(Vec3::new(0.0, 40.0, 0.0), Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0))
            .unwrap();
        let proj = Mat4::orthographic(-20.0, 20.0, -20.0, 20.0, 0.1, 100.0).unwrap();
        assert!(light_volume(&proj.multiply(view).as_cols_array()).is_some());
    }

    /// A world matrix that collapses an object to nothing cannot produce a valid
    /// box; the instance is kept rather than silently dropped.
    #[test]
    fn an_unbuildable_world_box_is_kept_rather_than_dropped() {
        let light = light_at(Vec3::ZERO, 20.0);
        let unit = local_bounds(&stream(&[(-0.5, -0.5, -0.5), (0.5, 0.5, 0.5)]), 12).unwrap();
        // A non-finite world transform makes the centre/extent arithmetic
        // non-finite, so `from_center_extents` rejects it and the fallback keeps
        // the draw — a missing shadow is visible, a redundant one is not.
        let mut broken = translation(0.0, 0.0, 0.0);
        broken[12] = f32::NAN;
        assert!(casts_into(&unit, &broken, &light));
    }
}
