//! The field generator: every static visible field piece, produced ONCE at
//! startup as plain placement data (unit engine primitives + two merged quad
//! meshes) that the composition layer spawns. Nothing here is rebuilt per
//! frame, and nothing is imported from an asset — turf, paint, numbers, and
//! goalposts are all procedural.

use axiom::prelude::{Transform, Vec3};
use axiom_math::Quat;

use super::coordinates::{FIELD_HALF_LENGTH, FIELD_HALF_WIDTH, GOAL_LINE_Z};
use super::markings::{build_markings, build_numbers, QuadBatch};

/// Which engine primitive a static piece uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMesh {
    Plane,
    Cube,
    Cylinder,
}

/// Which material slot a static piece uses (colors bound at scene install;
/// end-zone slots take their color from the team palettes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMaterial {
    Apron,
    TurfLight,
    TurfDark,
    HomeEndZone,
    AwayEndZone,
    White,
    Goalpost,
}

/// One static piece: a transform over a unit primitive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldPiece {
    pub transform: Transform,
    pub mesh: FieldMesh,
    pub material: FieldMaterial,
}

/// The complete generated field.
#[derive(Debug, Clone)]
pub struct FieldGeometry {
    pub pieces: Vec<FieldPiece>,
    /// All white line work, one merged mesh.
    pub markings: QuadBatch,
    /// Block field numbers, one merged mesh.
    pub numbers: QuadBatch,
}

fn plane(x: f32, y: f32, z: f32, sx: f32, sz: f32, material: FieldMaterial) -> FieldPiece {
    FieldPiece {
        transform: Transform::new(Vec3::new(x, y, z), Quat::IDENTITY, Vec3::new(sx, 1.0, sz)),
        mesh: FieldMesh::Plane,
        material,
    }
}

fn post(x: f32, y: f32, z: f32, scale: Vec3, rotation: Quat) -> FieldPiece {
    FieldPiece {
        transform: Transform::new(Vec3::new(x, y, z), rotation, scale),
        mesh: FieldMesh::Cylinder,
        material: FieldMaterial::Goalpost,
    }
}

/// Goalpost metrics (yards): crossbar at 10 ft, uprights to ~35 ft, 18.5 ft
/// apart, on each end line.
const CROSSBAR_Y: f32 = 10.0 / 3.0;
const UPRIGHT_TOP: f32 = 35.0 / 3.0;
const POST_HALF_SPAN: f32 = 18.5 / 6.0;

fn goalpost(end_sign: f32, pieces: &mut Vec<FieldPiece>) {
    let z = end_sign * (FIELD_HALF_LENGTH - 0.4);
    // Base stanchion.
    pieces.push(post(
        0.0,
        CROSSBAR_Y / 2.0,
        z,
        Vec3::new(0.3, CROSSBAR_Y, 0.3),
        Quat::IDENTITY,
    ));
    // Crossbar (cylinder axis Y rotated to lie along X).
    pieces.push(post(
        0.0,
        CROSSBAR_Y,
        z,
        Vec3::new(0.22, POST_HALF_SPAN * 2.0, 0.22),
        Quat::from_euler_xyz(0.0, 0.0, core::f32::consts::FRAC_PI_2),
    ));
    // Two uprights.
    let upright_h = UPRIGHT_TOP - CROSSBAR_Y;
    for side in [-1.0f32, 1.0] {
        pieces.push(post(
            side * POST_HALF_SPAN,
            CROSSBAR_Y + upright_h / 2.0,
            z,
            Vec3::new(0.18, upright_h, 0.18),
            Quat::IDENTITY,
        ));
    }
}

/// Generate the whole field. Alternating five-yard turf bands between the goal
/// lines, two team-colored end zones, an apron under everything, boundary +
/// yard-line paint, block numbers, and two goalposts.
pub fn generate_field() -> FieldGeometry {
    let mut pieces = Vec::new();

    // Apron: a larger dark surface under the field proper.
    pieces.push(plane(
        0.0,
        -0.02,
        0.0,
        FIELD_HALF_WIDTH * 2.0 + 14.0,
        FIELD_HALF_LENGTH * 2.0 + 14.0,
        FieldMaterial::Apron,
    ));

    // Twenty alternating five-yard turf bands between the goal lines.
    let mut band = 0;
    while band < 20 {
        let z0 = -GOAL_LINE_Z + band as f32 * 5.0;
        let material = if band % 2 == 0 {
            FieldMaterial::TurfLight
        } else {
            FieldMaterial::TurfDark
        };
        pieces.push(plane(
            0.0,
            0.0,
            z0 + 2.5,
            FIELD_HALF_WIDTH * 2.0,
            5.0,
            material,
        ));
        band += 1;
    }

    // End zones: home defends -Z, so the home-painted zone sits at -Z.
    pieces.push(plane(
        0.0,
        0.0,
        -(GOAL_LINE_Z + 5.0),
        FIELD_HALF_WIDTH * 2.0,
        10.0,
        FieldMaterial::HomeEndZone,
    ));
    pieces.push(plane(
        0.0,
        0.0,
        GOAL_LINE_Z + 5.0,
        FIELD_HALF_WIDTH * 2.0,
        10.0,
        FieldMaterial::AwayEndZone,
    ));

    goalpost(-1.0, &mut pieces);
    goalpost(1.0, &mut pieces);

    FieldGeometry {
        pieces,
        markings: build_markings(),
        numbers: build_numbers(),
    }
}
