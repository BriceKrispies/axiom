//! The **shallow half-pipe track segment** — the reusable-within-Gravix track
//! piece. One [`HalfPipeParams`] value drives a single **height function**
//! `height_at(local_x)` (a shallow-U cross section with raised side lips, constant
//! along the segment's length), and that one function produces *both*:
//!
//! - the **physics collision grid** ([`HalfPipeGrid`]) fed to
//!   `PhysicsApi::attach_heightfield_collider`, and
//! - the **visual surface mesh** ([`surface_mesh`](HalfPipeParams::surface_mesh))
//!   built by `axiom_terrain_mesh::TerrainMeshApi::heightfield_grid_mesh_rect`.
//!
//! Because both come from the same samples on the same grid, the playable surface
//! and the collision surface are one source of truth — the ball rolls where the
//! track looks. The segment is generated flat/level in its local frame; a
//! downhill slope and heading come from the world `Transform` the course places it
//! with (see `course.rs`). Deterministic: pure functions of the params.

use axiom::prelude::Vec3;
use axiom_kernel::Meters;
use axiom_terrain_mesh::{GridMesh, TerrainMeshApi};

use crate::settings;

/// The parameters of one shallow half-pipe segment. All lengths are world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HalfPipeParams {
    /// Length of the segment, along its local `+z` run direction.
    pub length: f32,
    /// Full width of the channel, across its local `x` (the roll/bank axis).
    pub width: f32,
    /// Shallow-U depth: how much higher the channel edge sits than its centre.
    pub curve_depth: f32,
    /// Extra raised lip height at the very edge (containment).
    pub lip_height: f32,
    /// Fraction of the half-width (0..1) at which the lip begins to rise.
    pub lip_start: f32,
    /// Grid vertex spacing across the width.
    pub tess_width: f32,
    /// Grid vertex spacing along the length.
    pub tess_length: f32,
}

/// A generated collision grid: the row-major heights (`heights[iz*nx + ix]`) plus
/// the grid dimensions the heightfield collider needs.
#[derive(Debug, Clone, PartialEq)]
pub struct HalfPipeGrid {
    pub nx: u32,
    pub nz: u32,
    pub spacing_x: f32,
    pub spacing_z: f32,
    pub heights: Vec<f32>,
}

impl HalfPipeParams {
    /// A straight segment of `length`, taking the channel shape + tessellation from
    /// the game [`settings`].
    pub fn straight(length: f32) -> Self {
        HalfPipeParams {
            length,
            width: settings::HALFPIPE_WIDTH,
            curve_depth: settings::HALFPIPE_CURVE_DEPTH,
            lip_height: settings::HALFPIPE_LIP_HEIGHT,
            lip_start: settings::HALFPIPE_LIP_START,
            tess_width: settings::HALFPIPE_TESS_WIDTH,
            tess_length: settings::HALFPIPE_TESS_LENGTH,
        }
    }

    /// The grid dimensions `(nx, nz, half_x, half_z)`: the vertex counts across /
    /// along, and the actual half-extents the grid spans (a whole number of cells).
    pub fn grid_dims(&self) -> (u32, u32, f32, f32) {
        let nx = (self.width / self.tess_width).ceil().max(1.0) as u32 + 1;
        let nz = (self.length / self.tess_length).ceil().max(1.0) as u32 + 1;
        let half_x = (nx - 1) as f32 * self.tess_width * 0.5;
        let half_z = (nz - 1) as f32 * self.tess_length * 0.5;
        (nx, nz, half_x, half_z)
    }

    /// The surface height at local cross-position `x` (centre `x = 0`): a shallow
    /// parabola `curve_depth·(x/half_x)²` plus a steep side **lip** past
    /// `lip_start`. Constant along the length, so the segment is a level channel.
    pub fn height_at(&self, x: f32) -> f32 {
        let (_, _, half_x, _) = self.grid_dims();
        let u = (x / half_x.max(1.0e-3)).abs().min(1.0);
        let curve = self.curve_depth * u * u;
        let lip_t = ((u - self.lip_start) / (1.0 - self.lip_start).max(1.0e-3)).clamp(0.0, 1.0);
        let lip = self.lip_height * lip_t * lip_t;
        curve + lip
    }

    /// The physics collision grid: the height function sampled on the whole grid,
    /// centred on the segment origin (matching the heightfield collider's frame).
    pub fn collider_grid(&self) -> HalfPipeGrid {
        let (nx, nz, half_x, _half_z) = self.grid_dims();
        let mut heights = Vec::with_capacity((nx * nz) as usize);
        for _iz in 0..nz {
            for ix in 0..nx {
                let x = ix as f32 * self.tess_width - half_x;
                heights.push(self.height_at(x));
            }
        }
        HalfPipeGrid {
            nx,
            nz,
            spacing_x: self.tess_width,
            spacing_z: self.tess_length,
            heights,
        }
    }

    /// The visual surface mesh, built by the engine's terrain grid mesher from the
    /// same height function — centred on the segment origin, so it lines up with
    /// the collider grid exactly.
    pub fn surface_mesh(&self) -> GridMesh {
        let (_, _, half_x, half_z) = self.grid_dims();
        let params = *self;
        TerrainMeshApi::heightfield_grid_mesh_rect(
            (Meters::finite_or_zero(0.0), Meters::finite_or_zero(0.0)),
            (
                Meters::finite_or_zero(half_x),
                Meters::finite_or_zero(half_z),
            ),
            (
                Meters::finite_or_zero(self.tess_width),
                Meters::finite_or_zero(self.tess_length),
            ),
            move |mx, _mz| Meters::finite_or_zero(params.height_at(mx.get())),
        )
    }

    /// The centre-line world height at a point local to the segment (for placing
    /// markers / spawns on the surface): just the channel-centre height (`0` by
    /// construction, but resolved through the height function for clarity).
    pub fn centre_height(&self) -> f32 {
        self.height_at(0.0)
    }

    /// The half-extents `(half_x, half_z)` the segment spans locally.
    pub fn half_extents(&self) -> (f32, f32) {
        let (_, _, half_x, half_z) = self.grid_dims();
        (half_x, half_z)
    }
}

/// The world-space centre of the top surface of a posed segment: the segment
/// origin lifted by the channel-centre height along the local up axis. Used to sit
/// the ball / markers on the track.
pub fn surface_point(origin: Vec3, up: Vec3, centre_height: f32) -> Vec3 {
    origin.add(up.mul_scalar(centre_height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cross_section_is_a_shallow_u_with_raised_lips() {
        let hp = HalfPipeParams::straight(40.0);
        let (_, _, half_x, _) = hp.grid_dims();
        // Centre is the lowest point; the edge rides higher (the shallow bank), and
        // the very edge (lip) is higher still than mid-bank.
        assert_eq!(hp.height_at(0.0), 0.0);
        let mid = hp.height_at(half_x * 0.5);
        let bank = hp.height_at(half_x * 0.8);
        let edge = hp.height_at(half_x);
        assert!(mid > 0.0 && bank > mid, "the bank rises toward the edge");
        assert!(edge > bank, "the lip rises above the bank");
        // Symmetric across the centre.
        assert!((hp.height_at(half_x * 0.5) - hp.height_at(-half_x * 0.5)).abs() < 1.0e-6);
        // Shallow: the bank rise is a fraction of the width.
        assert!(edge < half_x, "the channel is shallow, not a deep ramp");
    }

    #[test]
    fn the_collider_grid_matches_the_height_function_and_mesh_dimensions() {
        let hp = HalfPipeParams::straight(40.0);
        let (nx, nz, half_x, _) = hp.grid_dims();
        let grid = hp.collider_grid();
        assert_eq!(grid.nx, nx);
        assert_eq!(grid.nz, nz);
        assert_eq!(grid.heights.len(), (nx * nz) as usize);
        // Every stored height equals the height function at that grid column.
        for ix in 0..nx {
            let x = ix as f32 * hp.tess_width - half_x;
            assert!((grid.heights[ix as usize] - hp.height_at(x)).abs() < 1.0e-6);
        }
        // The visual mesh shares the grid's vertex count.
        let mesh = hp.surface_mesh();
        assert_eq!(mesh.positions().len(), (nx * nz) as usize);
        assert_eq!(mesh.indices().len(), ((nx - 1) * (nz - 1) * 6) as usize);
        // A longer segment has more length rows but the same width columns.
        let long = HalfPipeParams::straight(80.0);
        assert_eq!(long.grid_dims().0, nx);
        assert!(long.grid_dims().1 > nz);
    }

    #[test]
    fn surface_point_lifts_the_origin_along_up() {
        let p = surface_point(Vec3::new(1.0, 2.0, 3.0), Vec3::UNIT_Y, 0.5);
        assert_eq!(p, Vec3::new(1.0, 2.5, 3.0));
        assert_eq!(HalfPipeParams::straight(10.0).centre_height(), 0.0);
        assert!(HalfPipeParams::straight(10.0).half_extents().0 > 0.0);
    }
}
