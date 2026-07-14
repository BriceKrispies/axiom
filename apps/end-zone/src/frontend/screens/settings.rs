//! The settings screen: five category tabs over typed rows, an explicit
//! WORKING copy with live preview, and APPLY / RESET DEFAULTS / BACK.
//! Backing out with unapplied changes raises the app-styled discard dialog.
//! Reached from MainMenu or Paused; closing restores the exact origin.

use crate::frontend::actions::AudioIntent;
use crate::frontend::bindings::ControlBindings;
use crate::frontend::layout::{split_columns, stack_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::settings::{EndZoneSettings, SettingsCategory};
use crate::frontend::state::{
    FrontendState, ModalKind, ModalState, Screen, SettingsEdit, CAPTURE_TIMEOUT,
};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, CategoryTabs, HintSet, Placed, RowControl, SettingRow, Widget,
};

use super::settings_rows::{activate_field, adjust_field, fields_for, row_view, SettingField};
use super::ScreenBuild;

const TAB_BASE: u32 = 100;
const ROW_BASE: u32 = 200;
const APPLY: WidgetId = WidgetId(300);
const RESET: WidgetId = WidgetId(301);
const BACK: WidgetId = WidgetId(302);

/// Open the editor from `origin` with a working copy of the committed state.
pub fn open(fe: &mut FrontendState, origin: Screen) {
    fe.settings_edit = Some(SettingsEdit {
        origin,
        category: fe.profile.last_category,
        working: fe.profile.settings,
        working_bindings: fe.profile.bindings.clone(),
        capture: None,
    });
    fe.go(Screen::Settings, TransitionKind::AngledSlide);
}

fn dirty(fe: &FrontendState) -> bool {
    fe.settings_edit
        .as_ref()
        .map(|e| e.working != fe.profile.settings || e.working_bindings != fe.profile.bindings)
        .unwrap_or(false)
}

/// Left/right adjusts the focused row's value.
pub fn adjust(fe: &mut FrontendState, dx: i32) -> bool {
    let Some(focused) = fe.focus.focused() else {
        return false;
    };
    let Some(edit) = fe.settings_edit.as_mut() else {
        return false;
    };
    if focused.0 < ROW_BASE || focused.0 >= 300 {
        return false;
    }
    let fields = fields_for(edit.category);
    let index = (focused.0 - ROW_BASE) as usize;
    let Some(field) = fields.get(index) else {
        return false;
    };
    adjust_field(*field, &mut edit.working, dx)
}

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        APPLY => apply(fe),
        RESET => reset_defaults(fe),
        BACK => cancel(fe),
        _ if id.0 >= TAB_BASE && id.0 < TAB_BASE + 5 => {
            let category = SettingsCategory::ALL[(id.0 - TAB_BASE) as usize];
            if let Some(edit) = fe.settings_edit.as_mut() {
                edit.category = category;
                edit.capture = None;
            }
            fe.sound(AudioIntent::Navigate);
        }
        _ if id.0 >= ROW_BASE && id.0 < 300 => {
            let Some(edit) = fe.settings_edit.as_mut() else {
                return;
            };
            let fields = fields_for(edit.category);
            let index = (id.0 - ROW_BASE) as usize;
            let Some(field) = fields.get(index).copied() else {
                return;
            };
            match field {
                SettingField::Bind(action) => {
                    edit.capture = Some((action, CAPTURE_TIMEOUT));
                    fe.sound(AudioIntent::Navigate);
                }
                SettingField::RestoreBindings => {
                    edit.working_bindings.restore_all();
                    fe.sound(AudioIntent::Confirm);
                }
                _ => {
                    if activate_field(field, &mut edit.working) {
                        fe.sound(AudioIntent::Navigate);
                    }
                }
            }
        }
        _ => {}
    }
}

/// A captured raw token completes (or conflicts) an active rebind.
pub fn captured_token(fe: &mut FrontendState, token: &str) {
    let Some(edit) = fe.settings_edit.as_mut() else {
        return;
    };
    let Some((action, _)) = edit.capture.take() else {
        return;
    };
    if token == "Escape" {
        fe.sound(AudioIntent::Cancel);
        return;
    }
    edit.working_bindings.rebind(action, token);
    fe.sound(AudioIntent::Confirm);
}

fn apply(fe: &mut FrontendState) {
    let Some(edit) = fe.settings_edit.as_ref() else {
        return;
    };
    fe.profile.settings = edit.working.sanitized();
    fe.profile.bindings = edit.working_bindings.clone();
    fe.profile.last_category = edit.category;
    fe.persist_requested = true;
    fe.sound(AudioIntent::Confirm);
}

fn reset_defaults(fe: &mut FrontendState) {
    if let Some(edit) = fe.settings_edit.as_mut() {
        edit.working = EndZoneSettings::default();
        edit.working_bindings = ControlBindings::default();
        edit.capture = None;
    }
    fe.sound(AudioIntent::Cancel);
}

/// Discard confirmed from the modal: restore committed values and close.
pub fn discard(fe: &mut FrontendState) {
    let origin = fe
        .settings_edit
        .as_ref()
        .map(|e| e.origin)
        .unwrap_or(Screen::MainMenu);
    fe.settings_edit = None;
    fe.sound(AudioIntent::Cancel);
    fe.go(origin, TransitionKind::AngledSlide);
}

pub fn cancel(fe: &mut FrontendState) {
    if fe
        .settings_edit
        .as_ref()
        .map(|e| e.capture.is_some())
        .unwrap_or(false)
    {
        if let Some(edit) = fe.settings_edit.as_mut() {
            edit.capture = None;
        }
        fe.sound(AudioIntent::Cancel);
        return;
    }
    if dirty(fe) {
        fe.modal = Some(ModalState {
            kind: ModalKind::DiscardSettings,
            focused: 0,
        });
        fe.sound(AudioIntent::Denied);
        return;
    }
    let origin = fe
        .settings_edit
        .as_ref()
        .map(|e| e.origin)
        .unwrap_or(Screen::MainMenu);
    fe.settings_edit = None;
    fe.sound(AudioIntent::Cancel);
    fe.go(origin, TransitionKind::AngledSlide);
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    theme: &Theme,
) -> ScreenBuild {
    let Some(edit) = fe.settings_edit.as_ref() else {
        return (
            Vec::new(),
            Vec::new(),
            HintSet::menu(),
            BackgroundView {
                show_field: true,
                dim: 0.6,
                tint: None,
                animated: !theme.reduced_motion,
            },
            None,
        );
    };
    let focused = fe.focus.focused();
    let mut widgets = Vec::new();
    let mut entries = Vec::new();

    // Category tab strip across the header.
    let tabs_rect = shell.header;
    widgets.push(Placed::new(
        WidgetId(11),
        tabs_rect,
        Widget::Tabs(CategoryTabs {
            labels: SettingsCategory::ALL
                .iter()
                .map(|c| c.label().to_string())
                .collect(),
            active: edit.category.index(),
        }),
    ));
    let tab_cols = split_columns(tabs_rect, &[1.0, 1.0, 1.0, 1.0, 1.0], 6.0);
    for (index, rect) in tab_cols.iter().enumerate() {
        entries.push(FocusEntry::new(
            WidgetId(TAB_BASE + index as u32),
            *rect,
            0,
            index as i16,
        ));
    }

    // Rows for the active category (scrolls in the presenter when tall).
    let fields = fields_for(edit.category);
    let content = ctx.bounded(shell.content, 880.0);
    let row_h = if ctx.portrait { 58.0 } else { 52.0 };
    let rows = stack_rows(content, row_h, 10.0, fields.len());
    for (index, (field, rect)) in fields.iter().zip(rows.iter()).enumerate() {
        let id = WidgetId(ROW_BASE + index as u32);
        let row: SettingRow = row_view(*field, &edit.working, &edit.working_bindings, edit.capture);
        let selectable = !matches!(row.control, RowControl::ReadOnly);
        widgets.push(Placed {
            focused: focused == Some(id),
            enabled: selectable,
            ..Placed::new(id, *rect, Widget::SettingRow(row))
        });
        let entry = FocusEntry::new(id, *rect, 1 + index as i16, 0);
        entries.push(if selectable { entry } else { entry.disabled() });
    }

    // Footer: APPLY / RESET DEFAULTS / BACK.
    let footer_cols = split_columns(shell.footer, &[1.2, 1.4, 1.0], 14.0);
    let footer = [
        (APPLY, "APPLY", true),
        (RESET, "RESET DEFAULTS", false),
        (BACK, "BACK", false),
    ];
    let footer_row = 1 + fields.len() as i16;
    for (index, (id, label, primary)) in footer.into_iter().enumerate() {
        let button = if primary {
            ArcadeButton::primary(label)
        } else {
            ArcadeButton::flat(label)
        };
        widgets.push(Placed {
            focused: focused == Some(id),
            ..Placed::new(id, footer_cols[index], Widget::Button(button))
        });
        entries.push(FocusEntry::new(
            id,
            footer_cols[index],
            footer_row,
            index as i16,
        ));
    }

    (
        widgets,
        entries,
        HintSet {
            navigate: true,
            adjust: true,
            confirm: Some("CHANGE"),
            cancel: Some("BACK"),
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.62,
            tint: None,
            animated: !theme.reduced_motion,
        },
        None,
    )
}
