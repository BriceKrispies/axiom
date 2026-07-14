//! Frame-packet derivation coverage tests for `RenderApi`, split out of
//! `render_api.rs` to keep that file within the engine per-file size budget.

use super::*;

fn api() -> RenderApi {
    RenderApi::new()
}

/// A single triangle object with a known mesh/material/colour, plus a camera
/// and one directional light — exercises every populated packet field.
fn one_object_input() -> RenderInput {
    let mut input = api().new_input(800, 600);
    api().set_input_clear_color(&mut input, [0.1, 0.2, 0.3, 1.0]);
    api().set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
    api().add_input_directional_light(
        &mut input,
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::ONE,
        Ratio::new(1.0).unwrap(),
    );
    let mesh = api().add_input_mesh(&mut input, 42, vec![], vec![], vec![], vec![0, 1, 2]);
    let mat = api().add_input_basic_lit_material(&mut input, 99, Vec4::new(0.5, 0.5, 0.5, 1.0));
    api().add_input_object(&mut input, 7, Mat4::IDENTITY, mesh, mat, true);
    input
}

#[test]
fn packet_is_derived_from_the_command_list() {
    let input = one_object_input();
    let packet = api().build_frame_packet(&input, 4, 240, [9.0; 16]);

    assert_eq!(packet.draws().len(), 1);
    let draw = packet.draws()[0];
    assert_eq!(draw.object_id(), 7);
    assert_eq!(draw.mesh_id(), 42);
    assert_eq!(draw.material_id(), 99);
    // Identity camera => mvp == world == identity; colour is the material base.
    assert_eq!(draw.world(), Mat4::IDENTITY.as_cols_array());
    assert_eq!(draw.mvp(), Mat4::IDENTITY.as_cols_array());
    assert_eq!(draw.color(), [0.5, 0.5, 0.5, 1.0]);

    assert_eq!(packet.frame_index(), 4);
    assert_eq!(packet.tick(), 240);
    assert_eq!(packet.viewport(), FrameViewport::new(800, 600));
    assert_eq!(packet.clear_color(), [0.1, 0.2, 0.3, 1.0]);
    assert_eq!(packet.light_view_proj(), [9.0; 16]);

    // Camera present with view_proj = projection * view (identity here).
    let cam = packet.camera().expect("camera present");
    assert_eq!(cam.view(), Mat4::IDENTITY.as_cols_array());
    assert_eq!(cam.projection(), Mat4::IDENTITY.as_cols_array());
    assert_eq!(cam.view_proj(), Mat4::IDENTITY.as_cols_array());

    // One directional light → kind 0; features: no textures, shadows on.
    assert_eq!(packet.lights().len(), 1);
    assert_eq!(packet.lights()[0].kind(), 0);
    let f = packet.features();
    assert!(!f.uses_textures());
    assert!(f.uses_shadows());
    assert_eq!(f.directional_lights(), 1);
    assert_eq!(f.point_lights(), 0);
    // No SDF shapes were added, so the packet carries no SDF scene (the
    // camera-present-but-no-shapes arm of `build_sdf_scene`).
    assert!(packet.sdf().is_none());
}

#[test]
fn packet_draw_count_equals_draw_indexed_command_count() {
    let input = one_object_input();
    let list = api().build_command_list(&input);
    let draw_cmds = (0..list.len())
        .filter(|i| api().command_kind_at(&list, *i) == Some(RenderApi::KIND_DRAW_INDEXED))
        .count();
    let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
    assert_eq!(packet.draws().len(), draw_cmds);
    assert_eq!(packet.draws().len(), 1);
}

#[test]
fn packet_object_ids_and_order_match_the_command_list() {
    let mut input = api().new_input(100, 100);
    let mesh = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
    let mat = api().add_input_basic_lit_material(&mut input, 1, Vec4::ONE);
    api().add_input_object(&mut input, 100, Mat4::IDENTITY, mesh, mat, true);
    api().add_input_object(&mut input, 200, Mat4::IDENTITY, mesh, mat, true);
    api().add_input_object(&mut input, 300, Mat4::IDENTITY, mesh, mat, true);

    let list = api().build_command_list(&input);
    let cmd_ids: Vec<u64> = (0..list.len())
        .filter_map(|i| api().command_draw_object_id_at(&list, i))
        .collect();
    assert_eq!(cmd_ids, vec![100, 200, 300]);

    let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
    let packet_ids: Vec<u64> = packet.draws().iter().map(|d| d.object_id()).collect();
    assert_eq!(packet_ids, cmd_ids);
}

#[test]
fn command_draw_object_id_at_is_some_only_on_draws() {
    let input = one_object_input();
    let list = api().build_command_list(&input);
    // Index 0 is ClearFrame → None; the final command is the draw → Some(7).
    assert_eq!(api().command_draw_object_id_at(&list, 0), None);
    assert_eq!(
        api().command_draw_object_id_at(&list, list.len() - 1),
        Some(7)
    );
}

#[test]
fn empty_input_yields_a_cameraless_drawless_packet() {
    let input = api().new_input(320, 240);
    let packet = api().build_frame_packet(&input, 1, 2, [0.0; 16]);
    assert!(packet.camera().is_none());
    assert!(packet.draws().is_empty());
    assert!(packet.lights().is_empty());
    let f = packet.features();
    assert!(!f.uses_textures());
    assert!(!f.uses_shadows());
    assert_eq!(f.directional_lights(), 0);
    assert_eq!(f.point_lights(), 0);
    assert_eq!(packet.viewport(), FrameViewport::new(320, 240));
    assert!(packet.sdf().is_none());
}

#[test]
fn packet_carries_an_sdf_scene_when_shapes_and_camera_present() {
    let mut input = api().new_input(64, 64);
    api().set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
    api().add_input_sdf(
        &mut input,
        1,
        Mat4::IDENTITY,
        Vec3::new(1.0, 2.0, 3.0),
        Vec4::new(0.2, 0.4, 0.6, 1.0),
    );
    let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
    let scene = packet.sdf().expect("sdf scene present");
    assert_eq!(scene.primitives().len(), 1);
    let p = scene.primitives()[0];
    assert_eq!(p.kind(), 1);
    // dims ride into params[0..3]; an identity world has uniform scale 1.
    assert_eq!(p.params(), [1.0, 2.0, 3.0, 1.0]);
    assert_eq!(p.color(), [0.2, 0.4, 0.6, 1.0]);
    // Identity world → identity world→local matrix.
    assert_eq!(p.inv_transform(), Mat4::IDENTITY.as_cols_array());
    // Identity view → camera at the origin, identity inverse view-projection.
    assert_eq!(scene.camera_world_pos(), [0.0, 0.0, 0.0]);
    assert_eq!(scene.inv_view_proj(), Mat4::IDENTITY.as_cols_array());
    assert_eq!(scene.march(), [100.0, 0.001, 0.0, 0.0]);
}

#[test]
fn sdf_shapes_without_a_camera_produce_no_scene() {
    let mut input = api().new_input(64, 64);
    api().add_input_sdf(&mut input, 0, Mat4::IDENTITY, Vec3::ONE, Vec4::ONE);
    let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
    assert!(packet.sdf().is_none());
}

#[test]
fn build_sdf_scene_is_none_for_no_shapes_and_a_scene_for_some() {
    let r = api();
    // No shapes → no scene (the empty arm of the shared builder).
    assert!(r.build_sdf_scene(Mat4::IDENTITY, Vec3::ZERO, &[]).is_none());
    // Two shapes → a scene carrying both, with dims+scale in params and the
    // supplied camera world position and inverse view-projection.
    let shapes = [
        (
            0u32,
            Mat4::IDENTITY,
            Vec3::new(2.0, 0.0, 0.0),
            Vec4::new(1.0, 0.0, 0.0, 1.0),
        ),
        (1u32, Mat4::IDENTITY, Vec3::new(1.0, 2.0, 3.0), Vec4::ONE),
    ];
    let scene = r
        .build_sdf_scene(Mat4::IDENTITY, Vec3::new(0.0, 0.0, 5.0), &shapes)
        .expect("two shapes yield a scene");
    assert_eq!(scene.primitives().len(), 2);
    assert_eq!(scene.primitives()[0].kind(), 0);
    // Identity world → uniform scale 1, dims carried into params[0..3].
    assert_eq!(scene.primitives()[1].params(), [1.0, 2.0, 3.0, 1.0]);
    assert_eq!(scene.camera_world_pos(), [0.0, 0.0, 5.0]);
    assert_eq!(scene.view_proj(), Mat4::IDENTITY.as_cols_array());
    assert_eq!(scene.inv_view_proj(), Mat4::IDENTITY.as_cols_array());
    assert_eq!(scene.march(), [100.0, 0.001, 0.0, 0.0]);
}

#[test]
fn features_count_both_light_kinds_and_detect_textures() {
    let mut input = api().new_input(100, 100);
    api().add_input_directional_light(
        &mut input,
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::ONE,
        Ratio::new(1.0).unwrap(),
    );
    api().add_input_point_light(&mut input, Vec3::ZERO, Vec3::ONE, Ratio::new(0.5).unwrap());
    let mesh = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
    // A textured material flips uses_textures on.
    let mat = api().add_input_textured_material(&mut input, 5, Vec4::ONE, 77);
    api().add_input_object(&mut input, 1, Mat4::IDENTITY, mesh, mat, true);

    let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
    let f = packet.features();
    assert!(f.uses_textures());
    assert!(f.uses_shadows());
    assert_eq!(f.directional_lights(), 1);
    assert_eq!(f.point_lights(), 1);
    // Light kinds map directional→0, point→1, in input order.
    let kinds: Vec<u32> = packet.lights().iter().map(|l| l.kind()).collect();
    assert_eq!(kinds, vec![0, 1]);
    // The point light's colour+intensity ride through unchanged ([r,g,b,i]).
    assert_eq!(packet.lights()[1].color_intensity(), [1.0, 1.0, 1.0, 0.5]);
}

#[test]
fn material_base_color_resolves_by_id_with_white_fallback() {
    let mut input = api().new_input(10, 10);
    api().add_input_basic_lit_material(&mut input, 9, Vec4::new(0.2, 0.4, 0.6, 1.0));
    assert_eq!(material_base_color(&input, 9), [0.2, 0.4, 0.6, 1.0]);
    assert_eq!(material_base_color(&input, 404), [1.0, 1.0, 1.0, 1.0]);
}
