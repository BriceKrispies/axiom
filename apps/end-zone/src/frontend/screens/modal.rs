//! App-styled modal dialogs (never a browser alert): confined focus, a safe
//! option first, and explicit accept semantics per [`ModalKind`].

use crate::frontend::actions::{AudioIntent, FrontendAction, FrontendCommand};
use crate::frontend::layout::LayoutContext;
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, ModalKind, ModalState, Screen};
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{ModalOption, ModalView};

use super::settings;

/// Route one action into the open dialog (focus is confined here).
pub(super) fn handle(fe: &mut FrontendState, action: FrontendAction) {
    let Some(mut modal) = fe.modal else {
        return;
    };
    match action {
        FrontendAction::Navigate(_) => {
            modal.focused ^= 1;
            fe.modal = Some(modal);
            fe.sound(AudioIntent::Navigate);
        }
        FrontendAction::PointerMove { x, y } => {
            if let Some(id) = fe.focus.hover(x, y) {
                modal.focused = (id.0 % 2) as u8;
                fe.modal = Some(modal);
            }
        }
        FrontendAction::PointerActivate { x, y } => {
            if let Some(id) = fe.focus.activate_at(x, y) {
                modal.focused = (id.0 % 2) as u8;
                fe.modal = Some(modal);
                confirm(fe);
            }
        }
        FrontendAction::Confirm => confirm(fe),
        FrontendAction::Cancel | FrontendAction::Pause => {
            fe.modal = None;
            fe.sound(AudioIntent::Cancel);
        }
    }
}

fn confirm(fe: &mut FrontendState) {
    let Some(modal) = fe.modal.take() else {
        return;
    };
    let accepted = modal.focused == 1;
    match modal.kind {
        ModalKind::DiscardSettings => {
            if accepted {
                settings::discard(fe);
            } else {
                fe.sound(AudioIntent::Cancel);
            }
        }
        ModalKind::ReturnToMenu => {
            if accepted {
                fe.command(FrontendCommand::ReturnToMenu);
                fe.command(FrontendCommand::SetPaused(false));
                fe.launch = None;
                fe.sound(AudioIntent::Transition);
                fe.go(Screen::TransitionToMenu, TransitionKind::Wipe);
            } else {
                fe.sound(AudioIntent::Cancel);
            }
        }
    }
}

/// Build the dialog view + its confined focus entries.
pub(super) fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
) -> (Option<ModalView>, Vec<FocusEntry>) {
    let Some(ModalState { kind, focused }) = fe.modal else {
        return (None, Vec::new());
    };
    let (title, body, safe, accept, danger) = match kind {
        ModalKind::DiscardSettings => (
            "UNSAVED CHANGES",
            "Settings were changed but not applied.",
            "KEEP EDITING",
            "DISCARD CHANGES",
            true,
        ),
        ModalKind::ReturnToMenu => (
            "LEAVE MATCH?",
            "The current match will be lost.",
            "CANCEL",
            "RETURN TO MENU",
            true,
        ),
    };
    let options = vec![
        ModalOption {
            id: WidgetId(9000),
            label: safe.to_string(),
            focused: focused == 0,
            danger: false,
        },
        ModalOption {
            id: WidgetId(9001),
            label: accept.to_string(),
            focused: focused == 1,
            danger,
        },
    ];
    // Focus rects: two buttons centred in a dialog band.
    let w = (ctx.width * 0.62).min(560.0);
    let x = (ctx.width - w) / 2.0;
    let y = ctx.height * 0.42;
    let half = w / 2.0 - 12.0;
    let entries = vec![
        FocusEntry::new(
            WidgetId(9000),
            crate::frontend::layout::rect(x, y, half, 56.0),
            0,
            0,
        ),
        FocusEntry::new(
            WidgetId(9001),
            crate::frontend::layout::rect(x + half + 24.0, y, half, 56.0),
            0,
            1,
        ),
    ];
    (
        Some(ModalView {
            title: title.to_string(),
            body: body.to_string(),
            options,
        }),
        entries,
    )
}
