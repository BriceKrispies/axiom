//! **`RenderPipelineKind::UNLIT` stops being a lie.**
//!
//! This module has emitted two pipeline markers since it was written, and
//! run-length-encoded a `SetPipeline` per switch — but nothing ever *selected*
//! `UNLIT` except an app typing the number itself, and the value died at the
//! `axiom_host::FramePacket` boundary, which carries no pipeline lane. The
//! `RenderInput` seam test in `render_api.rs` says so in its own doc comment.
//!
//! It is now **derived**: a material naming a surface whose
//! `axiom_surface::LightingModel` is `Unlit` puts every draw of that material on
//! the unlit marker, whatever the object asked for. This test drives that through
//! the whole public facade — input, ordering, run-length encoding, command
//! list — because the claim is about the *stream*, not about one accessor.
//!
//! It lives in `tests/` rather than in `render_api.rs`'s own module, which is
//! already at the engine file-size budget.

use axiom_math::{Mat4, Vec4};
use axiom_render::RenderApi;
use axiom_surface::LightingModel;

/// Two objects that both ask for `BASIC_LIT`, drawn with two materials whose
/// only difference is the lighting model of the surface they name — and the
/// command stream carries a `SetPipeline(UNLIT)` between them.
#[test]
fn an_unlit_materials_draw_emits_set_pipeline_unlit_into_the_command_stream() {
    let api = RenderApi::new();
    let mut input = api.new_input(64, 64);
    let mesh = api.add_input_mesh(&mut input, 1, 3);
    let lit =
        input.push_surface_material(10, Vec4::ONE, 0x1111, LightingModel::LambertSpecular);
    let unlit = input.push_surface_material(11, Vec4::ONE, 0x2222, LightingModel::Unlit);
    [lit, unlit].iter().enumerate().for_each(|(index, material)| {
        api.add_input_bound_object(
            &mut input,
            10 + index as u64,
            Mat4::IDENTITY,
            mesh,
            *material,
            0,
            RenderApi::PIPELINE_BASIC_LIT,
            0,
            true,
        );
    });
    let list = api.build_command_list(&input);
    // ClearFrame + 2 x (SetPipeline, SetMesh, SetMaterial, DrawIndexed) = 9. The
    // run-length encoder emits a SECOND `SetPipeline` only when the marker
    // actually changed between draws, so the length is itself the assertion.
    assert_eq!(list.len(), 9);
    assert_eq!(
        api.command_pipeline_at(&list, 1),
        Some(RenderApi::PIPELINE_BASIC_LIT)
    );
    assert_eq!(
        api.command_pipeline_at(&list, 5),
        Some(RenderApi::PIPELINE_UNLIT),
        "an unlit material must select the unlit pipeline marker"
    );
    assert_eq!(api.command_draw_object_id_at(&list, 8), Some(11));
}

/// A frame in which **every** material is unlit emits exactly one
/// `SetPipeline`, and it is `UNLIT` — the run-length property still holds, so
/// selecting the marker costs no extra commands.
#[test]
fn an_all_unlit_frame_emits_one_set_pipeline_and_it_is_the_unlit_marker() {
    let api = RenderApi::new();
    let mut input = api.new_input(64, 64);
    let mesh = api.add_input_mesh(&mut input, 1, 3);
    let unlit = input.push_surface_material(10, Vec4::ONE, 0x3333, LightingModel::Unlit);
    (0..3_u64).for_each(|index| {
        api.add_input_object(&mut input, index, Mat4::IDENTITY, mesh, unlit, true);
    });
    let list = api.build_command_list(&input);
    // ClearFrame + SetPipeline + 3 x (SetMesh, SetMaterial, DrawIndexed) = 11.
    assert_eq!(list.len(), 11);
    assert_eq!(
        api.command_pipeline_at(&list, 1),
        Some(RenderApi::PIPELINE_UNLIT)
    );
    // No further pipeline command anywhere in the stream.
    assert!((2..list.len()).all(|index| api.command_pipeline_at(&list, index).is_none()));
}

/// The compatibility half: a frame whose materials say nothing about lighting
/// emits exactly the stream it always did — one `SetPipeline(BASIC_LIT)`.
#[test]
fn a_frame_of_default_materials_emits_the_stream_it_always_did() {
    let api = RenderApi::new();
    let mut input = api.new_input(64, 64);
    let mesh = api.add_input_mesh(&mut input, 1, 3);
    let plain = api.add_input_basic_lit_material(&mut input, 10, Vec4::ONE);
    (0..3_u64).for_each(|index| {
        api.add_input_object(&mut input, index, Mat4::IDENTITY, mesh, plain, true);
    });
    let list = api.build_command_list(&input);
    assert_eq!(list.len(), 11);
    assert_eq!(
        api.command_pipeline_at(&list, 1),
        Some(RenderApi::PIPELINE_BASIC_LIT)
    );
}
