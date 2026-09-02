//! [`DMat4`]: a double-precision 4x4 transform matrix.
//!
//! The f64 companion to [`crate::Mat4`], and the reason [`crate::DQuat`] exists.
//! A rig that walks a bone hierarchy composes a matrix per node per frame and
//! then inverts one to fit a hand to a grip; a port pinned to a JavaScript
//! reference has to do that at the reference's precision or the last bits drift.
//!
//! # Storage order is part of the algorithm
//!
//! `elements` is **column-major**, exactly as `three@0.180`'s `Matrix4.js` is:
//! `e[0..4]` is the first *column* and `e[12..15]` is the translation. Every
//! formula here is transcribed against that layout with its element indices
//! intact, rather than re-derived against a row-major convention.
//!
//! That is not pedantry about style. A quaternion-to-matrix conversion written
//! row-major where the source is column-major flips every off-diagonal sign,
//! compiles, and silently corrupts the result. It has already cost this codebase
//! once, in a rigid-body inertia tensor.
//!
//! # Branchless
//!
//! Two operations here are singular-guarded — a zero determinant makes the
//! inverse undefined — and this crate is under the Branchless Law. Both are
//! rewritten as selections, and both inherit a behaviour worth stating out loud:
//! **a singular matrix inverts to all zeros, not to an error.** That is what
//! `Matrix4.js` does (`if ( det === 0 ) return this.set( 0, 0, … )`), it is what
//! every call site in the port is written against, and "fixing" it to a
//! `Result` would change the behaviour of code that is pinned to goldens.

use crate::dquat::DQuat;
use crate::dvec3::DVec3;
use crate::mat4::Mat4;

/// A double-precision 4x4 matrix, stored column-major.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DMat4 {
    /// `Matrix4.elements`, column-major.
    pub e: [f64; 16],
}

impl DMat4 {
    /// `new THREE.Matrix4()` — the identity.
    pub const IDENTITY: DMat4 = DMat4 {
        e: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// The all-zero matrix — what a singular [`DMat4::invert`] returns.
    pub const ZERO: DMat4 = DMat4 { e: [0.0; 16] };

    pub const fn from_elements(e: [f64; 16]) -> Self {
        DMat4 { e }
    }

    /// Narrow to the interchange matrix.
    ///
    /// **The one place precision is dropped**, named so that "evaluate in f64,
    /// narrow once" is a symbol you can search for rather than an `as f32`
    /// scattered across call sites.
    pub fn to_single(self) -> Mat4 {
        Mat4::from_cols_array(self.e.map(|v| v as f32))
    }

    /// Widen from the interchange matrix. Exact — every `f32` is an `f64`.
    pub fn from_single(m: Mat4) -> Self {
        DMat4 {
            e: m.as_cols_array().map(f64::from),
        }
    }

    /// `Matrix4.compose(position, quaternion, scale)` — what
    /// `Object3D.updateMatrix` calls to turn a node's local TRS into its local
    /// matrix.
    ///
    /// Transcribed element for element, including the `x2 = x + x` doubling and
    /// the exact `(1 - (yy + zz)) * sx` grouping. Float addition is not
    /// associative, so the grouping is part of the result, not a preference.
    pub fn compose(position: DVec3, quaternion: DQuat, scale: DVec3) -> DMat4 {
        let (x, y, z, w) = (quaternion.x, quaternion.y, quaternion.z, quaternion.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        let (sx, sy, sz) = (scale.x, scale.y, scale.z);
        DMat4 {
            e: [
                (1.0 - (yy + zz)) * sx,
                (xy + wz) * sx,
                (xz - wy) * sx,
                0.0,
                (xy - wz) * sy,
                (1.0 - (xx + zz)) * sy,
                (yz + wx) * sy,
                0.0,
                (xz + wy) * sz,
                (yz - wx) * sz,
                (1.0 - (xx + yy)) * sz,
                0.0,
                position.x,
                position.y,
                position.z,
                1.0,
            ],
        }
    }

    /// `Matrix4.multiplyMatrices(a, b)` — `a * b`.
    ///
    /// Transcribed with `Matrix4.js`'s own `aNM`/`bNM` naming so the
    /// column-major index map (`a12 = ae[4]`, **not** `ae[1]`) stays visible
    /// where it is used rather than living in a comment.
    pub fn multiply(a: DMat4, b: DMat4) -> DMat4 {
        let (ae, be) = (a.e, b.e);
        let (a11, a12, a13, a14) = (ae[0], ae[4], ae[8], ae[12]);
        let (a21, a22, a23, a24) = (ae[1], ae[5], ae[9], ae[13]);
        let (a31, a32, a33, a34) = (ae[2], ae[6], ae[10], ae[14]);
        let (a41, a42, a43, a44) = (ae[3], ae[7], ae[11], ae[15]);

        let (b11, b12, b13, b14) = (be[0], be[4], be[8], be[12]);
        let (b21, b22, b23, b24) = (be[1], be[5], be[9], be[13]);
        let (b31, b32, b33, b34) = (be[2], be[6], be[10], be[14]);
        let (b41, b42, b43, b44) = (be[3], be[7], be[11], be[15]);

        DMat4 {
            e: [
                a11 * b11 + a12 * b21 + a13 * b31 + a14 * b41,
                a21 * b11 + a22 * b21 + a23 * b31 + a24 * b41,
                a31 * b11 + a32 * b21 + a33 * b31 + a34 * b41,
                a41 * b11 + a42 * b21 + a43 * b31 + a44 * b41,
                a11 * b12 + a12 * b22 + a13 * b32 + a14 * b42,
                a21 * b12 + a22 * b22 + a23 * b32 + a24 * b42,
                a31 * b12 + a32 * b22 + a33 * b32 + a34 * b42,
                a41 * b12 + a42 * b22 + a43 * b32 + a44 * b42,
                a11 * b13 + a12 * b23 + a13 * b33 + a14 * b43,
                a21 * b13 + a22 * b23 + a23 * b33 + a24 * b43,
                a31 * b13 + a32 * b23 + a33 * b33 + a34 * b43,
                a41 * b13 + a42 * b23 + a43 * b33 + a44 * b43,
                a11 * b14 + a12 * b24 + a13 * b34 + a14 * b44,
                a21 * b14 + a22 * b24 + a23 * b34 + a24 * b44,
                a31 * b14 + a32 * b24 + a33 * b34 + a34 * b44,
                a41 * b14 + a42 * b24 + a43 * b34 + a44 * b44,
            ],
        }
    }

    /// `Vector3.applyMatrix4(m)` — transform a point, with the perspective
    /// divide.
    ///
    /// The divide is unconditional, as in the source. For an affine matrix `w`
    /// is 1 and it costs a multiply; for a singular one it is an infinity, which
    /// is what the reference produces and therefore what a golden expects.
    pub fn transform_point(&self, v: DVec3) -> DVec3 {
        let e = &self.e;
        let (x, y, z) = (v.x, v.y, v.z);
        let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        DVec3::new(
            (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
            (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
            (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
        )
    }

    /// `Matrix4.invert()` — the cofactor expansion `Matrix4.js` uses verbatim,
    /// **including its singular case**: a zero determinant yields
    /// [`DMat4::ZERO`], not an error.
    ///
    /// **Branchless note.** The inverse is computed unconditionally; at
    /// `det == 0` every element is a NaN or an infinity, and the selection
    /// discards the whole matrix rather than blending it. Selection, not
    /// arithmetic — a NaN multiplied by zero and summed would poison the result;
    /// a NaN indexed past does nothing.
    pub fn invert(self) -> DMat4 {
        let te = self.e;
        let (n11, n21, n31, n41) = (te[0], te[1], te[2], te[3]);
        let (n12, n22, n32, n42) = (te[4], te[5], te[6], te[7]);
        let (n13, n23, n33, n43) = (te[8], te[9], te[10], te[11]);
        let (n14, n24, n34, n44) = (te[12], te[13], te[14], te[15]);

        let t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43
            - n23 * n32 * n44
            + n22 * n33 * n44;
        let t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43
            + n13 * n32 * n44
            - n12 * n33 * n44;
        let t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43
            - n13 * n22 * n44
            + n12 * n23 * n44;
        let t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33
            + n13 * n22 * n34
            - n12 * n23 * n34;

        let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
        let d = 1.0 / det;

        let inverse = DMat4 {
            e: [
                t11 * d,
                (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43
                    + n23 * n31 * n44
                    - n21 * n33 * n44)
                    * d,
                (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42
                    - n22 * n31 * n44
                    + n21 * n32 * n44)
                    * d,
                (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42
                    + n22 * n31 * n43
                    - n21 * n32 * n43)
                    * d,
                t12 * d,
                (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43
                    - n13 * n31 * n44
                    + n11 * n33 * n44)
                    * d,
                (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42
                    + n12 * n31 * n44
                    - n11 * n32 * n44)
                    * d,
                (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42
                    - n12 * n31 * n43
                    + n11 * n32 * n43)
                    * d,
                t13 * d,
                (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43
                    + n13 * n21 * n44
                    - n11 * n23 * n44)
                    * d,
                (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42
                    - n12 * n21 * n44
                    + n11 * n22 * n44)
                    * d,
                (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42
                    + n12 * n21 * n43
                    - n11 * n22 * n43)
                    * d,
                t14 * d,
                (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33
                    - n13 * n21 * n34
                    + n11 * n23 * n34)
                    * d,
                (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32
                    + n12 * n21 * n34
                    - n11 * n22 * n34)
                    * d,
                (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32
                    - n12 * n21 * n33
                    + n11 * n22 * n33)
                    * d,
            ],
        };
        [DMat4::ZERO, inverse][usize::from(det != 0.0)]
    }

    /// `Matrix3.getNormalMatrix(m4)` = `setFromMatrix4(m4).invert().transpose()`,
    /// transcribed step for step.
    ///
    /// Returns the `Matrix3`'s nine elements in THREE's column-major order. A
    /// singular upper-left 3x3 yields all zeros, matching `Matrix3.invert()`.
    ///
    /// It returns a bare `[f64; 9]` rather than a matrix type because there is
    /// no `DMat3` and inventing one to hold a single method would be a type for
    /// the type's sake. If a second caller ever needs 3x3 arithmetic in f64,
    /// that is the moment to add it.
    pub fn normal_matrix(&self) -> [f64; 9] {
        let me = &self.e;
        // `setFromMatrix4` takes row-major arguments and stores column-major, so
        // this is [n11, n21, n31, n12, n22, n32, n13, n23, n33].
        let (n11, n21, n31) = (me[0], me[1], me[2]);
        let (n12, n22, n32) = (me[4], me[5], me[6]);
        let (n13, n23, n33) = (me[8], me[9], me[10]);

        let t11 = n33 * n22 - n32 * n23;
        let t12 = n32 * n13 - n33 * n12;
        let t13 = n23 * n12 - n22 * n13;
        let det = n11 * t11 + n21 * t12 + n31 * t13;
        let d = 1.0 / det;

        let inv = [
            t11 * d,
            (n31 * n23 - n33 * n21) * d,
            (n32 * n21 - n31 * n22) * d,
            t12 * d,
            (n33 * n11 - n31 * n13) * d,
            (n31 * n12 - n32 * n11) * d,
            t13 * d,
            (n21 * n13 - n23 * n11) * d,
            (n22 * n11 - n21 * n12) * d,
        ];
        // ...then transpose, which `getNormalMatrix` does after inverting.
        let transposed = [
            inv[0], inv[3], inv[6], inv[1], inv[4], inv[7], inv[2], inv[5], inv[8],
        ];
        [[0.0; 9], transposed][usize::from(det != 0.0)]
    }
}

impl From<Mat4> for DMat4 {
    fn from(m: Mat4) -> Self {
        DMat4::from_single(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random doubles in `[-1, 1)`. A fixed LCG, so a
    /// failure is reproducible.
    fn samples(n: usize) -> Vec<f64> {
        let mut s = 0x9E37_79B9_7F4A_7C15_u64;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
            })
            .collect()
    }

    /// A well-conditioned affine transform built from a rotation, a translation
    /// and a non-uniform scale — the shape every real call site passes.
    fn affine(c: &[f64]) -> DMat4 {
        DMat4::compose(
            DVec3::new(c[0] * 10.0, c[1] * 10.0, c[2] * 10.0),
            DQuat::from_euler_xyz(c[3] * 3.0, c[4] * 3.0, c[5] * 3.0),
            DVec3::new(1.0 + c[6].abs(), 1.0 + c[7].abs(), 1.0 + c[8].abs()),
        )
    }

    // =================================================================
    // invert
    //
    // Deliberately NOT a differential test against a re-transcribed branchy
    // reference, which is what `dquat.rs` does. A cofactor expansion is sixteen
    // near-identical 6-term products, and re-typing it to compare against would
    // most likely reproduce whatever typo it was meant to catch.
    //
    // `M * M^-1 == I` cannot be fooled that way: it fails for ANY wrong
    // cofactor, wrong sign, or wrong column-major index, and it does not care
    // what the reference implementation looked like.
    // =================================================================

    #[test]
    fn a_matrix_times_its_inverse_is_the_identity() {
        let s = samples(900);
        for c in s.chunks_exact(9) {
            let m = affine(c);
            let i = DMat4::multiply(m, m.invert());
            for (k, (got, want)) in i.e.iter().zip(DMat4::IDENTITY.e.iter()).enumerate() {
                assert!(
                    (got - want).abs() < 1e-9,
                    "element {k}: {got} vs {want}\n  m = {m:?}"
                );
            }
        }
    }

    #[test]
    fn inverting_twice_returns_the_original() {
        let s = samples(90);
        for c in s.chunks_exact(9) {
            let m = affine(c);
            let back = m.invert().invert();
            for (k, (got, want)) in back.e.iter().zip(m.e.iter()).enumerate() {
                assert!((got - want).abs() < 1e-9, "element {k}: {got} vs {want}");
            }
        }
    }

    /// The singular case, which is the branch that was rewritten. Three returns
    /// the all-zero matrix rather than an error, and every call site in the port
    /// is written against that.
    #[test]
    fn a_singular_matrix_inverts_to_zero_rather_than_to_an_error() {
        // A zero scale collapses a dimension: determinant 0.
        let flat = DMat4::compose(
            DVec3::new(1.0, 2.0, 3.0),
            DQuat::from_euler_xyz(0.3, 0.4, 0.5),
            DVec3::new(1.0, 0.0, 1.0),
        );
        assert_eq!(flat.invert(), DMat4::ZERO);
        assert_eq!(DMat4::ZERO.invert(), DMat4::ZERO);
    }

    /// The identity is its own inverse, exactly — no rounding at all. This is
    /// the case a NaN leaking out of the discarded branch would break first.
    #[test]
    fn the_identity_inverts_to_itself_exactly() {
        assert_eq!(DMat4::IDENTITY.invert(), DMat4::IDENTITY);
    }

    // =================================================================
    // compose / multiply / transform_point
    // =================================================================

    #[test]
    fn compose_places_the_translation_in_the_last_column() {
        let m = DMat4::compose(
            DVec3::new(7.0, -8.0, 9.0),
            DQuat::IDENTITY,
            DVec3::new(1.0, 1.0, 1.0),
        );
        assert_eq!((m.e[12], m.e[13], m.e[14]), (7.0, -8.0, 9.0));
        assert_eq!(m.e[15], 1.0);
    }

    /// Scale is applied before rotation, and rotation before translation — the
    /// TRS order the name promises.
    #[test]
    fn compose_scales_then_rotates_then_translates() {
        let q = DQuat::from_euler_xyz(0.0, core::f64::consts::FRAC_PI_2, 0.0);
        let m = DMat4::compose(DVec3::new(5.0, 0.0, 0.0), q, DVec3::new(2.0, 2.0, 2.0));
        // (1,0,0) scaled by 2 -> (2,0,0); yawed a quarter turn -> (0,0,-2);
        // translated -> (5,0,-2).
        let p = m.transform_point(DVec3::new(1.0, 0.0, 0.0));
        assert!((p.x - 5.0).abs() < 1e-12, "{p:?}");
        assert!(p.y.abs() < 1e-12, "{p:?}");
        assert!((p.z + 2.0).abs() < 1e-12, "{p:?}");
    }

    /// `compose` and `DQuat::rotate` must agree, or one of the two transcribed
    /// the quaternion-to-matrix conversion against the wrong storage order —
    /// the failure this module's doc comment warns about, which compiles and
    /// silently flips every off-diagonal sign.
    #[test]
    fn compose_agrees_with_rotating_the_point_directly() {
        let s = samples(300);
        for c in s.chunks_exact(6) {
            let q = DQuat::from_euler_xyz(c[0] * 3.0, c[1] * 3.0, c[2] * 3.0);
            let v = DVec3::new(c[3], c[4], c[5]);
            let m = DMat4::compose(DVec3::new(0.0, 0.0, 0.0), q, DVec3::new(1.0, 1.0, 1.0));
            let (a, b) = (m.transform_point(v), q.rotate(v));
            assert!((a.x - b.x).abs() < 1e-12, "{a:?} vs {b:?}");
            assert!((a.y - b.y).abs() < 1e-12, "{a:?} vs {b:?}");
            assert!((a.z - b.z).abs() < 1e-12, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn multiply_composes_transforms_left_to_right() {
        let s = samples(180);
        for c in s.chunks_exact(18) {
            let (a, b) = (affine(&c[..9]), affine(&c[9..]));
            let v = DVec3::new(c[0], c[1], c[2]);
            let composed = DMat4::multiply(a, b).transform_point(v);
            let stepwise = a.transform_point(b.transform_point(v));
            assert!((composed.x - stepwise.x).abs() < 1e-9, "{composed:?}");
            assert!((composed.y - stepwise.y).abs() < 1e-9, "{composed:?}");
            assert!((composed.z - stepwise.z).abs() < 1e-9, "{composed:?}");
        }
    }

    #[test]
    fn multiplying_by_the_identity_changes_nothing() {
        let m = affine(&samples(9));
        assert_eq!(DMat4::multiply(m, DMat4::IDENTITY), m);
        assert_eq!(DMat4::multiply(DMat4::IDENTITY, m), m);
    }

    #[test]
    fn the_identity_transforms_a_point_to_itself() {
        let v = DVec3::new(0.5, -1.5, 2.5);
        let p = DMat4::IDENTITY.transform_point(v);
        assert_eq!((p.x, p.y, p.z), (v.x, v.y, v.z));
    }

    // =================================================================
    // normal_matrix
    // =================================================================

    /// The normal matrix of a pure rotation is that rotation: the
    /// inverse-transpose of an orthonormal basis is itself.
    #[test]
    fn the_normal_matrix_of_a_rotation_is_the_rotation() {
        let q = DQuat::from_euler_xyz(0.4, -1.1, 0.7);
        let m = DMat4::compose(
            DVec3::new(3.0, 4.0, 5.0),
            q,
            DVec3::new(1.0, 1.0, 1.0),
        );
        let n = m.normal_matrix();
        for (k, want) in [
            m.e[0], m.e[1], m.e[2], m.e[4], m.e[5], m.e[6], m.e[8], m.e[9], m.e[10],
        ]
        .into_iter()
        .enumerate()
        {
            // `got` is bound rather than passed as a positional format argument.
            // A positional argument is evaluated only when the assertion fails,
            // so it is its own coverage region and an assertion that never fires
            // leaves it uncovered — one region, no missing lines, and invisible
            // to `--show-missing-lines`. An inline capture of an
            // already-evaluated local has no such region.
            let got = n[k];
            assert!((got - want).abs() < 1e-12, "element {k}: {got} vs {want}");
        }
    }

    /// Why the normal matrix exists at all: under a non-uniform scale a surface
    /// normal transformed by the model matrix is no longer perpendicular to the
    /// surface, and the normal matrix is what fixes it. If this ever passes with
    /// the plain upper-left 3x3, the inverse-transpose has been lost.
    #[test]
    fn a_non_uniform_scale_makes_the_normal_matrix_differ_from_the_model_matrix() {
        let m = DMat4::compose(
            DVec3::new(0.0, 0.0, 0.0),
            DQuat::IDENTITY,
            DVec3::new(2.0, 0.5, 1.0),
        );
        let n = m.normal_matrix();
        // Model scales x by 2; the normal matrix scales it by 1/2.
        assert!((n[0] - 0.5).abs() < 1e-12, "{n:?}");
        assert!((n[4] - 2.0).abs() < 1e-12, "{n:?}");
        assert!((n[8] - 1.0).abs() < 1e-12, "{n:?}");
    }

    #[test]
    fn a_singular_normal_matrix_is_zero_rather_than_an_error() {
        let flat = DMat4::compose(
            DVec3::new(1.0, 2.0, 3.0),
            DQuat::IDENTITY,
            DVec3::new(1.0, 1.0, 0.0),
        );
        assert_eq!(flat.normal_matrix(), [0.0; 9]);
    }

    // =================================================================
    // The narrowing boundary
    // =================================================================

    #[test]
    fn widening_then_narrowing_is_the_identity_on_f32() {
        let m = Mat4::from_cols_array([
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
        ]);
        assert_eq!(DMat4::from_single(m).to_single().as_cols_array(), m.as_cols_array());
    }

    #[test]
    fn the_from_impl_is_the_named_boundary() {
        let m = Mat4::from_cols_array([0.5; 16]);
        assert_eq!(DMat4::from(m), DMat4::from_single(m));
    }

    #[test]
    fn from_elements_round_trips_its_storage() {
        let e = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 9.0, 8.0, 7.0, 1.0];
        assert_eq!(DMat4::from_elements(e).e, e);
    }

    #[test]
    fn narrowing_drops_precision_and_says_so_by_moving_the_value() {
        let mut e = [0.0; 16];
        e[0] = 1.0 + 1e-12;
        assert_eq!(DMat4::from_elements(e).to_single().as_cols_array()[0], 1.0_f32);
    }
}
