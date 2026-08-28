//! Ported from Claude-of-Duty `src/weapons/geometry.js:144-148` (`dome`),
//! plus `THREE.SphereGeometry` (`three/src/geometries/SphereGeometry.js`,
//! MIT licensed, Three.js authors), which `dome` wraps.

use super::xform;
use super::super::Geo;

/// Sphere-ish detail blob (buttons, bosses, knuckle pads): a partial sphere
/// cut at `cut` of a hemisphere-to-full-sphere range, then rotated so its
/// pole sits on `+Z`. `seg` default `16`, `cut` default `0.6`
/// (`geometry.js:144`).
pub fn dome(r: f32, seg: u32, cut: f32) -> Geo {
    let height_segments = 4u32.max((f64::from(seg) * 0.5).round() as u32);
    let mut g = sphere_geometry(
        f64::from(r),
        seg,
        height_segments,
        0.0,
        std::f64::consts::TAU,
        0.0,
        std::f64::consts::PI * f64::from(cut),
    );
    xform::rotate_x(&mut g, std::f32::consts::FRAC_PI_2);
    g.normalize_attributes();
    g
}

/// `new THREE.SphereGeometry(radius, widthSegments, heightSegments,
/// phiStart, phiLength, thetaStart, thetaLength)`
/// (`SphereGeometry.js:30-147`).
///
/// `pub(crate)` (rather than private, [`dome`]'s original visibility) so
/// `world::props::mesh` can build `sackGeometry`'s full Lp-ball sphere and
/// `satDish`'s partial dish from the same faithful port, instead of a third
/// copy of `THREE.SphereGeometry`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sphere_geometry(
    radius: f64,
    width_segments: u32,
    height_segments: u32,
    phi_start: f64,
    phi_length: f64,
    theta_start: f64,
    theta_length: f64,
) -> Geo {
    let width_segments = width_segments.max(3);
    let height_segments = height_segments.max(2);
    let theta_end = (theta_start + theta_length).min(std::f64::consts::PI);

    let mut vertices: Vec<f32> = Vec::new();
    let mut normals: Vec<f32> = Vec::new();
    let mut uvs: Vec<f32> = Vec::new();
    let mut grid: Vec<Vec<u32>> = Vec::with_capacity((height_segments + 1) as usize);
    let mut index: u32 = 0;

    (0..=height_segments).for_each(|iy| {
        let mut row = Vec::with_capacity((width_segments + 1) as usize);
        let v = f64::from(iy) / f64::from(height_segments);

        let u_offset = if iy == 0 && theta_start == 0.0 {
            0.5 / f64::from(width_segments)
        } else if iy == height_segments && theta_end == std::f64::consts::PI {
            -0.5 / f64::from(width_segments)
        } else {
            0.0
        };

        (0..=width_segments).for_each(|ix| {
            let u = f64::from(ix) / f64::from(width_segments);

            let vx = -radius * (phi_start + u * phi_length).cos() * (theta_start + v * theta_length).sin();
            let vy = radius * (theta_start + v * theta_length).cos();
            let vz = radius * (phi_start + u * phi_length).sin() * (theta_start + v * theta_length).sin();
            vertices.push(vx as f32);
            vertices.push(vy as f32);
            vertices.push(vz as f32);

            let len = (vx * vx + vy * vy + vz * vz).sqrt();
            let (nx, ny, nz) = if len > 0.0 { (vx / len, vy / len, vz / len) } else { (vx, vy, vz) };
            normals.push(nx as f32);
            normals.push(ny as f32);
            normals.push(nz as f32);

            uvs.push((u + u_offset) as f32);
            uvs.push((1.0 - v) as f32);

            row.push(index);
            index += 1;
        });
        grid.push(row);
    });

    let mut indices: Vec<u32> = Vec::new();
    (0..height_segments).for_each(|iy| {
        (0..width_segments).for_each(|ix| {
            let a = grid[iy as usize][(ix + 1) as usize];
            let b = grid[iy as usize][ix as usize];
            let c = grid[(iy + 1) as usize][ix as usize];
            let d = grid[(iy + 1) as usize][(ix + 1) as usize];
            if iy != 0 || theta_start > 0.0 {
                indices.extend_from_slice(&[a, b, d]);
            }
            if iy != height_segments - 1 || theta_end < std::f64::consts::PI {
                indices.extend_from_slice(&[b, c, d]);
            }
        });
    });

    Geo {
        pos: vertices,
        normal: normals,
        uv: uvs,
        index: indices,
    }
}
