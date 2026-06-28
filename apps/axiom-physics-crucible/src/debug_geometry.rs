//! App-space debug geometry: the neutral renderable vocabulary the crucible draws.
//!
//! The renderer has three primitive meshes (cube, sphere, plane) and no line or
//! text primitive (see `README.md`), so every debug overlay here is expressed as
//! those primitives: a line/ray/normal/velocity is a row of small marker cubes
//! sampled along the vector (orientation-free, so no quaternion math is needed),
//! a contact point is one small cube, an overlap volume is a sphere mesh. This
//! module owns *only* the description; spawning the actual scene entities is done
//! by the app's `setup` closure from these descriptors. Nothing here touches
//! physics — it is pure app render data.

use axiom::prelude::{Transform, Vec3};

/// The renderer's three primitive meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrucibleMesh {
    Cube,
    Sphere,
    Plane,
}

/// One renderable instance: a primitive mesh, a world transform, and a linear-RGB
/// colour. The app's `setup` turns each of these into a scene entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderInstance {
    pub transform: Transform,
    pub mesh: CrucibleMesh,
    pub color: [f32; 3],
}

impl RenderInstance {
    pub fn new(transform: Transform, mesh: CrucibleMesh, color: [f32; 3]) -> Self {
        RenderInstance {
            transform,
            mesh,
            color,
        }
    }

    /// A small axis-aligned marker cube at `position`.
    pub fn marker(position: Vec3, color: [f32; 3], size: f32) -> Self {
        RenderInstance::new(
            Transform::new(position, axiom::prelude::Transform::IDENTITY.rotation, Vec3::new(size, size, size)),
            CrucibleMesh::Cube,
            color,
        )
    }
}

/// The crucible's fixed colour palette (linear RGB). Visual conventions are
/// documented in `README.md`.
pub mod palette {
    pub const STATIC: [f32; 3] = [0.55, 0.57, 0.60];
    pub const DYNAMIC: [f32; 3] = [0.30, 0.55, 0.95];
    pub const KINEMATIC: [f32; 3] = [0.95, 0.60, 0.15];
    pub const DISABLED: [f32; 3] = [0.38, 0.38, 0.40];
    pub const TRIGGER: [f32; 3] = [0.30, 0.85, 0.50];
    pub const CONTACT_POINT: [f32; 3] = [0.95, 0.20, 0.20];
    pub const CONTACT_NORMAL: [f32; 3] = [0.95, 0.85, 0.20];
    pub const RAY: [f32; 3] = [0.20, 0.90, 0.90];
    pub const RAY_HIT: [f32; 3] = [0.95, 0.30, 0.90];
    pub const OVERLAP: [f32; 3] = [0.25, 0.80, 0.80];
    pub const VELOCITY: [f32; 3] = [0.92, 0.92, 0.92];
    pub const FLOOR: [f32; 3] = [0.16, 0.17, 0.20];
    pub const DIVIDER: [f32; 3] = [0.26, 0.27, 0.32];
    pub const LABEL: [f32; 3] = [0.85, 0.86, 0.90];
    pub const REPLAY_OK: [f32; 3] = [0.20, 0.85, 0.30];
    pub const REPLAY_FAIL: [f32; 3] = [0.90, 0.20, 0.20];
}

/// The full ordered palette as a flat array. The live browser demo registers
/// exactly these as its material set every frame, so material ids stay stable
/// across the per-frame scene re-authors (the backend uploads materials once at
/// startup and maps each draw's material id back to that upload).
pub const LIVE_PALETTE: [[f32; 3]; 16] = [
    palette::STATIC,
    palette::DYNAMIC,
    palette::KINEMATIC,
    palette::DISABLED,
    palette::TRIGGER,
    palette::CONTACT_POINT,
    palette::CONTACT_NORMAL,
    palette::RAY,
    palette::RAY_HIT,
    palette::OVERLAP,
    palette::VELOCITY,
    palette::FLOOR,
    palette::DIVIDER,
    palette::LABEL,
    palette::REPLAY_OK,
    palette::REPLAY_FAIL,
];

/// The index of `color` within [`LIVE_PALETTE`] (0 if it is not a palette colour).
pub fn palette_index(color: [f32; 3]) -> usize {
    LIVE_PALETTE.iter().position(|c| *c == color).unwrap_or(0)
}

/// A debug shape a station emits each frame (queries, contacts, velocities).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DebugShape {
    /// A small marker at a world contact point.
    ContactPoint { position: Vec3 },
    /// A contact normal, drawn from `origin` along `direction` for `length`.
    ContactNormal {
        origin: Vec3,
        direction: Vec3,
        length: f32,
    },
    /// A ray, drawn from `origin` along `direction` for `length`, with an
    /// optional hit marker at `hit`.
    Ray {
        origin: Vec3,
        direction: Vec3,
        length: f32,
        hit: Option<Vec3>,
    },
    /// An overlap query sphere.
    OverlapSphere { center: Vec3, radius: f32 },
    /// A velocity arrow at `origin` along `velocity` (scaled down for legibility).
    Velocity { origin: Vec3, velocity: Vec3 },
    /// A generic label / status marker.
    Marker {
        position: Vec3,
        color: [f32; 3],
        size: f32,
    },
}

/// The small-marker size for sampled lines/points.
const SAMPLE_SIZE: f32 = 0.12;
/// How many marker cubes sample a line segment.
const SAMPLES: u32 = 6;

/// Expand a debug shape into concrete render instances using only the three
/// primitive meshes.
pub fn debug_instances(shape: DebugShape) -> Vec<RenderInstance> {
    match shape {
        DebugShape::ContactPoint { position } => {
            vec![RenderInstance::marker(position, palette::CONTACT_POINT, 0.18)]
        }
        DebugShape::ContactNormal {
            origin,
            direction,
            length,
        } => sampled_line(origin, direction, length, palette::CONTACT_NORMAL, SAMPLE_SIZE),
        DebugShape::Ray {
            origin,
            direction,
            length,
            hit,
        } => {
            let mut line = sampled_line(origin, direction, length, palette::RAY, SAMPLE_SIZE);
            hit.into_iter().for_each(|h| {
                line.push(RenderInstance::marker(h, palette::RAY_HIT, 0.22));
            });
            line
        }
        DebugShape::OverlapSphere { center, radius } => vec![RenderInstance::new(
            Transform::new(
                center,
                Transform::IDENTITY.rotation,
                Vec3::new(radius * 2.0, radius * 2.0, radius * 2.0),
            ),
            CrucibleMesh::Sphere,
            palette::OVERLAP,
        )],
        DebugShape::Velocity { origin, velocity } => {
            sampled_line(origin, velocity, 0.25, palette::VELOCITY, 0.10)
        }
        DebugShape::Marker {
            position,
            color,
            size,
        } => vec![RenderInstance::marker(position, color, size)],
    }
}

/// Sample `SAMPLES` marker cubes evenly along `origin -> origin + direction*length`.
/// A zero/degenerate direction collapses to a single marker at the origin.
fn sampled_line(
    origin: Vec3,
    direction: Vec3,
    length: f32,
    color: [f32; 3],
    size: f32,
) -> Vec<RenderInstance> {
    let len = direction.length();
    let unit = if len > 1.0e-6 {
        direction.mul_scalar(1.0 / len)
    } else {
        Vec3::ZERO
    };
    (0..SAMPLES)
        .map(|i| {
            let t = (i as f32) / ((SAMPLES - 1) as f32);
            let position = origin.add(unit.mul_scalar(t * length));
            RenderInstance::marker(position, color, size)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_point_is_one_marker_at_the_point() {
        let shapes = debug_instances(DebugShape::ContactPoint {
            position: Vec3::new(1.0, 2.0, 3.0),
        });
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].transform.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(shapes[0].mesh, CrucibleMesh::Cube);
        assert_eq!(shapes[0].color, palette::CONTACT_POINT);
    }

    #[test]
    fn a_ray_with_a_hit_samples_the_line_and_marks_the_hit() {
        let shapes = debug_instances(DebugShape::Ray {
            origin: Vec3::ZERO,
            direction: Vec3::new(1.0, 0.0, 0.0),
            length: 5.0,
            hit: Some(Vec3::new(3.0, 0.0, 0.0)),
        });
        // SAMPLES line markers + 1 hit marker.
        assert_eq!(shapes.len() as u32, SAMPLES + 1);
        // The last instance is the hit marker.
        assert_eq!(shapes[shapes.len() - 1].color, palette::RAY_HIT);
        assert_eq!(
            shapes[shapes.len() - 1].transform.translation,
            Vec3::new(3.0, 0.0, 0.0)
        );
        // The first sample sits at the ray origin.
        assert_eq!(shapes[0].transform.translation, Vec3::ZERO);
    }

    #[test]
    fn an_overlap_sphere_is_a_scaled_sphere_mesh() {
        let shapes = debug_instances(DebugShape::OverlapSphere {
            center: Vec3::new(0.0, 1.0, 0.0),
            radius: 1.5,
        });
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].mesh, CrucibleMesh::Sphere);
        assert_eq!(shapes[0].transform.scale, Vec3::new(3.0, 3.0, 3.0));
    }

    #[test]
    fn a_degenerate_line_collapses_to_the_origin() {
        let shapes = sampled_line(Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO, 4.0, palette::RAY, 0.1);
        assert_eq!(shapes.len() as u32, SAMPLES);
        shapes
            .iter()
            .for_each(|s| assert_eq!(s.transform.translation, Vec3::new(2.0, 0.0, 0.0)));
    }
}
