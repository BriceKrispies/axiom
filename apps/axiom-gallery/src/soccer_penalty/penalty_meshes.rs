//! App-local low-poly **mesh geometry** for the penalty diorama's fidelity pass.
//!
//! The diorama's object model is shape-and-size data (see [`crate::soccer_penalty::penalty_scene`]);
//! the engine's per-frame authoring closure can only name the catalog meshes
//! (cube/sphere/plane), which is why the scene historically rendered as a stack
//! of axis-aligned boxes. This module builds real [`MeshData`] (positions,
//! normals, UVs, indices) for the rounder low-poly forms the actors need —
//! smooth spheres, capsule limbs — so the meshed render path
//! ([`crate::soccer_penalty::penalty_render_meshed`]) can spawn genuine geometry
//! through `RunningApp::add_mesh_data` instead of boxes.
//!
//! TEMPORARY APP GLUE, like [`crate::soccer_penalty::low_poly_assets`]: Axiom has
//! no soccer/character mesh-asset module, so these generators live in the app.
//! Each returns a unit-extent mesh (fits a 1×1×1 box, centered at the origin);
//! the render path scales it per object. Apps are outside the Branchless Law, so
//! these use ordinary loops.

use axiom::prelude::{MeshData, Vec2, Vec3};
use std::f32::consts::PI;

/// A unit cube (extent 1, centered at the origin) with per-face normals — the
/// structural primitive (posts, crossbar, stadium wall, crowd cards, ad boards,
/// thin line/quad slabs when scaled flat).
pub fn unit_cube() -> MeshData {
    // (face normal, four CCW corners viewed from outside)
    let faces: [(Vec3, [Vec3; 4]); 6] = [
        (Vec3::new(0.0, 0.0, 1.0), [c(-1, -1, 1), c(1, -1, 1), c(1, 1, 1), c(-1, 1, 1)]),
        (Vec3::new(0.0, 0.0, -1.0), [c(1, -1, -1), c(-1, -1, -1), c(-1, 1, -1), c(1, 1, -1)]),
        (Vec3::new(1.0, 0.0, 0.0), [c(1, -1, 1), c(1, -1, -1), c(1, 1, -1), c(1, 1, 1)]),
        (Vec3::new(-1.0, 0.0, 0.0), [c(-1, -1, -1), c(-1, -1, 1), c(-1, 1, 1), c(-1, 1, -1)]),
        (Vec3::new(0.0, 1.0, 0.0), [c(-1, 1, 1), c(1, 1, 1), c(1, 1, -1), c(-1, 1, -1)]),
        (Vec3::new(0.0, -1.0, 0.0), [c(-1, -1, -1), c(1, -1, -1), c(1, -1, 1), c(-1, -1, 1)]),
    ];
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for (normal, corners) in faces {
        let base = positions.len() as u32;
        for (k, corner) in corners.iter().enumerate() {
            positions.push(*corner);
            normals.push(normal);
            uvs.push(Vec2::new((k == 1 || k == 2) as u32 as f32, (k >= 2) as u32 as f32));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    MeshData::new(positions, normals, uvs, indices)
}

/// A unit UV sphere (radius 0.5) — the ball, and the actors' heads/hands. When
/// scaled non-uniformly it becomes a smooth ellipsoid.
pub fn unit_sphere() -> MeshData {
    sphere_like(0.5, 0.0)
}

/// A unit capsule (radius 0.4, total height 1, long axis Y) — the actors' limbs
/// and torso. A sphere split at its equator and pushed apart by a short
/// cylinder, so a limb reads as a rounded tube instead of a box.
pub fn unit_capsule() -> MeshData {
    // radius 0.4 + half-cylinder 0.1 on each side = total height 1.
    sphere_like(0.4, 0.1)
}

/// A sphere of `radius` whose two hemispheres are pushed `push` apart along Y
/// (a cylinder of height `2*push` between them). `push = 0` is a plain sphere;
/// `push > 0` is a capsule. Total height is `2*(radius + push)`.
fn sphere_like(radius: f32, push: f32) -> MeshData {
    let stacks = 14usize;
    let slices = 18usize;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=stacks {
        let phi = PI * i as f32 / stacks as f32; // 0 (top) .. PI (bottom)
        let ny = phi.cos();
        let nr = phi.sin();
        // The northern hemisphere lifts by +push, the southern by -push, opening
        // a cylinder of height 2*push between them (push = 0 -> a plain sphere).
        let cy = radius * ny + push * ny.signum();
        for j in 0..=slices {
            let theta = 2.0 * PI * j as f32 / slices as f32;
            let nx = nr * theta.cos();
            let nz = nr * theta.sin();
            positions.push(Vec3::new(radius * nx, cy, radius * nz));
            normals.push(Vec3::new(nx, ny, nz));
            uvs.push(Vec2::new(j as f32 / slices as f32, i as f32 / stacks as f32));
        }
    }
    let ring = slices + 1;
    for i in 0..stacks {
        for j in 0..slices {
            let a = (i * ring + j) as u32;
            let b = a + ring as u32;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    MeshData::new(positions, normals, uvs, indices)
}

/// A corner of the unit cube from integer signs, at extent 0.5.
fn c(x: i32, y: i32, z: i32) -> Vec3 {
    Vec3::new(x as f32 * 0.5, y as f32 * 0.5, z as f32 * 0.5)
}
