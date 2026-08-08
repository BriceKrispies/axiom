//! [`FigureApi`]: the figure module's single behavioral facade.

use axiom_kernel::{BinaryReader, BinaryWriter};
use axiom_math::{Quat, Transform, Vec3};

use crate::bound_figure::BoundFigure;
use crate::definition::FigureDefinition;
use crate::figure_error::{FigureError, FigureResult};
use crate::posed_part::PosedPart;

/// The stateless facade over the figure mechanism: validate and round-trip a
/// [`FigureDefinition`], and pose a figure by pairing its per-part render boxes
/// with world transforms an app has already resolved (from an
/// `axiom-animation` model pose). It never touches the animation module — an app
/// drives the skeleton and hands the resulting world transforms here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FigureApi;

impl FigureApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        Self
    }

    /// Validate a figure's parent-before-child hierarchy.
    pub fn validate(&self, figure: &FigureDefinition) -> FigureResult<()> {
        figure.validate()
    }

    /// Encode a figure to a portable byte buffer.
    pub fn serialize(&self, figure: &FigureDefinition) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        figure.write_to(&mut writer);
        writer.into_bytes()
    }

    /// Decode a figure produced by [`FigureApi::serialize`], then validate it.
    /// Fails with `MalformedData` if the bytes cannot be decoded, or `BadParent`
    /// if the decoded hierarchy is illegal.
    pub fn deserialize(&self, bytes: &[u8]) -> FigureResult<FigureDefinition> {
        FigureDefinition::read_from(&mut BinaryReader::new(bytes))
            .map_err(|_| FigureError::MalformedData)
            .and_then(|figure| figure.validate().map(|()| figure))
    }

    /// Pose a figure: pair each part's render box/tag with the matching
    /// world-space transform. `world_transforms` must have exactly one transform
    /// per part (the app resolves these from an animation model pose, in part
    /// order), else `TransformCountMismatch`.
    pub fn posed_parts(
        &self,
        figure: &FigureDefinition,
        world_transforms: &[Transform],
    ) -> FigureResult<Vec<PosedPart>> {
        (figure.part_count() == world_transforms.len())
            .then_some(())
            .ok_or(FigureError::TransformCountMismatch)
            .map(|()| {
                figure
                    .parts()
                    .iter()
                    .zip(world_transforms.iter())
                    .map(|(part, &world)| {
                        // The part pivots at its joint (`world`); the box is
                        // centered along the segment by baking its local offset
                        // into the world transform it is drawn at.
                        let box_world =
                            Transform::combine(world, Transform::from_translation(part.box_offset));
                        PosedPart::new(box_world, part.box_size, part.tag)
                    })
                    .collect()
            })
    }

    /// Pose a figure from **per-part joint rotations** under one body transform:
    /// resolve the parent chain, place it under `body`, and bake the box offsets
    /// (as [`Self::posed_parts`]) — the whole hop from "this tick's joint angles"
    /// to "boxes a renderer draws".
    ///
    /// This is the other half of figure posing. [`Self::posed_parts`] takes world
    /// transforms an app has *already* resolved, which leaves every consumer of a
    /// jointed box figure hand-rolling the same parent-chain accumulation — and a
    /// chain walk is figure mechanism, not game meaning. Each part's local frame
    /// is its rest transform composed with that part's joint rotation about the
    /// rest pivot, so an identity rotation reproduces the figure's rest pose
    /// exactly.
    ///
    /// `joint_rotations` must have one entry per part, in part order, else
    /// `TransformCountMismatch`. A part whose parent index is not an earlier part
    /// (a figure that would fail [`Self::validate`]) resolves as a root rather
    /// than reading an unresolved frame, so an invalid hierarchy degrades to a
    /// defined pose instead of an arbitrary one.
    pub fn posed_parts_from_joints(
        &self,
        figure: &FigureDefinition,
        body: Transform,
        joint_rotations: &[Quat],
    ) -> FigureResult<Vec<PosedPart>> {
        (figure.part_count() == joint_rotations.len())
            .then_some(())
            .ok_or(FigureError::TransformCountMismatch)
            .and_then(|()| {
                let world = figure
                    .parts()
                    .iter()
                    .zip(joint_rotations.iter())
                    .fold(
                        Vec::with_capacity(figure.part_count()),
                        |mut chain, (part, &joint)| {
                            let local = Transform::combine(
                                part.rest,
                                Transform::new(Vec3::ZERO, joint, Vec3::ONE),
                            );
                            let resolved = part
                                .parent
                                .and_then(|parent| chain.get(parent as usize).copied())
                                .map_or(local, |parent| Transform::combine(parent, local));
                            chain.push(resolved);
                            chain
                        },
                    )
                    .into_iter()
                    .map(|local| Transform::combine(body, local))
                    .collect::<Vec<Transform>>();
                self.posed_parts(figure, &world)
            })
    }

    /// Pose a figure **and bind it to a scene node** in one step: resolve the
    /// figure's parts against `world_transforms` (as [`Self::posed_parts`]) and
    /// wrap the result with the scene `node_id` it animates, yielding a
    /// [`BoundFigure`] — the value that makes a posed character one engine object
    /// rather than a loose figure blob keyed by nothing. `node_id` is the same
    /// `u64` an app stamps onto that node's renderable `AnimationRef`, so the
    /// scene and the figure name the same object. Fails with
    /// `TransformCountMismatch` when the transform count differs from the part
    /// count (delegated to [`Self::posed_parts`]).
    pub fn bind(
        &self,
        node_id: u64,
        figure: &FigureDefinition,
        world_transforms: &[Transform],
    ) -> FigureResult<BoundFigure> {
        self.posed_parts(figure, world_transforms)
            .map(|parts| BoundFigure::new(node_id, parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::FigurePart;
    use axiom_math::{ApproxEq, Epsilon, Vec3};

    fn defaulted<T: Default>() -> T {
        T::default()
    }

    fn two_part_figure() -> FigureDefinition {
        FigureDefinition::new(vec![
            FigurePart::root(
                Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
                Vec3::new(0.4, 0.6, 0.4),
                Vec3::ZERO,
                1,
            ),
            FigurePart::child(
                0,
                Transform::from_translation(Vec3::new(0.0, -0.5, 0.0)),
                Vec3::new(0.2, 0.5, 0.2),
                Vec3::new(0.0, -0.25, 0.0),
                2,
            ),
        ])
    }

    #[test]
    fn new_and_default_agree() {
        assert_eq!(FigureApi::new(), FigureApi);
        assert_eq!(defaulted::<FigureApi>(), FigureApi::new());
    }

    #[test]
    fn validate_serialize_deserialize_round_trip() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        assert_eq!(api.validate(&figure), Ok(()));
        let bytes = api.serialize(&figure);
        assert_eq!(api.deserialize(&bytes).unwrap(), figure);
    }

    #[test]
    fn deserialize_rejects_garbage_and_illegal_hierarchy() {
        let api = FigureApi::new();
        assert_eq!(api.deserialize(&[0xFF]), Err(FigureError::MalformedData));
        // A structurally-decodable but illegal figure (child before parent).
        let bad = FigureDefinition::new(vec![FigurePart::child(
            5,
            Transform::IDENTITY,
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
            0,
        )]);
        let bytes = api.serialize(&bad);
        assert_eq!(api.deserialize(&bytes), Err(FigureError::BadParent));
    }

    #[test]
    fn posed_parts_pairs_boxes_with_world_transforms() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        let world = [
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
            Transform::from_translation(Vec3::new(0.0, 0.5, 0.0)),
        ];
        let posed = api.posed_parts(&figure, &world).unwrap();
        assert_eq!(posed.len(), 2);
        assert_eq!(posed[0].transform.translation, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(posed[0].box_size, Vec3::new(0.4, 0.6, 0.4));
        assert_eq!(posed[1].tag, 2);
    }

    #[test]
    fn posed_parts_rejects_length_mismatch() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        assert_eq!(
            api.posed_parts(&figure, &[Transform::IDENTITY]),
            Err(FigureError::TransformCountMismatch)
        );
    }

    #[test]
    fn posed_parts_from_joints_resolves_the_parent_chain_under_the_body() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        // Identity joints: the chain is just rest-under-body.
        let body = Transform::from_translation(Vec3::new(10.0, 0.0, 0.0));
        let rest = api
            .posed_parts_from_joints(&figure, body, &[Quat::IDENTITY; 2])
            .unwrap();
        assert_eq!(rest.len(), 2);
        // Root: body * rest(0, 1, 0), box centred on the pivot.
        assert_eq!(rest[0].transform.translation, Vec3::new(10.0, 1.0, 0.0));
        // Child: body * rest(0,1,0) * rest(0,-0.5,0), then the box offset baked.
        assert_eq!(rest[1].transform.translation, Vec3::new(10.0, 0.25, 0.0));
        assert_eq!(rest[1].box_size, Vec3::new(0.2, 0.5, 0.2));
        assert_eq!(rest[1].tag, 2);

        // A half-turn at the root swings the child's offset to the far side,
        // proving the child inherits the parent's rotation rather than only its
        // translation.
        let spun = api
            .posed_parts_from_joints(
                &figure,
                Transform::IDENTITY,
                &[
                    Quat::from_euler_xyz(core::f32::consts::PI, 0.0, 0.0),
                    Quat::IDENTITY,
                ],
            )
            .unwrap();
        assert!(spun[1]
            .transform
            .translation
            .approx_eq(&Vec3::new(0.0, 1.75, 0.0), Epsilon::DEFAULT));
    }

    #[test]
    fn posed_parts_from_joints_treats_an_unresolvable_parent_as_a_root() {
        let api = FigureApi::new();
        // A forward reference (illegal per `validate`): the part cannot read its
        // parent's frame, so it resolves under the body directly.
        let figure = FigureDefinition::new(vec![FigurePart::child(
            7,
            Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
            0,
        )]);
        assert_eq!(figure.validate(), Err(FigureError::BadParent));
        let posed = api
            .posed_parts_from_joints(
                &figure,
                Transform::from_translation(Vec3::new(0.0, 0.0, 3.0)),
                &[Quat::IDENTITY],
            )
            .unwrap();
        assert_eq!(posed[0].transform.translation, Vec3::new(0.0, 2.0, 3.0));
    }

    #[test]
    fn posed_parts_from_joints_rejects_a_joint_count_mismatch() {
        let api = FigureApi::new();
        assert_eq!(
            api.posed_parts_from_joints(&two_part_figure(), Transform::IDENTITY, &[Quat::IDENTITY]),
            Err(FigureError::TransformCountMismatch)
        );
    }

    #[test]
    fn bind_binds_the_posed_figure_to_a_scene_node() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        let world = [
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
            Transform::from_translation(Vec3::new(0.0, 0.5, 0.0)),
        ];
        let bound = api.bind(77, &figure, &world).unwrap();
        assert_eq!(bound.node(), 77);
        assert_eq!(bound.part_count(), 2);
        // The bound parts match a bare pose of the same figure.
        assert_eq!(bound.parts(), api.posed_parts(&figure, &world).unwrap());
    }

    #[test]
    fn bind_rejects_length_mismatch() {
        let api = FigureApi::new();
        let figure = two_part_figure();
        assert_eq!(
            api.bind(1, &figure, &[Transform::IDENTITY]),
            Err(FigureError::TransformCountMismatch)
        );
    }
}
