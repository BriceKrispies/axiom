//! [`TerrainMeshApi`] — the one facade: build a heightfield grid mesh.

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::ids::GridMesh;

/// The smallest normal length divisor, so a degenerate (flat) normal never divides
/// by zero. Matches the growth mesher's original `1.0e-6` guard.
const MIN_NORMAL_LEN: f32 = 1.0e-6;

/// The domain-neutral heightfield-meshing facade. Every mesh goes through it;
/// [`GridMesh`] is the value type it returns.
///
/// It knows nothing of colour, UVs, or world semantics — those decorate the
/// returned mesh in the caller. It owns only the geometry: sampling a height
/// callback over a square grid, deriving central-difference normals, and building
/// the grid triangulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainMeshApi;

impl TerrainMeshApi {
    /// Build a square grid mesh centred on `center`, spanning `radius` metres in
    /// each of `±x` / `±z`, with vertices `spacing` metres apart. Each vertex `y`
    /// is `height(x, z)`; its normal is the unit **central-difference** normal of
    /// `height` sampled at the `±spacing` neighbours
    /// (`nx = -(h(x+s)-h(x-s))`, `nz = -(h(z+s)-h(z-s))`, `ny = 2·spacing`,
    /// normalized). Triangles wind `[i0, i2, i1, i1, i2, i3]` per cell.
    ///
    /// The grid side is `ceil(2·radius / spacing) + 1`, so the mesh has `side²`
    /// vertices and `(side-1)² · 2` triangles.
    ///
    /// The `height` callback keeps naked floats off the boundary: it takes and
    /// returns [`Meters`], the dimensioned world coordinates and elevation.
    pub fn heightfield_grid_mesh<H>(
        center: (Meters, Meters),
        radius: Meters,
        spacing: Meters,
        height: H,
    ) -> GridMesh
    where
        H: Fn(Meters, Meters) -> Meters,
    {
        let cx = center.0.get();
        let cz = center.1.get();
        let r = radius.get();
        let s = spacing.get();
        let side = (2.0 * r / s).ceil() as usize + 1;

        // Absolute height at a world point, through the dimensioned callback.
        let sample =
            |x: f32, z: f32| height(Meters::finite_or_zero(x), Meters::finite_or_zero(z)).get();

        let (positions, normals): (Vec<Vec3>, Vec<Vec3>) = (0..side * side)
            .map(|k| {
                let ix = k % side;
                let jz = k / side;
                let x = cx - r + ix as f32 * s;
                let z = cz - r + jz as f32 * s;
                let y = sample(x, z);

                // Central-difference normal from the four `±spacing` neighbours.
                let hx0 = sample(x - s, z);
                let hx1 = sample(x + s, z);
                let hz0 = sample(x, z - s);
                let hz1 = sample(x, z + s);
                let nx = -(hx1 - hx0);
                let nz = -(hz1 - hz0);
                let ny = 2.0 * s;
                let len = (nx * nx + ny * ny + nz * nz).sqrt().max(MIN_NORMAL_LEN);

                (Vec3::new(x, y, z), Vec3::new(nx / len, ny / len, nz / len))
            })
            .unzip();

        let cells = side - 1;
        let indices: Vec<u32> = (0..cells * cells)
            .flat_map(|c| {
                let ix = c % cells;
                let jz = c / cells;
                let i0 = (jz * side + ix) as u32;
                let i1 = i0 + 1;
                let i2 = i0 + side as u32;
                let i3 = i2 + 1;
                // Same winding as the streamed surface quads.
                [i0, i2, i1, i1, i2, i3]
            })
            .collect();

        GridMesh::new(positions, normals, indices)
    }
}
