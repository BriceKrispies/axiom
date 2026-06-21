//! The deterministic result of one engine frame.

use std::collections::HashMap;

/// One drawn object: its wgpu-ready model-view-projection matrix (column-major,
/// 16 floats), its linear RGBA colour, and the ids of the mesh it draws and the
/// material it uses (so draws can be grouped into per-`(mesh, material)` instance
/// batches and bound to the matching albedo texture).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawData {
    mvp: [f32; 16],
    color: [f32; 4],
    mesh_id: u64,
    material_id: u64,
}

impl DrawData {
    pub(crate) const fn new(
        mvp: [f32; 16],
        color: [f32; 4],
        mesh_id: u64,
        material_id: u64,
    ) -> Self {
        DrawData {
            mvp,
            color,
            mesh_id,
            material_id,
        }
    }

    /// The column-major model-view-projection matrix.
    pub const fn mvp(&self) -> [f32; 16] {
        self.mvp
    }

    /// The linear RGBA colour.
    pub const fn color(&self) -> [f32; 4] {
        self.color
    }

    /// The id of the mesh this object draws.
    pub const fn mesh_id(&self) -> u64 {
        self.mesh_id
    }

    /// The id of the material this object uses (selects its albedo texture).
    pub const fn material_id(&self) -> u64 {
        self.material_id
    }
}

/// The deterministic summary of one [`crate::prelude::App`] frame: the tick, the
/// GPU command count, the clear colour, the per-object draw data, and the
/// backend flags. Equal inputs at the same tick produce an equal `FrameOutcome`.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameOutcome {
    tick: u64,
    command_count: usize,
    clear_color: [f32; 4],
    draws: Vec<DrawData>,
    presented: bool,
    recorded: bool,
}

impl FrameOutcome {
    pub(crate) fn new(
        tick: u64,
        command_count: usize,
        clear_color: [f32; 4],
        draws: Vec<DrawData>,
        presented: bool,
        recorded: bool,
    ) -> Self {
        FrameOutcome {
            tick,
            command_count,
            clear_color,
            draws,
            presented,
            recorded,
        }
    }

    /// A simulation-only outcome (rendering disabled): no commands, no draws.
    pub(crate) fn simulation_only(tick: u64, clear_color: [f32; 4]) -> Self {
        FrameOutcome::new(tick, 0, clear_color, Vec::new(), false, false)
    }

    /// The tick this outcome was produced at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// The number of GPU commands the frame submitted.
    pub const fn command_count(&self) -> usize {
        self.command_count
    }

    /// The frame's clear colour.
    pub const fn clear_color(&self) -> [f32; 4] {
        self.clear_color
    }

    /// The per-object draw data, in submission order.
    pub fn draws(&self) -> &[DrawData] {
        &self.draws
    }

    /// Whether the backend presented real pixels.
    pub const fn presented(&self) -> bool {
        self.presented
    }

    /// Whether a recording backend produced this outcome.
    pub const fn recorded(&self) -> bool {
        self.recorded
    }

    /// Pack the per-object draws into the live backend's instance layout: each
    /// draw contributes its 16 MVP floats followed by its 4 colour floats (20
    /// floats per instance), in submission order. This is the plain data the
    /// windowing run loop presents each frame.
    pub fn instance_floats(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.draws.len() * 20);
        self.draws.iter().for_each(|draw| {
            out.extend_from_slice(&draw.mvp);
            out.extend_from_slice(&draw.color);
        });
        out
    }

    /// Group the per-object draws into **per-`(mesh, material)` instance batches**
    /// for the multi-mesh, multi-material live backend: `(mesh_id, material_id,
    /// [mvp(16), colour(4)] per instance, count)`, one entry per distinct
    /// `(mesh, material)` pair in first-appearance order. This is the plain data
    /// the multi-mesh run loop presents each frame; the backend draws each batch
    /// against the matching uploaded mesh with the material's albedo bound.
    pub fn mesh_batches(&self) -> Vec<(u64, u64, Vec<f32>, u32)> {
        let mut order: Vec<(u64, u64)> = Vec::new();
        let mut packed: HashMap<(u64, u64), Vec<f32>> = HashMap::new();
        self.draws.iter().for_each(|draw| {
            let key = (draw.mesh_id, draw.material_id);
            let floats = packed.entry(key).or_insert_with(|| {
                order.push(key);
                Vec::new()
            });
            floats.extend_from_slice(&draw.mvp);
            floats.extend_from_slice(&draw.color);
        });
        order
            .into_iter()
            .map(|(mesh_id, material_id)| {
                let floats = packed.remove(&(mesh_id, material_id)).unwrap_or_default();
                let count = (floats.len() / 20) as u32;
                (mesh_id, material_id, floats, count)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_floats_pack_mvp_then_colour_per_draw() {
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            vec![
                DrawData::new([1.0; 16], [0.1, 0.2, 0.3, 1.0], 1, 1),
                DrawData::new([2.0; 16], [0.4, 0.5, 0.6, 1.0], 1, 1),
            ],
            false,
            true,
        );
        let floats = outcome.instance_floats();
        assert_eq!(floats.len(), 40); // 2 draws x (16 + 4)
        assert_eq!(&floats[0..16], &[1.0; 16]);
        assert_eq!(&floats[16..20], &[0.1, 0.2, 0.3, 1.0]);
        assert_eq!(&floats[20..36], &[2.0; 16]);
        assert_eq!(&floats[36..40], &[0.4, 0.5, 0.6, 1.0]);
    }

    #[test]
    fn instance_floats_empty_when_no_draws() {
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .instance_floats()
            .is_empty());
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .mesh_batches()
            .is_empty());
    }

    #[test]
    fn mesh_batches_group_draws_by_mesh_and_material_in_first_appearance_order() {
        // Same mesh 7, two materials: material 5 (draws 0,2) then material 6
        // (draw 1) — so the (mesh, material) pair, not the mesh alone, keys a
        // batch. A textured and an untextured material on one mesh must not merge.
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            vec![
                DrawData::new([1.0; 16], [0.1, 0.2, 0.3, 1.0], 7, 5),
                DrawData::new([2.0; 16], [0.4, 0.5, 0.6, 1.0], 7, 6),
                DrawData::new([3.0; 16], [0.7, 0.8, 0.9, 1.0], 7, 5),
            ],
            true,
            false,
        );
        // The per-draw accessors expose mvp/colour/mesh id/material id.
        assert_eq!(outcome.draws()[0].mesh_id(), 7);
        assert_eq!(outcome.draws()[0].material_id(), 5);
        assert_eq!(outcome.draws()[1].material_id(), 6);
        assert_eq!(outcome.draws()[0].mvp(), [1.0; 16]);
        assert_eq!(outcome.draws()[0].color(), [0.1, 0.2, 0.3, 1.0]);

        let batches = outcome.mesh_batches();
        assert_eq!(batches.len(), 2);
        // First-appearance order: (7,5) first (2 instances), then (7,6) (1).
        assert_eq!((batches[0].0, batches[0].1), (7, 5));
        assert_eq!(batches[0].3, 2);
        assert_eq!(batches[0].2.len(), 40); // 2 instances x 20 floats
        assert_eq!(&batches[0].2[0..16], &[1.0; 16]);
        assert_eq!(&batches[0].2[20..36], &[3.0; 16]);
        assert_eq!((batches[1].0, batches[1].1), (7, 6));
        assert_eq!(batches[1].3, 1);
        assert_eq!(&batches[1].2[0..16], &[2.0; 16]);
    }
}
