//! CPU evaluation of an authored [`Surface`], once per triangle.
//!
//! This backend shades **per triangle**, not per pixel
//! ([`crate::raster_triangle::RasterTriangle`] carries one flat colour), and it
//! executes no shader at all. That would make an authored surface impossible —
//! except that a surface's channels are not programs, they are *fields*: pure
//! functions of an explicitly supplied evaluation context, with a reference
//! evaluator that runs anywhere. So this backend does the one thing it can do
//! and the GPU arm cannot do without a compiler: it calls
//! [`axiom_field::FieldGraph::evaluate`] directly.
//!
//! **That makes `RenderCapability::ProceduralSurface` a substitute here, not a
//! drop.** The substitution is the sampling rate — one evaluation at the
//! triangle's centroid instead of one per fragment — which is the same fidelity
//! relationship every other capability has on this backend.
//!
//! ## Where the sample point comes from
//!
//! A surface's expressions are declared to be evaluated in **object space**, so
//! that a pattern rides with the object instead of swimming as it moves. The
//! rasterizer's mesh cache keeps positions exactly as uploaded — object space —
//! and the draw's `mvp` is what takes them to the screen. The centroid of the
//! three object-space positions is therefore the sample point directly: no
//! matrix is inverted, per frame or ever. The uv comes the same way (kept at
//! upload for this one purpose) and the normal is the object-space face normal.
//!
//! ## What is honoured, and what is not
//!
//! Base colour, emission and opacity are honoured: each has somewhere to land in
//! the existing flat-colour path. **Roughness and metallic are not, and are not
//! faked.** [`crate::canvas_depth_cue::shade_triangle`] has no view vector — it
//! is view-independent by construction — so there is no highlight for a
//! roughness to tighten. A frame presenting a surface that binds either is
//! reported degraded through the existing `SpecularHighlight` feature, which is
//! precisely the term that is missing.
//!
//! **Displacement is not honoured either, and it is reported dropped.** This
//! path shades geometry; it does not move it, however finely it samples. The
//! reason is cost, not principle: this backend already CPU-skins, allocating a
//! fresh [`crate::mesh_cache::MeshGeometry`] per skinned draw per frame, and
//! evaluating a field per *vertex* on top of that would multiply that cost on
//! the arm least able to pay it — while the GPU arm gets the same deformation
//! for free in a stage it already runs.
//!
//! So the two backends' silhouettes differ for a displacing surface, and the
//! frame is told: [`Self::displaces`] feeds the
//! `axiom_host::FrameFeature::ProceduralSurface` drop in
//! [`crate::Canvas2dBackendApi`]'s report. That divergence is consistent with
//! the software arm's declared policy — burnt-rubber's own convergence campaign
//! sets `guard_rule = "legibility, not parity"` for it — and a *stated* drop is
//! the whole difference between a degrade and a bug.
//!
//! [`Self::displaces`]: SurfaceCache::displaces

use axiom_field::{EvalContext, FieldValue};
use axiom_kernel::Seconds;
use axiom_math::{Vec2, Vec3};
use axiom_surface::{Surface, SurfaceChannel};

/// The channels one CPU evaluation produced, in the form the flat-colour path
/// consumes: a linear RGBA whose alpha already folds in the opacity channel, and
/// a linear RGB radiance added after the depth cues.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShadedChannels {
    base_color: [f32; 4],
    emission: [f32; 3],
}

impl ShadedChannels {
    /// The linear RGBA to fill the triangle with, before the depth cues.
    pub(crate) const fn base_color(self) -> [f32; 4] {
        self.base_color
    }

    /// The linear RGB self-illumination to add after the depth cues — the same
    /// place the draw's own emissive lands, because both are radiance.
    pub(crate) const fn emission(self) -> [f32; 3] {
        self.emission
    }
}

/// Evaluate a surface's shadeable channels at one object-space sample.
///
/// A channel bound to a constant answers with it; a channel bound to a field is
/// evaluated; a field that fails to evaluate falls back to the channel's own
/// default, which is the same value an unbound channel holds — so a hostile
/// graph degrades to the engine's default material rather than to a black hole.
pub(crate) fn shade_surface(
    surface: &Surface,
    centroid_object: Vec3,
    uv: Vec2,
    normal: Vec3,
    time: Seconds,
) -> ShadedChannels {
    let context = EvalContext::new(centroid_object, uv, normal, time);
    let base = channel_value(surface, SurfaceChannel::BaseColor, &context).as_vec4();
    let emission = channel_value(surface, SurfaceChannel::Emission, &context).as_vec4();
    let opacity = channel_value(surface, SurfaceChannel::Opacity, &context)
        .as_scalar()
        .get();
    ShadedChannels {
        base_color: [base.x, base.y, base.z, base.w * opacity],
        emission: [emission.x, emission.y, emission.z],
    }
}

/// One channel's value at `context`.
///
/// The fallback is computed eagerly rather than behind a closure: a channel's
/// default is a `const` table read, so there is nothing to defer, and deferring
/// it would only add an arm no validated surface can reach.
fn channel_value(surface: &Surface, channel: SurfaceChannel, context: &EvalContext) -> FieldValue {
    let binding = surface.binding(channel);
    binding
        .as_constant()
        .or_else(|| {
            binding
                .as_field()
                .and_then(|graph| graph.evaluate(context).ok())
        })
        .map_or(channel.default_value(), |value| value)
}

/// The frame's authored surfaces, flattened and keyed by the digest their draws
/// carry, plus the presentation time their `Time`-reading channels sample.
///
/// Built per present rather than retained — this backend holds no mutable
/// state — and *flattened* at build, because flattening is what resolves a
/// layered surface into one binding per channel, which is the only form a
/// per-triangle evaluation can consume without walking a tree per triangle.
///
/// A surface that binds a channel this backend cannot express is recorded here
/// as well, so the report can name what was not honoured without re-inspecting
/// the surfaces.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceCache {
    entries: Vec<(u64, Surface)>,
    time: Seconds,
    view_dependent_channels: bool,
    displaces: bool,
}

impl Default for SurfaceCache {
    /// The empty cache: no surfaces, time zero. What every present that was
    /// handed no surfaces uses, and what makes that present bit-identical to
    /// the pre-surface path.
    fn default() -> Self {
        SurfaceCache::build(&[], Seconds::finite_or_zero(0.0))
    }
}

impl SurfaceCache {
    /// Flatten and index `surfaces`, keyed by [`Surface::digest`] — the same
    /// number [`axiom_host::FrameDrawItem::surface_program`] carries.
    ///
    /// A surface that fails to flatten is dropped from the index, so a draw
    /// naming it misses and is reported unhonoured rather than half-shaded. It
    /// cannot happen for a `Surface` in hand: both of its constructors validate.
    pub(crate) fn build(surfaces: &[Surface], time: Seconds) -> SurfaceCache {
        SurfaceCache {
            entries: surfaces
                .iter()
                .filter_map(|surface| {
                    surface
                        .flatten()
                        .ok()
                        .map(|flat| (surface.digest().raw(), flat))
                })
                .collect(),
            time,
            view_dependent_channels: surfaces.iter().any(binds_view_dependent_channel),
            displaces: surfaces
                .iter()
                .any(|surface| surface.requirements().has_displacement()),
        }
    }

    /// Whether any presented surface binds a channel that only a view-dependent
    /// shade could express (roughness, metallic).
    pub(crate) const fn has_view_dependent_channels(&self) -> bool {
        self.view_dependent_channels
    }

    /// Whether any presented surface moves geometry — the one channel this
    /// path shades around rather than honouring.
    pub(crate) const fn displaces(&self) -> bool {
        self.displaces
    }

    /// Whether `program_id` names a surface this cache can shade. `0` — what
    /// every draw that authored no surface carries — never does.
    pub(crate) fn knows(&self, program_id: u64) -> bool {
        self.lookup(program_id).is_some()
    }

    /// Shade the surface `program_id` names at one triangle's object-space
    /// centroid, or `None` when this cache holds no such surface (including the
    /// `program_id = 0` every plain draw carries).
    pub(crate) fn shade(
        &self,
        program_id: u64,
        centroid_object: [f32; 3],
        uv: [f32; 2],
        normal: [f32; 3],
    ) -> Option<ShadedChannels> {
        self.lookup(program_id).map(|surface| {
            shade_surface(
                surface,
                Vec3::new(centroid_object[0], centroid_object[1], centroid_object[2]),
                Vec2::new(uv[0], uv[1]),
                Vec3::new(normal[0], normal[1], normal[2]),
                self.time,
            )
        })
    }

    /// The flattened surface `program_id` names.
    fn lookup(&self, program_id: u64) -> Option<&Surface> {
        self.entries
            .iter()
            .find(|(id, _)| *id == program_id)
            .map(|(_, surface)| surface)
    }
}

/// Whether a surface binds roughness or metallic away from the value an unbound
/// channel holds — the two channels this backend has no expression for.
fn binds_view_dependent_channel(surface: &Surface) -> bool {
    [SurfaceChannel::Roughness, SurfaceChannel::Metallic]
        .iter()
        .any(|channel| {
            surface.binding(*channel).as_constant() != Some(channel.default_value())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldOp, FieldValue};
    use axiom_math::Vec4;
    use axiom_recipe::{Param, Scalar};
    use axiom_surface::{LayerBlend, SurfaceBuilder, SurfaceLayer};

    /// A vec4 base colour that is `Uv.x` in every lane — the canonical
    /// field-authored surface, and one whose value at a centroid is a number a
    /// test can compute by hand.
    fn uv_x_color() -> axiom_field::FieldGraph {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("c2d/shade/uv"), 1).push(
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

    fn zero() -> Seconds {
        Seconds::finite_or_zero(0.0)
    }

    #[test]
    fn a_constant_surface_evaluates_to_its_constants() {
        let surface = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::BaseColor,
                FieldValue::vec4(Vec4::new(0.2, 0.4, 0.6, 1.0)),
            )
            .constant(
                SurfaceChannel::Emission,
                FieldValue::vec4(Vec4::new(0.0, 0.5, 0.0, 0.0)),
            )
            .constant(SurfaceChannel::Opacity, FieldValue::scalar(Scalar::new(0.5)))
            .build()
            .expect("legal");
        let shaded = shade_surface(&surface, Vec3::ZERO, Vec2::ZERO, Vec3::UNIT_Y, zero());
        // Opacity folds into alpha; it is not a fourth channel the flat path has
        // a lane for.
        assert_eq!(shaded.base_color(), [0.2, 0.4, 0.6, 0.5]);
        assert_eq!(shaded.emission(), [0.0, 0.5, 0.0]);
        assert_eq!(
            shaded,
            shade_surface(&surface, Vec3::ZERO, Vec2::ZERO, Vec3::UNIT_Y, zero())
        );
        assert!(format!("{shaded:?}").contains("ShadedChannels"));
    }

    #[test]
    fn a_uv_driven_colour_evaluates_to_the_uv_at_the_sample() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_x_color())
            .build()
            .expect("a vec4 uv field is a legal base colour")
            .flatten()
            .expect("flattens");
        let shaded = shade_surface(
            &surface,
            Vec3::ZERO,
            Vec2::new(0.25, 0.75),
            Vec3::UNIT_Y,
            zero(),
        );
        assert_eq!(shaded.base_color(), [0.25, 0.25, 0.25, 0.25]);
        // A different sample gives a different colour — the whole point.
        let elsewhere = shade_surface(
            &surface,
            Vec3::ZERO,
            Vec2::new(0.5, 0.0),
            Vec3::UNIT_Y,
            zero(),
        );
        assert_eq!(elsewhere.base_color(), [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn a_time_reading_channel_samples_the_presentation_time_it_was_given() {
        let (builder, node) = FieldBuilder::new(FieldId::of_name("c2d/shade/t"), 1).push(
            FieldOp::Time,
            Vec::new(),
            Vec::new(),
        );
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(node))
            .build()
            .expect("a scalar time field is a legal opacity");
        let cache = SurfaceCache::build(
            std::slice::from_ref(&surface),
            Seconds::finite_or_zero(0.25),
        );
        let shaded = cache
            .shade(surface.digest().raw(), [0.0; 3], [0.0; 2], [0.0, 1.0, 0.0])
            .expect("the cache holds it");
        // Base colour is the default opaque white; opacity is the time.
        assert_eq!(shaded.base_color(), [1.0, 1.0, 1.0, 0.25]);
    }

    #[test]
    fn a_point_driven_channel_samples_the_object_space_centroid() {
        let (builder, point) = FieldBuilder::new(FieldId::of_name("c2d/shade/pt"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(1)], vec![point]);
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(lane))
            .build()
            .expect("a scalar point lane is a legal opacity");
        let cache = SurfaceCache::build(std::slice::from_ref(&surface), zero());
        let shaded = cache
            .shade(
                surface.digest().raw(),
                [0.0, 0.625, 0.0],
                [0.0; 2],
                [0.0, 1.0, 0.0],
            )
            .expect("the cache holds it");
        assert_eq!(shaded.base_color()[3], 0.625);
    }

    #[test]
    fn a_normal_driven_channel_samples_the_object_space_face_normal() {
        let (builder, normal) = FieldBuilder::new(FieldId::of_name("c2d/shade/n"), 1).push(
            FieldOp::Normal,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(2)], vec![normal]);
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(lane))
            .build()
            .expect("legal");
        let cache = SurfaceCache::build(std::slice::from_ref(&surface), zero());
        let shaded = cache
            .shade(surface.digest().raw(), [0.0; 3], [0.0; 2], [0.0, 0.0, 1.0])
            .expect("the cache holds it");
        assert_eq!(shaded.base_color()[3], 1.0);
    }

    #[test]
    fn an_empty_cache_knows_nothing_and_shades_nothing() {
        let cache = SurfaceCache::default();
        assert!(!cache.knows(0));
        assert!(!cache.knows(1234));
        assert!(cache.shade(0, [0.0; 3], [0.0; 2], [0.0, 1.0, 0.0]).is_none());
        assert!(!cache.has_view_dependent_channels());
        assert!(!cache.displaces());
        assert_eq!(cache, SurfaceCache::build(&[], zero()));
        assert!(format!("{cache:?}").contains("SurfaceCache"));
    }

    #[test]
    fn a_cache_knows_the_digest_its_draws_carry_and_nothing_else() {
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, uv_x_color())
            .build()
            .expect("legal");
        let cache = SurfaceCache::build(std::slice::from_ref(&surface), zero());
        assert!(cache.knows(surface.digest().raw()));
        assert!(!cache.knows(surface.digest().raw() ^ 1));
        assert!(cache
            .shade(
                surface.digest().raw() ^ 1,
                [0.0; 3],
                [0.5, 0.0],
                [0.0, 1.0, 0.0]
            )
            .is_none());
    }

    #[test]
    fn a_layered_surface_is_flattened_once_at_build_not_walked_per_triangle() {
        // The layer's base colour wins under an opaque `Over` mask, so a shade
        // that walked only the root would answer white instead of the uv ramp.
        let layer = SurfaceLayer::new(
            SurfaceBuilder::new()
                .field(SurfaceChannel::BaseColor, uv_x_color())
                .build()
                .expect("legal"),
            SurfaceLayer::opaque_mask(),
            LayerBlend::Over,
        );
        let surface = SurfaceBuilder::new()
            .layer(layer)
            .build()
            .expect("one layer is within budget");
        let cache = SurfaceCache::build(std::slice::from_ref(&surface), zero());
        let shaded = cache
            .shade(
                surface.digest().raw(),
                [0.0; 3],
                [0.75, 0.0],
                [0.0, 1.0, 0.0],
            )
            .expect("the cache holds it");
        assert_eq!(shaded.base_color(), [0.75, 0.75, 0.75, 0.75]);
    }

    #[test]
    fn roughness_and_metallic_are_recorded_as_unexpressible_not_faked() {
        let plain = SurfaceBuilder::new().build().expect("legal");
        assert!(!SurfaceCache::build(std::slice::from_ref(&plain), zero())
            .has_view_dependent_channels());
        [SurfaceChannel::Roughness, SurfaceChannel::Metallic]
            .iter()
            .for_each(|channel| {
                let bound = SurfaceBuilder::new()
                    .constant(*channel, FieldValue::scalar(Scalar::new(0.875)))
                    .build()
                    .expect("a scalar constant is legal for both");
                let cache = SurfaceCache::build(std::slice::from_ref(&bound), zero());
                assert!(cache.has_view_dependent_channels(), "{channel:?}");
                // The colour is still shaded exactly — only the view-dependent
                // term is missing, and it is reported rather than approximated.
                assert_eq!(
                    cache
                        .shade(bound.digest().raw(), [0.0; 3], [0.0; 2], [0.0, 1.0, 0.0])
                        .expect("held")
                        .base_color(),
                    [1.0, 1.0, 1.0, 1.0]
                );
            });
    }

    #[test]
    fn a_displacing_surface_is_recorded_because_this_path_shades_but_never_moves() {
        let surface = SurfaceBuilder::new()
            .constant(
                SurfaceChannel::Displacement,
                FieldValue::vec3(Vec3::new(0.0, 1.0, 0.0)),
            )
            .build()
            .expect("a vec3 constant is a legal displacement");
        assert!(SurfaceCache::build(std::slice::from_ref(&surface), zero()).displaces());
    }
}
