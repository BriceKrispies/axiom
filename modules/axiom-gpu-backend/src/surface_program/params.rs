//! The surface parameter channel: one fixed-size uniform region per program.
//!
//! A surface's tunable numbers cannot ride any lane this backend already has.
//! The instance stream is `INSTANCE_FLOATS = 40` with zero free lanes
//! (`crate::frame_packet_adapter`), and the skinned pipeline already binds 16 of
//! the 16 vertex attributes the WebGL2 downlevel target guarantees — which is
//! why it silently reads a zero emissive and a zero specular. So parameters need
//! their own uniform channel, and this module owns its layout.
//!
//! **One shared layout, a fixed-size region, and never a rewritten buffer.**
//! Three constraints, each imposed by code that already exists:
//!
//! * Every surface program shares one `BindGroupLayout` with a **fixed-size**
//!   parameter region ([`MAX_SURFACE_PARAMS`] slots, [`SURFACE_PARAM_REGION_BYTES`]
//!   bytes). A per-surface layout would make bind groups 1 (`lights`) and 2
//!   (`shadow_sample`) un-hoistable from outside the batch loop, where the scene
//!   renderer sets them exactly once per pass today.
//! * A program's region is addressed by a **dynamic offset**
//!   ([`program_region_offset`]) into one large buffer, never by rewriting a
//!   single small buffer between draws. `crate::post_chain` documents why: a
//!   `queue.write_buffer` is ordered against *submission*, not against the passes
//!   inside the encoder, so N writes to one buffer means every draw in that pass
//!   reads the **last** write. The engine already paid for that bug once and
//!   fixed it with two separate buffers; this scheme generalises the fix.
//! * The region is 512 bytes, a multiple of the 256-byte dynamic-uniform-offset
//!   alignment every WebGPU implementation requires, so a program index maps to a
//!   legal offset with no padding arithmetic at the call site.
//!
//! The bytes are produced at the preparation barrier from a *flattened* surface,
//! so a layered surface's composed parameter table is the one that is packed.

use axiom_field::FieldValue;
use axiom_surface::{Surface, SurfaceChannel};

/// How many parameter slots one surface program may hold. A cap, not a
/// guess: it is what makes the region fixed-size, which is what makes the bind
/// group layout shared. A surface over the cap fails capability validation
/// rather than silently losing its tail.
pub(crate) const MAX_SURFACE_PARAMS: u16 = 32;

/// Bytes one parameter slot occupies: a `vec4<f32>`, the narrowest uniform
/// member WebGPU's `uniform` address space lays out without padding surprises.
/// Every [`FieldValue`] fits, whatever its declared width, because a value's
/// unused lanes are a defined zero.
pub(crate) const SURFACE_PARAM_SLOT_BYTES: u64 = 16;

/// Bytes one program's parameter region occupies: `32 * 16 = 512`. A multiple of
/// 256, the dynamic-offset alignment.
pub(crate) const SURFACE_PARAM_REGION_BYTES: u64 =
    MAX_SURFACE_PARAMS as u64 * SURFACE_PARAM_SLOT_BYTES;

/// Where program number `index` reads its parameters from, inside the one shared
/// buffer. This — not a rewritten buffer — is what keeps two draws in one pass
/// from reading each other's parameters.
pub(crate) const fn program_region_offset(index: u64) -> u64 {
    index * SURFACE_PARAM_REGION_BYTES
}

/// Where each of a program's parameter slots lives inside its region.
///
/// A layout is a *count*, because the mapping is fixed: slot `n` is at
/// `n * 16`. Two surfaces with the same parameter count therefore have the same
/// layout, which is the property that lets every program share one bind group
/// layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParamLayout {
    count: u16,
}

impl ParamLayout {
    /// The layout for a surface holding `count` parameter slots.
    pub(crate) const fn of(count: u16) -> ParamLayout {
        ParamLayout { count }
    }

    /// Whether the layout fits the fixed-size region. A layout that does not fit
    /// is a capability failure, never a truncation.
    pub(crate) const fn fits(self) -> bool {
        self.count <= MAX_SURFACE_PARAMS
    }

    /// The byte offset of slot `slot` inside the program's region, or `None`
    /// when the slot is past the layout or past the cap. Table arithmetic, so an
    /// out-of-range slot is a bounds miss rather than a branch.
    pub(crate) fn byte_offset(self, slot: u16) -> Option<u64> {
        (slot < self.count.min(MAX_SURFACE_PARAMS))
            .then(|| u64::from(slot) * SURFACE_PARAM_SLOT_BYTES)
    }
}

/// Pack a **flattened** surface's parameter values into one program region:
/// `SURFACE_PARAM_REGION_BYTES` little-endian bytes, four `f32` lanes per slot,
/// slots taken in [`SurfaceChannel`] order across the flattened surface's bound
/// graphs.
///
/// Values past the layout's count, and any slot past [`MAX_SURFACE_PARAMS`], are
/// dropped by [`ParamLayout::byte_offset`] — which cannot happen for a surface
/// that passed validation, and cannot corrupt the region for one that did not.
/// A slot nobody declares reads as four zeroes.
pub(crate) fn pack(layout: ParamLayout, flat: &Surface) -> Vec<u8> {
    let mut bytes = vec![0_u8; SURFACE_PARAM_REGION_BYTES as usize];
    SurfaceChannel::ALL
        .iter()
        .flat_map(|channel| {
            flat.binding(*channel)
                .as_field()
                .into_iter()
                .flat_map(|graph| graph.params().values().iter().copied())
        })
        .enumerate()
        .for_each(|(index, value)| {
            let slot = u16::try_from(index).unwrap_or_else(|_| u16::MAX);
            layout
                .byte_offset(slot)
                .map(|offset| write_slot(&mut bytes, offset as usize, value));
        });
    bytes
}

/// Write one value's four lanes at `offset`, little-endian.
fn write_slot(bytes: &mut [u8], offset: usize, value: FieldValue) {
    let lanes = value.as_vec4();
    [lanes.x, lanes.y, lanes.z, lanes.w]
        .iter()
        .enumerate()
        .for_each(|(lane, component)| {
            let at = offset + lane * 4;
            bytes
                .get_mut(at..at + 4)
                .map(|slice| slice.copy_from_slice(&component.to_le_bytes()));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::{FieldBuilder, FieldId, FieldType, FieldValue};
    use axiom_math::Vec4;
    use axiom_recipe::Scalar;
    use axiom_surface::SurfaceBuilder;

    fn lane_at(bytes: &[u8], slot: usize, lane: usize) -> f32 {
        let at = slot * SURFACE_PARAM_SLOT_BYTES as usize + lane * 4;
        f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn a_region_is_512_bytes_and_every_program_offset_is_dynamic_offset_aligned() {
        assert_eq!(SURFACE_PARAM_REGION_BYTES, 512);
        assert_eq!(MAX_SURFACE_PARAMS, 32);
        assert_eq!(SURFACE_PARAM_SLOT_BYTES, 16);
        // The whole point of the scheme: distinct programs read distinct,
        // aligned regions of ONE buffer, so no draw can read another draw's
        // last write.
        (0..4_u64).for_each(|index| {
            let offset = program_region_offset(index);
            assert_eq!(offset % 256, 0, "dynamic offsets must be 256-aligned");
            assert_eq!(offset, index * 512);
        });
        assert_ne!(program_region_offset(0), program_region_offset(1));
    }

    #[test]
    fn slot_offsets_are_dense_and_stop_at_the_count_and_the_cap() {
        let layout = ParamLayout::of(3);
        assert!(layout.fits());
        assert_eq!(layout.byte_offset(0), Some(0));
        assert_eq!(layout.byte_offset(1), Some(16));
        assert_eq!(layout.byte_offset(2), Some(32));
        assert_eq!(layout.byte_offset(3), None);
        // Exactly the cap fits; one over does not.
        assert!(ParamLayout::of(MAX_SURFACE_PARAMS).fits());
        assert!(!ParamLayout::of(MAX_SURFACE_PARAMS + 1).fits());
        // An over-cap layout still cannot address past the region.
        let over = ParamLayout::of(MAX_SURFACE_PARAMS + 5);
        assert_eq!(over.byte_offset(MAX_SURFACE_PARAMS - 1), Some(496));
        assert_eq!(over.byte_offset(MAX_SURFACE_PARAMS), None);
        assert_eq!(over, ParamLayout::of(MAX_SURFACE_PARAMS + 5));
        assert!(format!("{layout:?}").contains("ParamLayout"));
    }

    #[test]
    fn packing_writes_each_declared_value_at_its_slot_offset_and_zeroes_the_rest() {
        let (builder, tint) = FieldBuilder::new(FieldId::of_name("gpu/params/tint"), 1)
            .declare("tint", FieldValue::vec4(Vec4::new(0.25, 0.5, 0.75, 1.0)));
        let (builder, node) = builder.push_param(tint, FieldType::Vec4);
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::BaseColor, builder.build(node))
            .build()
            .expect("a vec4 param field is a legal base colour");
        let flat = surface.flatten().expect("a flat surface flattens to itself");
        let bytes = pack(ParamLayout::of(1), &flat);
        assert_eq!(bytes.len(), 512);
        assert_eq!(lane_at(&bytes, 0, 0), 0.25);
        assert_eq!(lane_at(&bytes, 0, 1), 0.5);
        assert_eq!(lane_at(&bytes, 0, 2), 0.75);
        assert_eq!(lane_at(&bytes, 0, 3), 1.0);
        // Every undeclared slot reads as four zeroes.
        assert!(bytes[16..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_scalar_parameter_occupies_one_slot_and_zeroes_its_unused_lanes() {
        let (builder, slot) = FieldBuilder::new(FieldId::of_name("gpu/params/rough"), 1)
            .declare("rough", FieldValue::scalar(Scalar::new(0.375)));
        let (builder, node) = builder.push_param(slot, FieldType::Scalar);
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Roughness, builder.build(node))
            .build()
            .expect("a scalar param field is a legal roughness");
        let flat = surface.flatten().expect("a flat surface flattens to itself");
        let bytes = pack(ParamLayout::of(1), &flat);
        assert_eq!(lane_at(&bytes, 0, 0), 0.375);
        assert_eq!(lane_at(&bytes, 0, 1), 0.0);
    }

    #[test]
    fn a_constant_only_surface_packs_an_all_zero_region() {
        let surface = SurfaceBuilder::new().build().expect("legal");
        let flat = surface.flatten().expect("flattens");
        let bytes = pack(ParamLayout::of(0), &flat);
        assert_eq!(bytes.len(), 512);
        assert!(bytes.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_value_past_the_layout_is_dropped_rather_than_written_out_of_region() {
        let (builder, a) = FieldBuilder::new(FieldId::of_name("gpu/params/two"), 1)
            .declare("a", FieldValue::scalar(Scalar::new(1.0)));
        let (builder, b) = builder.declare("b", FieldValue::scalar(Scalar::new(2.0)));
        let (builder, na) = builder.push_param(a, FieldType::Scalar);
        let (builder, nb) = builder.push_param(b, FieldType::Scalar);
        let (builder, sum) = builder.push(
            axiom_field::FieldOp::Add,
            Vec::new(),
            vec![na, nb],
        );
        let surface = SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(sum))
            .build()
            .expect("a scalar sum is a legal opacity");
        let flat = surface.flatten().expect("flattens");
        // A layout that claims one slot writes only the first value.
        let bytes = pack(ParamLayout::of(1), &flat);
        assert_eq!(lane_at(&bytes, 0, 0), 1.0);
        assert_eq!(lane_at(&bytes, 1, 0), 0.0);
        // The honest layout writes both.
        let both = pack(ParamLayout::of(2), &flat);
        assert_eq!(lane_at(&both, 0, 0), 1.0);
        assert_eq!(lane_at(&both, 1, 0), 2.0);
    }

    #[test]
    fn writing_a_slot_past_the_region_end_is_a_bounds_miss_not_a_panic() {
        let mut bytes = vec![0_u8; 8];
        write_slot(&mut bytes, 0, FieldValue::scalar(Scalar::new(3.0)));
        // The first lane fit; the rest were dropped rather than panicking.
        assert_eq!(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]), 3.0);
        assert_eq!(bytes.len(), 8);
    }
}
