//! Two-stage team selection. Stage 1 picks the PLAYER team on a big center
//! card (emblem, city/name, abbreviation, colors, rating bars, mini lineup)
//! with previous/next preview cards. Stage 2 locks the player card and picks
//! the OPPONENT from the remaining teams — never the same team twice, never
//! a hidden random pick. Both selections are remembered through persistence.

use crate::data::team::{league, LeagueTeamId, LEAGUE_SIZE};
use crate::frontend::actions::AudioIntent;
use crate::frontend::layout::{split_columns, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen, TeamStage};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, Side, TeamCard, Widget,
};

use super::ScreenBuild;

const PREV: WidgetId = WidgetId(1);
const CONFIRM: WidgetId = WidgetId(2);
const NEXT: WidgetId = WidgetId(3);
const BACK: WidgetId = WidgetId(4);

fn cursor(fe: &FrontendState) -> u8 {
    match fe.team_select.stage {
        TeamStage::Player => fe.team_select.player_cursor,
        TeamStage::Opponent => fe.team_select.opponent_cursor,
    }
}

/// Step a cursor around the league, skipping the locked player team while
/// picking the opponent.
fn step_cursor(fe: &FrontendState, from: u8, dx: i32) -> u8 {
    let blocked = fe
        .team_select
        .locked_player
        .filter(|_| fe.team_select.stage == TeamStage::Opponent);
    let mut value = from;
    for _ in 0..LEAGUE_SIZE {
        value = ((i32::from(value) + dx).rem_euclid(LEAGUE_SIZE as i32)) as u8;
        if blocked != Some(LeagueTeamId(value)) {
            return value;
        }
    }
    from
}

/// Left/right carousel movement (claims horizontal navigation).
pub fn adjust(fe: &mut FrontendState, dx: i32) -> bool {
    let next = step_cursor(fe, cursor(fe), dx);
    match fe.team_select.stage {
        TeamStage::Player => fe.team_select.player_cursor = next,
        TeamStage::Opponent => fe.team_select.opponent_cursor = next,
    }
    true
}

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        PREV => {
            adjust(fe, -1);
            fe.sound(AudioIntent::Navigate);
        }
        NEXT => {
            adjust(fe, 1);
            fe.sound(AudioIntent::Navigate);
        }
        CONFIRM => lock(fe),
        BACK => cancel(fe),
        _ => {}
    }
}

fn lock(fe: &mut FrontendState) {
    match fe.team_select.stage {
        TeamStage::Player => {
            let player = LeagueTeamId(fe.team_select.player_cursor);
            fe.team_select.locked_player = Some(player);
            fe.team_select.stage = TeamStage::Opponent;
            if fe.team_select.opponent_cursor == player.0 {
                fe.team_select.opponent_cursor = step_cursor(fe, player.0, 1);
            }
            fe.sound(AudioIntent::TeamLock);
            fe.haptic(crate::frontend::actions::HapticIntent::Confirm);
        }
        TeamStage::Opponent => {
            let Some(player) = fe.team_select.locked_player else {
                return;
            };
            let opponent = LeagueTeamId(fe.team_select.opponent_cursor);
            if opponent == player {
                fe.sound(AudioIntent::Denied);
                return;
            }
            fe.profile.last_player_team = player;
            fe.profile.last_opponent_team = opponent;
            fe.persist_requested = true;
            fe.match_options.difficulty = fe.profile.settings.difficulty;
            fe.match_options.game_speed = fe.profile.settings.game_speed;
            fe.roll_seed();
            fe.sound(AudioIntent::TeamLock);
            fe.go(Screen::MatchSetup, TransitionKind::AngledSlide);
        }
    }
}

pub fn cancel(fe: &mut FrontendState) {
    fe.sound(AudioIntent::Cancel);
    match fe.team_select.stage {
        TeamStage::Player => fe.go(Screen::MainMenu, TransitionKind::AngledSlide),
        TeamStage::Opponent => {
            // Unlock the player team, back to stage 1.
            fe.team_select.stage = TeamStage::Player;
            fe.team_select.locked_player = None;
        }
    }
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    theme: &Theme,
) -> ScreenBuild {
    let teams = league();
    let stage = fe.team_select.stage;
    let selected = usize::from(cursor(fe));
    let previous = usize::from(step_cursor(fe, cursor(fe) as u8, -1));
    let next = usize::from(step_cursor(fe, cursor(fe) as u8, 1));
    let focused = fe.focus.focused();

    let heading = match stage {
        TeamStage::Player => "SELECT YOUR TEAM",
        TeamStage::Opponent => "SELECT OPPONENT",
    };
    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Label(Label {
            text: heading.to_string(),
            size: LabelSize::Huge,
            accent: None,
            italic: true,
        }),
    )];
    let mut entries = Vec::new();

    // Layout: locked player rail (stage 2) | prev | CENTER | next.
    let content = ctx.bounded(shell.content, 1180.0);
    let columns = if ctx.portrait {
        split_columns(content, &[1.0, 2.6, 1.0], 8.0)
    } else if stage == TeamStage::Opponent {
        split_columns(content, &[1.2, 0.9, 2.4, 0.9], 14.0)
    } else {
        split_columns(content, &[0.9, 2.4, 0.9], 14.0)
    };
    let (locked_rect, prev_rect, main_rect, next_rect) =
        if stage == TeamStage::Opponent && !ctx.portrait {
            (Some(columns[0]), columns[1], columns[2], columns[3])
        } else {
            (None, columns[0], columns[1], columns[2])
        };

    // The locked player card stays visible on the opponent stage.
    if let (Some(rect), Some(player)) = (locked_rect, fe.team_select.locked_player) {
        let team = &teams[usize::from(player.0) % LEAGUE_SIZE];
        let mut card = TeamCard::of(team);
        card.locked = true;
        card.side = Some(Side::Home);
        card.compact = true;
        widgets.push(Placed::new(WidgetId(20), rect, Widget::TeamCard(card)));
    }

    // Preview cards (clickable) + the big center card.
    let mut prev_card = TeamCard::of(&teams[previous]);
    prev_card.compact = true;
    prev_card.preview = true;
    let mut next_card = TeamCard::of(&teams[next]);
    next_card.compact = true;
    next_card.preview = true;
    let mut main_card = TeamCard::of(&teams[selected]);
    main_card.lineup = true;
    main_card.side = Some(match stage {
        TeamStage::Player => Side::Home,
        TeamStage::Opponent => Side::Away,
    });

    widgets.push(Placed {
        focused: focused == Some(PREV),
        ..Placed::new(PREV, prev_rect, Widget::TeamCard(prev_card))
    });
    widgets.push(Placed {
        focused: focused == Some(CONFIRM),
        ..Placed::new(CONFIRM, main_rect, Widget::TeamCard(main_card))
    });
    widgets.push(Placed {
        focused: focused == Some(NEXT),
        ..Placed::new(NEXT, next_rect, Widget::TeamCard(next_card))
    });
    // Declared CONFIRM-first: a fresh screen focuses the big center card.
    entries.push(FocusEntry::new(CONFIRM, main_rect, 0, 1));
    entries.push(FocusEntry::new(PREV, prev_rect, 0, 0));
    entries.push(FocusEntry::new(NEXT, next_rect, 0, 2));

    // Footer back plate.
    let back_rect = crate::frontend::layout::rect(
        shell.footer.x.get(),
        shell.footer.y.get(),
        150.0,
        shell.footer.h.get().min(52.0),
    );
    widgets.push(Placed {
        focused: focused == Some(BACK),
        ..Placed::new(BACK, back_rect, Widget::Button(ArcadeButton::flat("BACK")))
    });
    entries.push(FocusEntry::new(BACK, back_rect, 1, 1));

    let accent = crate::frontend::theme::css_color(teams[selected].palette.primary());
    (
        widgets,
        entries,
        HintSet {
            navigate: true,
            adjust: true,
            confirm: Some(match stage {
                TeamStage::Player => "LOCK TEAM",
                TeamStage::Opponent => "LOCK OPPONENT",
            }),
            cancel: Some("BACK"),
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.55,
            tint: Some((accent.clone(), accent)),
            animated: !theme.reduced_motion,
        },
        None,
    )
}
