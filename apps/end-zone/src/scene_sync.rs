//! Per-tick scene synchronization: write this tick's immutable snapshot,
//! juice effects, camera pose, and debug markers into the retained entities.
//! The same snapshot + same effect state always produces the same submission.

use axiom::prelude::{Angle, Camera, Entity, PerspectiveProjection, RunningApp, Transform, Vec3};
use axiom_kernel::Meters;

use crate::camera::CameraPose;
use crate::debug::DebugInstance;
use crate::football::model::{ball_transform, lace_transform};
use crate::player::{animation, rig};
use crate::presentation::juice::JuiceStack;
use crate::presentation::particles::{effect_instances, trail_instances};
use crate::presentation::snapshot::PresentationSnapshot;
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
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(camera.fov_degrees.clamp(20.0, 110.0)),
                near: meters(0.1),
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

        // Players: state-driven procedural pose → figure boxes.
        for (index, view) in snapshot.players.iter().enumerate() {
            let pose = animation::pose(view.anim, view.anim_ticks, view.stride, view.speed);
            let squash = juice.squash_for(view.id, snapshot.tick);
            let body = rig::body_transform(view.pos, view.facing, &pose, squash);
            for (part_index, part) in rig::world_parts(&self.figure, body, &pose)
                .iter()
                .enumerate()
            {
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

        // The football + lace ridge.
        let carrier_facing = snapshot.carrier().map(|c| c.facing);
        let ball_world = ball_transform(&snapshot.ball, carrier_facing);
        app.set(self.ball, ball_world);
        app.set(self.lace, lace_transform(&ball_world));

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
