//! A small `f64` vector/quaternion kit for the viewmodel rig
//! (`weapons::viewmodel`, `weapons::hands`), faithful to the exact THREE.js
//! `Vector3`/`Quaternion`/`Matrix4` operations `viewmodel.js` and `hands.js`
//! call.
//!
//! This is a deliberate sibling to `weapons::mathx` rather than an addition to
//! it: `mathx.rs` ports `mathx.js`'s scalar kit (springs, noise, easing) and
//! landed already as its own slice, while the vector/quaternion operations
//! below have no `mathx.js` source file at all — they are THREE.js built-ins
//! (`Vector3.applyQuaternion`, `Quaternion.slerp`,
//! `Quaternion.setFromRotationMatrix`, ...) that `viewmodel.js`/`hands.js`
//! call directly. `weapons::geometry::assembly::Xform` is not reused here for
//! the same reason `Assembly::add` builds its own composition instead of
//! reaching for `axiom_math::Quat`: `axiom_math::Quat::from_euler_xyz`
//! composes `qz*qy*qx` where THREE's `'XYZ'` order composes `qx*qy*qz` — a
//! different rotation for the same three angles (see the port recipe's
//! "Euler order is a convention, not a spelling" trap) — and `Xform`/
//! `axiom_math` are `f32` throughout, matching the mesh-authoring pipeline's
//! precision, while the rig integrates every frame in `f64`, matching the
//! source (JS numbers are `f64`, and `THREE.Vector3`/`THREE.Quaternion` store
//! their components as plain JS numbers, not a `Float32Array`).
//!
//! Every formula below is transcribed line-for-line from the real
//! `three@0.180` source (`Vector3.js`, `Quaternion.js`, `Matrix4.js`,
//! `Euler.js`) rather than re-derived, exactly as the port recipe requires
//! for "GLSL held in JS strings" — there is no other native oracle to call
//! for a THREE.js built-in, so the transcription itself is the risk, and it
//! is kept mechanical rather than tidied. In particular the products below
//! are **not** reassociated or reordered from the source's expression
//! grouping (see the recipe's "float arithmetic is not associative" trap).

/// `THREE.Vector3`, minus everything `viewmodel.js`/`hands.js` never call
/// (`multiplyScalar` in place, `divideScalar`, `applyMatrix4`, ...). Pure and
/// `Copy`, unlike the source's mutate-in-place scratch objects — Rust has no
/// need for THREE's allocation-avoidance trick, so every operation returns a
/// new value and call sites reassign (`x = x.add(y)`) exactly where the
/// source would have called `x.add(y)` on a scratch `_v`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const ZERO: V3 = V3 { x: 0.0, y: 0.0, z: 0.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        V3 { x, y, z }
    }

    pub const fn from_array(a: [f64; 3]) -> Self {
        V3::new(a[0], a[1], a[2])
    }

    pub const fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    pub const fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    pub const fn scale(self, s: f64) -> V3 {
        V3::new(self.x * s, self.y * s, self.z * s)
    }

    /// `Vector3.addScaledVector(v, s)`.
    pub const fn add_scaled(self, o: V3, s: f64) -> V3 {
        V3::new(self.x + o.x * s, self.y + o.y * s, self.z + o.z * s)
    }

    pub const fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// `Vector3.crossVectors(a, b)` — `self` cross `o`.
    pub const fn cross(self, o: V3) -> V3 {
        V3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub const fn length_sq(self) -> f64 {
        self.dot(self)
    }

    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }

    /// `Vector3.normalize()`: `this.divideScalar(this.length() || 1)` —
    /// dividing by 1 (i.e. returning the vector unchanged, which for the zero
    /// vector means staying zero) rather than producing `NaN`, when the
    /// length is exactly zero.
    pub fn normalize(self) -> V3 {
        let len = self.length();
        let d = if len == 0.0 { 1.0 } else { len };
        self.scale(1.0 / d)
    }

    /// `Vector3.lerp(v, t)`: `this + (v - this) * t`.
    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self.add(o.sub(self).scale(t))
    }

    /// `Vector3.applyQuaternion(q)` — `Vector3.js`'s quaternion-rotate
    /// formula (the "sandwich product" expanded, not `q * v * q^-1` computed
    /// directly).
    pub const fn apply_quat(self, q: Q) -> V3 {
        let (vx, vy, vz) = (self.x, self.y, self.z);
        let (qx, qy, qz, qw) = (q.x, q.y, q.z, q.w);
        let tx = 2.0 * (qy * vz - qz * vy);
        let ty = 2.0 * (qz * vx - qx * vz);
        let tz = 2.0 * (qx * vy - qy * vx);
        V3::new(
            vx + qw * tx + (qy * tz - qz * ty),
            vy + qw * ty + (qz * tx - qx * tz),
            vz + qw * tz + (qx * ty - qy * tx),
        )
    }
}

/// `THREE.Quaternion`, likewise pared to what the rig calls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Q {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

/// `Number.EPSILON` — the exact value `Quaternion.slerp`'s degenerate-angle
/// branch compares against (`Quaternion.js`).
const JS_NUMBER_EPSILON: f64 = 2.220446049250313e-16;

impl Q {
    pub const IDENTITY: Q = Q { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Q { x, y, z, w }
    }

    /// `Quaternion.setFromEuler(new Euler(x, y, z, 'XYZ'))` —
    /// `Quaternion.js`'s `case 'XYZ'` branch.
    pub fn from_euler_xyz(x: f64, y: f64, z: f64) -> Q {
        let (c1, c2, c3) = ((x * 0.5).cos(), (y * 0.5).cos(), (z * 0.5).cos());
        let (s1, s2, s3) = ((x * 0.5).sin(), (y * 0.5).sin(), (z * 0.5).sin());
        Q::new(
            s1 * c2 * c3 + c1 * s2 * s3,
            c1 * s2 * c3 - s1 * c2 * s3,
            c1 * c2 * s3 + s1 * s2 * c3,
            c1 * c2 * c3 - s1 * s2 * s3,
        )
    }

    /// `Euler.setFromQuaternion(q, 'YXZ')`, going through
    /// `Matrix4.makeRotationFromQuaternion` + `Euler.setFromRotationMatrix`'s
    /// `case 'YXZ'` branch exactly as the source's call chain does (the
    /// rotation matrix is never materialised as a `Matrix4` here — its nine
    /// entries are the nine quaternion products below, computed directly).
    /// Returns `(x = pitch, y = yaw, z = roll)`, matching `Euler`'s field
    /// names — `viewmodel.js:650-651` reads `_e.y` as yaw and `_e.x` as
    /// pitch off exactly this decomposition.
    pub fn to_euler_yxz(self) -> V3 {
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, yy, zz) = (x * x2, y * y2, z * z2);
        let (xy, xz, yz) = (x * y2, x * z2, y * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        // m11=1-(yy+zz) m12=xy-wz m13=xz+wy
        // m21=xy+wz     m22=1-(xx+zz) m23=yz-wx
        // m31=xz-wy     m32=yz+wx m33=1-(xx+yy)
        let m23 = yz - wx;
        let m13 = xz + wy;
        let m33 = 1.0 - (xx + yy);
        let m21 = xy + wz;
        let m22 = 1.0 - (xx + zz);
        let m31 = xz - wy;
        let m11 = 1.0 - (yy + zz);
        let ex = (-clamp_unit(m23)).asin();
        if m23.abs() < 0.9999999 {
            let ey = m13.atan2(m33);
            let ez = m21.atan2(m22);
            V3::new(ex, ey, ez)
        } else {
            let ey = (-m31).atan2(m11);
            V3::new(ex, ey, 0.0)
        }
    }

    /// `Quaternion.multiplyQuaternions(a, b)`, called as `a.multiply(b)` —
    /// `this = a * b` in Hamilton-product order (apply `b`'s rotation first,
    /// then `a`'s, to a vector). `viewmodel.js` uses exactly this form:
    /// `this.rig.quaternion.copy(this._baseQuat).multiply(_q)`.
    pub const fn multiply(self, o: Q) -> Q {
        let (ax, ay, az, aw) = (self.x, self.y, self.z, self.w);
        let (bx, by, bz, bw) = (o.x, o.y, o.z, o.w);
        Q::new(
            ax * bw + aw * bx + ay * bz - az * by,
            ay * bw + aw * by + az * bx - ax * bz,
            az * bw + aw * bz + ax * by - ay * bx,
            aw * bw - ax * bx - ay * by - az * bz,
        )
    }

    /// `Quaternion.conjugate()` — `(-x, -y, -z, w)`.
    pub const fn conjugate(self) -> Q {
        Q::new(-self.x, -self.y, -self.z, self.w)
    }

    pub const fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// `Quaternion.invert()`: `this.conjugate()` (no separate normalize step
    /// in the source — it relies on the quaternion already being unit
    /// length, which every quaternion this rig inverts is: a `rig.quaternion`
    /// built from `from_euler_xyz`/`multiply`/`slerp` of unit quaternions).
    pub const fn invert(self) -> Q {
        self.conjugate()
    }

    /// `Quaternion.slerp(qb, t)`, called as `self.slerp(qb, t)` — mirrors the
    /// source's `a.slerp(b, t)` mutating `a` toward `b`; here it returns the
    /// new value rather than mutating, so call sites read `x = x.slerp(y,
    /// t)` where the source reads `x.slerp(y, t)`.
    pub fn slerp(self, qb: Q, t: f64) -> Q {
        if t == 0.0 {
            return self;
        }
        if t == 1.0 {
            return qb;
        }
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let mut cos_half_theta = w * qb.w + x * qb.x + y * qb.y + z * qb.z;
        let (qbx, qby, qbz, qbw) = if cos_half_theta < 0.0 {
            cos_half_theta = -cos_half_theta;
            (-qb.x, -qb.y, -qb.z, -qb.w)
        } else {
            (qb.x, qb.y, qb.z, qb.w)
        };
        if cos_half_theta >= 1.0 {
            return Q::new(x, y, z, w);
        }
        let sqr_sin_half_theta = 1.0 - cos_half_theta * cos_half_theta;
        if sqr_sin_half_theta <= JS_NUMBER_EPSILON {
            let s = 1.0 - t;
            return Q::new(s * x + t * qbx, s * y + t * qby, s * z + t * qbz, s * w + t * qbw).normalize();
        }
        let sin_half_theta = sqr_sin_half_theta.sqrt();
        let half_theta = sin_half_theta.atan2(cos_half_theta);
        let ratio_a = ((1.0 - t) * half_theta).sin() / sin_half_theta;
        let ratio_b = (t * half_theta).sin() / sin_half_theta;
        Q::new(
            x * ratio_a + qbx * ratio_b,
            y * ratio_a + qby * ratio_b,
            z * ratio_a + qbz * ratio_b,
            w * ratio_a + qbw * ratio_b,
        )
    }

    /// `Quaternion.normalize()`: divide by length, or snap to identity's `w`
    /// axis with everything else zero if the length is exactly zero (the
    /// source's `if (l === 0) { this._x=0; this._y=0; this._z=0; this._w=1;
    /// }` branch) — only reachable from [`Q::slerp`]'s degenerate-angle path
    /// above, and only if the two inputs' *unrotated* linear blend happens to
    /// land on the zero quaternion, which no call site in this rig can drive
    /// (every blend here is between two unit quaternions with `cosHalfTheta`
    /// already resolved to be < 1, so the linear blend is never exactly
    /// zero) — kept for fidelity with the source rather than asserted
    /// unreachable.
    pub fn normalize(self) -> Q {
        let l = self.length_sq().sqrt();
        if l == 0.0 {
            Q::IDENTITY
        } else {
            let d = 1.0 / l;
            Q::new(self.x * d, self.y * d, self.z * d, self.w * d)
        }
    }

    /// `Quaternion.setFromRotationMatrix(m)` applied to a matrix built by
    /// `Matrix4.makeBasis(bx, by, bz)` (`aimBone`/`handBasis`'s pattern in
    /// both `viewmodel.js` and `hands.js`) — the "trace" method, transcribed
    /// directly rather than materialising a `Matrix4` and re-deriving its
    /// element indices.
    pub fn from_basis(bx: V3, by: V3, bz: V3) -> Q {
        let (m11, m21, m31) = (bx.x, bx.y, bx.z);
        let (m12, m22, m32) = (by.x, by.y, by.z);
        let (m13, m23, m33) = (bz.x, bz.y, bz.z);
        let trace = m11 + m22 + m33;
        if trace > 0.0 {
            let s = 0.5 / (trace + 1.0).sqrt();
            Q::new((m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s)
        } else if m11 > m22 && m11 > m33 {
            let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
            Q::new(0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s)
        } else if m22 > m33 {
            let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
            Q::new((m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s)
        } else {
            let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
            Q::new((m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s)
        }
    }
}

/// `THREE.MathUtils.clamp(v, -1, 1)`, inlined at the one call site
/// (`Euler.setFromRotationMatrix`'s `clamp(m23, -1, 1)`) that needs it —
/// distinct from [`crate::weapons::mathx::clamp`] only in that it is a
/// private `const`-free helper scoped to this file's one use.
fn clamp_unit(v: f64) -> f64 {
    v.clamp(-1.0, 1.0)
}
