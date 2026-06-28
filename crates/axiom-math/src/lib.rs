//! # Axiom Math — Layer 02
//!
//! The deterministic math and geometry substrate. Provides the scalar policy,
//! vectors, quaternions, 4x4 matrices, transforms, AABBs, spheres, rays,
//! planes, and frusta that every later engine layer will build on.
//!
//! ## Public surface
//! `lib.rs` exposes exactly one public item: [`MathApi`]. Every internal
//! module file owns exactly one primary public type and lives behind a
//! private `mod`. Callers reach every math capability through the [`MathApi`]
//! facade.
//!
//! ## What this layer is not allowed to know
//! Rendering, WebGPU/WebGL, DOM, browser APIs, assets, physics, animation,
//! audio, ECS, scenes, input mapping, plugins, editor surfaces, async host
//! integration, or any game-specific concept. Determinism, finite scalars,
//! and checked failures are mandatory.

mod approx_eq;
mod epsilon;
mod math_error;
mod math_error_code;
mod math_result;
mod scalar;

mod vec2;
mod vec3;
mod vec4;

mod quat;

mod mat3;
mod mat4;

mod transform;

mod aabb;
mod frustum;
mod plane;
mod plane_side;
mod ray;
mod sphere;

mod math_api;

// --- Public surface (curated; see `tests/architecture.rs`) ---

// Primary entry point.
pub use math_api::MathApi;

// Scalar policy primitives — higher layers/modules need to name these to
// validate inputs and compare values without taking on a parallel finite
// scalar discipline.
pub use approx_eq::ApproxEq;
pub use epsilon::Epsilon;
pub use scalar::Scalar;

// Error / result primitives — higher layers return `MathResult` and match
// on `(code, optional kernel cause)` identity.
pub use math_error::MathError;
pub use math_error_code::MathErrorCode;
pub use math_result::MathResult;

// Workhorse value types — higher layers/modules need to *name* these to
// store transforms, build cameras, declare bounds, and so on. None of
// them carry hidden state; they are plain data with deterministic
// methods.
pub use mat3::Mat3;
pub use mat4::Mat4;
pub use quat::Quat;
pub use transform::Transform;
pub use vec2::Vec2;
pub use vec3::Vec3;
pub use vec4::Vec4;

// Geometry primitives — used by frame snapshots, scene bounding volumes,
// and future culling/picking modules.
pub use aabb::Aabb;
pub use frustum::Frustum;
pub use plane::Plane;
pub use plane_side::PlaneSide;
pub use ray::Ray;
pub use sphere::Sphere;
