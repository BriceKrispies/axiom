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
        Material::lit(Color::WHITE).with_surface_program(surface.digest().raw())
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
