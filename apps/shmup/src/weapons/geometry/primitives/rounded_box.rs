//! Ported from Claude-of-Duty `src/weapons/geometry.js:55-64` (`box`,
//! `blob`), plus the two Three.js geometry constructors they lean on:
//! `BoxGeometry` (`three/src/geometries/BoxGeometry.js`) and
//! `RoundedBoxGeometry` (`three/examples/jsm/geometries/
//! RoundedBoxGeometry.js`), both MIT licensed, Three.js authors.
//!
//! `RoundedBoxGeometry` starts from a unit `BoxGeometry` subdivided
//! `totalSegments` times per axis, expands it to non-indexed triangles, then
//! pushes every vertex outward along a per-corner rounded normal — the
//! chamfer this whole kit exists to guarantee (`geometry.js:8-13`).

use super::super::Geo;

/// `box(w, h, d, chamfer = 0.0012, seg = 1)` (`geometry.js:55-59`). `chamfer`
/// is the bevel radius in metres; `seg` 1 gives a hard 45-degree chamfer, 2-3
/// a rounded fillet. Falls back to a plain (unchamfered) indexed box when the
/// clamped radius is negligible (`r <= 1e-5`), exactly as the source does.
pub fn box_geo(w: f32, h: f32, d: f32, chamfer: f32, seg: u32) -> Geo {
    let r = chamfer.min(w.min(h).min(d) * 0.49);
    let mut g = if r <= 1e-5 {
        box_geometry(f64::from(w), f64::from(h), f64::from(d), 1, 1, 1)
    } else {
        rounded_box(w, h, d, seg, r)
    };
    g.normalize_attributes();
    g
}

/// `blob(w, h, d, radius = 0.006, seg = 3)` (`geometry.js:62-64`): a softly
/// rounded block, i.e. `box` with different defaults. Rust has no default
/// arguments, so the JS defaults (`radius = 0.006`, `seg = 3`) live here only
/// as documentation; callers pass them explicitly.
pub fn blob(w: f32, h: f32, d: f32, radius: f32, seg: u32) -> Geo {
    box_geo(w, h, d, radius, seg)
}

/// `new RoundedBoxGeometry(width, height, depth, segments, radius)`
/// (`RoundedBoxGeometry.js:65-192`).
fn rounded_box(width: f32, height: f32, depth: f32, segments: u32, radius: f32) -> Geo {
    // `radius = Math.min(width/2, height/2, depth/2, radius)`
    // (`RoundedBoxGeometry.js:72`) — re-clamped here even though `box_geo`
    // already clamped its own `r`, exactly mirroring the source's redundant
    // second clamp.
    let radius = radius.min(width / 2.0).min(height / 2.0).min(depth / 2.0);
    let total_segments = segments * 2 + 1;

    if total_segments == 1 {
        // Source quirk (`RoundedBoxGeometry.js:65-95`): `super(1, 1, 1,
        // totalSegments, totalSegments, totalSegments)` runs first, then the
        // constructor returns *before* the position remap when
        // `totalSegments === 1` (i.e. `segments == 0`) — the requested
        // width/height/depth are silently discarded and a unit box comes
        // back instead. Ported as-is, not fixed: the fix would change the
        // shape of every future caller that (mis)uses `seg = 0` expecting a
        // hard chamfer at the true size, same as upstream Three.js would.
        return box_geometry(1.0, 1.0, 1.0, 1, 1, 1);
    }

    let base = box_geometry(1.0, 1.0, 1.0, total_segments, total_segments, total_segments);
    let mut g = to_non_indexed(&base);

    let box_half = [
        f64::from(width) / 2.0 - f64::from(radius),
        f64::from(height) / 2.0 - f64::from(radius),
        f64::from(depth) / 2.0 - f64::from(radius),
    ];
    let half_segment_size = 0.5 / f64::from(total_segments);
    // `faceTris = positions.length / 6` (`RoundedBoxGeometry.js:115`) — the
    // component count of one of the six equal-sized cube faces (six faces
    // share the non-indexed component array evenly since every face uses the
    // same `totalSegments` subdivision).
    let face_tris = g.pos.len() / 6;
    let radius_f64 = f64::from(radius);

    (0..g.vert_count()).for_each(|vi| {
        let px = f64::from(g.pos[vi * 3]);
        let py = f64::from(g.pos[vi * 3 + 1]);
        let pz = f64::from(g.pos[vi * 3 + 2]);

        let nx0 = px - sign(px) * half_segment_size;
        let ny0 = py - sign(py) * half_segment_size;
        let nz0 = pz - sign(pz) * half_segment_size;
        let len = (nx0 * nx0 + ny0 * ny0 + nz0 * nz0).sqrt();
        let (nx, ny, nz) = if len > 0.0 {
            (nx0 / len, ny0 / len, nz0 / len)
        } else {
            (nx0, ny0, nz0)
        };

        g.pos[vi * 3] = (box_half[0] * sign(px) + nx * radius_f64) as f32;
        g.pos[vi * 3 + 1] = (box_half[1] * sign(py) + ny * radius_f64) as f32;
        g.pos[vi * 3 + 2] = (box_half[2] * sign(pz) + nz * radius_f64) as f32;

        g.normal[vi * 3] = nx as f32;
        g.normal[vi * 3 + 1] = ny as f32;
        g.normal[vi * 3 + 2] = nz as f32;

        let side = (vi * 3) / face_tris;
        let (u, v) = face_uv(
            side,
            [nx, ny, nz],
            radius_f64,
            f64::from(width),
            f64::from(height),
            f64::from(depth),
        );
        g.uv[vi * 2] = u;
        g.uv[vi * 2 + 1] = v;
    });

    g
}

/// `Math.sign`: `-1`/`0`/`1`, never `NaN` for the finite inputs this kit
/// only ever feeds it.
fn sign(x: f64) -> f64 {
    let positive = f64::from(i32::from(x > 0.0));
    let negative = f64::from(i32::from(x < 0.0));
    positive - negative
}

/// `getUv(faceDirVector, normal, uvAxis, projectionAxis, radius, sideLength)`
/// (`RoundedBoxGeometry.js:8-39`). `uv_axis`/`projection_axis` are `x=0`,
/// `y=1`, `z=2`, matching the source's string-indexed `Vector3` component
/// access.
fn get_uv(face_dir: [f64; 3], normal: [f64; 3], uv_axis: usize, projection_axis: usize, radius: f64, side_length: f64) -> f64 {
    let tot_arc_length = 2.0 * std::f64::consts::PI * radius / 4.0;
    let center_length = (side_length - 2.0 * radius).max(0.0);
    let half_arc = std::f64::consts::PI / 4.0;

    let mut temp_normal = normal;
    temp_normal[projection_axis] = 0.0;
    let tn_len = (temp_normal[0] * temp_normal[0] + temp_normal[1] * temp_normal[1] + temp_normal[2] * temp_normal[2]).sqrt();
    if tn_len > 0.0 {
        temp_normal[0] /= tn_len;
        temp_normal[1] /= tn_len;
        temp_normal[2] /= tn_len;
    }

    let arc_uv_ratio = 0.5 * tot_arc_length / (tot_arc_length + center_length);

    // `Vector3.angleTo(v)`: `acos(clamp(dot(a,b) / sqrt(|a|^2 * |b|^2), -1, 1))`.
    let dot = temp_normal[0] * face_dir[0] + temp_normal[1] * face_dir[1] + temp_normal[2] * face_dir[2];
    let a_len_sq = temp_normal[0] * temp_normal[0] + temp_normal[1] * temp_normal[1] + temp_normal[2] * temp_normal[2];
    let b_len_sq = face_dir[0] * face_dir[0] + face_dir[1] * face_dir[1] + face_dir[2] * face_dir[2];
    let denom = (a_len_sq * b_len_sq).sqrt();
    // `denom` is 0 only if `temp_normal` collapsed to zero (a degenerate
    // corner vertex never produced by this kit's real dimensions); guard it
    // the same way `Vector3.angleTo` implicitly does not (JS would divide by
    // zero and get `NaN`, then `acos(clamp(NaN,...))` stays `NaN`), so match
    // by producing the same `NaN` here rather than a fabricated fallback.
    let cos_angle = (dot / denom).clamp(-1.0, 1.0);
    let angle = cos_angle.acos();
    let arc_angle_ratio = 1.0 - angle / half_arc;

    if sign(temp_normal[uv_axis]) == 1.0 {
        arc_angle_ratio * arc_uv_ratio
    } else {
        let len_uv = center_length / (tot_arc_length + center_length);
        len_uv + arc_uv_ratio + arc_uv_ratio * (1.0 - arc_angle_ratio)
    }
}

/// The `switch (side)` UV block (`RoundedBoxGeometry.js:136-188`), `side`
/// being `floor(component_index / faceTris)` — `0..=5` for
/// right/left/top/bottom/front/back, the same order the six [`box_geometry`]
/// `build_plane` calls run in.
fn face_uv(side: usize, normal: [f64; 3], radius: f64, width: f64, height: f64, depth: f64) -> (f32, f32) {
    let (face_dir, u0, p0, len0, flip0, u1, p1, len1, flip1): (
        [f64; 3],
        usize,
        usize,
        f64,
        bool,
        usize,
        usize,
        f64,
        bool,
    ) = match side {
        0 => ([1.0, 0.0, 0.0], 2, 1, depth, false, 1, 2, height, true),
        1 => ([-1.0, 0.0, 0.0], 2, 1, depth, true, 1, 2, height, true),
        2 => ([0.0, 1.0, 0.0], 0, 2, width, true, 2, 0, depth, false),
        3 => ([0.0, -1.0, 0.0], 0, 2, width, true, 2, 0, depth, true),
        4 => ([0.0, 0.0, 1.0], 0, 1, width, true, 1, 0, height, true),
        _ => ([0.0, 0.0, -1.0], 0, 1, width, false, 1, 0, height, true),
    };
    let raw_u = get_uv(face_dir, normal, u0, p0, radius, len0);
    let raw_v = get_uv(face_dir, normal, u1, p1, radius, len1);
    let u = if flip0 { 1.0 - raw_u } else { raw_u };
    let v = if flip1 { 1.0 - raw_v } else { raw_v };
    (u as f32, v as f32)
}

/// `new THREE.BoxGeometry(width, height, depth, widthSegments,
/// heightSegments, depthSegments)` (`three/src/geometries/BoxGeometry.js`) —
/// indexed, six faces, each `buildPlane`'d independently. `RoundedBoxGeometry`
/// always calls this with equal per-axis segment counts
/// (`totalSegments,totalSegments,totalSegments`), and `box_geo`'s
/// unchamfered fallback calls it with `(1, 1, 1)`; the general per-axis
/// signature is kept anyway to mirror the source directly.
pub(super) fn box_geometry(width: f64, height: f64, depth: f64, width_seg: u32, height_seg: u32, depth_seg: u32) -> Geo {
    let mut pos = Vec::new();
    let mut normal = Vec::new();
    let mut uv = Vec::new();
    let mut index = Vec::new();
    let mut number_of_vertices = 0u32;

    // Axis indices: x=0, y=1, z=2, matching `BoxGeometry.js`'s
    // `vector['x'|'y'|'z']` component access as plain array indices.
    build_plane(&mut pos, &mut normal, &mut uv, &mut index, 2, 1, 0, -1.0, -1.0, depth, height, width, depth_seg, height_seg, &mut number_of_vertices); // px
    build_plane(&mut pos, &mut normal, &mut uv, &mut index, 2, 1, 0, 1.0, -1.0, depth, height, -width, depth_seg, height_seg, &mut number_of_vertices); // nx
    build_plane(&mut pos, &mut normal, &mut uv, &mut index, 0, 2, 1, 1.0, 1.0, width, depth, height, width_seg, depth_seg, &mut number_of_vertices); // py
    build_plane(&mut pos, &mut normal, &mut uv, &mut index, 0, 2, 1, 1.0, -1.0, width, depth, -height, width_seg, depth_seg, &mut number_of_vertices); // ny
    build_plane(&mut pos, &mut normal, &mut uv, &mut index, 0, 1, 2, 1.0, -1.0, width, height, depth, width_seg, height_seg, &mut number_of_vertices); // pz
    build_plane(&mut pos, &mut normal, &mut uv, &mut index, 0, 1, 2, -1.0, -1.0, width, height, -depth, width_seg, height_seg, &mut number_of_vertices); // nz

    Geo { pos, normal, uv, index }
}

/// `buildPlane(u, v, w, udir, vdir, width, height, depth, gridX, gridY,
/// materialIndex)` (`BoxGeometry.js:89-189`), minus the material-index
/// grouping (`Geo` has no group concept, and nothing in this kit reads it).
#[allow(clippy::too_many_arguments)]
fn build_plane(
    pos: &mut Vec<f32>,
    normal: &mut Vec<f32>,
    uv: &mut Vec<f32>,
    index: &mut Vec<u32>,
    u: usize,
    v: usize,
    w: usize,
    udir: f64,
    vdir: f64,
    width: f64,
    height: f64,
    depth: f64,
    grid_x: u32,
    grid_y: u32,
    number_of_vertices: &mut u32,
) {
    let segment_width = width / f64::from(grid_x);
    let segment_height = height / f64::from(grid_y);
    let width_half = width / 2.0;
    let height_half = height / 2.0;
    let depth_half = depth / 2.0;
    let grid_x1 = grid_x + 1;
    let grid_y1 = grid_y + 1;
    let mut vertex_counter: u32 = 0;

    (0..grid_y1).for_each(|iy| {
        let y = f64::from(iy) * segment_height - height_half;
        (0..grid_x1).for_each(|ix| {
            let x = f64::from(ix) * segment_width - width_half;

            let mut vector = [0.0f64; 3];
            vector[u] = x * udir;
            vector[v] = y * vdir;
            vector[w] = depth_half;
            pos.push(vector[0] as f32);
            pos.push(vector[1] as f32);
            pos.push(vector[2] as f32);

            let mut nvec = [0.0f64; 3];
            nvec[w] = if depth > 0.0 { 1.0 } else { -1.0 };
            normal.push(nvec[0] as f32);
            normal.push(nvec[1] as f32);
            normal.push(nvec[2] as f32);

            uv.push((f64::from(ix) / f64::from(grid_x)) as f32);
            uv.push((1.0 - f64::from(iy) / f64::from(grid_y)) as f32);

            vertex_counter += 1;
        });
    });

    (0..grid_y).for_each(|iy| {
        (0..grid_x).for_each(|ix| {
            let a = *number_of_vertices + ix + grid_x1 * iy;
            let b = *number_of_vertices + ix + grid_x1 * (iy + 1);
            let c = *number_of_vertices + (ix + 1) + grid_x1 * (iy + 1);
            let d = *number_of_vertices + (ix + 1) + grid_x1 * iy;
            index.extend_from_slice(&[a, b, d, b, c, d]);
        });
    });

    *number_of_vertices += vertex_counter;
}

/// `BufferGeometry.toNonIndexed()`, specialized to `Geo`'s fixed attribute
/// set — needed here (rather than reusing `merge`'s private copy) because
/// `RoundedBoxGeometry` always expands to non-indexed before its per-vertex
/// remap (`RoundedBoxGeometry.js:97-102`), independent of `mergeAll`.
fn to_non_indexed(g: &Geo) -> Geo {
    let mut pos = Vec::with_capacity(g.index.len() * 3);
    let mut normal = Vec::with_capacity(g.index.len() * 3);
    let mut uv = Vec::with_capacity(g.index.len() * 2);
    g.index.iter().for_each(|&i| {
        let i = i as usize;
        pos.extend_from_slice(&g.pos[i * 3..i * 3 + 3]);
        normal.extend_from_slice(&g.normal[i * 3..i * 3 + 3]);
        uv.extend_from_slice(&g.uv[i * 2..i * 2 + 2]);
    });
    Geo {
        pos,
        normal,
        uv,
        index: Vec::new(),
    }
}
