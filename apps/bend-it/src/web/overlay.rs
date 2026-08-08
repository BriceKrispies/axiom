//! The screen-space interface, painted as SVG over the 3D view.
//!
//! A *painter*, and nothing more: every rectangle, point and label it draws was
//! resolved natively in [`crate::editor::view`], where it is tested. This file
//! chooses colours and nothing else.
//!
//! SVG rather than canvas or in-scene geometry for two reasons. It stays crisp
//! at any device pixel ratio, which matters when the whole interface is a thin
//! line and a word; and its `viewBox` is set to the **physical** surface size, so
//! the model can work in one coordinate space — the same one the camera and the
//! pointer capture use — and the browser does the ratio conversion exactly once.

use crate::editor::{ButtonView, EditorView, PanelView};

use super::mount_div;

/// The overlay's element id.
const OVERLAY_ID: &str = "bend-it-overlay";
/// `pointer-events: none`: hit testing happens in Rust against the same
/// rectangles this draws, so the DOM must never intercept a touch.
const OVERLAY_STYLE: &str = "position:fixed;inset:0;z-index:20;pointer-events:none;\
     user-select:none;-webkit-user-select:none;touch-action:none;";

const ACCENT: &str = "#ffd15c";
const TARGET: &str = "#ff6a3d";
const INK: &str = "#f2f7f3";

/// Repaint the overlay from this frame's view model.
pub fn paint(view: &EditorView) {
    let Some(root) = mount_div(OVERLAY_ID, OVERLAY_STYLE) else {
        return;
    };
    let (w, h) = (view.viewport.x, view.viewport.y);
    let body = [
        goal(view),
        view.panel.as_ref().map(|p| panel(p, view.short)).unwrap_or_default(),
        view.action.map(|b| button(b, view.short, true)).unwrap_or_default(),
        view.back.map(|b| button(b, view.short, false)).unwrap_or_default(),
        prompt(view),
        tally(view),
        banner(view),
    ]
    .concat();
    root.set_inner_html(&format!(
        "<svg width=\"100%\" height=\"100%\" viewBox=\"0 0 {w} {h}\" \
         preserveAspectRatio=\"none\" xmlns=\"http://www.w3.org/2000/svg\">{body}</svg>"
    ));
}

/// The goal's mouth and the point the shot finishes on.
fn goal(view: &EditorView) -> String {
    let outline = view
        .goal_quad
        .map(|quad| {
            let points = quad
                .iter()
                .map(|p| format!("{:.1},{:.1}", p.x, p.y))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "<polygon points=\"{points}\" fill=\"rgba(255,255,255,0.05)\" \
                 stroke=\"rgba(255,255,255,{:.2})\" stroke-width=\"{:.1}\" \
                 stroke-linejoin=\"round\"/>",
                0.30 * view.aim_emphasis,
                view.short * 0.006
            )
        })
        .unwrap_or_default();
    let reticle = view
        .target
        .map(|p| {
            let r = view.short * 0.030;
            let arm = r * 1.9;
            format!(
                "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"{r:.1}\" fill=\"none\" \
                 stroke=\"{TARGET}\" stroke-width=\"{sw:.1}\" opacity=\"{o:.2}\"/>\
                 <line x1=\"{x:.1}\" y1=\"{ly:.1}\" x2=\"{x:.1}\" y2=\"{hy:.1}\" \
                 stroke=\"{TARGET}\" stroke-width=\"{sw:.1}\" opacity=\"{o:.2}\"/>\
                 <line x1=\"{lx:.1}\" y1=\"{y:.1}\" x2=\"{hx:.1}\" y2=\"{y:.1}\" \
                 stroke=\"{TARGET}\" stroke-width=\"{sw:.1}\" opacity=\"{o:.2}\"/>",
                x = p.x,
                y = p.y,
                r = r,
                sw = view.short * 0.008,
                o = 0.35 + 0.65 * view.aim_emphasis,
                ly = p.y - arm,
                hy = p.y + arm,
                lx = p.x - arm,
                hx = p.x + arm,
            )
        })
        .unwrap_or_default();
    format!("{outline}{reticle}")
}

/// The sculpt panel: the card, the reference line, the curve, and the handle.
fn panel(panel: &PanelView, short: f32) -> String {
    let r = &panel.rect;
    let radius = short * 0.030;
    let line = short * 0.009;
    let card = format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{radius:.1}\" \
         fill=\"rgba(6,20,14,{fill:.2})\" stroke=\"rgba(255,255,255,{border:.2})\" \
         stroke-width=\"{:.1}\"/>",
        r.x,
        r.y,
        r.w,
        r.h,
        short * 0.004,
        fill = [0.44f32, 0.56][usize::from(panel.active)],
        border = [0.14f32, 0.34][usize::from(panel.active)],
    );
    let baseline = format!(
        "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" \
         stroke=\"rgba(255,255,255,0.22)\" stroke-width=\"{:.1}\" stroke-dasharray=\"{d:.1} {d:.1}\"/>",
        panel.baseline[0].x,
        panel.baseline[0].y,
        panel.baseline[1].x,
        panel.baseline[1].y,
        short * 0.004,
        d = short * 0.018,
    );
    let points = panel
        .curve
        .iter()
        .map(|p| format!("{:.1},{:.1}", p.x, p.y))
        .collect::<Vec<_>>()
        .join(" ");
    // A wide, faint stroke under a thin bright one: the line reads as thin and
    // sharp while still being obvious against a busy pitch.
    let curve = format!(
        "<polyline points=\"{points}\" fill=\"none\" stroke=\"{ACCENT}\" \
         stroke-opacity=\"0.20\" stroke-width=\"{:.1}\" stroke-linecap=\"round\"/>\
         <polyline points=\"{points}\" fill=\"none\" stroke=\"{ACCENT}\" \
         stroke-width=\"{line:.1}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
        line * 4.2
    );
    let ends = format!(
        "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{INK}\"/>\
         <circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"none\" stroke=\"{TARGET}\" \
         stroke-width=\"{:.1}\"/>",
        panel.ball_end.x,
        panel.ball_end.y,
        short * 0.014,
        panel.goal_end.x,
        panel.goal_end.y,
        short * 0.020,
        short * 0.007,
    );
    let handle = panel
        .handle
        .map(|p| {
            format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{ACCENT}\" \
                 fill-opacity=\"0.18\"/>\
                 <circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{ACCENT}\"/>",
                p.x,
                p.y,
                short * 0.075,
                p.x,
                p.y,
                short * 0.030,
            )
        })
        .unwrap_or_default();
    // The label sits on the card, and the two directions are named at the edges
    // the drag moves toward.
    let label = text(
        r.x + short * 0.035,
        r.y + short * 0.070,
        panel.label,
        short * 0.052,
        ACCENT,
        "start",
        0.95,
    );
    let (low, high) = match panel.projection {
        crate::play::Projection::Horizontal => (
            text(r.x + short * 0.035, r.y + r.h - short * 0.030, panel.hint_low, short * 0.030, INK, "start", 0.42),
            text(r.x + r.w - short * 0.035, r.y + r.h - short * 0.030, panel.hint_high, short * 0.030, INK, "end", 0.42),
        ),
        crate::play::Projection::Vertical => (
            text(r.x + r.w - short * 0.035, r.y + r.h - short * 0.030, panel.hint_low, short * 0.030, INK, "end", 0.42),
            text(r.x + r.w - short * 0.035, r.y + short * 0.070, panel.hint_high, short * 0.030, INK, "end", 0.42),
        ),
    };
    format!("{card}{baseline}{curve}{ends}{handle}{label}{low}{high}")
}

/// One action button.
fn button(button: ButtonView, short: f32, primary: bool) -> String {
    let r = button.rect;
    let fill = match (primary, button.pressed) {
        (true, false) => "rgba(255,209,92,0.90)",
        (true, true) => "rgba(255,232,160,1.0)",
        (false, false) => "rgba(10,26,20,0.72)",
        (false, true) => "rgba(30,54,44,0.90)",
    };
    let ink = ["rgba(242,247,243,0.80)", "#0a1410"][usize::from(primary)];
    format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" \
         fill=\"{fill}\" stroke=\"rgba(255,255,255,0.16)\" stroke-width=\"{:.1}\"/>{}",
        r.x,
        r.y,
        r.w,
        r.h,
        short * 0.028,
        short * 0.004,
        text(
            r.x + r.w * 0.5,
            r.y + r.h * 0.5 + short * 0.018,
            button.label,
            short * 0.052,
            ink,
            "middle",
            1.0,
        )
    )
}

/// The one-word stage prompt.
///
/// While a sculpt panel is up the panel already carries the word, so this only
/// speaks during the aim — and it speaks from the empty band between the goal
/// and the action row, never across the goal the player is trying to touch (or
/// across the keeper standing in it).
fn prompt(view: &EditorView) -> String {
    view.panel
        .as_ref()
        .map(|_| String::new())
        .unwrap_or_else(|| {
            text(
                view.viewport.x * 0.5,
                view.viewport.y * 0.66,
                view.prompt,
                view.short * 0.076,
                ACCENT,
                "middle",
                0.90,
            )
        })
}

/// Goals and attempts.
fn tally(view: &EditorView) -> String {
    text(
        view.viewport.x * 0.5,
        view.short * 0.085,
        &format!("{} / {}", view.tally.0, view.tally.1),
        view.short * 0.040,
        INK,
        "middle",
        0.62,
    )
}

/// The result banner.
fn banner(view: &EditorView) -> String {
    view.banner
        .map(|word| {
            text(
                view.viewport.x * 0.5,
                view.viewport.y * 0.42,
                word,
                view.short * 0.130,
                INK,
                "middle",
                0.96,
            )
        })
        .unwrap_or_default()
}

/// One run of text in the game's single register: monospaced, letter-spaced,
/// short.
fn text(
    x: f32,
    y: f32,
    body: &str,
    size: f32,
    fill: &str,
    anchor: &str,
    opacity: f32,
) -> String {
    match body.is_empty() {
        true => String::new(),
        false => format!(
            "<text x=\"{x:.1}\" y=\"{y:.1}\" font-family=\"ui-monospace,Menlo,Consolas,monospace\" \
             font-size=\"{size:.1}\" font-weight=\"800\" letter-spacing=\"{ls:.1}\" \
             fill=\"{fill}\" fill-opacity=\"{opacity:.2}\" text-anchor=\"{anchor}\" \
             style=\"paint-order:stroke;stroke:rgba(0,0,0,0.55);stroke-width:{stroke:.1}px\">{body}</text>",
            ls = size * 0.14,
            stroke = size * 0.14,
        ),
    }
}
