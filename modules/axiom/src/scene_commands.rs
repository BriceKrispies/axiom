//! `SceneCommands`: the deferred scene-authoring surface an app's setup uses.
//!
//! `spawn`/`with_child` record [`SpawnCommand`]s; the engine calls
//! [`SceneCommands::realize_into`] once to replay them into a `SceneApi`. The
//! whole replay happens in that single function, so the scene's un-nameable node
//! ids only ever live as locals (`nodes: Vec<_>`).

use axiom_kernel::{Radians, Ratio};
use axiom_math::{MathApi, Vec3};
use axiom_scene::SceneApi;

use crate::bundle::{Bundle, NodeComponent, SpawnCommand};

/// The scene-authoring command buffer handed to an app's setup. It accumulates
/// spawns as plain umbrella values; nothing touches the scene until the engine
/// realizes it.
#[derive(Debug)]
pub struct SceneCommands {
    aspect: f32,
    commands: Vec<SpawnCommand>,
}

impl SceneCommands {
    /// Construct an empty command buffer for a viewport of the given aspect
    /// ratio (cameras resolve their projection against it).
    pub(crate) fn new(aspect: f32) -> Self {
        SceneCommands {
            aspect,
            commands: Vec::new(),
        }
    }

    /// How many renderable components were authored across all spawns. The live
    /// backend sizes its per-instance buffer to this (one instance per drawn
    /// renderable).
    pub(crate) fn renderable_count(&self) -> usize {
        self.commands
            .iter()
            .flat_map(|command| command.components.iter())
            .filter(|component| matches!(component, NodeComponent::Renderable(_)))
            .count()
    }

    /// Spawn a node carrying `bundle`. Returns a handle for attaching children.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> SpawnedNode<'_> {
        let index = self.record(None, bundle);
        SpawnedNode {
            commands: self,
            index,
        }
    }

    /// Record a spawn (optionally as a child of `parent`) and return its index.
    fn record<B: Bundle>(&mut self, parent: Option<usize>, bundle: B) -> usize {
        let mut command = SpawnCommand::new(parent);
        bundle.apply(&mut command);
        self.commands.push(command);
        self.commands.len() - 1
    }

    /// Replay the recorded spawns into `scene`, returning the world-space
    /// direction of the last directional light (the engine's per-frame light
    /// direction), if any. Parents are recorded before their children, so a
    /// child's parent node always already exists.
    pub(crate) fn realize_into(self, scene: &mut SceneApi, math: &MathApi) -> Option<Vec3> {
        let mut nodes = Vec::with_capacity(self.commands.len());
        let mut light_direction = None;
        self.commands.iter().for_each(|command| {
            let node = scene.create_node_with_transform(command.transform);
            command.parent.into_iter().for_each(|parent| {
                scene
                    .set_parent(node, nodes[parent])
                    .expect("a parent command is recorded before its child")
            });
            command.components.iter().for_each(|component| {
                match component {
                    NodeComponent::Renderable(r) => {
                        let mesh = scene.mesh_ref(r.mesh.id());
                        let material = scene.material_ref(r.material.id());
                        scene
                            .add_renderable(node, mesh, material)
                            .expect("renderable handle ids are valid refs");
                    }
                    NodeComponent::Camera(c) => {
                        let p = c.projection();
                        scene
                            .add_perspective_camera(
                                math,
                                node,
                                Radians::new(p.fov_y.as_radians()).expect("authored fov is finite"),
                                Ratio::new(self.aspect).expect("authored aspect is finite"),
                                p.near,
                                p.far,
                            )
                            .expect("authored camera intrinsics are valid");
                    }
                    NodeComponent::Light(l) => {
                        scene
                            .add_directional_light(
                                math,
                                node,
                                Vec3::new(l.color.r.get(), l.color.g.get(), l.color.b.get()),
                                l.intensity,
                            )
                            .expect("authored light parameters are valid");
                        light_direction = Some(l.direction);
                    }
                    NodeComponent::Spin(s) => {
                        scene
                            .add_spin(node, s.axis, s.period_ticks)
                            .expect("spin attaches to a just-created node");
                    }
                    NodeComponent::Player(p) => {
                        scene
                            .add_player(node, p.index)
                            .expect("player attaches to a just-created node");
                    }
                    NodeComponent::Controller(c) => {
                        scene
                            .add_controller(node, c.index)
                            .expect("controller attaches to a just-created node");
                    }
                }
            });
            nodes.push(node);
        });
        light_direction
    }
}

/// A handle to a just-spawned node, used to attach children. Borrows the command
/// buffer for the duration of one `spawn(..).with_child(..)` statement.
#[derive(Debug)]
pub struct SpawnedNode<'a> {
    commands: &'a mut SceneCommands,
    index: usize,
}

impl SpawnedNode<'_> {
    /// Spawn `bundle` as a child of this node. Returns this node again so several
    /// children can be chained.
    pub fn with_child<B: Bundle>(self, bundle: B) -> Self {
        self.commands.record(Some(self.index), bundle);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::angle::Angle;
    use crate::camera::{Camera, PerspectiveProjection};
    use crate::color::Color;
    use crate::directional_light::DirectionalLight;
    use crate::handle::Handle;
    use crate::material::Material;
    use crate::mesh::Mesh;
    use crate::renderable::Renderable;
    use crate::spin::Spin;
    use axiom_kernel::Meters;
    use axiom_math::Transform;

    fn math() -> MathApi {
        MathApi::new()
    }

    #[test]
    fn realizes_a_parent_child_camera_and_light_scene() {
        let mut cmds = SceneCommands::new(4.0 / 3.0);

        // A translation parent with a spinning, renderable child.
        let mesh: Handle<Mesh> = {
            let mut a = crate::assets::Assets::new();
            a.add(Mesh::cube())
        };
        let material: Handle<Material> = {
            let mut a = crate::assets::Assets::new();
            a.add(Material::lit(Color::WHITE))
        };
        cmds.spawn(Transform::from_translation(Vec3::new(-2.6, 0.0, 0.0)))
            .with_child((
                Renderable { mesh, material },
                Spin::around(Vec3::UNIT_Y).period(360),
            ));

        // A camera and a directional light, each a 2-tuple bundle.
        cmds.spawn((
            Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(60.0),
                near: Meters::new(0.1).unwrap(),
                far: Meters::new(100.0).unwrap(),
            }),
        ));
        cmds.spawn((
            Transform::IDENTITY,
            DirectionalLight {
                direction: Vec3::new(0.3, -1.0, 0.4),
                color: Color::WHITE,
                intensity: Ratio::new(1.0).unwrap(),
            },
        ));

        let mut scene = SceneApi::new();
        let light_dir = cmds.realize_into(&mut scene, &math());

        let snap = scene.snapshot();
        // parent + child + camera node + light node = 4 nodes.
        assert_eq!(snap.nodes().len(), 4);
        assert_eq!(snap.renderables().len(), 1);
        assert_eq!(snap.cameras().len(), 1);
        assert_eq!(snap.lights().len(), 1);
        // The child is parented (carries a parent id).
        assert!(snap.nodes().iter().any(|n| n.parent().is_some()));
        assert_eq!(light_dir, Some(Vec3::new(0.3, -1.0, 0.4)));
    }

    #[test]
    fn realizes_a_controller_marked_camera_node() {
        use crate::controller::Controller;
        let mut cmds = SceneCommands::new(4.0 / 3.0);
        cmds.spawn((
            Transform::IDENTITY,
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(60.0),
                near: Meters::new(0.1).unwrap(),
                far: Meters::new(100.0).unwrap(),
            }),
            Controller::new(0),
        ));
        let mut scene = SceneApi::new();
        // The Controller arm of realize_into runs add_controller without error.
        assert_eq!(cmds.realize_into(&mut scene, &math()), None);
        assert_eq!(scene.snapshot().nodes().len(), 1);
        assert_eq!(scene.snapshot().cameras().len(), 1);
    }

    #[test]
    fn a_scene_with_no_light_returns_no_direction() {
        let mut cmds = SceneCommands::new(1.0);
        cmds.spawn(Transform::IDENTITY);
        let mut scene = SceneApi::new();
        assert_eq!(cmds.realize_into(&mut scene, &math()), None);
        assert_eq!(scene.snapshot().nodes().len(), 1);
    }
}
