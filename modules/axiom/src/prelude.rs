//! The curated `axiom` prelude: the single barrel an app imports.
//!
//! `use axiom::prelude::*;` brings in the whole high-level surface. The skeleton
//! re-exports the math primitives and the ecs schedule phases an app names; the
//! ergonomic value types (`Mesh`, `Material`, `Assets`, `SceneCommands`, the
//! component bundles) and `App`/`DefaultPlugins` are added as features land.

pub use axiom_ecs::SchedulePhase;
// The first-class entity handle an app holds: spawn returns one, and despawn /
// component access / spatial queries take one. Re-exported from the scene's
// identity vocabulary under the engine-standard name `Entity` (Bevy-shaped). The
// app-facing world API on `RunningApp` is built on this handle.
pub use axiom_scene::SceneNodeId as Entity;
// Kernel quantity types in the authoring vocabulary: `Ratio` for colour
// channels/intensities, `Meters` for camera clip planes. Re-exported so an app
// depends only on `axiom`.
pub use axiom_kernel::{Meters, Ratio};
// The deterministic fixed-step accumulator and its integer step budget, from the
// `frame` layer the umbrella already composes. An app driving its own variable-dt
// run loop (a wasm `requestAnimationFrame` host) banks real elapsed time into
// whole fixed steps through these, so they belong in the one authoring barrel an
// app imports — re-exported, not re-derived (the loop arithmetic lives in `frame`).
pub use axiom_frame::{FrameAccumulator, StepBudget};
pub use axiom_math::{Mat4, Transform, Vec2, Vec3, Vec4};

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
pub use crate::player::{Player, PlayerInput};
pub use crate::point_light::PointLight;
pub use crate::procanim::ProcAnim;
pub use crate::renderable::Renderable;
pub use crate::scene_commands::SceneCommands;
pub use crate::spawn::Spawn;
pub use crate::spin::Spin;
pub use crate::texture::Texture;
pub use crate::window::Window;
