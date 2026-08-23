//! A material description an app adds to an `Assets<Material>` collection.

use axiom_host::TextureSampling;
use axiom_kernel::Ratio;
use axiom_surface::Surface;

use crate::color::Color;
use crate::texture::Texture;

/// A const `Ratio` from a literal, built in const context. The `match` lives in a
/// macro expansion, so the branchless lint skips it and the fallible conversion
/// never runs at runtime — the same shape as `color::unit!`.
macro_rules! ratio_lit {
    ($value:expr) => {{
        const R: Ratio = match Ratio::new($value) {
            Ok(r) => r,
            Err(_) => panic!("material ratio literal is finite"),
        };
        R
    }};
}

/// A material an app registers with the engine.
/// The engine provides the built-in basic-lit material: a base [`Color`], an
/// optional albedo [`Texture`], and the catalog scalar fields the contract names
/// — `emissive` (self-illumination), `roughness` (`0` mirror-smooth … `1` matte),
/// and `opacity` (`1` opaque; blends only once SPEC-04 lands the alpha path). A
/// `Material` value is a *description*; the engine resolves it into real material
/// data when the app runs. The final surface colour is the sampled albedo × the
/// base colour × the per-vertex colour, plus the emissive term.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    base_color: Color,
    texture: Option<Texture>,
    emissive: Color,
    roughness: Ratio,
    opacity: Ratio,
    /// An app-authored raw-pixel albedo texture id (0 = none). Unlike `texture`
    /// (the built-in procedural [`Texture`] enum), this references RGBA8 pixels the
    /// app registered via `RunningApp::add_texture_data`. Kept a scalar so
    /// `Material` stays `Copy`.
    custom_texture: u64,
    /// How this material's texture must be filtered as it minifies. See
    /// [`TextureSampling`].
    texture_sampling: TextureSampling,
    /// How metallic the surface is (`0` dielectric … `1` metal). A *channel*,
    /// not a BRDF: carried and reported, read by no lighting model yet —
    /// exactly as `opacity` was carried before the alpha path landed, and for
    /// the same reason (the vocabulary lands before the shading that spends it).
    metallic: Ratio,
    /// The **appearance program** this material names — the content digest of an
    /// authored [`Surface`], or `0` for the engine's built-in fixed material
    /// path. A `u64` and not a `Surface`, because a `Surface` owns graphs and a
    /// `Vec` while a `Material` is a `Copy` per-asset *description*; a surface
    /// is preparation-time data, addressed by identity afterwards. See
    /// [`Material::from_surface`].
    surface_program: u64,
    /// The four **non-albedo** maps the runtime material shader binds beside the
    /// albedo, each an id into the *same* `RunningApp::add_texture_data` store
    /// `custom_texture` reads (`0` = none, so the backend binds its neutral).
    ///
    /// One store and not five. A map is RGBA8 pixels registered at runtime, which
    /// is exactly what that store holds; a second registration API would have been
    /// a parallel lane carrying the identical payload, differing only in which
    /// slot the material later names it in — and *which slot* is a property of the
    /// material, not of the pixels. Keeping them scalars is also what keeps
    /// `Material` `Copy`, for the same reason `custom_texture` is one.
    ///
    /// Tangent-space normal, RGB, linear.
    normal_texture: u64,
    /// `(occlusion, roughness, metalness, height)`, linear.
    orm_texture: u64,
    /// The micro-detail tile. Linear; the channel packing is the backend's, and
    /// is documented at `axiom_host::MaterialTexture::detail`.
    detail_texture: u64,
    /// The macro variation field. Linear; the backend's neutral is mid-grey, not
    /// zero, because the layer is a variation *around* a midpoint.
    macro_texture: u64,
}

impl Material {
    /// A basic-lit material with the given linear base colour, no texture, no
    /// emissive, fully matte, and fully opaque.
    pub const fn lit(base_color: Color) -> Self {
        Material {
            base_color,
            texture: None,
            emissive: Color::BLACK,
            roughness: ratio_lit!(1.0),
            opacity: ratio_lit!(1.0),
            custom_texture: 0,
            texture_sampling: TextureSampling::Crisp,
            metallic: ratio_lit!(0.0),
            surface_program: 0,
            normal_texture: 0,
            orm_texture: 0,
            detail_texture: 0,
            macro_texture: 0,
        }
    }

    /// A material whose appearance is the authored `surface` — its channels, its
    /// layering and its lighting model — rather than the built-in fixed material
    /// path.
    ///
    /// The surface is reduced here, once, to its **content digest**
    /// ([`Surface::digest`]): a structural hash that two independently-authored
    /// but identical surfaces share, and that a parameter retune deliberately
    /// does *not* move. That number is what travels the render chain, so the
    /// engine dedupes equal appearances for free and a material tweak cannot
    /// invalidate a compiled program.
    ///
    /// Everything else starts where [`Material::lit`] starts: white, untextured,
    /// matte, opaque. The catalog builders still apply, so a surface-backed
    /// material can carry a texture and an emissive exactly like any other.
    pub fn from_surface(surface: Surface) -> Self {
        Material::lit(Color::WHITE).with_surface_program(surface.param_key().raw())
    }

    /// This material's own colour and textures, driven by `surface`'s program.
    ///
    /// [`Material::from_surface`] starts from white, which is right when the
    /// surface authors the whole appearance. A **runtime material** does not: it
    /// modulates the albedo it is handed — the source's shader multiplies its
    /// tint and its macro variation into `diffuseColor`, it does not replace it
    /// — so an app that has already chosen a per-batch colour needs to keep it.
    ///
    /// The surface still reduces to its content digest, so two batches naming
    /// the same surface with different colours are still one program and one
    /// pipeline; only the instance colour lane differs.
    pub fn with_surface(self, surface: Surface) -> Self {
        self.with_surface_program(surface.param_key().raw())
    }

    /// This material with an explicit appearance program id. Private because the
    /// number is only ever a [`Surface`]'s digest — an app names a surface, never
    /// a raw program id.
    const fn with_surface_program(mut self, surface_program: u64) -> Self {
        self.surface_program = surface_program;
        self
    }

    /// This material's metallic-ness (`0` dielectric … `1` metal).
    ///
    /// Carried now and read by no lighting model yet — the engine's shading is
    /// Lambert plus one specular term, and this is a *channel*, not a new BRDF.
    /// It is authorable here so material descriptions and
    /// `axiom_surface::SurfaceChannel::Metallic` name the same axis.
    pub const fn with_metallic(mut self, metallic: Ratio) -> Self {
        self.metallic = metallic;
        self
    }

    /// This material with an albedo [`Texture`] attached (sampled × base colour).
    pub const fn with_texture(mut self, texture: Texture) -> Self {
        self.texture = Some(texture);
        self
    }

    /// This material with an app-authored raw-pixel albedo texture attached, by the
    /// `id` from `RunningApp::add_texture_data` (sampled × base colour). `0` clears
    /// it. Takes precedence over the built-in [`Texture`] when both are set.
    pub const fn with_custom_texture(mut self, id: u64) -> Self {
        self.custom_texture = id;
        self
    }

    /// This material with a **tangent-space normal map** attached, by the `id`
    /// from `RunningApp::add_texture_data` (`0` clears it, and the backend then
    /// binds a flat `+Z` normal).
    ///
    /// This is the map that used to reach only the off-screen renderer, through a
    /// slice parallel to the material set that the live browser arm passed empty.
    /// Naming it on the material is what gives the browser one at all.
    pub const fn with_normal_texture(mut self, id: u64) -> Self {
        self.normal_texture = id;
        self
    }

    /// This material with an **`(occlusion, roughness, metalness, height)`** map
    /// attached, by the `id` from `RunningApp::add_texture_data` (`0` clears it).
    /// Its alpha channel is the height the shader's parallax-occlusion layer
    /// marches; without it that layer is inert.
    pub const fn with_orm_texture(mut self, id: u64) -> Self {
        self.orm_texture = id;
        self
    }

    /// This material with a **micro-detail tile** attached, by the `id` from
    /// `RunningApp::add_texture_data` (`0` clears it). Feeds the shader's
    /// micro-detail layer; the channel packing is documented at
    /// `axiom_host::MaterialTexture::detail`.
    pub const fn with_detail_texture(mut self, id: u64) -> Self {
        self.detail_texture = id;
        self
    }

    /// This material with a **macro variation field** attached, by the `id` from
    /// `RunningApp::add_texture_data` (`0` clears it). Feeds the shader's
    /// de-tiling and macro-variation layers; without it both are inert.
    pub const fn with_macro_texture(mut self, id: u64) -> Self {
        self.macro_texture = id;
        self
    }

    /// This material with a self-illumination (emissive) colour added on top of
    /// the lit result.
    pub const fn with_emissive(mut self, emissive: Color) -> Self {
        self.emissive = emissive;
        self
    }

    /// This material with an explicit texture sampling mode.
    ///
    /// Reach for [`TextureSampling::Anisotropic`] when the surface is seen at a
    /// grazing angle across a wide depth range — a road, a floor, a terrain. Those
    /// are minified hard along one screen axis while staying near 1:1 along the
    /// other, and the default trilinear filter picks its mip level from the larger
    /// of the two, so it blurs away lateral detail the pixel grid could still
    /// resolve. It is opt-in rather than automatic because anisotropy forces
    /// linear magnification, which would smooth the hard texels that are the
    /// engine's look everywhere else.
    pub const fn with_texture_sampling(mut self, sampling: TextureSampling) -> Self {
        self.texture_sampling = sampling;
        self
    }

    /// This material with a surface roughness (`0` = mirror-smooth, `1` = matte).
    pub const fn with_roughness(mut self, roughness: Ratio) -> Self {
        self.roughness = roughness;
        self
    }

    /// This material with an opacity (`1` = opaque). Carried now; visually blends
    /// only after SPEC-04 lands the alpha-blend path.
    pub const fn with_opacity(mut self, opacity: Ratio) -> Self {
        self.opacity = opacity;
        self
    }

    /// The material's base colour.
    pub const fn base_color(self) -> Color {
        self.base_color
    }

    /// The material's albedo texture, if any.
    pub const fn texture(self) -> Option<Texture> {
        self.texture
    }

    /// The material's app-authored raw-pixel albedo texture id (0 = none).
    pub const fn custom_texture(self) -> u64 {
        self.custom_texture
    }

    /// The material's tangent-space normal-map texture id (0 = none).
    pub const fn normal_texture(self) -> u64 {
        self.normal_texture
    }

    /// The material's `(occlusion, roughness, metalness, height)` texture id
    /// (0 = none).
    pub const fn orm_texture(self) -> u64 {
        self.orm_texture
    }

    /// The material's micro-detail tile texture id (0 = none).
    pub const fn detail_texture(self) -> u64 {
        self.detail_texture
    }

    /// The material's macro variation field texture id (0 = none).
    pub const fn macro_texture(self) -> u64 {
        self.macro_texture
    }

    /// How this material's texture is filtered as it minifies.
    pub const fn texture_sampling(self) -> TextureSampling {
        self.texture_sampling
    }

    /// The material's emissive (self-illumination) colour.
    pub const fn emissive(self) -> Color {
        self.emissive
    }

    /// The material's surface roughness.
    pub const fn roughness(self) -> Ratio {
        self.roughness
    }

    /// The material's opacity.
    pub const fn opacity(self) -> Ratio {
        self.opacity
    }

    /// The material's metallic-ness (`0` dielectric … `1` metal).
    pub const fn metallic(self) -> Ratio {
        self.metallic
    }

    /// The appearance program this material names — an authored [`Surface`]'s
    /// content digest, or `0` for the engine's built-in fixed material path.
    pub const fn surface_program(self) -> u64 {
        self.surface_program
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_carries_its_base_color() {
        use axiom_kernel::Ratio;
        let red = || {
            Color::linear_rgb(
                Ratio::new(0.8).expect("authored colour channel is finite"),
                Ratio::new(0.2).expect("authored colour channel is finite"),
                Ratio::new(0.2).expect("authored colour channel is finite"),
            )
        };
        let m = Material::lit(red());
        assert_eq!(m.base_color(), red());
        assert_eq!(m.texture(), None);
    }

    #[test]
    fn with_texture_attaches_an_albedo() {
        let m = Material::lit(Color::WHITE).with_texture(Texture::Checker);
        assert_eq!(m.texture(), Some(Texture::Checker));
        assert_eq!(m.base_color(), Color::WHITE);
    }

    #[test]
    fn with_custom_texture_attaches_a_raw_pixel_id() {
        let m = Material::lit(Color::WHITE);
        assert_eq!(m.custom_texture(), 0, "default is no custom texture");
        let textured = m.with_custom_texture(7);
        assert_eq!(textured.custom_texture(), 7);
        assert_ne!(textured, m, "the custom-texture id is part of equality");
    }

    /// The four non-albedo maps default to "none", each setter fills exactly its
    /// own slot, and all five texture ids stay independent. A swapped pair here
    /// would light a surface with its own occlusion pack — silently, since every
    /// slot is a `u64`.
    #[test]
    fn the_four_map_ids_default_to_none_and_never_cross() {
        let m = Material::lit(Color::WHITE);
        assert_eq!(
            (
                m.custom_texture(),
                m.normal_texture(),
                m.orm_texture(),
                m.detail_texture(),
                m.macro_texture()
            ),
            (0, 0, 0, 0, 0),
            "a material authors no maps unless asked"
        );
        let mapped = m
            .with_custom_texture(1)
            .with_normal_texture(2)
            .with_orm_texture(3)
            .with_detail_texture(4)
            .with_macro_texture(5);
        assert_eq!(
            (
                mapped.custom_texture(),
                mapped.normal_texture(),
                mapped.orm_texture(),
                mapped.detail_texture(),
                mapped.macro_texture()
            ),
            (1, 2, 3, 4, 5)
        );
        // Every slot is part of identity, so two materials differing only in
        // which normal map they name are two materials.
        assert_ne!(mapped, m);
        assert_ne!(
            m.with_normal_texture(2),
            m.with_normal_texture(3),
            "the normal-map id is part of equality"
        );
        // Clearing is `0`, and it clears only the slot named.
        let cleared = mapped.with_orm_texture(0);
        assert_eq!(cleared.orm_texture(), 0);
        assert_eq!(cleared.normal_texture(), 2);
        assert_eq!(cleared.macro_texture(), 5);
        // The maps are orthogonal to everything else a material carries.
        assert_eq!(mapped.base_color(), m.base_color());
        assert_eq!(mapped.roughness(), m.roughness());
        assert_eq!(mapped.surface_program(), m.surface_program());
    }

    /// `Material` is `Copy`, and the four ids are scalars precisely so it stays
    /// that way. A `Vec` or an `Option<MapPixels>` here would have taken it away
    /// from every app that holds materials by value.
    #[test]
    fn a_material_carrying_every_map_is_still_copy() {
        fn takes_copy<T: Copy>(value: T) -> (T, T) {
            (value, value)
        }
        let m = Material::lit(Color::WHITE)
            .with_normal_texture(2)
            .with_orm_texture(3)
            .with_detail_texture(4)
            .with_macro_texture(5);
        let (a, b) = takes_copy(m);
        assert_eq!(a, b);
        assert_eq!(a.macro_texture(), 5);
    }

    #[test]
    fn lit_defaults_the_catalog_fields() {
        let m = Material::lit(Color::WHITE);
        assert_eq!(m.emissive(), Color::BLACK);
        assert_eq!(m.roughness().get(), 1.0);
        assert_eq!(m.opacity().get(), 1.0);
        assert_eq!(m.metallic().get(), 0.0);
    }

    /// The compatibility invariant of the whole surface chain: a material an app
    /// authored the way every existing app authors one takes the built-in fixed
    /// material path, in one line, with no graph anywhere near it.
    #[test]
    fn a_material_with_no_surface_names_the_builtin_program() {
        assert_eq!(Material::lit(Color::WHITE).surface_program(), 0);
        assert_eq!(
            Material::lit(Color::WHITE)
                .with_texture(Texture::Checker)
                .surface_program(),
            0,
            "a built-in procedural texture is not a surface program"
        );
    }

    /// A surface-backed material carries that surface's *content* digest, so two
    /// materials authored from equal surfaces name one program and two different
    /// surfaces name two.
    #[test]
    fn from_surface_carries_the_surfaces_digest() {
        let default_surface = || {
            axiom_surface::SurfaceBuilder::new()
                .build()
                .expect("a default surface is legal")
        };
        let unlit_surface = || {
            axiom_surface::SurfaceBuilder::new()
                .lighting(axiom_surface::LightingModel::Unlit)
                .build()
                .expect("an unlit surface is legal")
        };

        let a = Material::from_surface(default_surface());
        let b = Material::from_surface(default_surface());
        let c = Material::from_surface(unlit_surface());

        assert_eq!(a.surface_program(), default_surface().digest().raw());
        assert_ne!(a.surface_program(), 0, "an authored surface is not the built-in path");
        assert_eq!(
            a.surface_program(),
            b.surface_program(),
            "equal surfaces authored independently collapse to one program"
        );
        assert_ne!(
            a.surface_program(),
            c.surface_program(),
            "a structural difference is a different program"
        );
        assert_eq!(a, b);
        assert_ne!(a, c, "the program is part of a material's identity");

        // Everything else starts where `lit` starts, and the catalog builders
        // still compose on top.
        assert_eq!(a.base_color(), Color::WHITE);
        assert_eq!(a.texture(), None);
        assert_eq!(a.roughness().get(), 1.0);
        assert_eq!(a.opacity().get(), 1.0);
        let dressed = a.with_texture(Texture::Checker).with_emissive(Color::WHITE);
        assert_eq!(dressed.surface_program(), a.surface_program());
        assert_eq!(dressed.texture(), Some(Texture::Checker));
    }

    #[test]
    fn with_metallic_round_trips_and_is_part_of_identity() {
        let m = Material::lit(Color::WHITE);
        let metal = m.with_metallic(Ratio::new(1.0).expect("finite"));
        assert_eq!(metal.metallic().get(), 1.0);
        assert_eq!(metal.base_color(), m.base_color());
        assert_eq!(metal.roughness(), m.roughness());
        assert_ne!(metal, m);
    }

    /// A material filters its texture the default way unless it asks otherwise.
    /// The default is what every existing app relies on, so it is pinned here as
    /// well as at the host type.
    #[test]
    fn texture_sampling_defaults_to_crisp_and_is_opt_in() {
        let m = Material::lit(Color::WHITE);
        assert_eq!(m.texture_sampling(), TextureSampling::Crisp);
        let ground = m.with_texture_sampling(TextureSampling::Anisotropic);
        assert_eq!(ground.texture_sampling(), TextureSampling::Anisotropic);
        assert_ne!(ground, m, "the sampling mode is part of equality");
        // And it is orthogonal to everything else the builder carries.
        assert_eq!(ground.base_color(), m.base_color());
        assert_eq!(ground.custom_texture(), m.custom_texture());
    }

    #[test]
    fn catalog_builders_round_trip_distinct_from_defaults() {
        let half = || Ratio::new(0.5).expect("finite");
        let m = Material::lit(Color::WHITE)
            .with_emissive(Color::WHITE)
            .with_roughness(half())
            .with_opacity(half());
        assert_eq!(m.emissive(), Color::WHITE);
        assert_eq!(m.roughness().get(), 0.5);
        assert_eq!(m.opacity().get(), 0.5);
        // Equality requires every field: a differing roughness breaks it.
        let other = Material::lit(Color::WHITE)
            .with_emissive(Color::WHITE)
            .with_opacity(half());
        assert_ne!(m, other);
    }
}
