//! The composition root: the engine scene, the session, the drawing layer and
//! the input boundary, wired into one frame.
//!
//! One rendered frame is one fixed simulation tick, the repo's live-loop
//! convention. Everything the frame does is ordered so that what the player sees
//! and what the player draws agree:
//!
//! 1. sample input into the neutral snapshot;
//! 2. resolve the camera for *this* viewport, and the screen mapping from it;
//! 3. fold the pointer into the drawing;
//! 4. on release, read the drawing as a shot and hand the session that one
//!    command;
//! 5. pose the scene and hand the frame to the renderer.
//!
//! Step 2 before step 3 is the important one: the line is read against the
//! camera the frame is about to be drawn with, not the one it was drawn with
//! last time.

use axiom::prelude::{FrameOutcome, RunningApp, Vec2};
use axiom_input::{ActionId, DeviceFrame, InputState, KeyToken, Pointer};
use axiom_kernel::Tick;

use crate::camera::{self, CameraPose};
use crate::debug::{self, DebugMarker};
use crate::pitch::GoalMouth;
use crate::play::{Phase, PlayCommand, Session};
use crate::projection::ScreenProjection;
use crate::scene::BendItScene;
use crate::stroke::{
    hint_for, interpret, Drawing, GameView, Reading, Stroke, StrokeCapture, StrokeView,
};
use crate::tuning::{Tuning, DT};

/// Actions the keyboard can fire. Touch never needs them — the whole interface
/// is the drawing — but a desktop player expects a keyboard to work, and routing
/// it through the same commands means there is one game, not two.
pub const ACTION_RESTART: ActionId = ActionId::new(1);
pub const ACTION_DEBUG: ActionId = ActionId::new(2);

/// The whole game.
#[derive(Debug)]
pub struct BendIt {
    running: RunningApp,
    scene: BendItScene,
    session: Session,
    capture: StrokeCapture,
    input: InputState,
    surface: Vec2,
    frame_n: u64,
    debug: bool,
    markers: Vec<DebugMarker>,
    view: GameView,
    /// The line left over from the last release, flicking away.
    ghost: Option<(Stroke, f32)>,
    /// The most recent reading, kept for the debug view.
    reading: Option<Reading>,
    last_phase: Phase,
}

impl BendIt {
    /// Build the game for a surface, in physical pixels.
    pub fn new(width: u32, height: u32) -> BendIt {
        let (running, scene) = BendItScene::install(width, height);
        let mut input = InputState::new();
        input.bind_action(
            ACTION_RESTART,
            &[KeyToken::new("KeyR"), KeyToken::new("Escape")],
        );
        input.bind_action(ACTION_DEBUG, &[KeyToken::new("F1")]);
        let surface = Vec2::new(width.max(1) as f32, height.max(1) as f32);
        BendIt {
            running,
            scene,
            session: Session::new(Tuning::DEFAULT),
            capture: StrokeCapture::new(),
            input,
            surface,
            frame_n: 0,
            debug: false,
            markers: Vec::new(),
            view: GameView::empty(Phase::Ready, surface, (0, 0)),
            ghost: None,
            reading: None,
            last_phase: Phase::Ready,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn view(&self) -> &GameView {
        &self.view
    }

    pub fn debug_enabled(&self) -> bool {
        self.debug
    }

    /// The most recent reading of a drawing.
    pub fn reading(&self) -> Option<&Reading> {
        self.reading.as_ref()
    }

    pub fn overlay_rows(&self) -> Vec<(String, String)> {
        debug::rows(&self.session, self.reading.as_ref())
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

    fn flight_progress(&self) -> f32 {
        self.session
            .ball()
            .elapsed()
            .map(|t| (t / self.session.shot().trajectory.duration().max(1.0e-3)).clamp(0.0, 1.0))
            .unwrap_or(0.0)
    }

    /// Tell the game the surface changed size. The camera and the drawing's own
    /// scale re-derive from it next frame.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.surface = Vec2::new(width.max(1.0), height.max(1.0));
    }

    /// One frame.
    pub fn frame(&mut self, keys: &[KeyToken], pointers: &[(Vec2, bool)]) -> FrameOutcome {
        self.advance(keys, pointers);
        self.present()
    }

    /// Sample input, fold the drawing, and step the session by one tick.
    pub fn advance(&mut self, keys: &[KeyToken], pointers: &[(Vec2, bool)]) {
        let device = DeviceFrame::new(self.surface, keys, pointers);
        self.input.sample(Tick::new(self.frame_n), &device);
        self.debug ^= self.input.pressed(ACTION_DEBUG);

        let projection = ScreenProjection::new(&self.camera(), self.surface);
        let pointer = self.input.pointer();
        let mut commands = self.draw(pointer, &projection);
        self.input
            .pressed(ACTION_RESTART)
            .then(|| commands.push(PlayCommand::Restart));

        self.session.step(&commands);
        self.fade();
        self.view = self.compose();
    }

    /// Fold this tick's contact into the drawing, and — if it finished — read it.
    fn draw(
        &mut self,
        pointer: Option<Pointer>,
        projection: &ScreenProjection,
    ) -> Vec<PlayCommand> {
        let tuning = *self.session.tuning();
        let short = self.surface.x.min(self.surface.y);
        // A phase change under the player's finger abandons the line rather than
        // letting it fire into the next attempt.
        (self.session.phase() != self.last_phase).then(|| self.capture.cancel());
        self.last_phase = self.session.phase();

        let accepting = self.session.phase().accepts_drawing();
        let sample = pointer.filter(|_| accepting);
        match self
            .capture
            .update(sample, short * tuning.stroke.spacing, short)
        {
            Drawing::Idle | Drawing::Drawing => Vec::new(),
            Drawing::Finished(line) => {
                // The line leaves the screen the moment it is let go, whether or
                // not it was long enough to mean anything.
                self.ghost = Some((line.clone(), 1.0));
                let reading = interpret(
                    &line,
                    projection,
                    self.session.shot().origin,
                    self.session.mouth(),
                    &tuning,
                );
                self.reading = reading.clone();
                reading
                    .map(|r| vec![PlayCommand::Kick(r.intent)])
                    .unwrap_or_default()
            }
        }
    }

    /// Advance the released line's flick-away.
    fn fade(&mut self) {
        let rate = DT / self.session.tuning().stroke.fade.max(1.0e-3);
        self.ghost = self
            .ghost
            .take()
            .map(|(line, life)| (line, life - rate))
            .filter(|(_, life)| *life > 0.0);
    }

    /// What the screen should show.
    fn compose(&self) -> GameView {
        let tuning = self.session.tuning();
        let short = self.surface.x.min(self.surface.y);
        let mut view = GameView::empty(
            self.session.phase(),
            self.surface,
            (self.session.tally().goals, self.session.tally().attempts),
        );
        view.banner = self.session.result().map(|r| r.banner());
        view.hint = hint_for(self.session.phase(), self.session.tally().attempts);
        // The line under the finger, or the one that just left it.
        let live = self.capture.drawing().then(|| StrokeView {
            points: self.capture.stroke().points().to_vec(),
            fade: 1.0,
            live: self.capture.stroke().length() >= short * tuning.stroke.min_length,
        });
        view.stroke = live.or_else(|| {
            self.ghost.as_ref().map(|(line, life)| StrokeView {
                points: line.points().to_vec(),
                fade: *life,
                live: true,
            })
        });
        view
    }

    /// Pose the scene and hand the frame to the renderer.
    pub fn present(&mut self) -> FrameOutcome {
        self.pose();
        let outcome = self.running.tick(self.frame_n);
        self.frame_n += 1;
        outcome
    }

    /// Pose the scene without ticking the engine — the capture harness poses
    /// here and lets the renderer drive the tick.
    pub fn pose(&mut self) {
        match self.debug {
            true => debug::markers(&self.session, self.reading.as_ref(), &mut self.markers),
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
    use crate::shot::ResolvedShot;

    /// Run frames until the game is ready for a drawing.
    fn armed(w: u32, h: u32) -> BendIt {
        let mut game = BendIt::new(w, h);
        while game.session().phase() != Phase::Aiming {
            let _ = game.frame(&[], &[]);
        }
        game
    }

    /// Trace the screen picture of a shot, drawing it one point per frame.
    fn draw_shot(game: &mut BendIt, shot: &ResolvedShot) {
        let projection = ScreenProjection::new(&game.camera(), game.surface);
        let points: Vec<Vec2> = (0..30)
            .filter_map(|i| projection.project(shot.trajectory.at_progress(i as f32 / 29.0)))
            .collect();
        points.iter().for_each(|p| {
            let _ = game.frame(&[], &[(*p, true)]);
        });
        let _ = game.frame(&[], &[]);
    }

    fn a_shot(game: &BendIt, h: f32, v: f32, bend: f32) -> ResolvedShot {
        use crate::shot::{BendCurve, GoalTarget, ShotIntent};
        ResolvedShot::build(
            game.session().shot().origin,
            ShotIntent {
                target: GoalTarget::new(h, v),
                bend: BendCurve::through(0.55, bend, 0.14),
                loft: BendCurve::through(0.5, 1.0, 0.14),
            },
            game.session().mouth(),
            game.session().tuning(),
        )
    }

    #[test]
    fn the_game_builds_and_waits_for_a_drawing() {
        let mut game = BendIt::new(390, 844);
        (0..30).for_each(|_| {
            let _ = game.frame(&[], &[]);
        });
        assert_eq!(game.frame_index(), 30);
        assert_eq!(game.session().phase(), Phase::Aiming);
        assert!(!game.debug_enabled());
        assert_eq!(game.view().hint, Some("DRAW THE SHOT"));
    }

    #[test]
    fn drawing_a_line_takes_the_shot_it_pictures() {
        let mut game = armed(390, 844);
        let wanted = a_shot(&game, -0.7, 0.6, 1.4);
        draw_shot(&mut game, &wanted);
        assert_eq!(game.session().phase(), Phase::ShotReady);
        let taken = game.session().shot();
        assert!(
            taken.world_target.subtract(wanted.world_target).length() < 0.4,
            "drew at {:?} but the kicker aimed at {:?}",
            wanted.world_target,
            taken.world_target
        );
        assert!(game.reading().is_some());
    }

    #[test]
    fn the_line_shows_while_it_is_drawn_and_leaves_when_it_is_let_go() {
        let mut game = armed(390, 844);
        let projection = ScreenProjection::new(&game.camera(), game.surface);
        let start = projection
            .project(game.session().shot().origin)
            .expect("the ball is on screen");
        (0..10).for_each(|i| {
            let at = Vec2::new(start.x, start.y - i as f32 * 18.0);
            let _ = game.frame(&[], &[(at, true)]);
        });
        let drawn = game.view().stroke.clone().expect("the line is drawn");
        assert!(drawn.points.len() > 3);
        assert_eq!(drawn.fade, 1.0);
        // Release, and it flicks away rather than lingering.
        let _ = game.frame(&[], &[]);
        let ghost = game.view().stroke.clone().expect("it is still leaving");
        assert!(ghost.fade < 1.0);
        let fade_frames = (game.session().tuning().stroke.fade / DT).ceil() as usize + 2;
        (0..fade_frames).for_each(|_| {
            let _ = game.frame(&[], &[]);
        });
        assert_eq!(game.view().stroke, None, "and then it is gone");
    }

    #[test]
    fn a_tap_is_not_a_shot() {
        let mut game = armed(390, 844);
        let _ = game.frame(&[], &[(Vec2::new(200.0, 700.0), true)]);
        let _ = game.frame(&[], &[(Vec2::new(202.0, 699.0), true)]);
        let _ = game.frame(&[], &[]);
        assert_eq!(game.session().phase(), Phase::Aiming, "still waiting");
        assert_eq!(game.session().tally().attempts, 0);
    }

    #[test]
    fn the_same_drawing_always_takes_the_same_shot() {
        let mut a = armed(390, 844);
        let mut b = armed(390, 844);
        let wanted = a_shot(&a, 0.5, 0.45, -1.2);
        draw_shot(&mut a, &wanted);
        draw_shot(&mut b, &wanted);
        assert_eq!(a.session().intent(), b.session().intent());
        assert_eq!(
            a.session().shot().world_target,
            b.session().shot().world_target
        );
    }

    #[test]
    fn the_debug_view_toggles_and_stays_out_of_the_normal_frame() {
        let mut game = armed(390, 844);
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
        let mut game = armed(390, 844);
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
