//! Deterministic replay artifacts: the bit-exact state digest, and the
//! full-showcase trace the replay tests compare run against run.

use crate::camera::{CameraMode, CameraPose};
use crate::config::{EndZoneConfig, PLAYER_COUNT};
use crate::events::StampedEvent;
use crate::showcase::{DiagnosticCommand, ShowcaseRun, TRACE_THROW_TICK};
use crate::state::SimState;

impl SimState {
    /// A deterministic digest of the authoritative state (bit-exact floats).
    pub fn digest(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(PLAYER_COUNT * 8 + 16);
        out.push(self.tick as u32);
        out.push(self.possession.map(|p| u32::from(p.0) + 1).unwrap_or(0));
        for player in &self.players {
            for v in [
                player.pos.x,
                player.pos.y,
                player.pos.z,
                player.vel.x,
                player.vel.z,
                player.facing,
                player.balance,
            ] {
                out.push(v.to_bits());
            }
        }
        for v in [
            self.ball.pos.x,
            self.ball.pos.y,
            self.ball.pos.z,
            self.ball.vel.x,
            self.ball.vel.y,
            self.ball.vel.z,
            self.ball.spin_angle,
        ] {
            out.push(v.to_bits());
        }
        out
    }
}

/// Deterministic artifacts of one showcase run — what the replay tests
/// compare bit-for-bit.
#[derive(Debug, Clone, PartialEq)]
pub struct ShowcaseTrace {
    pub events: Vec<StampedEvent>,
    /// Ball position each tick.
    pub ball_samples: Vec<axiom::prelude::Vec3>,
    /// Possession transitions `(tick, holder)`.
    pub possession: Vec<(u64, Option<crate::identity::PlayerId>)>,
    /// Every player's intent, every tick.
    pub intents: Vec<Vec<crate::ai::PlayerIntent>>,
    /// Camera mode transitions `(tick, mode)`.
    pub camera_modes: Vec<(u64, CameraMode)>,
    /// The final camera pose each tick.
    pub camera_poses: Vec<CameraPose>,
    pub final_digest: Vec<u32>,
}

/// Run the whole showcase for `ticks` fixed steps with ONE scripted input —
/// the throw press at [`TRACE_THROW_TICK`] (the quarterback never throws on
/// his own) — and collect the deterministic artifacts.
pub fn run_trace(config: EndZoneConfig, ticks: u64) -> ShowcaseTrace {
    let mut run = ShowcaseRun::new(config);
    let mut trace = ShowcaseTrace {
        events: Vec::new(),
        ball_samples: Vec::new(),
        possession: Vec::new(),
        intents: Vec::new(),
        camera_modes: Vec::new(),
        camera_poses: Vec::new(),
        final_digest: Vec::new(),
    };
    let mut last_possession = None;
    for tick in 0..ticks {
        let scripted: &[DiagnosticCommand] = if tick == TRACE_THROW_TICK {
            &[DiagnosticCommand::PrimaryAction]
        } else {
            &[]
        };
        let output = run.step(scripted);
        trace.events.extend_from_slice(&output.events);
        trace.ball_samples.push(output.snapshot.ball.pos);
        if output.snapshot.possession != last_possession {
            last_possession = output.snapshot.possession;
            trace
                .possession
                .push((output.snapshot.tick, last_possession));
        }
        trace
            .intents
            .push(output.snapshot.players.iter().map(|p| p.intent).collect());
        trace.camera_poses.push(output.camera);
    }
    trace.camera_modes = run.director.history().to_vec();
    trace.final_digest = run.sim.digest();
    trace
}
