//! The game-over screen: RUN OVER, the run summary (final score, touchdowns,
//! first downs, longest play), and exactly PLAY AGAIN / RETURN TO TITLE.

use crate::drive::RunSummary;
use crate::frontend::actions::{AudioIntent, FrontendCommand};
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, SettingRow, Widget,
};

use super::ScreenBuild;

const PLAY_AGAIN: WidgetId = WidgetId(1);
const RETURN: WidgetId = WidgetId(2);

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        PLAY_AGAIN => super::launch_fresh_run(fe, TransitionKind::ScaleImpact),
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
    let summary = fe.summary.unwrap_or(RunSummary {
        score: 0,
        touchdowns: 0,
        first_downs: 0,
        longest_play: 0,
    });

    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Label(Label {
            italic: true,
            accent: Some("#e33e30".to_string()),
            ..Label::new("RUN OVER", LabelSize::Huge)
        }),
    )];

    let stats = [
        ("FINAL SCORE", format!("{:06}", summary.score)),
        ("TOUCHDOWNS", summary.touchdowns.to_string()),
        ("FIRST DOWNS", summary.first_downs.to_string()),
        ("LONGEST PLAY", format!("{} YD", summary.longest_play)),
    ];
    let stat_count = stats.len();
    let width = (ctx.width * 0.62).clamp(320.0, 520.0);
    let rects = centered_rows(shell.content, width, 52.0, 14.0, stat_count + 2);
    for (index, (label, value)) in stats.into_iter().enumerate() {
        widgets.push(Placed::new(
            WidgetId(100 + index as u32),
            rects[index],
            Widget::Setting(SettingRow {
                label: label.to_string(),
                value,
                fill: None,
            }),
        ));
    }

    let again_rect = rects[stat_count];
    let return_rect = rects[stat_count + 1];
    widgets.push(
        Placed::new(
            PLAY_AGAIN,
            again_rect,
            Widget::Button(ArcadeButton::primary("PLAY AGAIN")),
        )
        .focused(focused == Some(PLAY_AGAIN) || focused.is_none()),
    );
    widgets.push(
        Placed::new(
            RETURN,
            return_rect,
            Widget::Button(ArcadeButton::flat("RETURN TO TITLE")),
        )
        .focused(focused == Some(RETURN)),
    );
    let entries = vec![
        FocusEntry::new(PLAY_AGAIN, again_rect, 0, 0),
        FocusEntry::new(RETURN, return_rect, 1, 0),
    ];

    (
        widgets,
        entries,
        HintSet {
            navigate: true,
            adjust: false,
            confirm: Some("SELECT"),
            cancel: None,
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.62,
        },
    )
}
