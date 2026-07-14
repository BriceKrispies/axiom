//! Math (SPEC-03 / SPEC-11 §4.2) composed into the bridge: the `v3` / `mat4` /
//! `quat` ops the TS `HostBridge` math surface projects, every one forwarding to
//! [`axiom_math`] — the engine's **single deterministic source of truth** for
//! math (SPEC-03 §3.2). Nothing here re-implements a math operation: each method
//! either calls a `MathApi` / value-type primitive directly or composes a few of
//! them (`v3_lerp` folds the facade's scalar `lerp`; `quat_from_euler` composes
//! `Quat::from_axis_angle` + `Quat::multiply`).
//! ## Boundary convention (the established `slice / scalar` rule)
//! A vector / matrix / quaternion crosses the wasm boundary as a `&[f64]` slice
//! and returns as a `Vec<f64>` (JS `Float64Array`): a `Vec3` is a 3-element slice
//! `[x, y, z]`, a `Mat4` a 16-element slice (column-major, exactly
//! `Mat4::as_cols_array` order), a `Quat` a 4-element slice `[x, y, z, w]`. A lone
//! scalar (a blend factor, an angle, a clamp bound) stays a scalar `f64` arg. The
//! TS host edge (`wasm-host.ts`) packs the contract's `{ x, y, z }` / `number[]` /
//! `[x, y, z, w]` value shapes into these slices and back — the math analogue of
//! the component / physics codecs. All scalars are `f64` at the boundary and
//! narrowed to the engine's `f32` here, the one place the precision step happens.
//! (Carrying each vector as one slice — rather than flat scalar components — also
//! keeps every method within the engine's argument-count budget.)
//! ## `mat4Invert` (closed gap)
//! `mat4Invert` now forwards to the math layer's general 4×4 inverse
//! (`MathApi::mat4_invert` / `Mat4::inverse`), landed as a Wave-1 primitive — so
//! nothing is re-derived here. A singular / non-finite matrix (which the facade
//! returns `None` for) falls back to the identity, the inert boundary value the
//! other `mat4` ops use.

use axiom_kernel::Radians;
use axiom_math::{Aabb, Mat4, MathApi, Quat, Sphere, Vec2, Vec3};

use crate::GameBridge;

/// A `Vec2` from a 2-element boundary slice (missing entries read `0`).
fn v2_in(v: &[f64]) -> Vec2 {
    let [x, y]: [f32; 2] = core::array::from_fn(|i| *v.get(i).unwrap_or(&0.0) as f32);
    Vec2::new(x, y)
}

/// A `Vec2`'s two components as boundary scalars.
fn v2_out(v: Vec2) -> Vec<f64> {
    vec![f64::from(v.x), f64::from(v.y)]
}

/// A `Vec3` from a 3-element boundary slice (missing entries read `0`).
fn v3_in(v: &[f64]) -> Vec3 {
    let [x, y, z]: [f32; 3] = core::array::from_fn(|i| *v.get(i).unwrap_or(&0.0) as f32);
    Vec3::new(x, y, z)
}

/// A `Vec3`'s three components as boundary scalars.
fn v3_out(v: Vec3) -> Vec<f64> {
    vec![f64::from(v.x), f64::from(v.y), f64::from(v.z)]
}

/// A `Rect` boundary slice `[x, y, w, h]` (min-corner + size) as a z=0 [`Aabb`]
/// (min `(x, y, 0)`, max `(x+w, y+h, 0)`). An inverted (negative size) or
/// non-finite rect is the facade's reject path (`None`) — the predicates fold it
/// to the inert `false`, never re-deriving the geometry in the app.
fn rect_aabb(r: &[f64]) -> Option<Aabb> {
    let [x, y, w, h]: [f32; 4] = core::array::from_fn(|i| *r.get(i).unwrap_or(&0.0) as f32);
    Aabb::new(Vec3::new(x, y, 0.0), Vec3::new(x + w, y + h, 0.0)).ok()
}

/// A circle boundary slice `[centerX, centerY, radius]` as a z=0 [`Sphere`]. A
/// negative / non-finite radius is the facade's reject path (`None`).
fn circle_sphere(c: &[f64]) -> Option<Sphere> {
    let [x, y, radius]: [f32; 3] = core::array::from_fn(|i| *c.get(i).unwrap_or(&0.0) as f32);
    Sphere::new(Vec3::new(x, y, 0.0), radius).ok()
}

/// A `Mat4` from its 16 column-major boundary scalars (missing entries read `0`).
fn mat_in(m: &[f64]) -> Mat4 {
    Mat4::from_cols_array(core::array::from_fn(|i| *m.get(i).unwrap_or(&0.0) as f32))
}

/// A `Mat4`'s 16 column-major elements as boundary scalars.
fn mat_out(m: Mat4) -> Vec<f64> {
    m.as_cols_array().iter().copied().map(f64::from).collect()
}

/// A `Quat` from its 4 boundary scalars `[x, y, z, w]` (missing entries read `0`).
fn quat_in(q: &[f64]) -> Quat {
    let [x, y, z, w]: [f32; 4] = core::array::from_fn(|i| *q.get(i).unwrap_or(&0.0) as f32);
    Quat::new(x, y, z, w)
}

/// A `Quat`'s 4 components as boundary scalars `[x, y, z, w]`.
fn quat_out(q: Quat) -> Vec<f64> {
    vec![
        f64::from(q.x),
        f64::from(q.y),
        f64::from(q.z),
        f64::from(q.w),
    ]
}

impl GameBridge {
    /// `lhs + rhs` (`v2Add`).
    pub fn v2_add(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
        v2_out(v2_in(lhs).add(v2_in(rhs)))
    }

    /// `lhs - rhs` (`v2Sub`).
    pub fn v2_sub(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
        v2_out(v2_in(lhs).subtract(v2_in(rhs)))
    }

    /// `vector * scalar` (`v2Scale`).
    pub fn v2_scale(&self, v: &[f64], k: f64) -> Vec<f64> {
        v2_out(v2_in(v).mul_scalar(k as f32))
    }

    /// `lhs · rhs` (`v2Dot`).
    pub fn v2_dot(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
        f64::from(v2_in(lhs).dot(v2_in(rhs)))
    }

    /// Euclidean length (`v2Len`).
    pub fn v2_len(&self, v: &[f64]) -> f64 {
        f64::from(v2_in(v).length())
    }

    /// Unit vector in the same direction (`v2Normalize`); the zero vector — which
    /// `axiom-math` refuses to normalize — returns the zero vector (the inert
    /// boundary value).
    pub fn v2_normalize(&self, v: &[f64]) -> Vec<f64> {
        v2_out(v2_in(v).normalize().unwrap_or(Vec2::ZERO))
    }

    /// Distance between two points (`v2Dist`).
    pub fn v2_dist(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
        f64::from(v2_in(lhs).distance(v2_in(rhs)))
    }

    /// Component-wise linear blend (`v2Lerp`), each component through the facade's
    /// scalar `lerp` — the single `lerp` source of truth — with the start value as
    /// the inert fallback on the (finite-input) error arm.
    pub fn v2_lerp(&self, lhs: &[f64], rhs: &[f64], t: f64) -> Vec<f64> {
        let (a, b) = (v2_in(lhs), v2_in(rhs));
        let m = MathApi::new();
        let blend = |from: f32, to: f32| f64::from(m.lerp(from, to, t as f32).unwrap_or(from));
        vec![blend(a.x, b.x), blend(a.y, b.y)]
    }

    /// `lhs + rhs` (`v3Add`).
    pub fn v3_add(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
        v3_out(v3_in(lhs).add(v3_in(rhs)))
    }

    /// `lhs - rhs` (`v3Sub`).
    pub fn v3_sub(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
        v3_out(v3_in(lhs).subtract(v3_in(rhs)))
    }

    /// `vector * scalar` (`v3Scale`).
    pub fn v3_scale(&self, v: &[f64], k: f64) -> Vec<f64> {
        v3_out(v3_in(v).mul_scalar(k as f32))
    }

    /// `lhs · rhs` (`v3Dot`).
    pub fn v3_dot(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
        f64::from(v3_in(lhs).dot(v3_in(rhs)))
    }

    /// `lhs × rhs` (`v3Cross`).
    pub fn v3_cross(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
        v3_out(v3_in(lhs).cross(v3_in(rhs)))
    }

    /// Euclidean length (`v3Len`).
    pub fn v3_len(&self, v: &[f64]) -> f64 {
        f64::from(v3_in(v).length())
    }

    /// Unit vector in the same direction (`v3Normalize`); the zero vector — which
    /// `axiom-math` refuses to normalize — returns the zero vector (the inert
    /// boundary value).
    pub fn v3_normalize(&self, v: &[f64]) -> Vec<f64> {
        v3_out(v3_in(v).normalize().unwrap_or(Vec3::ZERO))
    }

    /// Distance between two points (`v3Dist`).
    pub fn v3_dist(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
        f64::from(v3_in(lhs).distance(v3_in(rhs)))
    }

    /// Component-wise linear blend (`v3Lerp`), each component through the facade's
    /// scalar `lerp` — the single `lerp` source of truth — with the start value as
    /// the inert fallback on the (finite-input) error arm.
    pub fn v3_lerp(&self, lhs: &[f64], rhs: &[f64], t: f64) -> Vec<f64> {
        let (a, b) = (v3_in(lhs), v3_in(rhs));
        let m = MathApi::new();
        let blend = |from: f32, to: f32| f64::from(m.lerp(from, to, t as f32).unwrap_or(from));
        vec![blend(a.x, b.x), blend(a.y, b.y), blend(a.z, b.z)]
    }

    /// The 4×4 identity (`mat4Identity`).
    pub fn mat4_identity(&self) -> Vec<f64> {
        mat_out(MathApi::new().mat4_identity())
    }

    /// Matrix product `lhs · rhs` (`mat4Multiply`).
    pub fn mat4_multiply(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
        mat_out(mat_in(lhs).multiply(mat_in(rhs)))
    }

    /// A right-handed perspective projection (`mat4Perspective`); invalid intrinsics
    /// (which the facade rejects) fall back to the identity (the inert boundary value).
    pub fn mat4_perspective(&self, fovy: f64, aspect: f64, near: f64, far: f64) -> Vec<f64> {
        mat_out(
            MathApi::new()
                .mat4_perspective(fovy as f32, aspect as f32, near as f32, far as f32)
                .unwrap_or(Mat4::IDENTITY),
        )
    }

    /// A right-handed look-at view matrix (`mat4LookAt`); a degenerate basis (which
    /// the facade rejects) falls back to the identity.
    pub fn mat4_look_at(&self, eye: &[f64], target: &[f64], up: &[f64]) -> Vec<f64> {
        mat_out(
            MathApi::new()
                .mat4_look_at(v3_in(eye), v3_in(target), v3_in(up))
                .unwrap_or(Mat4::IDENTITY),
        )
    }

    /// A TRS (translate · rotate · scale) composition matrix (`mat4FromTRS`),
    /// through the math layer's `Transform::to_matrix`.
    pub fn mat4_from_trs(&self, t: &[f64], r: &[f64], s: &[f64]) -> Vec<f64> {
        mat_out(
            MathApi::new()
                .transform(v3_in(t), quat_in(r), v3_in(s))
                .to_matrix(),
        )
    }

    /// The inverse of a 4×4 matrix (`mat4Invert`), through the math layer's
    /// general inverse; a singular / non-finite matrix (which the facade rejects)
    /// falls back to the identity (the inert boundary value).
    pub fn mat4_invert(&self, m: &[f64]) -> Vec<f64> {
        mat_out(
            MathApi::new()
                .mat4_invert(mat_in(m))
                .unwrap_or(Mat4::IDENTITY),
        )
    }

    /// The identity quaternion (`quatIdentity`).
    pub fn quat_identity(&self) -> Vec<f64> {
        quat_out(MathApi::new().quat_identity())
    }

    /// A quaternion from intrinsic Euler angles in radians (`quatFromEuler`),
    /// composed `yaw · pitch · roll` from the facade's axis-angle primitive (unit
    /// axes are always finite, so the identity fallback is unreachable in practice).
    pub fn quat_from_euler(&self, pitch: f64, yaw: f64, roll: f64) -> Vec<f64> {
        let m = MathApi::new();
        let axis = |a: Vec3, angle: f64| {
            m.quat_from_axis_angle(a, angle as f32)
                .unwrap_or(Quat::IDENTITY)
        };
        let qx = axis(Vec3::UNIT_X, pitch);
        let qy = axis(Vec3::UNIT_Y, yaw);
        let qz = axis(Vec3::UNIT_Z, roll);
        quat_out(qy.multiply(qx).multiply(qz))
    }

    /// Quaternion product (`quatMultiply`).
    pub fn quat_multiply(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
        quat_out(quat_in(lhs).multiply(quat_in(rhs)))
    }

    /// The unit quaternion in the same direction (`quatNormalize`); a zero
    /// quaternion (which the facade refuses) falls back to the identity.
    pub fn quat_normalize(&self, q: &[f64]) -> Vec<f64> {
        quat_out(quat_in(q).normalize().unwrap_or(Quat::IDENTITY))
    }

    /// The rotation matrix of a quaternion (`quatToMat4`).
    pub fn quat_to_mat4(&self, q: &[f64]) -> Vec<f64> {
        mat_out(MathApi::new().mat4_from_quaternion(quat_in(q)))
    }

    /// Constrain `v` to `[lo, hi]` (`clamp`); an inverted/non-finite range (which
    /// the facade rejects) returns `v` unchanged (the inert identity behaviour).
    pub fn clamp_scalar(&self, v: f64, lo: f64, hi: f64) -> f64 {
        f64::from(
            MathApi::new()
                .clamp(v as f32, lo as f32, hi as f32)
                .unwrap_or(v as f32),
        )
    }

    /// Wrap `angle` to `(-π, π]` (`normalizeAngle`); a non-finite angle returns `0`.
    pub fn normalize_angle(&self, angle: f64) -> f64 {
        let radians = Radians::new(angle as f32).unwrap_or(Radians::new(0.0).expect("0 is finite"));
        f64::from(MathApi::new().normalize_angle(radians).get())
    }

    /// Linear blend `a + (b - a) * t` (`lerp`), through the facade's scalar `lerp`
    /// — the single `lerp` source of truth (SPEC-03 §3.4); a non-finite argument
    /// (which the facade rejects) returns `a` unchanged (the inert start value).
    pub fn lerp(&self, a: f64, b: f64, t: f64) -> f64 {
        f64::from(
            MathApi::new()
                .lerp(a as f32, b as f32, t as f32)
                .unwrap_or(a as f32),
        )
    }

    /// Whether rects `a` and `b` (each `[x, y, w, h]`) share any point
    /// (`aabbOverlap`), via the math layer's z=0 [`Aabb::overlaps`]; an invalid
    /// rect (the facade's reject path) yields `false`.
    pub fn aabb_overlap(&self, a: &[f64], b: &[f64]) -> bool {
        rect_aabb(a)
            .zip(rect_aabb(b))
            .map(|(lhs, rhs)| lhs.overlaps(&rhs))
            .unwrap_or(false)
    }

    /// Whether point `p` (`[x, y]`) lies inside rect `r` (`[x, y, w, h]`)
    /// (`pointInRect`), via the math layer's z=0 [`Aabb::contains_point`]; an
    /// invalid rect yields `false`.
    pub fn point_in_rect(&self, p: &[f64], r: &[f64]) -> bool {
        let v = v2_in(p);
        rect_aabb(r)
            .map(|aabb| aabb.contains_point(Vec3::new(v.x, v.y, 0.0)))
            .unwrap_or(false)
    }

    /// Whether circles `a` and `b` (each `[centerX, centerY, radius]`) share any
    /// point (`circleOverlap`), via the math layer's z=0 [`Sphere::overlaps`]; a
    /// negative-radius circle (the facade's reject path) yields `false`.
    pub fn circle_overlap(&self, a: &[f64], b: &[f64]) -> bool {
        circle_sphere(a)
            .zip(circle_sphere(b))
            .map(|(lhs, rhs)| lhs.overlaps(&rhs))
            .unwrap_or(false)
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// `lhs + rhs` (`v2Add`).
        #[wasm_bindgen(js_name = v2Add)]
        pub fn v2_add(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
            self.bridge.v2_add(lhs, rhs)
        }

        /// `lhs - rhs` (`v2Sub`).
        #[wasm_bindgen(js_name = v2Sub)]
        pub fn v2_sub(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
            self.bridge.v2_sub(lhs, rhs)
        }

        /// `vector * scalar` (`v2Scale`).
        #[wasm_bindgen(js_name = v2Scale)]
        pub fn v2_scale(&self, v: &[f64], k: f64) -> Vec<f64> {
            self.bridge.v2_scale(v, k)
        }

        /// `lhs · rhs` (`v2Dot`).
        #[wasm_bindgen(js_name = v2Dot)]
        pub fn v2_dot(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
            self.bridge.v2_dot(lhs, rhs)
        }

        /// Euclidean length (`v2Len`).
        #[wasm_bindgen(js_name = v2Len)]
        pub fn v2_len(&self, v: &[f64]) -> f64 {
            self.bridge.v2_len(v)
        }

        /// Unit vector (`v2Normalize`).
        #[wasm_bindgen(js_name = v2Normalize)]
        pub fn v2_normalize(&self, v: &[f64]) -> Vec<f64> {
            self.bridge.v2_normalize(v)
        }

        /// Distance between two points (`v2Dist`).
        #[wasm_bindgen(js_name = v2Dist)]
        pub fn v2_dist(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
            self.bridge.v2_dist(lhs, rhs)
        }

        /// Component-wise linear blend (`v2Lerp`).
        #[wasm_bindgen(js_name = v2Lerp)]
        pub fn v2_lerp(&self, lhs: &[f64], rhs: &[f64], t: f64) -> Vec<f64> {
            self.bridge.v2_lerp(lhs, rhs, t)
        }

        /// `lhs + rhs` (`v3Add`).
        #[wasm_bindgen(js_name = v3Add)]
        pub fn v3_add(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
            self.bridge.v3_add(lhs, rhs)
        }

        /// `lhs - rhs` (`v3Sub`).
        #[wasm_bindgen(js_name = v3Sub)]
        pub fn v3_sub(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
            self.bridge.v3_sub(lhs, rhs)
        }

        /// `vector * scalar` (`v3Scale`).
        #[wasm_bindgen(js_name = v3Scale)]
        pub fn v3_scale(&self, v: &[f64], k: f64) -> Vec<f64> {
            self.bridge.v3_scale(v, k)
        }

        /// `lhs · rhs` (`v3Dot`).
        #[wasm_bindgen(js_name = v3Dot)]
        pub fn v3_dot(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
            self.bridge.v3_dot(lhs, rhs)
        }

        /// `lhs × rhs` (`v3Cross`).
        #[wasm_bindgen(js_name = v3Cross)]
        pub fn v3_cross(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
            self.bridge.v3_cross(lhs, rhs)
        }

        /// Euclidean length (`v3Len`).
        #[wasm_bindgen(js_name = v3Len)]
        pub fn v3_len(&self, v: &[f64]) -> f64 {
            self.bridge.v3_len(v)
        }

        /// Unit vector (`v3Normalize`).
        #[wasm_bindgen(js_name = v3Normalize)]
        pub fn v3_normalize(&self, v: &[f64]) -> Vec<f64> {
            self.bridge.v3_normalize(v)
        }

        /// Distance between two points (`v3Dist`).
        #[wasm_bindgen(js_name = v3Dist)]
        pub fn v3_dist(&self, lhs: &[f64], rhs: &[f64]) -> f64 {
            self.bridge.v3_dist(lhs, rhs)
        }

        /// Component-wise linear blend (`v3Lerp`).
        #[wasm_bindgen(js_name = v3Lerp)]
        pub fn v3_lerp(&self, lhs: &[f64], rhs: &[f64], t: f64) -> Vec<f64> {
            self.bridge.v3_lerp(lhs, rhs, t)
        }

        /// The 4×4 identity (`mat4Identity`).
        #[wasm_bindgen(js_name = mat4Identity)]
        pub fn mat4_identity(&self) -> Vec<f64> {
            self.bridge.mat4_identity()
        }

        /// Matrix product (`mat4Multiply`).
        #[wasm_bindgen(js_name = mat4Multiply)]
        pub fn mat4_multiply(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
            self.bridge.mat4_multiply(lhs, rhs)
        }

        /// Perspective projection (`mat4Perspective`).
        #[wasm_bindgen(js_name = mat4Perspective)]
        pub fn mat4_perspective(&self, fovy: f64, aspect: f64, near: f64, far: f64) -> Vec<f64> {
            self.bridge.mat4_perspective(fovy, aspect, near, far)
        }

        /// Look-at view matrix (`mat4LookAt`).
        #[wasm_bindgen(js_name = mat4LookAt)]
        pub fn mat4_look_at(&self, eye: &[f64], target: &[f64], up: &[f64]) -> Vec<f64> {
            self.bridge.mat4_look_at(eye, target, up)
        }

        /// TRS composition matrix (`mat4FromTRS`).
        #[wasm_bindgen(js_name = mat4FromTRS)]
        pub fn mat4_from_trs(&self, t: &[f64], r: &[f64], s: &[f64]) -> Vec<f64> {
            self.bridge.mat4_from_trs(t, r, s)
        }

        /// The inverse of a 4×4 matrix (`mat4Invert`).
        #[wasm_bindgen(js_name = mat4Invert)]
        pub fn mat4_invert(&self, m: &[f64]) -> Vec<f64> {
            self.bridge.mat4_invert(m)
        }

        /// The identity quaternion (`quatIdentity`).
        #[wasm_bindgen(js_name = quatIdentity)]
        pub fn quat_identity(&self) -> Vec<f64> {
            self.bridge.quat_identity()
        }

        /// A quaternion from Euler angles (`quatFromEuler`).
        #[wasm_bindgen(js_name = quatFromEuler)]
        pub fn quat_from_euler(&self, pitch: f64, yaw: f64, roll: f64) -> Vec<f64> {
            self.bridge.quat_from_euler(pitch, yaw, roll)
        }

        /// Quaternion product (`quatMultiply`).
        #[wasm_bindgen(js_name = quatMultiply)]
        pub fn quat_multiply(&self, lhs: &[f64], rhs: &[f64]) -> Vec<f64> {
            self.bridge.quat_multiply(lhs, rhs)
        }

        /// Unit quaternion (`quatNormalize`).
        #[wasm_bindgen(js_name = quatNormalize)]
        pub fn quat_normalize(&self, q: &[f64]) -> Vec<f64> {
            self.bridge.quat_normalize(q)
        }

        /// Rotation matrix of a quaternion (`quatToMat4`).
        #[wasm_bindgen(js_name = quatToMat4)]
        pub fn quat_to_mat4(&self, q: &[f64]) -> Vec<f64> {
            self.bridge.quat_to_mat4(q)
        }

        /// Constrain `v` to `[lo, hi]` (`clamp`).
        #[wasm_bindgen(js_name = clamp)]
        pub fn clamp_scalar(&self, v: f64, lo: f64, hi: f64) -> f64 {
            self.bridge.clamp_scalar(v, lo, hi)
        }

        /// Wrap an angle to `(-π, π]` (`normalizeAngle`).
        #[wasm_bindgen(js_name = normalizeAngle)]
        pub fn normalize_angle(&self, angle: f64) -> f64 {
            self.bridge.normalize_angle(angle)
        }

        /// Linear blend `a + (b - a) * t` (`lerp`).
        #[wasm_bindgen(js_name = lerp)]
        pub fn lerp(&self, a: f64, b: f64, t: f64) -> f64 {
            self.bridge.lerp(a, b, t)
        }

        /// Whether two rects share any point (`aabbOverlap`).
        #[wasm_bindgen(js_name = aabbOverlap)]
        pub fn aabb_overlap(&self, a: &[f64], b: &[f64]) -> bool {
            self.bridge.aabb_overlap(a, b)
        }

        /// Whether a point lies inside a rect (`pointInRect`).
        #[wasm_bindgen(js_name = pointInRect)]
        pub fn point_in_rect(&self, p: &[f64], r: &[f64]) -> bool {
            self.bridge.point_in_rect(p, r)
        }

        /// Whether two circles share any point (`circleOverlap`).
        #[wasm_bindgen(js_name = circleOverlap)]
        pub fn circle_overlap(&self, a: &[f64], b: &[f64]) -> bool {
            self.bridge.circle_overlap(a, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};
    use axiom_math::{Aabb, Mat4, MathApi, Quat, Sphere, Vec2, Vec3};

    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// The boundary narrows f64 -> f32, so a projected vector equals the native
    /// `Vec3` op promoted back to f64. This compares each `v3` projection against
    /// the `axiom-math` value-type op directly — there is no second math impl.
    #[test]
    fn v3_ops_match_axiom_math_for_sample_inputs() {
        let b = bridge();
        let lhs = Vec3::new(1.0, 2.0, 3.0);
        let rhs = Vec3::new(4.0, 5.0, 6.0);
        let promote = |v: Vec3| vec![f64::from(v.x), f64::from(v.y), f64::from(v.z)];
        let a = [1.0, 2.0, 3.0];
        let c = [4.0, 5.0, 6.0];
        assert_eq!(b.v3_add(&a, &c), promote(lhs.add(rhs)));
        assert_eq!(b.v3_sub(&c, &a), promote(rhs.subtract(lhs)));
        assert_eq!(b.v3_scale(&a, 2.0), promote(lhs.mul_scalar(2.0)));
        assert_eq!(b.v3_dot(&a, &c), f64::from(lhs.dot(rhs)));
        assert_eq!(b.v3_cross(&a, &c), promote(lhs.cross(rhs)));
        assert_eq!(b.v3_len(&[3.0, 4.0, 0.0]), 5.0);
        assert_eq!(b.v3_normalize(&[0.0, 0.0, 2.0]), vec![0.0, 0.0, 1.0]);
        // The zero vector is the un-normalizable case: it returns the zero vector.
        assert_eq!(b.v3_normalize(&[0.0, 0.0, 0.0]), vec![0.0, 0.0, 0.0]);
        assert_eq!(b.v3_dist(&[0.0, 0.0, 0.0], &[3.0, 4.0, 0.0]), 5.0);
        assert_eq!(
            b.v3_lerp(&[0.0, 0.0, 0.0], &[2.0, 4.0, 8.0], 0.5),
            vec![1.0, 2.0, 4.0]
        );
    }

    /// Mirrors the `v3` cross-check for the 2D vector surface: each `v2` projection
    /// equals the `axiom-math` `Vec2` value-type op promoted back to f64 — no second
    /// math impl (SPEC-03 §3.4, one deterministic source of truth).
    #[test]
    fn v2_ops_match_axiom_math_for_sample_inputs() {
        let b = bridge();
        let lhs = Vec2::new(1.0, 2.0);
        let rhs = Vec2::new(4.0, 6.0);
        let promote = |v: Vec2| vec![f64::from(v.x), f64::from(v.y)];
        let a = [1.0, 2.0];
        let c = [4.0, 6.0];
        assert_eq!(b.v2_add(&a, &c), promote(lhs.add(rhs)));
        assert_eq!(b.v2_sub(&c, &a), promote(rhs.subtract(lhs)));
        assert_eq!(b.v2_scale(&a, 2.0), promote(lhs.mul_scalar(2.0)));
        assert_eq!(b.v2_dot(&a, &c), f64::from(lhs.dot(rhs)));
        assert_eq!(b.v2_len(&[3.0, 4.0]), 5.0);
        assert_eq!(b.v2_normalize(&[0.0, 2.0]), vec![0.0, 1.0]);
        // The zero vector is the un-normalizable case: it returns the zero vector.
        assert_eq!(b.v2_normalize(&[0.0, 0.0]), vec![0.0, 0.0]);
        assert_eq!(b.v2_dist(&[0.0, 0.0], &[3.0, 4.0]), 5.0);
        assert_eq!(b.v2_lerp(&[0.0, 0.0], &[2.0, 8.0], 0.5), vec![1.0, 4.0]);
    }

    /// The scalar `lerp` projection equals the facade's `MathApi::lerp` (the single
    /// `lerp` source of truth), and a non-finite arg is the inert start fallback.
    #[test]
    fn scalar_lerp_matches_axiom_math() {
        let b = bridge();
        let m = MathApi::new();
        assert_eq!(
            b.lerp(0.0, 10.0, 0.5),
            f64::from(m.lerp(0.0, 10.0, 0.5).unwrap())
        );
        assert_eq!(
            b.lerp(-4.0, 4.0, 0.25),
            f64::from(m.lerp(-4.0, 4.0, 0.25).unwrap())
        );
        assert_eq!(b.lerp(0.0, 10.0, 0.0), 0.0);
        assert_eq!(b.lerp(0.0, 10.0, 1.0), 10.0);
        // A non-finite arg is the facade's reject path: lerp returns `a` unchanged.
        assert_eq!(b.lerp(7.0, 9.0, f64::INFINITY), 7.0);
    }

    /// Each pure predicate equals the `axiom-math` geometry op it forwards to, and
    /// an invalid (rejected) input folds to the inert `false`.
    #[test]
    fn predicates_match_axiom_math() {
        let b = bridge();
        // aabbOverlap: rects as [x, y, w, h] -> z=0 Aabbs.
        let base = Aabb::new(Vec3::ZERO, Vec3::new(2.0, 2.0, 0.0)).unwrap();
        let hit = Aabb::new(Vec3::new(1.0, 1.0, 0.0), Vec3::new(3.0, 3.0, 0.0)).unwrap();
        let miss = Aabb::new(Vec3::new(5.0, 5.0, 0.0), Vec3::new(6.0, 6.0, 0.0)).unwrap();
        assert_eq!(
            b.aabb_overlap(&[0.0, 0.0, 2.0, 2.0], &[1.0, 1.0, 2.0, 2.0]),
            base.overlaps(&hit)
        );
        assert_eq!(
            b.aabb_overlap(&[0.0, 0.0, 2.0, 2.0], &[5.0, 5.0, 1.0, 1.0]),
            base.overlaps(&miss)
        );
        assert!(b.aabb_overlap(&[0.0, 0.0, 2.0, 2.0], &[1.0, 1.0, 2.0, 2.0]));
        assert!(!b.aabb_overlap(&[0.0, 0.0, 2.0, 2.0], &[5.0, 5.0, 1.0, 1.0]));
        // An inverted rect (negative width) is the reject path -> false.
        assert!(!b.aabb_overlap(&[0.0, 0.0, -1.0, 2.0], &[0.0, 0.0, 2.0, 2.0]));

        // pointInRect: point as [x, y], rect [x, y, w, h].
        let rect = Aabb::new(Vec3::ZERO, Vec3::new(4.0, 4.0, 0.0)).unwrap();
        assert_eq!(
            b.point_in_rect(&[2.0, 2.0], &[0.0, 0.0, 4.0, 4.0]),
            rect.contains_point(Vec3::new(2.0, 2.0, 0.0))
        );
        assert!(b.point_in_rect(&[2.0, 2.0], &[0.0, 0.0, 4.0, 4.0]));
        assert!(!b.point_in_rect(&[5.0, 2.0], &[0.0, 0.0, 4.0, 4.0]));
        // An inverted rect is the reject path -> false.
        assert!(!b.point_in_rect(&[2.0, 2.0], &[0.0, 0.0, -4.0, 4.0]));

        // circleOverlap: circles as [centerX, centerY, radius] -> z=0 Spheres.
        let s1 = Sphere::new(Vec3::ZERO, 2.0).unwrap();
        let near = Sphere::new(Vec3::new(3.0, 0.0, 0.0), 2.0).unwrap();
        let far = Sphere::new(Vec3::new(10.0, 0.0, 0.0), 1.0).unwrap();
        assert_eq!(
            b.circle_overlap(&[0.0, 0.0, 2.0], &[3.0, 0.0, 2.0]),
            s1.overlaps(&near)
        );
        assert_eq!(
            b.circle_overlap(&[0.0, 0.0, 2.0], &[10.0, 0.0, 1.0]),
            s1.overlaps(&far)
        );
        assert!(b.circle_overlap(&[0.0, 0.0, 2.0], &[3.0, 0.0, 2.0]));
        assert!(!b.circle_overlap(&[0.0, 0.0, 2.0], &[10.0, 0.0, 1.0]));
        // A negative radius is the reject path -> false.
        assert!(!b.circle_overlap(&[0.0, 0.0, -1.0], &[0.0, 0.0, 2.0]));
    }

    #[test]
    fn mat4_ops_match_axiom_math_for_sample_inputs() {
        let b = bridge();
        let m = MathApi::new();
        let promote = |mat: Mat4| {
            mat.as_cols_array()
                .iter()
                .copied()
                .map(f64::from)
                .collect::<Vec<f64>>()
        };
        assert_eq!(b.mat4_identity(), promote(Mat4::IDENTITY));
        // identity · identity == identity.
        assert_eq!(
            b.mat4_multiply(&b.mat4_identity(), &b.mat4_identity()),
            promote(Mat4::IDENTITY)
        );
        assert_eq!(
            b.mat4_perspective(1.0, 1.5, 0.1, 100.0),
            promote(m.mat4_perspective(1.0, 1.5, 0.1, 100.0).unwrap())
        );
        assert_eq!(
            b.mat4_look_at(&[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0], &[0.0, 1.0, 0.0]),
            promote(
                m.mat4_look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::UNIT_Y)
                    .unwrap()
            )
        );
        let trs = m
            .transform(
                Vec3::new(1.0, 2.0, 3.0),
                Quat::IDENTITY,
                Vec3::new(2.0, 2.0, 2.0),
            )
            .to_matrix();
        assert_eq!(
            b.mat4_from_trs(&[1.0, 2.0, 3.0], &[0.0, 0.0, 0.0, 1.0], &[2.0, 2.0, 2.0]),
            promote(trs)
        );
    }

    #[test]
    fn mat4_invert_matches_axiom_math_and_falls_back_on_singular() {
        let b = bridge();
        let m = MathApi::new();
        let promote = |mat: Mat4| {
            mat.as_cols_array()
                .iter()
                .copied()
                .map(f64::from)
                .collect::<Vec<f64>>()
        };
        // A translation matrix inverts to its negation — the same value the facade
        // (the single source of truth) produces; no inverse is re-derived here.
        let translate = Mat4::translation(Vec3::new(3.0, -4.0, 5.0));
        assert_eq!(
            b.mat4_invert(&promote(translate)),
            promote(m.mat4_invert(translate).unwrap())
        );
        // The zero matrix is singular ⇒ the facade returns None ⇒ identity fallback.
        assert_eq!(b.mat4_invert(&promote(Mat4::ZERO)), promote(Mat4::IDENTITY));
    }

    #[test]
    fn quat_ops_match_axiom_math_for_sample_inputs() {
        let b = bridge();
        let m = MathApi::new();
        let promote_q = |q: Quat| {
            vec![
                f64::from(q.x),
                f64::from(q.y),
                f64::from(q.z),
                f64::from(q.w),
            ]
        };
        let promote_m = |mat: Mat4| {
            mat.as_cols_array()
                .iter()
                .copied()
                .map(f64::from)
                .collect::<Vec<f64>>()
        };
        assert_eq!(b.quat_identity(), promote_q(Quat::IDENTITY));
        // The Euler composition matches the same axis-angle product the bridge uses.
        let qx = m.quat_from_axis_angle(Vec3::UNIT_X, 0.5).unwrap();
        let qy = m.quat_from_axis_angle(Vec3::UNIT_Y, 0.25).unwrap();
        let qz = m.quat_from_axis_angle(Vec3::UNIT_Z, 0.75).unwrap();
        assert_eq!(
            b.quat_from_euler(0.5, 0.25, 0.75),
            promote_q(qy.multiply(qx).multiply(qz))
        );
        let a = Quat::new(0.1, 0.2, 0.3, 0.4);
        let c = Quat::new(0.5, 0.6, 0.7, 0.8);
        assert_eq!(
            b.quat_multiply(&[0.1, 0.2, 0.3, 0.4], &[0.5, 0.6, 0.7, 0.8]),
            promote_q(a.multiply(c))
        );
        assert_eq!(
            b.quat_normalize(&[0.1, 0.2, 0.3, 0.4]),
            promote_q(a.normalize().unwrap())
        );
        // The zero quaternion is the un-normalizable case: it returns the identity.
        assert_eq!(
            b.quat_normalize(&[0.0, 0.0, 0.0, 0.0]),
            promote_q(Quat::IDENTITY)
        );
        assert_eq!(
            b.quat_to_mat4(&[0.1, 0.2, 0.3, 0.4]),
            promote_m(Mat4::from_quaternion(a))
        );
    }

    #[test]
    fn scalar_ops_match_axiom_math() {
        let b = bridge();
        let m = MathApi::new();
        assert_eq!(b.clamp_scalar(5.0, 0.0, 3.0), 3.0);
        assert_eq!(b.clamp_scalar(-1.0, 0.0, 3.0), 0.0);
        // An inverted range is the facade's reject path: clamp returns v unchanged.
        assert_eq!(b.clamp_scalar(5.0, 3.0, 0.0), 5.0);
        let wrapped = f64::from(
            m.normalize_angle(axiom_kernel::Radians::new(7.0).unwrap())
                .get(),
        );
        assert_eq!(b.normalize_angle(7.0), wrapped);
        // A non-finite angle is the reject path: normalize returns 0.
        assert_eq!(b.normalize_angle(f64::INFINITY), 0.0);
    }

    /// Math forwards are pure: the same inputs always produce the same outputs.
    #[test]
    fn math_projections_are_deterministic() {
        let b = bridge();
        assert_eq!(
            b.v3_cross(&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0]),
            b.v3_cross(&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0])
        );
        assert_eq!(
            b.quat_from_euler(0.3, 0.6, 0.9),
            b.quat_from_euler(0.3, 0.6, 0.9)
        );
    }
}
