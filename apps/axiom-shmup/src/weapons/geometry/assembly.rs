//! Ported from Claude-of-Duty `src/weapons/geometry.js:368-421` — the
//! `Assembly` class that buckets transformed geometry by material and merges
//! each bucket.
//!
//! **`BTreeMap`, not `HashMap`.** JS `Map` iterates in insertion order; a
//! Rust `HashMap` is randomised per-process, which would make the merged
//! output — and therefore its hash — differ between runs of the exact same
//! build. `BTreeMap` iterates in sorted key order, which is both
//! deterministic and, being a total order on `String`, independent of
//! insertion order too (a stronger guarantee than the source needs, but the
//! cheapest one that is unconditionally reproducible).
//!
//! This is app code (`apps/`), outside the Branchless Law — plain `if`/`for`
//! throughout.

use std::collections::BTreeMap;

use axiom_math::{Mat4, Quat, Vec3};

use super::geo::Geo;
use super::merge::merge_all;

/// One `Assembly.add`/`addMirrored` transform: `{ x,y,z, rx,ry,rz, sx,sy,sz }`
/// (`geometry.js:378`). `Default` matches the source's per-field `?? `
/// fallbacks: zero translation/rotation, unit scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xform {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rx: f32,
    pub ry: f32,
    pub rz: f32,
    pub sx: f32,
    pub sy: f32,
    pub sz: f32,
}

impl Default for Xform {
    fn default() -> Self {
        Xform {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
            sx: 1.0,
            sy: 1.0,
            sz: 1.0,
        }
    }
}

/// One named attachment point (`this.nodes.set(name, { pos, rot })`,
/// `geometry.js:405-408`) — the muzzle, sight axis, grip, and every other
/// point the animation rig drives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub pos: [f32; 3],
    pub rot: [f32; 3],
}

/// `class Assembly` (`geometry.js:368-420`): collects transformed geometry
/// per material bucket, then merges each bucket into one mesh at [`Assembly::build`].
pub struct Assembly {
    name: String,
    buckets: BTreeMap<String, Vec<Geo>>,
    nodes: BTreeMap<String, Node>,
}

impl Assembly {
    /// `constructor(name)` (`geometry.js:369-373`).
    pub fn new(name: &str) -> Self {
        Assembly {
            name: name.to_string(),
            buckets: BTreeMap::new(),
            nodes: BTreeMap::new(),
        }
    }

    /// `this.name` (`geometry.js:370`), read back — the source never reads
    /// it either, but it is a plain public field there and this is the Rust
    /// equivalent of that visibility.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `this.nodes` (`geometry.js:372`), the map [`Assembly::node`] fills —
    /// the animation rig's future read side. `build()` clears `buckets` but
    /// never `nodes` (`geometry.js:411-419`), matched here: this map
    /// outlives `build()`.
    pub fn nodes(&self) -> &BTreeMap<String, Node> {
        &self.nodes
    }

    /// `add(geo, mat, t = null)` (`geometry.js:380-396`).
    ///
    /// `t` composes translate x rotate x scale — Euler order **XYZ**
    /// (`_e.set(t.rx, t.ry, t.rz, 'XYZ')`, `geometry.js:384`) — exactly as
    /// `THREE.Matrix4.compose(pos, quat, scale)` does, then applies it and
    /// flips winding when the scale determinant is negative
    /// (`sx*sy*sz < 0`), which is what [`Assembly::add_mirrored`] relies on.
    ///
    /// The rotation is built by [`euler_xyz_quat`], not
    /// [`axiom_math::Quat::from_euler_xyz`] — that function composes in the
    /// *opposite* axis order (`qz * qy * qx`, a different Euler convention)
    /// and does not reproduce `THREE.Euler`'s `'XYZ'` order. See
    /// [`euler_xyz_quat`]'s doc for the golden capture that pins this.
    pub fn add(&mut self, geo: Geo, mat: &str, t: Option<Xform>) -> &mut Self {
        let mut g = geo;
        if let Some(t) = t {
            let translate = Mat4::translation(Vec3::new(t.x, t.y, t.z));
            let rotate = Mat4::from_quaternion(euler_xyz_quat(t.rx, t.ry, t.rz));
            let scale = Mat4::scale(Vec3::new(t.sx, t.sy, t.sz));
            let m = translate.multiply(rotate).multiply(scale);
            g.apply(&m);
            if t.sx * t.sy * t.sz < 0.0 {
                g.flip_winding();
            }
        }
        g.normalize_attributes();
        self.buckets.entry(mat.to_string()).or_default().push(g);
        self
    }

    /// `addMirrored(geo, mat, t)` (`geometry.js:399-403`): the same piece on
    /// both sides of the weapon — once as given, once with `x` and `sx`
    /// negated.
    pub fn add_mirrored(&mut self, geo: Geo, mat: &str, t: Xform) -> &mut Self {
        self.add(geo.clone(), mat, Some(t));
        let mirrored = Xform {
            x: -t.x,
            sx: -t.sx,
            ..t
        };
        self.add(geo, mat, Some(mirrored));
        self
    }

    /// `node(name, x, y, z, rx = 0, ry = 0, rz = 0)` (`geometry.js:405-408`).
    pub fn node(&mut self, name: &str, x: f32, y: f32, z: f32, rx: f32, ry: f32, rz: f32) -> &mut Self {
        self.nodes.insert(
            name.to_string(),
            Node {
                pos: [x, y, z],
                rot: [rx, ry, rz],
            },
        );
        self
    }

    /// `build()` (`geometry.js:411-419`): merges every bucket via
    /// `mergeAll`, keeping only the buckets that produced a geometry, and
    /// clears `buckets` (leaving `nodes` untouched).
    pub fn build(&mut self) -> BTreeMap<String, Geo> {
        let mut out = BTreeMap::new();
        for (mat, list) in std::mem::take(&mut self.buckets) {
            if let Some(merged) = merge_all(list) {
                out.insert(mat, merged);
            }
        }
        out
    }
}

/// Compose a unit rotation for Euler angles `(x, y, z)` in `THREE.Euler`'s
/// `'XYZ'` order — the same per-axis half-angle construction
/// [`axiom_math::Quat::from_euler_xyz`] uses internally, but composed as
/// `qx * qy * qz` rather than that function's `qz * qy * qx`.
///
/// This matters because the two orders are genuinely different rotations,
/// not the same rotation written two ways. Verified against a real
/// `three@0.180` `new THREE.Euler(0.3, -0.5, 0.7, 'XYZ')` /
/// `Quaternion.setFromEuler`, captured under Node:
/// `(x,y,z,w) = (0.052132410889547995, -0.2794438940784743,
/// 0.29377717233096856, 0.9126271389863014)`. `qx.multiply(qy).multiply(qz)`
/// for the same angles reproduces that value exactly; `qz.multiply(qy).multiply(qx)`
/// (`axiom_math`'s order) does not — see
/// `tests/weapons_geometry_port.rs::euler_xyz_matches_three_and_not_axiom_math_order`.
fn euler_xyz_quat(x: f32, y: f32, z: f32) -> Quat {
    let (hx, hy, hz) = (x * 0.5, y * 0.5, z * 0.5);
    let qx = Quat::new(hx.sin(), 0.0, 0.0, hx.cos());
    let qy = Quat::new(0.0, hy.sin(), 0.0, hy.cos());
    let qz = Quat::new(0.0, 0.0, hz.sin(), hz.cos());
    qx.multiply(qy).multiply(qz)
}
