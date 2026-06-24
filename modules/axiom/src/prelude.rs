//! The curated `axiom` prelude: the single barrel an app imports.
//!
//! `use axiom::prelude::*;` brings in the whole high-level surface. The skeleton
//! re-exports the math primitives and the ecs schedule phases an app names; the
//! ergonomic value types (`Mesh`, `Material`, `Assets`, `SceneCommands`, the
//! component bundles) and `App`/`DefaultPlugins` are added as features land.

pub use axiom_ecs::SchedulePhase;
// Kernel quantity types in the authoring vocabulary: `Ratio` for colour
// channels/intensities, `Meters` for camera clip planes. Re-exported so an app
// depends only on `axiom`.
pub use axiom_kernel::{Meters, Ratio};
pub use axiom_math::{Mat4, Transform, Vec2, Vec3, Vec4};

pub use crate::angle::Angle;
pub use crate::app::{App, RunningApp};
pub use crate::assets::Assets;
pub use crate::camera::{Camera, PerspectiveProjection};
pub use crate::color::Color;
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
pub use crate::renderable::Renderable;
pub use crate::scene_commands::SceneCommands;
pub use crate::spin::Spin;
pub use crate::texture::Texture;
pub use crate::window::Window;
