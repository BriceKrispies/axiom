//! The title screen: the oversized END ZONE mark over the live field, a
//! blinking PRESS START plate, and nothing else in the way.

use crate::frontend::actions::AudioIntent;
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{ArcadeButton, BackgroundView, HintSet, Placed, TitleLogo, Widget};

use super::ScreenBuild;

const START: WidgetId = WidgetId(1);

pub fn confirm(fe: &mut FrontendState) {
    fe.sound(AudioIntent::Confirm);
    fe.go(Screen::MainMenu, TransitionKind::Wipe);
}

pub fn build(
    _fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    theme: &Theme,
) -> ScreenBuild {
    let rows = centered_rows(shell.content, ctx.width.min(720.0), 170.0, 26.0, 2);
    let button_rect = crate::frontend::layout::rect(
        rows[1].x.get() + rows[1].w.get() / 2.0 - 150.0,
        rows[1].y.get() + 40.0,
        300.0,
        64.0,
    );
    let widgets = vec![
        Placed::new(
            WidgetId(10),
            rows[0],
            Widget::Logo(TitleLogo {
                small: false,
                press_start: false,
            }),
        ),
        Placed {
            focused: true,
            ..Placed::new(
                START,
                button_rect,
                Widget::Button(ArcadeButton::primary("PRESS START")),
            )
        },
    ];
    let entries = vec![FocusEntry::new(START, button_rect, 0, 0)];
    (
        widgets,
        entries,
        HintSet {
            navigate: false,
            adjust: false,
            confirm: Some("START"),
            cancel: None,
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.30,
            tint: None,
            animated: !theme.reduced_motion,
        },
        None,
    )
}
