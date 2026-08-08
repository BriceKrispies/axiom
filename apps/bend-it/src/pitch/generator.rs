//! The static pitch, produced once at startup as placement data.
//!
//! Everything the environment is made of is here and nothing is imported: an
//! apron, mown turf bands, the penalty-area paint, the goal frame, hoardings and
//! a raked stand. The mown-band idea and the raked bowl are lifted straight from
//! the football field this game's environment descends from — bands close enough
//! in value to read as *distance* rather than as stripes are what give a flat
//! Lambert pitch its perspective — but every football marking is gone: no yard
//! numbers, no hashes, no uprights, no end zones.

use axiom::prelude::{Transform, Vec3};
use axiom_math::Quat;

use super::coordinates::{
    BEHIND_GOAL, GOAL_AREA_DEPTH, GOAL_AREA_HALF_WIDTH, GOAL_HALF_WIDTH, GOAL_HEIGHT, LINE_WIDTH,
    PAINT_Y, PENALTY_AREA_DEPTH, PENALTY_AREA_HALF_WIDTH, PENALTY_ARC_RADIUS, PENALTY_SPOT_Z,
    PITCH_DEPTH, PITCH_HALF_WIDTH, POST_RADIUS,
};

/// Which engine primitive a static piece uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitchMesh {
    Plane,
    Cube,
    Cylinder,
}

/// Which material slot a static piece uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitchMaterial {
    Apron,
    TurfLight,
    TurfDark,
    Paint,
    Frame,
    Hoarding,
    Stand,
    Crowd,
}

/// One static piece: a transform over a unit primitive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchPiece {
    pub transform: Transform,
    pub mesh: PitchMesh,
    pub material: PitchMaterial,
}

/// Width of one mown band, metres.
const BAND_METRES: f32 = 4.4;

fn plane(centre: Vec3, sx: f32, sz: f32, material: PitchMaterial) -> PitchPiece {
    PitchPiece {
        transform: Transform::new(centre, Quat::IDENTITY, Vec3::new(sx, 1.0, sz)),
        mesh: PitchMesh::Plane,
        material,
    }
}

fn slab(centre: Vec3, size: Vec3, material: PitchMaterial) -> PitchPiece {
    PitchPiece {
        transform: Transform::new(centre, Quat::IDENTITY, size),
        mesh: PitchMesh::Cube,
        material,
    }
}

/// A painted line as a flat quad floated just above the turf.
fn line(centre: Vec3, sx: f32, sz: f32) -> PitchPiece {
    plane(
        Vec3::new(centre.x, PAINT_Y, centre.z),
        sx,
        sz,
        PitchMaterial::Paint,
    )
}

/// A rectangle drawn as its four sides (the box markings).
fn box_outline(half_width: f32, depth: f32, out: &mut Vec<PitchPiece>) {
    out.push(line(
        Vec3::new(0.0, 0.0, depth),
        half_width * 2.0 + LINE_WIDTH,
        LINE_WIDTH,
    ));
    [-1.0f32, 1.0].into_iter().for_each(|side| {
        out.push(line(
            Vec3::new(side * half_width, 0.0, depth * 0.5),
            LINE_WIDTH,
            depth,
        ));
    });
}

/// The D: the part of the 9.15 m circle around the spot that lies outside the
/// penalty area, as short chord segments.
fn penalty_arc(out: &mut Vec<PitchPiece>) {
    const SEGMENTS: usize = 15;
    // Half-angle, measured from straight up-pitch, at which the circle leaves
    // the box. Outside the box means z > PENALTY_AREA_DEPTH.
    let cut = ((PENALTY_AREA_DEPTH - PENALTY_SPOT_Z) / PENALTY_ARC_RADIUS).clamp(-1.0, 1.0);
    let half_span = cut.acos();
    (0..SEGMENTS).for_each(|i| {
        let t0 = -half_span + (i as f32 / SEGMENTS as f32) * 2.0 * half_span;
        let t1 = -half_span + ((i + 1) as f32 / SEGMENTS as f32) * 2.0 * half_span;
        let p0 = Vec3::new(
            PENALTY_ARC_RADIUS * t0.sin(),
            PAINT_Y,
            PENALTY_SPOT_Z + PENALTY_ARC_RADIUS * t0.cos(),
        );
        let p1 = Vec3::new(
            PENALTY_ARC_RADIUS * t1.sin(),
            PAINT_Y,
            PENALTY_SPOT_Z + PENALTY_ARC_RADIUS * t1.cos(),
        );
        let mid = p0.add(p1).mul_scalar(0.5);
        let span = p1.subtract(p0);
        let yaw = span.x.atan2(span.z);
        out.push(PitchPiece {
            transform: Transform::new(
                mid,
                Quat::from_euler_xyz(0.0, yaw, 0.0),
                Vec3::new(LINE_WIDTH, 1.0, span.length()),
            ),
            mesh: PitchMesh::Plane,
            material: PitchMaterial::Paint,
        });
    });
}

/// The goal frame: two posts and a crossbar, as cylinders matching the collision
/// capsules in [`super::goal`] exactly.
fn goal_frame(out: &mut Vec<PitchPiece>) {
    let top = GOAL_HEIGHT - POST_RADIUS;
    [-1.0f32, 1.0].into_iter().for_each(|side| {
        out.push(PitchPiece {
            transform: Transform::new(
                Vec3::new(side * GOAL_HALF_WIDTH, top * 0.5, 0.0),
                Quat::IDENTITY,
                Vec3::new(POST_RADIUS * 2.0, top, POST_RADIUS * 2.0),
            ),
            mesh: PitchMesh::Cylinder,
            material: PitchMaterial::Frame,
        });
    });
    out.push(PitchPiece {
        transform: Transform::new(
            Vec3::new(0.0, GOAL_HEIGHT, 0.0),
            Quat::from_euler_xyz(0.0, 0.0, core::f32::consts::FRAC_PI_2),
            Vec3::new(POST_RADIUS * 2.0, GOAL_HALF_WIDTH * 2.0, POST_RADIUS * 2.0),
        ),
        mesh: PitchMesh::Cylinder,
        material: PitchMaterial::Frame,
    });
}

/// Hoardings and a raked stand behind the goal, plus low side stands. Boxes are
/// the honest proxy for a crowd here: what matters is that the horizon behind the
/// goal is *closed*, so the goal reads against a wall rather than against sky.
fn surroundings(out: &mut Vec<PitchPiece>) {
    let back = -(BEHIND_GOAL - 1.6);
    out.push(slab(
        Vec3::new(0.0, 0.55, back),
        Vec3::new(PITCH_HALF_WIDTH * 1.4, 1.1, 0.35),
        PitchMaterial::Hoarding,
    ));
    (0..4).for_each(|tier| {
        let t = tier as f32;
        let height = 2.0 + t * 1.35;
        let z = back - 3.2 - t * 3.0;
        out.push(slab(
            Vec3::new(0.0, height * 0.5, z),
            Vec3::new(PITCH_HALF_WIDTH * 1.9, height, 2.6),
            PitchMaterial::Stand,
        ));
        out.push(slab(
            Vec3::new(0.0, height + 0.42, z),
            Vec3::new(PITCH_HALF_WIDTH * 1.9, 0.85, 2.6),
            PitchMaterial::Crowd,
        ));
        [-1.0f32, 1.0].into_iter().for_each(|side| {
            let x = side * (PITCH_HALF_WIDTH + 4.5 + t * 3.0);
            out.push(slab(
                Vec3::new(x, height * 0.5, PITCH_DEPTH * 0.35),
                Vec3::new(2.6, height, PITCH_DEPTH * 1.3),
                PitchMaterial::Stand,
            ));
            out.push(slab(
                Vec3::new(x, height + 0.42, PITCH_DEPTH * 0.35),
                Vec3::new(2.6, 0.85, PITCH_DEPTH * 1.3),
                PitchMaterial::Crowd,
            ));
        });
    });
}

/// Generate the whole static pitch.
pub fn generate_pitch() -> Vec<PitchPiece> {
    let mut out = Vec::new();

    out.push(plane(
        Vec3::new(0.0, -0.03, PITCH_DEPTH * 0.4),
        PITCH_HALF_WIDTH * 3.4,
        (PITCH_DEPTH + BEHIND_GOAL) * 2.2,
        PitchMaterial::Apron,
    ));

    // Mown bands running across the pitch, so the eye reads depth toward the
    // goal. They start behind the goal line and run up the half.
    let start = -BEHIND_GOAL;
    let band_count = ((PITCH_DEPTH + BEHIND_GOAL) / BAND_METRES).ceil() as i32;
    (0..band_count).for_each(|band| {
        let z0 = start + band as f32 * BAND_METRES;
        let material = [PitchMaterial::TurfLight, PitchMaterial::TurfDark]
            [(band.rem_euclid(2)) as usize];
        out.push(plane(
            Vec3::new(0.0, 0.0, z0 + BAND_METRES * 0.5),
            PITCH_HALF_WIDTH * 2.0,
            BAND_METRES,
            material,
        ));
    });

    // The markings: goal line, six-yard box, penalty area, spot and D.
    out.push(line(Vec3::ZERO, PITCH_HALF_WIDTH * 2.0, LINE_WIDTH));
    box_outline(GOAL_AREA_HALF_WIDTH, GOAL_AREA_DEPTH, &mut out);
    box_outline(PENALTY_AREA_HALF_WIDTH, PENALTY_AREA_DEPTH, &mut out);
    penalty_arc(&mut out);
    out.push(PitchPiece {
        transform: Transform::new(
            Vec3::new(0.0, PAINT_Y, PENALTY_SPOT_Z),
            Quat::IDENTITY,
            Vec3::new(0.24, 0.01, 0.24),
        ),
        mesh: PitchMesh::Cylinder,
        material: PitchMaterial::Paint,
    });
    // The touchlines, far enough out to close the frame in a wide shot.
    [-1.0f32, 1.0].into_iter().for_each(|side| {
        out.push(line(
            Vec3::new(side * PITCH_HALF_WIDTH, 0.0, PITCH_DEPTH * 0.5),
            LINE_WIDTH,
            PITCH_DEPTH,
        ));
    });

    goal_frame(&mut out);
    surroundings(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pitch_is_built_once_and_contains_every_kind_of_piece() {
        let pieces = generate_pitch();
        let has = |m: PitchMaterial| pieces.iter().any(|p| p.material == m);
        assert!(has(PitchMaterial::Apron));
        assert!(has(PitchMaterial::TurfLight));
        assert!(has(PitchMaterial::TurfDark));
        assert!(has(PitchMaterial::Paint));
        assert!(has(PitchMaterial::Frame));
        assert!(has(PitchMaterial::Hoarding));
        assert!(has(PitchMaterial::Stand));
        assert!(has(PitchMaterial::Crowd));
        assert!(pieces
            .iter()
            .any(|p| p.mesh == PitchMesh::Cylinder && p.material == PitchMaterial::Frame));
        assert!(pieces.iter().any(|p| p.mesh == PitchMesh::Cube));
        // Deterministic: the same call yields the same pitch.
        assert_eq!(pieces, generate_pitch());
    }

    #[test]
    fn the_paint_floats_above_the_turf_and_the_frame_matches_the_capsules() {
        let pieces = generate_pitch();
        pieces
            .iter()
            .filter(|p| p.material == PitchMaterial::Paint)
            .for_each(|p| assert!(p.transform.translation.y > 0.0));
        let posts: Vec<_> = pieces
            .iter()
            .filter(|p| p.material == PitchMaterial::Frame)
            .collect();
        assert_eq!(posts.len(), 3);
        assert!(posts
            .iter()
            .any(|p| (p.transform.translation.x + GOAL_HALF_WIDTH).abs() < 1.0e-5));
        assert!(posts
            .iter()
            .any(|p| (p.transform.translation.y - GOAL_HEIGHT).abs() < 1.0e-5));
    }

    #[test]
    fn the_d_only_exists_outside_the_penalty_area() {
        let mut arc = Vec::new();
        penalty_arc(&mut arc);
        assert!(!arc.is_empty());
        arc.iter().for_each(|p| {
            assert!(
                p.transform.translation.z > PENALTY_AREA_DEPTH - 0.5,
                "the arc segment at z={} is inside the box",
                p.transform.translation.z
            );
        });
    }
}
