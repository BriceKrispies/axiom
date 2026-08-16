//! Mask-driven layering: one surface composed over another.

use axiom_field::FieldValue;
use axiom_recipe::Scalar;

use crate::binding::ChannelBinding;
use crate::surface::Surface;

/// How many layers a whole surface tree may hold, counting nested ones.
///
/// Four, because a layered surface **flattens into one field graph per channel**
/// and `axiom_recipe::MAX_NODES` (256) is the real budget the flattened graph
/// must fit. Exceeding this is a
/// [`crate::SurfaceErrorCode::LayerBudgetExceeded`], never a silent truncation,
/// and the answer to a scene that does not fit is a simpler surface — not a
/// raised cap.
pub const MAX_LAYERS: usize = 4;

/// How a layer's channel value combines with what is under it.
///
/// The discriminant **is** the wire code and it indexes [`LayerBlend::ALL`] and
/// the flattener's output table, so this order is the wire order.
///
/// Each rule is stated once, here, as the exact `axiom-field` expression the
/// flattener builds — `under` is the accumulated value, `over` is the layer's
/// value and `mask` is the layer's scalar mask:
///
/// | Blend | Expression |
/// |---|---|
/// | [`LayerBlend::Over`] | `Mix(under, over, mask)` |
/// | [`LayerBlend::Add`] | `Add(under, Mul(over, mask))` |
/// | [`LayerBlend::Multiply`] | `Mix(under, Mul(under, over), mask)` |
///
/// The spellings are exact and not interchangeable with the algebraically equal
/// ones a mirror might reach for: `Mix` is `a + (b - a) * t` in the field layer,
/// so a masked multiply written as `under * (1 + (over - 1) * mask)` would
/// differ in the last `f32` bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum LayerBlend {
    /// Interpolate toward the layer by the mask.
    Over = 0,
    /// Add the masked layer.
    Add = 1,
    /// Interpolate toward the product by the mask.
    Multiply = 2,
}

impl LayerBlend {
    /// Every blend, in discriminant order. The array **is** the decode table and
    /// its index **is** the blend code.
    pub const ALL: [LayerBlend; 3] = [LayerBlend::Over, LayerBlend::Add, LayerBlend::Multiply];

    /// The wire code — the discriminant, which is also the table index.
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// The blend's index into the flattener's output table.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The blend a wire code names, or `None` if the code names no blend.
    pub fn from_code(code: u16) -> Option<LayerBlend> {
        LayerBlend::ALL.get(code as usize).copied()
    }
}

/// One layer of a surface: a whole [`Surface`], the scalar mask that selects it,
/// and how it combines with what is under it.
///
/// A layer's surface may itself have layers. The nesting is bounded by
/// [`MAX_LAYERS`] over the whole tree, and it is flattened **iteratively** — a
/// recursive value type does not license a recursive walk.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceLayer {
    surface: Surface,
    mask: ChannelBinding,
    blend: LayerBlend,
}

impl SurfaceLayer {
    /// A layer: `surface` composed over what is under it, selected by `mask` —
    /// which must be a `Scalar` binding — through `blend`.
    pub fn new(surface: Surface, mask: ChannelBinding, blend: LayerBlend) -> Self {
        SurfaceLayer {
            surface,
            mask,
            blend,
        }
    }

    /// The fully-selecting mask: the constant [`Scalar`] `1.0`.
    ///
    /// It is also what a surface's canonical bytes record for the **root**,
    /// which owns no mask of its own — a fixed synthesized value, so the wire
    /// form stays uniform and canonical without a conditional write.
    pub fn opaque_mask() -> ChannelBinding {
        ChannelBinding::constant(FieldValue::scalar(Scalar::new(1.0)))
    }

    /// The layer's own surface.
    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// The scalar mask that selects the layer.
    pub fn mask(&self) -> &ChannelBinding {
        &self.mask
    }

    /// How the layer combines with what is under it.
    pub const fn blend(&self) -> LayerBlend {
        self.blend
    }

    /// Consume the layer, keeping only its surface.
    pub fn into_surface(self) -> Surface {
        self.surface
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface_builder::SurfaceBuilder;
    use axiom_field::FieldType;

    #[test]
    fn codes_are_their_table_indices() {
        assert_eq!(LayerBlend::Over as u16, 0);
        assert_eq!(LayerBlend::Add as u16, 1);
        assert_eq!(LayerBlend::Multiply as u16, 2);
        LayerBlend::ALL.iter().enumerate().for_each(|(index, blend)| {
            assert_eq!(blend.code() as usize, index);
            assert_eq!(blend.index(), index);
        });
    }

    #[test]
    fn a_known_blend_code_decodes_and_an_unknown_one_does_not() {
        assert_eq!(LayerBlend::from_code(0), Some(LayerBlend::Over));
        assert_eq!(LayerBlend::from_code(2), Some(LayerBlend::Multiply));
        assert_eq!(LayerBlend::from_code(3), None);
        assert_eq!(LayerBlend::from_code(u16::MAX), None);
    }

    #[test]
    fn a_layer_reports_the_three_parts_it_was_built_from() {
        let surface = SurfaceBuilder::new().build().expect("a default surface is legal");
        let layer = SurfaceLayer::new(
            surface.clone(),
            SurfaceLayer::opaque_mask(),
            LayerBlend::Multiply,
        );
        assert_eq!(layer.surface(), &surface);
        assert_eq!(layer.mask(), &SurfaceLayer::opaque_mask());
        assert_eq!(layer.blend(), LayerBlend::Multiply);
        assert_eq!(layer.clone().into_surface(), surface);
    }

    #[test]
    fn the_opaque_mask_is_the_scalar_one() {
        let mask = SurfaceLayer::opaque_mask();
        assert_eq!(mask.ty(), Ok(FieldType::Scalar));
        assert_eq!(
            mask.as_constant(),
            Some(FieldValue::scalar(Scalar::new(1.0)))
        );
    }

    #[test]
    fn the_layer_budget_is_four() {
        assert_eq!(MAX_LAYERS, 4);
    }
}
