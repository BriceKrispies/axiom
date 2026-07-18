//! The controls screen: a read-only list of the actual current controls,
//! derived from the fixed input map, plus BACK. There is no rebinding, no
//! profiles, and no conflict detection.

use crate::frontend::bindings::{token_label, BindableAction, ControlBindings};
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::FrontendState;
use crate::frontend::theme::Theme;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, SettingRow, Widget,
};

use super::ScreenBuild;

const BACK: WidgetId = WidgetId(1);

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    if id == BACK {
        super::pause::back_to_pause(fe);
    }
}

/// The control rows shown, in order (the four move keys are one MOVE row).
fn rows() -> Vec<(&'static str, String)> {
    let bindings = ControlBindings::default();
    let joined = |action: BindableAction| -> String {
        bindings
            .tokens(action)
            .iter()
            .map(|t| token_label(t))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    vec![
        ("MOVE", "W A S D · ARROWS · D-PAD".to_string()),
        ("SNAP / THROW", joined(BindableAction::GamePrimary)),
        ("PAUSE", joined(BindableAction::Pause)),
        ("CONFIRM", joined(BindableAction::Confirm)),
        ("CANCEL / BACK", joined(BindableAction::Cancel)),
    ]
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
            ..Label::new("CONTROLS", LabelSize::Huge)
        }),
    )];

    let control_rows = rows();
    let width = (ctx.width * 0.72).clamp(320.0, 580.0);
    let rects = centered_rows(shell.content, width, 54.0, 14.0, control_rows.len() + 1);
    for (index, (label, value)) in control_rows.into_iter().enumerate() {
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

    let back_rect = rects[rects.len() - 1];
    let entries = vec![FocusEntry::new(BACK, back_rect, 0, 0)];
    widgets.push(
        Placed::new(BACK, back_rect, Widget::Button(ArcadeButton::flat("BACK")))
            .focused(focused == Some(BACK) || focused.is_none()),
    );

    (
        widgets,
        entries,
        HintSet {
            navigate: false,
            adjust: false,
            confirm: Some("BACK"),
            cancel: Some("BACK"),
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.6,
        },
    )
}
