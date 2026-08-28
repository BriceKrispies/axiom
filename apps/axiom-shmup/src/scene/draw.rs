//! **Driving the engine's scene nodes from the frame the game resolved.**
//!
//! The camera, and the viewmodel the camera carries. Both are the same job —
//! take a pose the simulation produced and write it onto nodes `scene::install`
//! spawned — and neither has anything to do with composing the app or booting
//! the browser, which is the company they used to keep in `scene::app`.
//!
//! `drive_viewmodel` in particular earns its own home: it is the function whose
//! absence from the browser loop left the rifle lying in the street while every
//! test said the viewmodel was wired. A frame step that only one of two frame
//! paths calls is the defect this port has hit most often, and it is easier to
//! notice a missing call to `scene::draw::drive_viewmodel` than to a private
//! helper three hundred lines up its own file.


use axiom::prelude::*;
use axiom_math::Quat;

use crate::scene::app::{Scene, FAR, NEAR};
use crate::scene::game::CameraPose;
use crate::scene::wiring::weapon_look::drive_hands;

/// The frame's camera, written onto the engine's single camera node.
///
/// **Not** Three's default `'XYZ'` order. The source explicitly overrides it —
/// `this.camera.rotation.order = 'YXZ'` (`engine.js:30`) — and `camera.rs`'s
/// own doc comment names the consequence: "apply yaw, then pitch, then roll".
/// For Euler order `'YXZ'`, Three composes the rotation matrix as `Ry * Rx *
/// Rz` (yaw outermost, roll innermost — `qy * qx * qz`), so that is what this
/// composes too: yaw always rotates around the true world-up axis, so a pure
/// pitch (or a pitch layered under any yaw) never introduces roll. Composing
/// `qx * qy * qz` instead (Three's *default* order, which this file used to
/// assume) rotates pitch around the world-fixed X axis rather than the
/// camera's own local right vector once any yaw is present — which bakes a
/// spurious, non-decaying bank into the view the moment the player looks
/// anywhere off dead-centre. `Quat::from_euler_xyz` composes a third way (`qz *
/// qy * qx`) and is not what either order means; the composition is spelled
/// out explicitly here rather than reached for.
pub fn write_camera(running: &mut RunningApp, pose: CameraPose) {
    let axis = |a: Vec3, angle: f64| {
        Quat::from_axis_angle(a, angle as f32).expect("an authored camera angle is finite")
    };
    let rotation = axis(Vec3::UNIT_Y, pose.rotation.yaw)
        .multiply(axis(Vec3::UNIT_X, pose.rotation.pitch))
        .multiply(axis(Vec3::UNIT_Z, pose.rotation.roll));
    let transform = Transform::new(
        Vec3::new(pose.eye[0] as f32, pose.eye[1] as f32, pose.eye[2] as f32),
        rotation,
        Vec3::new(1.0, 1.0, 1.0),
    );
    running.set_camera(
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(pose.fov_degrees as f32),
            near: Meters::new(NEAR as f32).expect("authored near plane is finite"),
            far: Meters::new(FAR as f32).expect("authored far plane is finite"),
        }),
        transform,
    );
}

/// Advance one rendered frame: step the game with this frame's input, write the
/// camera it resolved, then let the engine render.
pub fn frame(scene: &mut Scene, dt: f64, input: &mut crate::input::Input, tick: u64) -> FrameOutcome {
    let pose = scene.game.frame(dt, input);
    write_camera(&mut scene.app, pose);
    drive_viewmodel(scene, pose);
    // The HUD: model on every target, DOM on wasm32. Its damped channels are
    // stateful, so it ticks whether or not a view is mounted.
    scene.game.hud_frame(input);
    scene.fx_draw.frame(
        &mut scene.app,
        &scene.game.fx_audio.fx,
        pose,
        scene.game.time.elapsed,
    );
    // Every visible soldier, skinned, this frame. Must precede `tick`, which is
    // what drains the queued skinned draws.
    scene.soldier_draw.frame(&mut scene.app, &scene.game.ai);
    scene.app.tick(tick)
}

//// Advance the weapon rig and hang the rifle off the camera.
///
/// `viewmodel.js` composes the rig as a **child of the camera anchor**, and
/// `Viewmodel::rig_pose` returns that local transform — view-model space, not
/// world. So the world transform is the camera's own composed with it, which is
/// what turns "a rifle lying in the road" into "a rifle held in front of the
/// eye".
///
/// Every bucket takes the same transform because the source moves one `group`;
/// the per-part animation (bolt, mag, trigger) lives in
/// [`crate::weapons::viewmodel::PartsState`] and needs per-part nodes, which
/// this scene does not build yet — the buckets are merged **per material**, not
/// per part. That is a real limit and it is stated rather than faked: the rig
/// sways, breathes, kicks and transitions to ADS, and the bolt does not cycle.
pub fn drive_viewmodel(scene: &mut Scene, pose: CameraPose) {
    let axis = |a: Vec3, angle: f64| {
        Quat::from_axis_angle(a, angle as f32).expect("an authored camera angle is finite")
    };
    // The camera's own rotation, composed exactly as `write_camera` composes
    // it — YXZ, because the source overrides Three's default order. Composing
    // it differently here would make the gun bank against the view.
    let camera_rot = axis(Vec3::UNIT_Y, pose.rotation.yaw)
        .multiply(axis(Vec3::UNIT_X, pose.rotation.pitch))
        .multiply(axis(Vec3::UNIT_Z, pose.rotation.roll));

    // The pose comes from the weapons core, which already stepped its own
    // viewmodel in `Game::frame` off real input — including the trigger, so the
    // rifle now recoils. This function used to build a `FrameInput` and drive a
    // SECOND viewmodel with `trigger: false` hardcoded, which is why the gun
    // could never kick.
    let (rig_pos, rig_quat) = scene.game.weapons.rig_pose();
    let local = Vec3::new(rig_pos.x as f32, rig_pos.y as f32, rig_pos.z as f32);
    let rig_rot = Quat::new(
        rig_quat.x as f32,
        rig_quat.y as f32,
        rig_quat.z as f32,
        rig_quat.w as f32,
    );
    let eye = Vec3::new(pose.eye[0] as f32, pose.eye[1] as f32, pose.eye[2] as f32);
    let world = Transform::new(
        eye.add(camera_rot.rotate(local)),
        camera_rot.multiply(rig_rot),
        Vec3::new(1.0, 1.0, 1.0),
    );
    scene.rifle_nodes.iter().for_each(|node| {
        scene.app.set(*node, world);
    });
    // The arms ride the same rig transform the rifle does: `solve_hands`
    // rebases both shoulders out of camera space into rig space before solving,
    // so an arm's root IS the rig. This lives inside `drive_viewmodel` on
    // purpose — both frame paths call it, so neither can silently skip the
    // hands the way the viewmodel itself once was skipped.
    let viewmodel = &mut scene.game.weapons.core_mut().viewmodel;
    drive_hands(
        &mut scene.app,
        &scene.hand_nodes,
        &mut viewmodel.arm_l,
        &mut viewmodel.arm_r,
        world,
    );
}

