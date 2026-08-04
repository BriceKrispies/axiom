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

/// One material's albedo texture: which material it belongs to, its extent, its
/// RGBA8 pixels, and how it must be sampled.
///
/// `pixels` is row-major `width * height * 4` bytes. The authoring layer
/// validates that length before a texture id is issued, so a backend receiving
/// this can treat the three as consistent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialTexture {
    material_id: u64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    sampling: TextureSampling,
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
        }
    }

    /// This texture with an explicit sampling mode.
    #[must_use]
    pub fn with_sampling(mut self, sampling: TextureSampling) -> Self {
        self.sampling = sampling;
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
