//! Widget → HTML markup: each typed view-model widget becomes one small
//! procedural DOM fragment (classes + inline geometry only; all styling lives
//! in the injected stylesheet).

use crate::frontend::widgets::{
    ArcadeButton, ArcadePanel, ButtonStyle, Hint, Label, LabelSize, SettingRow, TitleLogo,
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
