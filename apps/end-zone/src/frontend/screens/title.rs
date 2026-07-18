//! The title screen: the oversized END ZONE mark over the live field, a
//! blinking PRESS START plate, and nothing else. Any confirm starts the run.

use crate::frontend::layout::{centered_rows, rect, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::FrontendState;
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{ArcadeButton, BackgroundView, HintSet, Placed, TitleLogo, Widget};

use super::ScreenBuild;

const START: WidgetId = WidgetId(1);

pub fn confirm(fe: &mut FrontendState) {
    super::launch_fresh_run(fe, TransitionKind::Wipe);
}

pub fn build(
    _fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    _theme: &Theme,
) -> ScreenBuild {
    let rows = centered_rows(shell.content, ctx.width.min(760.0), 180.0, 28.0, 2);
    let button_rect = rect(
        rows[1].x.get() + rows[1].w.get() / 2.0 - 160.0,
        rows[1].y.get() + 46.0,
        320.0,
        66.0,
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
        Placed::new(
            START,
            button_rect,
            Widget::Button(ArcadeButton::primary("PRESS START")),
        )
        .focused(true),
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
            dim: 0.28,
        },
    )
}
