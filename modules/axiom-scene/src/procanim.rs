//! A data-declared procedural animation: a node that bobs and spins over time.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};
use axiom_math::{Quat, Transform, Vec3};

/// A procedural-animation component: the node's local transform is its **resting**
/// pose (`base`) with a per-frame **bob** (a sine offset along +Y) and **spin** (a
/// rotation about `spin_axis`) composed on top, derived from the frame tick.
///
/// This generalizes [`crate::spin::Spin`] from "rotate over time" to "bob + spin
/// over time, around a resting pose": where `Spin` *overwrites* the local with a
/// pure rotation (so it suits a parent-positioned node), `ProcAnim` keeps the
/// node's authored translation/scale and animates around it, so a *positioned*
/// node (a wall cube at a grid cell) can come alive without losing its place.
/// [`crate::scene_storage::ProcAnimSystem`] animates it each frame from the
/// [`axiom_ecs::WorldStep`] tick — no per-tick application code in the app. A
/// per-node `phase` offsets the bob so a whole scene of nodes never pulses in
/// lockstep; an app draws that variety from the procedural-generation substrate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcAnim {
    base: Transform,
    bob_amplitude: f32,
    bob_period: u32,
    spin_axis: Vec3,
    spin_period: u32,
    phase: u32,
}

impl ProcAnim {
    /// The reflected shape of a procedural-animation component.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "ProcAnim",
        &[
            FieldSchema::new("base", "Transform"),
            FieldSchema::new("bob_amplitude", "f32"),
            FieldSchema::new("bob_period", "u32"),
            FieldSchema::new("spin_axis", "Vec3"),
            FieldSchema::new("spin_period", "u32"),
            FieldSchema::new("phase", "u32"),
        ],
    );

    /// Construct a procedural animation around a `base` resting pose: a bob of
    /// `bob_amplitude` units along +Y every `bob_period` ticks, a full revolution
    /// about `spin_axis` every `spin_period` ticks, offset by `phase` ticks.
    /// Crate-internal: the public authoring boundary is
    /// [`crate::SceneApi::add_procanim`], which takes a typed `Meters` amplitude.
    pub(crate) const fn new(
        base: Transform,
        bob_amplitude: f32,
        bob_period: u32,
        spin_axis: Vec3,
        spin_period: u32,
        phase: u32,
    ) -> Self {
        ProcAnim {
            base,
            bob_amplitude,
            bob_period,
            spin_axis,
            spin_period,
            phase,
        }
    }

    /// The animated local transform at frame `tick`: the resting pose with the
    /// bob added to its Y translation and the spin set as its rotation. Always
    /// computed from `base` (never the previous frame), so it never drifts. A
    /// zero period is treated as one; a degenerate `spin_axis` yields no spin
    /// (identity rotation) while the bob still applies.
    pub fn local_at(&self, tick: u64) -> Transform {
        let bob = (fraction(tick + self.phase as u64, self.bob_period) * std::f32::consts::TAU)
            .sin()
            * self.bob_amplitude;
        let spin = fraction(tick, self.spin_period) * std::f32::consts::TAU;
        let rotation = Quat::from_axis_angle(self.spin_axis, spin).unwrap_or(Quat::IDENTITY);
        let base = self.base;
        Transform {
            translation: Vec3::new(
                base.translation.x,
                base.translation.y + bob,
                base.translation.z,
            ),
            rotation,
            scale: base.scale,
        }
    }
}

/// The fraction `[0, 1)` of the way through a `period`-tick cycle at `tick`. A
/// zero period is floored to one so the division is always safe.
fn fraction(tick: u64, period: u32) -> f32 {
    let p = period.max(1);
    (tick % p as u64) as f32 / p as f32
}

impl Reflect for ProcAnim {
    const SCHEMA: TypeSchema = ProcAnim::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.base.reflect_write(writer);
        self.bob_amplitude.reflect_write(writer);
        self.bob_period.reflect_write(writer);
        self.spin_axis.reflect_write(writer);
        self.spin_period.reflect_write(writer);
        self.phase.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Transform::reflect_read(reader).and_then(|base| {
            f32::reflect_read(reader).and_then(|bob_amplitude| {
                u32::reflect_read(reader).and_then(|bob_period| {
                    Vec3::reflect_read(reader).and_then(|spin_axis| {
                        u32::reflect_read(reader).and_then(|spin_period| {
                            u32::reflect_read(reader).map(|phase| ProcAnim {
                                base,
                                bob_amplitude,
                                bob_period,
                                spin_axis,
                                spin_period,
                                phase,
                            })
                        })
                    })
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim() -> ProcAnim {
        ProcAnim::new(
            Transform::from_translation(Vec3::new(2.0, 5.0, -3.0)),
            0.5,
            120,
            Vec3::UNIT_Y,
            240,
            0,
        )
    }

    #[test]
    fn at_tick_zero_the_pose_rests_with_identity_spin() {
        let t = anim().local_at(0);
        // sin(0) = 0 → no bob; spin angle 0 → identity rotation; scale unchanged.
        assert_eq!(t.translation, Vec3::new(2.0, 5.0, -3.0));
        assert!((t.rotation.w - 1.0).abs() < 1.0e-6);
        assert_eq!(t.scale, anim().base.scale);
    }

    #[test]
    fn the_bob_lifts_the_node_along_y_only() {
        // A quarter through the 120-tick bob period → sin(π/2) = 1 → full amplitude.
        let t = anim().local_at(30);
        assert!((t.translation.y - 5.5).abs() < 1.0e-5);
        assert_eq!(t.translation.x, 2.0);
        assert_eq!(t.translation.z, -3.0);
    }

    #[test]
    fn the_spin_advances_with_the_tick() {
        let a = anim();
        assert_ne!(a.local_at(0).rotation.w, a.local_at(60).rotation.w);
    }

    #[test]
    fn the_phase_offsets_the_bob_between_nodes() {
        let base = Transform::from_translation(Vec3::ZERO);
        let a = ProcAnim::new(base, 1.0, 100, Vec3::UNIT_Y, 200, 0);
        let b = ProcAnim::new(base, 1.0, 100, Vec3::UNIT_Y, 200, 25);
        assert_ne!(a.local_at(0).translation.y, b.local_at(0).translation.y);
    }

    #[test]
    fn a_degenerate_spin_axis_yields_identity_rotation_but_still_bobs() {
        // Zero axis → from_axis_angle errs → unwrap_or(IDENTITY); the bob remains.
        let a = ProcAnim::new(
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
            0.5,
            80,
            Vec3::new(0.0, 0.0, 0.0),
            200,
            0,
        );
        let t = a.local_at(20);
        assert!((t.rotation.w - 1.0).abs() < 1.0e-6);
        assert_ne!(t.translation.y, 1.0); // bob applied
    }

    #[test]
    fn zero_periods_are_floored_and_never_divide_by_zero() {
        let t = ProcAnim::new(Transform::IDENTITY, 1.0, 0, Vec3::UNIT_Y, 0, 0).local_at(7);
        // fraction → 0 for both → no bob, no spin, no panic.
        assert_eq!(t.translation, Vec3::ZERO);
        assert!((t.rotation.w - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn schema_names_the_proc_anim_fields() {
        assert_eq!(ProcAnim::SCHEMA.name(), "ProcAnim");
        assert_eq!(ProcAnim::SCHEMA.fields().len(), 6);
        assert_eq!(ProcAnim::SCHEMA.fields()[1].name(), "bob_amplitude");
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let a = anim();
        let mut w = BinaryWriter::new();
        a.reflect_write(&mut w);
        let got = ProcAnim::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(got, a);
        assert!(ProcAnim::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }
}
