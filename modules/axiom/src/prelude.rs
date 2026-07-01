//! The curated `axiom` prelude: the single barrel an app imports.
//!
//! `use axiom::prelude::*;` brings in the whole high-level surface. The skeleton
//! re-exports the math primitives and the ecs schedule phases an app names; the
//! ergonomic value types (`Mesh`, `Material`, `Assets`, `SceneCommands`, the
//! component bundles) and `App`/`DefaultPlugins` are added as features land.

pub use axiom_ecs::SchedulePhase;
// Re-exported from the scene's identity vocabulary under the engine-standard
// name `Entity` (Bevy-shaped); the app-facing world API on `RunningApp` is
// built on this handle.
pub use axiom_scene::SceneNodeId as Entity;
pub use axiom_kernel::{Meters, Ratio};
// An app driving its own variable-dt run loop (a wasm `requestAnimationFrame`
// host) banks real elapsed time into whole fixed steps through these.
pub use axiom_frame::{FrameAccumulator, StepBudget};
// The embed seam (SPEC-12): `HostSessionConfig` (seed + opaque params) an app
// decodes before tick 0, and the outbound `HostOutcome` it reports once.
// `Score` is the single sanctioned f64 boundary. The browser channel that
// carries them (`postMessage`, `window.location.search`) is the app's platform
// edge, never here.
pub use axiom_host::{
    HostApi, HostMetrics, HostOutcome, HostOutcomeSet, HostParamValue, HostSessionConfig,
    HostSessionParams, PlayerId, Score,
};
pub use axiom_math::{Mat4, Transform, Vec2, Vec3, Vec4};
// `Reflect` is the trait an app implements to declare its own dynamic
// component vocabulary; the rest are the (de)serialization primitives its
// hand-written impls call.
pub use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

pub use crate::angle::Angle;
pub use crate::app::{App, RunningApp};
pub use crate::assets::Assets;
pub use crate::bounds::Bounds;
pub use crate::camera::{Camera, PerspectiveProjection};
pub use crate::color::Color;
pub use crate::component::Component;
pub use crate::contact_shadow_caster::ContactShadowCaster;
pub use crate::controller::{Controller, FirstPersonInput};
pub use crate::default_plugins::DefaultPlugins;
pub use crate::directional_light::DirectionalLight;
pub use crate::frame_outcome::{DrawData, FrameOutcome};
pub use crate::handle::Handle;
pub use crate::material::Material;
pub use crate::mesh::Mesh;
pub use crate::mesh_data::{MeshData, MeshDataError};
pub use crate::player::{Player, PlayerInput};
pub use crate::point_light::PointLight;
pub use crate::procanim::ProcAnim;
pub use crate::renderable::Renderable;
pub use crate::scene_commands::SceneCommands;
pub use crate::sdf_shape::SdfShape;
pub use crate::spawn::Spawn;
pub use crate::spin::Spin;
pub use crate::texture::Texture;
pub use crate::window::Window;
