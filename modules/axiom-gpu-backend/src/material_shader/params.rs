//! The runtime material shader's parameter block — re-exported from the layer
//! that owns it.
//!
//! These types **used to be defined here**, next to the WGSL that reads them.
//! That was the wrong home and the reason is worth keeping: an app authors
//! `MaterialParams`, and **authored data does not belong in a module**. They now
//! live in [`axiom_surface::MaterialParams`], which is the same split
//! `axiom_surface::Surface` (a layer type) and the WGSL generated from it (this
//! module's business) already make.
//!
//! Putting them anywhere else would have forced one of two bad shapes: an app
//! depending on a GPU backend in order to describe a material, or the host's
//! material contract naming a module's type. Both invert the dependency
//! direction the Module Law fixes.
//!
//! The re-export keeps this module's own call sites reading as
//! `material_shader::params::MaterialParams`, which is where a reader of the
//! shader expects to find them.

pub(crate) use axiom_surface::{
    hex_to_linear, srgb_to_linear, MaterialParams, UvMode, SLOTS_USED, SLOT_COUNT,
};

/// The packed block as the bytes the surface parameter buffer takes.
///
/// [`MaterialParams::pack`] produces `[[f32; 4]; 32]` — the authored values in
/// their slot order, which is a layer concern. Turning that into a byte run for
/// a uniform buffer is transport, and transport is this module's job, so the
/// conversion lives here rather than in the layer.
pub(crate) fn param_bytes(params: &MaterialParams) -> Vec<u8> {
    params
        .pack()
        .iter()
        .flat_map(|slot| slot.iter().flat_map(|v| v.to_le_bytes()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_packed_block_is_four_bytes_per_float_in_slot_order() {
        let params = MaterialParams::default();
        let bytes = param_bytes(&params);
        assert_eq!(bytes.len(), SLOT_COUNT * 4 * 4);
        // Slot 0's `z` lane is `scale`, whose default is 2.0. Reading it back out
        // of the byte run proves the ordering is little-endian, lane-major and
        // slot-major — the layout the WGSL `array<vec4<f32>, 32>` expects.
        let scale = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        assert_eq!(scale, 2.0);
    }

    #[test]
    fn a_retuned_parameter_changes_the_bytes_and_nothing_else() {
        let a = param_bytes(&MaterialParams::default());
        let b = param_bytes(&MaterialParams {
            scale: 7.0,
            ..MaterialParams::default()
        });
        assert_ne!(a, b);
        assert_eq!(a.len(), b.len());
    }

    /// The re-export is the point of this module: the names must resolve here.
    #[test]
    fn the_layers_vocabulary_is_reachable_under_this_path() {
        assert_eq!(UvMode::default(), UvMode::Planar);
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert_eq!(hex_to_linear(0x00_0000), [0.0, 0.0, 0.0]);
        assert!(SLOTS_USED <= SLOT_COUNT);
    }
}
