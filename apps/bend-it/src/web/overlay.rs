//! The screen-space interface, painted as SVG over the 3D view.
//!
//! A *painter*, and nothing more: every point and label it draws was resolved
//! natively in [`crate::stroke::view`], where it is tested. This file chooses
//! colours.
//!
//! There is very little to choose. The interface is the line under your finger —
//! no panels, no buttons, no stages — so this draws a stroke, a score, a speed,
//! and at most one word.
//!
//! SVG rather than canvas for two reasons: it stays crisp at any device pixel
//! ratio, which matters when the whole interface is a thin line and a word; and
//! its `viewBox` is the **physical** surface size, so the model works in one
//! coordinate space — the same one the camera and the pointer capture use — and
//! the browser does the ratio conversion exactly once.

use crate::stroke::{GameView, StrokeView};

use super::mount_div;

/// The overlay's element id.
const OVERLAY_ID: &str = "bend-it-overlay";
/// `pointer-events: none`: the drawing is captured on the canvas beneath, so the
/// picture of it must never intercept a touch.
const OVERLAY_STYLE: &str = "position:fixed;inset:0;z-index:20;pointer-events:none;\
     user-select:none;-webkit-user-select:none;touch-action:none;";

const ACCENT: &str = "#ffd15c";
const INK: &str = "#f2f7f3";

/// Repaint the overlay from this frame's view model.
pub fn paint(view: &GameView) {
    let Some(root) = mount_div(OVERLAY_ID, OVERLAY_STYLE) else {
        return;
    };
    let (w, h) = (view.viewport.x, view.viewport.y);
    let body = [
        view.stroke
            .as_ref()
            .map(|s| stroke(s, view.short))
            .unwrap_or_default(),
        hint(view),
        tally(view),
        speed(view),
        banner(view),
    ]
    .concat();
    root.set_inner_html(&format!(
        "<svg width=\"100%\" height=\"100%\" viewBox=\"0 0 {w} {h}\" \
         preserveAspectRatio=\"none\" xmlns=\"http://www.w3.org/2000/svg\">{body}</svg>"
    ));
}

/// The drawn line.
///
/// A wide faint stroke under a thin bright one: it reads as sharp against a busy
/// pitch while still being unmissable. Below the length that counts as a shot it
/// is drawn dim — the only "keep going" feedback the gesture needs — and once
/// released it keeps its shape and thins away, so the eye follows the shot
/// rather than watching a line be deleted.
fn stroke(stroke: &StrokeView, short: f32) -> String {
    let points = stroke
        .points
        .iter()
        .map(|p| format!("{:.1},{:.1}", p.x, p.y))
        .collect::<Vec<_>>()
        .join(" ");
    let alpha = stroke.fade.clamp(0.0, 1.0) * [0.34f32, 1.0][usize::from(stroke.live)];
    let width = short * 0.011 * (0.45 + 0.55 * stroke.fade.clamp(0.0, 1.0));
    let head = stroke
        .points
        .last()
        .map(|p| {
            format!(
                "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{ACCENT}\" \
                 fill-opacity=\"{:.2}\"/>",
                p.x,
                p.y,
                short * 0.020 * stroke.fade.clamp(0.0, 1.0).max(0.35),
                alpha
            )
        })
        .unwrap_or_default();
    format!(
        "<polyline points=\"{points}\" fill=\"none\" stroke=\"{ACCENT}\" \
         stroke-opacity=\"{:.2}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\" \
         stroke-linejoin=\"round\"/>\
         <polyline points=\"{points}\" fill=\"none\" stroke=\"{ACCENT}\" \
         stroke-opacity=\"{:.2}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\" \
         stroke-linejoin=\"round\"/>{head}",
        alpha * 0.22,
        width * 4.0,
        alpha,
        width,
    )
}

/// The one-line instruction, low on the screen and out of the goal's way.
fn hint(view: &GameView) -> String {
    view.hint
        .map(|words| {
            text(
                view.viewport.x * 0.5,
                view.viewport.y * 0.90,
                words,
                view.short * 0.046,
                INK,
                0.50,
            )
        })
        .unwrap_or_default()
}

/// Goals and attempts.
fn tally(view: &GameView) -> String {
    text(
        view.viewport.x * 0.5,
        view.short * 0.085,
        &format!("{} / {}", view.tally.0, view.tally.1),
        view.short * 0.040,
        INK,
        0.62,
    )
}

/// How hard the ball was hit, under the score.
///
/// It is the one number the game shows, so it is worth being clear about what it
/// measures: the speed the ball genuinely **left the boot at**, taken off the
/// ball on the tick it was struck. Not an average over the flight, which would
/// read low, and not what the shot was authored at, which would be the game
/// marking its own homework.
///
/// Accented rather than plain, because it is the answer to a question the player
/// asked with the tempo of their line — and it holds through the flight and the
/// result, so there is time to look at it.
fn speed(view: &GameView) -> String {
    view.speed
        .map(|kmh| {
            text(
                view.viewport.x * 0.5,
                view.short * 0.150,
                &format!("{kmh} KM/H"),
                view.short * 0.045,
                ACCENT,
                0.88,
            )
        })
        .unwrap_or_default()
}

/// The result banner.
fn banner(view: &GameView) -> String {
    view.banner
        .map(|word| {
            text(
                view.viewport.x * 0.5,
                view.viewport.y * 0.42,
                word,
                view.short * 0.130,
                INK,
                0.96,
            )
        })
        .unwrap_or_default()
}

/// One run of centred text in the game's single register: monospaced,
/// letter-spaced, short.
fn text(x: f32, y: f32, body: &str, size: f32, fill: &str, opacity: f32) -> String {
    match body.is_empty() {
        true => String::new(),
        false => format!(
            "<text x=\"{x:.1}\" y=\"{y:.1}\" font-family=\"ui-monospace,Menlo,Consolas,monospace\" \
             font-size=\"{size:.1}\" font-weight=\"800\" letter-spacing=\"{ls:.1}\" \
             fill=\"{fill}\" fill-opacity=\"{opacity:.2}\" text-anchor=\"middle\" \
             style=\"paint-order:stroke;stroke:rgba(0,0,0,0.55);stroke-width:{stroke:.1}px\">{body}</text>",
            ls = size * 0.14,
            stroke = size * 0.14,
        ),
    }
}
