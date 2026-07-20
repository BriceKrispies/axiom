//! Per-tick scene synchronization: write this tick's immutable snapshot,
//! juice effects, camera pose, and debug markers into the retained entities.
//! The same snapshot + same effect state always produces the same submission.

use axiom::prelude::{Angle, Camera, Entity, PerspectiveProjection, RunningApp, Transform, Vec3};
use axiom_kernel::Meters;

use crate::camera::CameraPose;
use crate::debug::DebugInstance;
use crate::football::model::{
    ball_transform, cradled_ball_transform, lace_transform, throw_ready_ball_transform,
};
use crate::player::animation::BallHold;
use crate::player::model::{R_FOREARM, R_HAND};
use crate::player::rig;
use crate::presentation::juice::JuiceStack;
use crate::presentation::particles::{effect_instances, trail_instances};
use crate::presentation::snapshot::PresentationSnapshot;
use crate::presentation::PlayerPose;
use crate::scene::{hidden_transform, EndZoneScene};

fn meters(v: f32) -> Meters {
    Meters::finite_or_zero(v)
}

impl EndZoneScene {
    /// Sync the whole scene to this tick's snapshot + effects + camera.
    pub fn update(
        &mut self,
        app: &mut RunningApp,
        snapshot: &PresentationSnapshot,
        poses: &[PlayerPose],
        juice: &JuiceStack,
        camera: &CameraPose,
        debug_markers: &[DebugInstance],
    ) {
        // Camera: the app owns the pose every frame (never the engine's
        // first-person controller, which would overwrite it).
        let pose = Transform::from_translation(camera.eye)
            .looking_at(camera.target, Vec3::UNIT_Y)
            .unwrap_or(Transform::from_translation(camera.eye));
        app.set_camera(
            // The camera never sits closer than ~6 yd to any subject (the
            // tightest follow is 9 yd), so a 0.1 yd near plane only burned
            // depth precision and let the near-coplanar field paint (turf at
            // y=0, yard lines at y≈0.03) z-fight and flicker at distance. A
            // 0.5 yd near plane recovers ~5× the depth resolution with margin
            // to spare, which is what keeps the far lines steady.
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(camera.fov_degrees.clamp(20.0, 110.0)),
                near: meters(0.5),
                far: meters(400.0),
            }),
            pose,
        );

        // Field wobble (decays to exactly zero → base transforms restored).
        let wobble = juice.field_wobble(snapshot.tick);
        for (entity, base) in &self.turf {
            let mut t = *base;
            t.translation = Vec3::new(t.translation.x, t.translation.y + wobble, t.translation.z);
            app.set(*entity, t);
        }

        // Players: state-driven procedural pose → figure boxes. The ball carrier
        // holds the ball throw-ready by the ear (quarterback in the pocket) or
        // cradled in the crook (scrambling QB / any runner); capture the joint
        // the ball pins to so it can be placed against that arm/hand below.
        let mut ball_anchor: Option<(BallHold, Transform, f32)> = None;
        for (index, view) in snapshot.players.iter().enumerate() {
            let player_pose = &poses[index];
            let hold = player_pose.hold;
            let pose = &player_pose.pose;
            let squash = juice.squash_for(view.id, snapshot.tick);
            let body = rig::body_transform(view.pos, view.facing, pose, squash);
            let parts = rig::world_parts(&self.figure, body, pose);
            let anchor = match hold {
                BallHold::Cradle => parts.get(R_FOREARM).map(|p| p.transform),
                BallHold::ThrowReady => parts.get(R_HAND).map(|p| p.transform),
                BallHold::None => None,
            };
            if let Some(transform) = anchor {
                ball_anchor = Some((hold, transform, view.facing));
            }
            for (part_index, part) in parts.iter().enumerate() {
                let scale = Vec3::new(
                    part.box_size.x * part.transform.scale.x,
                    part.box_size.y * part.transform.scale.y,
                    part.box_size.z * part.transform.scale.z,
                );
                app.set(
                    self.player_parts[index][part_index],
                    Transform::new(part.transform.translation, part.transform.rotation, scale),
                );
            }
        }

        // The football + lace ridge. A throw-ready hold pins the ball to the
        // raised throwing hand; a cradle pins it to the forearm crook; otherwise
        // (in flight, loose, or carried in a self-posing anim) it follows its
        // sim transform.
        let ball_world = match ball_anchor {
            Some((BallHold::ThrowReady, hand, facing)) => throw_ready_ball_transform(&hand, facing),
            Some((_, forearm, _)) => cradled_ball_transform(&forearm),
            None => ball_transform(&snapshot.ball, snapshot.carrier().map(|c| c.facing)),
        };
        app.set(self.ball, ball_world);
        app.set(self.lace, lace_transform(&ball_world));

        // The line-to-gain marker: a thin bright bar spanning the field at the
        // to-gain yard line (hidden when no drive is active).
        let to_gain = snapshot
            .to_gain_z
            .map(|z| {
                Transform::new(
                    Vec3::new(0.0, 0.06, z),
                    axiom_math::Quat::IDENTITY,
                    Vec3::new(crate::field::FIELD_HALF_WIDTH * 2.0, 0.12, 0.5),
                )
            })
            .unwrap_or_else(hidden_transform);
        app.set(self.line_to_gain, to_gain);

        // Juice instances into the pools.
        self.juice_scratch.clear();
        for effect in juice.effects() {
            effect_instances(
                effect,
                snapshot.tick,
                juice.tuning(),
                &mut self.juice_scratch,
            );
        }
        trail_instances(juice.trail(), &mut self.juice_scratch);
        let scratch = core::mem::take(&mut self.juice_scratch);
        assign_pool(app, &self.juice_pool, &scratch, |i| {
            (i.transform, i.material)
        });
        self.juice_scratch = scratch;

        // Debug markers into the pools.
        assign_pool(app, &self.debug_pool, debug_markers, |m| {
            (m.transform, m.material)
        });
    }
}

/// Fill a typed pool from an instance list: each instance takes the next
/// free pool slot of its material; instances beyond a material's pool size
/// are dropped (bounded by construction); unused slots hide.
fn assign_pool<I, M: PartialEq + Copy>(
    app: &mut RunningApp,
    pool: &[(Entity, M)],
    instances: &[I],
    project: impl Fn(&I) -> (Transform, M),
) {
    let mut used = vec![false; pool.len()];
    for instance in instances {
        let (transform, material) = project(instance);
        let slot = pool
            .iter()
            .enumerate()
            .position(|(index, (_, m))| !used[index] && *m == material);
        if let Some(index) = slot {
            used[index] = true;
            app.set(pool[index].0, transform);
        }
    }
    for (index, (entity, _)) in pool.iter().enumerate() {
        if !used[index] {
            app.set(*entity, hidden_transform());
        }
    }
}
