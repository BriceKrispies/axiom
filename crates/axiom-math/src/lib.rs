//! # Axiom Math — Layer 02
//!
//! The deterministic math and geometry substrate. Provides the scalar policy,
//! vectors, quaternions, 4x4 matrices, transforms, AABBs, spheres, rays,
//! planes, and frusta that every later engine layer will build on.
//!
//! ## Public surface
//! The behavioral facade is [`MathApi`], and alongside it `lib.rs` re-exports
//! the workhorse value types (`Vec3`, `Quat`, `Mat4`, geometry primitives, …)
//! future layers must be able to *name*. The one exception to "one primary
//! public type per module" is [`mod@geo`], a small set of spherical / geodesic
//! *free functions* over unit directions ([`latitude`], [`longitude`],
//! [`great_circle_distance`], [`tangent_basis`], [`unit_dir_from_lat_lon`],
//! [`slerp`], [`unit_vec3`]): they are pure transforms of `Vec3` directions and kernel
//! angle/ratio quantities, with no type of their own to hang them on, so callers
//! name them directly (`axiom_math::latitude(dir)`). Every internal module lives
//! behind a private `mod`; the curated public set is pinned by
//! `tests/architecture.rs`.
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

mod geo;

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

// Spherical / geodesic operations over unit directions — latitude/longitude,
// great-circle distance, tangent frames, and spherical interpolation. Unlike the
// items above these are free functions, not a value type: they transform `Vec3`
// directions plus kernel angle/ratio quantities, so callers name them directly
// (`axiom_math::latitude(dir)`) the way they name `Vec3::new`. Angles are
// `Radians` and blends are `Ratio`, so no unit is left to guess.
pub use geo::great_circle_distance;
pub use geo::latitude;
pub use geo::longitude;
pub use geo::slerp;
pub use geo::tangent_basis;
pub use geo::unit_dir_from_lat_lon;
pub use geo::unit_vec3;
