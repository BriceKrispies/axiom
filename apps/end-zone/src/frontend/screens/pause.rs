//! The pause menu: exactly RESUME / RESTART RUN / SETTINGS / CONTROLS / RETURN
//! TO TITLE over the frozen run. No confirmation dialogs — restart and return
//! act immediately.

use crate::frontend::actions::{AudioIntent, FrontendCommand};
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, Widget,
};

use super::ScreenBuild;

const RESUME: WidgetId = WidgetId(1);
const RESTART: WidgetId = WidgetId(2);
const SETTINGS: WidgetId = WidgetId(3);
const CONTROLS: WidgetId = WidgetId(4);
const RETURN: WidgetId = WidgetId(5);

/// Enter the pause menu, freezing the run.
pub fn open(fe: &mut FrontendState) {
    fe.command(FrontendCommand::SetPaused(true));
    fe.sound(AudioIntent::PauseHit);
    fe.go(Screen::Paused, TransitionKind::Fade);
}

/// Resume play (pause key, cancel, or the RESUME item).
pub fn resume(fe: &mut FrontendState) {
    fe.command(FrontendCommand::SetPaused(false));
    fe.sound(AudioIntent::ResumeRise);
    fe.go(Screen::InGame, TransitionKind::Fade);
}

/// Return to the pause menu from a settings/controls sub-screen.
pub fn back_to_pause(fe: &mut FrontendState) {
    fe.sound(AudioIntent::Cancel);
    fe.go(Screen::Paused, TransitionKind::Fade);
}

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        RESUME => resume(fe),
        RESTART => {
            fe.command(FrontendCommand::RestartRun);
            fe.command(FrontendCommand::SetPaused(false));
            fe.summary = None;
            fe.sound(AudioIntent::Confirm);
            fe.go(Screen::InGame, TransitionKind::ScaleImpact);
        }
        SETTINGS => {
            fe.sound(AudioIntent::Confirm);
            fe.go(Screen::Settings, TransitionKind::Fade);
        }
        CONTROLS => {
            fe.sound(AudioIntent::Confirm);
            fe.go(Screen::Controls, TransitionKind::Fade);
        }
        RETURN => {
            fe.command(FrontendCommand::ReturnToTitle);
            fe.summary = None;
            fe.sound(AudioIntent::Cancel);
            fe.go(Screen::Title, TransitionKind::Fade);
        }
        _ => {}
    }
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    _theme: &Theme,
) -> ScreenBuild {
    let focused = fe.focus.focused();
    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Label(Label {
            italic: true,
            ..Label::new("PAUSED", LabelSize::Huge)
        }),
    )];
    let mut entries = Vec::new();

    let width = (ctx.width * 0.5).clamp(300.0, 480.0);
    let items = [
        (RESUME, "RESUME", true),
        (RESTART, "RESTART RUN", false),
        (SETTINGS, "SETTINGS", false),
        (CONTROLS, "CONTROLS", false),
        (RETURN, "RETURN TO TITLE", false),
    ];
    let rows = centered_rows(shell.content, width, 58.0, 16.0, items.len());
    for (index, (id, label, primary)) in items.into_iter().enumerate() {
        let rect = rows[index];
        let button = if primary {
            ArcadeButton::primary(label)
        } else {
            ArcadeButton::flat(label)
        };
        widgets.push(Placed::new(id, rect, Widget::Button(button)).focused(focused == Some(id)));
        entries.push(FocusEntry::new(id, rect, index as i16, 0));
    }

    (
        widgets,
        entries,
        HintSet {
            navigate: true,
            adjust: false,
            confirm: Some("SELECT"),
            cancel: Some("RESUME"),
            pause: Some("RESUME"),
        },
        BackgroundView {
            show_field: true,
            dim: 0.55,
        },
    )
}
