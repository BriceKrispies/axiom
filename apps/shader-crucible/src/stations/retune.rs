//! **Station 4 — the parameter retune.** The load-bearing property of the whole
//! design, made visible: a surface's three knobs move the pixels and **do not
//! move its digest**.
//!
//! The claim is not "the digest is stable" as a nicety. It is the reason a
//! program cache is safe to key on a digest at all:
//!
//! * A material tweak cannot invalidate a compiled shader — so retuning is a
//!   **uniform write**, not a recompile, and cannot stutter a frame.
//! * Animating a knob cannot explode into program variants — so a hundred
//!   differently-tuned instances of this station are **one** program.
//!
//! That second consequence is the sharpest possible demonstration and it is what
//! [`retune_series`] exists for: hand the preparation barrier the same surface at
//! nine different tunings and it compiles **one** program, because the catalog is
//! content-addressed on the digest and all nine digests are the same number.
//!
//! ## What *does* move the digest, stated so nobody has to infer it
//!
//! A channel bound to a **constant** is structure, exactly as a `Const` node is
//! in a graph: changing it moves the digest. To retune a channel without moving
//! the digest, bind it to a graph that reads a parameter slot — which is what
//! every knob here does. `retuning_a_constant_channel_does_move_the_digest`
//! pins the other side of that line, because a demonstration that only shows the
//! happy case is not a demonstration.

use axiom_field::{FieldBuilder, FieldGraph, FieldId};
use axiom_math::Vec4;
use axiom_noise::{FbmConfig, Frequency};
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

use crate::authoring::{
    add, clamp, component, compose3, knob, konst, konst4, mix, mul, pow, remap01, uv,
};

/// The seed of the retune station's grain.
const GRAIN_SEED: u64 = 0x77A2_31FE;

/// How tight the banding is.
pub const PARAM_FREQUENCY: &str = "crucible/retune/frequency";
/// How sharply the bands resolve — a `Pow` exponent.
pub const PARAM_SHARPNESS: &str = "crucible/retune/sharpness";
/// How far the grain warps the bands.
pub const PARAM_WARP: &str = "crucible/retune/warp";

/// The three knobs station 4 retunes, named at the call site because naming them
/// **is** the point of the station.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetuneTuning {
    /// Band frequency across the parameterisation.
    pub frequency: f32,
    /// Band sharpness (the `Pow` exponent).
    pub sharpness: f32,
    /// How far the grain warps the band phase.
    pub warp: f32,
}

impl RetuneTuning {
    /// The shipped tuning.
    pub const SHIPPED: RetuneTuning = RetuneTuning {
        frequency: 26.0,
        sharpness: 2.2,
        warp: 1.1,
    };
}

/// The banded pattern, with every knob a **parameter slot**.
fn banded(tuning: RetuneTuning) -> FieldGraph {
    let builder = FieldBuilder::new(FieldId::of_name("crucible/retune/bands"), 1);
    let (builder, coords) = uv(builder);
    let (builder, u) = component(builder, coords, 0);
    let (builder, v) = component(builder, coords, 1);
    let (builder, zero) = konst(builder, 0.0);
    let (builder, flat) = compose3(builder, u, v, zero);

    let (builder, grain) = builder.push_fbm(
        GRAIN_SEED,
        FbmConfig::new(3, Frequency::new(3.0).expect("an authored frequency is positive")),
        flat,
    );
    let (builder, warp_knob) = knob(builder, PARAM_WARP, tuning.warp);
    let (builder, warped) = mul(builder, grain, warp_knob);

    let (builder, frequency) = knob(builder, PARAM_FREQUENCY, tuning.frequency);
    let (builder, scaled) = mul(builder, u, frequency);
    let (builder, phase) = add(builder, scaled, warped);
    let (builder, wave) = crate::authoring::sin(builder, phase);
    let (builder, unit) = remap01(builder, wave);

    // `Pow(a, b)` is `powf` where `a > 0` and exactly `0.0` at or below zero,
    // and `unit` is already in `[0, 1]`, so the sharpening is total and needs no
    // guard. (A *square* would be `Mul(x, x)` — `Pow(x, 2)` is zero across the
    // whole negative half and is never the right spelling for one.)
    let (builder, sharpness) = knob(builder, PARAM_SHARPNESS, tuning.sharpness);
    let (builder, sharp) = pow(builder, unit, sharpness);
    let (builder, one) = konst(builder, 1.0);
    let (builder, bounded) = clamp(builder, sharp, zero, one);

    let (builder, dark) = konst4(builder, 0.075, 0.055, 0.110, 1.0);
    let (builder, lit) = konst4(builder, 0.898, 0.631, 0.208, 1.0);
    let (builder, color) = mix(builder, dark, lit, bounded);
    builder.build(color)
}

/// **Station 4** at the shipped tuning.
pub fn retune_surface() -> Surface {
    retune_surface_tuned(RetuneTuning::SHIPPED)
}

/// **Station 4** at an arbitrary tuning.
///
/// **Every surface this returns has the identical [`Surface::digest`].**
pub fn retune_surface_tuned(tuning: RetuneTuning) -> Surface {
    SurfaceBuilder::new()
        .lighting(LightingModel::LambertSpecular)
        .field(SurfaceChannel::BaseColor, banded(tuning))
        .constant(
            SurfaceChannel::Emission,
            axiom_field::FieldValue::vec4(Vec4::new(0.0, 0.0, 0.0, 0.0)),
        )
        .build()
        .expect("a vec4 field is a legal base colour")
}

/// Nine tunings sweeping every knob — the set the barrier is handed to show that
/// nine retunes cost **one** program.
pub fn retune_series() -> Vec<Surface> {
    (0..9)
        .map(|step| {
            let t = step as f32 / 8.0;
            retune_surface_tuned(RetuneTuning {
                frequency: 4.0 + t * 12.0,
                sharpness: 1.2 + t * 4.0,
                warp: t * 1.4,
            })
        })
        .collect()
}

/// The digest station 4 displays on screen, as the sixteen hex digits a human
/// can read off a label and compare against the next frame's.
pub fn displayed_digest() -> String {
    format!("{:016X}", retune_surface().digest().raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_field::EvalContext;
    use axiom_math::{Vec2, Vec3};

    fn sample(surface: &Surface, u: f32, v: f32) -> [f32; 4] {
        let graph = surface
            .binding(SurfaceChannel::BaseColor)
            .as_field()
            .expect("station 4's base colour is a field")
            .clone();
        let value = graph
            .evaluate(&EvalContext::at(
                Vec3::new(u, v, 0.0),
                Vec2::new(u, v),
                Vec3::UNIT_Y,
            ))
            .expect("a validated graph evaluates")
            .as_vec4();
        [value.x, value.y, value.z, value.w]
    }

    /// **The load-bearing assertion of the whole design.** Retuning every knob
    /// leaves `Surface::digest` bit-identical, and moves the pixels.
    #[test]
    fn retuning_every_knob_leaves_the_surface_digest_identical() {
        let shipped = retune_surface();
        let retuned = retune_surface_tuned(RetuneTuning {
            frequency: 19.0,
            sharpness: 5.5,
            warp: 1.9,
        });
        assert_eq!(
            shipped.digest(),
            retuned.digest(),
            "a knob value moved the structural digest — a program cache keyed on \
             it would recompile on a material tweak"
        );
        // ...and the serialized state genuinely differs, so this is not two
        // identical surfaces agreeing by accident.
        assert_ne!(shipped.serialize(), retuned.serialize());
        // ...and it is a change a viewer can see.
        assert_ne!(sample(&shipped, 0.31, 0.44), sample(&retuned, 0.31, 0.44));
    }

    /// **The other side of the line.** A *constant* channel is structure, so
    /// changing it does move the digest. Stated as a test so the rule is not
    /// folklore.
    #[test]
    fn retuning_a_constant_channel_does_move_the_digest() {
        let dim = retune_surface_tuned(RetuneTuning::SHIPPED);
        let glowing = SurfaceBuilder::new()
            .lighting(LightingModel::LambertSpecular)
            .field(SurfaceChannel::BaseColor, banded(RetuneTuning::SHIPPED))
            .constant(
                SurfaceChannel::Emission,
                axiom_field::FieldValue::vec4(Vec4::new(0.2, 0.1, 0.0, 0.0)),
            )
            .build()
            .expect("legal");
        assert_ne!(dim.digest(), glowing.digest());
    }

    /// **Nine tunings are one digest**, which is what makes them one program at
    /// the barrier.
    #[test]
    fn a_whole_retune_sweep_collapses_to_one_digest() {
        let series = retune_series();
        assert_eq!(series.len(), 9);
        let digests: std::collections::BTreeSet<u64> =
            series.iter().map(|s| s.digest().raw()).collect();
        assert_eq!(digests.len(), 1, "the sweep produced {} programs", digests.len());
        // Every member is genuinely a different material.
        let bytes: std::collections::BTreeSet<Vec<u8>> =
            series.iter().map(Surface::serialize).collect();
        assert_eq!(bytes.len(), 9);
    }

    #[test]
    fn the_displayed_digest_is_the_surfaces_own() {
        assert_eq!(
            displayed_digest(),
            format!("{:016X}", retune_surface().digest().raw())
        );
        assert_eq!(displayed_digest().len(), 16);
    }
}
