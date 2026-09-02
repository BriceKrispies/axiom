//! Three-component double-precision vector.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;

/// A deterministic three-component `f64` vector.
///
/// The double-precision sibling of [`crate::Vec3`], with the same operations
/// and the same never-panics discipline. It exists for the domains whose
/// *internal* precision is load-bearing — a collision kernel over a city-scale
/// world, an atmosphere LUT, a bake-time noise oracle a shader is pinned
/// against — where evaluating in `f32` does not merely lose digits but
/// introduces disagreements the reference does not have. See
/// [`crate::Scalar`] for the rule, and the measurement behind it.
///
/// It is **not** a second engine scalar. `f32` remains what crosses a facade,
/// fills a vertex buffer and stores a transform; a `DVec3` is narrowed to
/// [`crate::Vec3`] at that boundary, once, by [`DVec3::to_single`]. Nothing
/// should reach for this type to hold a transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DVec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl DVec3 {
    /// `(0, 0, 0)`.
    pub const ZERO: DVec3 = DVec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    /// `(1, 1, 1)`.
    pub const ONE: DVec3 = DVec3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    /// `(1, 0, 0)`.
    pub const UNIT_X: DVec3 = DVec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    /// `(0, 1, 0)`.
    pub const UNIT_Y: DVec3 = DVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    /// `(0, 0, 1)`.
    pub const UNIT_Z: DVec3 = DVec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    /// Component constructor.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        DVec3 { x, y, z }
    }

    /// Component-wise sum.
    pub const fn add(self, other: DVec3) -> DVec3 {
        DVec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    /// Component-wise difference.
    pub const fn subtract(self, other: DVec3) -> DVec3 {
        DVec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    /// Component-wise product. The `f32` sibling has no such method because
    /// its callers never needed one; the procedural-field callers this type
    /// serves scale each axis independently all the time (a per-axis noise
    /// frequency), and expressing that as three field multiplies at every call
    /// site is how transcription errors get in.
    pub const fn mul_componentwise(self, other: DVec3) -> DVec3 {
        DVec3::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }

    /// Scale by a scalar.
    pub const fn mul_scalar(self, k: f64) -> DVec3 {
        DVec3::new(self.x * k, self.y * k, self.z * k)
    }

    /// Divide by a scalar, returning [`crate::math_error_code::MathErrorCode::DivideByZero`]
    /// if `k` is `0.0` and [`crate::math_error_code::MathErrorCode::NonFiniteScalar`]
    /// if `k` is not finite.
    pub fn div_scalar(self, k: f64) -> MathResult<DVec3> {
        (!k.is_finite())
            .then_some(Err(MathError::non_finite_scalar(
                "dvec3 scalar divisor must be finite",
            )))
            .or_else(|| {
                (k == 0.0).then_some(Err(MathError::divide_by_zero(
                    "dvec3 scalar divisor was zero",
                )))
            })
            .unwrap_or_else(|| Ok(DVec3::new(self.x / k, self.y / k, self.z / k)))
    }

    /// Dot product.
    pub const fn dot(self, other: DVec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product. Right-handed: `unit_x × unit_y = unit_z`.
    pub const fn cross(self, other: DVec3) -> DVec3 {
        DVec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Squared length.
    pub const fn length_squared(self) -> f64 {
        self.dot(self)
    }

    /// Euclidean length.
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    /// Unit-length copy. Fails with
    /// [`crate::math_error_code::MathErrorCode::NormalizeZeroLength`] for the
    /// zero vector.
    pub fn normalize(self) -> MathResult<DVec3> {
        let len = self.length();
        let valid = (len != 0.0) & len.is_finite();
        valid
            .then_some(len)
            .map(|len| DVec3::new(self.x / len, self.y / len, self.z / len))
            .ok_or_else(|| MathError::normalize_zero_length("cannot normalize zero-length DVec3"))
    }

    /// Euclidean distance between `self` and `other`.
    pub fn distance(self, other: DVec3) -> f64 {
        self.subtract(other).length()
    }

    /// Component-wise floor.
    ///
    /// Present here and not on [`crate::Vec3`] because a lattice coordinate is
    /// what double precision is carried *for*: every value-noise and
    /// gradient-noise basis starts by splitting a position into its integer
    /// cell and fractional offset, and doing that split at single precision is
    /// where a coarse world coordinate loses the fraction entirely.
    pub fn floor(self) -> DVec3 {
        DVec3::new(self.x.floor(), self.y.floor(), self.z.floor())
    }

    /// The fractional part, `self - self.floor()`, always in `[0, 1)`.
    ///
    /// This is GLSL's `fract`, **not** Rust's `f64::fract`, which keeps the
    /// sign of its input (`(-0.3).fract() == -0.3`). The difference is not
    /// cosmetic: a lattice basis that used the signed form would fold
    /// negative-coordinate cells onto the wrong corner and break periodicity
    /// on exactly half the domain.
    pub fn fract(self) -> DVec3 {
        self.subtract(self.floor())
    }

    /// Build from a bare `[x, y, z]`.
    ///
    /// Ported geometry arrives as arrays far more often than as constructor
    /// calls, and writing `DVec3::new(a[0], a[1], a[2])` at every such site is
    /// three chances to transpose an index.
    pub const fn from_array(a: [f64; 3]) -> Self {
        DVec3::new(a[0], a[1], a[2])
    }

    /// `Vector3.addScaledVector(v, s)` — `self + o * s`.
    ///
    /// One operation, not `self.add(o.mul_scalar(s))`, because the source fuses
    /// it and float addition is not associative: the split form rounds the
    /// intermediate and can differ in the last bits.
    pub const fn add_scaled(self, o: DVec3, s: f64) -> DVec3 {
        DVec3::new(self.x + o.x * s, self.y + o.y * s, self.z + o.z * s)
    }

    /// `Vector3.lerp(v, t)` — `self + (v - self) * t`.
    ///
    /// Written in exactly that grouping rather than the algebraically equal
    /// `self * (1 - t) + v * t`. The two differ in the last bits, and the second
    /// does not reproduce `self` exactly at `t == 0`.
    pub const fn lerp(self, o: DVec3, t: f64) -> DVec3 {
        DVec3::new(
            self.x + (o.x - self.x) * t,
            self.y + (o.y - self.y) * t,
            self.z + (o.z - self.z) * t,
        )
    }

    /// `Vector3.distanceToSquared(v)` — `dx*dx + dy*dy + dz*dz`.
    ///
    /// **Not** `hypot(dx, dy, dz).powi(2)`. `hypot` scales by the largest
    /// magnitude first to avoid overflow, so it rounds differently; substituting
    /// it here would be a strictly better function and a different answer.
    pub const fn distance_squared(self, o: DVec3) -> f64 {
        let (dx, dy, dz) = (self.x - o.x, self.y - o.y, self.z - o.z);
        dx * dx + dy * dy + dz * dz
    }

    /// Unit length, or the zero vector if there is no direction to speak of.
    ///
    /// The infallible companion to [`DVec3::normalize`], and the difference is
    /// not a convenience — it is a different function. `normalize` reports a
    /// zero-length input as an error, which is right when the caller has a
    /// meaningful response to that. This one is `Vector3.normalize()`'s
    /// `divideScalar(this.length() || 1)`: a zero vector stays zero and nothing
    /// is reported.
    ///
    /// Ported code needs the second, because the reference has no error channel
    /// here and a caller that never checked one cannot start checking it without
    /// changing what the frame looks like. Reaching for `.normalize().unwrap()`
    /// instead would turn a silent zero into a panic on the first degenerate
    /// input.
    pub fn normalize_or_zero(self) -> DVec3 {
        self.mul_scalar(1.0 / crate::nonzero_or_one(self.length()))
    }

    /// Narrow to the engine's interchange scalar.
    ///
    /// The single, explicit narrowing point. Naming it — rather than letting
    /// call sites write `as f32` three times — is what makes "compute in `f64`,
    /// narrow once" auditable: the boundary is a symbol you can search for.
    pub fn to_single(self) -> crate::vec3::Vec3 {
        crate::vec3::Vec3::new(self.x as f32, self.y as f32, self.z as f32)
    }

    /// Widen from the engine's interchange scalar. Exact — every `f32` is
    /// representable as an `f64`.
    pub fn from_single(v: crate::vec3::Vec3) -> DVec3 {
        DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
    }

    /// Append the three `f64` components in declaration order.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_f64(self.x);
        writer.write_f64(self.y);
        writer.write_f64(self.z);
    }

    /// Read three `f64` components in declaration order.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<DVec3> {
        reader.read_f64().and_then(|x| {
            reader
                .read_f64()
                .and_then(|y| reader.read_f64().map(|z| DVec3::new(x, y, z)))
        })
    }
}

impl ApproxEq for DVec3 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.x.approx_eq(&other.x, epsilon)
            & self.y.approx_eq(&other.y, epsilon)
            & self.z.approx_eq(&other.z, epsilon)
    }
}

impl Reflect for DVec3 {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "DVec3",
        &[
            FieldSchema::new("x", "f64"),
            FieldSchema::new("y", "f64"),
            FieldSchema::new("z", "f64"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.x.reflect_write(writer);
        self.y.reflect_write(writer);
        self.z.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f64::reflect_read(reader).and_then(|x| {
            f64::reflect_read(reader)
                .and_then(|y| f64::reflect_read(reader).map(|z| DVec3::new(x, y, z)))
        })
    }
}

#[cfg(test)]
mod reflect_tests {
    use super::*;

    #[test]
    fn reflect_round_trips_describes_and_rejects_truncation() {
        let v = DVec3::new(1.5, -2.0, 0.25);
        let mut w = BinaryWriter::new();
        v.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            DVec3::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            v
        );
        for len in 0..bytes.len() {
            assert!(DVec3::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err());
        }
        assert_eq!(<DVec3 as Reflect>::SCHEMA.name(), "DVec3");
        assert_eq!(<DVec3 as Reflect>::SCHEMA.fields().len(), 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;
    use axiom_kernel::KernelApi;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT_DOUBLE
    }

    #[test]
    fn constants_match_documentation() {
        assert!(DVec3::ZERO.approx_eq(&DVec3::new(0.0, 0.0, 0.0), eps()));
        assert!(DVec3::ONE.approx_eq(&DVec3::new(1.0, 1.0, 1.0), eps()));
        assert!(DVec3::UNIT_X.approx_eq(&DVec3::new(1.0, 0.0, 0.0), eps()));
        assert!(DVec3::UNIT_Y.approx_eq(&DVec3::new(0.0, 1.0, 0.0), eps()));
        assert!(DVec3::UNIT_Z.approx_eq(&DVec3::new(0.0, 0.0, 1.0), eps()));
    }

    #[test]
    fn add_is_component_wise() {
        let r = DVec3::new(1.0, 2.0, 3.0).add(DVec3::new(4.0, 5.0, 6.0));
        assert!(r.approx_eq(&DVec3::new(5.0, 7.0, 9.0), eps()));
    }

    #[test]
    fn subtract_is_component_wise() {
        let r = DVec3::new(5.0, 7.0, 9.0).subtract(DVec3::new(1.0, 2.0, 3.0));
        assert!(r.approx_eq(&DVec3::new(4.0, 5.0, 6.0), eps()));
    }

    #[test]
    fn mul_componentwise_scales_each_axis_independently() {
        let r = DVec3::new(2.0, 3.0, 4.0).mul_componentwise(DVec3::new(5.0, 7.0, 11.0));
        assert!(r.approx_eq(&DVec3::new(10.0, 21.0, 44.0), eps()));
    }

    #[test]
    fn mul_scalar_scales_each_component() {
        let r = DVec3::new(1.0, -2.0, 4.0).mul_scalar(0.5);
        assert!(r.approx_eq(&DVec3::new(0.5, -1.0, 2.0), eps()));
    }

    #[test]
    fn div_scalar_scales_each_component() {
        let r = DVec3::new(2.0, -4.0, 8.0).div_scalar(2.0).unwrap();
        assert!(r.approx_eq(&DVec3::new(1.0, -2.0, 4.0), eps()));
    }

    #[test]
    fn div_by_zero_is_rejected() {
        let err = DVec3::ONE.div_scalar(0.0).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::DivideByZero);
    }

    #[test]
    fn div_by_non_finite_is_rejected() {
        let err = DVec3::ONE.div_scalar(f64::NAN).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn dot_matches_geometry() {
        assert_eq!(DVec3::UNIT_X.dot(DVec3::UNIT_Y), 0.0);
        assert_eq!(DVec3::new(1.0, 2.0, 3.0).dot(DVec3::new(4.0, 5.0, 6.0)), 32.0);
    }

    #[test]
    fn cross_is_right_handed() {
        assert!(DVec3::UNIT_X
            .cross(DVec3::UNIT_Y)
            .approx_eq(&DVec3::UNIT_Z, eps()));
        assert!(DVec3::UNIT_Y
            .cross(DVec3::UNIT_X)
            .approx_eq(&DVec3::UNIT_Z.mul_scalar(-1.0), eps()));
    }

    #[test]
    fn length_and_length_squared_agree() {
        let v = DVec3::new(2.0, 3.0, 6.0);
        assert_eq!(v.length_squared(), 49.0);
        assert_eq!(v.length(), 7.0);
    }

    #[test]
    fn normalize_produces_unit_length() {
        let n = DVec3::new(0.0, 0.0, 7.0).normalize().unwrap();
        assert!(n.approx_eq(&DVec3::UNIT_Z, eps()));
    }

    #[test]
    fn normalize_zero_fails() {
        let err = DVec3::ZERO.normalize().unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NormalizeZeroLength);
    }

    #[test]
    fn normalize_non_finite_length_fails() {
        assert!(DVec3::new(f64::MAX, f64::MAX, f64::MAX).normalize().is_err());
    }

    #[test]
    fn distance_is_symmetric() {
        let a = DVec3::new(1.0, 2.0, 3.0);
        let b = DVec3::new(3.0, 5.0, 9.0);
        assert_eq!(a.distance(b), 7.0);
        assert_eq!(b.distance(a), 7.0);
    }

    #[test]
    fn approx_eq_rejects_nan_components() {
        let nan = DVec3::new(f64::NAN, 0.0, 0.0);
        assert!(!nan.approx_eq(&DVec3::ZERO, eps()));
        assert!(!DVec3::ZERO.approx_eq(&nan, eps()));
        assert!(!DVec3::new(0.0, f64::NAN, 0.0).approx_eq(&DVec3::ZERO, eps()));
        assert!(!DVec3::new(0.0, 0.0, f64::NAN).approx_eq(&DVec3::ZERO, eps()));
    }

    #[test]
    fn binary_round_trip_preserves_components() {
        let api = KernelApi::new();
        let v = DVec3::new(0.1, -2.25, 1.0e-13);

        let mut writer = api.binary_writer();
        v.write_to(&mut writer);
        let bytes = writer.into_bytes();
        assert_eq!(bytes.len(), 24);

        let mut reader = api.binary_reader(&bytes);
        // Exact, not approximate: the whole point of the type is that these
        // digits survive, and `1.0e-13` is below `DEFAULT_DOUBLE` so an
        // approximate assertion would pass even if the component were dropped.
        assert_eq!(DVec3::read_from(&mut reader).unwrap(), v);
    }

    #[test]
    fn read_from_truncated_each_component() {
        assert!(DVec3::read_from(&mut BinaryReader::new(&[])).is_err());
        assert!(DVec3::read_from(&mut BinaryReader::new(&[0u8; 8])).is_err());
        assert!(DVec3::read_from(&mut BinaryReader::new(&[0u8; 16])).is_err());
    }
}

#[cfg(test)]
mod precision {
    use super::*;

    /// The reason the type exists, asserted rather than asserted-about-in-prose.
    /// A city-scale coordinate carrying a sub-millimetre offset survives in
    /// `f64` and is annihilated in `f32` — which is exactly the case a collision
    /// kernel and a noise lattice both sit on.
    #[test]
    fn double_precision_retains_a_fraction_single_precision_annihilates() {
        let coarse = 8_192.0_f64;
        let fine = coarse + 1.0e-4;
        assert_ne!(fine, coarse);
        assert_eq!(fine as f32, coarse as f32);

        let v = DVec3::new(fine, fine, fine);
        assert!(v.fract().x > 0.0);
        assert_eq!(DVec3::from_single(v.to_single()).fract().x, 0.0);
    }

    #[test]
    fn floor_and_fract_split_a_position_into_cell_and_offset() {
        let v = DVec3::new(3.25, -1.75, 0.0);
        assert_eq!(v.floor(), DVec3::new(3.0, -2.0, 0.0));
        assert_eq!(v.fract(), DVec3::new(0.25, 0.25, 0.0));
        assert_eq!(v.floor().add(v.fract()), v);
    }

    /// `fract` is GLSL's, not Rust's. Rust's `f64::fract` keeps the sign, which
    /// would put a negative coordinate on the wrong lattice corner.
    #[test]
    fn fract_is_non_negative_for_negative_inputs_unlike_rusts() {
        assert_eq!((-0.3_f64).fract(), -0.3);
        assert_eq!(DVec3::new(-0.3, -0.3, -0.3).fract().x, 0.7);
    }

    #[test]
    fn to_single_and_from_single_are_the_named_narrowing_boundary() {
        let v = DVec3::new(1.5, -2.25, 0.5);
        assert_eq!(v.to_single(), crate::vec3::Vec3::new(1.5, -2.25, 0.5));
        // Widening is exact, so a value that started as f32 round-trips.
        assert_eq!(DVec3::from_single(v.to_single()), v);
    }
}

#[cfg(test)]
mod rig_ops_tests {
    use super::DVec3;

    #[test]
    fn from_array_keeps_the_component_order() {
        assert_eq!(DVec3::from_array([1.0, 2.0, 3.0]), DVec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn add_scaled_is_the_fused_form() {
        let a = DVec3::new(1.0, 2.0, 3.0);
        let b = DVec3::new(10.0, 20.0, 30.0);
        assert_eq!(a.add_scaled(b, 2.0), DVec3::new(21.0, 42.0, 63.0));
        assert_eq!(a.add_scaled(b, 0.0), a);
    }

    #[test]
    fn lerp_hits_both_endpoints_exactly() {
        let a = DVec3::new(0.1, -0.2, 0.3);
        let b = DVec3::new(9.9, 8.8, -7.7);
        // Exactly, not approximately. `self + (o - self) * t` reproduces `self`
        // at t == 0 bit for bit; the algebraically equal `self*(1-t) + o*t` does
        // not, and a rig at rest would jitter in the last bits every frame.
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, 0.5), DVec3::new(5.0, 4.3, -3.7));
    }

    #[test]
    fn distance_squared_is_the_plain_sum_of_squares() {
        let a = DVec3::new(1.0, 2.0, 3.0);
        let b = DVec3::new(4.0, 6.0, 15.0);
        assert_eq!(a.distance_squared(b), 9.0 + 16.0 + 144.0);
        assert_eq!(a.distance_squared(a), 0.0);
    }

    #[test]
    fn normalize_or_zero_gives_a_unit_vector() {
        let n = DVec3::new(3.0, 4.0, 0.0).normalize_or_zero();
        assert!((n.length() - 1.0).abs() < 1e-15);
        assert!((n.x - 0.6).abs() < 1e-15 && (n.y - 0.8).abs() < 1e-15);
    }

    /// The two normalizations are **not** bit-identical, and that is deliberate.
    ///
    /// [`DVec3::normalize`] divides each component by the length.
    /// `normalize_or_zero` multiplies by the reciprocal, because that is what
    /// `Vector3.normalize()` does (`divideScalar` is `multiplyScalar(1/s)`) and
    /// what the ported rig is pinned to. `3 / 5` is exactly `0.6`; `3 * (1/5)`
    /// is `0.6000000000000001`.
    ///
    /// This is pinned rather than left to chance because it is precisely the
    /// kind of difference someone "cleans up" — swapping one for the other looks
    /// like a no-op, compiles, and moves a golden.
    #[test]
    fn the_two_normalizations_round_differently_on_purpose() {
        let v = DVec3::new(3.0, 4.0, 0.0);
        let divided = v.normalize().expect("non-zero");
        let reciprocal = v.normalize_or_zero();
        assert_eq!(divided.x, 0.6);
        assert_eq!(reciprocal.x, 0.6000000000000001);
        assert_ne!(divided.x.to_bits(), reciprocal.x.to_bits());
    }

    /// The whole reason this exists beside the fallible `normalize`: a zero
    /// vector stays zero instead of becoming an error or a NaN.
    #[test]
    fn normalize_or_zero_leaves_the_zero_vector_alone() {
        assert_eq!(DVec3::ZERO.normalize_or_zero(), DVec3::ZERO);
        assert!(DVec3::ZERO.normalize().is_err(), "the fallible one still reports it");
    }

    #[test]
    fn normalize_or_zero_agrees_with_normalize_wherever_normalize_succeeds() {
        for v in [
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(-2.0, 5.0, 0.5),
            DVec3::new(1e-8, 1e-8, 1e-8),
        ] {
            let strict = v.normalize().expect("non-zero");
            let loose = v.normalize_or_zero();
            assert!((strict.x - loose.x).abs() < 1e-15, "{strict:?} vs {loose:?}");
            assert!((strict.y - loose.y).abs() < 1e-15, "{strict:?} vs {loose:?}");
            assert!((strict.z - loose.z).abs() < 1e-15, "{strict:?} vs {loose:?}");
        }
    }
}
