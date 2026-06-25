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
use crate::contact_shadow_caster::ContactShadowCaster;
use crate::controller::Controller;
use crate::directional_light::DirectionalLight;
use crate::player::Player;
use crate::point_light::PointLight;
use crate::procanim::ProcAnim;
use crate::renderable::Renderable;
use crate::spin::Spin;

/// One component attached to a spawned node, recorded for deferred realization.
///
/// A tagged struct rather than a sum type: `kind` selects which payload field is
/// populated, so realization dispatches on `kind` (a `u8` compare) instead of
/// pattern-matching variants. Exactly one payload is `Some`, the one named by
/// `kind`; the constructors below are the only way to build a value, so that
/// invariant holds by construction.
#[derive(Debug, Clone, Copy)]
pub struct NodeComponent {
    kind: u8,
    renderable: Option<Renderable>,
    camera: Option<Camera>,
    light: Option<DirectionalLight>,
    point_light: Option<PointLight>,
    spin: Option<Spin>,
    procanim: Option<ProcAnim>,
    player: Option<Player>,
    controller: Option<Controller>,
}

impl NodeComponent {
    /// Component-kind tags. Exactly one payload field is populated per value,
    /// the one this tag names.
    pub(crate) const KIND_RENDERABLE: u8 = 0;
    pub(crate) const KIND_CAMERA: u8 = 1;
    pub(crate) const KIND_LIGHT: u8 = 2;
    pub(crate) const KIND_SPIN: u8 = 3;
    pub(crate) const KIND_PLAYER: u8 = 4;
    pub(crate) const KIND_CONTROLLER: u8 = 5;
    pub(crate) const KIND_POINT_LIGHT: u8 = 6;
    pub(crate) const KIND_CONTACT_SHADOW_CASTER: u8 = 7;
    pub(crate) const KIND_PROCANIM: u8 = 8;

    /// The all-`None` base used by every constructor; each then fills in its one
    /// payload field and sets its `kind`.
    const EMPTY: Self = NodeComponent {
        kind: Self::KIND_RENDERABLE,
        renderable: None,
        camera: None,
        light: None,
        point_light: None,
        spin: None,
        procanim: None,
        player: None,
        controller: None,
    };

    /// Which kind of component this is (see the `KIND_*` tags).
    pub(crate) fn kind(&self) -> u8 {
        self.kind
    }

    /// A renderable component.
    pub(crate) fn renderable(renderable: Renderable) -> Self {
        NodeComponent {
            kind: Self::KIND_RENDERABLE,
            renderable: Some(renderable),
            ..Self::EMPTY
        }
    }

    /// A camera component.
    pub(crate) fn camera(camera: Camera) -> Self {
        NodeComponent {
            kind: Self::KIND_CAMERA,
            camera: Some(camera),
            ..Self::EMPTY
        }
    }

    /// A directional-light component.
    pub(crate) fn light(light: DirectionalLight) -> Self {
        NodeComponent {
            kind: Self::KIND_LIGHT,
            light: Some(light),
            ..Self::EMPTY
        }
    }

    /// A point-light component.
    pub(crate) fn point_light(point_light: PointLight) -> Self {
        NodeComponent {
            kind: Self::KIND_POINT_LIGHT,
            point_light: Some(point_light),
            ..Self::EMPTY
        }
    }

    /// A spin component.
    pub(crate) fn spin(spin: Spin) -> Self {
        NodeComponent {
            kind: Self::KIND_SPIN,
            spin: Some(spin),
            ..Self::EMPTY
        }
    }

    /// A procedural-animation component.
    pub(crate) fn procanim(procanim: ProcAnim) -> Self {
        NodeComponent {
            kind: Self::KIND_PROCANIM,
            procanim: Some(procanim),
            ..Self::EMPTY
        }
    }

    /// A player-marker component.
    pub(crate) fn player(player: Player) -> Self {
        NodeComponent {
            kind: Self::KIND_PLAYER,
            player: Some(player),
            ..Self::EMPTY
        }
    }

    /// A controller-marker component.
    pub(crate) fn controller(controller: Controller) -> Self {
        NodeComponent {
            kind: Self::KIND_CONTROLLER,
            controller: Some(controller),
            ..Self::EMPTY
        }
    }

    /// A contact-shadow-caster marker component. It carries no data — the kind
    /// is the whole signal — so there is no payload to store or read back.
    pub(crate) fn contact_shadow_caster() -> Self {
        NodeComponent {
            kind: Self::KIND_CONTACT_SHADOW_CASTER,
            ..Self::EMPTY
        }
    }

    /// The renderable payload, present iff `kind == KIND_RENDERABLE`.
    pub(crate) fn as_renderable(&self) -> Option<&Renderable> {
        self.renderable.as_ref()
    }

    /// The camera payload, present iff `kind == KIND_CAMERA`.
    pub(crate) fn as_camera(&self) -> Option<&Camera> {
        self.camera.as_ref()
    }

    /// The directional-light payload, present iff `kind == KIND_LIGHT`.
    pub(crate) fn as_light(&self) -> Option<&DirectionalLight> {
        self.light.as_ref()
    }

    /// The point-light payload, present iff `kind == KIND_POINT_LIGHT`.
    pub(crate) fn as_point_light(&self) -> Option<&PointLight> {
        self.point_light.as_ref()
    }

    /// The spin payload, present iff `kind == KIND_SPIN`.
    pub(crate) fn as_spin(&self) -> Option<&Spin> {
        self.spin.as_ref()
    }

    /// The procedural-animation payload, present iff `kind == KIND_PROCANIM`.
    pub(crate) fn as_procanim(&self) -> Option<&ProcAnim> {
        self.procanim.as_ref()
    }

    /// The player payload, present iff `kind == KIND_PLAYER`.
    pub(crate) fn as_player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    /// The controller payload, present iff `kind == KIND_CONTROLLER`.
    pub(crate) fn as_controller(&self) -> Option<&Controller> {
        self.controller.as_ref()
    }
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
        command.components.push(NodeComponent::renderable(self));
    }
}

impl Bundle for Camera {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::camera(self));
    }
}

impl Bundle for DirectionalLight {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::light(self));
    }
}

impl Bundle for PointLight {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::point_light(self));
    }
}

impl Bundle for Spin {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::spin(self));
    }
}

impl Bundle for ProcAnim {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::procanim(self));
    }
}

impl Bundle for Player {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::player(self));
    }
}

impl Bundle for Controller {
    fn apply(self, command: &mut SpawnCommand) {
        command.components.push(NodeComponent::controller(self));
    }
}

impl Bundle for ContactShadowCaster {
    fn apply(self, command: &mut SpawnCommand) {
        command
            .components
            .push(NodeComponent::contact_shadow_caster());
    }
}

impl<A: Bundle, B: Bundle> Bundle for (A, B) {
    fn apply(self, command: &mut SpawnCommand) {
        self.0.apply(command);
        self.1.apply(command);
    }
}

impl<A: Bundle, B: Bundle, C: Bundle> Bundle for (A, B, C) {
    fn apply(self, command: &mut SpawnCommand) {
        self.0.apply(command);
        self.1.apply(command);
        self.2.apply(command);
    }
}

impl<A: Bundle, B: Bundle, C: Bundle, D: Bundle> Bundle for (A, B, C, D) {
    fn apply(self, command: &mut SpawnCommand) {
        self.0.apply(command);
        self.1.apply(command);
        self.2.apply(command);
        self.3.apply(command);
    }
}
