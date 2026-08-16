//! Committed goldens for the surface wire format.
//!
//! Two kinds, for two reasons.
//!
//! The **plain** surface — every channel at its default, no layers — is
//! committed as literal bytes, annotated record by record. It is the smallest
//! complete example of the format, so a reader can check the layout by eye and a
//! format change costs a deliberate edit here.
//!
//! The **layered, multi-channel** surface is committed as its exact byte length,
//! the [`StableHash`] of its bytes, and its structural digest. A thousand
//! literal bytes would be a golden nobody reads and everybody regenerates; a
//! length plus a content hash fails exactly as loudly and stays legible. Both
//! forms make a silent format change impossible, which is the whole job.

use axiom_field::{FieldBuilder, FieldGraph, FieldId, FieldOp, FieldType, FieldValue};
use axiom_kernel::{Meters, Ratio, StableHash};
use axiom_math::Vec4;
use axiom_recipe::{Param, Scalar};
use axiom_surface::{
    ChannelBinding, LayerBlend, LightingModel, Surface, SurfaceBuilder, SurfaceChannel,
    SurfaceLayer,
};

/// `uv.x` — the one varying the layered golden rides on.
fn uv_x() -> FieldGraph {
    let (builder, uv) = FieldBuilder::new(FieldId::of_name("golden/uv"), 1).push(
        FieldOp::Uv,
        Vec::new(),
        Vec::new(),
    );
    let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
    builder.build(lane)
}

/// `point.x` — an object-space ramp, the height the derived normal is built
/// from. It reads `Point` deliberately: `normal_from_height` differences in
/// **object space**, so a height authored over `Uv` alone has no object-space
/// gradient to find.
fn point_x() -> FieldGraph {
    let (builder, point) = FieldBuilder::new(FieldId::of_name("golden/ramp"), 1).push(
        FieldOp::Point,
        Vec::new(),
        Vec::new(),
    );
    let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![point]);
    builder.build(lane)
}

/// A layered, multi-channel surface: a field roughness, a parameterised
/// opacity, a metallic constant, a derived normal, and two layers with
/// different blends — one of them masked by a field.
fn layered() -> Surface {
    let (builder, slot) = FieldBuilder::new(FieldId::of_name("golden/fade"), 1)
        .declare("fade", FieldValue::scalar(Scalar::new(0.5)));
    let (builder, fade) = builder.push_param(slot, FieldType::Scalar);
    let paint = SurfaceBuilder::new()
        .constant(
            SurfaceChannel::BaseColor,
            FieldValue::vec4(Vec4::new(0.8, 0.1, 0.1, 1.0)),
        )
        .constant(
            SurfaceChannel::Roughness,
            FieldValue::scalar(Scalar::new(0.25)),
        )
        .build()
        .expect("a constant paint layer is legal");
    let dirt = SurfaceBuilder::new()
        .constant(
            SurfaceChannel::BaseColor,
            FieldValue::vec4(Vec4::new(0.2, 0.15, 0.1, 1.0)),
        )
        .lighting(LightingModel::Lambert)
        .build()
        .expect("a constant dirt layer is legal");
    SurfaceBuilder::new()
        .field(SurfaceChannel::Roughness, uv_x())
        .field(SurfaceChannel::Opacity, builder.build(fade))
        .constant(
            SurfaceChannel::Metallic,
            FieldValue::scalar(Scalar::new(1.0)),
        )
        .normal_from_height(
            point_x(),
            Meters::finite_or_zero(0.25),
            Ratio::finite_or_zero(2.0),
        )
        .expect("a scalar height derives a normal")
        .lighting(LightingModel::LambertSpecular)
        .layer(SurfaceLayer::new(
            paint,
            ChannelBinding::field(uv_x()),
            LayerBlend::Over,
        ))
        .layer(SurfaceLayer::new(
            dirt,
            ChannelBinding::constant(FieldValue::scalar(Scalar::new(0.375))),
            LayerBlend::Multiply,
        ))
        .build()
        .expect("two layers are within budget")
}

/// The canonical bytes of `SurfaceBuilder::new().build()`.
///
/// One record: no parent, the `Over` blend and the opaque mask the root
/// synthesizes, the `LambertSpecular` lighting model, then the seven channel
/// defaults in channel order. Every binding is `kind=1` (constant), a `u16` type
/// code, four `f32` lanes, and a zero-length graph payload — a constant carries
/// no graph bytes at all.
#[rustfmt::skip]
const PLAIN_BYTES: [u8; 208] = [
    1, 0, 0, 0,                                          // surface schema 1.0
    1, 0, 0, 0,                                          // one record
    255, 255, 255, 255,                                  // record 0 has no parent
    0, 0,                                                // blend Over (synthesized)
    2, 0,                                                // lighting LambertSpecular
    1, 0, 0, 0,                                          // mask: constant, Scalar
    0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,   //   lanes 1, 0, 0, 0
    0, 0, 0, 0,                                          //   no graph payload
    1, 0, 3, 0,                                          // BaseColor: constant, Vec4
    0, 0, 128, 63, 0, 0, 128, 63, 0, 0, 128, 63, 0, 0, 128, 63,
    0, 0, 0, 0,
    1, 0, 0, 0,                                          // Roughness: constant, Scalar
    0, 0, 0, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,     //   0.5
    0, 0, 0, 0,
    1, 0, 0, 0,                                          // Metallic: constant, Scalar
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,      //   0.0
    0, 0, 0, 0,
    1, 0, 2, 0,                                          // Normal: constant, Vec3
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0,   //   (0, 0, 1)
    0, 0, 0, 0,
    1, 0, 3, 0,                                          // Emission: constant, Vec4
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,      //   (0, 0, 0, 0)
    0, 0, 0, 0,
    1, 0, 0, 0,                                          // Opacity: constant, Scalar
    0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,   //   1.0
    0, 0, 0, 0,
    1, 0, 2, 0,                                          // Displacement: constant, Vec3
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,      //   (0, 0, 0)
    0, 0, 0, 0,
];

/// The structural digest of [`PLAIN_BYTES`]' surface.
const PLAIN_DIGEST: u64 = 6_852_318_313_184_180_331;

/// The exact byte length, content hash and structural digest of [`layered`].
const LAYERED_LEN: usize = 1274;
const LAYERED_HASH: u64 = 13_223_552_525_189_429_878;
const LAYERED_DIGEST: u64 = 17_552_574_593_001_653_655;

/// The structural digest of [`layered`] once flattened. Flattening is a pure
/// function, so this is a fact about the blend rules, not about a run.
const FLATTENED_DIGEST: u64 = 5_603_923_885_759_528_144;

#[test]
fn the_plain_golden_bytes_and_digest_are_unchanged() {
    let plain = SurfaceBuilder::new().build().expect("a default surface is legal");
    assert_eq!(plain.serialize(), PLAIN_BYTES);
    assert_eq!(plain.digest(), StableHash::from_raw(PLAIN_DIGEST));
    assert_eq!(
        Surface::deserialize(&PLAIN_BYTES),
        Ok(plain),
        "the golden bytes must still decode to the surface that produced them"
    );
}

#[test]
fn the_layered_golden_is_unchanged() {
    let bytes = layered().serialize();
    assert_eq!(bytes.len(), LAYERED_LEN);
    assert_eq!(StableHash::of_bytes(&bytes), StableHash::from_raw(LAYERED_HASH));
    assert_eq!(layered().digest(), StableHash::from_raw(LAYERED_DIGEST));
    assert_eq!(Surface::deserialize(&bytes), Ok(layered()));
}

#[test]
fn every_truncation_of_the_layered_golden_fails_cleanly() {
    let bytes = layered().serialize();
    (0..bytes.len()).for_each(|n| {
        assert!(
            Surface::deserialize(&bytes[..n]).is_err(),
            "prefix of length {n} must not decode"
        );
    });
}

#[test]
fn flattening_the_layered_golden_is_reproducible() {
    let flat = layered().flatten().expect("two layers compose");
    assert!(flat.layers().is_empty());
    assert_eq!(flat.digest(), StableHash::from_raw(FLATTENED_DIGEST));
    assert_eq!(
        layered().flatten().expect("composes").serialize(),
        flat.serialize()
    );
    assert_eq!(Surface::deserialize(&flat.serialize()), Ok(flat));
}

#[test]
fn the_layered_golden_reports_the_requirements_its_graphs_imply() {
    let needs = layered().requirements();
    assert!(needs.inputs().contains(axiom_surface::SurfaceInput::UV));
    assert!(needs.inputs().contains(axiom_surface::SurfaceInput::POINT));
    assert!(!needs.inputs().contains(axiom_surface::SurfaceInput::TIME));
    assert!(!needs.inputs().contains(axiom_surface::SurfaceInput::NORMAL));
    assert!(!needs.has_displacement());
    // Every channel varies: one of the two layers is masked by a field.
    SurfaceChannel::ALL
        .iter()
        .for_each(|channel| assert!(needs.varies(*channel)));
}

#[test]
fn retuning_a_parameter_leaves_the_layered_digest_alone() {
    let tuned = |value: f32| {
        let (builder, slot) = FieldBuilder::new(FieldId::of_name("golden/fade"), 1)
            .declare("fade", FieldValue::scalar(Scalar::new(value)));
        let (builder, node) = builder.push_param(slot, FieldType::Scalar);
        SurfaceBuilder::new()
            .field(SurfaceChannel::Opacity, builder.build(node))
            .layer(SurfaceLayer::new(
                SurfaceBuilder::new().build().expect("legal"),
                ChannelBinding::field(uv_x()),
                LayerBlend::Over,
            ))
            .build()
            .expect("one layer is within budget")
    };
    assert_ne!(tuned(0.1).serialize(), tuned(0.9).serialize());
    assert_eq!(tuned(0.1).digest(), tuned(0.9).digest());
}

