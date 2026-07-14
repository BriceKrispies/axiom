//! Screen dispatch: routes device-independent actions to the active screen
//! (modal dialogs confine input first), and builds the per-tick scene view +
//! focus list. Screens never see raw devices; they see focused widget ids.

pub mod attract;
pub mod credits;
pub mod main_menu;
pub mod match_setup;
pub mod modal;
pub mod pause;
pub mod settings;
pub mod settings_rows;
pub mod settings_values;
pub mod team_select;
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

/// Handle one action against the current screen (or the open modal).
pub fn handle(fe: &mut FrontendState, action: DeviceAction) {
    fe.inactivity = 0;
    if fe.modal.is_some() {
        modal::handle(fe, action.action);
        return;
    }
    match action.action {
        FrontendAction::PointerMove { x, y } => {
            fe.focus.hover(x, y);
        }
        FrontendAction::PointerActivate { x, y } => {
            if let Some(id) = fe.focus.activate_at(x, y) {
                confirm(fe, id);
            } else if fe.screen == Screen::Attract || fe.screen == Screen::Title {
                // Anywhere counts as PRESS START on the attract/title flow.
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
    // Any directional press wakes attract mode.
    if fe.screen == Screen::Attract {
        attract::confirm(fe);
        return;
    }
    let (dx, dy) = match direction {
        NavDirection::Up => (0, -1),
        NavDirection::Down => (0, 1),
        NavDirection::Left => (-1, 0),
        NavDirection::Right => (1, 0),
    };
    // Screens may claim horizontal input for value adjustment / carousels.
    if dx != 0 {
        let claimed = match fe.screen {
            Screen::TeamSelect => team_select::adjust(fe, dx),
            Screen::MatchSetup => match_setup::adjust(fe, dx),
            Screen::Settings => settings::adjust(fe, dx),
            _ => false,
        };
        if claimed {
            fe.sound(AudioIntent::Navigate);
            return;
        }
    }
    if fe.focus.step(dx, dy) == MoveOutcome::Moved {
        fe.sound(AudioIntent::Navigate);
    }
}

fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match fe.screen {
        Screen::Attract => attract::confirm(fe),
        Screen::Title => title::confirm(fe),
        Screen::MainMenu => main_menu::confirm(fe, id),
        Screen::TeamSelect => team_select::confirm(fe, id),
        Screen::MatchSetup => match_setup::confirm(fe, id),
        Screen::Settings => settings::confirm(fe, id),
        Screen::Credits => credits::confirm(fe, id),
        Screen::Paused => pause::confirm(fe, id),
        Screen::InGame | Screen::TransitionToGame | Screen::TransitionToMenu => {}
    }
}

fn confirm_screen_default(fe: &mut FrontendState) {
    match fe.screen {
        Screen::Attract => attract::confirm(fe),
        Screen::Title => title::confirm(fe),
        _ => {}
    }
}

fn cancel(fe: &mut FrontendState) {
    match fe.screen {
        Screen::Attract => attract::confirm(fe),
        Screen::Title => {}
        Screen::MainMenu => {
            fe.sound(AudioIntent::Cancel);
            fe.go(Screen::Title, TransitionKind::Fade);
        }
        Screen::TeamSelect => team_select::cancel(fe),
        Screen::MatchSetup => match_setup::cancel(fe),
        Screen::Settings => settings::cancel(fe),
        Screen::Credits => credits::cancel(fe),
        Screen::InGame => pause_game(fe),
        Screen::Paused => pause::resume(fe),
        Screen::TransitionToGame | Screen::TransitionToMenu => {}
    }
}

fn pause(fe: &mut FrontendState) {
    match fe.screen {
        Screen::InGame => pause_game(fe),
        Screen::Paused => pause::resume(fe),
        Screen::Attract => attract::confirm(fe),
        _ => {}
    }
}

pub(crate) fn pause_game(fe: &mut FrontendState) {
    fe.command(FrontendCommand::SetPaused(true));
    fe.sound(AudioIntent::PauseHit);
    fe.go(Screen::Paused, TransitionKind::Fade);
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
    let (widgets, entries, hint_set, background, ticker) = match fe.screen {
        Screen::Attract => attract::build(fe, ctx, &shell, theme),
        Screen::Title => title::build(fe, ctx, &shell, theme),
        Screen::MainMenu => main_menu::build(fe, ctx, &shell, theme),
        Screen::TeamSelect => team_select::build(fe, ctx, &shell, theme),
        Screen::MatchSetup => match_setup::build(fe, ctx, &shell, theme),
        Screen::Settings => settings::build(fe, ctx, &shell, theme),
        Screen::Credits => credits::build(fe, ctx, &shell, theme),
        Screen::Paused => pause::build(fe, ctx, &shell, theme),
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
                tint: None,
                animated: !theme.reduced_motion,
            },
            None,
        ),
        Screen::TransitionToGame | Screen::TransitionToMenu => (
            Vec::new(),
            Vec::new(),
            HintSet {
                navigate: false,
                adjust: false,
                confirm: None,
                cancel: None,
                pause: None,
            },
            BackgroundView {
                show_field: true,
                dim: 0.4,
                tint: None,
                animated: !theme.reduced_motion,
            },
            None,
        ),
    };

    let (modal_view, modal_entries) = modal::build(fe, ctx);
    let hints: Vec<Hint> = hints_for(device, hint_set);
    let scene = SceneView {
        screen: fe.screen,
        widgets,
        modal: modal_view,
        hints,
        background,
        transition: fe.transition.as_ref().map(|t| TransitionView {
            kind: t.kind,
            progress: t.progress(),
        }),
        ticker,
    };
    // Modal confinement: when a dialog is open, ONLY its options are
    // focusable.
    let entries = if fe.modal.is_some() {
        modal_entries
    } else {
        entries
    };
    (scene, entries)
}

/// The tuple every screen's `build` returns.
pub type ScreenBuild = (
    Vec<crate::frontend::widgets::Placed>,
    Vec<FocusEntry>,
    HintSet,
    BackgroundView,
    Option<String>,
);
