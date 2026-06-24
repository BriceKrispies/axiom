//! Adapt a backend-neutral [`axiom_host::FramePacket`] into the live GPU path's
//! input.
//!
//! The live `SceneRenderer` consumes per-`(mesh, material)` instance batches and
//! a flat light list (the shape [`crate::GpuBackendApi::present_frame`] takes).
//! This module derives exactly that shape from a `FramePacket`, so the GPU
//! backend presents the shared packet with **no** change to the renderer. The
//! packing layout is byte-identical to the legacy batch format: 36 floats per
//! instance — `mvp[16]`, then `world[16]`, then `colour[4]` — grouped by
//! `(mesh_id, material_id)` in first-appearance order.

use std::collections::HashMap;

use axiom_host::FramePacket;

/// Group a packet's draws into per-`(mesh, material)` instance batches:
/// `(mesh_id, material_id, [mvp(16), world(16), colour(4)] per instance, count)`,
/// one entry per distinct `(mesh, material)` pair in first-appearance order.
/// Byte-identical to the legacy `mesh_batches` layout the live renderer consumes.
pub(crate) fn frame_packet_to_batches(packet: &FramePacket) -> Vec<(u64, u64, Vec<f32>, u32)> {
    let mut order: Vec<(u64, u64)> = Vec::new();
    let mut packed: HashMap<(u64, u64), Vec<f32>> = HashMap::new();
    packet.draws().iter().for_each(|draw| {
        let key = (draw.mesh_id(), draw.material_id());
        let floats = packed.entry(key).or_insert_with(|| {
            order.push(key);
            Vec::new()
        });
        floats.extend_from_slice(&draw.mvp());
        floats.extend_from_slice(&draw.world());
        floats.extend_from_slice(&draw.color());
    });
    order
        .into_iter()
        .map(|(mesh_id, material_id)| {
            let floats = packed.remove(&(mesh_id, material_id)).unwrap_or_default();
            let count = (floats.len() / 36) as u32;
            (mesh_id, material_id, floats, count)
        })
        .collect()
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
            FrameDrawItem::new(2, 7, 5, [7.0; 16], [3.0; 16], [0.7, 0.8, 0.9, 1.0], false),
        ];
        let batches = frame_packet_to_batches(&packet(draws, Vec::new()));

        assert_eq!(batches.len(), 2);
        // First-appearance order: (7,5) first with 2 instances, then (7,6) with 1.
        assert_eq!((batches[0].0, batches[0].1), (7, 5));
        assert_eq!(batches[0].3, 2);
        assert_eq!(batches[0].2.len(), 72); // 2 instances x 36 floats
                                            // Instance 0 = draw 0: mvp, then world, then colour.
        assert_eq!(&batches[0].2[0..16], &[1.0; 16]);
        assert_eq!(&batches[0].2[16..32], &[9.0; 16]);
        assert_eq!(&batches[0].2[32..36], &[0.1, 0.2, 0.3, 1.0]);
        // Instance 1 = draw 2 (same pair).
        assert_eq!(&batches[0].2[36..52], &[3.0; 16]);
        assert_eq!(&batches[0].2[52..68], &[7.0; 16]);
        assert_eq!(&batches[0].2[68..72], &[0.7, 0.8, 0.9, 1.0]);

        assert_eq!((batches[1].0, batches[1].1), (7, 6));
        assert_eq!(batches[1].3, 1);
        assert_eq!(&batches[1].2[0..16], &[2.0; 16]);
        assert_eq!(&batches[1].2[16..32], &[8.0; 16]);
    }

    #[test]
    fn empty_packet_yields_no_batches_and_no_lights() {
        let p = packet(Vec::new(), Vec::new());
        assert!(frame_packet_to_batches(&p).is_empty());
        assert!(frame_packet_lights(&p).is_empty());
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
