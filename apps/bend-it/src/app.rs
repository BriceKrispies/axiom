//! The composition root: the engine scene, the session, the editor and the input
//! boundary, wired into one frame.
//!
//! One rendered frame is one fixed simulation tick, the repo's live-loop
//! convention. Everything the frame does is ordered so that what the player sees
//! and what the player touches agree:
//!
//! 1. sample input into the neutral snapshot;
//! 2. resolve the camera for *this* viewport, and the screen mapping from it;
//! 3. read gestures against that mapping into commands;
//! 4. step the session by exactly those commands;
//! 5. pose the scene and hand the frame to the renderer.
//!
//! Step 2 before step 3 is the important one: the finger is tested against the
//! camera the frame is about to be drawn with, not the one it was drawn with
//! last time — so an aim never lands a frame behind the goal it was aimed at.

use axiom::prelude::{FrameOutcome, RunningApp, Vec2};
use axiom_input::{ActionId, DeviceFrame, InputState, KeyToken, Pointer};
use axiom_kernel::Tick;

use crate::camera::{self, CameraPose};
use crate::debug::{self, DebugMarker};
use crate::editor::{Editor, EditorView};
use crate::pitch::GoalMouth;
use crate::play::{EditorCommand, Phase, Session};
use crate::projection::ScreenProjection;
use crate::scene::BendItScene;
use crate::tuning::Tuning;

/// Actions the keyboard can fire. Touch never needs them — the whole interface
/// is on screen — but a desktop player expects a keyboard to work, and routing
/// it through the same commands means there is one game, not two.
pub const ACTION_ADVANCE: ActionId = ActionId::new(1);
pub const ACTION_BACK: ActionId = ActionId::new(2);
pub const ACTION_RESTART: ActionId = ActionId::new(3);
pub const ACTION_DEBUG: ActionId = ActionId::new(4);

/// The whole game.
#[derive(Debug)]
pub struct BendIt {
    running: RunningApp,
    scene: BendItScene,
    session: Session,
    editor: Editor,
    input: InputState,
    surface: Vec2,
    frame_n: u64,
    debug: bool,
    markers: Vec<DebugMarker>,
    view: EditorView,
}

impl BendIt {
    /// Build the game for a surface, in physical pixels.
    pub fn new(width: u32, height: u32) -> BendIt {
        let (running, scene) = BendItScene::install(width, height);
        let mut input = InputState::new();
        input.bind_action(
            ACTION_ADVANCE,
            &[KeyToken::new("Space"), KeyToken::new("Enter")],
        );
        input.bind_action(
            ACTION_BACK,
            &[KeyToken::new("Escape"), KeyToken::new("Backspace")],
        );
        input.bind_action(ACTION_RESTART, &[KeyToken::new("KeyR")]);
        input.bind_action(ACTION_DEBUG, &[KeyToken::new("F1")]);
        let surface = Vec2::new(width.max(1) as f32, height.max(1) as f32);
        let session = Session::new(Tuning::DEFAULT);
        let view = EditorView::quiet(Phase::Ready, surface, surface.x.min(surface.y), (0, 0));
        BendIt {
            running,
            scene,
            session,
            editor: Editor::new(),
            input,
            surface,
            frame_n: 0,
            debug: false,
            markers: Vec::new(),
            view,
        }
    }

    /// The session (read-only; the debug overlay and the tests read it).
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// This frame's overlay model.
    pub fn view(&self) -> &EditorView {
        &self.view
    }

    /// Whether the debug view is on.
    pub fn debug_enabled(&self) -> bool {
        self.debug
    }

    /// The debug overlay's text rows.
    pub fn overlay_rows(&self) -> Vec<(String, String)> {
        debug::rows(&self.session)
    }

    /// The wrapped engine app (the browser loop needs its upload lanes).
    pub fn running(&mut self) -> &mut RunningApp {
        &mut self.running
    }

    pub fn frame_index(&self) -> u64 {
        self.frame_n
    }

    /// The camera this frame is framed with.
    pub fn camera(&self) -> CameraPose {
        let tuning = self.session.tuning();
        camera::frame(
            self.surface,
            &GoalMouth::new(tuning.goal.inset),
            self.session.shot().origin,
            self.session.kick().start,
            self.flight_progress(),
            &tuning.camera,
        )
    }

    /// How far through the flight the ball is, `0..1` — what the camera dollies
    /// on and what the preview fades with.
    fn flight_progress(&self) -> f32 {
        self.session
            .ball()
            .elapsed()
            .map(|t| (t / self.session.shot().trajectory.duration().max(1.0e-3)).clamp(0.0, 1.0))
            .unwrap_or(0.0)
    }

    /// Tell the game the surface changed size (an orientation change, a resized
    /// window). The camera and the whole interface re-derive from it next frame.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.surface = Vec2::new(width.max(1.0), height.max(1.0));
    }

    /// One frame.
    pub fn frame(&mut self, keys: &[KeyToken], pointers: &[(Vec2, bool)]) -> FrameOutcome {
        self.advance(keys, pointers);
        self.present()
    }

    /// Sample input and step the session by one tick, without touching the
    /// renderer.
    pub fn advance(&mut self, keys: &[KeyToken], pointers: &[(Vec2, bool)]) {
        let device = DeviceFrame::new(self.surface, keys, pointers);
        self.input.sample(Tick::new(self.frame_n), &device);
        self.debug ^= self.input.pressed(ACTION_DEBUG);

        let projection = ScreenProjection::new(&self.camera(), self.surface);
        let pointer: Option<Pointer> = self.input.pointer();
        let mut commands =
            self.editor
                .update(pointer, &self.session, &projection, self.session.tuning());
        // The keyboard says the same things the on-screen buttons do.
        self.input
            .pressed(ACTION_ADVANCE)
            .then(|| commands.push(EditorCommand::Advance));
        self.input
            .pressed(ACTION_BACK)
            .then(|| commands.push(EditorCommand::Back));
        self.input
            .pressed(ACTION_RESTART)
            .then(|| commands.push(EditorCommand::Restart));

        self.session.step(&commands);
        self.view = self
            .editor
            .view(&self.session, &projection, self.session.tuning());
    }

    /// Pose the scene and hand the frame to the renderer.
    pub fn present(&mut self) -> FrameOutcome {
        self.pose();
        let outcome = self.running.tick(self.frame_n);
        self.frame_n += 1;
        outcome
    }

    /// Pose the scene without ticking the engine — the capture harness poses here
    /// and lets the renderer drive the tick.
    pub fn pose(&mut self) {
        match self.debug {
            true => debug::markers(&self.session, &mut self.markers),
            false => self.markers.clear(),
        }
        let camera = self.camera();
        self.scene
            .update(&mut self.running, &self.session, &camera, &self.markers);
    }

    /// Consume into the engine app (the repo's capture-harness convention).
    pub fn into_running(self) -> RunningApp {
        self.running
    }
}

/// The repo-standard capture builder: the composed game, advanced one frame so
/// the scene is posed.
pub fn build_bend_it() -> RunningApp {
    let mut game = BendIt::new(720, 1280);
    let _ = game.frame(&[], &[]);
    game.into_running()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::Phase;

    #[test]
    fn the_game_builds_and_runs_frames() {
        let mut game = BendIt::new(390, 844);
        (0..30).for_each(|_| {
            let _ = game.frame(&[], &[]);
        });
        assert_eq!(game.frame_index(), 30);
        assert_eq!(game.session().phase(), Phase::TargetSelection);
        assert!(!game.debug_enabled());
        assert!(game.view().goal_quad.is_some());
    }

    #[test]
    fn the_keyboard_drives_the_same_flow_the_buttons_do() {
        let mut game = BendIt::new(390, 844);
        (0..14).for_each(|_| {
            let _ = game.frame(&[], &[]);
        });
        let space = [KeyToken::new("Space")];
        let _ = game.frame(&space, &[]);
        assert_eq!(game.session().phase(), Phase::HorizontalSculpt);
        // Held keys do not auto-repeat: the edge is what fires.
        let _ = game.frame(&space, &[]);
        assert_eq!(game.session().phase(), Phase::HorizontalSculpt);
        let _ = game.frame(&[], &[]);
        let _ = game.frame(&space, &[]);
        assert_eq!(game.session().phase(), Phase::VerticalSculpt);
        // And back again.
        let _ = game.frame(&[], &[]);
        let _ = game.frame(&[KeyToken::new("Escape")], &[]);
        assert_eq!(game.session().phase(), Phase::HorizontalSculpt);
    }

    #[test]
    fn a_touch_inside_the_goal_aims_the_shot() {
        let mut game = BendIt::new(390, 844);
        (0..14).for_each(|_| {
            let _ = game.frame(&[], &[]);
        });
        let projection = ScreenProjection::new(&game.camera(), Vec2::new(390.0, 844.0));
        let corner = game.session().mouth().to_world(-0.9, 0.85);
        let at = projection.project(corner).expect("the corner is on screen");
        let _ = game.frame(&[], &[(at, true)]);
        let target = game.session().shot().world_target;
        assert!(
            target.subtract(corner).length() < 0.25,
            "aimed at {corner:?} but the shot finishes at {target:?}"
        );
        assert!(game.view().target.is_some());
    }

    #[test]
    fn the_debug_view_toggles_and_stays_out_of_the_normal_frame() {
        let mut game = BendIt::new(390, 844);
        let _ = game.frame(&[], &[]);
        assert!(game.markers.is_empty());
        let _ = game.frame(&[KeyToken::new("F1")], &[]);
        assert!(game.debug_enabled());
        assert!(!game.markers.is_empty(), "the debug geometry appears");
        let _ = game.frame(&[], &[]);
        let _ = game.frame(&[KeyToken::new("F1")], &[]);
        assert!(!game.debug_enabled());
        assert!(game.markers.is_empty());
        assert!(!game.overlay_rows().is_empty());
    }

    #[test]
    fn resizing_re_frames_the_camera_without_rebuilding_anything() {
        let mut game = BendIt::new(390, 844);
        let _ = game.frame(&[], &[]);
        let portrait = game.camera();
        game.resize(1440.0, 900.0);
        let _ = game.frame(&[], &[]);
        let landscape = game.camera();
        assert_ne!(landscape, portrait, "the framing follows the viewport");
        assert!(landscape.eye.z < portrait.eye.z);
        assert_eq!(game.view().viewport, Vec2::new(1440.0, 900.0));
    }

    #[test]
    fn the_capture_builder_produces_a_posed_scene() {
        let mut running = build_bend_it();
        let outcome = running.tick(1);
        let drawn: usize = outcome.mesh_batches().iter().map(|b| b.2.len()).sum();
        assert!(drawn > 0, "the capture frame submits geometry");
    }
}
