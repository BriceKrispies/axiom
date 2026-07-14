//! Attract mode: the real deterministic showcase runs full-screen behind the
//! oversized END ZONE mark, a blinking PRESS START, and cycling original
//! feature phrases. Any confirm-style input returns to the title flow; the
//! inactivity clock lives entirely in the frontend.

use crate::frontend::actions::AudioIntent;
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::FocusEntry;
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    BackgroundView, HintSet, Label, LabelSize, Placed, TitleLogo, Widget,
};

use super::ScreenBuild;

/// Original feature phrases (cycled on a fixed tick period).
pub const PHRASES: [&str; 4] = [
    "ARCADE FOOTBALL",
    "BIG HITS",
    "NO HESITATION",
    "TAKE THE END ZONE",
];

/// Ticks each phrase holds.
pub const PHRASE_PERIOD: u64 = 240;

pub fn confirm(fe: &mut FrontendState) {
    fe.sound(AudioIntent::Confirm);
    fe.go(Screen::Title, TransitionKind::Fade);
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    theme: &Theme,
) -> ScreenBuild {
    let rows = centered_rows(shell.content, ctx.width.min(760.0), 150.0, 18.0, 2);
    let phrase = PHRASES[((fe.tick / PHRASE_PERIOD) as usize) % PHRASES.len()];
    let widgets = vec![
        Placed::new(
            crate::frontend::navigation::WidgetId(1),
            rows[0],
            Widget::Logo(TitleLogo {
                small: false,
                press_start: true,
            }),
        ),
        Placed::new(
            crate::frontend::navigation::WidgetId(2),
            rows[1],
            Widget::Label(Label {
                text: phrase.to_string(),
                size: LabelSize::Heading,
                accent: None,
                italic: true,
            }),
        ),
    ];
    (
        widgets,
        Vec::<FocusEntry>::new(),
        HintSet {
            navigate: false,
            adjust: false,
            confirm: Some("PRESS START"),
            cancel: None,
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.12,
            tint: None,
            animated: !theme.reduced_motion,
        },
        Some(phrase.to_string()),
    )
}
