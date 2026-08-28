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
pub use axiom_kernel::{Meters, Ratio};
pub use axiom_scene::SceneNodeId as Entity;
// An app driving its own variable-dt run loop (a wasm `requestAnimationFrame`
// host) banks real elapsed time into whole fixed steps through these.
pub use axiom_frame::{FrameAccumulator, StepBudget};
// The startup preparation contract: the trait an app implements for launch-only
// work and hands to `App::prepare_with`. Re-exported here because most apps do
// not Cargo-depend on `axiom-runtime`, and adding that dependency purely to name
// a trait would be a ceremonial dependency.
pub use axiom_runtime::PreparationTask;
// The embed seam (SPEC-12): `HostSessionConfig` (seed + opaque params) an app
// decodes before tick 0, and the outbound `HostOutcome` it reports once.
// `Score` is the single sanctioned f64 boundary. The browser channel that
// carries them (`postMessage`, `window.location.search`) is the app's platform
// edge, never here.
pub use axiom_host::{
    // `FrameCamera` is app-facing because presenting a frame now means handing
    // over the camera whole — view, projection and their product. An app builds
    // one from the three `FrameOutcome` accessors; it is not something only the
    // engine names.
    // `FrameCloudDetail` shapes the cloud field `FrameSky` evaluates — an app
    // authoring weather needs to name it for the same reason it names the sky.
    FrameAmbient, FrameBloom, FrameCamera, FrameCloudDetail, FrameDepthFog, FrameIndirect,
    FramePostProcess, FrameSky, FrameTonemap,
    HostApi,
    HostMetrics,
    HostOutcome, HostOutcomeSet, HostParamValue, HostSessionConfig, HostSessionParams, PlayerId,
    Score,
};
pub use axiom_math::{Mat4, Transform, Vec2, Vec3, Vec4};
// The neutral appearance artifact `Material::from_surface` takes, and the
// builder that authors one. Re-exported here for the same reason
// `PreparationTask` is: an app should not Cargo-depend on a layer purely to name
// the argument of a prelude method.
pub use axiom_surface::{
    // `runtime_material` + `MaterialParams` author the hand-written runtime
    // material shader — the port of Claude-of-Duty's `materials/shader.js`. They
    // belong in the prelude for the same reason `SurfaceBuilder` does: an app
    // authors appearance through this vocabulary and should never have to reach
    // past the engine umbrella to a layer crate to do it.
    runtime_material, LightingModel, MaterialParams, Surface, SurfaceBuilder, SurfaceChannel,
    SurfaceKind, UvMode,
};
// `Reflect` is the trait an app implements to declare its own dynamic
// component vocabulary; the rest are the (de)serialization primitives its
// hand-written impls call.
pub use axiom_kernel::{
    BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema,
};

pub use crate::angle::Angle;
pub use crate::app::{App, RunningApp, TextureDataError};
pub use crate::assets::Assets;
pub use crate::bounds::Bounds;
pub use crate::camera::{Camera, PerspectiveProjection};
pub use crate::color::Color;
pub use crate::component::Component;
pub use crate::contact_shadow_caster::ContactShadowCaster;
pub use crate::controller::{Controller, FirstPersonInput};
pub use crate::default_plugins::DefaultPlugins;
pub use crate::directional_light::DirectionalLight;
pub use crate::frame_outcome::{DrawData, FrameOutcome, SkinnedDraw};
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
// How a material's texture is filtered as it minifies — `Crisp` (the default,
// hard magnified texels) or `Anisotropic` (for ground surfaces seen at a grazing
// angle across a wide depth range).
pub use axiom_host::TextureSampling;
pub use crate::visible::Visible;
pub use crate::window::Window;
