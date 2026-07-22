//! Widget → HTML markup: each typed view-model widget becomes one small
//! procedural DOM fragment (classes + inline geometry only; all styling lives
//! in the injected stylesheet).

use crate::frontend::widgets::{
    ArcadeButton, ArcadePanel, ButtonStyle, DiagramGlyph, Hint, Label, LabelSize, PlayDiagramView,
    SettingRow, TitleLogo,
};

/// Minimal HTML escaping (labels are app-authored, but stay correct).
pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn button_html(button: &ArcadeButton, focused: bool) -> String {
    let style_class = match button.style {
        ButtonStyle::Primary => "ez-primary",
        ButtonStyle::Danger => "ez-danger",
        ButtonStyle::Flat => "",
    };
    let angled = if button.angled { "ez-angled" } else { "" };
    let focus = if focused { "ez-focused" } else { "" };
    format!(
        "<div class='ez-btn {style_class} {angled} {focus}'>{}</div>",
        esc(&button.label)
    )
}

pub fn label_html(label: &Label) -> String {
    let size = match label.size {
        LabelSize::Huge => "ez-huge",
        LabelSize::Heading => "ez-heading",
        LabelSize::Body => "ez-body",
        LabelSize::Small => "ez-smalltext",
    };
    let italic = if label.italic { "ez-italic" } else { "" };
    let accent = label
        .accent
        .as_ref()
        .map(|c| format!("style='color:{c};text-shadow:0 0 14px {c},0 2px 4px #000'"))
        .unwrap_or_default();
    format!(
        "<div class='ez-label {size} {italic}' {accent}>{}</div>",
        esc(&label.text)
    )
}

pub fn logo_html(logo: &TitleLogo) -> String {
    let size = if logo.small { "ez-small" } else { "ez-big" };
    let press = if logo.press_start {
        "<div class='ez-press'>PRESS START</div>"
    } else {
        ""
    };
    format!(
        "<div class='ez-logo {size} ez-widgetfill'>\
         <div class='ez-mark'>END ZONE</div>\
         <div class='ez-sub'>ARCADE FOOTBALL</div>{press}</div>"
    )
}

pub fn panel_html(panel: &ArcadePanel) -> String {
    let title = panel
        .title
        .as_ref()
        .map(|t| {
            format!(
                "<div class='ez-label ez-heading ez-italic'>{}</div>",
                esc(t)
            )
        })
        .unwrap_or_default();
    format!("<div class='ez-plate ez-widgetfill'>{title}</div>")
}

pub fn setting_row_html(row: &SettingRow, focused: bool) -> String {
    let focus = if focused { "ez-focused" } else { "" };
    let value = match row.fill {
        Some(fill) => {
            let pct = (fill.clamp(0.0, 1.0) * 100.0).round();
            format!(
                "<div class='ez-vol'><i style='width:{pct:.0}%'></i></div>\
                 <span class='ez-rowval'>{}</span>",
                esc(&row.value)
            )
        }
        None => format!("<span class='ez-rowval'>{}</span>", esc(&row.value)),
    };
    format!(
        "<div class='ez-row {focus}'><div class='ez-rowlabel'>{}</div>\
         <div class='ez-rowvalue'>{value}</div></div>",
        esc(&row.label)
    )
}

/// The chalkboard play diagram as an SVG card: a board, the line of scrimmage,
/// each player's route polyline, and each player's glyph — from the normalized
/// view. `focused` draws the selectable card's highlight ring.
pub fn play_diagram_html(view: &PlayDiagramView, focused: bool) -> String {
    let los = view.los_y * 100.0;
    let mut body = format!(
        "<rect class='ez-board' x='0' y='0' width='100' height='100'/>\
         <line class='ez-los' x1='3' y1='{los:.1}' x2='97' y2='{los:.1}'/>"
    );
    for mark in &view.marks {
        if mark.route.len() >= 2 {
            let points: Vec<String> = mark
                .route
                .iter()
                .map(|(x, y)| format!("{:.1},{:.1}", x * 100.0, y * 100.0))
                .collect();
            let class = if mark.decoy {
                "ez-route ez-decoy"
            } else if mark.primary {
                "ez-route ez-primary"
            } else {
                "ez-route"
            };
            body.push_str(&format!(
                "<polyline class='{class}' points='{}'/>",
                points.join(" ")
            ));
        }
    }
    for mark in &view.marks {
        let (cx, cy) = (mark.x * 100.0, mark.y * 100.0);
        let class = if mark.primary { "ez-mark ez-primary" } else { "ez-mark" };
        match mark.glyph {
            DiagramGlyph::Receiver | DiagramGlyph::Quarterback => body.push_str(&format!(
                "<circle class='{class}' cx='{cx:.1}' cy='{cy:.1}' r='3.2'/>"
            )),
            DiagramGlyph::Blocker => body.push_str(&format!(
                "<rect class='{class}' x='{:.1}' y='{:.1}' width='5' height='5'/>",
                cx - 2.5,
                cy - 2.5
            )),
        }
        if matches!(mark.glyph, DiagramGlyph::Quarterback) {
            body.push_str(&format!(
                "<circle class='ez-qbdot' cx='{cx:.1}' cy='{cy:.1}' r='1.1'/>"
            ));
        }
    }
    let focus = if focused { "ez-focused" } else { "" };
    format!(
        "<div class='ez-diagram ez-widgetfill {focus}'>\
         <svg class='ez-chalk' viewBox='0 0 100 100' preserveAspectRatio='none'>{body}</svg>\
         <div class='ez-diagram-name'>{}</div>\
         </div>",
        esc(&view.name)
    )
}

pub fn hints_html(hints: &[Hint]) -> String {
    let chips: String = hints
        .iter()
        .map(|h| {
            format!(
                "<span class='ez-hint'><b>{}</b>{}</span>",
                esc(&h.control),
                esc(&h.action)
            )
        })
        .collect();
    format!("<div class='ez-hints'>{chips}</div>")
}
