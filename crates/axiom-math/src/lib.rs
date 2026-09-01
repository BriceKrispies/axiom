//! # Axiom Math — Layer 02
//!
//! The deterministic math and geometry substrate. Provides the scalar policy,
//! vectors, quaternions, 4x4 matrices, transforms, AABBs, spheres, rays,
//! planes, and frusta that every later engine layer will build on.
//!
//! ## Contact queries
//! On top of those shapes sit the primitives a character controller, a
//! hitbox test and a navigation probe are all made of: [`Segment`],
//! [`Capsule`], [`Triangle`] and [`Obb`], the closest-point solves between
//! them ([`Segment::closest_points_to_segment`],
//! [`Segment::closest_points_to_triangle`], [`Triangle::closest_point_to`]),
//! the casts ([`Triangle::raycast`], [`Triangle::intersect_segment`],
//! [`Capsule::raycast`], [`Obb::raycast`]) and the sweeps
//! ([`Sphere::sweep_triangle`], [`Capsule::sweep_triangle`],
//! [`Capsule::sweep_capsule`]). Every one of them answers with the same
//! record, [`Hit`]: the time of impact, the contact point on the struck
//! surface, and the normal of that surface facing the mover.
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

/// The double-precision vector family. See [`Scalar`] for when to reach for it
/// and, more importantly, when not to.
mod dvec3;

mod quat;

mod mat3;
mod mat4;

mod transform;

mod aabb;
mod frustum;
mod obb;
mod plane;
mod plane_side;
mod ray;
mod sphere;

mod capsule;
mod hit;
mod segment;
mod triangle;

mod capsule_cast;
mod capsule_sweep;
mod sphere_sweep;
mod triangle_cast;

mod curve;
mod curve_kind;
mod curve_sample;

mod geo;

mod math_api;

pub use math_api::MathApi;

pub use approx_eq::ApproxEq;
pub use epsilon::Epsilon;
pub use scalar::Scalar;

pub use math_error::MathError;
pub use math_error_code::MathErrorCode;
pub use math_result::MathResult;

pub use mat3::Mat3;
pub use mat4::Mat4;
pub use quat::Quat;
pub use transform::Transform;
pub use vec2::Vec2;
pub use vec3::Vec3;
pub use vec4::Vec4;

pub use dvec3::DVec3;

pub use aabb::Aabb;
pub use frustum::Frustum;
pub use obb::Obb;
pub use plane::Plane;
pub use plane_side::PlaneSide;
pub use ray::Ray;
pub use sphere::Sphere;

pub use capsule::Capsule;
pub use hit::Hit;
pub use segment::Segment;
pub use triangle::Triangle;

pub use curve::Curve;
pub use curve_kind::CurveKind;
pub use curve_sample::CurveSample;

pub use geo::great_circle_distance;
pub use geo::latitude;
pub use geo::longitude;
pub use geo::slerp;
pub use geo::tangent_basis;
pub use geo::unit_dir_from_lat_lon;
pub use geo::unit_vec3;
