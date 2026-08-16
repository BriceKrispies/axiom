//! How an authored surface participates in lighting, lowered to this backend.
//!
//! Three things live here, and they are all consequences of one decision.
//!
//! ## The decision: a discriminant the program STATES, not a variant it selects
//!
//! [`axiom_surface::LightingModel`] is a closed three-valued set — `Unlit`,
//! `Lambert`, `LambertSpecular`. All three are compiled into the **one** lit
//! shader this backend has ([`crate::scene_wgsl`]), and the main pass's `fs`
//! picks between them with a `select` and two multipliers, exactly the way it
//! already gates its twelve capability bits. So a lighting model costs **zero
//! additional pipelines**: three models across N surfaces is N programs, never
//! 3N. The engine has an explicit, twice-written anti-variant doctrine
//! (`crate::post_chain`, `crate::surface_encode`) — *"so a device cannot stutter
//! compiling a second variant mid-session"* — and this work does not reverse it.
//!
//! The model reaches the shader as the return value of a generated nullary
//! function, [`lighting_model_function`], emitted alongside the six-channel
//! `axiom_surface` into **one** program keyed by **one** digest. It is not a lane
//! in the surface parameter buffer, and that is deliberate: the main pass binds
//! no parameter buffer yet and hands every program the zero value, and code `0`
//! is `Unlit` — a parameter lane would unlight every frame in the engine today.
//! A value the program states cannot default to zero by accident.
//!
//! ## `metallic` is reserved and INERT, on purpose
//!
//! [`axiom_surface::SurfaceChannel::Metallic`] is authored, digested, packed into
//! the parameter region and emitted into `SurfaceOut` — and **no lighting model
//! reads it**. `docs/specs/SPEC-11-3d-scene-surface.md` says *"Resist PBR scope
//! creep"*, and a metallic term is not one term: it is a Fresnel term plus an
//! environment term plus a split between diffuse and specular albedo, which is a
//! different project with its own capability, its own probes and its own budget.
//! Shipping a channel that *looks* wired but moves no pixel is worse than
//! shipping one that is documented inert, so this is stated here, at the site,
//! and proven by `metallic_is_reserved_and_changes_no_pixel` in
//! [`crate::surface_program::parity_lighting`].
//!
//! (The cautionary precedent is `roughness`, which sat inert long enough that
//! three app-side documents still call it dead months after it became the
//! `1.0 - roughness` specular strength. An inert channel must say so where the
//! reader is.)
//!
//! ## `RenderPipelineKind`, finally connected
//!
//! `axiom_render::RenderPipelineKind` declares `BASIC_LIT = 1` and `UNLIT = 2`,
//! the render module run-length-encodes a `SetPipeline` per switch — and the
//! value has died at the `axiom_host::FramePacket` boundary since it was written,
//! because the packet carries no pipeline lane. [`pipeline_kind`] is the other
//! end of that seam: this backend **derives** the marker from the surface a
//! draw's `surface_program` digest names, rather than the packet growing an
//! eighth lane for something a backend can compute. The two constants are a
//! mirror of the render module's — module isolation forbids importing them, the
//! same relationship the `CAP_*` bits in the main pass's WGSL have with
//! `axiom_host::RenderCapability` — and `axiom-render` derives the identical
//! marker from the identical `LightingModel` on its own side, so the mapping is
//! stated once per module from one shared layer type rather than guessed twice.

use axiom_surface::{LightingModel, Surface};

use crate::surface_program::emit::surface_function;
use crate::surface_program::program_error::SurfaceProgramError;

/// The pipeline marker a lit surface selects — `axiom_render::RenderPipelineKind::BASIC_LIT`.
pub(crate) const PIPELINE_BASIC_LIT: u32 = 1;

/// The pipeline marker an [`LightingModel::Unlit`] surface selects —
/// `axiom_render::RenderPipelineKind::UNLIT`, which until now nothing selected
/// and no backend saw.
pub(crate) const PIPELINE_UNLIT: u32 = 2;

/// The marker `model` selects. A table index, not a branch: the discriminant is
/// already a number, and `Unlit` is the only model that is not lit.
pub(crate) fn pipeline_kind(model: LightingModel) -> u32 {
    [PIPELINE_BASIC_LIT, PIPELINE_UNLIT][usize::from(model == LightingModel::Unlit)]
}

/// The WGSL a lighting model lowers to: a nullary function returning its wire
/// code.
///
/// A per-surface **constant**, so the shader compiler folds it and the three
/// models' gates collapse to literals inside one program — which is why all
/// three arms being present costs a compiled program nothing.
pub(crate) fn lighting_model_function(model: LightingModel) -> String {
    format!(
        "fn axiom_lighting_model() -> u32 {{\n    return {}u;\n}}\n",
        model.code()
    )
}

/// The whole **fragment-stage** program for `surface`: how it is lit, then what
/// its six channels are.
///
/// Two functions, one string, one digest. They are generated together because
/// they are two facts about the *same* authored surface, and splitting them
/// across two modules is precisely the variant multiplication this design
/// refuses.
pub(crate) fn fragment_program(surface: &Surface) -> Result<String, SurfaceProgramError> {
    surface_function(surface)
        .map(|channels| lighting_model_function(surface.lighting()) + &channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldValue};
    use axiom_recipe::{Param, Scalar};
    use axiom_surface::{LayerBlend, SurfaceBuilder, SurfaceChannel, SurfaceLayer};

    /// A vec4 base colour driven by `Uv.x` — a surface with no constant colour.
    fn uv_color() -> axiom_field::FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("gpu/light/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let (builder, splat) = builder.push(
            FieldOp::Compose,
            vec![Param::int(4)],
            vec![lane, lane, lane, lane],
        );
        builder.build(splat)
    }

    fn surface_with(model: LightingModel) -> Surface {
        SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_color())
            .lighting(model)
            .build()
            .expect("a vec4 uv field is a legal base colour under every model")
    }

    #[test]
    fn every_model_emits_its_own_wire_code_and_nothing_else() {
        LightingModel::ALL.iter().for_each(|model| {
            assert_eq!(
                lighting_model_function(*model),
                format!(
                    "fn axiom_lighting_model() -> u32 {{\n    return {}u;\n}}\n",
                    model.code()
                )
            );
        });
        assert!(lighting_model_function(LightingModel::Unlit).contains("return 0u;"));
        assert!(lighting_model_function(LightingModel::Lambert).contains("return 1u;"));
        assert!(
            lighting_model_function(LightingModel::LambertSpecular).contains("return 2u;"),
            "the default must be the code the DEFAULT program already returns"
        );
    }

    /// **The no-new-variants proof, in text.** Two surfaces differing only in
    /// lighting model emit programs that differ in exactly one token — the
    /// returned code — and each declares exactly one of each function. There is
    /// no per-model arm, no duplicated channel body, and nothing for a second
    /// pipeline to be keyed on.
    #[test]
    fn changing_only_the_lighting_model_changes_exactly_one_token_of_the_program() {
        let programs: Vec<String> = LightingModel::ALL
            .iter()
            .map(|model| fragment_program(&surface_with(*model)).expect("flattens"))
            .collect();
        programs.iter().for_each(|program| {
            assert_eq!(program.matches("fn axiom_lighting_model()").count(), 1);
            assert_eq!(program.matches("fn axiom_surface(").count(), 1);
            // The channel body is identical across the three: only the code moves.
            assert_eq!(
                program.split_once("fn axiom_surface(").expect("both halves").1,
                programs[0]
                    .split_once("fn axiom_surface(")
                    .expect("both halves")
                    .1
            );
        });
        assert_ne!(programs[0], programs[1]);
        assert_ne!(programs[1], programs[2]);
    }

    #[test]
    fn a_program_states_the_model_before_it_states_the_channels() {
        let text = fragment_program(&surface_with(LightingModel::Unlit)).expect("flattens");
        let model_at = text.find("fn axiom_lighting_model()").expect("emitted");
        let channels_at = text.find("fn axiom_surface(").expect("emitted");
        // WGSL requires a declaration before its use, and `fs` calls both.
        assert!(model_at < channels_at);
        assert!(text.ends_with("    return out;\n}\n"));
    }

    /// A surface authoring nothing about lighting emits the default model, which
    /// is the code the default program already returns — the compatibility
    /// guarantee, at the emitter.
    #[test]
    fn a_surface_that_says_nothing_about_lighting_emits_the_default_model() {
        let plain = SurfaceBuilder::new().build().expect("legal");
        assert_eq!(plain.lighting(), LightingModel::LambertSpecular);
        assert!(fragment_program(&plain)
            .expect("flattens")
            .contains("return 2u;"));
    }

    /// Flattening keeps the ROOT's model, so a layered surface's program states
    /// the model its author chose rather than a layer's.
    #[test]
    fn a_layered_surfaces_program_states_the_roots_model() {
        let layer = SurfaceLayer::new(
            SurfaceBuilder::new()
                .lighting(LightingModel::LambertSpecular)
                .field(SurfaceChannel::BaseColor, uv_color())
                .build()
                .expect("legal"),
            SurfaceLayer::opaque_mask(),
            LayerBlend::Over,
        );
        let root = SurfaceBuilder::new()
            .lighting(LightingModel::Unlit)
            .layer(layer)
            .build()
            .expect("one layer is within budget");
        assert!(fragment_program(&root)
            .expect("flattens")
            .contains("return 0u;"));
    }

    /// A surface whose channel graphs will not compose is a program failure, and
    /// the lighting model does not paper over it: no half-program is emitted.
    #[test]
    fn a_surface_that_will_not_flatten_emits_no_program_at_all() {
        let chain = |name: &str, steps: u16| {
            let (builder, node) = (0..steps).fold(
                FieldBuilder::new(FieldId::of_name(name), 1)
                    .push_const(FieldValue::scalar(Scalar::new(1.0))),
                |(builder, acc), _| {
                    let (builder, one) = builder.push_const(FieldValue::scalar(Scalar::new(1.0)));
                    builder.push(FieldOp::Add, Vec::new(), vec![acc, one])
                },
            );
            builder.build(node)
        };
        let over = SurfaceBuilder::new()
            .lighting(LightingModel::Unlit)
            .field(SurfaceChannel::Opacity, chain("gpu/light/under", 65))
            .layer(SurfaceLayer::new(
                SurfaceBuilder::new()
                    .field(SurfaceChannel::Opacity, chain("gpu/light/over", 65))
                    .build()
                    .expect("legal"),
                SurfaceLayer::opaque_mask(),
                LayerBlend::Over,
            ))
            .build()
            .expect("one layer is within budget");
        assert!(fragment_program(&over).is_err());
    }

    /// **`RenderPipelineKind::UNLIT` now has something behind it.** The mapping
    /// is total over the closed set: `Unlit` selects the unlit marker and both
    /// lit models select the lit one.
    #[test]
    fn only_an_unlit_surface_selects_the_unlit_pipeline_marker() {
        assert_eq!(pipeline_kind(LightingModel::Unlit), PIPELINE_UNLIT);
        assert_eq!(pipeline_kind(LightingModel::Lambert), PIPELINE_BASIC_LIT);
        assert_eq!(
            pipeline_kind(LightingModel::LambertSpecular),
            PIPELINE_BASIC_LIT
        );
        // The markers are the render module's numbers, mirrored — 1 and 2.
        assert_eq!(PIPELINE_BASIC_LIT, 1);
        assert_eq!(PIPELINE_UNLIT, 2);
        assert_ne!(PIPELINE_BASIC_LIT, PIPELINE_UNLIT);
    }

    /// The `metallic` channel is carried through the whole lowering and reaches
    /// `SurfaceOut` — and no lighting model reads it. This pins the *carried*
    /// half; `parity_lighting` pins the *inert* half on a real GPU.
    #[test]
    fn metallic_is_carried_into_the_program_and_read_by_no_model() {
        let metal = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Metallic,
                FieldValue::scalar(Scalar::new(1.0)),
            )
            .build()
            .expect("a constant metallic is legal");
        let text = fragment_program(&metal).expect("flattens");
        assert!(text.contains("out.metallic = "));
        // Two surfaces differing only in metallic are two distinct programs —
        // the channel is genuinely carried, not dropped on the floor — and
        // neither of them can move a pixel, because nothing reads the lane.
        let matte = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Metallic,
                FieldValue::scalar(Scalar::new(0.25)),
            )
            .build()
            .expect("a constant metallic is legal");
        assert_ne!(metal.digest().raw(), matte.digest().raw());
        assert_ne!(text, fragment_program(&matte).expect("flattens"));
    }
}
