//! **The uv construction** — the `uvMode` dispatch of the runtime material
//! shader: how a fragment decides *where in the texture it is*.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js` — the `owAxisFrame` uv
//! lines (source ~185-199), the `MAIN_FRAGMENT` space selection and the
//! `OW_TRIPLANAR` / `OW_MESH_UV` / planar dispatch at the top of that chunk
//! (source ~253-345). The three modes the source's `DEFAULT_PARAMS.uvMode`
//! names:
//!
//! * **`planar`** — project on the world's (or object's) *dominant* axis. One
//!   fetch. A wall gets a wall's projection and a floor gets a floor's, chosen
//!   per fragment from the normal.
//! * **`triplanar`** — project on all three axes and blend, nine fetches in the
//!   full shader. No seam on a curved or diagonal surface, at three times the
//!   bandwidth.
//! * **`mesh`** — the interpolated parameterisation the mesh already carries.
//!
//! plus the three knobs every mode shares: `scale` (**metres per texture
//! tile**), `offset`, and `localSpace` (project in object space rather than
//! world space).
//!
//! ## What this layer owns, and what it does not
//!
//! It owns everything that produces a **`vec2` uv** and the **axis choice** that
//! feeds it: the space selection, the per-axis projection with its handedness
//! flips, the dominant-axis comparison chain, the triplanar blend weights, and
//! the tile transform.
//!
//! It does **not** own the projection *frames* — `owAxisFrame`'s `T`/`B`/`N`
//! basis vectors, `owOrthonormalise` and `owTangentFrame`. Those are the
//! `material_shader::frames` layer. This layer exports [`axiom_uv_axis_sign`]
//! (see [`UV_MODE_WGSL`]) because the frame basis is built from the same `s`
//! vector as the uv, and the two must agree by construction rather than by
//! coincidence.
//!
//! ## The traps, and what was done about each
//!
//! * **`scale` divides.** The source computes `1 / p.scale` **on the CPU**
//!   (`extendMaterial`, source ~794) and uploads it as `owTile.xy`, so the
//!   shader multiplies. [`tile_scale`] is that division, and it is a division —
//!   never a reciprocal folded into a later multiply. In `mesh` mode `scale` is
//!   a repeat count and is passed through undivided, which is the source's own
//!   ternary.
//! * **Grouping is the specification.** `uv * tile.xy + tile.zw` is written as
//!   the source writes it, per component, and never folded.
//! * **The sign is `step`, not `sign`.** `owAxisFrame` builds its handedness from
//!   `mix( vec3( -1.0 ), vec3( 1.0 ), step( 0.0, n ) )`. GLSL `step(edge, x)` is
//!   `x < edge ? 0 : 1`, so a **zero** component (either signed zero) selects
//!   `+1`. GLSL `sign` would have produced `0.0` there and `f32::signum` would
//!   have produced `-1.0` for `-0.0`; both are wrong, and an axis-aligned normal
//!   with a zero off-axis component is not a rare input, it is the common one.
//! * **Per-axis handedness.** Each projection flips exactly one component —
//!   `-p.z * s.x`, `-p.z * s.y`, `p.x * s.z` — so a texture is not mirrored on
//!   the two opposing faces of a box. The flips are transcribed literally; the
//!   `+X` face and the `-X` face read the same way round, which is only visible
//!   once there is text on the wall.
//! * **Two different tie rules.** The planar dominant-axis chain and the
//!   triplanar *detail*-plane choice are **not** the same comparison, and they
//!   disagree: at `|n| = (0.5, 0.5, 0.1)` the planar chain picks Y and the
//!   detail chain picks X. Both are transcribed as written rather than unified.
//!
//! ## Storage width
//!
//! Everything here is `f32` on both sides, matching the GPU, with one deliberate
//! exception: [`tile_scale`]'s reciprocal is evaluated in `f64` and rounded to
//! `f32`, because the source evaluates it in JavaScript (`f64`) and `three`
//! rounds it on upload. A 400k-sample search over `scale in 0.01..50` found no
//! value where that differs from an `f32` division, so the choice is not
//! observable — it is made because it is what the source does.

/// The uv construction, as WGSL.
///
/// Free functions taking explicit arguments — no globals, no assumed binding
/// index — so the orchestrator can compose them into `axiom_surface` and wire
/// `tile` from wherever the parameter block ends up putting it.
///
/// | entry point | signature |
/// |---|---|
/// | `axiom_uv_projection_pos` | `(object_pos: vec3<f32>, world_pos: vec3<f32>, local_space: f32) -> vec3<f32>` |
/// | `axiom_uv_projection_normal` | `(object_normal: vec3<f32>, world_normal: vec3<f32>, face_dir: f32, local_space: f32) -> vec3<f32>` |
/// | `axiom_uv_tile` | `(uv: vec2<f32>, tile: vec4<f32>) -> vec2<f32>` |
/// | `axiom_uv_axis_sign` | `(n: vec3<f32>) -> vec3<f32>` |
/// | `axiom_uv_axis_project` | `(p: vec3<f32>, n: vec3<f32>, axis: i32) -> vec2<f32>` |
/// | `axiom_uv_axis` | `(p: vec3<f32>, n: vec3<f32>, axis: i32, tile: vec4<f32>) -> vec2<f32>` |
/// | `axiom_uv_dominant_axis` | `(n: vec3<f32>) -> i32` |
/// | `axiom_uv_planar` | `(p: vec3<f32>, n: vec3<f32>, tile: vec4<f32>) -> vec2<f32>` |
/// | `axiom_uv_triplanar_weights` | `(n: vec3<f32>) -> vec3<f32>` |
/// | `axiom_uv_triplanar_detail_axis` | `(n: vec3<f32>) -> i32` |
///
/// `mesh` mode has no entry point of its own: it is exactly
/// `axiom_uv_tile(in.uv, tile)`, which is the source's
/// `vMapUv * owTile.xy + owTile.zw` verbatim. A second name for the same two
/// operations would be a shim, not a layer.
pub(crate) const UV_MODE_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// uv construction — Claude-of-Duty `materials/shader.js`, the `uvMode` dispatch
// (`owAxisFrame`'s uv, and the top of `MAIN_FRAGMENT`).
//
// `tile` is the source's `owTile`: .xy = tiles per metre (i.e. 1 / scale,
// divided on the CPU), .zw = offset. In `mesh` mode .xy is the repeat count.
// `local_space` is the source's `OW_OBJECT_SPACE` define, as a runtime flag:
// > 0.5 projects in object space, otherwise in world space.
// `face_dir` is the source's `owFaceDir`: +1 front-facing, -1 back-facing.
// ---------------------------------------------------------------------------

// `owP`: the position the projection is measured in.
//   #ifdef OW_OBJECT_SPACE  vec3 owP = vOwOPos;  #else  vec3 owP = vOwWPos;
fn axiom_uv_projection_pos(object_pos: vec3<f32>, world_pos: vec3<f32>, local_space: f32) -> vec3<f32> {
    return select(world_pos, object_pos, local_space > 0.5);
}

// `owNp`: the normal the axis choice is made from, flipped for a back face.
//   vec3 owNp = normalize( vOwONrm ) * owFaceDir;   (world: vOwWNrm)
fn axiom_uv_projection_normal(object_normal: vec3<f32>, world_normal: vec3<f32>, face_dir: f32, local_space: f32) -> vec3<f32> {
    return normalize(select(world_normal, object_normal, local_space > 0.5)) * face_dir;
}

// `f.uv = f.uv * owTile.xy + owTile.zw;`  — also the whole of `mesh` mode:
// `vec2 baseUv = vMapUv * owTile.xy + owTile.zw;`
fn axiom_uv_tile(uv: vec2<f32>, tile: vec4<f32>) -> vec2<f32> {
    return uv * tile.xy + tile.zw;
}

// `vec3 s = mix( vec3( -1.0 ), vec3( 1.0 ), step( 0.0, n ) );`
// step, NOT sign: a zero component selects +1.
fn axiom_uv_axis_sign(n: vec3<f32>) -> vec3<f32> {
    return mix(vec3<f32>(-1.0), vec3<f32>(1.0), step(vec3<f32>(0.0), n));
}

// The per-axis projection, before the tile transform. One component is flipped
// per axis so opposing faces are not mirror images of one another.
fn axiom_uv_axis_project(p: vec3<f32>, n: vec3<f32>, axis: i32) -> vec2<f32> {
    let s = axiom_uv_axis_sign(n);
    if ( axis == 0 ){
        return vec2<f32>(-p.z * s.x, p.y);
    } else if ( axis == 1 ){
        return vec2<f32>(p.x, -p.z * s.y);
    }
    return vec2<f32>(p.x * s.z, p.y);
}

// `owAxisFrame( p, n, axis ).uv` in full.
fn axiom_uv_axis(p: vec3<f32>, n: vec3<f32>, axis: i32, tile: vec4<f32>) -> vec2<f32> {
    return axiom_uv_tile(axiom_uv_axis_project(p, n, axis), tile);
}

// int axis = ( abs( owNp.x ) > abs( owNp.y ) )
//   ? ( ( abs( owNp.x ) > abs( owNp.z ) ) ? 0 : 2 )
//   : ( ( abs( owNp.y ) > abs( owNp.z ) ) ? 1 : 2 );
fn axiom_uv_dominant_axis(n: vec3<f32>) -> i32 {
    return select(
        select(2, 1, abs(n.y) > abs(n.z)),
        select(2, 0, abs(n.x) > abs(n.z)),
        abs(n.x) > abs(n.y),
    );
}

// `planar`: the dominant-axis projection.
fn axiom_uv_planar(p: vec3<f32>, n: vec3<f32>, tile: vec4<f32>) -> vec2<f32> {
    return axiom_uv_axis(p, n, axiom_uv_dominant_axis(n), tile);
}

// vec3 an = abs( owNp );
// vec3 w = pow( an, vec3( 5.0 ) );
// w /= max( w.x + w.y + w.z, 1e-4 );
//
// The exponent IS the look: 5.0 is a hard sharpening that keeps a flat face on
// one projection and confines the blend to the corners. A softer exponent is a
// different material, not a tidier one.
fn axiom_uv_triplanar_weights(n: vec3<f32>) -> vec3<f32> {
    let an = abs(n);
    let w = pow(an, vec3<f32>(5.0));
    return w / max(w.x + w.y + w.z, 1e-4);
}

// OwFrame fd = fz;
// if ( an.y > max( an.x, an.z ) ) fd = fy;
// else if ( an.x > an.z ) fd = fx;
//
// The dominant plane the triplanar path projects its DETAIL layer on (one extra
// fetch instead of three). Note this is a different comparison from
// `axiom_uv_dominant_axis` and they genuinely disagree on a tie.
fn axiom_uv_triplanar_detail_axis(n: vec3<f32>) -> i32 {
    let an = abs(n);
    var axis = 2;
    if ( an.y > max( an.x, an.z ) ){
        axis = 1;
    } else if ( an.x > an.z ){
        axis = 0;
    }
    return axis;
}
"#;

/// `DEFAULT_PARAMS.uvMode`: `'planar' | 'triplanar' | 'mesh'`.
///
/// The discriminants are the packing order the parameter block will use; only
/// [`Mesh`](UvMode::Mesh) changes what [`tile_scale`] does, because it is the
/// one mode where `scale` is a repeat count rather than metres per tile.
///
/// `Clone`/`Copy` and nothing else: a derive that no caller uses is a function
/// the coverage gate would rightly report as unreached.
#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum UvMode {
    /// Project on the dominant axis. The source's default.
    Planar = 0,
    /// Project on all three axes and blend.
    Triplanar = 1,
    /// Use the mesh's own parameterisation.
    Mesh = 2,
}

/// `owTile.xy` — the source's
/// `const tileScale = p.uvMode === 'mesh' ? p.scale : 1 / p.scale;`
///
/// **`scale` is metres per texture tile, so it divides.** For the two projected
/// modes this is `1 / scale`, evaluated as a division (JavaScript's `f64`
/// division, rounded to `f32` on upload — see the module header on storage
/// width). For `mesh` it is the repeat count, passed through.
pub(crate) fn tile_scale(mode: UvMode, scale: f32) -> f32 {
    [
        (1.0_f64 / f64::from(scale)) as f32,
        scale,
    ][usize::from(mode as u8 == UvMode::Mesh as u8)]
}

/// `owTile` in full: `(tileScale, tileScale, offset[0], offset[1])`.
pub(crate) fn tile(mode: UvMode, scale: f32, offset: [f32; 2]) -> [f32; 4] {
    let s = tile_scale(mode, scale);
    [s, s, offset[0], offset[1]]
}

/// GLSL `step(edge, x)`: `0.0` when `x < edge`, `1.0` otherwise. A *signed* zero
/// therefore yields `1.0`, which is the whole point at an axis-aligned normal.
fn glsl_step(edge: f32, x: f32) -> f32 {
    [0.0_f32, 1.0][usize::from(x >= edge)]
}

/// `owP` — the space the projection is measured in.
pub(crate) fn projection_pos(
    object_pos: [f32; 3],
    world_pos: [f32; 3],
    local_space: f32,
) -> [f32; 3] {
    [world_pos, object_pos][usize::from(local_space > 0.5)]
}

/// `owNp` — `normalize( n ) * owFaceDir`, in the selected space.
///
/// The length is GLSL's `length`, `sqrt( ( x*x + y*y ) + z*z )`, and the
/// normalise is a division by it. A GPU is free to use a reciprocal square root
/// instead; that difference is the hardware's and the parity tolerance carries
/// it.
pub(crate) fn projection_normal(
    object_normal: [f32; 3],
    world_normal: [f32; 3],
    face_dir: f32,
    local_space: f32,
) -> [f32; 3] {
    let n = [world_normal, object_normal][usize::from(local_space > 0.5)];
    let length = ((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]).sqrt();
    n.map(|c| (c / length) * face_dir)
}

/// `mix( vec3( -1.0 ), vec3( 1.0 ), step( 0.0, n ) )` — the per-component
/// handedness of the projection.
pub(crate) fn axis_sign(n: [f32; 3]) -> [f32; 3] {
    n.map(|c| {
        let t = glsl_step(0.0, c);
        (-1.0) * (1.0 - t) + 1.0 * t
    })
}

/// The per-axis projection before the tile transform. `axis` is `0`/`1`/`2`.
pub(crate) fn axis_project(p: [f32; 3], n: [f32; 3], axis: usize) -> [f32; 2] {
    let s = axis_sign(n);
    [
        [-p[2] * s[0], p[1]],
        [p[0], -p[2] * s[1]],
        [p[0] * s[2], p[1]],
    ][axis]
}

/// `uv * owTile.xy + owTile.zw` — the tile transform, and the whole of `mesh`
/// mode.
pub(crate) fn tile_uv(uv: [f32; 2], tile: [f32; 4]) -> [f32; 2] {
    [uv[0] * tile[0] + tile[2], uv[1] * tile[1] + tile[3]]
}

/// `owAxisFrame( p, n, axis ).uv`.
pub(crate) fn axis_uv(p: [f32; 3], n: [f32; 3], axis: usize, tile: [f32; 4]) -> [f32; 2] {
    tile_uv(axis_project(p, n, axis), tile)
}

/// The `planar` mode's axis: the largest normal component, with the source's
/// exact tie behaviour — `x` must *beat* `y` to win, and must then *beat* `z`,
/// so an all-equal normal lands on `z` and an `x == y` tie lands on `y`.
pub(crate) fn dominant_axis(n: [f32; 3]) -> usize {
    let a = n.map(f32::abs);
    [
        [2, 1][usize::from(a[1] > a[2])],
        [2, 0][usize::from(a[0] > a[2])],
    ][usize::from(a[0] > a[1])]
}

/// `planar`: the dominant-axis projection, tiled.
pub(crate) fn planar_uv(p: [f32; 3], n: [f32; 3], tile: [f32; 4]) -> [f32; 2] {
    axis_uv(p, n, dominant_axis(n), tile)
}

/// The triplanar blend weights: `abs(n)` raised to the fifth, normalised by
/// their sum with a `1e-4` floor.
///
/// The exponent and the normalisation *are* the look — a different sharpening
/// is a different material — so both are transcribed literally, including that
/// the sum is `( x + y ) + z` and that the normalisation is a division.
pub(crate) fn triplanar_weights(n: [f32; 3]) -> [f32; 3] {
    let an = n.map(f32::abs);
    let w = an.map(|c| c.powf(5.0));
    let sum = (w[0] + w[1]) + w[2];
    let divisor = sum.max(1.0e-4);
    w.map(|c| c / divisor)
}

/// The plane the triplanar path projects its detail layer on. **Not** the same
/// comparison as [`dominant_axis`]: `y` must beat *both* others, then `x` must
/// beat `z`, otherwise `z`.
pub(crate) fn triplanar_detail_axis(n: [f32; 3]) -> usize {
    let a = n.map(f32::abs);
    [[2, 0][usize::from(a[0] > a[2])], 1][usize::from(a[1] > a[0].max(a[2]))]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry point the WGSL promises, by the name a sibling layer will
    /// call. A renamed function is a composition failure the orchestrator would
    /// otherwise meet at link time.
    #[test]
    fn the_wgsl_declares_every_entry_point_the_doc_comment_names() {
        [
            "fn axiom_uv_projection_pos(object_pos: vec3<f32>, world_pos: vec3<f32>, local_space: f32) -> vec3<f32>",
            "fn axiom_uv_projection_normal(object_normal: vec3<f32>, world_normal: vec3<f32>, face_dir: f32, local_space: f32) -> vec3<f32>",
            "fn axiom_uv_tile(uv: vec2<f32>, tile: vec4<f32>) -> vec2<f32>",
            "fn axiom_uv_axis_sign(n: vec3<f32>) -> vec3<f32>",
            "fn axiom_uv_axis_project(p: vec3<f32>, n: vec3<f32>, axis: i32) -> vec2<f32>",
            "fn axiom_uv_axis(p: vec3<f32>, n: vec3<f32>, axis: i32, tile: vec4<f32>) -> vec2<f32>",
            "fn axiom_uv_dominant_axis(n: vec3<f32>) -> i32",
            "fn axiom_uv_planar(p: vec3<f32>, n: vec3<f32>, tile: vec4<f32>) -> vec2<f32>",
            "fn axiom_uv_triplanar_weights(n: vec3<f32>) -> vec3<f32>",
            "fn axiom_uv_triplanar_detail_axis(n: vec3<f32>) -> i32",
        ]
        .iter()
        .for_each(|signature| {
            assert!(
                UV_MODE_WGSL.contains(signature),
                "the WGSL must declare `{signature}`"
            );
        });
        // The traps, pinned in the shader text itself: the exponent, the sum
        // floor, and the fact that the handedness comes from `step`.
        assert!(UV_MODE_WGSL.contains("pow(an, vec3<f32>(5.0))"));
        assert!(UV_MODE_WGSL.contains("max(w.x + w.y + w.z, 1e-4)"));
        assert!(UV_MODE_WGSL.contains("step(vec3<f32>(0.0), n)"));
        // `scale` divides on the CPU; the shader must only ever multiply by the
        // tile it was handed.
        assert!(UV_MODE_WGSL.contains("uv * tile.xy + tile.zw"));
    }

    #[test]
    fn scale_is_metres_per_tile_and_divides() {
        // The source's DEFAULT_PARAMS.scale.
        assert_eq!(tile_scale(UvMode::Planar, 2.0), 0.5);
        assert_eq!(tile_scale(UvMode::Triplanar, 4.0), 0.25);
        // A prop scale, where the reciprocal is not representable: the value is
        // the correctly-rounded division, not something a multiply produced.
        assert_eq!(tile_scale(UvMode::Planar, 0.55), 1.0 / 0.55_f32);
        assert_eq!(tile_scale(UvMode::Triplanar, 0.3), 1.0 / 0.3_f32);
        // Mesh mode treats `scale` as a repeat count and does NOT divide.
        assert_eq!(tile_scale(UvMode::Mesh, 2.0), 2.0);
        assert_eq!(tile_scale(UvMode::Mesh, 0.55), 0.55);
    }

    #[test]
    fn the_tile_vector_is_scale_scale_offset_offset() {
        assert_eq!(tile(UvMode::Planar, 2.0, [0.25, -0.5]), [0.5, 0.5, 0.25, -0.5]);
        assert_eq!(tile(UvMode::Mesh, 3.0, [0.0, 0.0]), [3.0, 3.0, 0.0, 0.0]);
    }

    /// The `step`-not-`sign` trap, at every zero the language distinguishes.
    #[test]
    fn a_zero_normal_component_takes_the_positive_handedness() {
        assert_eq!(axis_sign([0.0, -0.0, 1.0]), [1.0, 1.0, 1.0]);
        assert_eq!(axis_sign([-1.0, -0.0, 0.0]), [-1.0, 1.0, 1.0]);
        assert_eq!(axis_sign([-1.0, -1.0, -1.0]), [-1.0, -1.0, -1.0]);
        // `signum` would have said -1 for -0.0, and GLSL `sign` would have said
        // 0.0 for both zeros. Neither is what `step` does.
        assert_ne!(axis_sign([-0.0, -0.0, -0.0])[0], (-0.0_f32).signum());
    }

    /// Each axis flips exactly one component, so the two faces perpendicular to
    /// that axis are not mirror images of one another.
    #[test]
    fn each_axis_projection_flips_the_component_the_source_flips() {
        let p = [1.5, 2.5, 3.5];
        // +X and -X: the flipped lane is `-p.z * s.x`, so it changes sign with
        // the face while `p.y` does not.
        assert_eq!(axis_project(p, [1.0, 0.0, 0.0], 0), [-3.5, 2.5]);
        assert_eq!(axis_project(p, [-1.0, 0.0, 0.0], 0), [3.5, 2.5]);
        // +Y and -Y: `-p.z * s.y`.
        assert_eq!(axis_project(p, [0.0, 1.0, 0.0], 1), [1.5, -3.5]);
        assert_eq!(axis_project(p, [0.0, -1.0, 0.0], 1), [1.5, 3.5]);
        // +Z and -Z: `p.x * s.z`.
        assert_eq!(axis_project(p, [0.0, 0.0, 1.0], 2), [1.5, 2.5]);
        assert_eq!(axis_project(p, [0.0, 0.0, -1.0], 2), [-1.5, 2.5]);
    }

    #[test]
    fn the_tile_transform_scales_then_offsets_per_component() {
        assert_eq!(tile_uv([2.0, 4.0], [0.5, 0.25, 0.125, -1.0]), [1.125, 0.0]);
        // `axis_uv` is the projection followed by that transform.
        assert_eq!(
            axis_uv([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], 2, [0.5, 0.5, 1.0, 1.0]),
            [1.5, 2.0]
        );
    }

    /// The dominant-axis chain, including both of its tie rules.
    #[test]
    fn the_dominant_axis_chain_matches_the_sources_ties() {
        assert_eq!(dominant_axis([0.9, 0.1, 0.2]), 0);
        assert_eq!(dominant_axis([0.1, 0.9, 0.2]), 1);
        assert_eq!(dominant_axis([0.1, 0.2, 0.9]), 2);
        // Sign is irrelevant to the choice — only magnitude.
        assert_eq!(dominant_axis([-0.9, 0.1, 0.2]), 0);
        // x == y: `x > y` is false, so the chain falls to the y/z comparison
        // and y wins.
        assert_eq!(dominant_axis([0.5, 0.5, 0.1]), 1);
        // x == z with x > y: `x > z` is false, so z wins.
        assert_eq!(dominant_axis([0.5, 0.1, 0.5]), 2);
        // y == z with y > x: `y > z` is false, so z wins.
        assert_eq!(dominant_axis([0.1, 0.5, 0.5]), 2);
        // All equal: every comparison is false, so z.
        assert_eq!(dominant_axis([0.5, 0.5, 0.5]), 2);
    }

    #[test]
    fn planar_uv_is_the_dominant_axis_projection_tiled() {
        let p = [1.0, 2.0, 3.0];
        let unit = [1.0, 1.0, 0.0, 0.0];
        assert_eq!(planar_uv(p, [1.0, 0.0, 0.0], unit), axis_uv(p, [1.0, 0.0, 0.0], 0, unit));
        assert_eq!(planar_uv(p, [0.0, 1.0, 0.0], unit), axis_uv(p, [0.0, 1.0, 0.0], 1, unit));
        assert_eq!(planar_uv(p, [0.0, 0.0, 1.0], unit), axis_uv(p, [0.0, 0.0, 1.0], 2, unit));
    }

    /// The detail-plane chain is a *different* comparison from the planar one,
    /// and this is the input where they disagree. Unifying them would be a
    /// quiet retexturing of every triplanar surface.
    #[test]
    fn the_triplanar_detail_axis_is_not_the_planar_dominant_axis() {
        assert_eq!(triplanar_detail_axis([0.9, 0.1, 0.2]), 0);
        assert_eq!(triplanar_detail_axis([0.1, 0.9, 0.2]), 1);
        assert_eq!(triplanar_detail_axis([0.1, 0.2, 0.9]), 2);
        assert_eq!(triplanar_detail_axis([-0.1, -0.9, 0.2]), 1);
        // y ties the max: `y > max(x, z)` is false, so it falls through.
        assert_eq!(triplanar_detail_axis([0.5, 0.5, 0.1]), 0);
        assert_eq!(dominant_axis([0.5, 0.5, 0.1]), 1);
        // x == z: `x > z` is false, so z.
        assert_eq!(triplanar_detail_axis([0.5, 0.1, 0.5]), 2);
        assert_eq!(triplanar_detail_axis([0.5, 0.5, 0.5]), 2);
    }

    /// The weights: fifth power, normalised, with the `1e-4` floor.
    #[test]
    fn the_triplanar_weights_are_the_fifth_power_normalised() {
        // An axis-aligned normal collapses onto one projection entirely.
        assert_eq!(triplanar_weights([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(triplanar_weights([-1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        // A 45-degree normal splits evenly, and the exponent is what keeps the
        // split from reaching the third axis at all.
        let diagonal = triplanar_weights([0.707_106_8, 0.707_106_8, 0.0]);
        assert!((diagonal[0] - 0.5).abs() < 1.0e-6, "{diagonal:?}");
        assert!((diagonal[1] - 0.5).abs() < 1.0e-6, "{diagonal:?}");
        assert_eq!(diagonal[2], 0.0);
        // The exponent IS the look: at a normal 30 degrees off the X axis the
        // fifth power leaves the side projection at a few percent, where a
        // squared blend would leave a quarter. If this number moves, the
        // material changed.
        let off_axis = triplanar_weights([0.866_025_4, 0.5, 0.0]);
        assert!((off_axis[0] - 0.939_717_2).abs() < 1.0e-5, "{off_axis:?}");
        assert!((off_axis[1] - 0.060_282_9).abs() < 1.0e-5, "{off_axis:?}");
        // The weights are a partition of unity wherever the floor is not hit.
        assert!(((off_axis[0] + off_axis[1]) + off_axis[2] - 1.0).abs() < 1.0e-6);
        // The floor: a zero normal would otherwise divide by zero. `1e-4` makes
        // it a finite zero-weight instead of a NaN that would blacken the
        // fragment.
        assert_eq!(triplanar_weights([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        // And it is a floor, not a clamp of the result: a normal short enough
        // for the sum to fall under 1e-4 does not renormalise to unity.
        let tiny = triplanar_weights([0.1, 0.0, 0.0]);
        assert!(tiny[0] < 1.0, "the 1e-4 floor must bite: {tiny:?}");
        assert!((tiny[0] - 0.1_f32.powf(5.0) / 1.0e-4).abs() < 1.0e-6, "{tiny:?}");
    }

    /// `localSpace` is the whole of the space selection, on both lanes.
    #[test]
    fn local_space_selects_the_object_lanes_and_world_space_the_world_lanes() {
        let object = [1.0, 2.0, 3.0];
        let world = [-4.0, -5.0, -6.0];
        assert_eq!(projection_pos(object, world, 1.0), object);
        assert_eq!(projection_pos(object, world, 0.0), world);
        assert_eq!(projection_normal([0.0, 2.0, 0.0], [3.0, 0.0, 0.0], 1.0, 1.0), [0.0, 1.0, 0.0]);
        assert_eq!(projection_normal([0.0, 2.0, 0.0], [3.0, 0.0, 0.0], 1.0, 0.0), [1.0, 0.0, 0.0]);
    }

    /// `owFaceDir` flips the normal on a back face, which flips the projection's
    /// handedness with it — a back face reads the same way round as the front
    /// face opposite it.
    #[test]
    fn a_back_face_flips_the_normal_and_therefore_the_handedness() {
        let n = projection_normal([0.0, 0.0, 3.0], [0.0, 0.0, 0.0], -1.0, 1.0);
        assert_eq!(n, [-0.0, -0.0, -1.0]);
        // And here is the `step`-not-`sign` trap in the place it actually bites:
        // the two off-axis lanes are NEGATIVE zeros, and `step( 0.0, -0.0 )` is
        // 1.0 because `-0.0 < 0.0` is false. `f32::signum` would have made them
        // -1.0 and silently mirrored the frame's tangent basis on every
        // back-facing axis-aligned fragment.
        assert_eq!(axis_sign(n), [1.0, 1.0, -1.0]);
        assert_eq!(dominant_axis(n), 2);
        assert_eq!(axis_project([1.0, 2.0, 3.0], n, 2), [-1.0, 2.0]);
        // The normalise is a division by GLSL's `length`, so a non-unit input
        // comes back unit.
        let long = projection_normal([0.0, 0.0, 0.0], [3.0, 4.0, 0.0], 1.0, 0.0);
        assert_eq!(long, [0.6, 0.8, 0.0]);
    }
}

// ---------------------------------------------------------------------------
// CPU <-> GPU parity, on a real adapter. The shape `surface_program::parity`
// establishes: a real device or a loud failure, never a skip.
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;

    /// How many contexts one run compares, and the target's width.
    const SAMPLES: usize = 24;
    /// `vec4`s of context per sample: object pos + flag, world pos + face dir,
    /// object normal + mesh u, world normal + mesh v, tile.
    const SLOTS: usize = 5;
    /// `copy_texture_to_buffer` row alignment.
    const ROW_ALIGN: u32 = 256;

    /// **Bit-for-bit.** The space selection is a `select` and the dominant-axis
    /// choice is a chain of comparisons over values both sides hold identically:
    /// nothing is approximated and nothing is contracted, so the measured delta
    /// is exactly `0.0` and so is the budget. Any drift here at all is a real
    /// change, not the hardware.
    const EXACT_TOLERANCE: f32 = 0.0;

    /// `normalize`: the reference divides by GLSL's `length`, and a GPU is free
    /// to multiply by a reciprocal square root instead. Measured `5.96e-8` on a
    /// unit-magnitude output — half an ulp — so this budget is five times what
    /// the hardware needed and nowhere near [`SLACK_LIMIT`].
    const NORMALIZE_TOLERANCE: f32 = 3.0e-7;

    /// `pow(an, vec3(5.0))`: both sides *approximate* it, with different
    /// polynomials. Measured `1.19e-7` on weights that live in `0..=1` — one ulp
    /// — which is the answer to whether `pow` needed a transcendental-sized
    /// budget here. It did not.
    const WEIGHT_TOLERANCE: f32 = 6.0e-7;

    /// The uv lanes. The maths is a multiply and an add, but `uv * tile.xy +
    /// tile.zw` is exactly the shape a GPU may contract into a single-rounding
    /// `fma`, and this adapter does. Measured `7.63e-6`, which is `2^-17`: **one
    /// ulp** of a uv near 85, the largest this sweep produces (a `mesh`-mode
    /// repeat count of 12 over a position four metres out). Relative, that is
    /// `9e-8`; the constant is absolute because the comparison is.
    const UV_TOLERANCE: f32 = 3.0e-5;

    /// How far above the measured worst case a declared tolerance may sit
    /// before it stops being a measurement and becomes a hiding place. The same
    /// discipline, and the same constant, as
    /// `surface_program::parity_transcendental`.
    const SLACK_LIMIT: f32 = 10.0;

    /// How far the live measurement may drift above the committed one before
    /// the record is stale and has to be retaken.
    const DRIFT_LIMIT: f32 = 2.0;

    /// **The measurement, committed as data**, one worst absolute lane delta per
    /// entry point in [`ENTRY_POINTS`] order, taken on Vulkan (discrete) — the
    /// same adapter class `surface_program::parity_transcendental` records its
    /// numbers on.
    ///
    /// A table rather than a printed line on purpose: a number in a test log is
    /// read once and rots, and console output is banned in a module anyway.
    /// [`the_tolerances_are_not_looser_than_the_hardware_needs`] re-takes it on
    /// every run and fails if the record, or a tolerance, has stopped describing
    /// the hardware.
    const MEASURED_WORST_DELTA: [f32; 6] = [
        0.0,
        5.960_464_5e-8,
        1.192_092_9e-7,
        7.629_394_5e-6,
        7.629_394_5e-6,
        7.629_394_5e-6,
    ];

    /// The parity harness: one fragment entry point per four lanes of result.
    /// Each reads the same context and runs the same chain a composed
    /// `axiom_surface` would — space selection, then axis choice, then uv — so
    /// what is compared is the composition, not a restatement of one function.
    const HARNESS_WGSL: &str = r#"
struct UvContexts { items: array<vec4<f32>, 120> };
@group(0) @binding(0) var<uniform> contexts: UvContexts;

struct UvSample { p: vec3<f32>, n: vec3<f32>, tile: vec4<f32>, mesh: vec2<f32> };

fn uv_sample(index: u32) -> UvSample {
    let a = contexts.items[index * 5u + 0u];
    let b = contexts.items[index * 5u + 1u];
    let c = contexts.items[index * 5u + 2u];
    let d = contexts.items[index * 5u + 3u];
    let e = contexts.items[index * 5u + 4u];
    return UvSample(
        axiom_uv_projection_pos(a.xyz, b.xyz, a.w),
        axiom_uv_projection_normal(c.xyz, d.xyz, b.w, a.w),
        e,
        vec2<f32>(c.w, d.w),
    );
}

@vertex
fn uv_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn uv_pos_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = uv_sample(u32(position.x));
    return vec4<f32>(s.p, f32(axiom_uv_dominant_axis(s.n)));
}

@fragment
fn uv_normal_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = uv_sample(u32(position.x));
    return vec4<f32>(s.n, f32(axiom_uv_triplanar_detail_axis(s.n)));
}

@fragment
fn uv_weights_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = uv_sample(u32(position.x));
    return vec4<f32>(axiom_uv_triplanar_weights(s.n), axiom_uv_axis_sign(s.n).x);
}

@fragment
fn uv_planar_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = uv_sample(u32(position.x));
    return vec4<f32>(axiom_uv_planar(s.p, s.n, s.tile), axiom_uv_tile(s.mesh, s.tile));
}

@fragment
fn uv_axes01_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = uv_sample(u32(position.x));
    return vec4<f32>(axiom_uv_axis(s.p, s.n, 0, s.tile), axiom_uv_axis(s.p, s.n, 1, s.tile));
}

@fragment
fn uv_axis2_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = uv_sample(u32(position.x));
    return vec4<f32>(
        axiom_uv_axis(s.p, s.n, 2, s.tile),
        axiom_uv_axis_sign(s.n).y,
        axiom_uv_axis_sign(s.n).z,
    );
}
"#;

    /// One context: the eight inputs the chain takes.
    #[derive(Clone, Copy)]
    struct Context {
        object_pos: [f32; 3],
        world_pos: [f32; 3],
        object_normal: [f32; 3],
        world_normal: [f32; 3],
        mesh_uv: [f32; 2],
        tile: [f32; 4],
        face_dir: f32,
        local_space: f32,
    }

    /// The [`SAMPLES`] contexts, chosen for the places this layer is easy to get
    /// wrong: every axis dominant, every sign of every axis, exact ties on each
    /// pair and on all three, zero components (the `step`-not-`sign` case),
    /// non-unit normals, both face directions, both spaces, and a tile with a
    /// non-power-of-two scale and a non-zero offset.
    fn contexts() -> Vec<Context> {
        let normals: [[f32; 3]; SAMPLES] = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [0.5, 0.5, 0.1],
            [0.5, 0.1, 0.5],
            [0.1, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, -0.5],
            [0.707_106_8, 0.707_106_8, 0.0],
            [0.866_025_4, 0.5, 0.0],
            [-0.866_025_4, -0.5, 0.0],
            [0.267_261_2, 0.534_522_5, 0.801_783_7],
            [0.0, -0.6, 0.8],
            [3.0, 4.0, 0.0],
            [-2.0, 0.0, 0.0],
            [0.301, -0.902, 0.309],
            [0.9, -0.1, -0.42],
            [-0.12, 0.33, -0.93],
            [0.577_350_3, -0.577_350_3, 0.577_350_3],
            [0.05, 0.04, 0.03],
            [-0.71, 0.0, -0.71],
        ];
        normals
            .iter()
            .enumerate()
            .map(|(index, normal)| {
                let t = index as f32;
                Context {
                    object_pos: [t * 0.37 - 4.0, t * -0.53 + 2.5, t * 0.19 - 1.25],
                    world_pos: [t * -0.21 + 6.5, t * 0.44 - 3.0, t * 0.63 + 0.75],
                    object_normal: *normal,
                    world_normal: [normal[2], normal[0], normal[1]],
                    mesh_uv: [t * 0.041, 1.0 - t * 0.037],
                    tile: tile(
                        [UvMode::Planar, UvMode::Triplanar, UvMode::Mesh][index % 3],
                        [2.0, 0.55, 12.0][index % 3],
                        [t * 0.03 - 0.2, 0.17 - t * 0.011],
                    ),
                    face_dir: [1.0, -1.0][index % 2],
                    local_space: [0.0, 1.0][(index / 2) % 2],
                }
            })
            .collect()
    }

    /// The context uniform's bytes, in the order `uv_sample` unpacks them.
    fn context_bytes(contexts: &[Context]) -> Vec<u8> {
        let mut bytes: Vec<u8> = contexts
            .iter()
            .flat_map(|c| {
                [
                    c.object_pos[0],
                    c.object_pos[1],
                    c.object_pos[2],
                    c.local_space,
                    c.world_pos[0],
                    c.world_pos[1],
                    c.world_pos[2],
                    c.face_dir,
                    c.object_normal[0],
                    c.object_normal[1],
                    c.object_normal[2],
                    c.mesh_uv[0],
                    c.world_normal[0],
                    c.world_normal[1],
                    c.world_normal[2],
                    c.mesh_uv[1],
                    c.tile[0],
                    c.tile[1],
                    c.tile[2],
                    c.tile[3],
                ]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(SAMPLES * SLOTS * 16, 0);
        bytes
    }

    /// The CPU side of one context, as the six four-lane bundles the six
    /// fragment entry points return, in the same order.
    fn reference(c: &Context) -> [[f32; 4]; 6] {
        let p = projection_pos(c.object_pos, c.world_pos, c.local_space);
        let n = projection_normal(
            c.object_normal,
            c.world_normal,
            c.face_dir,
            c.local_space,
        );
        let s = axis_sign(n);
        let w = triplanar_weights(n);
        let planar = planar_uv(p, n, c.tile);
        let mesh = tile_uv(c.mesh_uv, c.tile);
        let a0 = axis_uv(p, n, 0, c.tile);
        let a1 = axis_uv(p, n, 1, c.tile);
        let a2 = axis_uv(p, n, 2, c.tile);
        [
            [p[0], p[1], p[2], dominant_axis(n) as f32],
            [n[0], n[1], n[2], triplanar_detail_axis(n) as f32],
            [w[0], w[1], w[2], s[0]],
            [planar[0], planar[1], mesh[0], mesh[1]],
            [a0[0], a0[1], a1[0], a1[1]],
            [a2[0], a2[1], s[1], s[2]],
        ]
    }

    /// The six entry points, and the budget each is held to. One tolerance per
    /// entry point rather than one for the file: a single shared number would be
    /// the loosest of the six, which is precisely the hiding place
    /// [`SLACK_LIMIT`] exists to prevent.
    const ENTRY_POINTS: [(&str, f32); 6] = [
        ("uv_pos_fs", EXACT_TOLERANCE),
        ("uv_normal_fs", NORMALIZE_TOLERANCE),
        ("uv_weights_fs", WEIGHT_TOLERANCE),
        ("uv_planar_fs", UV_TOLERANCE),
        ("uv_axes01_fs", UV_TOLERANCE),
        ("uv_axis2_fs", UV_TOLERANCE),
    ];

    /// A real adapter, or a loud failure.
    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            Gpu {
                device,
                queue,
                backend: gpu.backend,
            }
        }

        fn module(&self) -> wgpu::ShaderModule {
            // The error scope is the SHARED device's, so it is entered exclusively;
            // see `crate::test_gpu::validating`.
            let (module, failure) = crate::test_gpu::validating(&self.device, || {
                self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("axiom-uv-mode-parity-shader"),
                        source: wgpu::ShaderSource::Wgsl([UV_MODE_WGSL, HARNESS_WGSL].concat().into()),
                    })
            });
            assert!(
                failure.is_none(),
                "the uv-mode WGSL must compile: {failure:?}"
            );
            module
        }

        /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target —
        /// float, because a `Rgba8Unorm` target would quantise to 1/255, tens of
        /// thousands of times coarser than the tolerance.
        fn render(
            &self,
            module: &wgpu::ShaderModule,
            entry_point: &str,
            contexts: &[u8],
        ) -> Vec<[f32; 4]> {
            let layout =
                self.device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("axiom-uv-mode-parity-bgl"),
                        entries: &[wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        }],
                    });
            let buffer = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-uv-mode-parity-uniform"),
                    contents: contexts,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-uv-mode-parity-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-uv-mode-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-uv-mode-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("uv_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry_point),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba32Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-uv-mode-parity-target"),
                size: wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-uv-mode-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-uv-mode-parity-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..SAMPLES)
                .map(|sample| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = sample * 16 + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
                .collect()
        }
    }

    /// The worst absolute lane delta per entry point.
    fn worst_deltas(gpu: &Gpu) -> [f32; 6] {
        let all = contexts();
        let bytes = context_bytes(&all);
        let module = gpu.module();
        let expected: Vec<[[f32; 4]; 6]> = all.iter().map(reference).collect();
        let mut worst = [0.0_f32; 6];
        ENTRY_POINTS
            .iter()
            .enumerate()
            .for_each(|(bundle, (entry_point, _))| {
                let rendered = gpu.render(&module, entry_point, &bytes);
                worst[bundle] = expected
                    .iter()
                    .zip(rendered.iter())
                    .flat_map(|(cpu, actual)| {
                        [0_usize, 1, 2, 3].map(|lane| (cpu[bundle][lane] - actual[lane]).abs())
                    })
                    .fold(0.0_f32, f32::max);
            });
        worst
    }

    /// **The parity proof.** Every uv the layer can produce, and every axis
    /// choice, on a real adapter, against the CPU reference that is the semantic
    /// definition.
    #[test]
    fn the_uv_construction_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        let all = contexts();
        let bytes = context_bytes(&all);
        let module = gpu.module();
        let expected: Vec<[[f32; 4]; 6]> = all.iter().map(reference).collect();
        ENTRY_POINTS
            .iter()
            .enumerate()
            .for_each(|(bundle, (entry_point, tolerance))| {
                let rendered = gpu.render(&module, entry_point, &bytes);
                expected.iter().zip(rendered.iter()).enumerate().for_each(
                    |(sample, (cpu, actual))| {
                        (0..4).for_each(|lane| {
                            let delta = (cpu[bundle][lane] - actual[lane]).abs();
                            assert!(
                                delta <= *tolerance,
                                "{entry_point} disagrees at sample {sample} lane {lane}: \
                                 CPU {} vs GPU {} (delta {delta}, tolerance {tolerance})",
                                cpu[bundle][lane],
                                actual[lane]
                            );
                        });
                    },
                );
            });
        // An axis index is an integer travelling through a float target: it is
        // exact or the comparison above is meaningless, so say so directly.
        let axes: Vec<f32> = expected.iter().map(|bundle| bundle[0][3]).collect();
        assert!(
            axes.contains(&0.0) && axes.contains(&1.0) && axes.contains(&2.0),
            "the contexts must drive every dominant axis, or the sweep has a hole: {axes:?}"
        );
    }

    /// **The measurement, re-taken.** For each entry point it measures the live
    /// worst absolute lane delta and holds three relations, so neither the
    /// record nor the tolerance can quietly stop describing the hardware:
    ///
    /// 1. the live delta is within [`DRIFT_LIMIT`] of [`MEASURED_WORST_DELTA`];
    /// 2. the declared tolerance covers the live delta;
    /// 3. the declared tolerance is no more than [`SLACK_LIMIT`] above it —
    ///    being *too generous* fails here.
    #[test]
    fn the_tolerances_are_not_looser_than_the_hardware_needs() {
        let gpu = Gpu::acquire();
        assert_ne!(gpu.backend, wgpu::Backend::Noop);
        let worst = worst_deltas(&gpu);
        // Every failure below quotes the WHOLE run, so one red test hands over
        // the complete re-measurement instead of the first number that missed.
        let report = format!("full run on {:?}: {worst:?}", gpu.backend);
        ENTRY_POINTS
            .iter()
            .zip(worst.iter())
            .zip(MEASURED_WORST_DELTA.iter())
            .for_each(|(((entry_point, tolerance), measured), recorded)| {
                assert!(
                    *measured <= *recorded * DRIFT_LIMIT,
                    "{entry_point}'s worst CPU/GPU delta is now {measured:e} against a committed \
                     measurement of {recorded:e}. Re-measure and re-record it rather than \
                     widening a tolerance. {report}"
                );
                assert!(
                    *recorded <= *tolerance,
                    "{entry_point}'s recorded measurement {recorded:e} is outside its declared \
                     tolerance {tolerance:e}"
                );
                assert!(
                    *measured <= *tolerance,
                    "{entry_point}'s tolerance must cover the hardware: worst {measured:e} \
                     against {tolerance:e}"
                );
                assert!(
                    *tolerance <= *measured * SLACK_LIMIT,
                    "{entry_point}'s tolerance is {tolerance:e} against a measured worst case of \
                     {measured:e}; a budget more than {SLACK_LIMIT}x what the hardware needs \
                     hides the next regression"
                );
            });
    }
}
