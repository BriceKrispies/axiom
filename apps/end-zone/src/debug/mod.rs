//! Diagnostic visualization: bounded 3D marker instances (routes, steering
//! targets, collision circles, catch volumes, ball-trajectory prediction, the
//! camera aim) plus the text rows for the overlay. Reads only the immutable
//! snapshot and static route data — debug rendering can never affect the
//! simulation.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

pub mod locomotion;

use crate::ai::PlayerIntent;
use crate::camera::{CameraMode, CameraPose};
use crate::football::{predict_position, BallState};
use crate::presentation::snapshot::PresentationSnapshot;
use crate::presentation::{LocomotionSample, PlayerPose};
use crate::state::PlayPhase;

/// Which pooled debug material an instance uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugMaterial {
    Route,
    Target,
    Collision,
    CatchVolume,
    Trajectory,
    CameraAim,
    /// A planted-foot world lock target.
    FootLock,
    /// A current solved foot position.
    FootNow,
    /// A swing foot's next intended landing.
    FootLanding,
    /// A player's resolved movement vector.
    MoveVector,
    /// The authoritative gameplay root (simulated position).
    GameplayRoot,
    /// The derived visual body root (cosmetic weight-transfer frame).
    VisualRoot,
    /// The pelvis joint riding under the visual body root.
    Pelvis,
    /// The weight-shift point: the pelvis dropped to the turf.
    WeightPoint,
    /// The foot currently bearing weight.
    StanceFoot,
}

/// One debug marker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebugInstance {
    pub transform: Transform,
    pub material: DebugMaterial,
}

/// Hard cap on debug markers (the scene pool size).
pub const DEBUG_CAP: usize = 512;

pub(super) fn push(out: &mut Vec<DebugInstance>, transform: Transform, material: DebugMaterial) {
    if out.len() < DEBUG_CAP {
        out.push(DebugInstance {
            transform,
            material,
        });
    }
}

pub(super) fn cube(center: Vec3, size: f32) -> Transform {
    Transform::new(center, Quat::IDENTITY, Vec3::new(size, size, size))
}

/// Build all debug markers for this tick. `routes` is the play's static
/// per-player world-waypoint table (cloned once at build, not per tick).
pub fn build_markers(
    snapshot: &PresentationSnapshot,
    poses: &[PlayerPose],
    routes: &[Vec<Vec3>],
    camera: &CameraPose,
    out: &mut Vec<DebugInstance>,
) {
    out.clear();

    // Locomotion foot markers: each planted-foot lock, each solved foot, the
    // next intended landing, and the resolved movement vector. Debug-only.
    for (view, player_pose) in snapshot.players.iter().zip(poses.iter()) {
        locomotion::foot_markers(&player_pose.sample, view.pos, out);
        locomotion::markers(&player_pose.sample, out);
    }

    // Route paths: waypoint markers plus interpolated dots between them.
    for (index, route) in routes.iter().enumerate() {
        let mut previous = snapshot.players[index].pos;
        for waypoint in route {
            let lifted = Vec3::new(waypoint.x, 0.25, waypoint.z);
            push(out, cube(lifted, 0.22), DebugMaterial::Route);
            for step in 1..4 {
                let t = step as f32 / 4.0;
                let dot = Vec3::new(
                    previous.x + (waypoint.x - previous.x) * t,
                    0.18,
                    previous.z + (waypoint.z - previous.z) * t,
                );
                push(out, cube(dot, 0.09), DebugMaterial::Route);
            }
            previous = *waypoint;
        }
    }

    for player in &snapshot.players {
        // Steering target.
        if let Some((point, _)) = player.intent.movement() {
            push(
                out,
                cube(Vec3::new(point.x, 0.35, point.z), 0.18),
                DebugMaterial::Target,
            );
        }
        // Collision circle: eight dots at the body radius.
        for segment in 0..8 {
            let angle = segment as f32 / 8.0 * core::f32::consts::TAU;
            let dot = Vec3::new(
                player.pos.x + angle.cos() * player.body_radius,
                0.12,
                player.pos.z + angle.sin() * player.body_radius,
            );
            push(out, cube(dot, 0.07), DebugMaterial::Collision);
        }
    }

    // Catch volume of the intended receiver while the pass is live.
    if let Some(flight) = snapshot.flight {
        let receiver = snapshot.player(flight.intended);
        let center = receiver.pos.add(Vec3::new(0.0, 1.45, 0.0));
        for segment in 0..10 {
            let angle = segment as f32 / 10.0 * core::f32::consts::TAU;
            let dot = center.add(Vec3::new(
                angle.cos() * receiver.catch_radius,
                0.0,
                angle.sin() * receiver.catch_radius,
            ));
            push(out, cube(dot, 0.08), DebugMaterial::CatchVolume);
        }
        // Predicted trajectory from release to arrival.
        for sample in 0..=16 {
            let seconds = flight.eta_ticks as f32 / 60.0 * sample as f32 / 16.0;
            let point =
                predict_position(flight.release, flight.velocity, snapshot.gravity, seconds);
            push(out, cube(point, 0.10), DebugMaterial::Trajectory);
        }
        push(out, cube(flight.target, 0.24), DebugMaterial::Trajectory);
    }

    // Camera aim: the look-at point.
    push(out, cube(camera.target, 0.2), DebugMaterial::CameraAim);
}

/// The always-on overlay rows (tick, phase, ball, possession, camera, seed,
/// impulses, selected player, its locomotion read-out) — text only.
pub fn overlay_rows(
    snapshot: &PresentationSnapshot,
    locomotion: Option<&LocomotionSample>,
    camera_mode: CameraMode,
    forced: bool,
    impulses: usize,
    debug_enabled: bool,
) -> Vec<(String, String)> {
    let ball_state = match snapshot.ball.state {
        BallState::Dead => "dead".to_string(),
        BallState::Held { carrier } => format!("held by #{}", snapshot.player(carrier).jersey),
        BallState::Snap { .. } => "snap".to_string(),
        BallState::Airborne { .. } => "airborne pass".to_string(),
        BallState::Loose => "loose".to_string(),
        BallState::Grounded => "grounded".to_string(),
    };
    let possession = snapshot
        .possession
        .map(|id| format!("player {}", id.0))
        .unwrap_or_else(|| "none".to_string());
    let phase = match snapshot.phase {
        PlayPhase::PreSnap => "pre-snap",
        PlayPhase::Live => "live",
        PlayPhase::Ended => "ended",
    };
    let selected = snapshot.player(snapshot.quarterback);
    let mut rows = vec![
        ("app".to_string(), "END ZONE showcase".to_string()),
        ("tick".to_string(), snapshot.tick.to_string()),
        ("phase".to_string(), phase.to_string()),
        ("ball".to_string(), ball_state),
        ("possession".to_string(), possession),
        (
            "camera".to_string(),
            format!(
                "{:?}{}",
                camera_mode,
                if forced { " (forced)" } else { " (auto)" }
            ),
        ),
        ("seed".to_string(), format!("{:#x}", snapshot.seed)),
        ("impulses".to_string(), impulses.to_string()),
        (
            "qb".to_string(),
            format!("{:?} / {:?}", selected.role, intent_name(&selected.intent)),
        ),
        ("debug (F1)".to_string(), debug_enabled.to_string()),
    ];
    if let Some(fault) = snapshot.fault {
        rows.push(("fault".to_string(), fault.to_string()));
    }
    if let Some(loco) = locomotion {
        locomotion::push_locomotion_rows(&mut rows, snapshot.player(snapshot.quarterback).speed, loco);
    }
    rows
}

fn intent_name(intent: &PlayerIntent) -> &'static str {
    match intent {
        PlayerIntent::Hold => "hold",
        PlayerIntent::Face { .. } => "face",
        PlayerIntent::MoveToward { .. } => "move",
        PlayerIntent::DropBack { .. } => "dropback",
        PlayerIntent::Block { .. } => "block",
        PlayerIntent::Pursue { .. } => "pursue",
        PlayerIntent::PrepareCatch { .. } => "prepare-catch",
        PlayerIntent::Throw => "throw",
        PlayerIntent::Carry { .. } => "carry",
        PlayerIntent::Tackle { .. } => "tackle",
        PlayerIntent::Recover => "recover",
    }
}
