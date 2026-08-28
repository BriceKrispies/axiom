//! Ported from Claude-of-Duty `src/weapons/hands.js:1-1163` — the complete
//! first-person arm rig: the glove/finger/thumb/sleeve mesh authoring
//! (`hands.js:51-459`), the two-bone analytic IK solved from the hand
//! (`Arm.solve`, `hands.js:999-1042`), the build-time contact solve that
//! clamps the support hand onto a handguard cylinder (`Arm.fitToCylinder`,
//! `hands.js:690-866`), the curvature/contact mask bakes
//! (`hands.js:897-969`) and the six authored pose tables (`HAND_POSES`,
//! `hands.js:1056-1163`).
//!
//! From the source's own header: two bones per arm, solved analytically from
//! the hand (which is the thing the animation drives — the hands are welded
//! to the weapon, the elbows follow). Hand-local space is `-Z` along the
//! fingers, `+Y` out of the back of the hand, `+X` toward the thumb (a right
//! hand; the left is mirrored).
//!
//! ## The five things this slice has to get right
//!
//! - **Bone lengths are cheated 10% long.** [`L_UPPER`]/[`L_FORE`] are
//!   330/300 mm, not the anatomical 300/272 — see their doc comment for the
//!   reach arithmetic that forces this.
//! - **The pole vector lives in rig space, not hand space.** [`Arm::pole`] is
//!   a fixed direction in the arm root's parent space (the viewmodel rig's
//!   space) — `hands.js:531-539`'s comment explains why hand space swings the
//!   support elbow through the near plane. [`Arm::solve`] never transforms
//!   it; the caller hands it a target already in that space.
//! - **Chirality is handled by mirroring the RIGHT arm.** The authored
//!   glove/finger geometry puts the thumb at `+X`, which makes it a *left*
//!   hand, so `handInner.scale.x` is `1` on the left arm and `-1` on the
//!   right (`hands.js:583-596`). See [`Arm::hand_mirror_x`].
//! - **Euler order is `'XYZ'`, implicitly.** `hands.js` never writes an order
//!   string: every rotation it sets is `Object3D.rotation`, a `THREE.Euler`
//!   whose default order is `'XYZ'`, composed as `qx*qy*qz`. That is *not*
//!   what `axiom_math::Quat::from_euler_xyz` computes (it composes
//!   `qz*qy*qx`), so every node rotation here goes through
//!   [`crate::weapons::rig_math::Q::from_euler_xyz`], which is a literal
//!   transcription of Three's `case 'XYZ'` branch. This is the port's
//!   "Euler order is a convention, not a spelling" trap, and it is live in
//!   [`Arm::fit_to_cylinder`]'s two-axis thumb-base scan, where an `'XYZ'`
//!   vs `'ZYX'` mix-up silently picks a different `(y, z)` pair.
//! - **`Float32Array` storage width is part of the algorithm.**
//!   [`Arm::bake_contact_ao`] allocates the `color` attribute as a
//!   `Float32Array` (`hands.js:949`) and its `Math.max` accumulate therefore
//!   reads back an `f32`-rounded value each time. [`HandMesh::color`] is
//!   `Vec<f32>` for exactly that reason, and the max is taken in `f64`
//!   before the `f32` store, matching the source's evaluation order.
//!
//! ## Scene graph
//!
//! The source builds a `THREE.Object3D` hierarchy. This port carries it as an
//! arena ([`Arm::nodes`]) with the source's child order preserved, because
//! `fitToCylinder` and `bakeContactAO` both *walk the real transform chain* —
//! that is the whole point of the contact solve (`hands.js:660-688`: the
//! analytic version ignored the 0.88 Y-scale on the finger capsules, the
//! -6 mm palmar MCP offset, and the per-finger fan-out, and was 8-14 mm out
//! on screen). A node's local matrix is `Matrix4.compose(position,
//! quaternion, scale)` and its world matrix is `parent.matrixWorld * local`,
//! transcribed in [`crate::weapons::rig_math::M4`].
//!
//! ## Deliberate divergences
//!
//! - **Materials are an enum, not objects.** The source takes a
//!   `{ glove, pad, seam, sleeve }` bag of `THREE.Material`s and classifies a
//!   mesh in `bakeSurfaceMasks` by identity comparison. There is no material
//!   library in this port yet (`materials.js` is a separate slice), so
//!   [`HandSurface`] names the four slots and [`Arm::bake_surface_masks`]
//!   dispatches on it. `materials.seam ?? materials.glove` resolves to `seam`
//!   in the real caller (`viewmodel.js:114-119` binds `glove_seam`), so the
//!   `??` fallback is unreachable there and is not modelled.
//! - **`dispose()` (`hands.js:1044-1048`) has no counterpart** — Rust frees
//!   the geometry when the [`Arm`] drops.
//! - **Render flags** (`castShadow`/`receiveShadow`/`frustumCulled`,
//!   `hands.js:641-647`) are carried as fields on [`HandMesh`] rather than
//!   dropped: they are the source's authored values and a renderer will need
//!   them.

use std::collections::BTreeMap;

use crate::weapons::geometry::primitives::{blob, box_geo, dome, lathe_z, ring};
use crate::weapons::geometry::{merge_all, Geo};
use crate::weapons::rig_math::{M4, Q, V3};

/// `hands.js:43`. See the long derivation there: a real 300/272 mm arm locks
/// the two-bone solve at 99.5% extension once the shoulder is far enough back
/// to stay behind the eye, which reads as a broomstick. Cheating both bones
/// 10% long buys 91% extension instead — visible bend, and the extra length
/// pushes the elbow further out of frame rather than into it.
pub const L_UPPER: f64 = 0.33;
/// `hands.js:44`.
pub const L_FORE: f64 = 0.3;

/// Cast a dimension computed in `f64` down to the `f32` the geometry kit
/// takes. The source multiplies its literals as JS numbers (`f64`) and Three
/// rounds once, on store into the `Float32Array`; computing the product in
/// `f64` and rounding once here is the faithful order, not rounding the
/// operands first.
fn dim(v: f64) -> f32 {
    v as f32
}

/* -------------------------------------------------------------------------- */
/*  geometry                                                                  */
/* -------------------------------------------------------------------------- */

/// `THREE.BufferGeometry.applyMatrix4(m)` (`core/BufferGeometry.js`) — what
/// `translate`/`scale`/`rotateX`/`rotateY`/`rotateZ` each reduce to.
///
/// **This computes in `f64` and stores `f32`, because that is exactly what
/// Three does**: `BufferAttribute.applyMatrix4` reads each component out of
/// the `Float32Array`, widens it to a JS number, transforms it against `f64`
/// matrix elements, and rounds once on store.
///
/// The geometry kit's own [`Geo::apply`] does the whole thing in `f32`
/// (`axiom_math::Mat4`) and builds its rotations from an `f32`
/// `Quat::from_axis_angle`. That is accurate enough for the primitives it was
/// written for — they are pinned at `1e-6` — but it is **not** faithful here,
/// and the difference is not rounding noise: an `f32` `rotateY(PI)` carries an
/// off-diagonal shear of `sin(f32::PI) = -8.74e-8` where Three's
/// `makeRotationY` carries `sin(PI_f64) = 1.22e-16`, a factor of 7e8. On a
/// 25 mm finger segment that displaced a vertex by 2.2 nm, and
/// [`Arm::bake_contact_ao`]'s smootherstep amplifies a position error by
/// ~109x (`0.7 * max(ds/dt) / radius`), which is how a nanometre became a
/// golden failure. See
/// `docs/work-manifests/shmup-port/notes/weapons-hands.md`.
///
/// The same defect exists one layer down in [`Geo::apply`] itself, and the
/// structurally right fix is to lift *that* to `f64`; this module cannot do
/// so without moving every other geometry slice's numbers mid-wave, so it is
/// flagged in the notes instead.
///
/// **Public on purpose.** This and the `m4_*` builders below are the only
/// place a defect of the `sin(f32::PI)` class is visible: 2.2 nm on a vertex
/// is three orders under the 1e-5 the merged-and-welded geometry needs, and
/// the primitive kit's own output already differs from Three's by up to one
/// f32 ULP on 99.97% of position components. So the golden pins this layer
/// directly — see `transform_helpers_are_bit_exact_against_three` in
/// `tests/weapons_hands_port.rs`. That is a real contract worth exporting,
/// not a widening of the API to reach an internal.
pub fn geo_apply(g: &mut Geo, m: M4) {
    for p in g.pos.chunks_exact_mut(3) {
        let v = V3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])).apply_matrix4(m);
        p[0] = v.x as f32;
        p[1] = v.y as f32;
        p[2] = v.z as f32;
    }
    // `if ( normal !== undefined )` — every primitive in this kit has run
    // `normalizeAttributes`, so the attribute is always there; an empty
    // buffer would simply iterate zero times.
    let nm = normal_matrix(m);
    for n in g.normal.chunks_exact_mut(3) {
        let v = V3::new(f64::from(n[0]), f64::from(n[1]), f64::from(n[2]));
        // `Vector3.applyNormalMatrix(m)` = `applyMatrix3(m).normalize()`.
        let r = V3::new(
            nm[0] * v.x + nm[3] * v.y + nm[6] * v.z,
            nm[1] * v.x + nm[4] * v.y + nm[7] * v.z,
            nm[2] * v.x + nm[5] * v.y + nm[8] * v.z,
        )
        .normalize();
        n[0] = r.x as f32;
        n[1] = r.y as f32;
        n[2] = r.z as f32;
    }
    // `uv` and `index` are untouched — `applyMatrix4` never names either.
}

/// `new THREE.Matrix3().getNormalMatrix(m4)` =
/// `setFromMatrix4(m4).invert().transpose()` (`math/Matrix3.js`), returned as
/// Three's own **column-major** 9-element layout.
///
/// Transcribed as those three steps rather than as the algebraically equal
/// `cofactor(A) / det(A)` shortcut [`Geo::apply`] uses: the two differ in the
/// last bits, and `invert`'s singular case (all zeros, not `None`) is part of
/// the contract.
pub fn normal_matrix(m: M4) -> [f64; 9] {
    let e = m.e;
    // `setFromMatrix4`: the upper-left 3x3, column-major.
    let (n11, n21, n31) = (e[0], e[1], e[2]);
    let (n12, n22, n32) = (e[4], e[5], e[6]);
    let (n13, n23, n33) = (e[8], e[9], e[10]);
    // `Matrix3.invert()`.
    let t11 = n33 * n22 - n32 * n23;
    let t12 = n32 * n13 - n33 * n12;
    let t13 = n23 * n12 - n22 * n13;
    let det = n11 * t11 + n21 * t12 + n31 * t13;
    if det == 0.0 {
        return [0.0; 9];
    }
    let di = 1.0 / det;
    let inv = [
        t11 * di,
        (n31 * n23 - n33 * n21) * di,
        (n32 * n21 - n31 * n22) * di,
        t12 * di,
        (n33 * n11 - n31 * n13) * di,
        (n31 * n12 - n32 * n11) * di,
        t13 * di,
        (n21 * n13 - n23 * n11) * di,
        (n22 * n11 - n21 * n12) * di,
    ];
    // `Matrix3.transpose()`: swap 1<->3, 2<->6, 5<->7.
    [
        inv[0], inv[3], inv[6], //
        inv[1], inv[4], inv[7], //
        inv[2], inv[5], inv[8],
    ]
}

/// `Matrix4.makeTranslation(x, y, z)`. `Matrix4.set` takes its arguments
/// row-major and stores column-major, so the translation lands in `e[12..15]`.
pub fn m4_translation(x: f64, y: f64, z: f64) -> M4 {
    let mut e = M4::IDENTITY.e;
    e[12] = x;
    e[13] = y;
    e[14] = z;
    M4 { e }
}

/// `Matrix4.makeScale(x, y, z)`.
pub fn m4_scale(x: f64, y: f64, z: f64) -> M4 {
    let mut e = M4::IDENTITY.e;
    e[0] = x;
    e[5] = y;
    e[10] = z;
    M4 { e }
}

/// `Matrix4.makeRotationX(theta)`: row-major `(1,0,0 / 0,c,-s / 0,s,c)`.
pub fn m4_rotation_x(theta: f64) -> M4 {
    let (c, s) = (theta.cos(), theta.sin());
    let mut e = M4::IDENTITY.e;
    e[5] = c;
    e[9] = -s;
    e[6] = s;
    e[10] = c;
    M4 { e }
}

/// `Matrix4.makeRotationY(theta)`: row-major `(c,0,s / 0,1,0 / -s,0,c)`.
pub fn m4_rotation_y(theta: f64) -> M4 {
    let (c, s) = (theta.cos(), theta.sin());
    let mut e = M4::IDENTITY.e;
    e[0] = c;
    e[8] = s;
    e[2] = -s;
    e[10] = c;
    M4 { e }
}

/// `Matrix4.makeRotationZ(theta)`: row-major `(c,-s,0 / s,c,0 / 0,0,1)`.
pub fn m4_rotation_z(theta: f64) -> M4 {
    let (c, s) = (theta.cos(), theta.sin());
    let mut e = M4::IDENTITY.e;
    e[0] = c;
    e[4] = -s;
    e[1] = s;
    e[5] = c;
    M4 { e }
}

/// `BufferGeometry.translate(x, y, z)`. The arguments stay `f64` — the source
/// passes JS numbers straight into `makeTranslation`, and narrowing them to
/// `f32` first would round the matrix before it ever touches a vertex.
fn geo_translate(g: &mut Geo, x: f64, y: f64, z: f64) {
    geo_apply(g, m4_translation(x, y, z));
}

/// `BufferGeometry.scale(x, y, z)`.
fn geo_scale(g: &mut Geo, x: f64, y: f64, z: f64) {
    geo_apply(g, m4_scale(x, y, z));
}

/// `BufferGeometry.rotateX(angle)`.
fn geo_rotate_x(g: &mut Geo, angle: f64) {
    geo_apply(g, m4_rotation_x(angle));
}

/// `BufferGeometry.rotateY(angle)`.
fn geo_rotate_y(g: &mut Geo, angle: f64) {
    geo_apply(g, m4_rotation_y(angle));
}

/// `BufferGeometry.rotateZ(angle)`.
fn geo_rotate_z(g: &mut Geo, angle: f64) {
    geo_apply(g, m4_rotation_z(angle));
}

/// One finger segment: a tapered, chamfered capsule with a joint crease.
/// `segment(len, r0, r1)`, `hands.js:51-69`.
fn segment(len: f64, r0: f64, r1: f64) -> Geo {
    let mut g = lathe_z(
        &[
            [0.0, 0.0],
            [0.0, dim(r0 * 0.86)],
            [dim(r0 * 0.5), dim(r0)],
            [dim(len * 0.42), dim(r0 * 0.99)],
            [dim(len * 0.55), dim(r1 * 1.04)],
            [dim(len - r1 * 0.7), dim(r1)],
            [dim(len - r1 * 0.2), dim(r1 * 0.8)],
            [dim(len), dim(r1 * 0.35)],
            [dim(len), 0.0],
        ],
        12,
        0.0,
        std::f32::consts::TAU,
    );
    geo_scale(&mut g, 1.0, 0.88, 1.0); // fingers are wider than they are deep
    geo_rotate_y(&mut g, std::f64::consts::PI); // extend along -Z
    g
}

/// Padded segment cover on the dorsal side (glove reinforcement).
/// `segmentPad(len, r)`, `hands.js:72-76`.
fn segment_pad(len: f64, r: f64) -> Geo {
    let mut g = blob(dim(r * 1.55), dim(r * 0.55), dim(len * 0.78), dim(r * 0.25), 2);
    geo_translate(&mut g, 0.0, r * 0.78, -len * 0.46);
    g
}

/// Stitched seam down the OUTBOARD side of a finger segment.
/// `segmentSeam(len, r0, r1, sx)`, `hands.js:91-96`. `sx` is `+1` outboard on
/// the thumb side, `-1` on the little-finger side. A 1.5 mm strip at 1.4x the
/// shell albedo survives to about 3 px, which is one pixel of separation per
/// finger — the only thing that keeps four fingers from merging into one
/// paddle at 40 px across the hand.
fn segment_seam(len: f64, r0: f64, r1: f64, sx: f64) -> Geo {
    let mut g = box_geo(0.0015, dim((r0 + r1) * 0.34), dim(len * 0.86), 0.0003, 1);
    // The finger capsule is scaled to 0.88 in Y, so its side wall sits at r in X.
    geo_translate(&mut g, sx * (r0 + r1) * 0.49, r0 * 0.1, -len * 0.47);
    g
}

/// Which of the four authored materials a mesh wears
/// (`{ glove, pad, seam, sleeve }`, `hands.js:102`/`153`/`295`/`337`). The
/// source compares `THREE.Material` identities in `bakeSurfaceMasks`; with no
/// material library ported yet this enum is the identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HandSurface {
    /// `materials.glove` — the leather shell (`glove` in `materials.js`).
    Glove,
    /// `materials.pad` — moulded TPR reinforcement (`glove_pad`).
    Pad,
    /// `materials.seam` — the stitched panel seam (`glove_seam`).
    Seam,
    /// `materials.sleeve` — the combat-shirt sleeve.
    Sleeve,
}

/// One `THREE.Mesh` in the arm rig: a geometry, the surface it wears, the
/// three render flags `hands.js:641-647` authors, and the `color` vertex
/// attribute [`Arm::bake_contact_ao`] writes into.
#[derive(Debug, Clone, PartialEq)]
pub struct HandMesh {
    pub surface: HandSurface,
    pub geo: Geo,
    /// The `color` attribute, `xyz` per vertex. **A `Float32Array` in the
    /// source** (`hands.js:949`) — the width matters, because
    /// [`Arm::bake_contact_ao`]'s `Math.max` accumulate reads the stored,
    /// `f32`-rounded value back on every pass. Empty until that bake runs.
    pub color: Vec<f32>,
    /// `hands.js:643` — the arms receive the world sun shadow and cast nothing.
    pub cast_shadow: bool,
    /// `hands.js:644`.
    pub receive_shadow: bool,
    /// `hands.js:645`.
    pub frustum_culled: bool,
}

impl HandMesh {
    fn new(surface: HandSurface, geo: Geo) -> Self {
        HandMesh {
            surface,
            geo,
            color: Vec::new(),
            cast_shadow: false,
            receive_shadow: true,
            frustum_culled: false,
        }
    }
}

/// One `THREE.Object3D` in the rig arena.
///
/// `rotation` is a `THREE.Euler` in its default `'XYZ'` order and
/// `quaternion` is the value `Matrix4.compose` actually consumes; Three keeps
/// the two in sync through `Euler.onChange`, and so does
/// [`Arm::set_node_rotation`]. The one place the source writes the quaternion
/// *directly* (`aimBone` on the two sleeve pivots) leaves `rotation` stale in
/// Three as well as here — nothing reads it, and reproducing Three's
/// quaternion-to-Euler back-sync would be inventing a value the source never
/// uses.
#[derive(Debug, Clone, PartialEq)]
pub struct RigNode {
    /// `Object3D.name`, set only on the four nodes the source names.
    pub name: &'static str,
    pub position: V3,
    /// `Object3D.rotation`, Euler order `'XYZ'`.
    pub rotation: V3,
    pub quaternion: Q,
    pub scale: V3,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Index into [`Arm::meshes`], for a node that is a `THREE.Mesh`.
    pub mesh: Option<usize>,
    /// `Object3D.matrixWorld`, refreshed by [`Arm::update_world_matrix`].
    pub matrix_world: M4,
}

/// Thumb dimensions, shared by the mesh and the contact solve.
/// `const THUMB`, `hands.js:330`.
///
/// The proximal segment is the METACARPAL as well as the proximal phalanx,
/// which is why `l0` is 50 mm rather than 38: this rig has no metacarpal
/// segment, and 68 mm of thumb cannot reach a 54 mm tube from the heel of the
/// palm (`hands.js:276-293`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbSpec {
    pub l0: f64,
    pub l1: f64,
    pub r0: f64,
    pub r1: f64,
    pub r2: f64,
}

/// `const THUMB = { l0: 0.05, l1: 0.032, r0: 0.0115, r1: 0.0102, r2: 0.0078 }`.
pub const THUMB: ThumbSpec = ThumbSpec {
    l0: 0.05,
    l1: 0.032,
    r0: 0.0115,
    r1: 0.0102,
    r2: 0.0078,
};

/// One entry of the `fingerSpecs` table, `hands.js:602-607`.
#[derive(Debug, Clone, Copy)]
struct FingerSpec {
    x: f64,
    len: [f64; 3],
    r: [f64; 4],
}

/// `fingerSpecs`, `hands.js:602-607`: index (separate so it can work the
/// trigger), middle, ring, little.
const FINGER_SPECS: [FingerSpec; 4] = [
    FingerSpec {
        x: 0.0298,
        len: [0.045, 0.028, 0.022],
        r: [0.0102, 0.0096, 0.0086, 0.0062],
    },
    FingerSpec {
        x: 0.0102,
        len: [0.049, 0.031, 0.023],
        r: [0.0104, 0.0098, 0.0088, 0.0064],
    },
    FingerSpec {
        x: -0.0104,
        len: [0.046, 0.029, 0.022],
        r: [0.01, 0.0094, 0.0084, 0.006],
    },
    FingerSpec {
        x: -0.0298,
        len: [0.038, 0.024, 0.02],
        r: [0.0092, 0.0086, 0.0078, 0.0056],
    },
];

/// `buildSleeve`'s options bag (`hands.js:337`): `{ folds, elbowPad, cuff }`.
#[derive(Debug, Clone, Copy)]
struct SleeveOpts {
    /// `opts.folds ?? 3`.
    folds: usize,
    /// `opts.elbowPad`.
    elbow_pad: bool,
    /// `opts.cuff`.
    cuff: bool,
}

/// Tapered sleeve with fold rings, an elbow pad and a rolled cuff. Both ends
/// are CLOSED — an open lathe reads as a length of pipe, which is exactly the
/// "grey sausage" failure this rig has to avoid. `buildSleeve(material, len,
/// r0, r1, opts)`, `hands.js:337-459`.
fn build_sleeve(len: f64, r0: f64, r1: f64, opts: SleeveOpts) -> Geo {
    let mut parts: Vec<Geo> = Vec::new();
    // SEGMENT COUNT: the support forearm's closest approach to the eye is
    // ~0.38 m and it is ~120 px wide, so a 20-gon puts a facet sagitta of
    // 0.7 px on the silhouette. 32 takes it to 0.28 px, under the AA
    // threshold. (`hands.js:339-345`)
    const SEG: u32 = 32;
    // The shell profile is not a smooth cone: it bells behind the elbow, is
    // pulled tight over the muscle belly a third of the way down, and bunches
    // again at the cuff.
    let shell = lathe_z(
        &[
            [0.0, 0.0],
            [0.0, dim(r0 * 0.55)],
            [-0.004, dim(r0 * 0.82)],
            [-0.006, dim(r0 * 0.98)],
            [0.004, dim(r0)],
            [dim(len * 0.16), dim(r0 * 1.03)],
            [dim(len * 0.34), dim(r0 * 0.9)],
            [dim(len * 0.52), dim((r0 + r1) * 0.5)],
            [dim(len * 0.72), dim(r1 * 1.1)],
            [dim(len - 0.016), dim(r1 * 1.0)],
            [dim(len - 0.005), dim(r1 * 1.07)],
            [dim(len), dim(r1 * 0.98)],
            [dim(len + 0.003), dim(r1 * 0.8)],
            [dim(len + 0.004), 0.0],
        ],
        SEG,
        0.0,
        std::f32::consts::TAU,
    );
    parts.push(shell);
    // Joint mass at the far end so the two bones read as one limb.
    let mut joint = lathe_z(
        &[
            [dim(len - r1 * 1.1), 0.0],
            [dim(len - r1 * 0.9), dim(r1 * 0.75)],
            [dim(len - r1 * 0.2), dim(r1 * 1.04)],
            [dim(len + r1 * 0.5), dim(r1 * 0.9)],
            [dim(len + r1 * 0.8), dim(r1 * 0.4)],
            [dim(len + r1 * 0.85), 0.0],
        ],
        20,
        0.0,
        std::f32::consts::TAU,
    );
    geo_scale(&mut joint, 1.0, 0.94, 1.0);
    parts.push(joint);
    // Fold rings. The only concave creases on the whole limb, and the
    // curvature mask bake turns every one of them into a grime line with a
    // dust-rubbed crown either side. Ellipticity and a per-fold radius jitter
    // matter as much as the count.
    let folds = opts.folds;
    for i in 0..folds {
        let fi = i as f64;
        let t = 0.14 + (fi / (folds as f64 - 1.0).max(1.0)) * 0.7;
        // deterministic wobble, so captures stay byte-identical
        let j = (fi * 2.399 + 0.7).sin() * 0.5 + (fi * 5.13).sin() * 0.25;
        let r = (r0 + (r1 - r0) * t) * (1.0 + j * 0.06);
        let mut f = ring(
            dim(r * 0.985),
            dim(r * (0.085 + j * 0.03)),
            24,
            6,
            std::f32::consts::TAU,
        );
        geo_rotate_x(&mut f, std::f64::consts::FRAC_PI_2);
        geo_rotate_y(&mut f, j * 0.12);
        geo_scale(&mut f, 1.0, 0.93, 1.0);
        geo_translate(&mut f, 0.0, 0.0, len * t + j * 0.004);
        parts.push(f);
    }
    // Two longitudinal wrinkle ridges down the inboard and outboard flanks.
    for sx in [-1.0f64, 1.0f64] {
        let mut w = lathe_z(
            &[
                [dim(len * 0.2), 0.0],
                [dim(len * 0.3), dim(r0 * 0.16)],
                [dim(len * 0.55), dim(r0 * 0.2)],
                [dim(len * 0.78), dim(r0 * 0.13)],
                [dim(len * 0.86), 0.0],
            ],
            10,
            0.0,
            std::f32::consts::TAU,
        );
        geo_scale(&mut w, 1.0, 0.5, 1.0);
        geo_rotate_z(&mut w, sx * 0.4);
        geo_translate(&mut w, sx * (r0 + r1) * 0.46, -(r0 + r1) * 0.1, 0.0);
        parts.push(w);
    }
    if opts.elbow_pad {
        let mut pad = blob(
            dim(r0 * 1.5),
            dim(r0 * 0.6),
            dim(len * 0.3),
            dim(r0 * 0.3),
            3,
        );
        geo_translate(&mut pad, 0.0, r0 * 0.75, len * 0.12);
        parts.push(pad);
    }
    if opts.cuff {
        // Rolled, stitched cuff: two proud bands with a seam channel between
        // them, giving the wrist a hard terminator so the sleeve does not
        // appear to melt into the glove.
        let cuff = lathe_z(
            &[
                [dim(len - 0.032), dim(r1 * 1.02)],
                [dim(len - 0.029), dim(r1 * 1.17)],
                [dim(len - 0.019), dim(r1 * 1.16)],
                [dim(len - 0.016), dim(r1 * 1.08)],
                [dim(len - 0.012), dim(r1 * 1.08)],
                [dim(len - 0.009), dim(r1 * 1.18)],
                [dim(len - 0.003), dim(r1 * 1.17)],
                [dim(len), dim(r1 * 1.02)],
            ],
            SEG,
            0.0,
            std::f32::consts::TAU,
        );
        parts.push(cuff);
    }
    let mut g = merge_all(parts).expect("buildSleeve always pushes at least the shell");
    geo_rotate_y(&mut g, std::f64::consts::PI); // extend along -Z, like the bones
    g
}

/* -------------------------------------------------------------------------- */
/*  poses                                                                     */
/* -------------------------------------------------------------------------- */

/// One [`hand_pose`] entry: per-joint flexion in radians, proximal to distal,
/// one triple per finger (index, middle, ring, little), plus the thumb's two
/// hinges and its base (abduction/rotation) orientation. `hands.js:1056-1163`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandPose {
    pub fingers: [[f64; 3]; 4],
    pub thumb: [f64; 2],
    pub thumb_base: [f64; 3],
}

/// The six authored grip shapes a pose name in [`crate::weapons::clips`] or a
/// weapon's `lhandPose` selects. `hands.js:1056-1163`, one variant per
/// top-level `HAND_POSES` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandPoseName {
    /// Firing grip on a pistol grip. `hands.js:1057-1075`.
    Grip,
    /// Support hand wrapped around a handguard. `hands.js:1077-1086`.
    Wrap,
    /// C-clamp on a handguard. `hands.js:1101-1129`.
    Clamp,
    /// Two-handed pistol grip, support hand cups the shooting hand.
    /// `hands.js:1131-1140`.
    Cup,
    /// Open hand: mag grab, charging handle, inspect. `hands.js:1142-1151`.
    Open,
    /// Pinch: holding the charging handle or a magazine spine.
    /// `hands.js:1153-1162`.
    Pinch,
}

impl HandPoseName {
    /// The `HAND_POSES` key this variant is, as the source spells it. The
    /// source addresses poses by string because `fitToCylinder` writes
    /// *synthetic* keys (`clamp:rifle`) alongside the authored six — see
    /// [`Arm::set_pose_key`].
    pub fn key(self) -> &'static str {
        match self {
            HandPoseName::Grip => "grip",
            HandPoseName::Wrap => "wrap",
            HandPoseName::Clamp => "clamp",
            HandPoseName::Cup => "cup",
            HandPoseName::Open => "open",
            HandPoseName::Pinch => "pinch",
        }
    }
}

impl From<crate::weapons::clips::Pose> for HandPoseName {
    /// `clips.js`'s four grip-shape literals are a subset of `HAND_POSES`'
    /// six; `grip`/`cup` are never named by an authored clip key
    /// (`clips.rs`'s `Pose` enum has no variant for them).
    fn from(p: crate::weapons::clips::Pose) -> Self {
        match p {
            crate::weapons::clips::Pose::Wrap => HandPoseName::Wrap,
            crate::weapons::clips::Pose::Pinch => HandPoseName::Pinch,
            crate::weapons::clips::Pose::Open => HandPoseName::Open,
            crate::weapons::clips::Pose::Clamp => HandPoseName::Clamp,
        }
    }
}

/// `HAND_POSES`, `hands.js:1056-1163`, transcribed field-for-field.
///
/// Read straight off reference photos of a firing grip. `clamp`'s numbers are
/// not authored by eye: they come out of a per-joint bisection against the
/// rifle's 47 mm handguard that puts the PIP, the DIP and the fingertip all
/// exactly 8.2 mm from the surface, and the distribution that falls out
/// (MCP ~0.6, PIP ~1.2, DIP ~0.8) is what a real hand does on a tube — the
/// LONGEST finger curls most, not the little one (`hands.js:1102-1115`).
pub fn hand_pose(name: HandPoseName) -> HandPose {
    match name {
        HandPoseName::Grip => HandPose {
            fingers: [
                [0.55, 0.72, 0.34],
                [1.15, 1.2, 0.62],
                [1.2, 1.25, 0.65],
                [1.22, 1.28, 0.66],
            ],
            thumb: [0.5, 0.34],
            thumb_base: [0.15, -1.02, -0.62],
        },
        HandPoseName::Wrap => HandPose {
            fingers: [
                [1.18, 1.05, 0.45],
                [1.26, 1.12, 0.5],
                [1.3, 1.16, 0.55],
                [1.34, 1.2, 0.6],
            ],
            thumb: [0.42, 0.3],
            thumb_base: [0.1, -1.15, -0.35],
        },
        HandPoseName::Clamp => HandPose {
            fingers: [
                [0.612, 1.059, 0.797],
                [0.731, 1.286, 0.863],
                [0.73, 1.268, 0.808],
                [0.601, 1.105, 0.684],
            ],
            thumb: [0.3, 0.24],
            thumb_base: [0.04, 0.76, -0.05],
        },
        HandPoseName::Cup => HandPose {
            fingers: [
                [1.05, 0.95, 0.4],
                [1.12, 1.0, 0.44],
                [1.16, 1.04, 0.48],
                [1.2, 1.08, 0.52],
            ],
            thumb: [0.28, 0.2],
            thumb_base: [0.0, -1.25, -0.2],
        },
        HandPoseName::Open => HandPose {
            fingers: [
                [0.35, 0.28, 0.14],
                [0.32, 0.26, 0.12],
                [0.34, 0.28, 0.14],
                [0.4, 0.32, 0.16],
            ],
            thumb: [0.12, 0.1],
            thumb_base: [0.1, -0.8, -0.35],
        },
        HandPoseName::Pinch => HandPose {
            fingers: [
                [0.95, 0.85, 0.55],
                [1.0, 0.9, 0.6],
                [0.7, 0.6, 0.35],
                [0.6, 0.5, 0.3],
            ],
            thumb: [0.62, 0.55],
            thumb_base: [0.25, -0.75, -0.7],
        },
    }
}

/// `HAND_POSES[name]` for an arbitrary key — `None` where the source's object
/// lookup would be `undefined`.
pub fn hand_pose_by_key(key: &str) -> Option<HandPose> {
    match key {
        "grip" => Some(hand_pose(HandPoseName::Grip)),
        "wrap" => Some(hand_pose(HandPoseName::Wrap)),
        "clamp" => Some(hand_pose(HandPoseName::Clamp)),
        "cup" => Some(hand_pose(HandPoseName::Cup)),
        "open" => Some(hand_pose(HandPoseName::Open)),
        "pinch" => Some(hand_pose(HandPoseName::Pinch)),
        _ => None,
    }
}

/* -------------------------------------------------------------------------- */
/*  mask-bake profiles                                                        */
/* -------------------------------------------------------------------------- */

/// One `bakeSurfaceMasks` amplitude profile (`hands.js:900-903`). Cloth,
/// moulded TPR and a stitched seam weather in completely different ways.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskProfile {
    pub wear_amp: f64,
    pub wear_exp: f64,
    pub grime_amp: f64,
    pub grime_exp: f64,
    pub ao_amp: f64,
    pub ao_exp: f64,
}

/// `CLOTH` (`hands.js:900`). Broad, soft; the exponent stays LOW so the mask
/// spreads off the fold crease and dusts the whole crown.
pub const CLOTH: MaskProfile = MaskProfile {
    wear_amp: 0.5,
    wear_exp: 1.6,
    grime_amp: 1.0,
    grime_exp: 1.15,
    ao_amp: 0.9,
    ao_exp: 1.1,
};
/// `SLEEVE` (`hands.js:901`).
pub const SLEEVE: MaskProfile = MaskProfile {
    wear_amp: 0.62,
    wear_exp: 1.5,
    grime_amp: 1.0,
    grime_exp: 1.0,
    ao_amp: 0.95,
    ao_exp: 1.0,
};
/// `PAD` (`hands.js:902`). A TPR knuckle cap polishes on its dome and
/// collects grime in the flex gap around it, so wear is high and tight.
pub const PAD: MaskProfile = MaskProfile {
    wear_amp: 0.85,
    wear_exp: 2.2,
    grime_amp: 0.95,
    grime_exp: 1.4,
    ao_amp: 1.0,
    ao_exp: 1.2,
};
/// `SEAM` (`hands.js:903`). A proud sewn edge is the FIRST thing to go pale.
pub const SEAM: MaskProfile = MaskProfile {
    wear_amp: 1.0,
    wear_exp: 2.6,
    grime_amp: 0.7,
    grime_exp: 1.6,
    ao_amp: 0.8,
    ao_exp: 1.2,
};

/// The options `bakeSurfaceMasks` hands `materials.bakeMasks`
/// (`hands.js:915`). A lower edge threshold than the weapon's 0.16: the limb
/// is all lathes and blobs, so its creases are gentle and a hard-edge
/// threshold finds nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BakeMaskOpts {
    pub wear: f64,
    pub grime: f64,
    pub ao: f64,
    pub edge_threshold: f64,
}

/// `{ wear: 1, grime: 1, ao: 1, edgeThreshold: 0.09 }`, `hands.js:915`. The
/// source also forwards an `rng`; on this side the caller's closure owns it.
pub const BAKE_MASK_OPTS: BakeMaskOpts = BakeMaskOpts {
    wear: 1.0,
    grime: 1.0,
    ao: 1.0,
    edge_threshold: 0.09,
};

/* -------------------------------------------------------------------------- */
/*  arm rig                                                                   */
/* -------------------------------------------------------------------------- */

/// `new Arm(side, materials, opts)`'s options (`hands.js:514-529`), minus
/// `materials` — see the module doc. Field defaults match the source's
/// `opts.x ?? default` chain.
#[derive(Debug, Clone, Copy)]
pub struct ArmOpts {
    pub scale: f64,
    pub upper: f64,
    pub fore: f64,
    pub shoulder_x: f64,
    pub shoulder_y: f64,
    pub shoulder_z: f64,
    pub pose: HandPoseName,
}

impl Default for ArmOpts {
    /// `hands.js:516-529`'s defaults: `scale=1, upper=L_UPPER, fore=L_FORE,
    /// shoulderX=0.19, shoulderY=-0.19, shoulderZ=0.12, pose='wrap'`.
    fn default() -> Self {
        ArmOpts {
            scale: 1.0,
            upper: L_UPPER,
            fore: L_FORE,
            shoulder_x: 0.19,
            shoulder_y: -0.19,
            shoulder_z: 0.12,
            pose: HandPoseName::Wrap,
        }
    }
}

/// A finger's node indices: its root and its three flexion joints
/// (`buildFinger`'s `{ root, joints }`, `hands.js:102-147`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FingerNodes {
    pub root: usize,
    pub joints: [usize; 3],
}

/// The thumb's node indices (`buildThumb`'s `{ root, joints }`,
/// `hands.js:295-327`). Two segments, not three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbNodes {
    pub root: usize,
    pub joints: [usize; 2],
}

/// Everything the contact scan measures against, gathered once so the scan
/// body reads like `gapAt`'s closure does (`hands.js:705-712`).
#[derive(Debug, Clone, Copy)]
struct FitCtx {
    /// `_fitInv` — the inverse of the arm root's world matrix.
    inv: M4,
    /// `_fitAx0` — a point on the cylinder axis.
    ax0: V3,
    /// `_fitAxis` — the normalized cylinder axis.
    axis: V3,
    radius: f64,
    clearance: f64,
}

/// One arm: shoulder -> upper -> fore -> hand, solved from the hand target.
/// `class Arm`, `hands.js:513-1049`.
#[derive(Debug, Clone)]
pub struct Arm {
    /// `-1` left, `+1` right. `hands.js:515`.
    pub side: f64,
    pub scale: f64,
    pub l1: f64,
    pub l2: f64,
    /// Body-fixed shoulder, in the arm root's parent space. `hands.js:525-529`.
    pub shoulder: V3,
    /// Elbow-swing direction, in the arm root's space — **not** hand space.
    /// Expressing the pole in hand space is the intuitive choice and it is
    /// wrong: the support hand is rolled palm-up on the handguard, so its
    /// local "down" points at the sky and the elbow swings UP, through the
    /// near plane. Elbows go down and outboard, always. `hands.js:531-540`.
    pub pole: V3,

    /// The rig's `THREE.Object3D` arena. `nodes[0]` is `this.root`.
    pub nodes: Vec<RigNode>,
    /// Every `THREE.Mesh`'s geometry, referenced by [`RigNode::mesh`].
    pub meshes: Vec<HandMesh>,

    /// `this.root`.
    pub root: usize,
    /// `this.upperPivot` / `this.forePivot` (`hands.js:571-576`).
    pub upper_pivot: usize,
    pub fore_pivot: usize,
    /// `this.upper` / `this.fore`, the two sleeve meshes' nodes.
    pub upper: usize,
    pub fore: usize,
    /// `this.hand` (`hands.js:579`) and `this.handInner` (`hands.js:581`),
    /// the latter carrying the chirality mirror.
    pub hand: usize,
    pub hand_inner: usize,
    /// `this.glove` (`hands.js:597`) — `buildGlove`'s root, and the subtree
    /// [`Arm::bake_contact_ao`] walks.
    pub glove: usize,
    /// `this.fingers` (`hands.js:608`), index/middle/ring/little.
    pub fingers: [FingerNodes; 4],
    /// `this.thumb` (`hands.js:632`).
    pub thumb: ThumbNodes,

    /// `this._segRadius` (`hands.js:611`) — per-finger segment radii, scaled.
    pub seg_radius: [[f64; 4]; 4],
    /// `this._segLength` (`hands.js:612`) — per-finger segment lengths, scaled.
    pub seg_length: [[f64; 3]; 4],

    /// The current solved hand target, set by [`Arm::solve`]. `hands.js:1000-1001`.
    pub hand_pos: V3,
    pub hand_quat: Q,
    /// Upper-arm pivot: position = shoulder, orientation aims the bone at the
    /// elbow. `hands.js:1031-1033`. Mirrored out of the node arena for the
    /// convenience of a renderer that only wants the two bone transforms.
    pub upper_pos: V3,
    pub upper_quat: Q,
    pub elbow: V3,
    /// Forearm pivot: position = elbow, orientation aims the bone at the
    /// hand, rolled toward the back of the hand. `hands.js:1037-1040`.
    pub fore_pos: V3,
    pub fore_quat: Q,

    /// `this.poses` (`hands.js:655`) — per-weapon pose overrides, written by
    /// [`Arm::fit_to_cylinder`]. [`Arm::set_pose_key`] looks here first, so a
    /// pose solved against one weapon's handguard cannot leak onto another's
    /// — and a clip that swaps the support hand to `open` and back to `clamp`
    /// restores the FITTED clamp, not the authored one.
    ///
    /// `BTreeMap`, not `HashMap`: a JS object iterates in insertion order and
    /// a Rust `HashMap` is randomised per process, which would make any dump
    /// of this map differ between runs of the same build.
    pub poses: BTreeMap<String, HandPose>,

    /// The curl values the last pose application wrote. `hands.js:972-983`.
    pub pose: HandPose,
    /// `this.pose` (`hands.js:864`/`981`) — the pose KEY, which may be a
    /// synthetic `clamp:<weapon>` written by [`Arm::fit_to_cylinder`].
    pub pose_key: String,
    /// The last *authored* pose [`Arm::set_pose`] was given. Unchanged by
    /// [`Arm::set_pose_key`] with a synthetic key, which has no enum spelling
    /// — [`Arm::pose_key`] is the authoritative one.
    pub pose_name: HandPoseName,
    /// The index finger's three joint rotations (**not** curls — these carry
    /// the source's leading minus). Written by [`Arm::set_trigger`] and by
    /// every pose application, mirroring `this.fingers[0].joints[j].rotation.x`.
    pub trigger_curl: [f64; 3],
}

impl Arm {
    /// `constructor(side, materials, opts)`, `hands.js:514-658`.
    pub fn new(side: f64, opts: ArmOpts) -> Self {
        let scale = opts.scale;
        let l1 = opts.upper * scale;
        let l2 = opts.fore * scale;
        let shoulder = V3::new(side * opts.shoulder_x, opts.shoulder_y, opts.shoulder_z);
        let pole = V3::new(side * 0.46, -0.86, 0.22).normalize();

        let mut arm = Arm {
            side,
            scale,
            l1,
            l2,
            shoulder,
            pole,
            nodes: Vec::new(),
            meshes: Vec::new(),
            root: 0,
            upper_pivot: 0,
            fore_pivot: 0,
            upper: 0,
            fore: 0,
            hand: 0,
            hand_inner: 0,
            glove: 0,
            fingers: [FingerNodes {
                root: 0,
                joints: [0; 3],
            }; 4],
            thumb: ThumbNodes {
                root: 0,
                joints: [0; 2],
            },
            seg_radius: [[0.0; 4]; 4],
            seg_length: [[0.0; 3]; 4],
            hand_pos: V3::ZERO,
            hand_quat: Q::IDENTITY,
            upper_pos: shoulder,
            upper_quat: Q::IDENTITY,
            elbow: shoulder,
            fore_pos: shoulder,
            fore_quat: Q::IDENTITY,
            poses: BTreeMap::new(),
            pose: hand_pose(opts.pose),
            pose_key: String::new(),
            pose_name: opts.pose,
            trigger_curl: [0.0; 3],
        };

        arm.root = arm.add_node(if side < 0.0 { "arm-left" } else { "arm-right" }, None);

        // Bones. Geometry extends along -Z from each joint.
        //
        // Sleeve radii, MEASURED twice. At 78 mm across the elbow / 54 mm at
        // the wrist the support forearm rendered as a 160 px-wide smooth tube
        // crossing the lower third of every hipfire frame. A real combat shirt
        // over a forearm is 68 mm at the elbow tapering to 48 mm at the wrist,
        // and that is what these are: 0.034/0.024. Fold counts go UP, not
        // down: with the tube narrower the folds carry the silhouette.
        // (`hands.js:544-562`)
        let upper_geo = build_sleeve(
            l1,
            0.044 * scale,
            0.036 * scale,
            SleeveOpts {
                folds: 5,
                elbow_pad: true,
                cuff: false,
            },
        );
        let fore_geo = build_sleeve(
            l2,
            0.034 * scale,
            0.024 * scale,
            SleeveOpts {
                folds: 7,
                elbow_pad: false,
                cuff: true,
            },
        );
        let arm_root = arm.root;
        arm.upper_pivot = arm.add_node("", Some(arm_root));
        arm.fore_pivot = arm.add_node("", Some(arm_root));
        let (up_pivot, fp_pivot) = (arm.upper_pivot, arm.fore_pivot);
        arm.upper = arm.add_mesh_node(up_pivot, HandSurface::Sleeve, upper_geo);
        arm.fore = arm.add_mesh_node(fp_pivot, HandSurface::Sleeve, fore_geo);

        // Hand.
        arm.hand = arm.add_node(if side < 0.0 { "hand-left" } else { "hand-right" }, None);
        let hand_node = arm.hand;
        arm.hand_inner = arm.add_node("", Some(hand_node));
        // CHIRALITY. The basis built by handBasis is right-handed with
        // X = Y cross Z, so for a hand whose fingers run along -Z and whose
        // palm faces -Y, +X points AWAY from the thumb on a right hand and
        // TOWARD it on a left hand. The geometry below puts the thumb at +X,
        // which makes the authored mesh a LEFT hand — so it is the RIGHT arm
        // that needs the mirror, not the left. (`hands.js:583-595`)
        arm.nodes[arm.hand_inner].scale.x = if side < 0.0 { 1.0 } else { -1.0 };
        let hand_inner = arm.hand_inner;
        arm.glove = arm.build_glove(hand_inner, scale);
        // `this.root.add(this.hand)` happens AFTER the two pivots, so the
        // root's child order is [upperPivot, forePivot, hand] — which is the
        // traversal order every bake below walks.
        let (root, hand) = (arm.root, arm.hand);
        arm.attach(root, hand);

        // Fingers: index is separate so it can work the trigger.
        for i in 0..4 {
            let sp = FINGER_SPECS[i];
            arm.seg_radius[i] = [
                sp.r[0] * scale,
                sp.r[1] * scale,
                sp.r[2] * scale,
                sp.r[3] * scale,
            ];
            arm.seg_length[i] = [sp.len[0] * scale, sp.len[1] * scale, sp.len[2] * scale];
        }
        let glove = arm.glove;
        for i in 0..4 {
            let sp = FINGER_SPECS[i];
            let lengths = arm.seg_length[i];
            let radii = arm.seg_radius[i];
            let f = arm.build_finger(glove, lengths, radii, [0.0, 0.0, 0.0]);
            // The metacarpophalangeal joints sit on the PALMAR half of the
            // hand, not on its centre line: -6 mm puts the finger axis 8 mm
            // off the palm's contact plane, which is one finger radius.
            // (`hands.js:620-626`)
            arm.nodes[f.root].position = V3::new(sp.x * scale, -0.006 * scale, -0.096 * scale);
            // fingers fan out very slightly
            arm.set_node_rotation(f.root, 0.0, -sp.x * 2.2, 0.0);
            arm.fingers[i] = f;
        }
        arm.thumb = arm.build_thumb(glove, scale);
        // The carpometacarpal joint is palmar and a little further into the
        // hand than the old placement: a thumb rooted on the hand's centre
        // plane rotates in the plane of the back of the hand, which is why
        // the old one read as a spur. (`hands.js:634-637`)
        let thumb_root = arm.thumb.root;
        arm.nodes[thumb_root].position = V3::new(0.037 * scale, -0.009 * scale, -0.04 * scale);
        arm.set_node_rotation(thumb_root, 0.2, -0.95, -0.5);

        // `this.root.traverse(...)` (`hands.js:641-647`) sets the same three
        // render flags on every mesh — the values `HandMesh::new` already
        // authors, so there is nothing left for the sweep to change.

        arm.set_pose(opts.pose);
        arm
    }

    /* ---- scene graph ---------------------------------------------------- */

    fn add_node(&mut self, name: &'static str, parent: Option<usize>) -> usize {
        let i = self.nodes.len();
        self.nodes.push(RigNode {
            name,
            position: V3::ZERO,
            rotation: V3::ZERO,
            quaternion: Q::IDENTITY,
            scale: V3::new(1.0, 1.0, 1.0),
            parent,
            children: Vec::new(),
            mesh: None,
            matrix_world: M4::IDENTITY,
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(i);
        }
        i
    }

    /// `parent.add(child)` for a node that already exists — the source builds
    /// `this.hand` before parenting it, and the parenting ORDER decides
    /// traversal order.
    fn attach(&mut self, parent: usize, child: usize) {
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    fn add_mesh_node(&mut self, parent: usize, surface: HandSurface, geo: Geo) -> usize {
        let mi = self.meshes.len();
        self.meshes.push(HandMesh::new(surface, geo));
        let n = self.add_node("", Some(parent));
        self.nodes[n].mesh = Some(mi);
        n
    }

    /// `node.rotation.set(x, y, z)` — writes the Euler and re-derives the
    /// quaternion through Three's `'XYZ'` branch, exactly as
    /// `Euler.onChange -> Quaternion.setFromEuler` does.
    fn set_node_rotation(&mut self, i: usize, x: f64, y: f64, z: f64) {
        self.nodes[i].rotation = V3::new(x, y, z);
        self.nodes[i].quaternion = Q::from_euler_xyz(x, y, z);
    }

    /// `node.rotation.x = a`, leaving `y`/`z` alone.
    fn set_node_rotation_x(&mut self, i: usize, x: f64) {
        let r = self.nodes[i].rotation;
        self.set_node_rotation(i, x, r.y, r.z);
    }

    /// `Object3D.updateWorldMatrix(updateParents, updateChildren)`
    /// (`three/src/core/Object3D.js`). Every node here has
    /// `matrixAutoUpdate`, so the local matrix is always recomposed.
    ///
    /// `this.root.updateMatrixWorld(true)` (`hands.js:697`, `939`) reduces to
    /// `update_world_matrix(root, false, true)`: with `force` set, Three's
    /// `updateMatrixWorld` recomposes every local and every world down the
    /// tree, which is what this does.
    pub fn update_world_matrix(&mut self, i: usize, update_parents: bool, update_children: bool) {
        if update_parents {
            if let Some(p) = self.nodes[i].parent {
                self.update_world_matrix(p, true, false);
            }
        }
        let n = &self.nodes[i];
        let local = M4::compose(n.position, n.quaternion, n.scale);
        let world = match n.parent {
            None => local,
            Some(p) => M4::multiply_matrices(self.nodes[p].matrix_world, local),
        };
        self.nodes[i].matrix_world = world;
        if update_children {
            let kids = self.nodes[i].children.clone();
            for k in kids {
                self.update_world_matrix(k, false, true);
            }
        }
    }

    /// `Object3D.traverse(cb)`'s visit order: the node itself, then each
    /// child's subtree in order.
    pub fn traverse(&self, i: usize) -> Vec<usize> {
        let mut out = Vec::new();
        self.traverse_into(i, &mut out);
        out
    }

    fn traverse_into(&self, i: usize, out: &mut Vec<usize>) {
        out.push(i);
        for k in 0..self.nodes[i].children.len() {
            let c = self.nodes[i].children[k];
            self.traverse_into(c, out);
        }
    }

    /* ---- mesh builders --------------------------------------------------- */

    /// Build one finger as three nested groups so it can curl.
    /// `buildFinger(materials, spec)`, `hands.js:102-147`.
    ///
    /// `seamSide` is never supplied by any caller in the source, so
    /// `(seamSide ?? 0) === 0` is always true and every finger gets seams down
    /// BOTH flanks. One seam per finger leaves three boundaries out of five
    /// unmarked; seaming both sides puts a light line at every boundary. Two
    /// segments only — the distal phalanx is 22 mm long and a seam on it is
    /// sub-pixel.
    fn build_finger(
        &mut self,
        parent: usize,
        lengths: [f64; 3],
        radii: [f64; 4],
        curl: [f64; 3],
    ) -> FingerNodes {
        let root = self.add_node("", Some(parent));
        let mut joints = [0usize; 3];
        let mut p = root;
        for i in 0..3 {
            let j = self.add_node("", Some(p));
            self.set_node_rotation_x(j, -curl[i]);
            let geo =
                merge_all(vec![segment(lengths[i], radii[i], radii[i + 1])]).expect("one geometry");
            self.add_mesh_node(j, HandSurface::Glove, geo);
            if i < 2 {
                let seams = merge_all(vec![
                    segment_seam(lengths[i], radii[i], radii[i + 1], 1.0),
                    segment_seam(lengths[i], radii[i], radii[i + 1], -1.0),
                ])
                .expect("two geometries");
                self.add_mesh_node(j, HandSurface::Seam, seams);
            }
            if i < 2 {
                self.add_mesh_node(j, HandSurface::Pad, segment_pad(lengths[i], radii[i]));
            } else {
                // fingertip grip patch on the palm side
                let mut tip = blob(
                    dim(radii[i] * 1.5),
                    dim(radii[i] * 0.5),
                    dim(lengths[i] * 0.7),
                    dim(radii[i] * 0.2),
                    2,
                );
                geo_translate(&mut tip, 0.0, -radii[i] * 0.72, -lengths[i] * 0.45);
                self.add_mesh_node(j, HandSurface::Pad, tip);
            }
            let next = self.add_node("", Some(j));
            self.nodes[next].position.z = -lengths[i];
            p = next;
            joints[i] = j;
        }
        FingerNodes { root, joints }
    }

    /// Glove: palm, thumb web, knuckle plate, wrist strap. Fingers are added
    /// as children by the constructor so they can be posed per-weapon.
    /// `buildGlove(materials, opts)`, `hands.js:153-274`.
    fn build_glove(&mut self, parent: usize, scale: f64) -> usize {
        let w = 0.088 * scale;
        let h = 0.032 * scale;
        let palm_len = 0.098 * scale;
        let root = self.add_node("", Some(parent));

        let mut shell: Vec<Geo> = Vec::new();
        // Palm. Two overlapping blocks rather than one, because a single
        // 88 x 98 mm slab is exactly what the support hand presents to the
        // camera in a C-clamp and it reads as a brick. A hand is ~88 mm across
        // the knuckles and ~72 mm across the wrist, so the taper is real.
        let mut palm = blob(dim(w), dim(h), dim(palm_len * 0.62), dim(0.012 * scale), 3);
        geo_translate(&mut palm, 0.0, 0.0, -palm_len * 0.66);
        shell.push(palm);
        let mut palm_rear = blob(
            dim(w * 0.83),
            dim(h * 0.96),
            dim(palm_len * 0.52),
            dim(0.012 * scale),
            3,
        );
        geo_translate(&mut palm_rear, 0.0, -h * 0.01, -palm_len * 0.26);
        shell.push(palm_rear);
        // Thenar (thumb muscle) and the heel of the hand.
        let mut thenar = blob(
            dim(w * 0.42),
            dim(h * 0.92),
            dim(palm_len * 0.6),
            dim(0.014 * scale),
            3,
        );
        geo_translate(&mut thenar, w * 0.3, -h * 0.06, -palm_len * 0.3);
        shell.push(thenar);
        let mut heel = blob(
            dim(w * 0.92),
            dim(h * 0.86),
            dim(0.03 * scale),
            dim(0.012 * scale),
            3,
        );
        geo_translate(&mut heel, 0.0, -h * 0.04, -0.012 * scale);
        shell.push(heel);
        // Knuckle lumps.
        for i in 0..4 {
            let x = w * (0.34 - f64::from(i) * 0.225);
            let mut k = dome(dim(0.0072 * scale), 10, 0.62);
            geo_rotate_x(&mut k, -std::f64::consts::FRAC_PI_2);
            geo_translate(&mut k, x, h * 0.42, -palm_len * 0.94);
            shell.push(k);
        }
        let shell_geo = merge_all(shell).expect("shell is never empty");
        self.add_mesh_node(root, HandSurface::Glove, shell_geo);

        // Dorsal armour. COVERAGE BUDGET: the caps plus everything else on the
        // dorsum must not exceed 55% of the back of the hand. Four caps at
        // 17% x 30% (= 20.4%) over the knuckles only, and one small metacarpal
        // panel at 44% x 22% (= 9.7%) with a clear 12% gap of bare shell
        // between it and the caps. Total 30%. (`hands.js:192-229`)
        let mut pads: Vec<Geo> = Vec::new();
        for i in 0..4 {
            let x = w * (0.335 - f64::from(i) * 0.223);
            let mut cap = blob(
                dim(w * 0.17),
                dim(h * 0.3),
                dim(palm_len * 0.3),
                dim(0.005 * scale),
                3,
            );
            // outboard caps sit slightly lower, following the knuckle arch
            let drop = if (f64::from(i) - 1.5).abs() > 1.0 {
                h * 0.055
            } else {
                0.0
            };
            geo_translate(&mut cap, x, h * 0.46 - drop, -palm_len * 0.82);
            pads.push(cap);
        }
        let mut back_panel = blob(
            dim(w * 0.44),
            dim(h * 0.17),
            dim(palm_len * 0.22),
            dim(0.005 * scale),
            3,
        );
        geo_translate(&mut back_panel, 0.0, h * 0.44, -palm_len * 0.4);
        pads.push(back_panel);
        // Palm grip patch.
        let mut patch = blob(
            dim(w * 0.82),
            dim(h * 0.18),
            dim(palm_len * 0.66),
            dim(0.006 * scale),
            3,
        );
        geo_translate(&mut patch, 0.0, -h * 0.52, -palm_len * 0.48);
        pads.push(patch);
        let pads_geo = merge_all(pads).expect("pads is never empty");
        self.add_mesh_node(root, HandSurface::Pad, pads_geo);

        // Seams down the sides of the hand. NOTE: `materials.pad`, not
        // `materials.seam` — the source's own choice (`hands.js:243`), and
        // one that changes which `bakeSurfaceMasks` profile these get (PAD,
        // not SEAM). Ported as written.
        let mut seams: Vec<Geo> = Vec::new();
        for sx in [-1.0f64, 1.0f64] {
            let mut s = box_geo(
                dim(0.0016 * scale),
                dim(h * 0.5),
                dim(palm_len * 0.8),
                0.0004,
                1,
            );
            geo_translate(&mut s, sx * w * 0.5, 0.0, -palm_len * 0.5);
            seams.push(s);
        }
        let seams_geo = merge_all(seams).expect("two geometries");
        self.add_mesh_node(root, HandSurface::Pad, seams_geo);

        // Wrist cuff + strap + a small steel keeper.
        let mut cuff = lathe_z(
            &[
                [0.0, dim(w * 0.44)],
                [dim(0.004 * scale), dim(w * 0.47)],
                [dim(0.03 * scale), dim(w * 0.46)],
                [dim(0.034 * scale), dim(w * 0.42)],
            ],
            16,
            0.0,
            std::f32::consts::TAU,
        );
        geo_scale(&mut cuff, 1.0, 0.82, 1.0);
        let cuff_node = self.add_mesh_node(root, HandSurface::Glove, cuff);
        self.nodes[cuff_node].position.z = 0.004 * scale;
        let mut strap = lathe_z(
            &[
                [0.0, dim(w * 0.47)],
                // 0.0022 is NOT multiplied by `scale` in the source; ported
                // as written rather than "corrected".
                [0.0022, dim(w * 0.5)],
                [dim(0.009 * scale), dim(w * 0.5)],
                [dim(0.0112 * scale), dim(w * 0.47)],
            ],
            16,
            0.0,
            std::f32::consts::TAU,
        );
        geo_scale(&mut strap, 1.0, 0.82, 1.0);
        let strap_node = self.add_mesh_node(root, HandSurface::Pad, strap);
        self.nodes[strap_node].position.z = 0.02 * scale;

        root
    }

    /// Thumb: two segments on the +X side, angled across the grip.
    /// `buildThumb(materials, scale, spec)`, `hands.js:295-327`.
    fn build_thumb(&mut self, parent: usize, scale: f64) -> ThumbNodes {
        let spec = THUMB;
        let root = self.add_node("", Some(parent));
        let j1 = self.add_node("", Some(root));
        self.add_mesh_node(
            j1,
            HandSurface::Glove,
            segment(spec.l0 * scale, spec.r0 * scale, spec.r1 * scale),
        );
        self.add_mesh_node(
            j1,
            HandSurface::Pad,
            segment_pad(spec.l0 * scale, spec.r0 * scale),
        );
        // Seams down both flanks, as on the fingers — the thumb is the widest
        // single digit on screen in the support grip and a bare capsule reads
        // as a sausage.
        let seams = merge_all(vec![
            segment_seam(spec.l0 * scale, spec.r0 * scale, spec.r1 * scale, 1.0),
            segment_seam(spec.l0 * scale, spec.r0 * scale, spec.r1 * scale, -1.0),
        ])
        .expect("two geometries");
        self.add_mesh_node(j1, HandSurface::Seam, seams);

        let j2 = self.add_node("", Some(j1));
        self.nodes[j2].position.z = -spec.l0 * scale;
        self.add_mesh_node(
            j2,
            HandSurface::Glove,
            segment(spec.l1 * scale, spec.r1 * scale, spec.r2 * scale),
        );
        // Grip patch on the PALMAR side of the pad, matching the fingers, and
        // a small dorsal nail plate.
        let mut pad = blob(
            dim(spec.r2 * 1.6 * scale),
            dim(spec.r2 * 0.55 * scale),
            dim(spec.l1 * 0.66 * scale),
            0.0012,
            2,
        );
        geo_translate(&mut pad, 0.0, -spec.r2 * 0.78 * scale, -spec.l1 * 0.45 * scale);
        self.add_mesh_node(j2, HandSurface::Pad, pad);
        let mut nail = blob(
            dim(0.011 * scale),
            dim(0.0035 * scale),
            dim(0.016 * scale),
            0.0012,
            2,
        );
        geo_translate(&mut nail, 0.0, spec.r2 * scale, -0.016 * scale);
        self.add_mesh_node(j2, HandSurface::Pad, nail);

        ThumbNodes {
            root,
            joints: [j1, j2],
        }
    }

    /// `hands.js:595`: `this.handInner.scale.x = side < 0 ? 1 : -1`. Getting
    /// this backwards puts the trigger finger at the bottom-rear of the grip
    /// instead of on the trigger face (`hands.js:585-594`).
    pub fn hand_mirror_x(&self) -> f64 {
        if self.side < 0.0 {
            1.0
        } else {
            -1.0
        }
    }

    /* ---- contact solve --------------------------------------------------- */

    /// Signed distance from a joint-local point to the cylinder surface, and
    /// the point itself in arm-root space. `gapAt`, `hands.js:705-712`.
    fn gap_at(&mut self, joint: usize, l: [f64; 3], ctx: &FitCtx) -> (f64, V3) {
        self.update_world_matrix(joint, true, true);
        let p = V3::new(l[0], l[1], l[2])
            .apply_matrix4(self.nodes[joint].matrix_world)
            .apply_matrix4(ctx.inv);
        let mut d = p.sub(ctx.ax0);
        d = d.add_scaled(ctx.axis, -d.dot(ctx.axis));
        (d.length() - ctx.radius, p)
    }

    /// Scan a joint's flexion for the angle that puts `local` on the surface.
    /// `fitJoint`, `hands.js:722-738`.
    ///
    /// A scan, not a bisection: the gap is not monotonic in curl (past ~110
    /// deg the tip starts coming back OUT the far side of the tube), so a
    /// bisection can converge on the wrong root. 49 samples over the
    /// anatomical range is 2.5 deg of resolution, which is 0.4 mm at the
    /// fingertip.
    fn fit_joint(
        &mut self,
        joint: usize,
        local: [f64; 3],
        lo: f64,
        hi: f64,
        standoff: f64,
        ctx: &FitCtx,
    ) -> f64 {
        let mut best = self.nodes[joint].rotation.x;
        let mut best_cost = f64::INFINITY;
        for i in 0..=48 {
            let a = lo + ((hi - lo) * f64::from(i)) / 48.0;
            self.set_node_rotation_x(joint, a);
            let g = self.gap_at(joint, local, ctx).0 - standoff;
            // Target: on the surface, up to `clearance` proud, at most 1.5 mm
            // buried.
            let cost = (g - ctx.clearance * 0.5).abs()
                + if g < -0.0015 {
                    (-g - 0.0015) * 8.0
                } else {
                    0.0
                };
            if cost < best_cost {
                best_cost = cost;
                best = a;
            }
        }
        self.set_node_rotation_x(joint, best);
        best
    }

    /// BUILD-TIME CONTACT SOLVE: clamp every fingertip onto a cylinder.
    /// `fitToCylinder(handPos, handQuat, axisPoint, axisDir, radius, opts)`,
    /// `hands.js:690-866`. Returns the contact points in arm-root space, for
    /// the caller's baked AO.
    ///
    /// The authored `clamp` curls were derived analytically from a 47 mm tube
    /// and one nominal contact clock angle, and on paper they put the PIP, DIP
    /// and tip all 8.2 mm off the surface. On screen they did not, because the
    /// analytic solve ignored (a) the 0.88 Y-scale on the finger capsules,
    /// (b) the -6 mm palmar offset of the MCP row, (c) the fan-out rotation on
    /// each finger root and (d) the fact that the four fingers start at four
    /// different X, so they meet the cylinder at four different clock angles.
    /// Rather than push more algebra at it, this MEASURES it through the real
    /// transform chain (`hands.js:661-676`).
    ///
    /// `pose_name` is `opts.poseName ?? this.pose` and may be synthetic
    /// (`clamp:rifle`); the base pose it starts from is
    /// `this.poses[poseName] ?? HAND_POSES[poseName] ?? HAND_POSES.clamp` —
    /// note the fallback here is **clamp**, where [`Arm::set_pose_key`]'s is
    /// **wrap**.
    pub fn fit_to_cylinder(
        &mut self,
        hand_pos: V3,
        hand_quat: Q,
        axis_point: [f64; 3],
        axis_dir: [f64; 3],
        radius: f64,
        clearance: f64,
        pose_name: &str,
    ) -> Vec<V3> {
        let base = self
            .poses
            .get(pose_name)
            .copied()
            .or_else(|| hand_pose_by_key(pose_name))
            .unwrap_or_else(|| hand_pose(HandPoseName::Clamp));

        let (root, hand) = (self.root, self.hand);
        self.nodes[hand].position = hand_pos;
        self.nodes[hand].quaternion = hand_quat;
        self.update_world_matrix(root, false, true);
        // Everything is measured in the ARM ROOT's space, so the result is
        // independent of wherever the rig happens to be this frame.
        let ctx = FitCtx {
            inv: self.nodes[root].matrix_world.invert(),
            ax0: V3::new(axis_point[0], axis_point[1], axis_point[2]),
            axis: V3::new(axis_dir[0], axis_dir[1], axis_dir[2]).normalize(),
            radius,
            clearance,
        };

        // Wrap all three joints, PROXIMAL FIRST. Fitting only the distal joint
        // cannot wrap a cylinder: if the MCP and PIP are authored for a
        // different contact clock angle the finger traces the wrong spiral,
        // and the distal joint is then asked to close a gap it is 22 mm long
        // and physically cannot reach. (`hands.js:740-750`)
        let mut fingers = [[0.0f64; 3]; 4];
        let mut contacts: Vec<V3> = Vec::new();
        for i in 0..4 {
            let f = self.fingers[i];
            let mut curl = base.fingers[i];
            for j in 0..3 {
                self.set_node_rotation_x(f.joints[j], -curl[j]);
            }
            let rr = self.seg_radius[i];
            let ll = self.seg_length[i];
            for j in 0..2 {
                // The next joint's origin sits ON the finger's own axis, so it
                // wants to be one segment-radius clear of the surface, not on
                // it.
                let a = self.fit_joint(
                    f.joints[j],
                    [0.0, 0.0, -ll[j]],
                    -1.75,
                    -0.05,
                    rr[j + 1] * 0.92,
                    &ctx,
                );
                curl[j] = -a;
            }
            // The fingertip grip patch: palmar side, one radius below the
            // axis, half way along the distal segment — the same numbers as
            // the `tip` blob in buildFinger, so the mask and the mesh agree.
            let local = [0.0, -rr[3] * 1.05, -ll[2] * 0.5];
            let a2 = self.fit_joint(f.joints[2], local, -1.95, -0.1, 0.0, &ctx);
            curl[2] = -a2;
            fingers[i] = curl;
            let (_, p) = self.gap_at(f.joints[2], local, &ctx);
            contacts.push(p);
        }

        // ---- thumb: over the top and down the FAR side ---------------------
        //
        // THE THUMB BASE IS SOLVED TOO, and it has to be. MEASURED on the
        // shipped build: the four fingertips landed 0.4-0.7 mm off the
        // handguard and the THUMB TIP was 13.5 mm clear of it, because the two
        // flexion joints were fitted against a base rotation that was
        // AUTHORED, not solved. (`hands.js:777-799`)
        let mut thumb_base = base.thumb_base;
        let mut thumb = base.thumb;
        let t = self.thumb;
        self.set_node_rotation(t.root, thumb_base[0], thumb_base[1], thumb_base[2]);
        self.set_node_rotation_x(t.joints[0], -thumb[0]);
        self.set_node_rotation_x(t.joints[1], -thumb[1]);
        let tr = THUMB.r2 * self.scale;
        let tlen = THUMB.l1 * self.scale;
        let t_local = [0.0, -tr * 1.05, -tlen * 0.55];
        {
            // Mid-flex the two hinges while the base is searched, so the scan
            // measures where a naturally curled thumb would land rather than
            // where a straight one would.
            self.set_node_rotation_x(t.joints[0], -0.55);
            self.set_node_rotation_x(t.joints[1], -0.45);
            let y0 = thumb_base[1];
            let z0 = thumb_base[2];
            let mut best_y = y0;
            let mut best_z = z0;
            let mut best_cost = f64::INFINITY;
            // Two axes, not one. MEASURED: scanning abduction alone still left
            // the tip 13.2 mm clear, because from a metacarpal root sitting
            // 40-55 mm off a 54 mm tube a 68 mm thumb only reaches if it is
            // aimed at the surface in BOTH the across-the-palm and the
            // up-off-the-palm sense. 21 x 15 samples, build time.
            for i in 0..=20 {
                let yy = y0 - 1.3 + (2.6 * f64::from(i)) / 20.0;
                for k in 0..=14 {
                    let zz = z0 - 0.9 + (1.8 * f64::from(k)) / 14.0;
                    // `rotation.y = yy` then `rotation.z = zz` — two separate
                    // writes in the source, but Three re-derives the whole
                    // quaternion on each, so only the final state matters.
                    let rx = self.nodes[t.root].rotation.x;
                    self.set_node_rotation(t.root, rx, yy, zz);
                    let g = self.gap_at(t.joints[1], t_local, &ctx).0;
                    // Prefer just-touching; punish burying much harder than
                    // standing off, and add a small pull toward the authored
                    // pose so the solve stays plausible.
                    let cost = (g - clearance).abs()
                        + if g < -0.002 { (-g - 0.002) * 10.0 } else { 0.0 }
                        + ((yy - y0).abs() + (zz - z0).abs()) * 0.0009;
                    if cost < best_cost {
                        best_cost = cost;
                        best_y = yy;
                        best_z = zz;
                    }
                }
            }
            let rx = self.nodes[t.root].rotation.x;
            self.set_node_rotation(t.root, rx, best_y, best_z);
            thumb_base[1] = best_y;
            thumb_base[2] = best_z;
        }
        let a0 = self.fit_joint(
            t.joints[0],
            [0.0, 0.0, -THUMB.l0 * self.scale],
            -1.45,
            -0.02,
            THUMB.r1 * self.scale,
            &ctx,
        );
        thumb[0] = -a0;
        let a1 = self.fit_joint(t.joints[1], t_local, -1.6, -0.05, 0.0, &ctx);
        thumb[1] = -a1;
        let (_, tp) = self.gap_at(t.joints[1], t_local, &ctx);
        contacts.push(tp);

        let fitted = HandPose {
            fingers,
            thumb,
            thumb_base,
        };
        self.poses.insert(pose_name.to_string(), fitted);
        self.pose = fitted;
        self.pose_key = pose_name.to_string();
        contacts
    }

    /* ---- mask bakes ------------------------------------------------------ */

    /// BAKE CURVATURE MASKS ON THE WHOLE LIMB. `bakeSurfaceMasks(bake, shape,
    /// rng)`, `hands.js:897-919`.
    ///
    /// This is the fix for "a huge UNTEXTURED tan tube". Every weapon mesh has
    /// had wear/grime/AO vertex masks baked since the first build; the arms
    /// never did, so the shader read `vColor = (0,0,0)` and the wear, grime and
    /// cavity-AO layers of `sleeve`, `glove`, `glove_pad` and `glove_seam`
    /// were ALL switched off.
    ///
    /// `bake`/`shape` are `materials.bakeMasks` and the mask re-shaper, which
    /// live in the not-yet-ported `materials.js`; they are closures here, and
    /// the source's `rng` argument is whatever `bake` captures. The source's
    /// `if (!bake) return this` guard has no counterpart — a Rust caller that
    /// has no baker simply does not call this.
    ///
    /// The `done` set (`hands.js:904`) dedupes by *geometry identity*; every
    /// mesh in this rig owns its own geometry, so the set never fires — kept
    /// as the per-mesh visit it degenerates to.
    pub fn bake_surface_masks<B, S>(&mut self, mut bake: B, mut shape: S) -> &mut Self
    where
        B: FnMut(&mut Geo, BakeMaskOpts),
        S: FnMut(&mut Geo, MaskProfile),
    {
        let root = self.root;
        for node in self.traverse(root) {
            let Some(mi) = self.nodes[node].mesh else {
                continue;
            };
            let prof = match self.meshes[mi].surface {
                HandSurface::Sleeve => SLEEVE,
                HandSurface::Pad => PAD,
                HandSurface::Seam => SEAM,
                HandSurface::Glove => CLOTH,
            };
            bake(&mut self.meshes[mi].geo, BAKE_MASK_OPTS);
            shape(&mut self.meshes[mi].geo, prof);
        }
        self
    }

    /// Bake a contact-AO gradient into the GLOVE side of each contact.
    /// `bakeContactAO(contacts, radius, peak)`, `hands.js:937-969`.
    ///
    /// Geometric contact alone does not read as contact: two surfaces can be
    /// 0.5 mm apart and still look like two floating objects, because nothing
    /// in the lighting says they occlude each other. The mask goes in
    /// `vColor.b`, which `materials/shader.js` uses as
    /// `orm.r *= 1.0 - vColor.b * wear[2]`. Writing `(0, 0, ao)` leaves the
    /// wear and grime terms exactly as they were and only lights up the AO.
    ///
    /// The source's defaults are `radius = 0.012`, `peak = 0.9`; the one real
    /// caller passes `0.012, 0.7` (`viewmodel.js:482`).
    pub fn bake_contact_ao(&mut self, contacts: &[V3], radius: f64, peak: f64) -> &mut Self {
        if contacts.is_empty() {
            return self;
        }
        let root = self.root;
        self.update_world_matrix(root, false, true);
        let inv = self.nodes[root].matrix_world.invert();
        let r2 = radius * radius;
        let glove = self.glove;
        for node in self.traverse(glove) {
            let Some(mi) = self.nodes[node].mesh else {
                continue;
            };
            let count = self.meshes[mi].geo.vert_count();
            // `new Float32Array(pos.count * 3)` — zero-filled, f32-wide.
            if self.meshes[mi].color.len() != count * 3 {
                self.meshes[mi].color = vec![0.0f32; count * 3];
            }
            let m = M4::multiply_matrices(inv, self.nodes[node].matrix_world);
            for i in 0..count {
                let pos = &self.meshes[mi].geo.pos;
                let p = V3::new(
                    f64::from(pos[i * 3]),
                    f64::from(pos[i * 3 + 1]),
                    f64::from(pos[i * 3 + 2]),
                )
                .apply_matrix4(m);
                let mut closest = f64::INFINITY;
                for c in contacts {
                    let d2 = p.distance_to_squared(*c);
                    if d2 < closest {
                        closest = d2;
                    }
                }
                if closest > r2 {
                    continue;
                }
                let t = 1.0 - closest.sqrt() / radius;
                // smootherstep so the gradient has no visible terminator
                let s = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
                // `Math.max` runs in f64 on the value read back out of the
                // Float32Array; the store rounds to f32 again. Both halves of
                // that matter — see the module doc.
                let cur = f64::from(self.meshes[mi].color[i * 3 + 2]);
                self.meshes[mi].color[i * 3 + 2] = cur.max(peak * s) as f32;
            }
        }
        self
    }

    /* ---- posing ----------------------------------------------------------- */

    /// Static finger poses, by the source's string key. `setPose(name)`,
    /// `hands.js:972-983`. The lookup is
    /// `this.poses[name] ?? HAND_POSES[name] ?? HAND_POSES.wrap` — a fitted
    /// override wins, and an unknown key falls back to `wrap` (**not**
    /// `clamp`, which is [`Arm::fit_to_cylinder`]'s fallback).
    pub fn set_pose_key(&mut self, name: &str) -> &mut Self {
        let p = self
            .poses
            .get(name)
            .copied()
            .or_else(|| hand_pose_by_key(name))
            .unwrap_or_else(|| hand_pose(HandPoseName::Wrap));
        for i in 0..4 {
            let curl = p.fingers[i];
            for j in 0..3 {
                let joint = self.fingers[i].joints[j];
                self.set_node_rotation_x(joint, -curl[j]);
            }
        }
        let t = self.thumb;
        self.set_node_rotation_x(t.joints[0], -p.thumb[0]);
        self.set_node_rotation_x(t.joints[1], -p.thumb[1]);
        // `if (P.thumbBase)` — every authored pose and every fitted pose has
        // one, so the guard is always taken.
        let tb = p.thumb_base;
        self.set_node_rotation(t.root, tb[0], tb[1], tb[2]);
        self.pose = p;
        self.pose_key = name.to_string();
        self.trigger_curl = [-p.fingers[0][0], -p.fingers[0][1], -p.fingers[0][2]];
        self
    }

    /// [`Arm::set_pose_key`] for one of the six authored poses, keeping
    /// [`Arm::pose_name`] in step.
    pub fn set_pose(&mut self, name: HandPoseName) -> &mut Self {
        self.set_pose_key(name.key());
        self.pose_name = name;
        self
    }

    /// Trigger-finger curl, 0 = off the trigger, 1 = fully pressed.
    /// `setTrigger(t)`, `hands.js:986-993`. The rest pose (`t = 0`) matches
    /// `HAND_POSES.grip.fingers[0]`: the finger is already ON the trigger with
    /// the slack taken up, not standing off it straight.
    pub fn set_trigger(&mut self, t: f64) {
        let f = self.fingers[0];
        self.trigger_curl = [
            -(0.55 + t * 0.3),
            -(0.72 + t * 0.42),
            -(0.34 + t * 0.3),
        ];
        for j in 0..3 {
            let a = self.trigger_curl[j];
            self.set_node_rotation_x(f.joints[j], a);
        }
    }

    /* ---- IK --------------------------------------------------------------- */

    /// Orient a bone whose geometry runs along its local -Z so that -Z points
    /// along `dir`, with local +Y rolled toward `up`. `aimBone`,
    /// `hands.js:493-506`.
    ///
    /// This deliberately does NOT use `Object3D.lookAt()`: for non-camera
    /// objects lookAt aims local **+Z** at the target (so a -Z bone would
    /// point backwards), and it interprets the target in WORLD space, which is
    /// wrong here because every joint position is authored in the rig's local
    /// space.
    pub fn aim_bone(dir: V3, up: V3) -> Q {
        let bz = dir.scale(-1.0).normalize(); // local +Z is opposite the bone
        let mut by = up.add_scaled(bz, -up.dot(bz));
        if by.length_sq() < 1e-9 {
            // Degenerate roll reference: pick any axis that is not parallel to
            // the bone.
            by = V3::new(0.0, 1.0, 0.0).add_scaled(bz, -bz.y);
            if by.length_sq() < 1e-9 {
                by = V3::new(1.0, 0.0, 0.0).add_scaled(bz, -bz.x);
            }
        }
        by = by.normalize();
        let bx = by.cross(bz).normalize();
        Q::from_basis(bx, by, bz)
    }

    /// Solve the two-bone chain so the hand lands exactly on `target_pos`
    /// with orientation `target_quat`, elbow swung toward [`Arm::pole`].
    /// `solve(targetPos, targetQuat)`, `hands.js:999-1042`.
    ///
    /// `target_pos`/`target_quat` and [`Arm::shoulder`]/[`Arm::pole`] must all
    /// already be expressed in the same space (the arm root's parent space) —
    /// this method performs no space conversion itself.
    pub fn solve(&mut self, target_pos: V3, target_quat: Q) -> &mut Self {
        let hand = self.hand;
        self.nodes[hand].position = target_pos;
        self.nodes[hand].quaternion = target_quat;
        self.hand_pos = target_pos;
        self.hand_quat = target_quat;

        let mut t = target_pos.sub(self.shoulder);
        let mut d = t.length();
        let max_d = (self.l1 + self.l2) * 0.995;
        let min_d = (self.l1 - self.l2).abs() * 1.05 + 1e-4;
        if d > max_d {
            t = t.scale(max_d / d);
            d = max_d;
        } else if d < min_d {
            t = if d < 1e-5 {
                V3::new(0.0, 0.0, -min_d)
            } else {
                t.scale(min_d / d)
            };
            d = min_d;
        }
        // `_dir.copy(_t).divideScalar(d)`, and `Vector3.divideScalar(s)` is
        // `multiplyScalar(1 / s)` — the reciprocal is formed first, so this is
        // not `t.x / d`.
        let dir = t.scale(1.0 / d);

        // Circle of possible elbow positions; pick the point toward the pole.
        let a = (self.l1 * self.l1 - self.l2 * self.l2 + d * d) / (2.0 * d);
        let h = (self.l1 * self.l1 - a * a).max(0.0).sqrt();
        let pole = self.pole;
        let mut perp = pole.add_scaled(dir, -pole.dot(dir));
        if perp.length_sq() < 1e-8 {
            // `hands.js:1023`'s `_perp.set(side, -1, 0).addScaledVector(_dir, 0)`
            // adds a zero-scaled vector, which is a literal no-op; the real
            // projection is the line after it. Ported as the two statements
            // the source has, minus the arithmetic identity, which cannot
            // change a bit.
            let seed = V3::new(self.side, -1.0, 0.0);
            perp = seed.add_scaled(dir, -seed.dot(dir));
        }
        perp = perp.normalize();
        let elbow = self.shoulder.add_scaled(dir, a).add_scaled(perp, h);

        // Upper arm: shoulder -> elbow. The elbow pad sits on the bone's +Y,
        // which must end up on the OUTSIDE of the bend — that is the pole side.
        let shoulder = self.shoulder;
        let (upper_pivot, fore_pivot) = (self.upper_pivot, self.fore_pivot);
        self.upper_pos = shoulder;
        self.nodes[upper_pivot].position = shoulder;
        let hp = elbow.sub(shoulder);
        if hp.length_sq() > 1e-12 {
            let q = Self::aim_bone(hp, perp);
            self.upper_quat = q;
            self.nodes[upper_pivot].quaternion = q;
        }

        // Forearm: elbow -> wrist, rolled with the back of the hand so the
        // cuff and the wrist line up with the glove.
        self.fore_pos = elbow;
        self.nodes[fore_pivot].position = elbow;
        let up = V3::new(0.0, 1.0, 0.0).apply_quat(target_quat);
        let hp2 = target_pos.sub(elbow);
        if hp2.length_sq() > 1e-12 {
            let q = Self::aim_bone(hp2, up);
            self.fore_quat = q;
            self.nodes[fore_pivot].quaternion = q;
        }
        self.elbow = elbow;
        self
    }
}
