//! The main menu: exactly START GAME / SETTINGS / CREDITS on angled arcade
//! plates over the live (subordinate) field presentation.

use crate::frontend::actions::AudioIntent;
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen, TeamStage};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{ArcadeButton, BackgroundView, HintSet, Placed, TitleLogo, Widget};

use super::settings;
use super::ScreenBuild;

const START_GAME: WidgetId = WidgetId(1);
const SETTINGS: WidgetId = WidgetId(2);
const CREDITS: WidgetId = WidgetId(3);

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        START_GAME => {
            fe.sound(AudioIntent::Confirm);
            fe.team_select.stage = TeamStage::Player;
            fe.team_select.locked_player = None;
            fe.go(Screen::TeamSelect, TransitionKind::AngledSlide);
        }
        SETTINGS => {
            fe.sound(AudioIntent::Confirm);
            settings::open(fe, Screen::MainMenu);
        }
        CREDITS => {
            fe.sound(AudioIntent::Confirm);
            fe.go(Screen::Credits, TransitionKind::AngledSlide);
        }
        _ => {}
    }
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    theme: &Theme,
) -> ScreenBuild {
    let logo_rect = crate::frontend::layout::rect(
        shell.header.x.get(),
        shell.header.y.get(),
        shell.header.w.get(),
        shell.header.h.get(),
    );
    let width = (ctx.width * 0.5).clamp(260.0, 460.0);
    let rows = centered_rows(shell.content, width, 72.0, 22.0, 3);
    let focused = fe.focus.focused();
    let items = [
        (START_GAME, "START GAME"),
        (SETTINGS, "SETTINGS"),
        (CREDITS, "CREDITS"),
    ];
    let mut widgets = vec![Placed::new(
        WidgetId(10),
        logo_rect,
        Widget::Logo(TitleLogo {
            small: true,
            press_start: false,
        }),
    )];
    let mut entries = Vec::new();
    for (index, (id, label)) in items.into_iter().enumerate() {
        let rect = rows[index];
        widgets.push(Placed {
            focused: focused == Some(id),
            ..Placed::new(id, rect, Widget::Button(ArcadeButton::primary(label)))
        });
        entries.push(FocusEntry::new(id, rect, index as i16, 0));
    }
    (
        widgets,
        entries,
        HintSet::menu(),
        BackgroundView {
            show_field: true,
            dim: 0.45,
            tint: None,
            animated: !theme.reduced_motion,
        },
        None,
    )
}
