//! The credits screen: the END ZONE mark, the engine/procedural statement,
//! and a fictional-league copyright over a slow (reduced-motion-aware)
//! field background. Any confirm or cancel returns to the main menu.

use crate::frontend::actions::AudioIntent;
use crate::frontend::layout::{centered_rows, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    ArcadeButton, BackgroundView, HintSet, Label, LabelSize, Placed, TitleLogo, Widget,
};

use super::ScreenBuild;

const BACK: WidgetId = WidgetId(1);

const LINES: [(&str, LabelSize); 5] = [
    ("AN ORIGINAL ARCADE FOOTBALL SHOWCASE", LabelSize::Heading),
    ("BUILT ON THE AXIOM ENGINE", LabelSize::Body),
    (
        "EVERY TEAM, PLAYER, EMBLEM AND SOUND IS FICTIONAL AND PROCEDURAL",
        LabelSize::Body,
    ),
    (
        "NO IMAGE ASSETS \u{2014} ALL VISUALS GENERATED AT RUNTIME",
        LabelSize::Body,
    ),
    (
        "\u{00a9} 2026 THE END ZONE LEAGUE \u{2014} A FICTIONAL LEAGUE",
        LabelSize::Small,
    ),
];

pub fn confirm(fe: &mut FrontendState, _id: WidgetId) {
    cancel(fe);
}

pub fn cancel(fe: &mut FrontendState) {
    fe.sound(AudioIntent::Cancel);
    fe.go(Screen::MainMenu, TransitionKind::AngledSlide);
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    theme: &Theme,
) -> ScreenBuild {
    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Logo(TitleLogo {
            small: true,
            press_start: false,
        }),
    )];

    let width = (ctx.width * 0.8).min(760.0);
    let rows = centered_rows(shell.content, width, 44.0, 14.0, LINES.len());
    for (index, ((text, size), rect)) in LINES.into_iter().zip(rows).enumerate() {
        widgets.push(Placed::new(
            WidgetId(20 + index as u32),
            rect,
            Widget::Label(Label {
                text: text.to_string(),
                size,
                accent: (index == 0).then(|| "#ffd23c".to_string()),
                italic: index == 0,
            }),
        ));
    }

    let back_rect = crate::frontend::layout::rect(
        shell.footer.x.get() + (shell.footer.w.get() - 200.0) / 2.0,
        shell.footer.y.get(),
        200.0,
        shell.footer.h.get().min(52.0),
    );
    widgets.push(Placed {
        focused: fe.focus.focused() == Some(BACK),
        ..Placed::new(BACK, back_rect, Widget::Button(ArcadeButton::flat("BACK")))
    });
    let entries = vec![FocusEntry::new(BACK, back_rect, 0, 0)];

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
            dim: 0.5,
            tint: None,
            animated: !theme.reduced_motion,
        },
        None,
    )
}
