//! The menu behind the title: exactly PLAY and SETTINGS over the attract field.
//! PLAY starts a fresh run; SETTINGS opens the shared settings screen (which
//! returns here on BACK). This is the pre-game menu where the menu music plays.

use crate::frontend::actions::AudioIntent;
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{ArcadeButton, BackgroundView, HintSet, Placed, TitleLogo, Widget};

use super::ScreenBuild;

const PLAY: WidgetId = WidgetId(1);
const SETTINGS: WidgetId = WidgetId(2);

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        PLAY => super::launch_fresh_run(fe, TransitionKind::Wipe),
        SETTINGS => {
            fe.sub_return = Screen::Menu;
            fe.sound(AudioIntent::Confirm);
            fe.go(Screen::Settings, TransitionKind::Fade);
        }
        _ => {}
    }
}

/// Leave the menu back to the title start plate (cancel).
pub fn back_to_title(fe: &mut FrontendState) {
    fe.sound(AudioIntent::Cancel);
    fe.go(Screen::Title, TransitionKind::Fade);
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
        Widget::Logo(TitleLogo {
            small: true,
            press_start: false,
        }),
    )];
    let mut entries = Vec::new();

    let width = (ctx.width * 0.5).clamp(300.0, 480.0);
    let items = [(PLAY, "PLAY", true), (SETTINGS, "SETTINGS", false)];
    let rows = centered_rows(shell.content, width, 62.0, 18.0, items.len());
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
            cancel: Some("BACK"),
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.4,
        },
    )
}
