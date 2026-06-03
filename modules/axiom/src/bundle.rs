//! Bundles: the components an app spawns together onto a node.
//!
//! A `Bundle` records itself into a [`SpawnCommand`] — an umbrella-owned value
//! type — rather than touching the scene directly. The engine replays the
//! recorded commands into a `SceneApi` later (see [`crate::scene_commands`]).
//! This deferral is what lets the umbrella build a scene at all: the scene's
//! node-identity type is un-nameable behind its facade, so a node id can only
//! ever be a local during realization, never a stored field.

use axiom_math::Transform;

use crate::camera::Camera;
use crate::directional_light::DirectionalLight;
use crate::renderable::Renderable;
use crate::spin::Spin;

/// One component attached to a spawned node, recorded for deferred realization.
#[derive(Debug, Clone, Copy)]
pub enum NodeComponent {
    Renderable(Renderable),
    Camera(Camera),
    Light(DirectionalLight),
    Spin(Spin),
}

/// A recorded spawn: the node's local transform, its components, and the index
/// of its parent command (if it was spawned as a child). `pub` so it can appear
/// in the [`Bundle`] trait signature, but it lives in a private module, so an
/// app can never name it.
#[derive(Debug, Clone)]
pub struct SpawnCommand {
    pub(crate) parent: Option<usize>,
    pub(crate) transform: Transform,
    pub(crate) components: Vec<NodeComponent>,
}

impl SpawnCommand {
    pub(crate) fn new(parent: Option<usize>) -> Self {
        SpawnCommand {
            parent,
            transform: Transform::IDENTITY,
            components: Vec::new(),
        }
    }
}

/// A set of components spawned together onto one node. Implemented for each
/// component and for tuples of components, so `spawn((A, B))` records both.
pub trait Bundle {
    /// Record this bundle's transform/components into `command`.
    fn apply(self, command: &mut SpawnCommand);
}

impl Bundle for Transform {
    fn apply(self, command: &mut SpawnCommand) {
        command.transform = self;
    }
}

impl Bundle for Renderable {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::Renderable(self));
    }
}

impl Bundle for Camera {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::Camera(self));
    }
}

impl Bundle for DirectionalLight {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::Light(self));
    }
}

impl Bundle for Spin {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::Spin(self));
    }
}

impl<A: Bundle, B: Bundle> Bundle for (A, B) {
    fn apply(self, command: &mut SpawnCommand) {
        self.0.apply(command);
        self.1.apply(command);
    }
}
