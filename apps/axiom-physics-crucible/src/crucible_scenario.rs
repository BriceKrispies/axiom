//! The scripted-scenario vocabulary every station authors with.
//!
//! These are *app-owned* description types — the nouns a station uses to ask the
//! harness to create bodies and colliders. They are deliberately phrased in plain
//! `f32`/`Vec3` because the app is a composition leaf; the harness
//! ([`crate::physics_crucible_app::CrucibleWorld`]) is the single chokepoint that
//! converts them into kernel `Ratio`/`Meters` value types and calls `PhysicsApi`.
//! Nothing here touches physics directly.

use axiom::prelude::Vec3;

use crate::crucible_station::CrucibleStation;
use crate::debug_geometry::DebugShape;
use crate::physics_crucible_app::CrucibleWorld;

/// The fixed simulation step: 1/120 s, expressed in nanoseconds. Substepping in
/// the physics module subdivides this further; the crucible always feeds this
/// exact, deterministic delta so every run is byte-reproducible.
pub const FIXED_STEP_NANOS: u64 = 8_333_333;

/// The "hero" step the rendered screenshot freezes at: far enough that dynamic
/// bodies have visibly fallen and contacts have formed, close enough that nothing
/// has come fully to rest.
pub const HERO_STEP: u64 = 48;

/// The total scripted length of a crucible run (steps `0..=RUN_STEPS`).
pub const RUN_STEPS: u64 = 96;

/// A body's simulation kind, with the dynamic mass carried inline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CrucibleKind {
    Static,
    Dynamic { mass: f32 },
    Kinematic,
}

/// The render/identity tag the harness stores per body so translation can colour
/// it without ever reading the physics module's (unexported) body-kind enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindTag {
    Static,
    Dynamic,
    Kinematic,
}

impl CrucibleKind {
    pub fn tag(self) -> KindTag {
        match self {
            CrucibleKind::Static => KindTag::Static,
            CrucibleKind::Dynamic { .. } => KindTag::Dynamic,
            CrucibleKind::Kinematic => KindTag::Kinematic,
        }
    }
}

/// A collider shape, in app units. `Capsule` exists for body-kind variety only —
/// the physics module does not yet generate capsule contacts (documented), so the
/// crucible never relies on a capsule resting on anything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CrucibleShape {
    Sphere { radius: f32 },
    BoxShape { half_extents: Vec3 },
    Plane { normal: Vec3, distance: f32 },
    Capsule { radius: f32, half_height: f32 },
}

/// A validated-on-build surface material description (friction / restitution /
/// density). Friction is stored and validated by physics but not yet solved
/// (documented), so the crucible's material station varies restitution + density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialSpec {
    pub friction: f32,
    pub restitution: f32,
    pub density: f32,
}

impl MaterialSpec {
    /// A neutral, fully elastic-free default (no bounce, unit density).
    pub const INELASTIC: MaterialSpec = MaterialSpec {
        friction: 0.5,
        restitution: 0.0,
        density: 1.0,
    };

    pub const fn new(friction: f32, restitution: f32, density: f32) -> Self {
        MaterialSpec {
            friction,
            restitution,
            density,
        }
    }

    /// The same material with a different restitution (the bounce ladder).
    pub const fn with_restitution(self, restitution: f32) -> Self {
        MaterialSpec { restitution, ..self }
    }

    /// The same material with a different density (mass via volume).
    pub const fn with_density(self, density: f32) -> Self {
        MaterialSpec { density, ..self }
    }
}

/// A full body description: kind, shape, local position, material, trigger flag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodySpec {
    pub kind: CrucibleKind,
    pub shape: CrucibleShape,
    pub local: Vec3,
    pub material: MaterialSpec,
    pub is_trigger: bool,
}

impl BodySpec {
    /// A dynamic sphere of `radius` and `mass` at `local`.
    pub fn dynamic_sphere(local: Vec3, radius: f32, mass: f32) -> Self {
        BodySpec {
            kind: CrucibleKind::Dynamic { mass },
            shape: CrucibleShape::Sphere { radius },
            local,
            material: MaterialSpec::INELASTIC,
            is_trigger: false,
        }
    }

    /// A dynamic axis-aligned box of `half_extents` and `mass` at `local`.
    pub fn dynamic_box(local: Vec3, half_extents: Vec3, mass: f32) -> Self {
        BodySpec {
            kind: CrucibleKind::Dynamic { mass },
            shape: CrucibleShape::BoxShape { half_extents },
            local,
            material: MaterialSpec::INELASTIC,
            is_trigger: false,
        }
    }

    /// A static ground plane with `normal` and signed `distance` from the origin.
    pub fn static_plane(normal: Vec3, distance: f32) -> Self {
        BodySpec {
            kind: CrucibleKind::Static,
            shape: CrucibleShape::Plane { normal, distance },
            local: Vec3::ZERO,
            material: MaterialSpec::INELASTIC,
            is_trigger: false,
        }
    }

    /// A static sphere of `radius` at `local` (a fixed query target).
    pub fn static_sphere(local: Vec3, radius: f32) -> Self {
        BodySpec {
            kind: CrucibleKind::Static,
            shape: CrucibleShape::Sphere { radius },
            local,
            material: MaterialSpec::INELASTIC,
            is_trigger: false,
        }
    }

    /// A static box (a wall / platform) of `half_extents` at `local`.
    pub fn static_box(local: Vec3, half_extents: Vec3) -> Self {
        BodySpec {
            kind: CrucibleKind::Static,
            shape: CrucibleShape::BoxShape { half_extents },
            local,
            material: MaterialSpec::INELASTIC,
            is_trigger: false,
        }
    }

    /// A kinematic box at `local` (ignores gravity; never integrated from velocity
    /// — the facade exposes no teleport, a documented gap).
    pub fn kinematic_box(local: Vec3, half_extents: Vec3) -> Self {
        BodySpec {
            kind: CrucibleKind::Kinematic,
            shape: CrucibleShape::BoxShape { half_extents },
            local,
            material: MaterialSpec::INELASTIC,
            is_trigger: false,
        }
    }

    /// A static sphere trigger (a sensor volume) at `local`.
    pub fn trigger_sphere(local: Vec3, radius: f32) -> Self {
        BodySpec {
            kind: CrucibleKind::Static,
            shape: CrucibleShape::Sphere { radius },
            local,
            material: MaterialSpec::INELASTIC,
            is_trigger: true,
        }
    }

    /// A capsule dynamic body (body-kind variety; no resting contact relied upon).
    pub fn dynamic_capsule(local: Vec3, radius: f32, half_height: f32, mass: f32) -> Self {
        BodySpec {
            kind: CrucibleKind::Dynamic { mass },
            shape: CrucibleShape::Capsule { radius, half_height },
            local,
            material: MaterialSpec::INELASTIC,
            is_trigger: false,
        }
    }

    /// This spec with a replaced material.
    pub fn with_material(mut self, material: MaterialSpec) -> Self {
        self.material = material;
        self
    }
}

/// A proving station: it populates the world, optionally scripts per-step
/// commands, and emits debug shapes describing the physics it is exercising.
///
/// A station never owns physics state; it asks the harness ([`CrucibleWorld`]) to
/// create and command bodies, and reads back snapshots/queries to describe what to
/// draw. The same station drives both the visible and the hidden replay world.
pub trait Station: std::fmt::Debug {
    /// The station's identity (and floor origin).
    fn id(&self) -> CrucibleStation;

    /// Create this station's bodies and colliders in `world`.
    fn populate(&self, world: &mut CrucibleWorld);

    /// Apply any scripted commands due at global `step` (forces, impulses,
    /// enable/disable). The default is a station with no scripted commands.
    fn script(&self, world: &mut CrucibleWorld, step: u64) {
        let _ = (world, step);
    }

    /// Debug shapes (queries, contacts, velocities, status markers) describing the
    /// station's current state, in world space. The default emits none.
    fn debug_shapes(&self, world: &CrucibleWorld) -> Vec<DebugShape> {
        let _ = world;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_tags_match_kind() {
        assert_eq!(CrucibleKind::Static.tag(), KindTag::Static);
        assert_eq!(CrucibleKind::Dynamic { mass: 2.0 }.tag(), KindTag::Dynamic);
        assert_eq!(CrucibleKind::Kinematic.tag(), KindTag::Kinematic);
    }

    #[test]
    fn material_builders_override_one_field_each() {
        let m = MaterialSpec::INELASTIC.with_restitution(0.8).with_density(3.0);
        assert_eq!(m.restitution, 0.8);
        assert_eq!(m.density, 3.0);
        assert_eq!(m.friction, MaterialSpec::INELASTIC.friction);
    }

    #[test]
    fn body_spec_constructors_set_the_expected_kind_and_shape() {
        let s = BodySpec::dynamic_sphere(Vec3::ZERO, 0.5, 1.0);
        assert_eq!(s.kind.tag(), KindTag::Dynamic);
        assert!(matches!(s.shape, CrucibleShape::Sphere { .. }));
        assert!(!s.is_trigger);

        let t = BodySpec::trigger_sphere(Vec3::ZERO, 1.0);
        assert!(t.is_trigger);
        assert_eq!(t.kind.tag(), KindTag::Static);

        let k = BodySpec::kinematic_box(Vec3::ZERO, Vec3::ONE);
        assert_eq!(k.kind.tag(), KindTag::Kinematic);

        let c = BodySpec::dynamic_capsule(Vec3::ZERO, 0.3, 0.6, 1.0);
        assert!(matches!(c.shape, CrucibleShape::Capsule { .. }));

        let p = BodySpec::static_plane(Vec3::UNIT_Y, 0.0);
        assert!(matches!(p.shape, CrucibleShape::Plane { .. }));

        let b = BodySpec::static_box(Vec3::ZERO, Vec3::ONE).with_material(MaterialSpec::INELASTIC);
        assert_eq!(b.material, MaterialSpec::INELASTIC);
        assert_eq!(b.kind.tag(), KindTag::Static);

        let db = BodySpec::dynamic_box(Vec3::ZERO, Vec3::ONE, 2.0);
        assert!(matches!(db.shape, CrucibleShape::BoxShape { .. }));
    }
}
