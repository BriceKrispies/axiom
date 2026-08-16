//! How a surface participates in lighting — a discriminant, not a hook.

/// The closed set of ways a surface participates in lighting.
///
/// This is the **smallest** extensibility point that answers "materials that can
/// participate differently in lighting without turning every material into
/// arbitrary raw shader code": a three-variant discriminant, not a programmable
/// lighting callback. The discriminant **is** the wire code and it indexes
/// [`LightingModel::ALL`], so this order is the wire order.
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
}

impl LightingModel {
    /// Every model, in discriminant order. The array **is** the decode table and
    /// its index **is** the model code.
    pub const ALL: [LightingModel; 3] = [
        LightingModel::Unlit,
        LightingModel::Lambert,
        LightingModel::LambertSpecular,
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
        assert_eq!(LightingModel::from_code(3), None);
        assert_eq!(LightingModel::from_code(u16::MAX), None);
    }

    #[test]
    fn the_default_is_what_the_engine_already_computes() {
        assert_eq!(LightingModel::default(), LightingModel::LambertSpecular);
        assert!(LightingModel::Unlit < LightingModel::Lambert);
        assert_ne!(LightingModel::Lambert, LightingModel::LambertSpecular);
    }
}
