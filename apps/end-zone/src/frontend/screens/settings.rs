//! The settings screen: exactly MASTER VOLUME, MUSIC VOLUME, SCREEN SHAKE,
//! REDUCED MOTION, and BACK. Changes apply immediately (no working/committed
//! copy, no apply or discard). Each setting drives real behavior; see
//! `SETTINGS.md`.

use crate::frontend::actions::AudioIntent;
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::settings::Volume;
use crate::frontend::state::FrontendState;
use crate::frontend::theme::Theme;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, SettingRow, Widget,
};
use crate::launch::ScreenShake;

use super::ScreenBuild;

const VOLUME: WidgetId = WidgetId(1);
const SHAKE: WidgetId = WidgetId(2);
const MOTION: WidgetId = WidgetId(3);
const BACK: WidgetId = WidgetId(4);
const MUSIC: WidgetId = WidgetId(5);

/// Adjust the focused setting by a horizontal step (`dx` sign). Returns whether
/// a setting was adjusted (BACK ignores horizontal input).
pub fn adjust(fe: &mut FrontendState, dx: i32) -> bool {
    let up = dx > 0;
    match fe.focus.focused() {
        Some(VOLUME) => {
            fe.edit_settings(|s| s.master_volume = s.master_volume.step(up));
            true
        }
        Some(MUSIC) => {
            fe.edit_settings(|s| s.music_volume = s.music_volume.step(up));
            true
        }
        Some(SHAKE) => {
            fe.edit_settings(|s| s.screen_shake = cycle_shake(s.screen_shake, up));
            true
        }
        Some(MOTION) => {
            fe.edit_settings(|s| s.reduced_motion = !s.reduced_motion);
            true
        }
        _ => false,
    }
}

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    if id == BACK {
        super::pause::back_to_pause(fe);
    } else if adjust(fe, 1) {
        // Confirm on a setting advances it (same as a right press).
        fe.sound(AudioIntent::Navigate);
    }
}

fn cycle_shake(shake: ScreenShake, up: bool) -> ScreenShake {
    let order = [ScreenShake::Off, ScreenShake::Low, ScreenShake::Full];
    let index = order.iter().position(|s| *s == shake).unwrap_or(2) as i32;
    let next = (index + if up { 1 } else { -1 }).rem_euclid(order.len() as i32) as usize;
    order[next]
}

fn shake_label(shake: ScreenShake) -> &'static str {
    match shake {
        ScreenShake::Off => "OFF",
        ScreenShake::Low => "LOW",
        ScreenShake::Full => "FULL",
    }
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    _theme: &Theme,
) -> ScreenBuild {
    let focused = fe.focus.focused();
    let s = fe.effective_settings();
    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Label(Label {
            italic: true,
            ..Label::new("SETTINGS", LabelSize::Huge)
        }),
    )];
    let mut entries = Vec::new();

    let width = (ctx.width * 0.7).clamp(320.0, 560.0);
    let rows = centered_rows(shell.content, width, 54.0, 15.0, 5);

    let volume = SettingRow {
        label: "MASTER VOLUME".to_string(),
        value: format!("{}/{}", s.master_volume.0, Volume::MAX),
        fill: Some(s.master_volume.ratio()),
    };
    let music = SettingRow {
        label: "MUSIC VOLUME".to_string(),
        value: format!("{}/{}", s.music_volume.0, Volume::MAX),
        fill: Some(s.music_volume.ratio()),
    };
    let shake = SettingRow {
        label: "SCREEN SHAKE".to_string(),
        value: shake_label(s.screen_shake).to_string(),
        fill: None,
    };
    let motion = SettingRow {
        label: "REDUCED MOTION".to_string(),
        value: if s.reduced_motion { "ON" } else { "OFF" }.to_string(),
        fill: None,
    };

    for (index, (id, row)) in [(VOLUME, volume), (MUSIC, music), (SHAKE, shake), (MOTION, motion)]
        .into_iter()
        .enumerate()
    {
        let rect = rows[index];
        widgets.push(Placed::new(id, rect, Widget::Setting(row)).focused(focused == Some(id)));
        entries.push(FocusEntry::new(id, rect, index as i16, 0));
    }

    let back_rect = rows[4];
    widgets.push(
        Placed::new(BACK, back_rect, Widget::Button(ArcadeButton::flat("BACK")))
            .focused(focused == Some(BACK)),
    );
    entries.push(FocusEntry::new(BACK, back_rect, 4, 0));

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
            dim: 0.6,
        },
    )
}
