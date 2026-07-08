//! The four procedural sports balls: one preset per ball, holding the physics
//! identity (mass / restitution / friction / collider radius) and the visual
//! identity (ellipsoid scale + which baked texture skins it). Every ball is a
//! real `axiom-physics` dynamic sphere; the football's elongation is visual-only
//! because the physics module's contact solver supports sphere colliders against
//! every arena surface (an ellipsoid/convex collider does not exist yet — see
//! the module docs in `mod.rs`).

use axiom::prelude::Vec3;

/// Which ball (also indexes the baked texture set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BallKind {
    Soccer,
    Football,
    Bowling,
    Baseball,
}

/// One ball's full recipe: physics material + collider + visual shape + spawn.
#[derive(Debug, Clone, Copy)]
pub struct BallPreset {
    pub kind: BallKind,
    /// The player-facing name (HUD "Pick up …" label + tests).
    pub name: &'static str,
    /// Physics sphere collider radius.
    pub radius: f32,
    /// Visual ellipsoid scale (full extents; the unit sphere mesh has diameter 1).
    pub visual_scale: Vec3,
    /// Mass (kg-ish world units).
    pub mass: f32,
    /// Surface restitution (0 dead … 1 superball).
    pub restitution: f32,
    /// Surface friction.
    pub friction: f32,
    /// Spawn position of the body center (the lineup near the player spawn).
    pub spawn: Vec3,
    /// Extra pitch (radians about X) baked into the spawn rotation — the football
    /// lies on its side (its long visual axis is local +Y, rotated to +Z).
    pub spawn_pitch: f32,
}

/// The lineup: soccer, football, bowling, baseball — side by side in front of
/// the player spawn, each resting on the field (center at collider radius).
pub const BALLS: [BallPreset; 4] = [
    BallPreset {
        kind: BallKind::Soccer,
        name: "Soccer Ball",
        radius: 0.35,
        visual_scale: Vec3::new(0.70, 0.70, 0.70),
        mass: 0.45,
        restitution: 0.65,
        friction: 0.45,
        spawn: Vec3::new(-2.4, 0.36, 2.5),
        spawn_pitch: 0.0,
    },
    BallPreset {
        kind: BallKind::Football,
        name: "Football",
        radius: 0.17,
        // Long axis is local +Y (so the tips sit at the texture poles where the
        // stripes bake); the spawn pitch lays it down along the field.
        visual_scale: Vec3::new(0.34, 0.58, 0.34),
        mass: 0.42,
        restitution: 0.45,
        friction: 0.55,
        spawn: Vec3::new(-0.8, 0.18, 2.5),
        spawn_pitch: core::f32::consts::FRAC_PI_2,
    },
    BallPreset {
        kind: BallKind::Bowling,
        name: "Bowling Ball",
        radius: 0.42,
        visual_scale: Vec3::new(0.84, 0.84, 0.84),
        mass: 7.0,
        restitution: 0.2,
        friction: 0.35,
        spawn: Vec3::new(0.8, 0.43, 2.5),
        spawn_pitch: 0.0,
    },
    BallPreset {
        kind: BallKind::Baseball,
        name: "Baseball",
        radius: 0.12,
        visual_scale: Vec3::new(0.24, 0.24, 0.24),
        mass: 0.15,
        restitution: 0.55,
        friction: 0.35,
        spawn: Vec3::new(2.4, 0.13, 2.5),
        spawn_pitch: 0.0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ball_preset_is_distinct_in_mass_restitution_and_friction() {
        for (i, a) in BALLS.iter().enumerate() {
            for b in BALLS.iter().skip(i + 1) {
                assert!(
                    (a.mass, a.restitution, a.friction) != (b.mass, b.restitution, b.friction),
                    "{} and {} share a physics identity",
                    a.name,
                    b.name
                );
            }
        }
    }

    #[test]
    fn the_bowling_ball_is_much_heavier_than_the_baseball() {
        let bowling = BALLS.iter().find(|b| b.kind == BallKind::Bowling).unwrap();
        let baseball = BALLS.iter().find(|b| b.kind == BallKind::Baseball).unwrap();
        assert!(bowling.mass > baseball.mass * 20.0);
        assert!(bowling.restitution < baseball.restitution);
    }

    #[test]
    fn balls_spawn_resting_on_the_field() {
        for ball in &BALLS {
            assert!(
                (ball.spawn.y - ball.radius).abs() < 0.03,
                "{} spawns at its collider radius above the field",
                ball.name
            );
        }
    }

    #[test]
    fn the_football_is_visually_elongated() {
        let football = BALLS.iter().find(|b| b.kind == BallKind::Football).unwrap();
        assert!(football.visual_scale.y > football.visual_scale.x * 1.5);
        assert!(football.spawn_pitch > 0.0, "the football spawns lying on its side");
    }
}
