//! The pause menu: exactly RESUME / RESTART MATCH / SETTINGS / RETURN TO
//! MAIN MENU over the frozen match. Return-to-menu confirms through the
//! app-styled modal; settings opened here return here with focus intact.

use crate::frontend::actions::{AudioIntent, FrontendCommand};
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, ModalKind, ModalState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, Widget,
};

use super::settings;
use super::ScreenBuild;

const RESUME: WidgetId = WidgetId(1);
const RESTART: WidgetId = WidgetId(2);
const SETTINGS: WidgetId = WidgetId(3);
const RETURN: WidgetId = WidgetId(4);

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        RESUME => resume(fe),
        RESTART => {
            fe.command(FrontendCommand::RestartMatch);
            fe.command(FrontendCommand::SetPaused(false));
            fe.sound(AudioIntent::Confirm);
            fe.go(Screen::InGame, TransitionKind::Fade);
        }
        SETTINGS => {
            fe.sound(AudioIntent::Confirm);
            settings::open(fe, Screen::Paused);
        }
        RETURN => {
            fe.modal = Some(ModalState {
                kind: ModalKind::ReturnToMenu,
                focused: 0,
            });
            fe.sound(AudioIntent::Navigate);
        }
        _ => {}
    }
}

/// Resume play (pause key, cancel, or the RESUME item).
pub fn resume(fe: &mut FrontendState) {
    fe.command(FrontendCommand::SetPaused(false));
    fe.sound(AudioIntent::ResumeRise);
    fe.go(Screen::InGame, TransitionKind::Fade);
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
            text: "PAUSED".to_string(),
            size: LabelSize::Huge,
            accent: None,
            italic: true,
        }),
    )];
    let mut entries = Vec::new();

    let width = (ctx.width * 0.5).clamp(280.0, 480.0);
    let rows = centered_rows(shell.content, width, 64.0, 18.0, 4);
    let items = [
        (RESUME, "RESUME", true),
        (RESTART, "RESTART MATCH", false),
        (SETTINGS, "SETTINGS", false),
        (RETURN, "RETURN TO MAIN MENU", false),
    ];
    for (index, (id, label, primary)) in items.into_iter().enumerate() {
        let rect = rows[index];
        let button = if primary {
            ArcadeButton::primary(label)
        } else {
            ArcadeButton::flat(label)
        };
        widgets.push(Placed {
            focused: focused == Some(id),
            ..Placed::new(id, rect, Widget::Button(button))
        });
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
            tint: None,
            // The match is frozen behind the pause menu; nothing animates.
            animated: false,
        },
        None,
    )
}
