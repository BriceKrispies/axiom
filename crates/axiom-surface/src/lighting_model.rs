//! How a surface participates in lighting — a discriminant, not a hook.

/// The closed set of ways a surface participates in lighting.
///
/// This is the **smallest** extensibility point that answers "materials that can
/// participate differently in lighting without turning every material into
/// arbitrary raw shader code": a four-variant discriminant, not a programmable
/// lighting callback. The discriminant **is** the wire code and it indexes
/// [`LightingModel::ALL`], so this order is the wire order.
///
/// **The order is append-only.** The codes are compared in WGSL
/// (`AXIOM_LIGHT_*` in the GPU backend's shader prelude) and serialized into
/// [`crate::Surface`]'s canonical bytes, so inserting a variant would silently
/// relight every surface already authored. A new model goes on the end.
///
/// [`LightingModel::LambertSpecular`] is the [`Default`] because it is what the
/// engine's one lit shader already computes — so a surface authored without
/// saying anything about lighting renders exactly as today's material does, and
/// this type changes no pixel on its own.
///
/// [`LightingModel::Unlit`] is what finally gives `RenderPipelineKind::UNLIT`
/// something behind it, and it removes the need for the "black albedo plus
/// emission" trick apps use today to fake an emissive-only surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum LightingModel {
    /// Base colour and emission are presented as-is. No light is gathered.
    Unlit = 0,
    /// Diffuse gathering only.
    Lambert = 1,
    /// Diffuse gathering plus the engine's existing specular term.
    LambertSpecular = 2,
    /// **The physically-based model**: Cook-Torrance with a GGX
    /// (Trowbridge-Reitz) distribution, Smith height-correlated visibility and
    /// a Schlick Fresnel over a Lambert diffuse — three.js r180's
    /// `MeshStandardMaterial`, transcribed from its own shader chunks.
    ///
    /// This is the model that makes [`crate::SurfaceChannel::Roughness`] and
    /// [`crate::SurfaceChannel::Metallic`] **live**. Under the three models
    /// above they are carried and read by nothing: specular strength comes from
    /// the legacy per-instance lane and there is no metal/dielectric split at
    /// all. Under this one, roughness is remapped to the GGX `alpha` as the
    /// source does (`alpha = roughness²`, Disney's reparameterisation) and
    /// metalness picks the surface apart into a diffuse albedo
    /// (`base * (1 - metalness)`) and a specular `F0`
    /// (`mix(0.04, base, metalness)`).
    ///
    /// **It is radiometrically scaled, and the three above are not.** Every
    /// term carries the source's `1/PI`, so a physical surface lit by the same
    /// light as a [`LightingModel::Lambert`] one is ~PI times dimmer. That is
    /// the source's unit system, not a defect: light intensities ported from a
    /// three.js scene are already in it. Mixing the models inside one frame
    /// means mixing two unit systems, and the author owns that choice.
    Physical = 3,
}

impl LightingModel {
    /// Every model, in discriminant order. The array **is** the decode table and
    /// its index **is** the model code.
    pub const ALL: [LightingModel; 4] = [
        LightingModel::Unlit,
        LightingModel::Lambert,
        LightingModel::LambertSpecular,
        LightingModel::Physical,
    ];

    /// The wire code — the discriminant, which is also the table index.
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// The model a wire code names, or `None` if the code names no model.
    pub fn from_code(code: u16) -> Option<LightingModel> {
        LightingModel::ALL.get(code as usize).copied()
    }
}

impl Default for LightingModel {
    /// [`LightingModel::LambertSpecular`] — what the engine already computes, so
    /// existing content is unchanged.
    fn default() -> Self {
        LightingModel::LambertSpecular
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_their_table_indices() {
        assert_eq!(LightingModel::Unlit as u16, 0);
        assert_eq!(LightingModel::Lambert as u16, 1);
        assert_eq!(LightingModel::LambertSpecular as u16, 2);
        // Appended, never inserted: the codes are compared in WGSL and
        // serialized into a surface's canonical bytes, so a reorder would
        // relight every surface already authored.
        assert_eq!(LightingModel::Physical as u16, 3);
        LightingModel::ALL
            .iter()
            .enumerate()
            .for_each(|(index, model)| assert_eq!(model.code() as usize, index));
    }

    #[test]
    fn a_known_code_decodes_and_an_unknown_one_does_not() {
        assert_eq!(LightingModel::from_code(0), Some(LightingModel::Unlit));
        assert_eq!(
            LightingModel::from_code(2),
            Some(LightingModel::LambertSpecular)
        );
        assert_eq!(LightingModel::from_code(3), Some(LightingModel::Physical));
        assert_eq!(LightingModel::from_code(4), None);
        assert_eq!(LightingModel::from_code(u16::MAX), None);
    }

    #[test]
    fn the_default_is_what_the_engine_already_computes() {
        assert_eq!(LightingModel::default(), LightingModel::LambertSpecular);
        assert!(LightingModel::Unlit < LightingModel::Lambert);
        assert_ne!(LightingModel::Lambert, LightingModel::LambertSpecular);
        // The default did NOT move when the physical model arrived: a surface
        // that says nothing about lighting still renders as it always has.
        assert_ne!(LightingModel::default(), LightingModel::Physical);
        assert!(LightingModel::LambertSpecular < LightingModel::Physical);
    }
}
