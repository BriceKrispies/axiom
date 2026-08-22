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

use axiom_field::{
    EvalContext, FieldBuilder, FieldGraph, FieldId, FieldOp, FieldType, FieldValue, Param, Scalar,
};
use axiom_kernel::{Meters, Ratio, Seconds, StableHash};
use axiom_math::{Vec2, Vec3, Vec4};
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
/// A header — the schema stamp and the surface's **kind** — then one record: no
/// parent, the `Over` blend and the opaque mask the root synthesizes, the
/// `LambertSpecular` lighting model, then the seven channel defaults in channel
/// order. Every binding is `kind=1` (constant), a `u16` type code, four `f32`
/// lanes, and a zero-length graph payload — a constant carries no graph bytes at
/// all.
///
/// **Re-recorded when the format gained a surface-kind code** (see
/// [`axiom_surface::SurfaceKind`]). Two bytes entered the header and the schema
/// stamp went 1.0 -> 2.0, so every byte after offset 4 shifted and the digest
/// moved with them. Nothing about *this* surface changed: it is still a plain
/// field surface with seven default channels, and it still decodes to exactly
/// the surface that produced it, which the test below asserts. The kind code is
/// `0` — `SurfaceKind::Field`.
#[rustfmt::skip]
const PLAIN_BYTES: [u8; 210] = [
    2, 0, 0, 0,                                          // surface schema 2.0
    0, 0,                                                // SurfaceKind::Field
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
const PLAIN_DIGEST: u64 = 9_638_930_373_458_452_976;

/// The exact byte length, content hash and structural digest of [`layered`].
///
/// **All three re-recorded when the format gained a surface-kind code** (see
/// [`axiom_surface::SurfaceKind`]): two bytes entered the header, so the length
/// went 1274 -> 1276 and both hashes moved with the bytes. The surface itself is
/// unchanged — same layers, same blends, same bindings — and it still decodes
/// back to itself, which the test asserts. A digest that moves because the
/// *format* moved is the golden doing its job; a digest that moved because the
/// surface moved would be a defect, and the round-trip assertion is what tells
/// the two apart.
const LAYERED_LEN: usize = 1276;
const LAYERED_HASH: u64 = 2_733_246_799_667_958_281;
const LAYERED_DIGEST: u64 = 9_267_803_130_087_330_740;

/// The structural digest of [`layered`] once flattened. Flattening is a pure
/// function, so this is a fact about the blend rules, not about a run.
///
/// **Re-recorded when `axiom_field` gained the exact identity
/// `Mix(x, x, t) -> x`** (was `5_603_923_885_759_528_144`). This digest is
/// *structural*: the flattened `Emission` and `Displacement` channels — whose
/// every surface binds the same constant, blended through a field mask — went
/// from 7-node graphs to 1, so the graph is a different graph and its digest is
/// a different digest. **No value moved**: every channel of this surface
/// evaluates to the bit-identical `f32` it did before, which
/// [`flattening_the_layered_golden_moves_no_value`] proves against the
/// hand-written blend expression rather than against a recorded number.
const FLATTENED_DIGEST: u64 = 1_818_877_878_594_399_193;

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

/// Three sample points, spanning the object-space ramp and the UV the masks
/// ride on.
fn samples() -> [EvalContext; 3] {
    [(-0.4_f32, -0.5_f32), (-0.17, -0.19), (0.29, 0.43)].map(|(a, b)| {
        EvalContext::new(
            Vec3::new(a, b, a - b),
            Vec2::new(a.abs(), b.abs()),
            Vec3::new(0.0, 1.0, 0.0),
            Seconds::new(a.abs()).expect("a finite time"),
        )
    })
}

/// Every lane of every flattened channel at [`samples`], as raw `f32` bits.
///
/// **A value golden, not a structural one.** `FLATTENED_DIGEST` above says what
/// the flattened *graphs* are; this says what they *compute*, and only one of
/// the two is allowed to move when canonicalisation gets smarter. A rewrite that
/// makes a graph smaller is a win; a rewrite that moves one of these bits is a
/// bug, and without this table the digest is the only witness and it cannot tell
/// the two apart.
///
/// Recorded when `axiom_field` gained the exact identity `Mix(x, x, t) -> x`,
/// and verified **bit-identical with that identity disabled** — which is the
/// whole claim the identity makes.
#[rustfmt::skip]
const FLATTENED_VALUES: [[[u32; 4]; 3]; 7] = [
    // BaseColor (Vec4)
    [[0x3f24_dd30, 0x3edf_3b64, 0x3ed9_1687, 0x3f80_0000],
     [0x3f2d_1b72, 0x3f13_b780, 0x3f0f_a6b5, 0x3f80_0000],
     [0x3f28_ce71, 0x3f00_e1b1, 0x3efa_ab37, 0x3f80_0000]],
    // Roughness (Scalar)
    [[0x3e8d_70a4, 0, 0, 0], [0x3e18_c155, 0, 0, 0], [0x3e67_a0f9, 0, 0, 0]],
    // Metallic (Scalar)
    [[0x3ec0_0000, 0, 0, 0], [0x3f04_cccc, 0, 0, 0], [0x3ee3_3334, 0, 0, 0]],
    // Normal (Vec3)
    [[0xbeab_bae4, 0, 0x3f2b_178e, 0],
     [0xbeed_8f52, 0, 0x3f0a_8b40, 0],
     [0xbecb_36c1, 0, 0x3f1b_868a, 0]],
    // Emission (Vec4) — every surface in the tree binds the same constant.
    [[0; 4]; 3],
    // Opacity (Scalar)
    [[0x3f33_3333, 0, 0, 0], [0x3f15_c28f, 0, 0, 0], [0x3f25_1eb8, 0, 0, 0]],
    // Displacement (Vec3) — likewise.
    [[0; 4]; 3],
];

#[test]
fn flattening_the_layered_golden_moves_no_value() {
    let flat = layered().flatten().expect("two layers compose");
    SurfaceChannel::ALL.iter().enumerate().for_each(|(index, channel)| {
        let graph = flat.binding(*channel).as_graph();
        samples().iter().enumerate().for_each(|(sample, context)| {
            let lanes = graph.evaluate(context).expect("evaluates").as_vec4();
            assert_eq!(
                [lanes.x.to_bits(), lanes.y.to_bits(), lanes.z.to_bits(), lanes.w.to_bits()],
                FLATTENED_VALUES[index][sample],
                "{channel:?} moved at sample {sample}"
            );
        });
    });
    // The two channels every surface in the tree binds identically must be the
    // channel default itself — a fact known without consulting the table above,
    // because blending a value with itself under any mask is that value.
    [SurfaceChannel::Emission, SurfaceChannel::Displacement]
        .iter()
        .for_each(|channel| {
            assert!(
                flat.binding(*channel).is_constant(),
                "{channel:?} is the same constant everywhere and must flatten back to one"
            );
        });
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

