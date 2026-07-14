//! The matchup screen: both team cards around a central VS mark, rating
//! summaries, quick difficulty / game-speed selectors, the deterministic
//! seed (diagnostic), and START MATCH / BACK. Confirming freezes the launch
//! configuration and runs the transition-to-game sequence — the simulation
//! is initialized exactly once, by the composition layer, from the frozen
//! config.

use crate::data::team::{league, LeagueTeamId, LEAGUE_SIZE};
use crate::frontend::actions::{AudioIntent, FrontendCommand};
use crate::frontend::layout::{rect, split_columns, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, Side, TeamCard, ValueSelector,
    Widget,
};
use crate::launch::{Difficulty, GameSpeed};

use super::ScreenBuild;

const DIFFICULTY: WidgetId = WidgetId(1);
const SPEED: WidgetId = WidgetId(2);
const START: WidgetId = WidgetId(3);
const BACK: WidgetId = WidgetId(4);

fn difficulty_label(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Rookie => "ROOKIE",
        Difficulty::Pro => "PRO",
        Difficulty::AllStar => "ALL-STAR",
    }
}

fn speed_label(s: GameSpeed) -> &'static str {
    match s {
        GameSpeed::Normal => "NORMAL",
        GameSpeed::Fast => "FAST",
        GameSpeed::Turbo => "TURBO",
    }
}

fn step_difficulty(d: Difficulty, dx: i32) -> Difficulty {
    const ALL: [Difficulty; 3] = [Difficulty::Rookie, Difficulty::Pro, Difficulty::AllStar];
    let index = ALL.iter().position(|v| *v == d).unwrap_or(1) as i32;
    ALL[(index + dx).clamp(0, 2) as usize]
}

fn step_speed(s: GameSpeed, dx: i32) -> GameSpeed {
    const ALL: [GameSpeed; 3] = [GameSpeed::Normal, GameSpeed::Fast, GameSpeed::Turbo];
    let index = ALL.iter().position(|v| *v == s).unwrap_or(0) as i32;
    ALL[(index + dx).clamp(0, 2) as usize]
}

/// Left/right adjusts the focused selector.
pub fn adjust(fe: &mut FrontendState, dx: i32) -> bool {
    match fe.focus.focused() {
        Some(DIFFICULTY) => {
            fe.match_options.difficulty = step_difficulty(fe.match_options.difficulty, dx);
            true
        }
        Some(SPEED) => {
            fe.match_options.game_speed = step_speed(fe.match_options.game_speed, dx);
            true
        }
        _ => false,
    }
}

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    match id {
        DIFFICULTY => {
            fe.match_options.difficulty = step_difficulty(fe.match_options.difficulty, 1);
            fe.sound(AudioIntent::Navigate);
        }
        SPEED => {
            fe.match_options.game_speed = step_speed(fe.match_options.game_speed, 1);
            fe.sound(AudioIntent::Navigate);
        }
        START => match fe.build_launch() {
            Some(config) => {
                fe.launch = Some(config);
                fe.command(FrontendCommand::LaunchMatch(config));
                fe.sound(AudioIntent::VsImpact);
                fe.haptic(crate::frontend::actions::HapticIntent::Impact);
                fe.go(Screen::TransitionToGame, TransitionKind::ScaleImpact);
            }
            None => fe.sound(AudioIntent::Denied),
        },
        BACK => cancel(fe),
        _ => {}
    }
}

pub fn cancel(fe: &mut FrontendState) {
    fe.sound(AudioIntent::Cancel);
    fe.go(Screen::TeamSelect, TransitionKind::AngledSlide);
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    theme: &Theme,
) -> ScreenBuild {
    let teams = league();
    let player = fe
        .team_select
        .locked_player
        .unwrap_or(LeagueTeamId(fe.team_select.player_cursor));
    let opponent = LeagueTeamId(fe.team_select.opponent_cursor);
    let home = &teams[usize::from(player.0) % LEAGUE_SIZE];
    let away = &teams[usize::from(opponent.0) % LEAGUE_SIZE];
    let focused = fe.focus.focused();

    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Label(Label {
            text: "MATCHUP".to_string(),
            size: LabelSize::Huge,
            accent: None,
            italic: true,
        }),
    )];
    let mut entries = Vec::new();

    // Cards around the VS mark.
    let content = ctx.bounded(shell.content, 1200.0);
    let card_band = rect(
        content.x.get(),
        content.y.get(),
        content.w.get(),
        content.h.get() * 0.62,
    );
    let columns = split_columns(card_band, &[1.0, 0.34, 1.0], 10.0);
    let mut home_card = TeamCard::of(home);
    home_card.locked = true;
    home_card.side = Some(Side::Home);
    let mut away_card = TeamCard::of(away);
    away_card.locked = true;
    away_card.side = Some(Side::Away);
    widgets.push(Placed::new(
        WidgetId(11),
        columns[0],
        Widget::TeamCard(home_card),
    ));
    widgets.push(Placed::new(
        WidgetId(12),
        columns[1],
        Widget::Label(Label {
            text: "VS".to_string(),
            size: LabelSize::Huge,
            accent: Some("#ffd23c".to_string()),
            italic: true,
        }),
    ));
    widgets.push(Placed::new(
        WidgetId(13),
        columns[2],
        Widget::TeamCard(away_card),
    ));

    // Options band: difficulty / speed selectors + seed (diagnostic).
    let options_y = card_band.y.get() + card_band.h.get() + 12.0;
    let options_h = content.h.get() - card_band.h.get() - 12.0;
    let options = rect(
        content.x.get(),
        options_y,
        content.w.get(),
        options_h.max(96.0),
    );
    let option_cols = split_columns(options, &[1.0, 1.0, 1.0], 16.0);

    widgets.push(Placed {
        focused: focused == Some(DIFFICULTY),
        ..Placed::new(
            DIFFICULTY,
            option_cols[0],
            Widget::Selector(ValueSelector::new(
                "DIFFICULTY",
                difficulty_label(fe.match_options.difficulty),
                fe.match_options.difficulty != Difficulty::Rookie,
                fe.match_options.difficulty != Difficulty::AllStar,
            )),
        )
    });
    widgets.push(Placed {
        focused: focused == Some(SPEED),
        ..Placed::new(
            SPEED,
            option_cols[1],
            Widget::Selector(ValueSelector::new(
                "GAME SPEED",
                speed_label(fe.match_options.game_speed),
                fe.match_options.game_speed != GameSpeed::Normal,
                fe.match_options.game_speed != GameSpeed::Turbo,
            )),
        )
    });
    widgets.push(Placed::new(
        WidgetId(14),
        option_cols[2],
        Widget::Label(Label {
            text: format!("SEED {:#018x}", fe.pending_seed),
            size: LabelSize::Small,
            accent: None,
            italic: false,
        }),
    ));
    // (Focus entries for the selectors are declared after START below, so a
    // fresh screen focuses START MATCH first.)

    // Footer: START MATCH / BACK.
    let footer_cols = split_columns(shell.footer, &[1.0, 1.6, 1.0], 16.0);
    widgets.push(Placed {
        focused: focused == Some(BACK),
        ..Placed::new(
            BACK,
            footer_cols[0],
            Widget::Button(ArcadeButton::flat("BACK")),
        )
    });
    widgets.push(Placed {
        focused: focused == Some(START),
        ..Placed::new(
            START,
            footer_cols[1],
            Widget::Button(ArcadeButton::primary("START MATCH")),
        )
    });
    entries.push(FocusEntry::new(START, footer_cols[1], 1, 1));
    entries.push(FocusEntry::new(BACK, footer_cols[0], 1, 0));
    entries.push(FocusEntry::new(DIFFICULTY, option_cols[0], 0, 0));
    entries.push(FocusEntry::new(SPEED, option_cols[1], 0, 1));

    let home_tint = crate::frontend::theme::css_color(home.palette.primary());
    let away_tint = crate::frontend::theme::css_color(away.palette.primary());
    (
        widgets,
        entries,
        HintSet {
            navigate: true,
            adjust: true,
            confirm: Some("SELECT"),
            cancel: Some("BACK"),
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.55,
            tint: Some((home_tint, away_tint)),
            animated: !theme.reduced_motion,
        },
        None,
    )
}
