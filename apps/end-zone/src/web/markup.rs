//! Widget → HTML markup: each typed view-model widget becomes one small
//! procedural DOM fragment (classes + inline geometry only; all styling
//! lives in the injected stylesheet).

use crate::frontend::widgets::{
    ArcadeButton, ArcadePanel, ButtonStyle, CategoryTabs, Hint, Label, LabelSize, RatingBars,
    RowControl, SettingRow, TeamCard, TitleLogo, ValueSelector,
};

use super::emblem::emblem_svg;

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
    let tint = button
        .tint
        .as_ref()
        .map(|t| format!("style='background:linear-gradient(180deg,{t},#0d1118)'"))
        .unwrap_or_default();
    format!(
        "<div class='ez-btn {style_class} {angled} {focus}' {tint}>{}</div>",
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

pub fn ratings_html(bars: &RatingBars) -> String {
    let row = |name: &str, value: u8| -> String {
        let cells: String = (1..=10)
            .map(|i| {
                let on = if i <= value { "on" } else { "" };
                format!("<i class='ez-cell {on}'></i>")
            })
            .collect();
        format!("<div class='ez-bar'><b>{name}</b><span class='ez-barcells'>{cells}</span></div>")
    };
    format!(
        "<div class='ez-bars'>{}{}{}{}</div>",
        row("POWER", bars.power),
        row("SPEED", bars.speed),
        row("PASS", bars.pass),
        row("DEFENSE", bars.defense)
    )
}

pub fn card_html(card: &TeamCard, focused: bool, enhanced: bool) -> String {
    let focus = if focused { "ez-focused" } else { "" };
    let compact = if card.compact { "ez-compact" } else { "" };
    let preview = if card.preview { "ez-preview" } else { "" };
    let vars = format!(
        "--card-primary:{};--card-secondary:{};--card-accent:{};",
        card.primary, card.secondary, card.accent
    );
    let side = card
        .side
        .filter(|_| card.side.is_some())
        .map(|s| format!("<div class='ez-sidechip'>{}</div>", s.label()))
        .unwrap_or_default();
    let lock = if card.locked {
        "<div class='ez-lockchip'>LOCKED</div>"
    } else {
        ""
    };
    let bars = if card.compact {
        String::new()
    } else {
        ratings_html(&card.ratings)
    };
    let lineup = if card.lineup {
        let chips: String = (0..7)
            .map(|_| "<i class='ez-jersey'></i>".to_string())
            .collect();
        format!("<div class='ez-lineup'>{chips}</div>")
    } else {
        String::new()
    };
    // Under enhanced color distinction the abbreviation is emphasized and the
    // card carries a non-color pattern edge.
    let distinct = if enhanced { "ez-distinct" } else { "" };
    format!(
        "<div class='ez-card ez-widgetfill {focus} {compact} {preview} {distinct}' style='{vars}'>\
         <div class='ez-cardtop'></div>{side}{lock}\
         <div class='ez-emblem'>{}</div>\
         <div class='ez-city'>{}</div><div class='ez-name'>{}</div>\
         <div class='ez-abbr'>{}</div>{bars}{lineup}</div>",
        emblem_svg(&card.emblem),
        esc(&card.city),
        esc(&card.name),
        esc(&card.abbreviation)
    )
}

pub fn tabs_html(tabs: &CategoryTabs) -> String {
    let chips: String = tabs
        .labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let on = if i == tabs.active { "on" } else { "" };
            format!("<div class='ez-tab {on}'>{}</div>", esc(label))
        })
        .collect();
    format!("<div class='ez-tabs'>{chips}</div>")
}

fn row_control_html(control: &RowControl, value: &str) -> String {
    match control {
        RowControl::Selector { has_prev, has_next } => format!(
            "<span class='ez-arrow {}'>\u{25c0}</span><span>{}</span>\
             <span class='ez-arrow {}'>\u{25b6}</span>",
            if *has_prev { "" } else { "off" },
            esc(value),
            if *has_next { "" } else { "off" }
        ),
        RowControl::Toggle { on } => format!(
            "<div class='ez-toggle {}'><i></i></div>",
            if *on { "on" } else { "" }
        ),
        RowControl::Volume { value, max } => {
            let cells: String = (1..=*max)
                .map(|i| format!("<i class='{}'></i>", if i <= *value { "on" } else { "" }))
                .collect();
            format!("<div class='ez-vol'>{cells}</div>")
        }
        RowControl::Binding {
            tokens,
            capturing,
            conflict,
        } => {
            if *capturing {
                "<span class='ez-capture'>PRESS A KEY\u{2026}</span>".to_string()
            } else {
                let warn = if *conflict { "warn" } else { "" };
                let chips: String = tokens
                    .iter()
                    .map(|t| format!("<b class='ez-chip {warn}'>{}</b>", esc(t)))
                    .collect();
                format!("<div class='ez-bindchips'>{chips}</div>")
            }
        }
        RowControl::Action => "<span class='ez-arrow'>\u{25b6}</span>".to_string(),
        RowControl::ReadOnly => format!("<span>{}</span>", esc(value)),
    }
}

pub fn setting_row_html(row: &SettingRow, focused: bool) -> String {
    let focus = if focused { "ez-focused" } else { "" };
    let detail = row
        .detail
        .as_ref()
        .map(|d| format!("<div class='ez-rowdetail'>{}</div>", esc(d)))
        .unwrap_or_default();
    format!(
        "<div class='ez-row {focus}'><div class='ez-rowlabel'>{}</div>{detail}\
         <div class='ez-rowvalue'>{}</div></div>",
        esc(&row.label),
        row_control_html(&row.control, &row.value)
    )
}

pub fn selector_html(selector: &ValueSelector, focused: bool) -> String {
    let focus = if focused { "ez-focused" } else { "" };
    format!(
        "<div class='ez-selector ez-widgetfill {focus}'>\
         <div class='ez-sellabel'>{}</div>\
         <div class='ez-selvalue'><span class='ez-arrow {}'>\u{25c0}</span><span>{}</span>\
         <span class='ez-arrow {}'>\u{25b6}</span></div></div>",
        esc(&selector.label),
        if selector.has_prev { "" } else { "off" },
        esc(&selector.value),
        if selector.has_next { "" } else { "off" }
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
