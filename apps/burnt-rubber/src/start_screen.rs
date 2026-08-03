//! The pre-race start screen: a title over the night road, and one button.
//!
//! The whole of it is a **model**. It owns two rectangles and answers one
//! question per frame — did the player start the race? — and it draws nothing.
//! The `wasm32` arm paints [`StartScreen::layout`] and feeds pointer presses
//! back in, which is the same division of labour [`crate::touch`] uses for the
//! on-screen pad and for the same reason: laying the button out and deciding
//! whether a press landed on it are decisions worth testing, and neither of them
//! needs a browser.
//!
//! ## It is presentation, not simulation
//!
//! Nothing here is on the fixed step and nothing here is replayed: the race has
//! not started yet. Nothing crosses into the deterministic half at all — the
//! screen's only output is "go", and the app answers it by building the race
//! exactly as it always did. That is why the screen can use pixels and pointer
//! positions freely without any of it reaching [`crate::sim`].

use axiom_math::Vec2;

/// The title across the top of the screen.
pub const START_TITLE: &str = "BURNT RUBBER";
/// The line under it.
pub const START_SUBTITLE: &str = "Nine kilometres of night road. Fill the meter, spend it.";
/// The label on the button.
pub const START_LABEL: &str = "START RACE";
/// The hint under the button.
pub const START_HINT: &str = "ENTER  ·  SPACE  ·  TAP";

/// An axis-aligned rectangle in pixels, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// A rectangle from its corner and size.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// The right edge.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// The bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// The centre.
    pub fn centre(&self) -> Vec2 {
        Vec2::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Whether `point` is inside, edges included.
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }
}

/// Where the title and the button sit, for one viewport.
///
/// Derived, never authored: one scale factor taken from the **smaller** viewport
/// dimension, exactly as [`crate::touch::PadLayout`] sizes the on-screen pad. It
/// is the same question ("how big is a thumb here") and two different answers
/// would show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartLayout {
    /// The viewport this was solved for.
    pub viewport: Vec2,
    /// The title band, across the upper third.
    pub title: Rect,
    /// The button.
    pub start: Rect,
}

impl StartLayout {
    /// Solve for a viewport.
    ///
    /// Deterministic: the same size always produces the same rectangles, with no
    /// state and no clock anywhere.
    pub fn for_viewport(width: f32, height: f32) -> StartLayout {
        let viewport = Vec2::new(width.max(1.0), height.max(1.0));
        let short = viewport.x.min(viewport.y);
        let margin = (short * 0.05).clamp(14.0, 48.0);

        // The title sits in the upper third and the button just below the
        // middle, so the road between and below them is the thing the eye lands
        // on — the screen is a title over the game, not a page in front of it.
        let title = Rect::new(
            margin,
            (viewport.y * 0.22).min(viewport.y * 0.5),
            (viewport.x - margin * 2.0).max(1.0),
            (short * 0.22).clamp(70.0, 190.0),
        );

        let button_width = (viewport.x - margin * 2.0)
            .max(1.0)
            .min(MAX_BUTTON_WIDTH);
        let button_height = (short * 0.11).clamp(MIN_TOUCH_TARGET, 92.0);
        // Below the title but *above* the car, which sits in the lower middle of
        // the chase camera's frame: a button over the car has the tail lights
        // glowing through it, and the one thing on this screen that has to be
        // legible is the button.
        let button_top = (viewport.y * 0.48)
            .max(title.bottom() + margin)
            .min((viewport.y - margin - button_height).max(0.0));
        let start = Rect::new(
            (viewport.x - button_width) * 0.5,
            button_top,
            button_width,
            button_height,
        );

        StartLayout {
            viewport,
            title,
            start,
        }
    }
}

/// Widest the button may grow to (px), so a desktop gets a button and not a
/// banner.
const MAX_BUTTON_WIDTH: f32 = 460.0;

/// The smallest comfortable touch target (px) — the floor the button is held
/// above however short the viewport is.
pub const MIN_TOUCH_TARGET: f32 = 48.0;

/// One frame of intent, in the same spirit as [`crate::command::DriveCommand`]:
/// the only thing the screen reads from the outside world.
///
/// `confirm` is **edge-triggered by the caller** — [`crate::controls::Controls`]
/// supplies it from the action table's press edge — so holding a key starts the
/// race once, not sixty times a second.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StartCommand {
    /// A confirmation press.
    pub confirm: bool,
    /// A pointer press, in viewport pixels.
    pub pointer: Option<Vec2>,
}

impl StartCommand {
    /// No input at all.
    pub const IDLE: StartCommand = StartCommand {
        confirm: false,
        pointer: None,
    };

    /// A confirmation press.
    pub const CONFIRM: StartCommand = StartCommand {
        confirm: true,
        ..StartCommand::IDLE
    };

    /// A pointer press at `point`.
    pub const fn tap(point: Vec2) -> StartCommand {
        StartCommand {
            pointer: Some(point),
            ..StartCommand::IDLE
        }
    }
}

/// What one frame of input did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// Nothing anything outside the screen needs to know about.
    Idle,
    /// The player started the race.
    Started,
}

/// The pre-race screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartScreen {
    layout: StartLayout,
}

impl StartScreen {
    /// Open the screen for a viewport.
    pub fn open(width: f32, height: f32) -> StartScreen {
        StartScreen {
            layout: StartLayout::for_viewport(width, height),
        }
    }

    /// The rectangles this frame should be drawn from.
    pub const fn layout(&self) -> &StartLayout {
        &self.layout
    }

    /// Re-solve for a new viewport.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.layout = StartLayout::for_viewport(width, height);
    }

    /// Fold one frame of input.
    ///
    /// A pointer press only counts on the button. A press anywhere else on the
    /// screen does nothing at all — the road behind is not a start button, and
    /// a stray tap while getting comfortable must not drop the flag.
    pub fn update(&self, command: StartCommand) -> StartOutcome {
        let tapped = command
            .pointer
            .is_some_and(|point| self.layout.start.contains(point));
        (command.confirm | tapped)
            .then_some(StartOutcome::Started)
            .unwrap_or(StartOutcome::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The viewports the app is actually played on.
    const VIEWPORTS: [(&str, f32, f32); 5] = [
        ("narrow portrait", 360.0, 740.0),
        ("wide portrait", 430.0, 932.0),
        ("landscape phone", 844.0, 390.0),
        ("tablet portrait", 820.0, 1180.0),
        ("desktop", 1600.0, 900.0),
    ];

    #[test]
    fn the_button_is_centred_and_on_screen_at_every_viewport() {
        for (name, width, height) in VIEWPORTS {
            let layout = StartLayout::for_viewport(width, height);
            let left = layout.start.x;
            let right = width - layout.start.right();
            assert!(
                (left - right).abs() < 1.0,
                "{name}: the button is off centre by {} px",
                (left - right).abs()
            );
            assert!(layout.start.x >= 0.0, "{name}");
            assert!(layout.start.right() <= width + 0.5, "{name}");
            assert!(layout.start.bottom() <= height + 0.5, "{name}: off the bottom");
            assert!(
                layout.start.height >= MIN_TOUCH_TARGET,
                "{name}: {} px is under a thumb",
                layout.start.height
            );
            assert!(
                layout.start.width <= MAX_BUTTON_WIDTH + 0.5,
                "{name}: a button, not a banner"
            );
        }
    }

    #[test]
    fn the_title_sits_above_the_button_and_never_collides_with_it() {
        for (name, width, height) in VIEWPORTS {
            let layout = StartLayout::for_viewport(width, height);
            assert!(
                layout.title.bottom() <= layout.start.y + 0.5,
                "{name}: the title runs into the button"
            );
            assert!(layout.title.x >= 0.0 && layout.title.right() <= width + 0.5, "{name}");
            assert!(layout.title.height > 0.0 && layout.title.width > 0.0, "{name}");
        }
    }

    /// A short landscape phone is the case that squeezes: the button must still
    /// be a usable target and still be on screen.
    #[test]
    fn a_very_short_viewport_still_produces_a_usable_button() {
        for height in [200.0f32, 260.0, 320.0] {
            let layout = StartLayout::for_viewport(844.0, height);
            assert!(layout.start.height >= MIN_TOUCH_TARGET, "{height}");
            assert!(layout.start.bottom() <= height + 0.5, "{height}");
            assert!(layout.start.y >= 0.0, "{height}");
        }
    }

    #[test]
    fn a_degenerate_viewport_still_lays_out_finite_rectangles() {
        let layout = StartLayout::for_viewport(0.0, 0.0);
        for rect in [layout.title, layout.start] {
            assert!(rect.x.is_finite() && rect.y.is_finite());
            assert!(rect.width > 0.0 && rect.height > 0.0);
        }
        assert!(layout.start.height >= MIN_TOUCH_TARGET);
    }

    #[test]
    fn the_same_viewport_always_produces_the_same_layout() {
        assert_eq!(
            StartLayout::for_viewport(1280.0, 720.0),
            StartLayout::for_viewport(1280.0, 720.0)
        );
        assert_ne!(
            StartLayout::for_viewport(1280.0, 720.0),
            StartLayout::for_viewport(360.0, 740.0)
        );
    }

    #[test]
    fn confirming_starts_the_race_and_an_idle_frame_does_not() {
        let screen = StartScreen::open(1280.0, 720.0);
        assert_eq!(screen.update(StartCommand::IDLE), StartOutcome::Idle);
        assert_eq!(screen.update(StartCommand::CONFIRM), StartOutcome::Started);
    }

    /// A tap on the button starts the race; a tap anywhere else is the road, not
    /// a button.
    #[test]
    fn only_a_press_on_the_button_starts_the_race() {
        let screen = StartScreen::open(1280.0, 720.0);
        let start = screen.layout().start;
        assert_eq!(
            screen.update(StartCommand::tap(start.centre())),
            StartOutcome::Started
        );
        // Every edge is inside.
        for corner in [
            Vec2::new(start.x, start.y),
            Vec2::new(start.right(), start.bottom()),
        ] {
            assert_eq!(screen.update(StartCommand::tap(corner)), StartOutcome::Started);
        }
        // And just outside is outside.
        for miss in [
            Vec2::new(start.x - 2.0, start.centre().y),
            Vec2::new(start.right() + 2.0, start.centre().y),
            Vec2::new(start.centre().x, start.y - 2.0),
            Vec2::new(start.centre().x, start.bottom() + 2.0),
            Vec2::new(4.0, 4.0),
        ] {
            assert_eq!(
                screen.update(StartCommand::tap(miss)),
                StartOutcome::Idle,
                "a press at {miss:?} started the race"
            );
        }
    }

    #[test]
    fn resizing_re_solves_the_layout() {
        let mut screen = StartScreen::open(1600.0, 900.0);
        let before = *screen.layout();
        screen.resize(360.0, 740.0);
        assert_ne!(*screen.layout(), before);
        assert_eq!(screen.layout().viewport, Vec2::new(360.0, 740.0));
        // And the button is still hit-testable where it now is.
        assert_eq!(
            screen.update(StartCommand::tap(screen.layout().start.centre())),
            StartOutcome::Started
        );
    }

    #[test]
    fn rectangles_answer_the_geometry_questions_they_are_asked() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.right(), 110.0);
        assert_eq!(r.bottom(), 70.0);
        assert_eq!(r.centre(), Vec2::new(60.0, 45.0));
        assert!(r.contains(Vec2::new(10.0, 20.0)), "edges are inside");
        assert!(!r.contains(Vec2::new(9.0, 45.0)));
        assert!(!r.contains(Vec2::new(60.0, 71.0)));
    }

    #[test]
    fn the_screen_copy_is_the_authored_copy() {
        assert_eq!(START_TITLE, "BURNT RUBBER");
        assert_eq!(START_LABEL, "START RACE");
        assert!(!START_SUBTITLE.is_empty());
        assert!(!START_HINT.is_empty());
    }
}
