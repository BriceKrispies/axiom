//! Adapt a backend-neutral [`axiom_host::FramePacket`] into the live GPU path's
//! input.
//!
//! The live `SceneRenderer` consumes per-`(mesh, material)` instance batches and
//! a flat light list (the shape [`crate::GpuBackendApi::present_frame`] takes).
//! This module derives exactly that shape from a `FramePacket`, so the GPU
//! backend presents the shared packet with **no** change to the renderer. The
//! packing layout is byte-identical to the legacy batch format:
//! `INSTANCE_FLOATS` floats per instance — `mvp[16]`, then `world[16]`, then
//! `colour[4]`, then `emissive[3]` + `specular[1]` — grouped by
//! `(mesh_id, material_id)` in first-appearance order.

use axiom_host::FramePacket;

use crate::surface_program::SurfaceProgramSet;

/// Floats one packed instance occupies: `mvp(16) + world(16) + colour(4) +
/// emissive(3) + specular(1)`. This module owns the number because it owns the
/// packing; the renderer's vertex layout is derived from it, and it must stay
/// equal to `axiom::FrameOutcome`'s `INSTANCE_FLOATS` (the same bytes, packed by
/// the other producer). The emissive lane is a full `vec4` because a vertex
/// attribute is the granularity both wgpu and the WebGL2 downlevel target
/// describe — and its fourth float, once an unread pad, now carries the
/// material's specular strength, which is why no attribute had to be added.
pub(crate) const INSTANCE_FLOATS: usize = 40;

/// Group a packet's draws into per-`(mesh, material)` instance batches:
/// `(mesh_id, material_id, [mvp(16), world(16), colour(4), emissive(3)+specular(1)]
/// per instance, count)`, one entry per distinct `(mesh, material)` pair in
/// first-appearance order. Byte-identical to the `mesh_batches` layout the live
/// renderer consumes.
/// Grouped by *sorting*, not by hashing.
///
/// This built a `HashMap<(u64, u64), Vec<f32>>` per frame until 2026-08-11, and
/// a throttled profile put ~10% of the frame inside `hashbrown` because of it.
/// The map was doing badly-paid work: in a scene like Burnt Rubber's road almost
/// every draw carries its *own* mesh, so nearly every key is distinct and the
/// per-frame cost was a fresh map allocation, one hash probe to insert and a
/// second to remove for ~1000 groups that mostly hold a single instance each —
/// all to discover that there was nothing to batch.
///
/// Sorting an index permutation makes equal keys contiguous, so `chunk_by` reads
/// the groups straight off in one pass with no hashing and no map. It also makes
/// each group's length known *before* its buffer is filled, so the instance
/// floats go into a `with_capacity` allocation instead of growing 40 floats at a
/// time.
///
/// The sort key carries the draw's original index, so a run's first element is
/// always its earliest draw — which is what restores **first-appearance order**
/// at the end, the ordering the renderer's batch contract requires and the one
/// property a sort would otherwise destroy.
///
/// `surfaces` is this frame's authored surface set. A draw naming a surface
/// program this backend could not lower still renders that surface's **constant**
/// channels: its constant base colour multiplies the instance colour and its
/// constant emission adds to the instance emissive, both through lanes the
/// stream already has. A draw carrying `surface_program = 0` — every draw in
/// every app that authors no surface — folds in the identity `(white, black)`,
/// which is an exact IEEE no-op, so the packed bytes are unchanged.
///
/// ## The key carries the SURFACE PROGRAM, and that is a draw-order decision
///
/// A surface program is a *pipeline*, so a batch cannot straddle two of them.
/// Extending the existing sort key to `(surface_program, mesh_id, material_id)`
/// makes every draw sharing a program contiguous, which is what lets the renderer
/// set each pipeline **once per frame** instead of once per batch — on the WebGL2
/// path a `set_pipeline` is not free and a draw already costs ~52 GL calls.
///
/// The key is *extended*, never replaced by a map. This function built a
/// `HashMap<(u64, u64), Vec<f32>>` per frame until 2026-08-11 and a throttled
/// profile put ~10% of the frame inside `hashbrown`; the sort is the fix and a
/// third key lane costs it nothing.
///
/// Within one program, first-appearance order is preserved exactly as before —
/// so a frame in which **every** draw carries `surface_program = 0` (every
/// existing app) produces byte-identical batches in an identical order. Across
/// programs the groups are ordered by program id, with `0` first because no
/// digest is zero: a scene mixing surfaced and unsurfaced geometry draws the
/// unsurfaced half first.
///
/// The second returned vector is each batch's program id, in batch order — what
/// `crate::scene_renderer::SceneRenderer::record` selects a pipeline with.
pub(crate) fn frame_packet_to_batches(
    packet: &FramePacket,
    surfaces: &SurfaceProgramSet,
) -> (Vec<(u64, u64, Vec<f32>, u32)>, Vec<u64>) {
    let draws = packet.draws();
    let key = |index: &u32| {
        let draw = &draws[*index as usize];
        (draw.surface_program(), draw.mesh_id(), draw.material_id())
    };

    let mut order: Vec<u32> = (0..draws.len() as u32).collect();
    order.sort_unstable_by_key(|index| (key(index), *index));

    // `(mesh, material, floats, instance count, earliest draw index, program)`.
    let mut batches: Vec<(u64, u64, Vec<f32>, u32, u32, u64)> = order
        .chunk_by(|a, b| key(a) == key(b))
        .map(|run| {
            let (program_id, mesh_id, material_id) = key(&run[0]);
            let mut floats = Vec::with_capacity(run.len() * INSTANCE_FLOATS);
            run.iter().for_each(|index| {
                let draw = &draws[*index as usize];
                let (tint, glow) = surfaces.constant_fallback(draw.surface_program());
                let c = draw.color();
                floats.extend_from_slice(&draw.mvp());
                floats.extend_from_slice(&draw.world());
                floats.extend_from_slice(&[
                    c[0] * tint[0],
                    c[1] * tint[1],
                    c[2] * tint[2],
                    c[3] * tint[3],
                ]);
                // The emissive lane, filled out to a `vec4` — the vertex-attribute
                // granularity the instance buffer is described in — by the material's
                // specular strength, which is what that fourth lane carries now that the
                // shader has a highlight term to spend it on.
                let e = draw.emissive();
                floats.extend_from_slice(&[
                    e[0] + glow[0],
                    e[1] + glow[1],
                    e[2] + glow[2],
                    draw.specular().get(),
                ]);
            });
            (
                mesh_id,
                material_id,
                floats,
                run.len() as u32,
                run[0],
                program_id,
            )
        })
        .collect();

    // Grouped by program, and inside a program by first appearance: the first key
    // is what keeps one `set_pipeline` per program, the second is the ordering
    // the renderer's batch contract has always required.
    batches.sort_unstable_by_key(|batch| (batch.5, batch.4));
    let programs: Vec<u64> = batches.iter().map(|batch| batch.5).collect();
    (
        batches
            .into_iter()
            .map(|(mesh_id, material_id, floats, count, _, _)| {
                (mesh_id, material_id, floats, count)
            })
            .collect(),
        programs,
    )
}

/// Every distinct surface program a packet's draws name, ascending.
///
/// What `crate::GpuBackendApi::frame_degradations` asks the prepared catalog
/// about: a program in this list that the preparation barrier did not prepare is
/// a frame-time cache miss, which is reported and rendered with the constant
/// fallback — never compiled. Deduplicated so a thousand draws of one unprepared
/// surface is one question, not a thousand.
pub(crate) fn frame_packet_programs(packet: &FramePacket) -> Vec<u64> {
    let mut programs: Vec<u64> = packet
        .draws()
        .iter()
        .map(axiom_host::FrameDrawItem::surface_program)
        .collect();
    programs.sort_unstable();
    programs.dedup();
    programs
}

/// Flatten a packet's lights into the live path's light tuples
/// `(kind, vec, colour, intensity)`, in packet order. The packet stores colour
/// and intensity packed as `[r, g, b, intensity]`; this splits them back out.
pub(crate) fn frame_packet_lights(packet: &FramePacket) -> Vec<(u32, [f32; 3], [f32; 3], f32)> {
    packet
        .lights()
        .iter()
        .map(|l| {
            let ci = l.color_intensity();
            (l.kind(), l.vec(), [ci[0], ci[1], ci[2]], ci[3])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{FrameDrawItem, FrameFeatureSet, FrameLight, FrameViewport};

    fn packet(draws: Vec<FrameDrawItem>, lights: Vec<FrameLight>) -> FramePacket {
        FramePacket::new(
            0,
            0,
            FrameViewport::new(1, 1),
            [0.0; 4],
            None,
            draws,
            lights,
            [0.0; 16],
            FrameFeatureSet::new(false, false, 0, 0),
        )
    }

    #[test]
    fn batches_match_the_legacy_mesh_batches_layout_exactly() {
        // Same scenario as axiom::frame_outcome's mesh_batches golden test:
        // mesh 7, materials 5 (draws 0,2) then 6 (draw 1); mvp [1;16]/[2;16]/[3;16],
        // world [9;16]/[8;16]/[7;16]. A textured + an untextured material on one
        // mesh must not merge (the pair, not the mesh, keys a batch).
        let draws = vec![
            FrameDrawItem::new(0, 7, 5, [9.0; 16], [1.0; 16], [0.1, 0.2, 0.3, 1.0], false),
            FrameDrawItem::new(1, 7, 6, [8.0; 16], [2.0; 16], [0.4, 0.5, 0.6, 1.0], false),
            FrameDrawItem::new(2, 7, 5, [7.0; 16], [3.0; 16], [0.7, 0.8, 0.9, 1.0], false)
                .with_emissive([2.0, 0.5, 0.0])
                .with_specular(axiom_kernel::Ratio::finite_or_zero(0.6)),
        ];
        let (batches, programs) =
            frame_packet_to_batches(&packet(draws, Vec::new()), &SurfaceProgramSet::default());

        assert_eq!(batches.len(), 2);
        // Nothing authored a surface, so every batch draws the default program.
        assert_eq!(programs, vec![0, 0]);
        // First-appearance order: (7,5) first with 2 instances, then (7,6) with 1.
        assert_eq!((batches[0].0, batches[0].1), (7, 5));
        assert_eq!(batches[0].3, 2);
        assert_eq!(batches[0].2.len(), 80); // 2 instances x 40 floats
                                            // Instance 0 = draw 0: mvp, world, colour, emissive+pad.
        assert_eq!(&batches[0].2[0..16], &[1.0; 16]);
        assert_eq!(&batches[0].2[16..32], &[9.0; 16]);
        assert_eq!(&batches[0].2[32..36], &[0.1, 0.2, 0.3, 1.0]);
        assert_eq!(&batches[0].2[36..40], &[0.0, 0.0, 0.0, 0.0], "matte, non-emissive");
        // Instance 1 = draw 2 (same pair), which authored an emissive.
        assert_eq!(&batches[0].2[40..56], &[3.0; 16]);
        assert_eq!(&batches[0].2[56..72], &[7.0; 16]);
        assert_eq!(&batches[0].2[72..76], &[0.7, 0.8, 0.9, 1.0]);
        // The emissive lane's fourth float is the specular strength, not a pad.
        assert_eq!(&batches[0].2[76..80], &[2.0, 0.5, 0.0, 0.6]);

        assert_eq!((batches[1].0, batches[1].1), (7, 6));
        assert_eq!(batches[1].3, 1);
        assert_eq!(&batches[1].2[0..16], &[2.0; 16]);
        assert_eq!(&batches[1].2[16..32], &[8.0; 16]);
    }

    /// Groups come back in **first-appearance** order, not key order.
    ///
    /// The distinction is invisible until a later key sorts before an earlier
    /// one, which is exactly what grouping-by-sorting would get wrong: mesh 9
    /// is drawn first but sorts last. Without the final reordering this test
    /// reports `[2, 5, 9]` and the renderer draws the scene in a different
    /// order than the packet asked for.
    #[test]
    fn groups_keep_first_appearance_order_even_when_keys_sort_the_other_way() {
        let draws = vec![
            FrameDrawItem::new(0, 9, 1, [0.0; 16], [0.0; 16], [1.0; 4], false),
            FrameDrawItem::new(1, 5, 1, [0.0; 16], [0.0; 16], [1.0; 4], false),
            FrameDrawItem::new(2, 2, 1, [0.0; 16], [0.0; 16], [1.0; 4], false),
            // A second instance of the mesh drawn first: it must join mesh 9's
            // batch without promoting that batch, which already leads.
            FrameDrawItem::new(3, 9, 1, [0.0; 16], [0.0; 16], [1.0; 4], false),
        ];
        let (batches, programs) =
            frame_packet_to_batches(&packet(draws, Vec::new()), &SurfaceProgramSet::default());
        let order: Vec<(u64, u32)> = batches.iter().map(|b| (b.0, b.3)).collect();
        assert_eq!(order, vec![(9, 2), (5, 1), (2, 1)]);
        assert_eq!(programs, vec![0, 0, 0]);
    }

    #[test]
    fn empty_packet_yields_no_batches_and_no_lights() {
        let p = packet(Vec::new(), Vec::new());
        let (batches, programs) = frame_packet_to_batches(&p, &SurfaceProgramSet::default());
        assert!(batches.is_empty());
        assert!(programs.is_empty());
        assert!(frame_packet_lights(&p).is_empty());
        assert!(frame_packet_programs(&p).is_empty());
    }

    /// A draw naming an unlowerable surface still gets that surface's constant
    /// channels, and a draw naming no surface is byte-identical to before.
    #[test]
    fn an_unlowerable_surfaces_constants_still_reach_the_instance_stream() {
        let surface = axiom_surface::SurfaceBuilder::new()
            .constant(
                axiom_surface::SurfaceChannel::BaseColor,
                axiom_field::FieldValue::vec4(axiom_math::Vec4::new(0.5, 0.25, 1.0, 1.0)),
            )
            .constant(
                axiom_surface::SurfaceChannel::Emission,
                axiom_field::FieldValue::vec4(axiom_math::Vec4::new(0.0, 0.125, 0.0, 0.0)),
            )
            .build()
            .expect("two vec4 constants are legal channels");
        let program = surface.digest().raw();
        let set = SurfaceProgramSet::build(
            std::slice::from_ref(&surface),
            axiom_host::BackendCapabilityProfile::all()
                .without(axiom_host::RenderCapability::ProceduralSurface),
        );
        let draws = vec![
            FrameDrawItem::new(0, 1, 1, [0.0; 16], [0.0; 16], [1.0, 1.0, 1.0, 1.0], false)
                .with_surface_program(program),
            // A plain draw, in the same batch, must be untouched.
            FrameDrawItem::new(1, 1, 1, [0.0; 16], [0.0; 16], [0.4, 0.4, 0.4, 1.0], false)
                .with_emissive([0.75, 0.0, 0.0]),
        ];
        let p = packet(draws, Vec::new());
        let (batches, programs) = frame_packet_to_batches(&p, &set);
        // TWO batches now, not one: a surface program is a pipeline, so a draw
        // that names one cannot share a batch with a draw that does not — even
        // on the same mesh and material. The unsurfaced draw leads, because no
        // digest is zero.
        assert_eq!(batches.len(), 2);
        assert_eq!(programs, vec![0, program]);
        // The plain draw: bit-identical to the stream this module packed before
        // surfaces existed.
        assert_eq!(&batches[0].2[32..36], &[0.4, 0.4, 0.4, 1.0]);
        assert_eq!(&batches[0].2[36..40], &[0.75, 0.0, 0.0, 0.0]);
        // The surfaced draw: the surface's constant base colour multiplied in,
        // its constant emission added.
        assert_eq!(&batches[1].2[32..36], &[0.5, 0.25, 1.0, 1.0]);
        assert_eq!(&batches[1].2[36..40], &[0.0, 0.125, 0.0, 0.0]);
        // And the packet's distinct programs, deduplicated and ascending — what
        // the prepared catalog is asked about.
        assert_eq!(frame_packet_programs(&p), vec![0, program]);
    }

    /// **Draws are grouped by program, and one program is one contiguous run.**
    ///
    /// Four draws alternating between two surfaces on one mesh/material come back
    /// as two batches, not four — which is what lets the renderer issue one
    /// `set_pipeline` per program per frame instead of one per batch.
    #[test]
    fn draws_are_grouped_by_surface_program_so_each_pipeline_is_set_once() {
        let draws = (0..4_u64)
            .map(|index| {
                FrameDrawItem::new(index, 1, 1, [0.0; 16], [0.0; 16], [1.0; 4], false)
                    .with_surface_program(7 + index % 2)
            })
            .collect();
        let (batches, programs) =
            frame_packet_to_batches(&packet(draws, Vec::new()), &SurfaceProgramSet::default());
        assert_eq!(batches.len(), 2);
        assert_eq!(programs, vec![7, 8]);
        assert_eq!(batches[0].3, 2, "both draws of program 7 in one batch");
        assert_eq!(batches[1].3, 2, "both draws of program 8 in one batch");
        // A run's instances keep the order the packet drew them in.
        assert_eq!(frame_packet_programs(&packet(
            (0..4_u64)
                .map(|index| FrameDrawItem::new(index, 1, 1, [0.0; 16], [0.0; 16], [1.0; 4], false)
                    .with_surface_program(7 + index % 2))
                .collect(),
            Vec::new(),
        )), vec![7, 8]);
    }

    /// How many `set_pipeline` calls the renderer's draw loop makes for a batch
    /// list, counting from the default program it starts bound to.
    ///
    /// A mirror of the loop's own condition (`*program != bound`, starting at
    /// `0`) — the one number the surface-program work could have made worse, so
    /// it is counted here rather than asserted about in prose.
    fn pipeline_switches(programs: &[u64]) -> usize {
        programs
            .iter()
            .fold((0_usize, 0_u64), |(count, bound), program| {
                (count + usize::from(*program != bound), *program)
            })
            .0
    }

    /// **A surface-free scene costs zero pipeline switches, whatever it draws.**
    ///
    /// The draw loop begins bound to the default pipeline, and a frame in which
    /// every draw carries `surface_program = 0` never leaves it — so the number
    /// of `set_pipeline` calls in the main pass is exactly the one it always was,
    /// and the number of draw calls is exactly the batch count it always was.
    /// The only thing the surface work adds to such a frame is one
    /// `set_bind_group(3, …)` per pass, which is the price of binding the
    /// parameter region at all.
    #[test]
    fn a_surface_free_scene_costs_no_pipeline_switch_and_no_extra_draw() {
        let draws: Vec<FrameDrawItem> = (0..12_u64)
            .map(|index| {
                FrameDrawItem::new(
                    index,
                    index % 4,
                    index % 3,
                    [0.0; 16],
                    [0.0; 16],
                    [1.0; 4],
                    false,
                )
            })
            .collect();
        let (batches, programs) =
            frame_packet_to_batches(&packet(draws, Vec::new()), &SurfaceProgramSet::default());
        // 12 draws over 4 meshes x 3 materials, each pair distinct: 12 batches,
        // one draw call each — the count before surfaces existed.
        assert_eq!(batches.len(), 12);
        assert_eq!(programs.len(), batches.len());
        assert!(programs.iter().all(|program| *program == 0));
        assert_eq!(pipeline_switches(&programs), 0);

        // And with surfaces in the frame, the count is the number of distinct
        // PROGRAMS, not of batches: two programs over six batches is two
        // switches, not six.
        let mixed: Vec<FrameDrawItem> = (0..6_u64)
            .map(|index| {
                FrameDrawItem::new(index, index, 0, [0.0; 16], [0.0; 16], [1.0; 4], false)
                    .with_surface_program(100 + index % 2)
            })
            .collect();
        let (mixed_batches, mixed_programs) =
            frame_packet_to_batches(&packet(mixed, Vec::new()), &SurfaceProgramSet::default());
        assert_eq!(mixed_batches.len(), 6);
        assert_eq!(pipeline_switches(&mixed_programs), 2);
    }

    #[test]
    fn lights_flatten_to_the_live_tuple_shape_in_order() {
        let lights = vec![
            FrameLight::new(0, [-0.3, 1.0, -0.4], [1.0, 1.0, 1.0, 1.0]),
            FrameLight::new(1, [2.0, 3.0, -4.0], [1.0, 0.0, 0.0, 2.5]),
        ];
        let out = frame_packet_lights(&packet(Vec::new(), lights));
        assert_eq!(
            out,
            vec![
                (0_u32, [-0.3, 1.0, -0.4], [1.0, 1.0, 1.0], 1.0),
                (1_u32, [2.0, 3.0, -4.0], [1.0, 0.0, 0.0], 2.5),
            ]
        );
    }
}
