//! A material's albedo pixels, and **how the sampler must read them** — the
//! backend-neutral carrier for the one texture property a backend cannot infer.
//!
//! ## Why the sampling mode has to be carried
//!
//! Everything else about a material texture is derivable at the backend: its
//! extent is in the payload, its colour space follows from the format the backend
//! chose. How it should be *filtered* is not derivable, because it is a statement
//! about how the surface will be **seen**, and only the author knows that.
//!
//! Two surfaces with byte-identical textures need opposite samplers:
//!
//! * a wall, a prop, a UI panel — seen at roughly one texel per pixel — wants
//!   hard, un-smoothed texels, which is the engine's whole look;
//! * a road, a floor, a terrain — seen at a grazing angle, running from under the
//!   camera to the horizon — is minified without bound along one axis while
//!   staying near 1:1 along the other, and wants the sampler to average along the
//!   long axis without blurring the short one.
//!
//! That second case is [`TextureSampling::Anisotropic`], and it cannot be the
//! engine-wide default: anisotropic filtering requires **linear magnification**
//! (a hardware-validation rule, not a preference), which would smooth every
//! magnified texel in every app and delete exactly the look the default exists to
//! protect. So it is authored per material, defaults to
//! [`TextureSampling::Crisp`], and every material that does not ask for it renders
//! exactly as it did before this type existed.
//!
//! ## Why the other four maps ride here too
//!
//! A material is not one image. The runtime material shader binds five: albedo, a
//! tangent-space normal map, an `(occlusion, roughness, metalness, height)` pack,
//! a shared micro-detail tile and a macro variation field. Four of them used to
//! have no lane at all — the normal map travelled beside this type as a bare
//! `&[(u64, u32, u32, Vec<u8>)]` on the off-screen path only (the live browser arm
//! passed an empty slice, so it had no normal maps whatsoever), and the other
//! three had nowhere to come from, which left the shader's parallax-occlusion,
//! de-tiling and micro-detail layers sampling neutral 1x1 placeholders forever.
//!
//! Adding a *second* parallel slice per map would have made five slices threaded
//! through six signatures. Instead each map is an `Option<MapPixels>` **on the
//! carrier**, which is the same argument the tuple made above one level up: the
//! thing that travels is "this material's textures", so that is what gets named.
//! The parallel `normals` slice collapses into this rather than being joined by
//! four more, and every signature it passed through gets *shorter*.
//!
//! A map that is `None` is not an error and not a black texture: the backend binds
//! its own documented neutral for that slot — occlusion 1, metalness 0, height 0,
//! a flat detail normal and a **mid-grey** macro field — each chosen so the
//! shader term it feeds is an identity. A material that authors nothing therefore
//! renders exactly as it did before these fields existed.
//!
//! All four are **linear** data, never sRGB. An ORM triple is three measurements,
//! a tangent-space normal is a direction and a macro field is a noise amplitude;
//! only the albedo is a colour.
//!
//! ## Why the pixels travel as a named value rather than a tuple
//!
//! Material pixels reach a backend at **bind** time, not through the frame packet
//! — they are resident GPU state, not per-frame data — and they used to travel as
//! a bare `(u64, u32, u32, Vec<u8>)` through six signatures across four modules.
//! A fifth positional field on that tuple would have been unreadable at every one
//! of them. Naming the value is what makes the sampling mode legible at the call
//! sites it passes through, and it is why this lives in the `host` layer: it is
//! the one place `axiom`, `axiom-windowing`, `axiom-gpu-backend` and
//! `axiom-canvas2d-backend` can all name a type.

/// How a material's texture must be filtered as it minifies.
///
/// The distinction is only ever visible on **minification** — a magnified texture
/// is drawn from the base level either way, so a material that is never seen small
/// renders identically under both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureSampling {
    /// Hard, un-smoothed texels when magnified; trilinear across the mip chain
    /// when minified. The default, and what every material was before this type
    /// existed.
    ///
    /// The two halves are not in tension. Point-sampling a *magnified* texture is
    /// the engine's deliberate look. Point-sampling a *minified* one is a defect:
    /// a pixel covering many texels gets one arbitrary texel of them, picked by
    /// sub-texel phase, so the surface moirés when still and crawls when the
    /// camera moves.
    #[default]
    Crisp,
    /// Fully linear filtering plus the highest anisotropy the device supports.
    ///
    /// For surfaces seen at a grazing angle across a wide depth range. Trilinear
    /// alone selects its mip level from the *larger* of the two screen-space
    /// derivatives, so on a road — where the along-view footprint can be tens of
    /// times the across-view one — it blurs the across-view axis by that same
    /// factor, wiping out lateral detail that the pixel grid could still resolve
    /// perfectly well. Anisotropic filtering takes several samples along the long
    /// axis instead, which is what keeps such a surface both stable and sharp.
    ///
    /// The cost is that magnification becomes linear for this material — the
    /// hardware requires it — so this is opted into per material rather than
    /// applied everywhere.
    Anisotropic,
}

/// One non-albedo map's texels: its extent and its row-major RGBA8 bytes.
///
/// Deliberately *not* a `MaterialTexture`. A map has no material id (it is
/// already inside the material it belongs to) and no sampling mode (a sampler is
/// a filtering rule, and every map of one material is filtered by the rule that
/// material authored — the backend binds one sampler for all five). What is left
/// is exactly an extent and some bytes, so that is what the type is.
///
/// The channel meaning depends on the slot it is bound to, and is documented at
/// each [`MaterialTexture`] accessor. All of them are **linear** data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPixels {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl MapPixels {
    /// A map of `width * height` row-major RGBA8 texels.
    pub const fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        MapPixels {
            width,
            height,
            pixels,
        }
    }

    /// The map's width in texels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// The map's height in texels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The map's RGBA8 texels, row-major.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// One material's textures: which material they belong to, the albedo's extent
/// and RGBA8 pixels, how they must be sampled, and the four optional non-albedo
/// maps the runtime material shader binds beside the albedo.
///
/// `pixels` is row-major `width * height * 4` bytes. The authoring layer
/// validates that length before a texture id is issued, so a backend receiving
/// this can treat the three as consistent.
///
/// Every one of the four maps defaults to `None`, and a `None` map means "bind
/// your neutral" — see the module docs. That is what makes these fields additive:
/// a producer that names none of them describes exactly the material it described
/// before they existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialTexture {
    material_id: u64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    sampling: TextureSampling,
    normal: Option<MapPixels>,
    orm_height: Option<MapPixels>,
    detail: Option<MapPixels>,
    macro_field: Option<MapPixels>,
}

impl MaterialTexture {
    /// A material texture sampled the default [`TextureSampling::Crisp`] way.
    pub const fn new(material_id: u64, width: u32, height: u32, pixels: Vec<u8>) -> Self {
        MaterialTexture {
            material_id,
            width,
            height,
            pixels,
            sampling: TextureSampling::Crisp,
            normal: None,
            orm_height: None,
            detail: None,
            macro_field: None,
        }
    }

    /// This texture with an explicit sampling mode.
    #[must_use]
    pub fn with_sampling(mut self, sampling: TextureSampling) -> Self {
        self.sampling = sampling;
        self
    }

    /// This material's **tangent-space normal map** (RGB = the normal, linear),
    /// or `None` for the backend's flat `+Z` neutral.
    ///
    /// The four map setters take an `Option` rather than a `MapPixels` because
    /// their one engine caller — `RunningApp::material_textures` — resolves four
    /// texture ids that may each be absent, and a `set-if-present` combinator
    /// around a by-value builder would move the whole carrier four times per
    /// material. An author who has a map writes `Some(map)`; nothing is hidden.
    #[must_use]
    pub fn with_normal(mut self, normal: Option<MapPixels>) -> Self {
        self.normal = normal;
        self
    }

    /// This material's **`(occlusion, roughness, metalness, height)`** pack,
    /// linear, or `None` for the backend's neutral (occlusion 1, metalness 0,
    /// height 0 — each the identity for the term it feeds).
    #[must_use]
    pub fn with_orm_height(mut self, orm_height: Option<MapPixels>) -> Self {
        self.orm_height = orm_height;
        self
    }

    /// This material's **micro-detail tile**, or `None` for the backend's neutral
    /// flat detail normal. See [`MaterialTexture::detail`] for the channel
    /// packing, which is a live question the backend owns.
    #[must_use]
    pub fn with_detail(mut self, detail: Option<MapPixels>) -> Self {
        self.detail = detail;
        self
    }

    /// This material's **macro variation field**, or `None` for the backend's
    /// neutral **mid-grey**. Mid-grey and not zero: the macro layer is a variation
    /// *around* a midpoint, so zero would darken the surface by the full macro
    /// amplitude rather than leaving it alone.
    #[must_use]
    pub fn with_macro_field(mut self, macro_field: Option<MapPixels>) -> Self {
        self.macro_field = macro_field;
        self
    }

    /// The id of the material this texture is the albedo of.
    pub const fn material_id(&self) -> u64 {
        self.material_id
    }

    /// The texture's width in texels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// The texture's height in texels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The texture's RGBA8 texels, row-major.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// How this texture must be filtered as it minifies.
    pub const fn sampling(&self) -> TextureSampling {
        self.sampling
    }

    /// The material's tangent-space normal map, if it authored one.
    pub const fn normal(&self) -> Option<&MapPixels> {
        self.normal.as_ref()
    }

    /// The material's `(occlusion, roughness, metalness, height)` pack, if it
    /// authored one.
    pub const fn orm_height(&self) -> Option<&MapPixels> {
        self.orm_height.as_ref()
    }

    /// The material's micro-detail tile, if it authored one.
    ///
    /// **The channel packing is the backend's, and it is currently in dispute.**
    /// The GPU backend documents binding 5 as `(normal.rgb, height.a)`, but the
    /// source samples *five* scalars through *two* detail textures — the detail
    /// normal's `xyz`, plus a micro-albedo and a micro-height from a second map.
    /// Under the documented packing the shader's micro-albedo term reads the
    /// normal's `x`, which on a near-flat detail normal is ~0.5, so that term
    /// contributes nothing and half the micro layer stays dead even once a real
    /// tile is bound. Resolving it (pack `(normal.xy, micro_albedo, height)`) is a
    /// shader change owned by `axiom-gpu-backend`'s `material_shader`; this
    /// carrier is packing-agnostic and does not pre-empt it.
    pub const fn detail(&self) -> Option<&MapPixels> {
        self.detail.as_ref()
    }

    /// The material's macro variation field, if it authored one.
    pub const fn macro_field(&self) -> Option<&MapPixels> {
        self.macro_field.as_ref()
    }
}

/// The plain `(material_id, width, height, pixels)` shape, sampled the default
/// way. Kept so a producer that has no opinion on filtering — a test fixture, an
/// app building a one-off capture scene — does not have to name a sampling mode
/// it does not care about.
impl From<(u64, u32, u32, Vec<u8>)> for MaterialTexture {
    fn from(value: (u64, u32, u32, Vec<u8>)) -> Self {
        MaterialTexture::new(value.0, value.1, value.2, value.3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_texture_carries_its_material_extent_and_pixels() {
        let t = MaterialTexture::new(7, 2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(t.material_id(), 7);
        assert_eq!(t.width(), 2);
        assert_eq!(t.height(), 1);
        assert_eq!(t.pixels(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    /// The default is the pre-existing behaviour. This is the assertion that
    /// stops a future edit from making anisotropic the default and silently
    /// smoothing every magnified texel in every app.
    #[test]
    fn sampling_defaults_to_crisp() {
        assert_eq!(TextureSampling::default(), TextureSampling::Crisp);
        assert_eq!(
            MaterialTexture::new(1, 1, 1, vec![0; 4]).sampling(),
            TextureSampling::Crisp
        );
        assert_eq!(
            MaterialTexture::from((1, 1, 1, vec![0; 4])).sampling(),
            TextureSampling::Crisp
        );
    }

    #[test]
    fn with_sampling_selects_the_anisotropic_mode_and_keeps_everything_else() {
        let base = MaterialTexture::new(3, 4, 4, vec![9; 64]);
        let aniso = base.clone().with_sampling(TextureSampling::Anisotropic);
        assert_eq!(aniso.sampling(), TextureSampling::Anisotropic);
        assert_eq!(aniso.material_id(), base.material_id());
        assert_eq!(aniso.pixels(), base.pixels());
        assert_ne!(aniso, base, "the sampling mode is part of equality");
    }

    /// A map is an extent and some bytes, and nothing else — no material id, no
    /// sampling mode. Pinned so a future edit does not re-grow it into a second
    /// `MaterialTexture`.
    #[test]
    fn a_map_carries_its_extent_and_texels() {
        let m = MapPixels::new(2, 3, vec![4; 24]);
        assert_eq!((m.width(), m.height()), (2, 3));
        assert_eq!(m.pixels(), &[4; 24]);
        assert_eq!(m.pixels().len(), 2 * 3 * 4);
        assert_eq!(m, MapPixels::new(2, 3, vec![4; 24]));
        assert_ne!(m, MapPixels::new(3, 2, vec![4; 24]));
    }

    /// **The additive invariant.** A texture built the way every existing producer
    /// builds one authors no maps at all, so every backend binds its neutrals and
    /// the material renders as it did before these fields existed. This is the
    /// assertion that stops a future edit from defaulting one of them to a real
    /// payload and silently moving every frame in every app.
    #[test]
    fn a_texture_authors_no_maps_unless_asked() {
        let plain = MaterialTexture::new(1, 1, 1, vec![0; 4]);
        assert_eq!(plain.normal(), None);
        assert_eq!(plain.orm_height(), None);
        assert_eq!(plain.detail(), None);
        assert_eq!(plain.macro_field(), None);
        // Including through the tuple conversion and the sampling builder, the
        // two other ways a producer reaches this type.
        let via_tuple = MaterialTexture::from((1, 1, 1, vec![0; 4]))
            .with_sampling(TextureSampling::Anisotropic);
        assert_eq!(via_tuple.normal(), None);
        assert_eq!(via_tuple.orm_height(), None);
        assert_eq!(via_tuple.detail(), None);
        assert_eq!(via_tuple.macro_field(), None);
    }

    /// Each setter fills exactly its own slot, and the four are independent: a
    /// swapped pair would light a surface with its occlusion pack, which is the
    /// defect this pins against.
    #[test]
    fn each_map_setter_fills_only_its_own_slot() {
        let map = |tag: u8| MapPixels::new(1, 1, vec![tag, tag, tag, 255]);
        let full = MaterialTexture::new(9, 1, 1, vec![255; 4])
            .with_normal(Some(map(1)))
            .with_orm_height(Some(map(2)))
            .with_detail(Some(map(3)))
            .with_macro_field(Some(map(4)));
        assert_eq!(full.normal(), Some(&map(1)));
        assert_eq!(full.orm_height(), Some(&map(2)));
        assert_eq!(full.detail(), Some(&map(3)));
        assert_eq!(full.macro_field(), Some(&map(4)));
        // The albedo half is untouched by any of them.
        assert_eq!(full.material_id(), 9);
        assert_eq!(full.pixels(), &[255; 4]);
        assert_eq!(full.sampling(), TextureSampling::Crisp);
        // And the maps are part of identity, so a backend cache keyed on the
        // carrier cannot serve a normal-mapped material from an un-mapped entry.
        assert_ne!(full, MaterialTexture::new(9, 1, 1, vec![255; 4]));
    }

    /// `None` is authorable, not just the default: clearing a map is how a
    /// producer says "bind the neutral" after having set one.
    #[test]
    fn a_map_can_be_cleared_back_to_the_neutral() {
        let mapped = MaterialTexture::new(2, 1, 1, vec![7; 4])
            .with_normal(Some(MapPixels::new(1, 1, vec![128, 128, 255, 255])))
            .with_orm_height(Some(MapPixels::new(1, 1, vec![255, 255, 0, 0])))
            .with_detail(Some(MapPixels::new(1, 1, vec![128, 128, 255, 0])))
            .with_macro_field(Some(MapPixels::new(1, 1, vec![128; 4])));
        let cleared = mapped
            .clone()
            .with_normal(None)
            .with_orm_height(None)
            .with_detail(None)
            .with_macro_field(None);
        assert_ne!(cleared, mapped);
        assert_eq!(cleared, MaterialTexture::new(2, 1, 1, vec![7; 4]));
    }

    #[test]
    fn the_tuple_conversion_preserves_every_field() {
        let t = MaterialTexture::from((42, 3, 2, vec![7; 24]));
        assert_eq!(
            (t.material_id(), t.width(), t.height()),
            (42, 3, 2)
        );
        assert_eq!(t.pixels().len(), 24);
    }
}
