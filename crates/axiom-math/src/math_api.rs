//! The Layer-02 math facade.

use axiom_kernel::{KernelApi, Tick};
use axiom_runtime::RuntimeContext;
// `TelemetryMetric` is the previous-layer-adjacent primitive Math hands to the
// runtime sink; importing it here is what makes Math a real semantic adapter
// over the runtime's telemetry surface (not just over the kernel facade).
use axiom_kernel::TelemetryMetric;

use crate::aabb::Aabb;
use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::frustum::Frustum;
use crate::mat4::Mat4;
use crate::math_result::MathResult;
use crate::plane::Plane;
use crate::plane_side::PlaneSide;
use crate::quat::Quat;
use crate::ray::Ray;
use crate::scalar::Scalar;
use crate::sphere::Sphere;
use crate::transform::Transform;
use crate::vec2::Vec2;
use crate::vec3::Vec3;
use crate::vec4::Vec4;

/// The single public entry point to the Axiom math layer.
///
/// `MathApi` is the *only* item `lib.rs` exports. Every math capability —
/// scalar/epsilon policy, vector, quaternion, matrix, transform, geometry —
/// is reached through one of its constructors. The facade is a zero-sized
/// value; it holds no state and reads nothing ambient.
///
/// `MathApi` is also the layer's adapter over the runtime: methods that take
/// a `&mut RuntimeContext` route deterministic telemetry through the kernel
/// sinks the runtime owns ([`Self::record_validation_failure`],
/// [`Self::record_intersection_test`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct MathApi {
    _sealed: (),
}

impl MathApi {
    /// The metric name used by [`Self::record_validation_failure`].
    pub const VALIDATION_FAILURE_METRIC: &'static str = "math.validation_failure";
    /// The metric name used by [`Self::record_intersection_test`].
    pub const INTERSECTION_TEST_METRIC: &'static str = "math.intersection_test";

    /// Construct the facade.
    pub const fn new() -> Self {
        MathApi { _sealed: () }
    }

    /// Whether the kernel this math layer is paired with serializes in
    /// little-endian order. Math's binary format inherits that contract;
    /// callers can assert it before they hand bytes to a future codec layer.
    pub fn serializes_little_endian(&self, kernel: &KernelApi) -> bool {
        kernel.serializes_little_endian()
    }

    // --- Scalar / epsilon policy ---

    /// Return `v` if finite, otherwise produce a math error.
    pub fn validate_finite(&self, v: f32) -> MathResult<f32> {
        Scalar::validate_finite(v)
    }

    /// Whether `v` is finite (neither `NaN` nor `±Inf`).
    pub fn is_finite_value(&self, v: f32) -> bool {
        Scalar::is_finite_value(v)
    }

    /// The engine-default epsilon.
    pub fn default_epsilon(&self) -> Epsilon {
        Epsilon::DEFAULT
    }

    /// Construct a validated tolerance.
    pub fn epsilon(&self, value: f32) -> MathResult<Epsilon> {
        Epsilon::new(value)
    }

    /// Compare two values under the supplied tolerance.
    pub fn approx_eq<T: ApproxEq>(&self, a: &T, b: &T, epsilon: Epsilon) -> bool {
        a.approx_eq(b, epsilon)
    }

    // --- Vectors ---

    pub fn vec2(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }
    pub fn vec2_zero(&self) -> Vec2 {
        Vec2::ZERO
    }
    pub fn vec2_one(&self) -> Vec2 {
        Vec2::ONE
    }

    pub fn vec3(&self, x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }
    pub fn vec3_zero(&self) -> Vec3 {
        Vec3::ZERO
    }
    pub fn vec3_one(&self) -> Vec3 {
        Vec3::ONE
    }
    pub fn vec3_unit_x(&self) -> Vec3 {
        Vec3::UNIT_X
    }
    pub fn vec3_unit_y(&self) -> Vec3 {
        Vec3::UNIT_Y
    }
    pub fn vec3_unit_z(&self) -> Vec3 {
        Vec3::UNIT_Z
    }

    pub fn vec4(&self, x: f32, y: f32, z: f32, w: f32) -> Vec4 {
        Vec4::new(x, y, z, w)
    }

    // --- Quaternions ---

    pub fn quat_identity(&self) -> Quat {
        Quat::IDENTITY
    }

    pub fn quat_from_axis_angle(&self, axis: Vec3, angle_radians: f32) -> MathResult<Quat> {
        Quat::from_axis_angle(axis, angle_radians)
    }

    // --- Matrices ---

    pub fn mat4_identity(&self) -> Mat4 {
        Mat4::IDENTITY
    }

    pub fn mat4_translation(&self, t: Vec3) -> Mat4 {
        Mat4::translation(t)
    }

    pub fn mat4_scale(&self, s: Vec3) -> Mat4 {
        Mat4::scale(s)
    }

    pub fn mat4_from_quaternion(&self, q: Quat) -> Mat4 {
        Mat4::from_quaternion(q)
    }

    pub fn mat4_perspective(
        &self,
        fovy_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> MathResult<Mat4> {
        Mat4::perspective(fovy_radians, aspect, near, far)
    }

    pub fn mat4_orthographic(
        &self,
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        near: f32,
        far: f32,
    ) -> MathResult<Mat4> {
        Mat4::orthographic(left, right, bottom, top, near, far)
    }

    pub fn mat4_look_at(&self, eye: Vec3, target: Vec3, up: Vec3) -> MathResult<Mat4> {
        Mat4::look_at(eye, target, up)
    }

    // --- Transforms ---

    pub fn transform_identity(&self) -> Transform {
        Transform::IDENTITY
    }

    pub fn transform_from_translation(&self, t: Vec3) -> Transform {
        Transform::from_translation(t)
    }

    pub fn transform_from_rotation(&self, r: Quat) -> Transform {
        Transform::from_rotation(r)
    }

    pub fn transform_from_scale(&self, s: Vec3) -> Transform {
        Transform::from_scale(s)
    }

    pub fn transform(&self, translation: Vec3, rotation: Quat, scale: Vec3) -> Transform {
        Transform::new(translation, rotation, scale)
    }

    pub fn combine_transforms(&self, parent: Transform, child: Transform) -> Transform {
        Transform::combine(parent, child)
    }

    // --- Geometry primitives ---

    pub fn aabb(&self, min: Vec3, max: Vec3) -> MathResult<Aabb> {
        Aabb::new(min, max)
    }

    pub fn aabb_from_center_extents(&self, center: Vec3, extents: Vec3) -> MathResult<Aabb> {
        Aabb::from_center_extents(center, extents)
    }

    pub fn sphere(&self, center: Vec3, radius: f32) -> MathResult<Sphere> {
        Sphere::new(center, radius)
    }

    pub fn ray(&self, origin: Vec3, direction: Vec3) -> MathResult<Ray> {
        Ray::new(origin, direction)
    }

    pub fn plane(&self, normal: Vec3, distance: f32) -> MathResult<Plane> {
        Plane::new(normal, distance)
    }

    pub fn frustum_from_view_projection(&self, clip_from_world: Mat4) -> MathResult<Frustum> {
        Frustum::from_view_projection(clip_from_world)
    }

    pub fn classify_point_against_plane(
        &self,
        plane: &Plane,
        point: Vec3,
        epsilon: Epsilon,
    ) -> PlaneSide {
        plane.classify_point(point, epsilon)
    }

    // --- Math telemetry through the runtime ---

    /// Emit a `math.validation_failure` counter into the runtime's sink.
    /// Used by higher layers to track how often math validation rejects
    /// authoring inputs.
    pub fn record_validation_failure(&self, ctx: &mut RuntimeContext<'_>) {
        let tick: Tick = ctx.step().tick();
        ctx.metric(TelemetryMetric::counter(
            Self::VALIDATION_FAILURE_METRIC,
            1,
            Some(tick),
        ));
    }

    /// Emit a `math.intersection_test` counter. Higher layers (culling,
    /// picking) call this once per intersection query so the load is visible
    /// in deterministic telemetry.
    pub fn record_intersection_test(&self, ctx: &mut RuntimeContext<'_>) {
        let tick: Tick = ctx.step().tick();
        ctx.metric(TelemetryMetric::counter(
            Self::INTERSECTION_TEST_METRIC,
            1,
            Some(tick),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{FrameIndex, KernelApi};
    use axiom_runtime::{RuntimeCommandQueue, RuntimeEventQueue, RuntimeStep};

    fn api() -> MathApi {
        MathApi::new()
    }

    #[test]
    fn new_and_default_are_equivalent() {
        assert_eq!(
            MathApi::new().default_epsilon().value(),
            MathApi::default().default_epsilon().value(),
        );
    }

    #[test]
    fn endian_contract_inherits_from_kernel() {
        let kernel = KernelApi::new();
        assert!(api().serializes_little_endian(&kernel));
    }

    #[test]
    fn finite_validation_routes_through_scalar_policy() {
        assert_eq!(api().validate_finite(2.5).unwrap(), 2.5);
        assert!(api().validate_finite(f32::NAN).is_err());
        assert!(api().is_finite_value(0.0));
        assert!(!api().is_finite_value(f32::INFINITY));
    }

    #[test]
    fn epsilon_constructor_rejects_invalid_inputs() {
        assert_eq!(api().default_epsilon(), Epsilon::DEFAULT);
        assert!(api().epsilon(1.0e-3).is_ok());
        assert!(api().epsilon(-1.0).is_err());
    }

    #[test]
    fn approx_eq_compares_under_supplied_tolerance() {
        let a = api().vec3(1.0, 2.0, 3.0);
        let b = api().vec3(1.0, 2.0, 3.0 + 1.0e-7);
        assert!(api().approx_eq(&a, &b, api().default_epsilon()));
    }

    // Kills `replace MathApi::approx_eq -> bool with true` at math_api.rs:85.
    // Two clearly distinct vectors must compare NOT approx-equal.
    #[test]
    fn approx_eq_returns_false_for_distinct_values() {
        let a = api().vec3(1.0, 2.0, 3.0);
        let b = api().vec3(1.0, 2.0, 9.0);
        assert!(!api().approx_eq(&a, &b, api().default_epsilon()));
    }

    #[test]
    fn vector_constructors_match_module_constants() {
        let m = api();
        let eps = m.default_epsilon();
        assert!(m.vec2(1.0, 2.0).approx_eq(&Vec2::new(1.0, 2.0), eps));
        assert!(m.vec2_zero().approx_eq(&Vec2::ZERO, eps));
        assert!(m.vec2_one().approx_eq(&Vec2::ONE, eps));

        assert!(m
            .vec3(1.0, 2.0, 3.0)
            .approx_eq(&Vec3::new(1.0, 2.0, 3.0), eps));
        assert!(m.vec3_zero().approx_eq(&Vec3::ZERO, eps));
        assert!(m.vec3_one().approx_eq(&Vec3::ONE, eps));
        assert!(m.vec3_unit_x().approx_eq(&Vec3::UNIT_X, eps));
        assert!(m.vec3_unit_y().approx_eq(&Vec3::UNIT_Y, eps));
        assert!(m.vec3_unit_z().approx_eq(&Vec3::UNIT_Z, eps));

        assert!(m
            .vec4(1.0, 2.0, 3.0, 4.0)
            .approx_eq(&Vec4::new(1.0, 2.0, 3.0, 4.0), eps));
    }

    #[test]
    fn quaternion_helpers_round_trip() {
        let m = api();
        let eps = m.epsilon(1.0e-5).unwrap();
        assert!(m.quat_identity().approx_eq(&Quat::IDENTITY, eps));
        let q = m
            .quat_from_axis_angle(m.vec3_unit_z(), std::f32::consts::FRAC_PI_2)
            .unwrap();
        assert!(q.rotate(m.vec3_unit_x()).approx_eq(&m.vec3_unit_y(), eps));
    }

    #[test]
    fn matrix_helpers_round_trip() {
        let m = api();
        let eps = m.epsilon(1.0e-5).unwrap();
        assert!(m.mat4_identity().approx_eq(&Mat4::IDENTITY, eps));
        let t = m.mat4_translation(m.vec3(1.0, 2.0, 3.0));
        assert!(t
            .transform_point(m.vec3_zero())
            .approx_eq(&m.vec3(1.0, 2.0, 3.0), eps));
        let s = m.mat4_scale(m.vec3(2.0, 2.0, 2.0));
        assert!(s
            .transform_point(m.vec3(1.0, 1.0, 1.0))
            .approx_eq(&m.vec3(2.0, 2.0, 2.0), eps));
        let q = m
            .quat_from_axis_angle(m.vec3_unit_z(), std::f32::consts::FRAC_PI_2)
            .unwrap();
        assert!(m
            .mat4_from_quaternion(q)
            .transform_vector(m.vec3_unit_x())
            .approx_eq(&m.vec3_unit_y(), eps));
        assert!(m
            .mat4_perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 100.0)
            .is_ok());
        assert!(m.mat4_orthographic(-1.0, 1.0, -1.0, 1.0, 0.0, 1.0).is_ok());
        assert!(m
            .mat4_look_at(m.vec3(0.0, 0.0, 5.0), m.vec3_zero(), m.vec3_unit_y(),)
            .is_ok());
    }

    #[test]
    fn transform_helpers_round_trip() {
        let m = api();
        let eps = m.epsilon(1.0e-5).unwrap();
        assert!(m.transform_identity().approx_eq(&Transform::IDENTITY, eps));
        assert!(m
            .transform_from_translation(m.vec3(1.0, 0.0, 0.0))
            .transform_point(m.vec3_zero())
            .approx_eq(&m.vec3(1.0, 0.0, 0.0), eps));
        assert!(m
            .transform_from_rotation(m.quat_identity())
            .approx_eq(&Transform::IDENTITY, eps));
        assert!(m
            .transform_from_scale(m.vec3_one())
            .approx_eq(&Transform::IDENTITY, eps));
        let composed = m.combine_transforms(
            m.transform(m.vec3(1.0, 0.0, 0.0), m.quat_identity(), m.vec3_one()),
            m.transform(m.vec3(0.0, 1.0, 0.0), m.quat_identity(), m.vec3_one()),
        );
        assert!(composed
            .transform_point(m.vec3_zero())
            .approx_eq(&m.vec3(1.0, 1.0, 0.0), eps));
    }

    #[test]
    fn geometry_constructors_validate_inputs() {
        let m = api();
        assert!(m.aabb(m.vec3_zero(), m.vec3_one()).is_ok());
        assert!(m
            .aabb_from_center_extents(m.vec3_zero(), m.vec3(1.0, 1.0, 1.0))
            .is_ok());
        assert!(m.sphere(m.vec3_zero(), 1.0).is_ok());
        assert!(m.sphere(m.vec3_zero(), -1.0).is_err());
        assert!(m.ray(m.vec3_zero(), m.vec3_unit_x()).is_ok());
        assert!(m.ray(m.vec3_zero(), m.vec3_zero()).is_err());
        assert!(m.plane(m.vec3_unit_z(), 0.0).is_ok());
        assert!(m.plane(m.vec3_zero(), 0.0).is_err());
        let frustum = m
            .frustum_from_view_projection(
                m.mat4_perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 100.0)
                    .unwrap(),
            )
            .unwrap();
        assert!(frustum.contains_point(m.vec3(0.0, 0.0, -10.0)));
    }

    #[test]
    fn classify_point_against_plane_delegates() {
        let m = api();
        let plane = m.plane(m.vec3_unit_z(), 0.0).unwrap();
        assert_eq!(
            m.classify_point_against_plane(&plane, m.vec3(0.0, 0.0, 1.0), m.default_epsilon(),),
            PlaneSide::Front,
        );
        assert_eq!(
            m.classify_point_against_plane(&plane, m.vec3_zero(), m.default_epsilon()),
            PlaneSide::On,
        );
    }

    fn fresh_runtime_context<'r>(
        kernel: &'r KernelApi,
        commands: &'r mut RuntimeCommandQueue,
        events: &'r mut RuntimeEventQueue,
        logs: &'r mut axiom_kernel::InMemoryLogSink,
        telemetry: &'r mut axiom_kernel::InMemoryTelemetrySink,
        step_index: u64,
    ) -> RuntimeContext<'r> {
        RuntimeContext::new(
            RuntimeStep::new(FrameIndex::new(step_index), Tick::new(step_index), 1_000, 0),
            commands,
            events,
            kernel,
            logs,
            telemetry,
        )
    }

    #[test]
    fn record_validation_failure_routes_through_runtime_context() {
        let m = api();
        let kernel = KernelApi::new();
        let mut commands = RuntimeCommandQueue::new();
        let mut events = RuntimeEventQueue::new();
        let mut logs = kernel.log_sink();
        let mut telemetry = kernel.telemetry_sink();

        {
            let mut ctx = fresh_runtime_context(
                &kernel,
                &mut commands,
                &mut events,
                &mut logs,
                &mut telemetry,
                0,
            );
            m.record_validation_failure(&mut ctx);
            m.record_validation_failure(&mut ctx);
        }

        assert_eq!(
            telemetry.counter_total(MathApi::VALIDATION_FAILURE_METRIC),
            2
        );
    }

    #[test]
    fn record_intersection_test_routes_through_runtime_context() {
        let m = api();
        let kernel = KernelApi::new();
        let mut commands = RuntimeCommandQueue::new();
        let mut events = RuntimeEventQueue::new();
        let mut logs = kernel.log_sink();
        let mut telemetry = kernel.telemetry_sink();

        {
            let mut ctx = fresh_runtime_context(
                &kernel,
                &mut commands,
                &mut events,
                &mut logs,
                &mut telemetry,
                7,
            );
            m.record_intersection_test(&mut ctx);
        }

        assert_eq!(
            telemetry.counter_total(MathApi::INTERSECTION_TEST_METRIC),
            1
        );
    }
}
