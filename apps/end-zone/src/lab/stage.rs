//! The deterministic core of the Animation Lab: one isolated actor driven
//! through the catalog and posed by the *real* [`LocomotionAnimator`] +
//! override poses, exactly as a game player is. Browser-free and wall-clock
//! free — the wasm edge (`crate::lab::web`) only renders what [`AnimLab::step`]
//! produces and forwards the picker's selection.

use axiom::prelude::Vec3;

use crate::ai::{PlayerIntent, RoleState};
use crate::camera::CameraPose;
use crate::data::{JuiceTuning, LocomotionTuning};
use crate::events::{EventId, SimEvent, StampedEvent};
use crate::football::BallSim;
use crate::identity::{PlayerId, TeamId};
use crate::lab::catalog::{catalog, LabClip};
use crate::lab::drive::{self, Actor};
use crate::presentation::snapshot::{PlayerView, PresentationSnapshot};
use crate::presentation::{JuiceStack, LocomotionAnimator, LocomotionSample, PlayerPose};
use crate::state::PlayPhase;

/// One stepped lab frame: the single-player snapshot, its composed pose, and
/// the trailing camera — everything the scene sync needs to draw the player.
#[derive(Debug, Clone)]
pub struct LabFrame {
    pub snapshot: PresentationSnapshot,
    pub poses: Vec<PlayerPose>,
    pub camera: CameraPose,
}

/// The isolated-player animation lab.
#[derive(Debug)]
pub struct AnimLab {
    locomotion: LocomotionAnimator,
    juice: JuiceStack,
    clips: Vec<LabClip>,
    selected: usize,
    actor: Actor,
    tick: u64,
    /// Set on a clip switch or path wrap so the next step re-anchors the feet
    /// (a synthetic `PlayReset`) instead of registering a teleport as a stride.
    reanchor: bool,
    /// Orbit-follow camera: azimuth offset from directly behind the character
    /// (rad), elevation above the horizon (rad), and boom length (yd). The
    /// character is always framed; drag input rotates the offset/elevation so
    /// you can watch the run from any angle. Persists across clip switches.
    orbit_yaw: f32,
    orbit_pitch: f32,
    orbit_distance: f32,
}

impl Default for AnimLab {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimLab {
    /// A fresh lab framed on the first catalog clip (Idle).
    pub fn new() -> Self {
        let clips = catalog();
        let first = clips[0].anim;
        AnimLab {
            locomotion: LocomotionAnimator::new(LocomotionTuning::default()),
            juice: JuiceStack::new(0, JuiceTuning::default()),
            clips,
            selected: 0,
            actor: Actor::rest(first),
            tick: 0,
            reanchor: true,
            // Defaults reproduce the original trailing three-quarter framing.
            orbit_yaw: 0.35,
            orbit_pitch: 0.47,
            orbit_distance: 6.6,
        }
    }

    /// Nudge the orbit-follow camera by a drag delta (radians): `dyaw` swings
    /// around the character, `dpitch` raises/lowers the view. Elevation is
    /// clamped so the boom never flips over the top or sinks under the field.
    pub fn orbit(&mut self, dyaw: f32, dpitch: f32) {
        self.orbit_yaw += dyaw;
        self.orbit_pitch = (self.orbit_pitch + dpitch).clamp(0.08, 1.35);
    }

    /// The picker labels, in catalog order.
    pub fn labels(&self) -> Vec<&'static str> {
        self.clips.iter().map(|c| c.label).collect()
    }

    /// The index of the currently framed clip.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Frame clip `index` (clamped) and re-seat the actor at its start.
    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.clips.len() - 1);
        self.actor = Actor::rest(self.clips[self.selected].anim);
        self.reanchor = true;
    }

    /// Frame the next / previous catalog clip, wrapping around.
    pub fn next(&mut self) {
        self.select((self.selected + 1) % self.clips.len());
    }
    pub fn prev(&mut self) {
        self.select((self.selected + self.clips.len() - 1) % self.clips.len());
    }

    /// The last-resolved locomotion sample (mode / phase / stride / foot-lock
    /// error) — the diagnostics the overlay shows and the tests assert on.
    pub fn sample(&self) -> Option<LocomotionSample> {
        self.locomotion.sample(0).copied()
    }

    /// The juice stack the scene sync reads (idle in the lab — no events).
    pub fn juice(&self) -> &JuiceStack {
        &self.juice
    }

    /// Advance one fixed tick: move the actor, pose it through the real
    /// animator, and frame the trailing camera.
    pub fn step(&mut self) -> LabFrame {
        let clip = self.clips[self.selected];
        self.reanchor |= drive::advance(&mut self.actor, clip);
        let events = self.take_events();
        let snapshot = self.snapshot();
        let poses = self.locomotion.step(&snapshot, &events);
        self.juice.step(&snapshot, &events);
        let camera = self.camera();
        self.tick += 1;
        LabFrame {
            snapshot,
            poses,
            camera,
        }
    }

    /// Diagnostic rows for the on-page overlay.
    pub fn overlay_rows(&self) -> Vec<(String, String)> {
        let clip = self.clips[self.selected];
        let mut rows = vec![(
            "clip".to_string(),
            format!("{} ({}/{})", clip.label, self.selected + 1, self.clips.len()),
        )];
        if let Some(s) = self.sample() {
            rows.push(("mode".to_string(), format!("{:?}", s.mode)));
            rows.push(("phase".to_string(), format!("{:.2}", s.gait_phase)));
            rows.push(("stride".to_string(), format!("{:.2} yd", s.stride_length)));
            rows.push(("cadence".to_string(), format!("{:.2} /s", s.cadence)));
            rows.push(("speed".to_string(), format!("{:.2} yd/s", s.speed)));
            rows.push((
                "foot-lock err".to_string(),
                format!("L {:.3}  R {:.3}", s.left_lock_error, s.right_lock_error),
            ));
            rows.push(("override".to_string(), format!("{:?}", s.reason)));
        }
        rows
    }

    /// This tick's synthetic events: a `PlayReset` on a switch/wrap so the
    /// animator re-anchors the planted foot instead of skating over the jump.
    fn take_events(&mut self) -> Vec<StampedEvent> {
        let mut events = Vec::new();
        if self.reanchor {
            events.push(StampedEvent {
                id: EventId::new(self.tick, 0),
                tick: self.tick,
                event: SimEvent::PlayReset,
            });
            self.reanchor = false;
        }
        events
    }

    /// Build the one-player presentation snapshot the animator consumes.
    fn snapshot(&self) -> PresentationSnapshot {
        let vel = self.actor.vel;
        let view = PlayerView {
            id: PlayerId(0),
            team: TeamId(0),
            jersey: 1,
            pos: self.actor.pos,
            vel,
            facing: self.actor.facing,
            anim: self.actor.anim,
            anim_ticks: self.actor.anim_ticks,
            speed: Vec3::new(vel.x, 0.0, vel.z).length(),
            body_radius: 0.5,
            catch_radius: 1.0,
            role: RoleState::Waiting,
            intent: PlayerIntent::Hold,
            responsibility: crate::ai::Responsibility::None,
            action_reason: None,
            commit_ticks: 0,
            engagement_state: None,
            engagement_advantage: 0.0,
            rush_lane: None,
        };
        PresentationSnapshot {
            tick: self.tick,
            seed: 0,
            phase: PlayPhase::Live,
            end_reason: None,
            possession: None,
            // A sentinel quarterback id (never the actor) keeps the ball out of
            // the hand: the running arms swing free instead of carrying.
            quarterback: PlayerId(1),
            ball: BallSim::dead_at(Vec3::new(0.0, -100.0, 0.0)),
            flight: None,
            players: vec![view],
            line_of_scrimmage_z: 0.0,
            drive_sign: 1.0,
            gravity: 0.0,
            fault: None,
            ball_situation: crate::football::BallSituation::PreSnap,
            drive: None,
            throwable: Vec::new(),
            to_gain_z: None,
        }
    }

    /// An orbit-follow camera: always framed on the actor, its azimuth taken
    /// relative to the actor's facing (so `orbit_yaw = 0` trails directly
    /// behind and the framing holds as the actor turns) plus the drag-controlled
    /// offset and elevation.
    fn camera(&self) -> CameraPose {
        let azimuth = self.actor.facing + core::f32::consts::PI + self.orbit_yaw;
        let cp = self.orbit_pitch.cos();
        let offset = Vec3::new(azimuth.sin() * cp, self.orbit_pitch.sin(), azimuth.cos() * cp)
            .mul_scalar(self.orbit_distance);
        let focus = self.actor.pos.add(Vec3::new(0.0, 1.0, 0.0));
        CameraPose {
            eye: focus.add(offset),
            target: focus,
            fov_degrees: 42.0,
        }
    }
}
