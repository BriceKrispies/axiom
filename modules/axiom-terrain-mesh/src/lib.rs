//! # Axiom terrain-mesh — Engine Module
//!
//! Domain-neutral **heightfield-grid meshing**: given a grid config (center,
//! radius, spacing) and a height callback, build a square grid mesh — positions,
//! central-difference normals, and grid triangle indices. This is the reusable
//! geometry core the growth game's far/snapshot terrain mesher grew — lifted out
//! of the app so any app that meshes a heightfield composes the same geometry
//! instead of re-deriving the double grid loop and the normal by hand.
//!
//! - **[`TerrainMeshApi`]** — the facade: [`TerrainMeshApi::heightfield_grid_mesh`]
//!   samples a `Fn(Meters, Meters) -> Meters` height callback over a square grid,
//!   derives the unit central-difference surface normal at each vertex, and builds
//!   the grid triangulation.
//! - **[`GridMesh`]** — the pure value type it hands back: index-aligned
//!   `positions` + `normals` (`Vec3`) and triangle `indices`.
//!
//! ## Neutral geometry, nothing more
//! The module owns *geometry*, never *appearance*. It emits positions, normals,
//! and indices; the caller turns them into whatever vertex layout it renders and
//! decorates them with colour, UVs, or world semantics. The module never sees a
//! colour, a UV, or a world meaning — which is exactly why it is reusable across
//! unrelated worlds. Naked floats stay off the boundary: coordinates and heights
//! are [`Meters`](axiom_kernel::Meters); positions and normals are
//! [`Vec3`](axiom_math::Vec3); the `impl Fn` height callback carries the rest.
//!
//! ## Determinism
//! The mesh is a pure function of `(center, radius, spacing, height)`: the same
//! inputs produce byte-identical positions, normals, and indices run-to-run. No
//! tick, RNG, or wall-clock reaches the mesher.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one behavioral facade** — [`TerrainMeshApi`] — plus
//! the pure value-type vocabulary it returns ([`GridMesh`]).

mod ids;
mod terrain_mesh_api;

pub use ids::GridMesh;
pub use terrain_mesh_api::TerrainMeshApi;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;

    /// A `Meters` from a finite `f32` (test-only convenience over the kernel ctor).
    fn m(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

    /// A flat height field: every height is the same constant.
    fn flat(_x: Meters, _z: Meters) -> Meters {
        Meters::finite_or_zero(5.0)
    }

    #[test]
    fn vertex_and_index_counts_match_the_grid_side() {
        // side = ceil(2*radius/spacing) + 1 = ceil(4/1) + 1 = 5.
        let mesh = TerrainMeshApi::heightfield_grid_mesh((m(0.0), m(0.0)), m(2.0), m(1.0), flat);
        let side = 5usize;
        assert_eq!(mesh.positions().len(), side * side);
        assert_eq!(mesh.normals().len(), side * side);
        assert_eq!(mesh.indices().len(), (side - 1) * (side - 1) * 6);
    }

    #[test]
    fn positions_span_the_grid_centred_on_center() {
        let mesh = TerrainMeshApi::heightfield_grid_mesh((m(10.0), m(20.0)), m(2.0), m(1.0), flat);
        let positions = mesh.positions();
        // First vertex is the (-radius, -radius) corner of the centred grid.
        assert_eq!(positions[0].x, 8.0);
        assert_eq!(positions[0].z, 18.0);
        // Row-major: index 1 steps +spacing in x; index `side` steps +spacing in z.
        assert_eq!(positions[1].x, 9.0);
        assert_eq!(positions[1].z, 18.0);
        assert_eq!(positions[5].x, 8.0);
        assert_eq!(positions[5].z, 19.0);
        // Every vertex carries the sampled height at its y.
        assert_eq!(positions[0].y, 5.0);
    }

    #[test]
    fn flat_field_normals_all_point_up() {
        let mesh = TerrainMeshApi::heightfield_grid_mesh((m(0.0), m(0.0)), m(2.0), m(1.0), flat);
        mesh.normals().iter().for_each(|n| {
            assert_eq!(n.x, 0.0);
            assert_eq!(n.y, 1.0);
            assert_eq!(n.z, 0.0);
        });
    }

    #[test]
    fn uphill_in_x_tilts_the_normal_toward_negative_x() {
        // Height rises toward +x: h = x. Central difference gives nx < 0 (the
        // normal leans away from the uphill direction), and nz = 0 (flat in z).
        let mesh =
            TerrainMeshApi::heightfield_grid_mesh((m(0.0), m(0.0)), m(2.0), m(1.0), |x, _z| x);
        let n = mesh.normals()[0];
        assert!(
            n.x < 0.0,
            "normal x should be negative on a +x uphill, got {}",
            n.x
        );
        assert_eq!(n.z, 0.0);
        assert!(n.y > 0.0);
        // Unit length.
        let len2 = n.x * n.x + n.y * n.y + n.z * n.z;
        assert!(
            (len2 - 1.0).abs() < 1.0e-5,
            "normal must be unit, len² = {len2}"
        );
    }

    #[test]
    fn uphill_in_z_tilts_the_normal_toward_negative_z() {
        // Height rises toward +z: h = z. Symmetric to the x case.
        let mesh =
            TerrainMeshApi::heightfield_grid_mesh((m(0.0), m(0.0)), m(2.0), m(1.0), |_x, z| z);
        let n = mesh.normals()[0];
        assert!(
            n.z < 0.0,
            "normal z should be negative on a +z uphill, got {}",
            n.z
        );
        assert_eq!(n.x, 0.0);
    }

    #[test]
    fn indices_wind_two_triangles_per_cell() {
        let mesh = TerrainMeshApi::heightfield_grid_mesh((m(0.0), m(0.0)), m(1.0), m(1.0), flat);
        // side = ceil(2/1)+1 = 3. First cell (ix=0, jz=0): i0=0,i1=1,i2=3,i3=4.
        assert_eq!(&mesh.indices()[0..6], &[0u32, 3, 1, 1, 3, 4]);
    }

    #[test]
    fn same_inputs_produce_byte_identical_meshes() {
        let a = TerrainMeshApi::heightfield_grid_mesh((m(3.0), m(-4.0)), m(5.0), m(2.0), |x, z| {
            Meters::finite_or_zero(x.get().sin() + z.get())
        });
        let b = TerrainMeshApi::heightfield_grid_mesh((m(3.0), m(-4.0)), m(5.0), m(2.0), |x, z| {
            Meters::finite_or_zero(x.get().sin() + z.get())
        });
        assert_eq!(a, b);
    }

    #[test]
    fn grid_mesh_accessors_are_index_aligned() {
        let mesh = TerrainMeshApi::heightfield_grid_mesh((m(0.0), m(0.0)), m(1.0), m(1.0), flat);
        // positions and normals share one length; indices reference into them.
        assert_eq!(mesh.positions().len(), mesh.normals().len());
        let max_index = mesh.indices().iter().copied().max().unwrap();
        assert!((max_index as usize) < mesh.positions().len());
    }

    #[test]
    fn rect_mesh_has_independent_x_and_z_sides_and_counts() {
        // half (6, 2) with spacing (2, 1): side_x = ceil(12/2)+1 = 7, side_z = ceil(4/1)+1 = 5.
        let mesh = TerrainMeshApi::heightfield_grid_mesh_rect(
            (m(0.0), m(0.0)),
            (m(6.0), m(2.0)),
            (m(2.0), m(1.0)),
            flat,
        );
        let (sx, sz) = (7usize, 5usize);
        assert_eq!(mesh.positions().len(), sx * sz);
        assert_eq!(mesh.normals().len(), sx * sz);
        assert_eq!(mesh.indices().len(), (sx - 1) * (sz - 1) * 6);
        // First vertex is the (−half_x, −half_z) corner; index 1 steps +spacing_x,
        // index side_x steps +spacing_z.
        let p = mesh.positions();
        assert_eq!((p[0].x, p[0].z), (-6.0, -2.0));
        assert_eq!((p[1].x, p[1].z), (-4.0, -2.0));
        assert_eq!((p[sx].x, p[sx].z), (-6.0, -1.0));
        // Flat field → all normals up.
        assert!(mesh
            .normals()
            .iter()
            .all(|n| (n.y - 1.0).abs() < 1.0e-6 && n.x == 0.0 && n.z == 0.0));
    }

    #[test]
    fn rect_mesh_tilts_its_normal_on_a_sloped_profile_and_is_unit() {
        // Height rises toward +x with a slope, constant in z (a shallow ramp).
        let mesh = TerrainMeshApi::heightfield_grid_mesh_rect(
            (m(0.0), m(0.0)),
            (m(4.0), m(1.0)),
            (m(1.0), m(1.0)),
            |x, _z| Meters::finite_or_zero(0.3 * x.get()),
        );
        let n = mesh.normals()[0];
        assert!(n.x < 0.0, "normal leans −x on a +x ramp, got {}", n.x);
        assert!(n.z.abs() < 1.0e-6, "flat in z");
        assert!(
            (n.x * n.x + n.y * n.y + n.z * n.z - 1.0).abs() < 1.0e-5,
            "unit normal"
        );
    }
}
