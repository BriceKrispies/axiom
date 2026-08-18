//! Ported from Claude-of-Duty `src/weapons/geometry.js:66-141` (`latheZ`,
//! `tubeZ`, `rodZ`), plus `THREE.LatheGeometry`
//! (`three/src/geometries/LatheGeometry.js`, MIT licensed, Three.js
//! authors), which `latheZ` wraps.

use super::xform;
use super::super::Geo;

/// Lathe around the Z axis. `profile` is a flat list of `[axialZ, radius]`
/// pairs from rear to front (or any order); radius `0` closes the form.
/// `seg` default `24`, `phi_start` default `0.0`, `phi_length` default
/// `TAU` (`geometry.js:70`).
///
/// `LatheGeometry` spins around `+Y`; a `+90`-degree rotation about `X` maps
/// `(r, a) -> (r, 0, a)`, so the axis becomes `+Z` and the axial coordinates
/// survive untouched (`geometry.js:76-78`).
pub fn lathe_z(profile: &[[f32; 2]], seg: u32, phi_start: f32, phi_length: f32) -> Geo {
    let pts: Vec<(f64, f64)> = profile
        .iter()
        .map(|p| (f64::from(p[1]).max(1e-5), f64::from(p[0])))
        .collect();
    let mut g = lathe_geometry(&pts, seg, f64::from(phi_start), f64::from(phi_length));
    xform::rotate_x(&mut g, std::f32::consts::FRAC_PI_2);
    g.normalize_attributes();
    g
}

/// Tube along Z with a real wall: outer surface, inner bore, and crowned
/// ends. `seg` default `24`, `crown` default `0.0006` (`geometry.js:106`).
pub fn tube_z(r_outer: f32, r_inner: f32, len: f32, seg: u32, crown: f32) -> Geo {
    let z0 = -len / 2.0;
    let z1 = len / 2.0;
    let c = crown.min((r_outer - r_inner) * 0.4);
    lathe_z(
        &[
            [z0 + c, r_inner],
            [z0, r_inner + c],
            [z0, r_outer - c],
            [z0 + c, r_outer],
            [z1 - c, r_outer],
            [z1, r_outer - c],
            [z1, r_inner + c],
            [z1 - c, r_inner],
        ],
        seg,
        0.0,
        std::f32::consts::TAU,
    )
}

/// Solid cylinder along Z with chamfered rims. `seg` default `20`, `chamfer`
/// default `0.0008` (`geometry.js:126`).
pub fn rod_z(r0: f32, r1: f32, len: f32, seg: u32, chamfer: f32) -> Geo {
    let z0 = -len / 2.0;
    let z1 = len / 2.0;
    let c = chamfer.min(len * 0.4).min(r0.min(r1) * 0.5);
    lathe_z(
        &[
            [z0, 0.0],
            [z0, r0 - c],
            [z0 + c, r0],
            [z1 - c, r1],
            [z1, r1 - c],
            [z1, 0.0],
        ],
        seg,
        0.0,
        std::f32::consts::TAU,
    )
}

/// `new THREE.LatheGeometry(points, segments, phiStart, phiLength)`
/// (`LatheGeometry.js:35-201`). `points` are `(radius, axial)` pairs
/// (`Vector2(x, y)` in the source, `x` = radius, `y` = the axial/height
/// coordinate).
fn lathe_geometry(points: &[(f64, f64)], segments: u32, phi_start: f64, phi_length: f64) -> Geo {
    let phi_length = phi_length.clamp(0.0, std::f64::consts::TAU);
    let n = points.len();

    // Pre-compute normals for the initial "meridian" (`LatheGeometry.js:82-132`).
    let mut init_normals: Vec<(f64, f64)> = Vec::with_capacity(n);
    let mut prev_normal = (0.0f64, 0.0f64);
    (0..n).for_each(|j| {
        if j == 0 {
            let dx = points[1].0 - points[0].0;
            let dy = points[1].1 - points[0].1;
            // `normal.z = dy * 0.0` in the source is always zero for finite
            // `dy` (the lathe profile is 2-D); kept as an explicit `0.0`
            // component (the third normal component is never used past this
            // point anyway — the lathe only ever needs the x/y parts).
            let (nx, ny) = (dy, -dx);
            prev_normal = (nx, ny);
            init_normals.push(normalize2(nx, ny));
        } else if j == n - 1 {
            // Source quirk, preserved: the last vertex reuses `prevNormal`
            // **without** normalizing it (`LatheGeometry.js:103-107` has no
            // `.normalize()` call, unlike every other branch) — the raw
            // `(dy, -dx)` edge vector from the second-to-last segment, not a
            // unit normal. `latheZ`'s tube/rod profiles keep adjacent
            // segment lengths close enough that this rarely reads as a
            // visible seam, but it is not unit length and this port does not
            // quietly fix it.
            init_normals.push(prev_normal);
        } else {
            let dx = points[j + 1].0 - points[j].0;
            let dy = points[j + 1].1 - points[j].1;
            let cur_normal = (dy, -dx);
            let summed = (cur_normal.0 + prev_normal.0, cur_normal.1 + prev_normal.1);
            init_normals.push(normalize2(summed.0, summed.1));
            prev_normal = cur_normal;
        }
    });

    let mut vertices: Vec<f32> = Vec::new();
    let mut uvs: Vec<f32> = Vec::new();
    let mut normals: Vec<f32> = Vec::new();
    let inverse_segments = 1.0 / f64::from(segments);

    (0..=segments).for_each(|i| {
        let phi = phi_start + f64::from(i) * inverse_segments * phi_length;
        let (sin_p, cos_p) = phi.sin_cos();
        (0..n).for_each(|j| {
            let (r, axial) = points[j];
            vertices.push((r * sin_p) as f32);
            vertices.push(axial as f32);
            vertices.push((r * cos_p) as f32);

            uvs.push((f64::from(i) / f64::from(segments)) as f32);
            uvs.push((j as f64 / n.saturating_sub(1) as f64) as f32);

            let (inx, iny) = init_normals[j];
            normals.push((inx * sin_p) as f32);
            normals.push(iny as f32);
            normals.push((inx * cos_p) as f32);
        });
    });

    let mut indices: Vec<u32> = Vec::new();
    let n_u32 = n as u32;
    (0..segments).for_each(|i| {
        (0..n_u32.saturating_sub(1)).for_each(|j| {
            let base = j + i * n_u32;
            let a = base;
            let b = base + n_u32;
            let c = base + n_u32 + 1;
            let d = base + 1;
            indices.extend_from_slice(&[a, b, d, c, d, b]);
        });
    });

    Geo {
        pos: vertices,
        normal: normals,
        uv: uvs,
        index: indices,
    }
}

fn normalize2(x: f64, y: f64) -> (f64, f64) {
    let len = (x * x + y * y).sqrt();
    if len > 0.0 {
        (x / len, y / len)
    } else {
        (x, y)
    }
}
