//! The deterministic result of one engine frame.

use std::collections::HashMap;

/// One drawn object: its wgpu-ready model-view-projection matrix and its
/// world (model) matrix (both column-major, 16 floats), its linear RGBA colour,
/// and the ids of the mesh it draws and the material it uses. The world matrix
/// rides alongside the MVP so the fragment shader can recover each pixel's world
/// position for point-light distance/direction; draws still group into
/// per-`(mesh, material)` instance batches for the matching albedo texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawData {
    mvp: [f32; 16],
    world: [f32; 16],
    color: [f32; 4],
    mesh_id: u64,
    material_id: u64,
}

impl DrawData {
    pub(crate) const fn new(
        mvp: [f32; 16],
        world: [f32; 16],
        color: [f32; 4],
        mesh_id: u64,
        material_id: u64,
    ) -> Self {
        DrawData {
            mvp,
            world,
            color,
            mesh_id,
            material_id,
        }
    }

    /// The column-major model-view-projection matrix.
    pub const fn mvp(&self) -> [f32; 16] {
        self.mvp
    }

    /// The column-major world (model) matrix.
    pub const fn world(&self) -> [f32; 16] {
        self.world
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

/// One resolved light for a frame: a kind (`0` directional / `1` point), a
/// world-space geometry vector (to-light direction for directional, world
/// position for point), a linear-RGB colour, and an intensity. Plain data the
/// live backend uploads into its lighting uniform each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightData {
    kind: u32,
    vec: [f32; 3],
    color: [f32; 3],
    intensity: f32,
}

impl LightData {
    pub(crate) const fn new(kind: u32, vec: [f32; 3], color: [f32; 3], intensity: f32) -> Self {
        LightData {
            kind,
            vec,
            color,
            intensity,
        }
    }

    /// `0` = directional, `1` = point.
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// World to-light direction (directional) or world position (point).
    pub const fn vec(&self) -> [f32; 3] {
        self.vec
    }

    /// Linear-RGB colour.
    pub const fn color(&self) -> [f32; 3] {
        self.color
    }

    /// Light intensity (a non-negative scalar multiplier).
    pub const fn intensity(&self) -> f32 {
        self.intensity
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
    lights: Vec<LightData>,
    light_view_proj: [f32; 16],
    presented: bool,
    recorded: bool,
}

impl FrameOutcome {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        tick: u64,
        command_count: usize,
        clear_color: [f32; 4],
        draws: Vec<DrawData>,
        lights: Vec<LightData>,
        light_view_proj: [f32; 16],
        presented: bool,
        recorded: bool,
    ) -> Self {
        FrameOutcome {
            tick,
            command_count,
            clear_color,
            draws,
            lights,
            light_view_proj,
            presented,
            recorded,
        }
    }

    /// The identity matrix as a column-major array (the no-shadow light VP).
    const IDENTITY_MAT4: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    /// A simulation-only outcome (rendering disabled): no commands, no draws, no
    /// lights.
    pub(crate) fn simulation_only(tick: u64, clear_color: [f32; 4]) -> Self {
        FrameOutcome::new(
            tick,
            0,
            clear_color,
            Vec::new(),
            Vec::new(),
            Self::IDENTITY_MAT4,
            false,
            false,
        )
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

    /// The frame's resolved lights, in scene order.
    pub fn lights(&self) -> &[LightData] {
        &self.lights
    }

    /// The directional shadow caster's wgpu-ready light view-projection
    /// (column-major, 16 floats). The live backend renders a shadow map through
    /// this and re-projects fragments into it; identity disables shadows.
    pub fn light_view_proj(&self) -> [f32; 16] {
        self.light_view_proj
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
    /// draw contributes its 16 MVP floats, then its 16 world-matrix floats, then
    /// its 4 colour floats (36 floats per instance), in submission order. The
    /// world matrix lets the shader recover world position for point lighting.
    /// This is the plain data the windowing run loop presents each frame.
    pub fn instance_floats(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.draws.len() * 36);
        self.draws.iter().for_each(|draw| {
            out.extend_from_slice(&draw.mvp);
            out.extend_from_slice(&draw.world);
            out.extend_from_slice(&draw.color);
        });
        out
    }

    /// Group the per-object draws into **per-`(mesh, material)` instance batches**
    /// for the multi-mesh, multi-material live backend: `(mesh_id, material_id,
    /// [mvp(16), world(16), colour(4)] per instance, count)`, one entry per
    /// distinct `(mesh, material)` pair in first-appearance order. This is the
    /// plain data the multi-mesh run loop presents each frame; the backend draws
    /// each batch against the matching uploaded mesh with the material's albedo
    /// bound.
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
            floats.extend_from_slice(&draw.world);
            floats.extend_from_slice(&draw.color);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_floats_pack_mvp_world_then_colour_per_draw() {
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            vec![
                DrawData::new([1.0; 16], [9.0; 16], [0.1, 0.2, 0.3, 1.0], 1, 1),
                DrawData::new([2.0; 16], [8.0; 16], [0.4, 0.5, 0.6, 1.0], 1, 1),
            ],
            Vec::new(),
            [0.0; 16],
            false,
            true,
        );
        let floats = outcome.instance_floats();
        assert_eq!(floats.len(), 72); // 2 draws x (16 mvp + 16 world + 4 colour)
        assert_eq!(&floats[0..16], &[1.0; 16]); // mvp 0
        assert_eq!(&floats[16..32], &[9.0; 16]); // world 0
        assert_eq!(&floats[32..36], &[0.1, 0.2, 0.3, 1.0]); // colour 0
        assert_eq!(&floats[36..52], &[2.0; 16]); // mvp 1
        assert_eq!(&floats[52..68], &[8.0; 16]); // world 1
        assert_eq!(&floats[68..72], &[0.4, 0.5, 0.6, 1.0]); // colour 1
    }

    #[test]
    fn instance_floats_empty_when_no_draws() {
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .instance_floats()
            .is_empty());
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .mesh_batches()
            .is_empty());
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .lights()
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
                DrawData::new([1.0; 16], [9.0; 16], [0.1, 0.2, 0.3, 1.0], 7, 5),
                DrawData::new([2.0; 16], [8.0; 16], [0.4, 0.5, 0.6, 1.0], 7, 6),
                DrawData::new([3.0; 16], [7.0; 16], [0.7, 0.8, 0.9, 1.0], 7, 5),
            ],
            Vec::new(),
            [0.0; 16],
            true,
            false,
        );
        // The per-draw accessors expose mvp/world/colour/mesh id/material id.
        assert_eq!(outcome.draws()[0].mesh_id(), 7);
        assert_eq!(outcome.draws()[0].material_id(), 5);
        assert_eq!(outcome.draws()[1].material_id(), 6);
        assert_eq!(outcome.draws()[0].mvp(), [1.0; 16]);
        assert_eq!(outcome.draws()[0].world(), [9.0; 16]);
        assert_eq!(outcome.draws()[0].color(), [0.1, 0.2, 0.3, 1.0]);

        let batches = outcome.mesh_batches();
        assert_eq!(batches.len(), 2);
        // First-appearance order: (7,5) first (2 instances), then (7,6) (1).
        assert_eq!((batches[0].0, batches[0].1), (7, 5));
        assert_eq!(batches[0].3, 2);
        assert_eq!(batches[0].2.len(), 72); // 2 instances x 36 floats
        assert_eq!(&batches[0].2[0..16], &[1.0; 16]); // draw 0 mvp
        assert_eq!(&batches[0].2[36..52], &[3.0; 16]); // draw 2 mvp
        assert_eq!((batches[1].0, batches[1].1), (7, 6));
        assert_eq!(batches[1].3, 1);
        assert_eq!(&batches[1].2[0..16], &[2.0; 16]);
    }

    #[test]
    fn lights_round_trip_through_the_outcome() {
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            Vec::new(),
            vec![
                LightData::new(0, [-0.3, 1.0, -0.4], [1.0, 1.0, 1.0], 1.0),
                LightData::new(1, [2.0, 3.0, -4.0], [1.0, 0.0, 0.0], 2.5),
            ],
            [5.0; 16],
            false,
            true,
        );
        assert_eq!(outcome.light_view_proj(), [5.0; 16]);
        assert_eq!(outcome.lights().len(), 2);
        assert_eq!(outcome.lights()[0].kind(), 0);
        assert_eq!(outcome.lights()[0].vec(), [-0.3, 1.0, -0.4]);
        assert_eq!(outcome.lights()[1].kind(), 1);
        assert_eq!(outcome.lights()[1].vec(), [2.0, 3.0, -4.0]);
        assert_eq!(outcome.lights()[1].color(), [1.0, 0.0, 0.0]);
        assert_eq!(outcome.lights()[1].intensity(), 2.5);
    }
}
