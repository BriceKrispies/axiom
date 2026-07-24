//! Tests for `scene_api`, split into a sibling file so `scene_api.rs` stays
//! under the engine 1000-line file budget. Included via `#[path]` as a child
//! module, so `super` still refers to `scene_api`.

use super::*;
use crate::scene_error_code::SceneErrorCode;
use axiom_kernel::{Meters, Radians, Ratio};

fn math() -> MathApi {
    MathApi::new()
}

fn api() -> SceneApi {
    SceneApi::new()
}

fn rad(x: f32) -> Radians {
    Radians::new(x).unwrap()
}
fn rat(x: f32) -> Ratio {
    Ratio::new(x).unwrap()
}
fn m(x: f32) -> Meters {
    Meters::new(x).unwrap()
}

#[test]
fn new_and_default_facades_are_equivalent() {
    assert_eq!(SceneApi::new().snapshot(), SceneApi::default().snapshot());
}

#[test]
fn add_player_marks_a_node_and_rejects_a_missing_one() {
    let mut s = api();
    let node = s.create_node();
    assert!(s.add_player(node, 0).is_ok());
    assert!(s.add_player(SceneNodeId::from_raw(404), 1).is_err());
}

#[test]
fn despawn_player_removes_a_marked_node_and_is_idempotent_through_the_facade() {
    let mut s = api();
    let node = s.create_node();
    s.add_player(node, 0).unwrap();
    assert_eq!(s.snapshot().nodes().len(), 1);
    assert!(s.despawn_player(0));
    assert_eq!(s.snapshot().nodes().len(), 0);
    assert!(!s.despawn_player(0));
}

#[test]
fn player_entity_recovers_the_handle_and_despawn_node_removes_it() {
    let mut s = api();
    let node = s.create_node();
    s.add_player(node, 2).unwrap();
    assert_eq!(s.player_entity(2), Some(node));
    assert_eq!(s.player_entity(9), None);
    assert!(s.despawn_node(node));
    assert_eq!(s.snapshot().nodes().len(), 0);
    assert!(!s.despawn_node(node));
}

#[test]
fn player_translation_reads_the_marked_node_and_is_none_for_unknown() {
    let mut s = api();
    let node = s.create_node_with_transform(Transform::from_translation(Vec3::new(-1.5, 0.0, 0.0)));
    s.add_player(node, 0).expect("node exists");
    assert_eq!(s.player_translation(0), Some(Vec3::new(-1.5, 0.0, 0.0)));
    assert_eq!(s.player_translation(7), None);
}

#[test]
fn move_command_encodes_a_decodable_move() {
    let s = api();
    let cmd = s.move_command(0, 3, Vec3::new(0.25, -0.75, 0.0));
    assert_eq!(
        crate::player_command::decode_move(&cmd),
        Some((3, Vec3::new(0.25, -0.75, 0.0)))
    );
}

#[test]
fn add_controller_marks_a_node_and_rejects_a_missing_one() {
    let mut s = api();
    let node = s.create_node();
    assert!(s.add_controller(node, 0).is_ok());
    assert!(s.add_controller(SceneNodeId::from_raw(404), 1).is_err());
}

#[test]
fn controller_command_encodes_a_decodable_input() {
    let s = api();
    let cmd = s.controller_command(0, 1, Vec3::new(0.5, 0.0, -0.25), rad(0.1), rad(-0.2), None);
    assert_eq!(
        crate::controller_command::decode_controller(&cmd),
        Some((1, Vec3::new(0.5, 0.0, -0.25), 0.1, -0.2, None))
    );
}

#[test]
fn controller_command_carries_an_absolute_vertical_seat() {
    let s = api();
    let seat = m(4.25);
    let cmd = s.controller_command(0, 1, Vec3::ZERO, rad(0.0), rad(0.0), Some(seat));
    assert_eq!(
        crate::controller_command::decode_controller(&cmd),
        Some((1, Vec3::ZERO, 0.0, 0.0, Some(seat)))
    );
}

#[test]
fn nodes_transforms_and_hierarchy_round_trip() {
    let mut a = api();
    let p = a.create_node();
    let c = a.create_node_with_transform(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
    a.set_local_transform(p, Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)))
        .unwrap();
    assert_eq!(a.local_transform(p).unwrap().translation.x, 1.0);
    a.set_parent(c, p).unwrap();
    assert_eq!(a.parent_of(c), Some(p));
    a.update_world_transforms();
    assert_eq!(a.world_transform(c).unwrap().translation.x, 1.0);
    a.clear_parent(c).unwrap();
    assert_eq!(a.parent_of(c), None);
}

#[test]
fn is_alive_tracks_spawn_and_despawn_and_is_false_for_absent() {
    let mut a = api();
    assert!(!a.is_alive(SceneNodeId::from_raw(404)));
    let n = a.create_node();
    assert!(a.is_alive(n));
    assert!(a.despawn_node(n));
    assert!(!a.is_alive(n));
}

#[test]
fn add_perspective_camera_valid_and_invalid() {
    let mut a = api();
    let n = a.create_node();
    a.add_perspective_camera(
        &math(),
        n,
        rad(std::f32::consts::FRAC_PI_3),
        rat(1.0),
        m(0.1),
        m(100.0),
    )
    .unwrap();
    let err = a
        .add_perspective_camera(&math(), n, rad(0.0), rat(1.0), m(0.1), m(100.0))
        .unwrap_err();
    assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    let m = a.camera_projection_matrix(&math(), n).unwrap();
    assert_eq!(m.as_cols_array(), m.as_cols_array());
    let n2 = a.create_node();
    assert_eq!(
        a.camera_projection_matrix(&math(), n2).unwrap_err().code(),
        SceneErrorCode::MissingCamera
    );
    a.remove_camera(n).unwrap();
}

#[test]
fn lights_valid_and_invalid() {
    let mut a = api();
    let n = a.create_node();
    a.add_directional_light(&math(), n, Vec3::ONE, rat(1.0))
        .unwrap();
    a.add_point_light(&math(), n, Vec3::new(0.5, 0.5, 0.5), rat(2.0))
        .unwrap();
    assert_eq!(
        a.add_directional_light(&math(), n, Vec3::ONE, rat(-1.0))
            .unwrap_err()
            .code(),
        SceneErrorCode::InvalidLightParameters
    );
    assert_eq!(
        a.add_point_light(&math(), n, Vec3::new(f32::NAN, 0.0, 0.0), rat(1.0))
            .unwrap_err()
            .code(),
        SceneErrorCode::InvalidLightParameters
    );
    a.remove_light(n).unwrap();
}

#[test]
fn renderables_valid_and_invalid() {
    let mut a = api();
    let n = a.create_node();
    let mesh = a.mesh_ref(1);
    let material = a.material_ref(2);
    a.add_renderable(n, mesh, material).unwrap();
    assert_eq!(
        a.add_renderable(n, MeshRef::INVALID, material)
            .unwrap_err()
            .code(),
        SceneErrorCode::InvalidRenderableReference
    );
    a.set_renderable_visibility(n, false).unwrap();
    a.set_renderable_casts_contact_shadow(n, true).unwrap();
    // The object-binding slots: bind a texture and an animation, then read
    // them back off the snapshot's renderable entry.
    a.set_renderable_texture(n, a.texture_ref(7)).unwrap();
    a.set_renderable_animation(n, a.animation_ref(13)).unwrap();
    let snap = a.snapshot();
    assert_eq!(snap.renderables()[0].texture(), a.texture_ref(7));
    assert_eq!(snap.renderables()[0].animation(), a.animation_ref(13));
    assert_eq!(
        a.set_renderable_texture(SceneNodeId::from_raw(99), a.texture_ref(1))
            .unwrap_err()
            .code(),
        SceneErrorCode::MissingRenderable
    );
    assert_eq!(
        a.set_renderable_animation(SceneNodeId::from_raw(99), a.animation_ref(1))
            .unwrap_err()
            .code(),
        SceneErrorCode::MissingRenderable
    );
    a.remove_renderable(n).unwrap();
    assert_eq!(
        a.set_renderable_casts_contact_shadow(n, true)
            .unwrap_err()
            .code(),
        SceneErrorCode::MissingRenderable
    );
}

#[test]
fn add_spin_valid_and_missing_node() {
    let mut a = api();
    let n = a.create_node();
    a.add_spin(n, Vec3::UNIT_Y, 360).unwrap();
    assert_eq!(
        a.add_spin(SceneNodeId::from_raw(99), Vec3::UNIT_Y, 360)
            .unwrap_err()
            .code(),
        SceneErrorCode::MissingNode
    );
}

#[test]
fn add_procanim_valid_and_missing_node() {
    let mut a = api();
    let n = a.create_node();
    a.add_procanim(n, Transform::IDENTITY, (m(0.5), 60), (Vec3::UNIT_Y, 120), 0)
        .unwrap();
    assert_eq!(
        a.add_procanim(
            SceneNodeId::from_raw(99),
            Transform::IDENTITY,
            (m(0.5), 60),
            (Vec3::UNIT_Y, 120),
            0
        )
        .unwrap_err()
        .code(),
        SceneErrorCode::MissingNode
    );
}

#[test]
fn bounds_and_spatial_queries_through_the_facade() {
    let mut a = api();
    let n = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
    a.add_bounds(n, Vec3::new(0.5, 0.5, 0.5)).unwrap();
    a.update_world_transforms();
    // raycast finds it; overlap_box finds it.
    assert_eq!(
        a.raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), m(100.0)),
        Some(n)
    );
    assert_eq!(
        a.overlap_box(Vec3::new(3.0, 0.0, 0.0), Vec3::new(0.1, 0.1, 0.1)),
        vec![n]
    );
    assert_eq!(a.player_index(n), None);
    a.add_player(n, 5).unwrap();
    assert_eq!(a.player_index(n), Some(5));
    assert_eq!(a.raycast(Vec3::ZERO, Vec3::ZERO, m(100.0)), None);
    a.remove_bounds(n).unwrap();
    assert_eq!(
        a.remove_bounds(n).unwrap_err().code(),
        SceneErrorCode::MissingBounds
    );
    assert_eq!(
        a.add_bounds(SceneNodeId::from_raw(404), Vec3::ONE)
            .unwrap_err()
            .code(),
        SceneErrorCode::MissingNode
    );
}

#[test]
fn radial_overlap_hierarchy_and_subtree_despawn_through_the_facade() {
    let mut a = api();
    // A bounded node 3m out: a circle that reaches it finds it; one that
    // stops short does not.
    let n = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
    a.add_bounds(n, Vec3::new(0.5, 0.5, 0.5)).unwrap();
    a.update_world_transforms();
    assert_eq!(a.overlap_circle(Vec3::ZERO, m(3.0)), vec![n]);
    assert!(a.overlap_circle(Vec3::ZERO, m(1.0)).is_empty());

    let parent = a.create_node();
    let first = a.create_node();
    let second = a.create_node();
    a.set_parent(first, parent).unwrap();
    a.set_parent(second, parent).unwrap();
    assert_eq!(a.children_of(parent), vec![first, second]);
    assert!(a.children_of(first).is_empty());

    assert!(a.despawn_subtree(parent));
    assert!(a.children_of(parent).is_empty());
    assert!(!a.despawn_subtree(parent));
}

#[test]
fn raycast_hit_and_tags_classify_through_the_facade() {
    let mut a = api();
    let wall = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
    a.add_bounds(wall, Vec3::new(0.5, 0.5, 0.5)).unwrap();
    a.add_tag(wall, 1).unwrap(); // 1 = "wall" in this game's vocabulary
    a.update_world_transforms();

    // The hit carries the node and the exact entry point (near face x=2.5),
    // and the agent classifies it by reading its tag.
    let (node, point) = a
        .raycast_hit(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), m(100.0))
        .expect("ray hits the wall");
    assert_eq!(node, wall);
    assert!((point.x - 2.5).abs() < 1.0e-5);
    assert_eq!(a.tag_of(node), Some(1));

    let untagged = a.create_node();
    assert_eq!(a.tag_of(untagged), None);
    assert_eq!(a.tagged_nodes(1), vec![wall]);
    assert!(a.tagged_nodes(2).is_empty());
    assert_eq!(
        a.add_tag(SceneNodeId::from_raw(404), 9).unwrap_err().code(),
        SceneErrorCode::MissingNode
    );
    assert!(a
        .raycast_hit(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), m(100.0))
        .is_none());
}

#[test]
fn typed_component_enumeration_lists_nodes_and_bounds() {
    let mut a = api();
    let n0 = a.create_node_with_transform(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
    let n1 = a.create_node_with_transform(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
    a.add_bounds(n1, Vec3::new(0.5, 0.5, 0.5)).unwrap();

    let transforms = a.node_transforms();
    assert_eq!(transforms.len(), 2);
    assert_eq!(transforms[0].0, n0);
    assert_eq!(transforms[0].1.translation, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(transforms[1].0, n1);

    assert_eq!(a.bounds_half_extents(n1), Some(Vec3::new(0.5, 0.5, 0.5)));
    assert_eq!(a.bounds_half_extents(n0), None);

    assert_eq!(a.bounded_nodes(), vec![(n1, Vec3::new(0.5, 0.5, 0.5))]);
}

#[test]
fn component_schemas_describe_the_standard_components() {
    let schemas = api().component_schemas();
    assert_eq!(schemas.len(), 9);
    assert_eq!(schemas[0].name(), "Transform");
    assert_eq!(schemas[1].name(), "Camera");
    assert_eq!(schemas[2].name(), "Light");
    assert_eq!(schemas[3].name(), "Renderable");
    assert_eq!(schemas[4].name(), "SdfShape");
    assert_eq!(schemas[5].name(), "Spin");
    assert_eq!(schemas[6].name(), "ProcAnim");
    assert_eq!(schemas[7].name(), "Bounds");
    assert_eq!(schemas[8].name(), "Tag");
}

#[test]
fn snapshot_reads_current_scene_state() {
    let mut a = api();
    let n = a.create_node();
    a.add_directional_light(&math(), n, Vec3::ONE, rat(1.0))
        .unwrap();
    let snap = a.snapshot();
    assert_eq!(snap.nodes().len(), 1);
    assert_eq!(snap.lights().len(), 1);
}

#[test]
fn snapshot_state_round_trips_through_bytes_and_rejects_truncation() {
    let mut a = api();
    let n = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
    a.add_directional_light(&math(), n, Vec3::ONE, rat(2.0))
        .unwrap();
    let bytes = a.snapshot_state();

    let mut restored = api();
    restored.restore_state(&bytes).unwrap();
    let original = a.snapshot();
    let after = restored.snapshot();
    assert_eq!(after.nodes().len(), original.nodes().len());
    assert_eq!(after.lights().len(), original.lights().len());
    assert_eq!(
        after.nodes()[0].world().translation.x,
        original.nodes()[0].world().translation.x
    );
    // A truncated buffer is a deterministic error, not a panic.
    assert!(restored.restore_state(&[9, 9]).is_err());
}

#[test]
fn advance_animates_and_propagates_across_ticks_on_a_held_scene() {
    use axiom_frame::FrameApi;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
        HostLifecycleState, HostStepPlan, HostViewport,
    };
    // Build the scene ONCE, then advance it at two different ticks: the
    // spun child's world transform must differ — proving a held, durable
    // world that evolves, not a rebuilt one.
    let mut a = api();
    let parent =
        a.create_node_with_transform(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
    let child = a.create_node();
    a.set_parent(child, parent).unwrap();
    a.add_spin(child, Vec3::UNIT_Y, 8).unwrap();

    let frame = |elapsed: u64| {
        let vp = HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        let visible = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        let input = HostFrameInput::new(1, elapsed, vp);
        let plan = HostStepPlan::build(&input, &cfg, &visible, 0);
        let report = HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp,
            visible,
        );
        FrameApi::new()
            .engine_frame_from_host_report(&report, elapsed, Vec::new())
            .unwrap()
    };

    let f0 = frame(1_000);
    let snap0 = a.advance(0, &FrameContext::new(&f0));
    let child0 = snap0
        .nodes()
        .iter()
        .find(|n| n.parent().is_some())
        .unwrap()
        .world();

    let f2 = frame(1_000);
    let snap2 = a.advance(2, &FrameContext::new(&f2));
    let child2 = snap2
        .nodes()
        .iter()
        .find(|n| n.parent().is_some())
        .unwrap()
        .world();

    // Same handle, different ticks -> different world rotation, same parent
    // translation carried through.
    assert_ne!(child0.rotation, child2.rotation);
    assert_eq!(child2.translation.x, 2.0);
}

#[test]
fn advance_systems_then_refresh_reuses_one_snapshot_buffer() {
    use axiom_frame::FrameApi;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
        HostLifecycleState, HostStepPlan, HostViewport,
    };
    let mut a = api();
    let parent =
        a.create_node_with_transform(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
    let child = a.create_node();
    a.set_parent(child, parent).unwrap();
    a.add_spin(child, Vec3::UNIT_Y, 8).unwrap();
    let frame = |elapsed: u64| {
        let vp = HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        let visible = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        let input = HostFrameInput::new(1, elapsed, vp);
        let plan = HostStepPlan::build(&input, &cfg, &visible, 0);
        let report = HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp,
            visible,
        );
        FrameApi::new()
            .engine_frame_from_host_report(&report, elapsed, Vec::new())
            .unwrap()
    };
    let world_rot = |a: &SceneApi| {
        a.snapshot_ref()
            .nodes()
            .iter()
            .find(|n| n.parent().is_some())
            .unwrap()
            .world()
            .rotation
    };
    // Step the systems, refresh the RETAINED snapshot: it matches a freshly
    // built one from the same state (reuse is behaviour-identical).
    a.advance_systems(3, &FrameContext::new(&frame(1_000)));
    a.refresh_snapshot();
    assert_eq!(a.snapshot_ref(), &a.snapshot());
    let rot_a = world_rot(&a);
    // Step later and refresh the SAME buffer — it updates in place to the new
    // state (the spun child rotated further).
    a.advance_systems(9, &FrameContext::new(&frame(1_000)));
    a.refresh_snapshot();
    assert_eq!(a.snapshot_ref(), &a.snapshot());
    assert_ne!(rot_a, world_rot(&a));
}
