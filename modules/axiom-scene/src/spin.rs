//! A data-declared spin: a node that rotates about an axis over time.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};
use axiom_math::{Quat, Vec3};

/// A spin component: the node's local transform becomes a pure rotation about
/// `axis`, completing one revolution every `period_ticks` engine frames.
///
/// This is the engine's answer to "rotate this over time" as **data**: a scene
/// declares the axis and period, and [`crate::scene_storage::SpinSystem`]
/// animates it each frame from the [`axiom_ecs::WorldStep`] tick — no per-tick
/// application code in the app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spin {
    axis: Vec3,
    period_ticks: u32,
}

impl Spin {
    /// The reflected shape of a spin component.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Spin",
        &[
            FieldSchema::new("axis", "Vec3"),
            FieldSchema::new("period_ticks", "u32"),
        ],
    );

    /// Construct a spin about `axis` with the given period in ticks.
    pub const fn new(axis: Vec3, period_ticks: u32) -> Self {
        Spin { axis, period_ticks }
    }

    /// The rotation at frame `tick`: one full turn per `period_ticks` (a zero
    /// period is treated as one). `None` iff the axis cannot form a rotation
    /// (e.g. a zero-length axis) — the system then leaves the node untouched.
    pub fn rotation_at(&self, tick: u64) -> Option<Quat> {
        let period = self.period_ticks.max(1);
        let fraction = (tick % period as u64) as f32 / period as f32;
        let angle = fraction * std::f32::consts::TAU;
        Quat::from_axis_angle(self.axis, angle).ok()
    }
}

impl Reflect for Spin {
    const SCHEMA: TypeSchema = Spin::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.axis.reflect_write(writer);
        self.period_ticks.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Vec3::reflect_read(reader).and_then(|axis| {
            u32::reflect_read(reader).map(|period_ticks| Spin { axis, period_ticks })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_at_zero_tick_is_identity_rotation() {
        let s = Spin::new(Vec3::UNIT_Y, 360);
        let q = s.rotation_at(0).unwrap();
        assert!((q.w - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn rotation_advances_with_tick() {
        let s = Spin::new(Vec3::UNIT_Y, 360);
        let a = s.rotation_at(0).unwrap();
        let b = s.rotation_at(90).unwrap();
        assert_ne!(a.w, b.w);
    }

    #[test]
    fn zero_period_is_treated_as_one_and_does_not_divide_by_zero() {
        let s = Spin::new(Vec3::UNIT_Y, 0);
        let q = s.rotation_at(5).unwrap();
        assert!((q.w - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn invalid_axis_yields_no_rotation() {
        let s = Spin::new(Vec3::new(0.0, 0.0, 0.0), 360);
        assert!(s.rotation_at(10).is_none());
    }

    #[test]
    fn schema_names_the_spin_fields() {
        assert_eq!(Spin::SCHEMA.name(), "Spin");
        assert_eq!(Spin::SCHEMA.fields().len(), 2);
        assert_eq!(Spin::SCHEMA.fields()[0].name(), "axis");
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let s = Spin::new(Vec3::new(0.0, 1.0, 0.0), 240);
        let mut w = BinaryWriter::new();
        s.reflect_write(&mut w);
        let got = Spin::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(got, s);
        assert!(Spin::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }
}
