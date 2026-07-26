//! The session summary: what the prototype's attempts added up to (how many,
//! how many were completed, the best gain, how many were given away), and
//! exactly PLAY AGAIN / RETURN TO TITLE.
//!
//! These are *statistics*, not progression — nothing here feeds back into the
//! next session's difficulty. The prototype is answering a design question, and
//! the numbers exist so the answer can be argued about.

use crate::attempt::SessionSummary;
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
    let summary = fe.summary.unwrap_or(SessionSummary {
        attempts: 0,
        completions: 0,
        touchdowns: 0,
        interceptions: 0,
        sacks: 0,
        best_yards: 0,
        yards_per_attempt: 0.0,
    });

    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Label(Label {
            italic: true,
            accent: Some("#39c0ff".to_string()),
            ..Label::new("SESSION", LabelSize::Huge)
        }),
    )];

    let stats = [
        ("ATTEMPTS", summary.attempts.to_string()),
        (
            "COMPLETIONS",
            format!("{} / {}", summary.completions, summary.attempts),
        ),
        ("YARDS PER TRY", format!("{:.1}", summary.yards_per_attempt)),
        ("BEST GAIN", format!("{} YD", summary.best_yards)),
        (
            "GIVEN AWAY",
            format!("{} INT   {} SACK", summary.interceptions, summary.sacks),
        ),
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
