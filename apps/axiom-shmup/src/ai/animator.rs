//! Ported from Claude-of-Duty `src/ai/animator.js:1-559`.
//!
//! AI — the animation runtime: a small layered blend tree plus the three IK
//! solvers that make an armed character believable.
//!
//! ```text
//! LAYERS, in evaluation order
//!   1  locomotion base      idle / walk / run / crouch-walk / crouch-idle,
//!                           crossfaded, phase driven by real ground speed so
//!                           the feet never skate
//!   2  additive             aim, suppression flinch, reload body language,
//!                           firing recoil, per-region hit reactions
//!   3  one-shots            turn-in-place, vault (override weight ramps)
//!
//! IK, after the pose is written to the bones
//!   A  aim      — the spine chain is rotated until the weapon's bore points at
//!                 the aim target; the residual is spread over Spine1/Spine2 and
//!                 clamped, so the AI has to physically turn to cover a wide arc
//!   B  look-at  — neck and head lead the aim, clamped to human limits
//!   C  arm      — two-bone solve puts the support hand back on the handguard
//!                 (or on the magazine during a reload) after the spine moved
//!   D  foot     — ground probe per foot, pelvis drops to keep both feet
//!                 planted, two-bone solve per leg, sole aligned to the normal
//! ```
//!
//! ## The traps this file walks through, named
//!
//! - **`Float32Array` storage width.** `Poser.d3` is a `Float32Array`
//!   (`animator.js:35`) — every additive layer's contribution is *rounded to
//!   `f32` on store and read back rounded*, and the layers accumulate through
//!   that rounding. [`Poser::d3`] is `Vec<f32>` and [`Poser::d`] rounds on
//!   every `+=`, exactly as the source does. Porting it as `f64` diverges by
//!   ~1e-8 per bone per layer, which the aim IK then integrates.
//! - **Euler order is a convention, not a spelling.** Grepped, not assumed:
//!   `setFromEuler` appears exactly **once** in `rig.js`+`clips.js`+
//!   `animator.js`, at `animator.js:311-312`, and it passes an **explicit
//!   `'XYZ'`** (`e.set(x*DEG, y*DEG, z*DEG, 'XYZ')`), which THREE composes as
//!   `qx*qy*qz`. [`crate::weapons::rig_math::Q::from_euler_xyz`] is a
//!   line-for-line transcription of THREE's `case 'XYZ'` branch and is the
//!   *only* correct conversion here. `axiom_math::Quat::from_euler_xyz`
//!   composes `qz*qy*qx` and is a different rotation — deliberately not used.
//!
//!   `'YXZ'` **is** used elsewhere in `src/ai/` — `parts.js:44` and
//!   `weapon.js:28` — but nowhere in this slice. Do not share a helper across
//!   that boundary: they are different rotations for the same three angles.
//!
//!   The one other Euler in this stack is the actor group's
//!   `group.rotation.y = yaw` (`agent.js:927`). `Euler.DEFAULT_ORDER` is
//!   `'XYZ'` in three r180 (`math/Euler.js:446`, checked), and in any case a
//!   single-axis rotation is order-independent — every order yields the same
//!   quaternion. See [`Animator::set_actor`].
//! - **Matrix storage order.** [`Mat4`] below is THREE's `Matrix4`: a
//!   **column-major** `[f64; 16]`, indices exactly as `elements` — so `e[12]`,
//!   `e[13]`, `e[14]` are the translation (that is what `_wp`,
//!   `animator.js:328-331`, reads). A row-major transcription flips every
//!   off-diagonal sign and still compiles.
//! - **Float arithmetic is not associative.** Every product below is
//!   transcribed in the source's grouping and left-to-right order, including
//!   THREE's own `Matrix4`/`Quaternion` bodies, which are copied from
//!   `three@0.180`'s sources rather than re-derived.
//!
//! ## Shape of the port
//!
//! The source's preallocated scratch (`this._v`, `this._q`, `this._footY`,
//! `this._probeOut`, ...) exists to stop THREE allocating inside `update()`.
//! `V3`/`Q`/`Mat4` here are `Copy` value types, so the scratch slots become
//! ordinary locals. That is behaviour-preserving *only* because none of them
//! carries state across a call — checked slot by slot; the one place where
//! aliasing is load-bearing (`_applyWorld` inverting `_qa` **after** `cur` has
//! already been built from it, `animator.js:344-348`) is reproduced by
//! sequencing, not by aliasing.
//!
//! `THREE.Object3D`'s transform graph *is* ported, as [`Skeleton`]: an arena
//! of [`Node`]s with `position`/`quaternion`/`scale`/`matrix`/`matrixWorld`
//! and THREE's exact `updateMatrix`/`updateMatrixWorld`/`updateWorldMatrix`/
//! `getWorldQuaternion` semantics. There is no way to skip it — the IK is
//! written entirely in world space and reads back through the hierarchy every
//! step. Node `0` is the actor's `THREE.Group` (`agent.js:112-118`) and bone
//! `i` is node `i + 1`.
//!
//! `THREE.Skeleton` itself used to be left out here — "pure skinning state,
//! read only by the unported `SkinnedMesh`". That reason expired: Axiom's
//! `RunningApp::submit_skinned_draw` **is** the `SkinnedMesh`, so the state has
//! a reader. [`Skeleton::bind_inverses`] is `calculateInverses()` and
//! [`Animator::joint_palette`] is `update()`. The bone-matrix *texture* is still
//! not ported and never will be: that is a THREE upload detail, and the engine
//! owns its own joint-palette texture behind `submit_skinned_draw`.

use std::collections::HashMap;

use super::clips::{self, ClipId, HitRegion};
use super::grounding::{Foot, FootSource};
use super::rig::{Rig, BORE_DIR};
use crate::rng::Rng;
use crate::weapons::rig_math::{Q, V3};

/// `const DEG = Math.PI / 180`. `animator.js:30`.
const DEG: f64 = std::f64::consts::PI / 180.0;

/* ==================================================================== */
/* THREE.Matrix4, the part this file uses                               */
/* ==================================================================== */

/// `THREE.Matrix4` — **column-major** `elements`, transcribed from
/// `three@0.180`'s `math/Matrix4.js`.
///
/// This lives here rather than in [`crate::weapons::rig_math`] for the same
/// reason that module states for itself: the viewmodel rig never materialises
/// a `Matrix4`, and the animator's bone hierarchy is the only thing in the
/// port that does. The `V3`/`Q` halves *are* reused from there rather than
/// duplicated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// THREE's `elements`: column-major, so `e[12..15]` is the translation.
    pub e: [f64; 16],
}

impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        e: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// `Matrix4.compose(position, quaternion, scale)`.
    pub fn compose(position: V3, quaternion: Q, scale: V3) -> Mat4 {
        let (x, y, z, w) = (quaternion.x, quaternion.y, quaternion.z, quaternion.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);

        let (sx, sy, sz) = (scale.x, scale.y, scale.z);

        let mut e = [0.0f64; 16];
        e[0] = (1.0 - (yy + zz)) * sx;
        e[1] = (xy + wz) * sx;
        e[2] = (xz - wy) * sx;
        e[3] = 0.0;

        e[4] = (xy - wz) * sy;
        e[5] = (1.0 - (xx + zz)) * sy;
        e[6] = (yz + wx) * sy;
        e[7] = 0.0;

        e[8] = (xz + wy) * sz;
        e[9] = (yz - wx) * sz;
        e[10] = (1.0 - (xx + yy)) * sz;
        e[11] = 0.0;

        e[12] = position.x;
        e[13] = position.y;
        e[14] = position.z;
        e[15] = 1.0;
        Mat4 { e }
    }

    /// `Matrix4.multiplyMatrices(a, b)`.
    pub fn multiply_matrices(a: &Mat4, b: &Mat4) -> Mat4 {
        let ae = &a.e;
        let be = &b.e;
        let (a11, a12, a13, a14) = (ae[0], ae[4], ae[8], ae[12]);
        let (a21, a22, a23, a24) = (ae[1], ae[5], ae[9], ae[13]);
        let (a31, a32, a33, a34) = (ae[2], ae[6], ae[10], ae[14]);
        let (a41, a42, a43, a44) = (ae[3], ae[7], ae[11], ae[15]);

        let (b11, b12, b13, b14) = (be[0], be[4], be[8], be[12]);
        let (b21, b22, b23, b24) = (be[1], be[5], be[9], be[13]);
        let (b31, b32, b33, b34) = (be[2], be[6], be[10], be[14]);
        let (b41, b42, b43, b44) = (be[3], be[7], be[11], be[15]);

        let mut e = [0.0f64; 16];
        e[0] = a11 * b11 + a12 * b21 + a13 * b31 + a14 * b41;
        e[4] = a11 * b12 + a12 * b22 + a13 * b32 + a14 * b42;
        e[8] = a11 * b13 + a12 * b23 + a13 * b33 + a14 * b43;
        e[12] = a11 * b14 + a12 * b24 + a13 * b34 + a14 * b44;

        e[1] = a21 * b11 + a22 * b21 + a23 * b31 + a24 * b41;
        e[5] = a21 * b12 + a22 * b22 + a23 * b32 + a24 * b42;
        e[9] = a21 * b13 + a22 * b23 + a23 * b33 + a24 * b43;
        e[13] = a21 * b14 + a22 * b24 + a23 * b34 + a24 * b44;

        e[2] = a31 * b11 + a32 * b21 + a33 * b31 + a34 * b41;
        e[6] = a31 * b12 + a32 * b22 + a33 * b32 + a34 * b42;
        e[10] = a31 * b13 + a32 * b23 + a33 * b33 + a34 * b43;
        e[14] = a31 * b14 + a32 * b24 + a33 * b34 + a34 * b44;

        e[3] = a41 * b11 + a42 * b21 + a43 * b31 + a44 * b41;
        e[7] = a41 * b12 + a42 * b22 + a43 * b32 + a44 * b42;
        e[11] = a41 * b13 + a42 * b23 + a43 * b33 + a44 * b43;
        e[15] = a41 * b14 + a42 * b24 + a43 * b34 + a44 * b44;
        Mat4 { e }
    }

    /// `Matrix4.invert()`.
    ///
    /// Delegated to [`crate::weapons::rig_math::M4::invert`], which is already
    /// the element-for-element transcription of `three@0.180`'s cofactor
    /// expansion — **including** its singular case (`det === 0` returns the
    /// all-zero matrix). Both types are the same column-major `[f64; 16]`
    /// `elements`, so the hand-off is a field copy; writing the expansion out a
    /// second time would leave this port with two transcriptions of one source
    /// function that could silently drift apart.
    ///
    /// Its one caller is [`Skeleton::bind_inverses`] — `THREE.Skeleton`'s
    /// `boneInverses`, which the module header records as unported because
    /// nothing read it. Something does now.
    pub fn invert(self) -> Mat4 {
        Mat4 { e: crate::weapons::rig_math::M4 { e: self.e }.invert().e }
    }

    /// `Matrix4.determinant()` — needed only by [`Mat4::decompose`]'s
    /// negative-scale correction, transcribed with the source's exact term
    /// order (a reassociated determinant differs in the last bits).
    pub fn determinant(&self) -> f64 {
        let te = &self.e;
        let (n11, n12, n13, n14) = (te[0], te[4], te[8], te[12]);
        let (n21, n22, n23, n24) = (te[1], te[5], te[9], te[13]);
        let (n31, n32, n33, n34) = (te[2], te[6], te[10], te[14]);
        let (n41, n42, n43, n44) = (te[3], te[7], te[11], te[15]);

        n41 * (n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33
            + n13 * n22 * n34
            - n12 * n23 * n34)
            + n42
                * (n11 * n23 * n34 - n11 * n24 * n33 + n14 * n21 * n33 - n13 * n21 * n34
                    + n13 * n24 * n31
                    - n14 * n23 * n31)
            + n43
                * (n11 * n24 * n32 - n11 * n22 * n34 - n14 * n21 * n32 + n12 * n21 * n34
                    + n14 * n22 * n31
                    - n12 * n24 * n31)
            + n44
                * (-n13 * n22 * n31 - n11 * n23 * n32 + n11 * n22 * n33 + n13 * n21 * n32
                    - n12 * n21 * n33
                    + n12 * n23 * n31)
    }

    /// `Matrix4.decompose(position, quaternion, scale)` — returns the triple
    /// the source writes into its three out-parameters.
    pub fn decompose(&self) -> (V3, Q, V3) {
        let te = &self.e;
        let mut sx = V3::new(te[0], te[1], te[2]).length();
        let sy = V3::new(te[4], te[5], te[6]).length();
        let sz = V3::new(te[8], te[9], te[10]).length();

        // if determine is negative, we need to invert one scale
        let det = self.determinant();
        if det < 0.0 {
            sx = -sx;
        }

        let position = V3::new(te[12], te[13], te[14]);

        let inv_sx = 1.0 / sx;
        let inv_sy = 1.0 / sy;
        let inv_sz = 1.0 / sz;

        // `setFromRotationMatrix` reads the three *columns* of the rescaled
        // upper 3x3 — which is exactly what `Q::from_basis` takes.
        let bx = V3::new(te[0] * inv_sx, te[1] * inv_sx, te[2] * inv_sx);
        let by = V3::new(te[4] * inv_sy, te[5] * inv_sy, te[6] * inv_sy);
        let bz = V3::new(te[8] * inv_sz, te[9] * inv_sz, te[10] * inv_sz);
        let quaternion = Q::from_basis(bx, by, bz);

        (position, quaternion, V3::new(sx, sy, sz))
    }
}

/// `Vector3.applyMatrix4(m)` — the perspective divide is kept even though
/// every matrix here is affine (`w` is always 1), because the source keeps it.
pub fn apply_matrix4(v: V3, m: &Mat4) -> V3 {
    let (x, y, z) = (v.x, v.y, v.z);
    let e = &m.e;
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    V3::new(
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    )
}

/// `Quaternion.setFromAxisAngle(axis, angle)`.
pub fn quat_from_axis_angle(axis: V3, angle: f64) -> Q {
    let half_angle = angle / 2.0;
    let s = half_angle.sin();
    Q::new(axis.x * s, axis.y * s, axis.z * s, half_angle.cos())
}

/// `Quaternion.setFromUnitVectors(vFrom, vTo)` — `three@0.180`'s body,
/// including the `1e-8` opposite-direction epsilon and its axis choice.
pub fn quat_from_unit_vectors(v_from: V3, v_to: V3) -> Q {
    // assumes direction vectors vFrom and vTo are normalized
    let mut r = v_from.dot(v_to) + 1.0;
    let q = if r < 1e-8 {
        // vFrom and vTo point in opposite directions
        r = 0.0;
        if v_from.x.abs() > v_from.z.abs() {
            Q::new(-v_from.y, v_from.x, 0.0, r)
        } else {
            Q::new(0.0, -v_from.z, v_from.y, r)
        }
    } else {
        // crossVectors( vFrom, vTo ); // inlined to avoid cyclic dependency
        Q::new(
            v_from.y * v_to.z - v_from.z * v_to.y,
            v_from.z * v_to.x - v_from.x * v_to.z,
            v_from.x * v_to.y - v_from.y * v_to.x,
            r,
        )
    };
    q.normalize()
}

/// `Quaternion.premultiply(q)` — `multiplyQuaternions(q, this)`.
fn premultiply(this: Q, q: Q) -> Q {
    q.multiply(this)
}

/* ==================================================================== */
/* THREE.Object3D transform graph                                       */
/* ==================================================================== */

/// One `THREE.Object3D` in the actor's transform graph — the actor group or a
/// `THREE.Bone`. Only the transform half exists; nothing here renders.
#[derive(Debug, Clone)]
pub struct Node {
    pub position: V3,
    pub quaternion: Q,
    pub scale: V3,
    pub matrix: Mat4,
    pub matrix_world: Mat4,
    pub matrix_auto_update: bool,
    pub matrix_world_needs_update: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// The actor's transform graph. Node `0` is the actor's `THREE.Group`
/// (`agent.js:112-118`, position = the agent's world position, `rotation.y` =
/// its yaw, uniform `scale`); bone `i` of the rig is node `i + 1`.
///
/// `rig.createSkeleton()` (`rig.js:243-261`) is what builds this — see
/// [`super::rig`]'s module comment for why the bone hierarchy lives here and
/// the shared bind pose lives there.
pub struct Skeleton {
    pub nodes: Vec<Node>,
}

/// Bone index -> node index. The actor group occupies node 0.
#[inline]
fn node_of(bone: usize) -> usize {
    bone + 1
}

/// The actor group's node index (`this.bones[0].parent`).
const ACTOR: usize = 0;

impl Skeleton {
    /// The actor group plus `rig.createSkeleton()`'s bones, wired up and with
    /// world matrices current — `agent.js:104-131`'s construction order:
    /// bones built with `matrixAutoUpdate = false` and one `updateMatrix()`
    /// each, parented, then `group.updateMatrixWorld(true)`.
    pub fn new(rig: &Rig, scale: f64) -> Skeleton {
        let mut nodes = Vec::with_capacity(rig.count + 1);
        // node 0: `this.group` — `matrixAutoUpdate` left at THREE's default
        // `true`, so `updateMatrixWorld` recomposes it from position/
        // quaternion/scale, exactly as the source relies on in `_drive`.
        nodes.push(Node {
            position: V3::ZERO,
            quaternion: Q::IDENTITY,
            scale: V3::new(scale, scale, scale), // `group.scale.setScalar(this.scale)`
            matrix: Mat4::IDENTITY,
            matrix_world: Mat4::IDENTITY,
            matrix_auto_update: true,
            matrix_world_needs_update: false,
            parent: None,
            children: Vec::new(),
        });
        for i in 0..rig.count {
            nodes.push(Node {
                position: rig.local_pos[i],
                quaternion: rig.local_quat[i],
                scale: V3::new(1.0, 1.0, 1.0),
                matrix: Mat4::IDENTITY,
                matrix_world: Mat4::IDENTITY,
                matrix_auto_update: false, // `b.matrixAutoUpdate = false`
                matrix_world_needs_update: false,
                parent: None,
                children: Vec::new(),
            });
        }
        let mut sk = Skeleton { nodes };
        for i in 0..rig.count {
            sk.update_matrix(node_of(i)); // `b.updateMatrix()`
        }
        // `if (pi >= 0) bones[pi].add(bones[i])`, then `this.group.add(root)`.
        for i in 0..rig.count {
            let pi = rig.parent[i];
            let parent_node = if pi >= 0 { node_of(pi as usize) } else { ACTOR };
            sk.nodes[node_of(i)].parent = Some(parent_node);
            sk.nodes[parent_node].children.push(node_of(i));
        }
        sk.update_matrix_world(ACTOR, true);
        sk
    }

    /// `Object3D.updateMatrix()`.
    pub fn update_matrix(&mut self, i: usize) {
        let n = &self.nodes[i];
        let m = Mat4::compose(n.position, n.quaternion, n.scale);
        let n = &mut self.nodes[i];
        n.matrix = m;
        n.matrix_world_needs_update = true;
    }

    /// `Object3D.updateMatrixWorld(force)`.
    pub fn update_matrix_world(&mut self, i: usize, force: bool) {
        let mut force = force;
        if self.nodes[i].matrix_auto_update {
            self.update_matrix(i);
        }
        if self.nodes[i].matrix_world_needs_update || force {
            self.nodes[i].matrix_world = match self.nodes[i].parent {
                None => self.nodes[i].matrix,
                Some(p) => {
                    let pm = self.nodes[p].matrix_world;
                    Mat4::multiply_matrices(&pm, &self.nodes[i].matrix)
                }
            };
            self.nodes[i].matrix_world_needs_update = false;
            force = true;
        }
        for ci in 0..self.nodes[i].children.len() {
            let c = self.nodes[i].children[ci];
            self.update_matrix_world(c, force);
        }
    }

    /// `Object3D.updateWorldMatrix(true, false)` — the form
    /// `getWorldQuaternion` calls. Note it does **not** clear
    /// `matrixWorldNeedsUpdate`; the source doesn't either.
    pub fn update_world_matrix_parents(&mut self, i: usize) {
        if let Some(p) = self.nodes[i].parent {
            self.update_world_matrix_parents(p);
        }
        if self.nodes[i].matrix_auto_update {
            self.update_matrix(i);
        }
        self.nodes[i].matrix_world = match self.nodes[i].parent {
            None => self.nodes[i].matrix,
            Some(p) => {
                let pm = self.nodes[p].matrix_world;
                Mat4::multiply_matrices(&pm, &self.nodes[i].matrix)
            }
        };
    }

    /// `Object3D.getWorldQuaternion(target)` — including its side effect of
    /// refreshing this node's and its ancestors' `matrixWorld`.
    pub fn world_quaternion(&mut self, i: usize) -> Q {
        self.update_world_matrix_parents(i);
        let (_pos, q, _scale) = self.nodes[i].matrix_world.decompose();
        q
    }

    /// `THREE.Skeleton.calculateInverses()` (`three@0.180`,
    /// `objects/Skeleton.js`): one `bone.matrixWorld.clone().invert()` per bone,
    /// taken with the skeleton **in its bind pose and the actor group left at
    /// the identity**.
    ///
    /// This is the half of `THREE.Skeleton` the module header records as
    /// deliberately unported — "pure skinning state, read only by the unported
    /// `SkinnedMesh`". Axiom's `RunningApp::submit_skinned_draw` **is** that
    /// `SkinnedMesh`, so the state has a reader now and belongs here, beside the
    /// bone hierarchy whose matrices it pairs with.
    ///
    /// **Why the group is left at the identity.** `super::geo`'s
    /// `CharacterGeometry.position` is authored in the rig's own bind space
    /// (`rig.bind_pos`: feet on `y = 0`, facing `+Z`, no group transform), so an
    /// inverse taken in that same space is the one those vertices pair with —
    /// `bone.matrixWorld * inverse` then carries a bind-space vertex all the way
    /// to world, actor position, yaw and uniform scale included, and the draw
    /// itself needs no world transform at all. THREE reaches the same place by a
    /// different route, cancelling the group with the `SkinnedMesh`'s
    /// `bindMatrix`/`bindMatrixInverse` pair; taking the inverse at the identity
    /// makes both of those the identity and removes the pair entirely.
    ///
    /// The rig is one shared static, so this is one table for every soldier in
    /// the level — compute it once, at install.
    #[must_use]
    pub fn bind_inverses(rig: &Rig) -> Vec<Mat4> {
        let bind = Skeleton::new(rig, 1.0);
        (0..rig.count)
            .map(|i| bind.nodes[node_of(i)].matrix_world.invert())
            .collect()
    }
}

/* ==================================================================== */
/* Poser                                                                */
/* ==================================================================== */

/// Pose accumulator handed to clip functions. `class Poser`,
/// `animator.js:33-66`.
pub struct Poser {
    /// `new Float32Array(rig.count * 3)` — **`f32`, and that is part of the
    /// algorithm.** Every layer's `+=` rounds to `f32` on store; see the
    /// module comment.
    pub d3: Vec<f32>,
    pub hip_off: V3,
    /// Layer weight applied to every `d`/`hip` contribution.
    pub w: f64,
    /// name -> index cache for the clip helpers (`this._idx`).
    idx: HashMap<&'static str, usize>,
}

impl Poser {
    /// `constructor(rig)`. `animator.js:34-42`.
    pub fn new(rig: &Rig) -> Poser {
        let mut idx = HashMap::with_capacity(rig.count);
        for i in 0..rig.count {
            idx.insert(rig.names[i], i);
        }
        Poser { d3: vec![0.0f32; rig.count * 3], hip_off: V3::ZERO, w: 1.0, idx }
    }

    /// `reset()`. `animator.js:44-48`.
    pub fn reset(&mut self) {
        self.d3.fill(0.0);
        self.hip_off = V3::ZERO;
        self.w = 1.0;
    }

    /// Additive euler delta in degrees. `animator.js:51-58`.
    ///
    /// The read-modify-write goes through `f32` in both directions, matching
    /// `Float32Array`'s `+=`: read (widen to `f64`), add in `f64`, store
    /// (round to `f32`).
    pub fn d(&mut self, name: &str, x: f64, y: f64, z: f64) {
        let Some(&i) = self.idx.get(name) else { return };
        let w = self.w;
        self.d3[i * 3] = (self.d3[i * 3] as f64 + x * w) as f32;
        self.d3[i * 3 + 1] = (self.d3[i * 3 + 1] as f64 + y * w) as f32;
        self.d3[i * 3 + 2] = (self.d3[i * 3 + 2] as f64 + z * w) as f32;
    }

    /// `hip(dx, dy, dz)`. `animator.js:60-65`. `hipOff` is a
    /// `THREE.Vector3` — plain JS numbers, so `f64`, unlike `d3`.
    pub fn hip(&mut self, dx: f64, dy: f64, dz: f64) {
        let w = self.w;
        self.hip_off = V3::new(
            self.hip_off.x + dx * w,
            self.hip_off.y + dy * w,
            self.hip_off.z + dz * w,
        );
    }
}

/* ==================================================================== */
/* Animator inputs                                                      */
/* ==================================================================== */

/// The four bind-space weapon anchors `Animator` reads off `def.weapon`
/// (`ai/weapon.js:284-289`, each a `Vector3.toArray()`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponAnchors {
    pub muzzle: [f64; 3],
    pub foregrip: [f64; 3],
    pub mag_bottom: [f64; 3],
    pub ejection: [f64; 3],
}

/// `out` of `probeGround(x, z, fromY, out)` (`ai/index.js:433-444`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeOut {
    pub y: f64,
    pub nx: f64,
    pub ny: f64,
    pub nz: f64,
    pub hit: bool,
}

impl Default for ProbeOut {
    /// `this._probeOut = { y: 0, nx: 0, ny: 1, nz: 0, hit: false }`.
    /// `animator.js:160`.
    fn default() -> Self {
        ProbeOut { y: 0.0, nx: 0.0, ny: 1.0, nz: 0.0, hit: false }
    }
}

/// `opts.probe` — the floor probe foot IK rays against
/// (`agent.js:138`, `ai/index.js:433`). Narrowed to the one call the animator
/// makes, in the same spirit as [`super::grounding::FootSource`]: the physics
/// raycast behind it is a different slice.
pub trait GroundProbe {
    fn probe(&self, x: f64, z: f64, from_y: f64, out: &mut ProbeOut) -> bool;
}

/// `this.state`. `animator.js:85-94`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimState {
    pub clip: ClipId,
    pub speed: f64,
    pub crouch: bool,
    pub aim_target: Option<V3>,
    pub look_target: Option<V3>,
    pub aim_weight: f64,
    pub suppress: f64,
    /// Set by `setState` and never read anywhere in `animator.js` — a dead
    /// field in the source, kept so the state block diffs against it.
    pub hurt: f64,
}

impl Default for AnimState {
    fn default() -> Self {
        AnimState {
            clip: ClipId::Idle,
            speed: 0.0,
            crouch: false,
            aim_target: None,
            look_target: None,
            aim_weight: 1.0,
            suppress: 0.0,
            hurt: 0.0,
        }
    }
}

/// The partial state `setState(s)` merges (`animator.js:169-183`). Each field
/// is `Some` where the JS object has the key defined; the nested
/// `Option<Option<V3>>` for the two targets is not ceremony — `{aimTarget:
/// null}` is a *defined* value that clears the target, which is different
/// from omitting the key.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StateUpdate {
    pub clip: Option<ClipId>,
    pub speed: Option<f64>,
    pub crouch: Option<bool>,
    pub aim_target: Option<Option<V3>>,
    pub look_target: Option<Option<V3>>,
    pub aim_weight: Option<f64>,
    pub suppress: Option<f64>,
    pub hurt: Option<f64>,
}

/* ==================================================================== */
/* Animator                                                             */
/* ==================================================================== */

/// `class Animator`. `animator.js:68-559`.
pub struct Animator {
    pub bones: Skeleton,
    pub weapon: Option<WeaponAnchors>,
    /// `opts.rng` (`agent.js:136`, `rng: this.rng.fork()`). The fork happens
    /// on the *agent*'s stream, so the draw-order contract lives there; the
    /// animator stores the forked generator and — checked — never draws from
    /// it anywhere in `animator.js`. Kept so a later slice that does draw
    /// from it inherits the same stream.
    pub rng: Option<Rng>,
    pub probe: Option<Box<dyn GroundProbe>>,
    pub scale: f64,
    pub enabled: bool,

    pub p: Poser,
    pub state: AnimState,

    pub phase: f64,
    pub prev_clip: ClipId,
    /// Weight of the current clip vs the previous one.
    pub blend: f64,
    /// `this.time = now` in `update`, and read nowhere in the source.
    pub time: f64,

    /* one-shot timers (negative = inactive) */
    pub recoil_t: f64,
    pub recoil_k: f64,
    pub hit_t: f64,
    pub hit_region: HitRegion,
    pub hit_side: f64,
    pub hit_k: f64,
    pub reload_t: f64,
    pub reload_dur: f64,
    pub vault_t: f64,
    pub vault_dur: f64,
    pub turn_t: f64,
    pub turn_dir: f64,
    pub foot_ik: bool,

    /* indices we touch often */
    i_hips: usize,
    i_spine: usize,
    i_spine1: usize,
    i_spine2: usize,
    i_neck: usize,
    i_head: usize,
    i_hand_r: usize,
    arm_l: [usize; 3],
    arm_r: [usize; 3],
    legs: [[usize; 3]; 2],

    /* weapon anchors, expressed in HandR bind-local space */
    pub bore_local: V3,
    pub muzzle_local: V3,
    pub foregrip_local: V3,
    pub mag_local: V3,
    pub eject_local: V3,

    /// `this._aimApplied = 0` (`animator.js:163`) — written once at
    /// construction and never read; dead in the source, kept.
    #[allow(dead_code)]
    aim_applied: f64,

    pub muzzle_world: V3,
    pub muzzle_dir: V3,
    pub eject_world: V3,

    /// The bone-name table, so `bone_pos` can resolve without borrowing the
    /// shared `RIG` static at every call.
    names: Vec<&'static str>,
    name_index: HashMap<&'static str, usize>,

    /// `rig.localPos` / `rig.localQuat`, the two arrays `_writePose` reads
    /// every frame (`animator.js:313-321`). Copied rather than borrowed: the
    /// source holds `this.rig`, but a `&Rig` field would infect every caller
    /// with a lifetime for two immutable tables that are 25 entries long and
    /// identical for every soldier.
    local_pos: Vec<V3>,
    local_quat: Vec<Q>,
}

impl Animator {
    /// `constructor(rig, bones, opts)`. `animator.js:74-167`.
    ///
    /// `bones` is not an argument here: the skeleton is built from `rig`
    /// (which is what `rig.createSkeleton()` does immediately before, at
    /// `agent.js:104`) and owned by the animator, because the IK mutates it
    /// on every frame and Rust has no second owner to hand it to. `scale` is
    /// both `opts.scale` and the actor group's uniform scale
    /// (`agent.js:117-118` sets them from the same number).
    pub fn new(
        rig: &Rig,
        weapon: Option<WeaponAnchors>,
        rng: Option<Rng>,
        probe: Option<Box<dyn GroundProbe>>,
        scale: f64,
    ) -> Animator {
        let bones = Skeleton::new(rig, scale);
        let p = Poser::new(rig);

        let i_hand_r = rig.index("HandR");

        /* ---- weapon anchors, expressed in HandR bind-local space ---- */
        let q_inv = rig.bind_quat[i_hand_r].invert();
        let hand_pos = rig.bind_pos[i_hand_r];
        let to_local = |p: [f64; 3]| {
            V3::new(p[0] - hand_pos.x, p[1] - hand_pos.y, p[2] - hand_pos.z).apply_quat(q_inv)
        };
        let bore_local = V3::from_array(*BORE_DIR).apply_quat(q_inv).normalize();
        let muzzle_local = match weapon {
            Some(w) => to_local(w.muzzle),
            None => V3::new(0.0, 0.0, 0.4),
        };
        let foregrip_local = match weapon {
            Some(w) => to_local(w.foregrip),
            None => V3::new(0.0, 0.0, 0.2),
        };
        let mag_local = match weapon {
            Some(w) => to_local(w.mag_bottom),
            None => V3::new(0.0, -0.2, 0.0),
        };
        let eject_local = match weapon {
            Some(w) => to_local(w.ejection),
            None => V3::new(0.0, 0.0, 0.0),
        };

        let mut name_index = HashMap::with_capacity(rig.count);
        for i in 0..rig.count {
            name_index.insert(rig.names[i], i);
        }

        Animator {
            bones,
            weapon,
            rng,
            probe,
            scale,
            enabled: true,
            p,
            state: AnimState::default(),
            phase: 0.0,
            prev_clip: ClipId::Idle,
            blend: 1.0,
            time: 0.0,
            recoil_t: -1.0,
            recoil_k: 1.0,
            hit_t: -1.0,
            hit_region: HitRegion::Torso,
            hit_side: 1.0,
            hit_k: 1.0,
            reload_t: -1.0,
            reload_dur: 2.4,
            vault_t: -1.0,
            vault_dur: 0.85,
            turn_t: -1.0,
            turn_dir: 1.0,
            foot_ik: true,
            i_hips: rig.index("Hips"),
            i_spine: rig.index("Spine"),
            i_spine1: rig.index("Spine1"),
            i_spine2: rig.index("Spine2"),
            i_neck: rig.index("Neck"),
            i_head: rig.index("Head"),
            i_hand_r,
            arm_l: [rig.index("UpperArmL"), rig.index("ForearmL"), rig.index("HandL")],
            arm_r: [rig.index("UpperArmR"), rig.index("ForearmR"), rig.index("HandR")],
            legs: [
                [rig.index("UpLegR"), rig.index("LegR"), rig.index("FootR")],
                [rig.index("UpLegL"), rig.index("LegL"), rig.index("FootL")],
            ],
            bore_local,
            muzzle_local,
            foregrip_local,
            mag_local,
            eject_local,
            aim_applied: 0.0,
            muzzle_world: V3::ZERO,
            muzzle_dir: V3::new(0.0, 0.0, 1.0),
            eject_world: V3::ZERO,
            names: (0..rig.count).map(|i| rig.names[i]).collect(),
            name_index,
            local_pos: rig.local_pos.clone(),
            local_quat: rig.local_quat.clone(),
        }
    }

    /// `agent.js:925-927` (`_drive`): place the actor group and refresh the
    /// whole graph before the animator runs. Not a method on the source's
    /// `Animator` — but the animator owns the group node here, so the agent's
    /// three lines land as one call.
    ///
    /// `group.rotation.y = yaw` goes through `Euler`'s **default `'XYZ'`
    /// order** — the second Euler-order site in this stack (see the module
    /// comment).
    pub fn set_actor(&mut self, position: V3, yaw: f64) {
        self.bones.nodes[ACTOR].position = position;
        self.bones.nodes[ACTOR].quaternion = Q::from_euler_xyz(0.0, yaw, 0.0);
        self.bones.update_matrix_world(ACTOR, true);
    }

    /// `setState(s)`. `animator.js:169-183`.
    pub fn set_state(&mut self, s: StateUpdate) {
        if let Some(clip) = s.clip {
            if clip != self.state.clip {
                self.prev_clip = self.state.clip;
                self.blend = 0.0;
                self.state.clip = clip;
            }
        }
        if let Some(v) = s.speed {
            self.state.speed = v;
        }
        if let Some(v) = s.crouch {
            self.state.crouch = v;
        }
        if let Some(v) = s.aim_target {
            self.state.aim_target = v;
        }
        if let Some(v) = s.look_target {
            self.state.look_target = v;
        }
        if let Some(v) = s.aim_weight {
            self.state.aim_weight = v;
        }
        if let Some(v) = s.suppress {
            self.state.suppress = v;
        }
        if let Some(v) = s.hurt {
            self.state.hurt = v;
        }
    }

    /* ---------------- one-shot triggers ---------------- */

    /// `fire(strength = 1)`. `animator.js:187-190`.
    pub fn fire(&mut self, strength: f64) {
        self.recoil_t = 0.0;
        self.recoil_k = strength;
    }

    /// `hit(region = 'torso', side = 1, strength = 1)`. `animator.js:192-197`.
    pub fn hit(&mut self, region: HitRegion, side: f64, strength: f64) {
        self.hit_t = 0.0;
        self.hit_region = region;
        self.hit_side = side;
        self.hit_k = strength;
    }

    /// `reload(duration = 2.4)`. `animator.js:199-202`.
    pub fn reload(&mut self, duration: f64) {
        self.reload_t = 0.0;
        self.reload_dur = duration;
    }

    /// `get reloading`. `animator.js:204-206`.
    pub fn reloading(&self) -> bool {
        self.reload_t >= 0.0
    }

    /// `vault(duration = 0.85)`. `animator.js:208-211`.
    pub fn vault(&mut self, duration: f64) {
        self.vault_t = 0.0;
        self.vault_dur = duration;
    }

    /// `get vaulting`. `animator.js:213-215`.
    pub fn vaulting(&self) -> bool {
        self.vault_t >= 0.0
    }

    /// `turn(dir)`. `animator.js:217-221` — a turn already running wins.
    pub fn turn(&mut self, dir: f64) {
        if self.turn_t >= 0.0 {
            return;
        }
        self.turn_t = 0.0;
        self.turn_dir = if dir >= 0.0 { 1.0 } else { -1.0 };
    }

    /* ---------------- main ---------------- */

    /// `update(dt, now)`. `animator.js:225-299`.
    pub fn update(&mut self, dt: f64, now: f64) {
        if !self.enabled {
            return;
        }
        self.time = now;

        /* --- phase advance: stride length keeps the feet stuck to the ground --- */
        let clip = self.state.clip;
        let stride_hz = match clip {
            ClipId::Run => (1.1f64).max(self.state.speed / 2.05),
            ClipId::Walk => (0.55f64).max(self.state.speed / 1.42),
            ClipId::CrouchWalk => (0.4f64).max(self.state.speed / 0.95),
            _ => 0.19, // idle breathing rate
        };
        // JS `%` on doubles is a remainder that keeps the sign of the
        // dividend — `f64::rem_euclid` would differ for a negative phase, and
        // `%` in Rust is the same truncated remainder JS uses.
        self.phase = (self.phase + dt * stride_hz) % 1.0;
        if self.blend < 1.0 {
            self.blend = (1.0f64).min(self.blend + dt / 0.18);
        }

        /* --- layer 1: locomotion, crossfaded --- */
        self.p.reset();
        let prev = self.prev_clip;
        let phase = self.phase;
        let blend = self.blend;
        if blend < 1.0 {
            self.p.w = 1.0 - blend;
            prev.eval(&mut self.p, phase);
            self.p.w = blend;
            clip.eval(&mut self.p, phase);
        } else {
            self.p.w = 1.0;
            clip.eval(&mut self.p, phase);
        }

        /* --- layer 2: additives --- */
        self.p.w = 1.0;
        if self.state.aim_weight > 0.0 && !self.vaulting() {
            let damp = if self.reload_t >= 0.0 { 0.6 } else { 0.0 };
            let w = self.state.aim_weight * (1.0 - damp);
            clips::aim_add(&mut self.p, w);
        }
        if self.state.suppress > 0.0 {
            let w = (1.0f64).min(self.state.suppress);
            clips::suppress_add(&mut self.p, w);
        }
        if self.recoil_t >= 0.0 {
            let (t, k) = (self.recoil_t, self.recoil_k);
            clips::recoil_add(&mut self.p, t, k);
            self.recoil_t += dt;
            if self.recoil_t > 0.3 {
                self.recoil_t = -1.0;
            }
        }
        if self.hit_t >= 0.0 {
            let (region, t, side, k) = (self.hit_region, self.hit_t, self.hit_side, self.hit_k);
            clips::hit_add(&mut self.p, region, t, side, k);
            self.hit_t += dt;
            if self.hit_t > 0.55 {
                self.hit_t = -1.0;
            }
        }
        if self.reload_t >= 0.0 {
            let t = self.reload_t / self.reload_dur;
            clips::reload_add(&mut self.p, t);
            self.reload_t += dt;
            if self.reload_t > self.reload_dur {
                self.reload_t = -1.0;
            }
        }
        if self.turn_t >= 0.0 {
            let (t, dir) = (self.turn_t / 0.42, self.turn_dir);
            clips::turn_step(&mut self.p, t, dir);
            self.turn_t += dt;
            if self.turn_t > 0.42 {
                self.turn_t = -1.0;
            }
        }
        if self.vault_t >= 0.0 {
            let t = self.vault_t / self.vault_dur;
            self.p.w = 1.0;
            clips::vault(&mut self.p, t);
            self.vault_t += dt;
            if self.vault_t > self.vault_dur {
                self.vault_t = -1.0;
            }
        }

        /* --- write the pose --- */
        self.write_pose();

        /* --- IK --- */
        self.bones.update_matrix_world(node_of(0), true);

        if self.foot_ik && !self.vaulting() {
            self.foot_ik_solve();
        }
        if self.state.aim_target.is_some() && self.state.aim_weight > 0.01 && !self.vaulting() {
            let t = self.state.aim_target.expect("checked is_some");
            let w = self.state.aim_weight;
            self.aim_ik(t, w);
        }
        if self.state.look_target.is_some() {
            let t = self.state.look_target.expect("checked is_some");
            let w = (0.35f64).max(self.state.aim_weight);
            self.look_at(t, w);
        }
        self.support_hand_ik();
        self.update_muzzle();
    }

    /// `_writePose()`. `animator.js:301-324`.
    fn write_pose(&mut self) {
        let count = self.names.len();
        for i in 0..count {
            let x = self.p.d3[i * 3] as f64;
            let y = self.p.d3[i * 3 + 1] as f64;
            let z = self.p.d3[i * 3 + 2] as f64;
            let ni = node_of(i);
            // `if (x || y || z)` — JS truthiness: `0`, `-0` and `NaN` are all
            // falsy. `x != 0.0` already treats `-0.0` as zero in Rust; the
            // `is_nan` guard is what makes it match on `NaN`.
            let quat = if truthy(x) || truthy(y) || truthy(z) {
                // `e.set(x * DEG, y * DEG, z * DEG, 'XYZ')` — THREE's 'XYZ'.
                let q = Q::from_euler_xyz(x * DEG, y * DEG, z * DEG);
                // `b.quaternion.copy(rig.localQuat[i]).multiply(q)`
                self.local_quat[i].multiply(q)
            } else {
                self.local_quat[i]
            };
            let pos = if i == 0 {
                self.local_pos[i].add(self.p.hip_off)
            } else {
                self.local_pos[i]
            };
            self.bones.nodes[ni].quaternion = quat;
            self.bones.nodes[ni].position = pos;
            self.bones.update_matrix(ni);
        }
    }

    /* ---------------- helpers on world transforms ---------------- */

    /// `_wp(i, out)`. `animator.js:328-331` — the translation columns of
    /// `matrixWorld`, i.e. `e[12], e[13], e[14]` (column-major).
    fn wp(&self, i: usize) -> V3 {
        let m = &self.bones.nodes[node_of(i)].matrix_world.e;
        V3::new(m[12], m[13], m[14])
    }

    /// `_wq(i, out)`. `animator.js:333-335`.
    fn wq(&mut self, i: usize) -> Q {
        self.bones.world_quaternion(node_of(i))
    }

    /// Rotate bone `i` in world space by quaternion `dq`, keeping its
    /// position. `_applyWorld`. `animator.js:338-351`.
    fn apply_world(&mut self, i: usize, dq: Q) {
        let ni = node_of(i);
        let parent = self.bones.nodes[ni].parent;
        let q = match parent {
            Some(p) => self.bones.world_quaternion(p),
            None => Q::IDENTITY,
        };
        let cur = q.multiply(self.bones.nodes[ni].quaternion); // current world
        let cur = premultiply(cur, dq);
        // `q.invert()` mutates `q` in place in the source, *after* `cur` has
        // already been built from it — reproduced by sequencing.
        self.bones.nodes[ni].quaternion = q.invert().multiply(cur);
        self.bones.update_matrix(ni);
        self.bones.update_matrix_world(ni, true);
    }

    /// Point bone `i`'s +Y axis along `dir` (world, unit), preserving twist.
    /// `_aimBone`. `animator.js:354-361`.
    fn aim_bone(&mut self, i: usize, dir: V3) {
        let wq = self.wq(i);
        let cur = V3::new(0.0, 1.0, 0.0).apply_quat(wq);
        let dq = quat_from_unit_vectors(cur, dir);
        self.apply_world(i, dq);
    }

    /* ---------------- A: aim ---------------- */

    /// `_aimIk(target, weight)`. `animator.js:365-392`.
    fn aim_ik(&mut self, target: V3, weight: f64) {
        let spread = [(self.i_spine, 0.12), (self.i_spine1, 0.34), (self.i_spine2, 0.54)];
        for iter in 0..2 {
            // Order matters: the source calls `_wq` *first* (`animator.js:373`
            // — and `getWorldQuaternion` refreshes `matrixWorld` from the
            // parent chain as a side effect) and reads `hand.matrixWorld`
            // only on the next line (`:374`).
            let hand_q = self.wq(self.i_hand_r);
            let hand_world = self.bones.nodes[node_of(self.i_hand_r)].matrix_world;
            let bore = self.bore_local.apply_quat(hand_q).normalize();
            let muzzle = apply_matrix4(self.muzzle_local, &hand_world);
            let want = target.sub(muzzle);
            if want.length_sq() < 1e-6 {
                return;
            }
            let want = want.normalize();
            let dot = (1.0f64).min((-1.0f64).max(bore.dot(want)));
            let mut ang = dot.acos() * weight;
            if ang < 0.0015 {
                return;
            }
            // clamp how far the spine will twist before the body has to turn
            let max_this_iter = if iter == 0 { 0.9 } else { 0.35 };
            if ang > max_this_iter {
                ang = max_this_iter;
            }
            let axis = bore.cross(want);
            if axis.length_sq() < 1e-10 {
                return;
            }
            let axis = axis.normalize();
            for (bi, f) in spread {
                let q3 = quat_from_axis_angle(axis, ang * f);
                self.apply_world(bi, q3);
            }
        }
    }

    /* ---------------- B: look-at ---------------- */

    /// `_lookAt(target, weight)`. `animator.js:396-418`.
    ///
    /// Note the mixed exits: a degenerate `want` `return`s out of the whole
    /// chain (the head never solves), while a tiny angle or a degenerate axis
    /// only `continue`s. Carried as written.
    fn look_at(&mut self, target: V3, weight: f64) {
        // the head's forward is its local +Z
        let chain = [(self.i_neck, 0.4), (self.i_head, 0.6)];
        for (bi, f) in chain {
            let wq = self.wq(bi);
            let fwd = V3::new(0.0, 0.0, 1.0).apply_quat(wq);
            let want = target.sub(self.wp(bi));
            if want.length_sq() < 1e-6 {
                return;
            }
            let want = want.normalize();
            let dot = (1.0f64).min((-1.0f64).max(fwd.dot(want)));
            let mut ang = dot.acos() * weight * f;
            if ang < 0.002 {
                continue;
            }
            if ang > 0.5 {
                ang = 0.5; // ~29 deg per bone per frame cap
            }
            let axis = fwd.cross(want);
            if axis.length_sq() < 1e-10 {
                continue;
            }
            let axis = axis.normalize();
            let q3 = quat_from_axis_angle(axis, ang);
            self.apply_world(bi, q3);
        }
    }

    /* ---------------- C: support hand ---------------- */

    /// `_supportHandIk()`. `animator.js:422-444`.
    fn support_hand_ik(&mut self) {
        let hand_world = self.bones.nodes[node_of(self.i_hand_r)].matrix_world;
        if self.vaulting() {
            return;
        }
        let t = if self.reload_t >= 0.0 {
            let p = self.reload_t / self.reload_dur;
            // magwell -> chest pouch -> magwell -> slap -> back to the handguard
            let mag = apply_matrix4(self.mag_local, &hand_world);
            let grip = apply_matrix4(self.foregrip_local, &hand_world);
            // chest pouch in world space, from the actor root
            let actor_world = self.bones.nodes[ACTOR].matrix_world;
            let chest = apply_matrix4(V3::new(0.02, 1.19, 0.17), &actor_world);
            if p < 0.18 {
                grip.lerp(mag, p / 0.18)
            } else if p < 0.42 {
                mag.lerp(chest, (p - 0.18) / 0.24)
            } else if p < 0.62 {
                chest.lerp(mag, (p - 0.42) / 0.20)
            } else if p < 0.78 {
                mag
            } else {
                mag.lerp(grip, (p - 0.78) / 0.22)
            }
        } else {
            apply_matrix4(self.foregrip_local, &hand_world)
        };
        // pole: elbow down and out to the character's left
        let actor_q = self.bones.world_quaternion(ACTOR);
        let pole = V3::new(0.6, -1.0, -0.25).apply_quat(actor_q);
        let arm_l = self.arm_l;
        self.two_bone(arm_l, t, pole);
    }

    /* ---------------- D: feet ---------------- */

    /// `_footIk()`. `animator.js:448-499`.
    ///
    /// `this._probeOut`, `this._footY` and `this._footN` are per-call scratch
    /// in the source (nothing reads them between frames), so they are locals
    /// here.
    fn foot_ik_solve(&mut self) {
        if self.probe.is_none() {
            return;
        }
        let s = self.scale;
        let ankle_h = 0.088 * s;
        let mut drop = 0.0f64;
        let mut foot_y = [0.0f64; 2];
        let mut foot_n = [V3::new(0.0, 1.0, 0.0), V3::new(0.0, 1.0, 0.0)];
        for k in 0..2 {
            let ankle = self.wp(self.legs[k][2]);
            let mut out = ProbeOut::default();
            let ok = self
                .probe
                .as_ref()
                .expect("checked is_none above")
                .probe(ankle.x, ankle.z, ankle.y + 0.55 * s, &mut out);
            if !ok {
                foot_y[k] = ankle.y;
                foot_n[k] = V3::new(0.0, 1.0, 0.0);
                continue;
            }
            let want = out.y + ankle_h;
            foot_y[k] = want;
            foot_n[k] = V3::new(out.nx, out.ny, out.nz);
            let d = want - ankle.y;
            if d < drop {
                drop = d;
            }
        }
        drop = (-0.32 * s).max(drop);
        if drop < -0.002 {
            let b = node_of(0);
            // `b.position.y += drop / s` — hips offset is in actor-local space
            let p0 = self.bones.nodes[b].position;
            self.bones.nodes[b].position = V3::new(p0.x, p0.y + drop / s, p0.z);
            self.bones.update_matrix(b);
            self.bones.update_matrix_world(b, true);
        }
        for k in 0..2 {
            let leg = self.legs[k];
            let ankle = self.wp(leg[2]);
            let target = V3::new(ankle.x, foot_y[k].max(ankle.y - 0.001), ankle.z);
            // knee pole: forward, in the actor's facing
            let actor_q = self.bones.world_quaternion(ACTOR);
            let pole = V3::new(if k == 0 { -0.12 } else { 0.12 }, 0.05, 1.0).apply_quat(actor_q);
            self.two_bone(leg, target, pole);
            // roll the sole onto the ground plane
            let n = foot_n[k];
            if n.y < 0.999 {
                let foot = leg[2];
                let fq = self.wq(foot);
                let up = V3::new(0.0, 0.0, 1.0).apply_quat(fq);
                let dot = (1.0f64).min((-1.0f64).max(up.dot(n)));
                let mut ang = dot.acos();
                if ang > 0.35 {
                    ang = 0.35;
                }
                let axis = up.cross(n);
                if axis.length_sq() > 1e-10 {
                    let axis = axis.normalize();
                    let q3 = quat_from_axis_angle(axis, ang);
                    self.apply_world(foot, q3);
                }
            }
        }
    }

    /* ---------------- two-bone solver ---------------- */

    /// Classic analytic two-bone IK. `chain` = `[upper, lower, end]` bone
    /// indices. Preserves the bones' twist and never over-extends.
    /// `_twoBone`. `animator.js:507-541`.
    fn two_bone(&mut self, chain: [usize; 3], target: V3, pole: V3) {
        let (iu, il, ie) = (chain[0], chain[1], chain[2]);
        let a = self.wp(iu);
        let b = self.wp(il);
        let cp = self.wp(ie);
        let l1 = distance_to(a, b);
        let l2 = distance_to(b, cp);
        if l1 < 1e-5 || l2 < 1e-5 {
            return;
        }
        let dir0 = target.sub(a);
        let mut d = dir0.length();
        if d < 1e-5 {
            return;
        }
        let dir = dir0.scale(1.0 / d);
        let min = (l1 - l2).abs() + 1e-4;
        let max = l1 + l2 - 1e-4;
        d = max.min(min.max(d));
        let a_len = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
        let h = (0.0f64).max(l1 * l1 - a_len * a_len).sqrt();
        // pole component perpendicular to the limb axis
        let mut perp = pole.add_scaled(dir, -pole.dot(dir));
        if perp.length_sq() < 1e-8 {
            perp = V3::new(0.0, 1.0, 0.0).add_scaled(dir, -dir.y);
        }
        let perp = perp.normalize();
        // elbow/knee position
        let ex = a.x + dir.x * a_len + perp.x * h;
        let ey = a.y + dir.y * a_len + perp.y * h;
        let ez = a.z + dir.z * a_len + perp.z * h;
        // upper segment
        let to_e = V3::new(ex - a.x, ey - a.y, ez - a.z);
        if to_e.length_sq() < 1e-10 {
            return;
        }
        self.aim_bone(iu, to_e.normalize());
        // lower segment (world matrices refreshed by _aimBone)
        let b2 = self.wp(il);
        let to_t = V3::new(target.x - b2.x, target.y - b2.y, target.z - b2.z);
        if to_t.length_sq() < 1e-10 {
            return;
        }
        self.aim_bone(il, to_t.normalize());
    }

    /* ---------------- outputs for FX / ballistics ---------------- */

    /// `_updateMuzzle()`. `animator.js:545-553`.
    fn update_muzzle(&mut self) {
        let hand_world = self.bones.nodes[node_of(self.i_hand_r)].matrix_world;
        self.muzzle_world = apply_matrix4(self.muzzle_local, &hand_world);
        self.eject_world = apply_matrix4(self.eject_local, &hand_world);
        let hand_q = self.bones.world_quaternion(node_of(self.i_hand_r));
        self.muzzle_dir = self.bore_local.apply_quat(hand_q).normalize();
    }

    /// World position of a bone, for hitboxes and FX. `bonePos(name, out)`.
    /// `animator.js:556-558`.
    pub fn bone_pos(&self, name: &str) -> V3 {
        match self.name_index.get(name) {
            Some(i) => self.wp(*i),
            None => panic!("[ai] unknown bone \"{name}\""),
        }
    }

    /// The bone index table, for callers that already resolved a name.
    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.name_index.get(name).copied()
    }

    /// `this.iHips`/`iSpine`/... — exposed so a test (and the hitbox sync a
    /// later slice will add) can read the same indices the IK uses.
    pub fn tracked_indices(&self) -> [usize; 7] {
        [
            self.i_hips,
            self.i_spine,
            self.i_spine1,
            self.i_spine2,
            self.i_neck,
            self.i_head,
            self.i_hand_r,
        ]
    }

    /// `this.armR` — read by nothing in `animator.js` (the right arm is posed
    /// by the clips, never solved), but declared in the constructor
    /// (`animator.js:125`). Dead in the source, kept and exposed rather than
    /// dropped.
    pub fn arm_r(&self) -> [usize; 3] {
        self.arm_r
    }

    /// `THREE.Skeleton.update()` (`objects/Skeleton.js`): this frame's
    /// **joint-matrix palette** — `bone.matrixWorld * boneInverses[i]` per bone,
    /// in the rig's bone order, which is the order `super::geo`'s `skin_index`
    /// addresses.
    ///
    /// `inverses` is [`Skeleton::bind_inverses`] for the same rig: one shared
    /// table, computed once. Passing a table built from a *different* rig
    /// silently produces a folded, inside-out character rather than an error,
    /// which is why this takes it as an argument instead of rebuilding it — a
    /// per-frame rebuild would be 25 matrix inversions per actor for a constant.
    ///
    /// Every bone's `matrixWorld` is current when this is called after
    /// [`Animator::update`]: `update` forces the whole hierarchy
    /// (`update_matrix_world(node_of(0), true)`) once the pose is written, and
    /// each IK solver refreshes the subtree it moved. That is the same freshness
    /// [`Animator::bone_pos`] and the hitbox sync already rely on.
    #[must_use]
    pub fn joint_palette(&self, inverses: &[Mat4]) -> Vec<Mat4> {
        inverses
            .iter()
            .enumerate()
            .map(|(i, inv)| {
                Mat4::multiply_matrices(&self.bones.nodes[node_of(i)].matrix_world, inv)
            })
            .collect()
    }
}

/// `Vector3.distanceTo(v)` — `Math.sqrt(distanceToSquared(v))`, not
/// `Math.hypot`.
fn distance_to(a: V3, b: V3) -> f64 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// JS truthiness for a number: everything except `0`, `-0` and `NaN`.
fn truthy(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

/// `agent.animator.bonePos('FootR'/'FootL', out)` — the one call
/// `grounding.js:159-162` makes into the animator, which
/// [`super::grounding::FootSource`] was cut to name. `None` is the source's
/// `Number.isFinite` guard.
impl FootSource for Animator {
    fn foot_world(&self, foot: Foot) -> Option<[f64; 3]> {
        let name = match foot {
            Foot::Right => "FootR",
            Foot::Left => "FootL",
        };
        let p = self.bone_pos(name);
        p.y.is_finite().then_some([p.x, p.y, p.z])
    }
}
