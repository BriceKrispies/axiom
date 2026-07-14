//! Translation: physics body state → renderable instances. Colour encodes the
//! body's *role* (static / dynamic / kinematic / disabled / trigger); mesh +
//! scale encode its collider shape.

use axiom::prelude::{Transform, Vec3};

use crate::crucible_report::BodyState;
use crate::crucible_scenario::{CrucibleShape, KindTag};
use crate::debug_geometry::{palette, CrucibleMesh, RenderInstance};
use crate::physics_crucible_app::CrucibleBody;

/// Kept under the inter-cell spacing so the UV-grid floor shows between station
/// pads and each cell reads as its own square.
const PLANE_HALF_SPAN: f32 = 6.0;

/// A state with no matching registry entry is skipped (not created by the crucible).
pub fn render_instances(states: &[BodyState], registry: &[CrucibleBody]) -> Vec<RenderInstance> {
    states
        .iter()
        .filter_map(|state| {
            registry
                .iter()
                .find(|b| b.handle == state.handle)
                .map(|body| instance_for(state, body))
        })
        .collect()
}

/// A ground plane is a station *pad*, so it reads in the neutral divider colour
/// rather than the bright static-body colour, keeping the bodies on it legible.
fn instance_for(state: &BodyState, body: &CrucibleBody) -> RenderInstance {
    let color = match body.shape {
        CrucibleShape::Plane { .. } => palette::DIVIDER,
        _ => body_color(state.enabled, body.is_trigger, body.kind),
    };
    let (mesh, scale) = mesh_and_scale(body.shape);
    let transform = Transform::new(state.translation, Transform::IDENTITY.rotation, scale);
    RenderInstance::new(transform, mesh, color)
}

/// A disabled body reads as inert regardless of kind; a trigger reads as a
/// sensor; otherwise the kind decides.
fn body_color(enabled: bool, is_trigger: bool, kind: KindTag) -> [f32; 3] {
    if !enabled {
        palette::DISABLED
    } else if is_trigger {
        palette::TRIGGER
    } else {
        match kind {
            KindTag::Static => palette::STATIC,
            KindTag::Dynamic => palette::DYNAMIC,
            KindTag::Kinematic => palette::KINEMATIC,
        }
    }
}

/// A capsule has no primitive mesh, so it renders as a vertically-stretched
/// cube — an honest approximation, and the crucible never relies on capsule
/// resting contact.
fn mesh_and_scale(shape: CrucibleShape) -> (CrucibleMesh, Vec3) {
    match shape {
        CrucibleShape::Sphere { radius } => (
            CrucibleMesh::Sphere,
            Vec3::new(radius * 2.0, radius * 2.0, radius * 2.0),
        ),
        CrucibleShape::BoxShape { half_extents } => (
            CrucibleMesh::Cube,
            Vec3::new(
                half_extents.x * 2.0,
                half_extents.y * 2.0,
                half_extents.z * 2.0,
            ),
        ),
        CrucibleShape::Plane { .. } => (
            CrucibleMesh::Plane,
            Vec3::new(PLANE_HALF_SPAN * 2.0, 1.0, PLANE_HALF_SPAN * 2.0),
        ),
        CrucibleShape::Capsule {
            radius,
            half_height,
        } => (
            CrucibleMesh::Cube,
            Vec3::new(radius * 2.0, (half_height + radius) * 2.0, radius * 2.0),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_physics::PhysicsBodyHandle;

    use crate::crucible_station::CrucibleStation;

    fn body(handle: u64, kind: KindTag, shape: CrucibleShape, is_trigger: bool) -> CrucibleBody {
        CrucibleBody {
            handle: PhysicsBodyHandle::from_raw(handle),
            station: CrucibleStation::BodyBay,
            kind,
            shape,
            is_trigger,
        }
    }

    fn state(handle: u64, enabled: bool) -> BodyState {
        BodyState {
            handle: PhysicsBodyHandle::from_raw(handle),
            translation: Vec3::new(1.0, 2.0, 3.0),
            linear_velocity: Vec3::ZERO,
            rotation: [0.0, 0.0, 0.0, 1.0],
            angular: Vec3::ZERO,
            enabled,
        }
    }

    #[test]
    fn dynamic_sphere_renders_blue_at_its_position_scaled_by_diameter() {
        let reg = [body(
            1,
            KindTag::Dynamic,
            CrucibleShape::Sphere { radius: 0.5 },
            false,
        )];
        let out = render_instances(&[state(1, true)], &reg);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].mesh, CrucibleMesh::Sphere);
        assert_eq!(out[0].color, palette::DYNAMIC);
        assert_eq!(out[0].transform.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(out[0].transform.scale, Vec3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn disabled_overrides_kind_and_trigger_overrides_kind() {
        let disabled = [body(
            1,
            KindTag::Dynamic,
            CrucibleShape::Sphere { radius: 1.0 },
            false,
        )];
        assert_eq!(
            render_instances(&[state(1, false)], &disabled)[0].color,
            palette::DISABLED
        );
        let trigger = [body(
            2,
            KindTag::Static,
            CrucibleShape::Sphere { radius: 1.0 },
            true,
        )];
        assert_eq!(
            render_instances(&[state(2, true)], &trigger)[0].color,
            palette::TRIGGER
        );
    }

    #[test]
    fn kind_colours_static_and_kinematic_distinctly() {
        let s = [body(
            1,
            KindTag::Static,
            CrucibleShape::BoxShape {
                half_extents: Vec3::ONE,
            },
            false,
        )];
        assert_eq!(
            render_instances(&[state(1, true)], &s)[0].color,
            palette::STATIC
        );
        let k = [body(
            2,
            KindTag::Kinematic,
            CrucibleShape::BoxShape {
                half_extents: Vec3::ONE,
            },
            false,
        )];
        assert_eq!(
            render_instances(&[state(2, true)], &k)[0].color,
            palette::KINEMATIC
        );
    }

    #[test]
    fn plane_and_capsule_map_to_their_meshes() {
        let plane = [body(
            1,
            KindTag::Static,
            CrucibleShape::Plane {
                normal: Vec3::UNIT_Y,
                distance: 0.0,
            },
            false,
        )];
        let p = render_instances(&[state(1, true)], &plane);
        assert_eq!(p[0].mesh, CrucibleMesh::Plane);
        assert!(p[0].transform.scale.x > 1.0);

        let capsule = [body(
            2,
            KindTag::Dynamic,
            CrucibleShape::Capsule {
                radius: 0.5,
                half_height: 1.0,
            },
            false,
        )];
        let c = render_instances(&[state(2, true)], &capsule);
        assert_eq!(c[0].mesh, CrucibleMesh::Cube);
        // Stretched taller than wide: (half_height + radius)*2 = 3.0 vs radius*2 = 1.0.
        assert_eq!(c[0].transform.scale, Vec3::new(1.0, 3.0, 1.0));
    }

    #[test]
    fn an_unregistered_state_is_skipped() {
        let reg = [body(
            1,
            KindTag::Dynamic,
            CrucibleShape::Sphere { radius: 1.0 },
            false,
        )];
        let out = render_instances(&[state(2, true)], &reg);
        assert!(out.is_empty());
    }
}
