//! Per-tick scene synchronisation: write this tick's state into the retained
//! entities. The same state always produces the same submission.

use axiom::prelude::{
    Angle, Camera, Entity, PerspectiveProjection, RunningApp, Transform, Vec3, Visible,
};
use axiom_kernel::Meters;
use axiom_math::Quat;

use crate::camera::CameraPose;
use crate::debug::DebugMarker;
use crate::figure::{body_transform, kick_frame, world_parts, JointPose};
use crate::pitch::{NetImpulse, NetStrand};
use crate::play::{Phase, Session};

use super::{hidden, BendItScene, PREVIEW_SEGMENTS};

fn meters(v: f32) -> Meters {
    Meters::finite_or_zero(v)
}

/// Radius of one net strand, metres.
const STRAND: f32 = 0.018;

/// A strand's transform, displaced along `-Z` by the net's response.
pub fn strand_transform(strand: &NetStrand, displacement: f32) -> Transform {
    let scale = match strand.horizontal {
        true => Vec3::new(strand.length, STRAND, STRAND),
        false => Vec3::new(STRAND, strand.length, STRAND),
    };
    // A side panel's runs recede along Z rather than spanning X, so they are
    // turned a quarter turn about the vertical.
    let rotation = match (strand.horizontal, strand.panel) {
        (true, -1) | (true, 1) => Quat::from_euler_xyz(0.0, core::f32::consts::FRAC_PI_2, 0.0),
        _ => Quat::IDENTITY,
    };
    Transform::new(
        Vec3::new(
            strand.rest.x,
            strand.rest.y,
            strand.rest.z + displacement,
        ),
        rotation,
        scale,
    )
}

impl BendItScene {
    /// Sync everything to this tick.
    pub fn update(
        &mut self,
        app: &mut RunningApp,
        session: &Session,
        camera: &CameraPose,
        markers: &[DebugMarker],
    ) {
        self.sync_camera(app, camera);
        self.sync_figures(app, session);
        self.sync_ball(app, session);
        self.sync_net(app, session.net_impulse());
        self.sync_preview(app, session);
        self.sync_debug(app, markers);
    }

    fn sync_camera(&self, app: &mut RunningApp, camera: &CameraPose) {
        let pose = Transform::from_translation(camera.eye)
            .looking_at(camera.target, Vec3::UNIT_Y)
            .unwrap_or(Transform::from_translation(camera.eye));
        app.set_camera(
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(camera.fov_degrees.clamp(20.0, 110.0)),
                // The camera never sits closer than ~5 m to anything, so a
                // generous near plane buys back the depth precision the
                // near-coplanar pitch paint needs to stay steady at distance.
                near: meters(0.35),
                far: meters(320.0),
            }),
            pose,
        );
    }

    /// The kicker and the keeper: both the same figure, posed by their own
    /// system, both written through the same rig.
    fn sync_figures(&self, app: &mut RunningApp, session: &Session) {
        let phase = session.phase();
        let kick = session.kick();
        // The run-up starts when the run-up starts. Before that the kicker waits
        // at the top of it; after contact the follow-through keeps playing on
        // into the flight, which is the whole reason the pose is a function of a
        // tick rather than of a phase.
        let (ground, facing, pose) = match phase {
            Phase::Kicking | Phase::BallInFlight | Phase::Resolution => kick_frame(
                kick,
                session.swing(),
                session.kick_tick(),
                &session.tuning().kick,
            ),
            _ => kick.waiting(),
        };
        self.write_figure(app, &self.kicker_parts, ground, facing, &pose);

        let keeper = session.keeper().frame(&session.tuning().keeper);
        self.write_figure(
            app,
            &self.keeper_parts,
            keeper.ground,
            keeper.facing,
            &keeper.pose,
        );
    }

    fn write_figure(
        &self,
        app: &mut RunningApp,
        parts: &[Entity],
        ground: Vec3,
        facing: f32,
        pose: &JointPose,
    ) {
        let body = body_transform(ground, facing, pose);
        world_parts(&self.figure, body, pose)
            .iter()
            .zip(parts.iter())
            .for_each(|(part, entity)| {
                app.set(
                    *entity,
                    Transform::new(
                        part.transform.translation,
                        part.transform.rotation,
                        Vec3::new(
                            part.box_size.x * part.transform.scale.x,
                            part.box_size.y * part.transform.scale.y,
                            part.box_size.z * part.transform.scale.z,
                        ),
                    ),
                );
            });
    }

    fn sync_ball(&self, app: &mut RunningApp, session: &Session) {
        let ball = session.ball();
        let radius = session.tuning().flight.ball_radius;
        app.set(
            self.ball,
            Transform::new(
                ball.position,
                ball.orientation,
                Vec3::new(radius * 2.0, radius * 2.0, radius * 2.0),
            ),
        );
        // A dark band across the ball, turning with it: without it a smooth
        // sphere has no visible spin at all, and the spin is half the read on a
        // curled shot.
        app.set(
            self.ball_panel,
            Transform::new(
                ball.position,
                ball.orientation,
                Vec3::new(radius * 2.04, radius * 0.66, radius * 2.04),
            ),
        );
    }

    fn sync_net(&self, app: &mut RunningApp, impulse: Option<NetImpulse>) {
        self.net.iter().for_each(|(entity, strand)| {
            let displacement = impulse.map(|i| i.displacement(strand)).unwrap_or(0.0);
            app.set(*entity, strand_transform(strand, displacement));
        });
    }

    /// The authored path, drawn in the world as a tapering dotted ribbon, plus a
    /// marker on the point it finishes.
    fn sync_preview(&self, app: &mut RunningApp, session: &Session) {
        let show = session.phase().shows_preview();
        let trajectory = &session.shot().trajectory;
        self.preview.iter().enumerate().for_each(|(i, entity)| {
            let u = (i as f32 + 0.5) / PREVIEW_SEGMENTS as f32;
            let at = trajectory.at_progress(u);
            // Beads that grow along the flight read as depth rather than as a
            // row of identical dots, and the first few are kept small so the
            // ribbon appears to leave the ball rather than swallow it.
            let size = 0.055 + 0.075 * u;
            app.set(*entity, Visible(show));
            app.set(
                *entity,
                match show {
                    true => Transform::new(at, Quat::IDENTITY, Vec3::new(size, size, size)),
                    false => hidden(),
                },
            );
        });
        let target = session.shot().world_target;
        app.set(self.target_marker, Visible(show));
        app.set(
            self.target_marker,
            match show {
                true => Transform::new(
                    Vec3::new(target.x, target.y, target.z + 0.03),
                    Quat::IDENTITY,
                    Vec3::new(0.30, 0.30, 0.04),
                ),
                false => hidden(),
            },
        );
    }

    fn sync_debug(&self, app: &mut RunningApp, markers: &[DebugMarker]) {
        let mut primary = 0usize;
        let mut alternate = 0usize;
        markers.iter().for_each(|marker| {
            let (pool, cursor) = match marker.alternate {
                false => (&self.debug, &mut primary),
                true => (&self.debug_alt, &mut alternate),
            };
            if let Some(entity) = pool.get(*cursor) {
                app.set(*entity, marker.transform);
                app.set(*entity, Visible(true));
                *cursor += 1;
            }
        });
        self.debug.iter().skip(primary).for_each(|e| {
            app.set(*e, Visible(false));
        });
        self.debug_alt.iter().skip(alternate).for_each(|e| {
            app.set(*e, Visible(false));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::net_strands;

    #[test]
    fn a_strand_is_a_thin_box_along_its_own_axis() {
        let strands = net_strands();
        let horizontal = strands
            .iter()
            .find(|s| s.horizontal && s.panel == 0)
            .copied()
            .expect("a back-panel run");
        let vertical = strands
            .iter()
            .find(|s| !s.horizontal && s.panel == 0)
            .copied()
            .expect("a back-panel post");
        let h = strand_transform(&horizontal, 0.0);
        let v = strand_transform(&vertical, 0.0);
        assert!(h.scale.x > h.scale.y, "a run is long across");
        assert!(v.scale.y > v.scale.x, "a post is long up");
        assert_eq!(h.rotation, Quat::IDENTITY, "a back run spans X unrotated");
    }

    #[test]
    fn an_impulse_pushes_a_strand_deeper_into_the_goal() {
        let strand = net_strands()
            .into_iter()
            .find(|s| s.panel == 0)
            .expect("a back strand");
        let rest = strand_transform(&strand, 0.0);
        let struck = strand_transform(&strand, -0.4);
        assert!(struck.translation.z < rest.translation.z);
        assert_eq!(struck.scale, rest.scale);
    }

    #[test]
    fn a_side_run_is_turned_to_recede_into_the_goal() {
        let side = net_strands()
            .into_iter()
            .find(|s| s.horizontal && s.panel == 1)
            .expect("a side run");
        assert_ne!(strand_transform(&side, 0.0).rotation, Quat::IDENTITY);
    }
}
