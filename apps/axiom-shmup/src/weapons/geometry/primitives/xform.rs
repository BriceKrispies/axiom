//! Local `translate`/`rotateX`/`rotateZ`/`scale` helpers for the primitive
//! builders — the direct `THREE.BufferGeometry` calls `geometry.js` makes on
//! a just-built geometry (`g.rotateX(Math.PI / 2)`, `g.translate(x, y, z)`,
//! `cell.scale(1, 1, 0.55)`, ...) before handing it back.
//!
//! These reuse [`Geo::apply`] (the normal-matrix-correct point/direction
//! transform `Assembly::add` also uses) rather than hand-rolling a second
//! transform path: a `Mat4::translation`/`::scale`/`::from_quaternion`
//! reproduces exactly what `BufferGeometry.translate`/`rotateX`/`rotateZ`/
//! `scale` do (`three/src/core/BufferGeometry.js:425-519`, each one
//! `applyMatrix4` of the named matrix), so building that same matrix and
//! calling the shared `apply` is the faithful port, not a shortcut.

use axiom_math::{Mat4, Quat, Vec3};

use super::super::Geo;

/// `BufferGeometry.translate(x, y, z)`.
pub(super) fn translate(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::translation(Vec3::new(x, y, z)));
}

/// `BufferGeometry.rotateX(angle)`.
pub(super) fn rotate_x(g: &mut Geo, angle: f32) {
    let q = Quat::from_axis_angle(Vec3::UNIT_X, angle).expect("Vec3::UNIT_X is nonzero");
    g.apply(&Mat4::from_quaternion(q));
}

/// `BufferGeometry.rotateZ(angle)`.
pub(super) fn rotate_z(g: &mut Geo, angle: f32) {
    let q = Quat::from_axis_angle(Vec3::UNIT_Z, angle).expect("Vec3::UNIT_Z is nonzero");
    g.apply(&Mat4::from_quaternion(q));
}

/// `BufferGeometry.scale(x, y, z)`.
pub(super) fn scale(g: &mut Geo, x: f32, y: f32, z: f32) {
    g.apply(&Mat4::scale(Vec3::new(x, y, z)));
}
