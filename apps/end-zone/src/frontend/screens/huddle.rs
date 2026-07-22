//! The pre-snap huddle: between every down the player calls a play. Every
//! offensive play is shown at once as a grid of clickable chalkboard cards
//! (each drawing that play's positions + routes); clicking one — or focusing it
//! and confirming — calls it and returns to the field. The drive composes it
//! against a hidden defensive answer.

use crate::data::DiagramRole;
use crate::data::{offensive_playbook, PlayDiagram};
use crate::field::OffensePoint;
use crate::frontend::actions::{AudioIntent, FrontendCommand};
use crate::frontend::layout::{rect, split_bands, LayoutContext, ShellRegions};
use crate::frontend::navigation::{FocusEntry, WidgetId};
use crate::frontend::state::{FrontendState, Screen};
use crate::frontend::theme::Theme;
use crate::frontend::transitions::TransitionKind;
use crate::frontend::widgets::{
    BackgroundView, DiagramGlyph, DiagramMarkView, HintSet, Label, LabelSize, Placed,
    PlayDiagramView, Widget,
};

use axiom_interface::UiRect;

use super::ScreenBuild;

const DOWN_DISTANCE: WidgetId = WidgetId(30);

/// Offense-relative lateral span mapped across a card (yards, full width).
const LATERAL_SPAN: f32 = 44.0;
/// Offense-relative downfield range shown, backfield → deep (yards).
const DOWNFIELD_MIN: f32 = -8.0;
const DOWNFIELD_MAX: f32 = 22.0;

pub fn confirm(fe: &mut FrontendState, id: WidgetId) {
    let count = offensive_playbook().len();
    if (id.0 as usize) < count {
        fe.command(FrontendCommand::CallPlay {
            index: id.0 as usize,
        });
        fe.huddle = None;
        fe.sound(AudioIntent::Confirm);
        fe.go(Screen::InGame, TransitionKind::Wipe);
    }
}

/// Project an offense-relative point to a card's normalized `0..1` box.
fn project(p: OffensePoint) -> (f32, f32) {
    let x = (0.5 + p.lateral / LATERAL_SPAN).clamp(0.05, 0.95);
    let t = ((p.downfield - DOWNFIELD_MIN) / (DOWNFIELD_MAX - DOWNFIELD_MIN)).clamp(0.0, 1.0);
    (x, 0.9 - t * 0.8)
}

fn glyph_for(role: DiagramRole) -> DiagramGlyph {
    match role {
        DiagramRole::Quarterback => DiagramGlyph::Quarterback,
        DiagramRole::Receiver | DiagramRole::Carrier => DiagramGlyph::Receiver,
        DiagramRole::Snapper | DiagramRole::Blocker => DiagramGlyph::Blocker,
    }
}

/// The render-ready diagram view for one offensive play card.
fn diagram_view(index: usize) -> PlayDiagramView {
    let diagram = PlayDiagram::of(&offensive_playbook()[index]);
    let marks = diagram
        .marks
        .iter()
        .map(|m| {
            let (x, y) = project(m.align);
            DiagramMarkView {
                x,
                y,
                glyph: glyph_for(m.role),
                primary: m.primary,
                decoy: m.decoy,
                route: m.route.iter().map(|p| project(*p)).collect(),
            }
        })
        .collect();
    PlayDiagramView {
        name: diagram.name.to_string(),
        marks,
        los_y: project(OffensePoint::new(0.0, 0.0)).1,
    }
}

fn ordinal(down: u8) -> &'static str {
    match down {
        1 => "1ST",
        2 => "2ND",
        3 => "3RD",
        _ => "4TH",
    }
}

fn down_distance(fe: &FrontendState) -> String {
    match fe.huddle {
        Some(h) if h.goal_to_go => format!("{} & GOAL", ordinal(h.down)),
        Some(h) => format!("{} & {}", ordinal(h.down), h.yards_to_go.round() as i32),
        None => "CALL A PLAY".to_string(),
    }
}

/// Row-major grid cells inside `region`, `cols` wide, one per play.
fn grid_cells(region: UiRect, cols: usize, count: usize, gap: f32) -> Vec<UiRect> {
    let rows = count.div_ceil(cols);
    let cw = (region.w.get() - gap * cols.saturating_sub(1) as f32) / cols as f32;
    let ch = (region.h.get() - gap * rows.saturating_sub(1) as f32) / rows as f32;
    (0..count)
        .map(|i| {
            let (r, c) = (i / cols, i % cols);
            rect(
                region.x.get() + c as f32 * (cw + gap),
                region.y.get() + r as f32 * (ch + gap),
                cw,
                ch,
            )
        })
        .collect()
}

pub fn build(
    fe: &FrontendState,
    ctx: &LayoutContext,
    shell: &ShellRegions,
    _theme: &Theme,
) -> ScreenBuild {
    let focused = fe.focus.focused();
    let playbook = offensive_playbook();

    let mut widgets = vec![Placed::new(
        WidgetId(10),
        shell.header,
        Widget::Label(Label {
            italic: true,
            accent: Some("#39c0ff".to_string()),
            ..Label::new("HUDDLE", LabelSize::Huge)
        }),
    )];

    // A thin down/distance strip, then the grid of clickable play cards.
    let bands = split_bands(shell.content, &[0.1, 0.9], 8.0);
    widgets.push(Placed::new(
        DOWN_DISTANCE,
        ctx.bounded(bands[0], 360.0),
        Widget::Label(Label::new(&down_distance(fe), LabelSize::Heading)),
    ));

    let cols = if ctx.portrait { 2 } else { playbook.len().min(4).max(2) };
    let grid = ctx.bounded(bands[1], 1040.0);
    let cells = grid_cells(grid, cols, playbook.len(), 16.0);
    let mut entries = Vec::new();
    for (i, _play) in playbook.iter().enumerate() {
        let id = WidgetId(i as u32);
        let cell = cells[i];
        widgets.push(
            Placed::new(id, cell, Widget::Diagram(diagram_view(i))).focused(focused == Some(id)),
        );
        entries.push(FocusEntry::new(id, cell, (i / cols) as i16, (i % cols) as i16));
    }

    (
        widgets,
        entries,
        HintSet {
            navigate: true,
            adjust: false,
            confirm: Some("CALL PLAY"),
            cancel: None,
            pause: None,
        },
        BackgroundView {
            show_field: true,
            dim: 0.62,
        },
    )
}
