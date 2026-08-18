//! Ported from Claude-of-Duty `src/weapons/geometry.js:208-211` (`ring`),
//! plus `THREE.TorusGeometry` (`three/src/geometries/TorusGeometry.js`, MIT
//! licensed, Three.js authors), which `ring` wraps.

use super::super::Geo;

/// Torus in the XY plane (sling loops, trigger guard bows, QD rings).
/// `seg` default `20`, `rings` default `8`, `arc` default `TAU`
/// (`geometry.js:208`). The source calls `new THREE.TorusGeometry(radius,
/// thickness, rings, seg, arc)` — `TorusGeometry`'s own parameter order is
/// `(radius, tube, radialSegments, tubularSegments, arc)`, so `rings` here
/// is the *radial* segment count and `seg` is the *tubular* one.
pub fn ring(radius: f32, thickness: f32, seg: u32, rings: u32, arc: f32) -> Geo {
    let mut g = torus_geometry(f64::from(radius), f64::from(thickness), rings, seg, f64::from(arc));
    g.normalize_attributes();
    g
}

/// `new THREE.TorusGeometry(radius, tube, radialSegments, tubularSegments,
/// arc)` (`TorusGeometry.js:28-128`).
fn torus_geometry(radius: f64, tube: f64, radial_segments: u32, tubular_segments: u32, arc: f64) -> Geo {
    let mut vertices: Vec<f32> = Vec::new();
    let mut normals: Vec<f32> = Vec::new();
    let mut uvs: Vec<f32> = Vec::new();

    (0..=radial_segments).for_each(|j| {
        (0..=tubular_segments).for_each(|i| {
            let u = f64::from(i) / f64::from(tubular_segments) * arc;
            let v = f64::from(j) / f64::from(radial_segments) * std::f64::consts::TAU;

            let vx = (radius + tube * v.cos()) * u.cos();
            let vy = (radius + tube * v.cos()) * u.sin();
            let vz = tube * v.sin();
            vertices.push(vx as f32);
            vertices.push(vy as f32);
            vertices.push(vz as f32);

            let cx = radius * u.cos();
            let cy = radius * u.sin();
            let (dx, dy, dz) = (vx - cx, vy - cy, vz);
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            let (nx, ny, nz) = if len > 0.0 { (dx / len, dy / len, dz / len) } else { (dx, dy, dz) };
            normals.push(nx as f32);
            normals.push(ny as f32);
            normals.push(nz as f32);

            uvs.push((f64::from(i) / f64::from(tubular_segments)) as f32);
            uvs.push((f64::from(j) / f64::from(radial_segments)) as f32);
        });
    });

    let mut indices: Vec<u32> = Vec::new();
    (1..=radial_segments).for_each(|j| {
        (1..=tubular_segments).for_each(|i| {
            let a = (tubular_segments + 1) * j + i - 1;
            let b = (tubular_segments + 1) * (j - 1) + i - 1;
            let c = (tubular_segments + 1) * (j - 1) + i;
            let d = (tubular_segments + 1) * j + i;
            indices.extend_from_slice(&[a, b, d, b, c, d]);
        });
    });

    Geo {
        pos: vertices,
        normal: normals,
        uv: uvs,
        index: indices,
    }
}
