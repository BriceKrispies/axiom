//! [`DQuat`]: a double-precision quaternion.
//!
//! The f64 companion to [`crate::Quat`], for the same reason [`crate::DVec3`]
//! exists: `f32` is the engine's *interchange* scalar, not a claim that every
//! computation runs at single precision. A rig that composes a dozen rotations
//! per bone per frame, or a port pinned to a JavaScript reference, evaluates in
//! `f64` and narrows once at a named boundary — [`DQuat::to_single`].
//!
//! # Semantics are Three.js's, deliberately
//!
//! Every formula here is transcribed from `three@0.180`'s `Quaternion.js`
//! against its own conventions, not re-derived. That matters because the
//! differences are invisible: `from_euler_yxz` differs from `from_euler_xyz` in
//! exactly two signs, and getting them wrong yaws about the already-pitched
//! local axis instead of about world up — which looks *almost* right.
//!
//! # Branchless
//!
//! Four of these operations are naturally written as branches, and this crate is
//! under the Branchless Law. The rewrites are not cosmetic and each is noted at
//! its site. The recurring technique: **every candidate is computed and one is
//! selected by index**. Where a discarded candidate divides by zero it produces
//! an infinity or a NaN, which is harmless precisely because it is discarded —
//! selection is not blending. Where that is load-bearing it is said so.

use crate::dvec3::DVec3;
use crate::quat::Quat;

/// `Number.EPSILON` — the exact value `Quaternion.slerp`'s degenerate-angle
/// test compares against (`Quaternion.js`).
const JS_NUMBER_EPSILON: f64 = 2.220446049250313e-16;

/// The cosine past which `Euler.setFromRotationMatrix`'s `YXZ` case treats the
/// rotation as gimbal-locked (`Euler.js`).
const GIMBAL_LOCK_COS: f64 = 0.9999999;

/// A double-precision quaternion, stored `(x, y, z, w)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DQuat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl DQuat {
    /// The identity rotation.
    pub const IDENTITY: DQuat = DQuat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        DQuat { x, y, z, w }
    }

    /// Narrow to the interchange quaternion.
    ///
    /// **The one place precision is dropped.** Named so that "evaluate in f64,
    /// narrow once" is a symbol you can search for rather than an `as f32`
    /// scattered across call sites.
    pub fn to_single(self) -> Quat {
        Quat::new(self.x as f32, self.y as f32, self.z as f32, self.w as f32)
    }

    /// Widen from the interchange quaternion. Exact — every `f32` is an `f64`.
    pub fn from_single(q: Quat) -> Self {
        DQuat::new(f64::from(q.x), f64::from(q.y), f64::from(q.z), f64::from(q.w))
    }

    /// `Quaternion.setFromEuler(new Euler(x, y, z, 'XYZ'))`.
    pub fn from_euler_xyz(x: f64, y: f64, z: f64) -> DQuat {
        let (c1, c2, c3) = ((x * 0.5).cos(), (y * 0.5).cos(), (z * 0.5).cos());
        let (s1, s2, s3) = ((x * 0.5).sin(), (y * 0.5).sin(), (z * 0.5).sin());
        DQuat::new(
            s1 * c2 * c3 + c1 * s2 * s3,
            c1 * s2 * c3 - s1 * c2 * s3,
            c1 * c2 * s3 + s1 * s2 * c3,
            c1 * c2 * c3 - s1 * s2 * s3,
        )
    }

    /// `Quaternion.setFromEuler(new Euler(x, y, z, 'YXZ'))`, the exact inverse
    /// of [`DQuat::to_euler_yxz`].
    ///
    /// It differs from [`DQuat::from_euler_xyz`] in **exactly two signs** — the
    /// `z` term's and the `w` term's — and that difference is what makes yaw
    /// rotate about world up rather than about the already-pitched local axis.
    /// Transcribed from Three's closed form rather than composed as
    /// `qy * qx * qz`, because the closed form is what runs in a browser and a
    /// port pinned to it is pinned to its float ops; the tests cross-check the
    /// two against each other.
    ///
    /// Arguments are `(pitch, yaw, roll)` — `Euler`'s `(x, y, z)`.
    pub fn from_euler_yxz(x: f64, y: f64, z: f64) -> DQuat {
        let (c1, c2, c3) = ((x * 0.5).cos(), (y * 0.5).cos(), (z * 0.5).cos());
        let (s1, s2, s3) = ((x * 0.5).sin(), (y * 0.5).sin(), (z * 0.5).sin());
        DQuat::new(
            s1 * c2 * c3 + c1 * s2 * s3,
            c1 * s2 * c3 - s1 * c2 * s3,
            c1 * c2 * s3 - s1 * s2 * c3,
            c1 * c2 * c3 + s1 * s2 * s3,
        )
    }

    /// `Euler.setFromQuaternion(q, 'YXZ')`, via
    /// `Matrix4.makeRotationFromQuaternion` + `Euler.setFromRotationMatrix`'s
    /// `YXZ` case. Returns `(x = pitch, y = yaw, z = roll)`.
    ///
    /// The rotation matrix is never materialised — its nine entries are the
    /// nine quaternion products below.
    ///
    /// **Branchless note.** Three takes a different `(yaw, roll)` pair when the
    /// rotation is gimbal-locked. Both pairs are computed and one is selected;
    /// neither divides, so neither candidate can be poisoned by the other's
    /// degeneracy.
    pub fn to_euler_yxz(self) -> DVec3 {
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, yy, zz) = (x * x2, y * y2, z * z2);
        let (xy, xz, yz) = (x * y2, x * z2, y * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        let m23 = yz - wx;
        let m13 = xz + wy;
        let m33 = 1.0 - (xx + yy);
        let m21 = xy + wz;
        let m22 = 1.0 - (xx + zz);
        let m31 = xz - wy;
        let m11 = 1.0 - (yy + zz);

        let pitch = (-m23.clamp(-1.0, 1.0)).asin();
        let free = [m13.atan2(m33), m21.atan2(m22)];
        let locked = [(-m31).atan2(m11), 0.0];
        let pair = [locked, free][usize::from(m23.abs() < GIMBAL_LOCK_COS)];
        DVec3::new(pitch, pair[0], pair[1])
    }

    /// `Quaternion.multiplyQuaternions(a, b)`, called as `a.multiply(b)` —
    /// `self * o` in Hamilton-product order: apply `o`'s rotation first, then
    /// `self`'s.
    pub const fn multiply(self, o: DQuat) -> DQuat {
        let (ax, ay, az, aw) = (self.x, self.y, self.z, self.w);
        let (bx, by, bz, bw) = (o.x, o.y, o.z, o.w);
        DQuat::new(
            ax * bw + aw * bx + ay * bz - az * by,
            ay * bw + aw * by + az * bx - ax * bz,
            az * bw + aw * bz + ax * by - ay * bx,
            aw * bw - ax * bx - ay * by - az * bz,
        )
    }

    /// `Quaternion.conjugate()` — `(-x, -y, -z, w)`.
    pub const fn conjugate(self) -> DQuat {
        DQuat::new(-self.x, -self.y, -self.z, self.w)
    }

    pub const fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }

    /// `Quaternion.invert()` — the conjugate.
    ///
    /// Three does not normalize here, relying on the quaternion already being
    /// unit length. That reliance is inherited rather than repaired: repairing
    /// it would change results for any caller whose quaternion is unit length,
    /// which is all of them, and would hide the one case where it is not.
    pub const fn invert(self) -> DQuat {
        self.conjugate()
    }

    /// `Quaternion.normalize()` — divide by length, or snap to identity when the
    /// length is exactly zero.
    ///
    /// **Branchless note.** The scaled value is computed unconditionally; at
    /// `l == 0` it is `NaN`, and the selection discards it. Selection, not
    /// blending — a `NaN` that is multiplied by zero and added would poison the
    /// result, a `NaN` that is indexed past does not.
    pub fn normalize(self) -> DQuat {
        let l = self.length();
        let d = 1.0 / l;
        let scaled = DQuat::new(self.x * d, self.y * d, self.z * d, self.w * d);
        [DQuat::IDENTITY, scaled][usize::from(l != 0.0)]
    }

    /// `Quaternion.slerp(qb, t)` — spherical interpolation from `self` toward
    /// `qb`, returning the result rather than mutating in place.
    ///
    /// **Branchless note, and this one is worth reading.** Three's version has
    /// five early exits: `t == 0`, `t == 1`, the antipodal sign flip, `cos >= 1`
    /// (already aligned), and a near-zero angle that falls back to a normalized
    /// linear blend. All five outcomes are computed here and one is selected.
    ///
    /// The `t == 0` and `t == 1` exits are kept as *selections* rather than
    /// dropped as redundant. They are not redundant: at `t == 0` the general
    /// formula returns `self` only up to rounding, and a caller holding a
    /// rotation still at rest would see it jitter in the last bits every frame.
    ///
    /// The sign flip is arithmetic (`1 - 2 * (cos < 0)`) rather than a branch,
    /// which is also how it reads more honestly — it is a reflection, not a
    /// decision.
    pub fn slerp(self, qb: DQuat, t: f64) -> DQuat {
        let dot = self.w * qb.w + self.x * qb.x + self.y * qb.y + self.z * qb.z;
        // Take the short way round: flip `qb` when the quaternions are antipodal.
        let flip = 1.0 - 2.0 * f64::from(u8::from(dot < 0.0));
        let b = DQuat::new(qb.x * flip, qb.y * flip, qb.z * flip, qb.w * flip);
        let cos_half_theta = dot * flip;

        let s = 1.0 - t;
        let linear = DQuat::new(
            s * self.x + t * b.x,
            s * self.y + t * b.y,
            s * self.z + t * b.z,
            s * self.w + t * b.w,
        )
        .normalize();

        let sqr_sin = 1.0 - cos_half_theta * cos_half_theta;
        let sin_half = sqr_sin.sqrt();
        let half_theta = sin_half.atan2(cos_half_theta);
        let ratio_a = (s * half_theta).sin() / sin_half;
        let ratio_b = (t * half_theta).sin() / sin_half;
        let spherical = DQuat::new(
            self.x * ratio_a + b.x * ratio_b,
            self.y * ratio_a + b.y * ratio_b,
            self.z * ratio_a + b.z * ratio_b,
            self.w * ratio_a + b.w * ratio_b,
        );

        // Priority order, innermost last: spherical unless the angle is
        // degenerate, then the endpoints, which win outright.
        let general = [linear, spherical][usize::from(sqr_sin > JS_NUMBER_EPSILON)];
        let unaligned = [self, general][usize::from(cos_half_theta < 1.0)];
        let not_at_end = [qb, unaligned][usize::from(t != 1.0)];
        [not_at_end, self][usize::from(t == 0.0)]
    }

    /// `Quaternion.setFromRotationMatrix(m)` for a matrix built by
    /// `Matrix4.makeBasis(bx, by, bz)` — the trace method, transcribed directly
    /// rather than materialising a matrix and re-deriving element indices.
    ///
    /// **Branchless note.** The trace method picks one of four formulas by which
    /// diagonal entry is largest, precisely so the square root is taken of a
    /// quantity that is safely positive. All four are computed here, so three of
    /// them take the root of a possibly-negative number and yield `NaN` — which
    /// is exactly what the branchy version avoids, and is harmless only because
    /// the selection discards them rather than combining them. The index is a
    /// priority encoder built from arithmetic on the three comparisons.
    pub fn from_basis(bx: DVec3, by: DVec3, bz: DVec3) -> DQuat {
        let (m11, m21, m31) = (bx.x, bx.y, bx.z);
        let (m12, m22, m32) = (by.x, by.y, by.z);
        let (m13, m23, m33) = (bz.x, bz.y, bz.z);
        let trace = m11 + m22 + m33;

        let s0 = 0.5 / (trace + 1.0).sqrt();
        let c0 = DQuat::new((m32 - m23) * s0, (m13 - m31) * s0, (m21 - m12) * s0, 0.25 / s0);

        let s1 = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
        let c1 = DQuat::new(0.25 * s1, (m12 + m21) / s1, (m13 + m31) / s1, (m32 - m23) / s1);

        let s2 = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
        let c2 = DQuat::new((m12 + m21) / s2, 0.25 * s2, (m23 + m32) / s2, (m13 - m31) / s2);

        let s3 = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
        let c3 = DQuat::new((m13 + m31) / s3, (m23 + m32) / s3, 0.25 * s3, (m21 - m12) / s3);

        // `if trace > 0 {0} else if m11 dominates {1} else if m22 > m33 {2} else {3}`,
        // as arithmetic. `&` rather than `&&`: both operands are pure comparisons.
        let take0 = usize::from(trace > 0.0);
        let take1 = usize::from((m11 > m22) & (m11 > m33));
        let take2 = usize::from(m22 > m33);
        let index = (1 - take0) * (1 + (1 - take1) * (1 + (1 - take2)));
        [c0, c1, c2, c3][index]
    }

    /// Rotate a vector by this quaternion — `Vector3.applyQuaternion`.
    pub fn rotate(self, v: DVec3) -> DVec3 {
        let (ix, iy, iz, iw) = (
            self.w * v.x + self.y * v.z - self.z * v.y,
            self.w * v.y + self.z * v.x - self.x * v.z,
            self.w * v.z + self.x * v.y - self.y * v.x,
            -self.x * v.x - self.y * v.y - self.z * v.z,
        );
        DVec3::new(
            ix * self.w + iw * -self.x + iy * -self.z - iz * -self.y,
            iy * self.w + iw * -self.y + iz * -self.x - ix * -self.z,
            iz * self.w + iw * -self.z + ix * -self.y - iy * -self.x,
        )
    }
}

impl From<Quat> for DQuat {
    fn from(q: Quat) -> Self {
        DQuat::from_single(q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random doubles in `[-1, 1)`. A fixed LCG, so a
    /// failure is reproducible and a fix is checkable.
    fn samples(n: usize) -> Vec<f64> {
        let mut s = 0x2545_F491_4F6C_DD1D_u64;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
            })
            .collect()
    }

    fn assert_bits(a: DQuat, b: DQuat, what: &str) {
        assert_eq!(
            (a.x.to_bits(), a.y.to_bits(), a.z.to_bits(), a.w.to_bits()),
            (b.x.to_bits(), b.y.to_bits(), b.z.to_bits(), b.w.to_bits()),
            "{what}: {a:?} vs {b:?}"
        );
    }

    // =================================================================
    // The branchy references.
    //
    // Tests are exempt from the Branchless Law, and that exemption is what
    // makes these worth writing: each is Three's own control flow, transcribed
    // straight, and the assertion is that the branchless rewrite above agrees
    // with it BIT FOR BIT. That is a much stronger claim than "within a
    // tolerance", and it is the claim that matters -- a port pinned to a
    // JavaScript golden cannot afford a rewrite that is merely close.
    // =================================================================

    fn normalize_branchy(q: DQuat) -> DQuat {
        let l = q.length_sq().sqrt();
        if l == 0.0 {
            DQuat::IDENTITY
        } else {
            let d = 1.0 / l;
            DQuat::new(q.x * d, q.y * d, q.z * d, q.w * d)
        }
    }

    fn slerp_branchy(a: DQuat, qb: DQuat, t: f64) -> DQuat {
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return qb;
        }
        let (x, y, z, w) = (a.x, a.y, a.z, a.w);
        let mut cos_half_theta = w * qb.w + x * qb.x + y * qb.y + z * qb.z;
        let (qbx, qby, qbz, qbw) = if cos_half_theta < 0.0 {
            cos_half_theta = -cos_half_theta;
            (-qb.x, -qb.y, -qb.z, -qb.w)
        } else {
            (qb.x, qb.y, qb.z, qb.w)
        };
        if cos_half_theta >= 1.0 {
            return DQuat::new(x, y, z, w);
        }
        let sqr_sin_half_theta = 1.0 - cos_half_theta * cos_half_theta;
        if sqr_sin_half_theta <= JS_NUMBER_EPSILON {
            let s = 1.0 - t;
            return normalize_branchy(DQuat::new(
                s * x + t * qbx,
                s * y + t * qby,
                s * z + t * qbz,
                s * w + t * qbw,
            ));
        }
        let sin_half_theta = sqr_sin_half_theta.sqrt();
        let half_theta = sin_half_theta.atan2(cos_half_theta);
        let ratio_a = ((1.0 - t) * half_theta).sin() / sin_half_theta;
        let ratio_b = (t * half_theta).sin() / sin_half_theta;
        DQuat::new(
            x * ratio_a + qbx * ratio_b,
            y * ratio_a + qby * ratio_b,
            z * ratio_a + qbz * ratio_b,
            w * ratio_a + qbw * ratio_b,
        )
    }

    fn from_basis_branchy(bx: DVec3, by: DVec3, bz: DVec3) -> DQuat {
        let (m11, m21, m31) = (bx.x, bx.y, bx.z);
        let (m12, m22, m32) = (by.x, by.y, by.z);
        let (m13, m23, m33) = (bz.x, bz.y, bz.z);
        let trace = m11 + m22 + m33;
        if trace > 0.0 {
            let s = 0.5 / (trace + 1.0).sqrt();
            DQuat::new((m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s)
        } else if m11 > m22 && m11 > m33 {
            let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
            DQuat::new(0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s)
        } else if m22 > m33 {
            let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
            DQuat::new((m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s)
        } else {
            let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
            DQuat::new((m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s)
        }
    }

    fn to_euler_yxz_branchy(q: DQuat) -> DVec3 {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, yy, zz) = (x * x2, y * y2, z * z2);
        let (xy, xz, yz) = (x * y2, x * z2, y * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        let m23 = yz - wx;
        let ex = (-m23.clamp(-1.0, 1.0)).asin();
        if m23.abs() < GIMBAL_LOCK_COS {
            DVec3::new(
                ex,
                (xz + wy).atan2(1.0 - (xx + yy)),
                (xy + wz).atan2(1.0 - (xx + zz)),
            )
        } else {
            DVec3::new(ex, (-(xz - wy)).atan2(1.0 - (yy + zz)), 0.0)
        }
    }

    // =================================================================
    // Equivalence
    // =================================================================

    #[test]
    fn the_branchless_normalize_agrees_bit_for_bit() {
        let s = samples(400);
        for c in s.chunks_exact(4) {
            let q = DQuat::new(c[0], c[1], c[2], c[3]);
            assert_bits(q.normalize(), normalize_branchy(q), "normalize");
        }
        // The one input the sampler will never produce.
        assert_bits(
            DQuat::new(0.0, 0.0, 0.0, 0.0).normalize(),
            DQuat::IDENTITY,
            "zero normalizes to identity",
        );
    }

    #[test]
    fn the_branchless_slerp_agrees_bit_for_bit() {
        let s = samples(800);
        for (i, c) in s.chunks_exact(8).enumerate() {
            let a = DQuat::new(c[0], c[1], c[2], c[3]).normalize();
            let b = DQuat::new(c[4], c[5], c[6], c[7]).normalize();
            for t in [0.0, 1.0, 0.5, 0.25, 0.999, 1e-9] {
                assert_bits(
                    a.slerp(b, t),
                    slerp_branchy(a, b, t),
                    &format!("slerp {i} t={t}"),
                );
            }
        }
    }

    /// The degenerate-angle fallback, reached on purpose rather than hoped for.
    ///
    /// Getting here is fiddly and worth writing down. Three tries `cos >= 1.0`
    /// *before* it tests the angle against `Number.EPSILON`, so a rotation that
    /// is merely tiny never reaches the fallback — `cos` rounds to exactly 1.0
    /// and the earlier exit takes it. The window is exactly one ULP wide: `cos`
    /// must be the largest double below 1.0, which makes `1 - cos*cos` land on
    /// `f64::EPSILON`, which is `Number.EPSILON`, which the test admits with
    /// `<=`.
    ///
    /// So this is not "a small rotation". It is the single representable
    /// rotation for which the fallback exists at all, and without constructing
    /// it deliberately that arm of both implementations is dead code that
    /// nothing proves.
    #[test]
    fn the_branchless_slerp_agrees_on_the_one_ulp_wide_degenerate_window() {
        // The largest double below 1.0. Both facts below are properties of
        // IEEE-754 doubles, not of this code, so they carry no failure message:
        // a formatted message on an assertion nothing can trip is an
        // unreachable region, and the coverage gate is right to say so.
        let c = 1.0 - f64::EPSILON / 2.0;
        assert!(c < 1.0);
        assert!(1.0 - c * c <= JS_NUMBER_EPSILON);

        let a = DQuat::IDENTITY;
        let b = DQuat::new((1.0 - c * c).sqrt(), 0.0, 0.0, c);
        for t in [0.25, 0.5, 0.75] {
            assert_bits(a.slerp(b, t), slerp_branchy(a, b, t), "degenerate window");
        }
        // ...and the same window on the antipodal side, so the sign flip and the
        // fallback are exercised together rather than one at a time.
        let n = DQuat::new(-b.x, -b.y, -b.z, -b.w);
        assert_bits(a.slerp(n, 0.5), slerp_branchy(a, n, 0.5), "antipodal window");
    }

    /// The zero quaternion through the branchy reference too, so the two agree
    /// on the input neither is ever handed in practice.
    #[test]
    fn both_normalizations_snap_the_zero_quaternion_to_identity() {
        let zero = DQuat::new(0.0, 0.0, 0.0, 0.0);
        assert_bits(normalize_branchy(zero), DQuat::IDENTITY, "branchy zero");
        assert_bits(zero.normalize(), normalize_branchy(zero), "both agree");
    }

    /// The three degenerate paths the sampler cannot reach on its own: already
    /// aligned, exactly antipodal, and an angle small enough to fall back to a
    /// normalized linear blend.
    #[test]
    fn the_branchless_slerp_agrees_on_the_degenerate_paths() {
        let a = DQuat::from_euler_xyz(0.3, -0.7, 1.1);
        let near = DQuat::new(a.x, a.y, a.z, a.w + 1e-12).normalize();
        for (name, b) in [
            ("identical", a),
            ("antipodal", DQuat::new(-a.x, -a.y, -a.z, -a.w)),
            ("near-identical", near),
        ] {
            for t in [0.3, 0.7] {
                assert_bits(a.slerp(b, t), slerp_branchy(a, b, t), name);
            }
        }
    }

    /// Every one of the four trace-method arms, chosen by construction rather
    /// than hoped for: a rotation with a positive trace, then three with the
    /// dominant term on each diagonal in turn.
    #[test]
    fn the_branchless_from_basis_agrees_on_all_four_arms() {
        let bases = [
            // trace > 0 -- the identity basis.
            (
                DVec3::new(1.0, 0.0, 0.0),
                DVec3::new(0.0, 1.0, 0.0),
                DVec3::new(0.0, 0.0, 1.0),
            ),
            // m11 dominates -- half a turn about x.
            (
                DVec3::new(1.0, 0.0, 0.0),
                DVec3::new(0.0, -1.0, 0.0),
                DVec3::new(0.0, 0.0, -1.0),
            ),
            // m22 dominates -- half a turn about y.
            (
                DVec3::new(-1.0, 0.0, 0.0),
                DVec3::new(0.0, 1.0, 0.0),
                DVec3::new(0.0, 0.0, -1.0),
            ),
            // m33 dominates -- half a turn about z.
            (
                DVec3::new(-1.0, 0.0, 0.0),
                DVec3::new(0.0, -1.0, 0.0),
                DVec3::new(0.0, 0.0, 1.0),
            ),
        ];
        for (i, (bx, by, bz)) in bases.into_iter().enumerate() {
            assert_bits(
                DQuat::from_basis(bx, by, bz),
                from_basis_branchy(bx, by, bz),
                &format!("from_basis arm {i}"),
            );
        }
        // ...and a sweep of real rotations, so the arm selection is exercised
        // on inputs nobody hand-picked.
        let s = samples(300);
        for c in s.chunks_exact(3) {
            let q = DQuat::from_euler_xyz(c[0] * 3.0, c[1] * 3.0, c[2] * 3.0);
            let (bx, by, bz) = (
                q.rotate(DVec3::new(1.0, 0.0, 0.0)),
                q.rotate(DVec3::new(0.0, 1.0, 0.0)),
                q.rotate(DVec3::new(0.0, 0.0, 1.0)),
            );
            assert_bits(
                DQuat::from_basis(bx, by, bz),
                from_basis_branchy(bx, by, bz),
                "from_basis sweep",
            );
        }
    }

    #[test]
    fn the_branchless_to_euler_yxz_agrees_including_at_gimbal_lock() {
        let s = samples(300);
        let mut angles: Vec<DQuat> = s
            .chunks_exact(3)
            .map(|c| DQuat::from_euler_yxz(c[0] * 3.0, c[1] * 3.0, c[2] * 3.0))
            .collect();
        // Straight up and straight down: the locked arm.
        angles.push(DQuat::from_euler_yxz(core::f64::consts::FRAC_PI_2, 0.4, 0.0));
        angles.push(DQuat::from_euler_yxz(
            -core::f64::consts::FRAC_PI_2,
            -0.9,
            0.0,
        ));
        for q in angles {
            let (a, b) = (q.to_euler_yxz(), to_euler_yxz_branchy(q));
            assert_eq!(
                (a.x.to_bits(), a.y.to_bits(), a.z.to_bits()),
                (b.x.to_bits(), b.y.to_bits(), b.z.to_bits()),
                "to_euler_yxz {q:?}"
            );
        }
    }

    // =================================================================
    // Semantics
    // =================================================================

    /// The two Euler orders differ in exactly two signs, and that difference is
    /// the whole reason both exist. If they ever agree for a rotation with both
    /// pitch and yaw, one of them has been "tidied" into the other.
    #[test]
    fn the_two_euler_orders_are_genuinely_different() {
        let a = DQuat::from_euler_xyz(0.5, 0.9, 0.2);
        let b = DQuat::from_euler_yxz(0.5, 0.9, 0.2);
        assert_ne!(a, b);
        // ...and they agree where they must: a rotation about a single axis is
        // order-independent.
        assert_bits(
            DQuat::from_euler_xyz(0.0, 0.9, 0.0),
            DQuat::from_euler_yxz(0.0, 0.9, 0.0),
            "yaw only",
        );
    }

    #[test]
    fn to_euler_yxz_inverts_from_euler_yxz() {
        for (p, y, r) in [(0.3, -1.2, 0.4), (-0.8, 2.0, -0.15), (0.0, 0.0, 0.0)] {
            let e = DQuat::from_euler_yxz(p, y, r).to_euler_yxz();
            assert!((e.x - p).abs() < 1e-12, "pitch {} vs {p}", e.x);
            assert!((e.y - y).abs() < 1e-12, "yaw {} vs {y}", e.y);
            assert!((e.z - r).abs() < 1e-12, "roll {} vs {r}", e.z);
        }
    }

    #[test]
    fn multiply_applies_the_right_hand_rotation_first() {
        let yaw = DQuat::from_euler_xyz(0.0, core::f64::consts::FRAC_PI_2, 0.0);
        let pitch = DQuat::from_euler_xyz(core::f64::consts::FRAC_PI_2, 0.0, 0.0);
        let v = DVec3::new(0.0, 0.0, 1.0);
        // (yaw * pitch) applied to v == yaw applied to (pitch applied to v).
        let composed = yaw.multiply(pitch).rotate(v);
        let stepwise = yaw.rotate(pitch.rotate(v));
        assert!((composed.x - stepwise.x).abs() < 1e-15);
        assert!((composed.y - stepwise.y).abs() < 1e-15);
        assert!((composed.z - stepwise.z).abs() < 1e-15);
    }

    #[test]
    fn a_rotation_composed_with_its_inverse_is_the_identity() {
        let q = DQuat::from_euler_yxz(0.4, -1.1, 0.25);
        let i = q.multiply(q.invert());
        assert!((i.w.abs() - 1.0).abs() < 1e-15, "{i:?}");
        assert!(
            i.x.abs() < 1e-15 && i.y.abs() < 1e-15 && i.z.abs() < 1e-15,
            "{i:?}"
        );
    }

    #[test]
    fn conjugate_negates_the_vector_part_only() {
        let q = DQuat::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(q.conjugate(), DQuat::new(-1.0, -2.0, -3.0, 4.0));
        assert_eq!(q.invert(), q.conjugate());
    }

    #[test]
    fn length_and_length_sq_agree() {
        let q = DQuat::new(1.0, 2.0, 2.0, 4.0);
        assert_eq!(q.length_sq(), 25.0);
        assert_eq!(q.length(), 5.0);
        assert!((q.normalize().length() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn rotating_by_the_identity_changes_nothing() {
        let v = DVec3::new(0.3, -1.7, 2.2);
        let r = DQuat::IDENTITY.rotate(v);
        assert_eq!((r.x, r.y, r.z), (v.x, v.y, v.z));
    }

    #[test]
    fn a_quarter_turn_about_y_sends_z_to_x() {
        let q = DQuat::from_euler_xyz(0.0, core::f64::consts::FRAC_PI_2, 0.0);
        let r = q.rotate(DVec3::new(0.0, 0.0, 1.0));
        assert!((r.x - 1.0).abs() < 1e-15, "{r:?}");
        assert!(r.y.abs() < 1e-15 && r.z.abs() < 1e-15, "{r:?}");
    }

    #[test]
    fn from_basis_recovers_the_rotation_that_built_the_basis() {
        let q = DQuat::from_euler_yxz(0.35, 1.1, -0.6);
        let r = DQuat::from_basis(
            q.rotate(DVec3::new(1.0, 0.0, 0.0)),
            q.rotate(DVec3::new(0.0, 1.0, 0.0)),
            q.rotate(DVec3::new(0.0, 0.0, 1.0)),
        );
        // q and -q are the same rotation, so compare up to sign.
        let flip = r.w.signum() * q.w.signum();
        assert!((r.x * flip - q.x).abs() < 1e-14, "{r:?} vs {q:?}");
        assert!((r.y * flip - q.y).abs() < 1e-14, "{r:?} vs {q:?}");
        assert!((r.z * flip - q.z).abs() < 1e-14, "{r:?} vs {q:?}");
    }

    // =================================================================
    // The narrowing boundary
    // =================================================================

    #[test]
    fn widening_then_narrowing_is_the_identity_on_f32() {
        let q = Quat::new(0.1, -0.2, 0.3, 0.9);
        let back = DQuat::from_single(q).to_single();
        assert_eq!((back.x, back.y, back.z, back.w), (q.x, q.y, q.z, q.w));
    }

    #[test]
    fn the_from_impl_is_the_named_boundary() {
        let q = Quat::new(0.5, 0.5, 0.5, 0.5);
        assert_eq!(DQuat::from(q), DQuat::from_single(q));
    }

    #[test]
    fn narrowing_drops_precision_and_says_so_by_moving_the_value() {
        // A value f64 can hold exactly and f32 cannot -- this is the boundary
        // doing its job, not a defect.
        let d = DQuat::new(1.0 + 1e-12, 0.0, 0.0, 0.0);
        assert_eq!(d.to_single().x, 1.0_f32);
        assert_ne!(d.x, 1.0);
    }
}
