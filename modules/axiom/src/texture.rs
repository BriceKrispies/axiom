//! A built-in albedo texture an app attaches to a [`crate::material::Material`].
//!
//! Like [`crate::mesh::Mesh`], a `Texture` value is a *description*; the engine
//! resolves it into real RGBA8 pixel data (via `axiom-resources`) when the app
//! runs. The resolution lives in the umbrella because it bridges the umbrella's
//! `Texture` enum to an `axiom-resources` generator — neither module can name the
//! other's contract types, so the composition is the feature module's job.

use axiom_resources::ResourcesApi;

/// A built-in procedural albedo texture. The fieldless discriminant doubles as a
/// stable, nonzero-able id (see [`Self::id`]) and as the index into the resolver
/// table in [`texture_rgba`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Texture {
    /// A two-tone checkerboard (light/dark), tinted by the material base colour.
    Checker,
    /// A `u`→red, `v`→green gradient with white grid lines — the classic UV grid.
    UvGrid,
    /// A 2×2 terrain biome atlas (sand/grass/rock/snow). A terrain mesh samples a
    /// biome by emitting UVs into the matching cell — see [`Self::biome_cell_origin`].
    BiomeAtlas,
}

impl Texture {
    /// A stable, nonzero id for this texture kind. `0` is reserved for
    /// "untextured", so the discriminant is offset by one.
    pub const fn id(self) -> u64 {
        (self as u64) + 1
    }

    /// Resolve this texture to `(width, height, RGBA8 pixels)`. The public form
    /// of [`texture_rgba`] for callers that drive the live backend directly (e.g.
    /// a terrain viewer passing a biome atlas to the streaming run loop).
    pub fn rgba(self) -> (u32, u32, Vec<u8>) {
        texture_rgba(self)
    }

    /// The top-left UV of biome `biome`'s cell in the [`Texture::BiomeAtlas`] (a
    /// 2×2 packing of sand/grass/rock/snow). A terrain vertex tagged with `biome`
    /// samples that biome by offsetting a fractional position within the
    /// `0.5 × 0.5` cell starting here. Out-of-range biome ids wrap into the grid.
    pub fn biome_cell_origin(biome: u32) -> (f32, f32) {
        ResourcesApi::new().biome_atlas_cell_origin(biome)
    }
}

/// Resolve a [`Texture`] description into `(width, height, RGBA8 pixels)` via the
/// matching `axiom-resources` generator. `Texture` is a fieldless enum, so
/// `texture as usize` is its discriminant: index a generator table instead of
/// `match`ing (branchless). The table order must match the variant order
/// (Checker = 0, UvGrid = 1, BiomeAtlas = 2).
pub(crate) fn texture_rgba(texture: Texture) -> (u32, u32, Vec<u8>) {
    let generators: [fn() -> (u32, u32, Vec<u8>); 3] =
        [checker_rgba, uv_grid_rgba, biome_atlas_rgba];
    generators[texture as usize]()
}

/// The built-in checkerboard as resolved RGBA8. The resources table/resolved
/// types are not nameable across the module boundary, so the register→resolve→
/// read-back is inlined here (kept as inferred locals) rather than factored into
/// a shared helper — the same constraint `mesh_geometry` works under.
fn checker_rgba() -> (u32, u32, Vec<u8>) {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources
        .register_checker_texture(
            &mut table,
            "axiom.builtin.checker",
            [235, 235, 235, 255],
            [60, 60, 60, 255],
        )
        .raw();
    let resolved = resources.resolve(&table);
    let width = resources
        .resolved_texture_width(&resolved, id)
        .expect("registered texture present");
    let height = resources
        .resolved_texture_height(&resolved, id)
        .expect("registered texture present");
    let pixels = resources
        .resolved_texture_pixels(&resolved, id)
        .expect("registered texture present")
        .to_vec();
    (width, height, pixels)
}

/// The built-in UV grid as resolved RGBA8. Mirrors [`checker_rgba`] (the
/// per-texture read-back cannot be factored across the un-nameable boundary).
fn uv_grid_rgba() -> (u32, u32, Vec<u8>) {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources
        .register_uv_grid_texture(&mut table, "axiom.builtin.uv_grid")
        .raw();
    let resolved = resources.resolve(&table);
    let width = resources
        .resolved_texture_width(&resolved, id)
        .expect("registered texture present");
    let height = resources
        .resolved_texture_height(&resolved, id)
        .expect("registered texture present");
    let pixels = resources
        .resolved_texture_pixels(&resolved, id)
        .expect("registered texture present")
        .to_vec();
    (width, height, pixels)
}

/// The built-in biome atlas as resolved RGBA8. Mirrors [`checker_rgba`].
fn biome_atlas_rgba() -> (u32, u32, Vec<u8>) {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources
        .register_biome_atlas_texture(&mut table, "axiom.builtin.biome_atlas")
        .raw();
    let resolved = resources.resolve(&table);
    let width = resources
        .resolved_texture_width(&resolved, id)
        .expect("registered texture present");
    let height = resources
        .resolved_texture_height(&resolved, id)
        .expect("registered texture present");
    let pixels = resources
        .resolved_texture_pixels(&resolved, id)
        .expect("registered texture present")
        .to_vec();
    (width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct_and_nonzero() {
        assert_eq!(Texture::Checker.id(), 1);
        assert_eq!(Texture::UvGrid.id(), 2);
        assert_eq!(Texture::BiomeAtlas.id(), 3);
    }

    #[test]
    fn every_texture_resolves_to_well_formed_rgba8() {
        [Texture::Checker, Texture::UvGrid, Texture::BiomeAtlas]
            .into_iter()
            .for_each(|t| {
                // Both the internal resolver and the public `rgba` agree.
                let (w, h, pixels) = texture_rgba(t);
                assert!((w > 0) & (h > 0));
                assert_eq!(pixels.len(), (w * h * 4) as usize);
                assert_eq!(t.rgba(), (w, h, pixels));
            });
    }

    #[test]
    fn resolution_is_deterministic() {
        assert_eq!(texture_rgba(Texture::Checker), texture_rgba(Texture::Checker));
    }

    #[test]
    fn biome_cell_origins_cover_the_atlas_grid() {
        assert_eq!(Texture::biome_cell_origin(0), (0.0, 0.0));
        assert_eq!(Texture::biome_cell_origin(3), (0.5, 0.5));
        // Out-of-range biome ids wrap into the 4-cell grid.
        assert_eq!(Texture::biome_cell_origin(4), Texture::biome_cell_origin(0));
    }
}
