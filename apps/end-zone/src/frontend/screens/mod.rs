//! Screen dispatch: routes device-independent actions to the active screen, and
//! builds the per-tick scene view + focus list. Screens never see raw devices;
//! they see focused widget ids.

pub mod controls;
pub mod gameover;
pub mod pause;
pub mod settings;
pub mod title;

use crate::frontend::actions::{
    AudioIntent, DeviceAction, FrontendAction, FrontendCommand, InputDevice, NavDirection,
};
use crate::frontend::layout::LayoutContext;
use crate::frontend::navigation::{FocusEntry, MoveOutcome, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::{TransitionKind, TransitionView};
use crate::frontend::widgets::{hints_for, BackgroundView, Hint, HintSet, SceneView};

/// The tuple every screen's `build` returns.
pub type ScreenBuild = (
    Vec<crate::frontend::widgets::Placed>,
    Vec<FocusEntry>,
    HintSet,
    BackgroundView,
);

/// Handle one action against the current screen.
pub fn handle(fe: &mut FrontendState, action: DeviceAction) {
    match action.action {
        FrontendAction::PointerMove { x, y } => {
            fe.focus.hover(x, y);
        }
        FrontendAction::PointerActivate { x, y } => {
            if let Some(id) = fe.focus.activate_at(x, y) {
                confirm(fe, id);
            } else {
                confirm_screen_default(fe);
            }
        }
        FrontendAction::Navigate(direction) => navigate(fe, direction),
        FrontendAction::Confirm => match fe.focus.focused() {
            Some(id) => confirm(fe, id),
            None => confirm_screen_default(fe),
        },
        FrontendAction::Cancel => cancel(fe),
        FrontendAction::Pause => pause(fe),
    }
}

fn navigate(fe: &mut FrontendState, direction: NavDirection) {
    let (dx, dy) = match direction {
        NavDirection::Up => (0, -1),
        NavDirection::Down => (0, 1),
        NavDirection::Left => (-1, 0),
        NavDirection::Right => (1, 0),
    };
    // The settings screen claims horizontal input for value adjustment.
    if dx != 0 && fe.screen == Screen::Settings && settings::adjust(fe, dx) {
        fe.sound(AudioIntent::Navigate);
        return;
    }
    if fe.focus.step(dx, dy) == MoveOutcome::Moved {
        fe.sound(AudioIntent::Navigate);
    }
}

fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match fe.screen {
        Screen::Title => title::confirm(fe),
        Screen::Paused => pause::confirm(fe, id),
        Screen::Settings => settings::confirm(fe, id),
        Screen::Controls => controls::confirm(fe, id),
        Screen::GameOver => gameover::confirm(fe, id),
        Screen::InGame => {}
    }
}

fn confirm_screen_default(fe: &mut FrontendState) {
    // Anywhere counts as PRESS START on the title.
    if fe.screen == Screen::Title {
        title::confirm(fe);
    }
}

fn cancel(fe: &mut FrontendState) {
    match fe.screen {
        Screen::InGame => pause::open(fe),
        Screen::Paused => pause::resume(fe),
        Screen::Settings | Screen::Controls => pause::back_to_pause(fe),
        Screen::Title | Screen::GameOver => {}
    }
}

fn pause(fe: &mut FrontendState) {
    match fe.screen {
        Screen::InGame => pause::open(fe),
        Screen::Paused => pause::resume(fe),
        _ => {}
    }
}

// --- scene building -----------------------------------------------------------

/// Build the scene + focus entries for this tick.
pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    theme: &Theme,
    device: InputDevice,
) -> (SceneView, Vec<FocusEntry>) {
    let shell = ctx.shell();
    let (widgets, entries, hint_set, background) = match fe.screen {
        Screen::Title => title::build(fe, ctx, &shell, theme),
        Screen::Paused => pause::build(fe, ctx, &shell, theme),
        Screen::Settings => settings::build(fe, ctx, &shell, theme),
        Screen::Controls => controls::build(fe, ctx, &shell, theme),
        Screen::GameOver => gameover::build(fe, ctx, &shell, theme),
        Screen::InGame => (
            Vec::new(),
            Vec::new(),
            HintSet {
                navigate: false,
                adjust: false,
                confirm: None,
                cancel: None,
                pause: Some("PAUSE"),
            },
            BackgroundView {
                show_field: true,
                dim: 0.0,
            },
        ),
    };

    let hints: Vec<Hint> = hints_for(device, hint_set);
    let scene = SceneView {
        screen: fe.screen,
        widgets,
        hints,
        background,
        transition: fe.transition.as_ref().map(|t| TransitionView {
            kind: t.kind,
            progress: t.progress(),
        }),
    };
    (scene, entries)
}

/// Emit the SNAP-latched launch of a fresh run at a new explicit seed and enter
/// gameplay with an impact transition. Shared by title START and PLAY AGAIN.
pub(crate) fn launch_fresh_run(fe: &mut FrontendState, kind: TransitionKind) {
    let seed = fe.next_run_seed();
    fe.command(FrontendCommand::LaunchRun { seed });
    fe.command(FrontendCommand::SetPaused(false));
    fe.summary = None;
    fe.sound(AudioIntent::Confirm);
    fe.go(Screen::InGame, kind);
}
