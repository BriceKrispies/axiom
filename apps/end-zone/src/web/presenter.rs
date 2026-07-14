//! The DOM menu presenter: a dumb renderer of the frontend's typed
//! [`SceneView`] — positioned wrappers around the widget markup, background
//! treatment layers, the modal, hints, and the transition overlay. It never
//! decides anything; the pure frontend already did.

use axiom_interface::UiColor;
use web_sys::Element;

use crate::frontend::actions::AudioIntent;
use crate::frontend::transitions::{TransitionKind, TransitionView};
use crate::frontend::widgets::{Placed, SceneView, Widget};
use crate::frontend::FrontendFrame;

use super::markup;
use super::mount_div;
use super::style::MENU_CSS;

fn hex(color: UiColor) -> String {
    format!("#{:06x}", color.rgba() >> 8)
}

/// How many frames the squash-and-snap press animation persists.
const PRESS_FRAMES: u8 = 10;

/// The mounted presenter.
#[derive(Debug)]
pub struct MenuPresenter {
    root: Option<Element>,
    last_html: String,
    last_attrs: (String, String),
    press_frames: u8,
}

impl MenuPresenter {
    /// Inject the stylesheet and mount the menu root.
    pub fn mount() -> Self {
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            if document.get_element_by_id("end-zone-style").is_none() {
                if let (Ok(tag), Some(head)) = (document.create_element("style"), document.head()) {
                    tag.set_id("end-zone-style");
                    tag.set_text_content(Some(MENU_CSS));
                    let _ = head.append_child(&tag);
                }
            }
        }
        let root = mount_div("end-zone-menu", "", None);
        MenuPresenter {
            root,
            last_html: String::new(),
            last_attrs: (String::new(), String::new()),
            press_frames: 0,
        }
    }

    /// Render one frontend frame (skips DOM writes when nothing changed).
    pub fn render(&mut self, view: &FrontendFrame, css_width: f32, css_height: f32) {
        let Some(root) = self.root.as_ref() else {
            return;
        };
        let theme = &view.theme;
        let scale = theme.ui_scale.max(0.1);
        let (lw, lh) = (css_width / scale, css_height / scale);

        let pressed = view.sounds.iter().any(|s| {
            matches!(
                s,
                AudioIntent::Confirm
                    | AudioIntent::TeamLock
                    | AudioIntent::VsImpact
                    | AudioIntent::PauseHit
            )
        });
        if pressed {
            self.press_frames = PRESS_FRAMES;
        } else {
            self.press_frames = self.press_frames.saturating_sub(1);
        }

        let style = format!(
            "width:{lw:.0}px;height:{lh:.0}px;transform:scale({scale});\
             --ez-ts:{ts};--ez-text:{};--ez-textdim:{};--ez-steel-l:{};--ez-steel-d:{};\
             --ez-chrome:{};--ez-electric:{};--ez-hot:{};--ez-volt:{};--ez-focus:{};",
            hex(theme.text),
            hex(theme.text_dim),
            hex(theme.steel_light),
            hex(theme.steel_dark),
            hex(theme.chrome),
            hex(theme.electric),
            hex(theme.hot),
            hex(theme.volt),
            hex(theme.focus_ring),
            ts = theme.text_scale,
        );
        let class = format!(
            "{}{}{}",
            if theme.reduced_motion {
                "ez-reduced "
            } else {
                "ez-anim "
            },
            if theme.high_contrast { "ez-hc " } else { "" },
            ""
        );
        if self.last_attrs != (style.clone(), class.clone()) {
            let _ = root.set_attribute("style", &style);
            let _ = root.set_attribute("class", &class);
            self.last_attrs = (style, class);
        }

        let html = scene_html(
            &view.scene,
            theme.enhanced_distinction,
            self.press_frames > 0,
            lw,
            lh,
        );
        if html != self.last_html {
            root.set_inner_html(&html);
            self.last_html = html;
        }
    }
}

fn widget_html(placed: &Placed, enhanced: bool, press: bool) -> String {
    let inner = match &placed.widget {
        Widget::Panel(panel) => markup::panel_html(panel),
        Widget::Button(button) => markup::button_html(button, placed.focused),
        Widget::TeamCard(card) => markup::card_html(card, placed.focused, enhanced),
        Widget::SettingRow(row) => markup::setting_row_html(row, placed.focused),
        Widget::Selector(selector) => markup::selector_html(selector, placed.focused),
        Widget::Tabs(tabs) => markup::tabs_html(tabs),
        Widget::Label(label) => markup::label_html(label),
        Widget::Emblem(view) => super::emblem::emblem_svg(view),
        Widget::Ratings(bars) => markup::ratings_html(bars),
        Widget::Logo(logo) => markup::logo_html(logo),
    };
    let press_class = if placed.focused && press {
        "ez-pressanim"
    } else {
        ""
    };
    let disabled = if placed.enabled { "" } else { "ez-disabled" };
    format!(
        "<div class='ez-widget {press_class} {disabled}' \
         style='left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px'>{inner}</div>",
        placed.rect.x.get(),
        placed.rect.y.get(),
        placed.rect.w.get(),
        placed.rect.h.get()
    )
}

fn background_html(scene: &SceneView) -> String {
    let bg = &scene.background;
    let mut out = String::new();
    out.push_str(&format!(
        "<div class='ez-dim' style='background:rgba(5,8,13,{:.3})'></div>",
        bg.dim.clamp(0.0, 1.0)
    ));
    if let Some((left, right)) = &bg.tint {
        out.push_str(&format!(
            "<div class='ez-tint' style='background:linear-gradient(90deg,{left}55,transparent 32%,\
             transparent 68%,{right}55)'></div>"
        ));
    }
    out.push_str("<div class='ez-scan'></div><div class='ez-vig'></div>");
    if bg.animated {
        out.push_str("<div class='ez-sweepbar'></div>");
    }
    out
}

fn transition_html(transition: &TransitionView) -> String {
    let p = transition.progress.clamp(0.0, 1.0);
    // Cover peaks mid-transition (the screen swap hides beneath it).
    let veil = 1.0 - (2.0 * p - 1.0).abs();
    match transition.kind {
        TransitionKind::Fade => format!(
            "<div class='ez-tr' style='background:#05070b;opacity:{veil:.3}'></div>"
        ),
        TransitionKind::Wipe => format!(
            "<div class='ez-tr'><div class='ez-wipeslab' style='left:{:.1}%'></div></div>",
            -70.0 + p * 170.0
        ),
        TransitionKind::AngledSlide => format!(
            "<div class='ez-tr'><div class='ez-wipeslab' style='right:{:.1}%;width:130%;\
             opacity:{:.3}'></div></div>",
            -130.0 + veil * 115.0,
            (veil * 1.4).min(1.0)
        ),
        TransitionKind::ScaleImpact => format!(
            "<div class='ez-tr' style='background:radial-gradient(circle,rgba(255,255,255,{:.3}) 0%,\
             rgba(5,8,13,{:.3}) 78%)'></div>",
            veil * 0.85,
            veil
        ),
    }
}

fn modal_html(scene: &SceneView, lw: f32, lh: f32) -> String {
    let Some(modal) = &scene.modal else {
        return String::new();
    };
    // Geometry mirrors the frontend's modal focus rects exactly, so pointer
    // hit-testing and the visual always agree.
    let dw = (lw * 0.62).min(560.0);
    let dx = (lw - dw) / 2.0;
    let by = lh * 0.42;
    let half = dw / 2.0 - 12.0;
    let mut out = format!(
        "<div class='ez-modalveil'></div>\
         <div class='ez-modal' style='left:{dx:.0}px;top:{:.0}px;width:{dw:.0}px;transform:none'>\
         <h3>{}</h3><p>{}</p></div>",
        by - 118.0,
        markup::esc(&modal.title),
        markup::esc(&modal.body)
    );
    for (index, option) in modal.options.iter().enumerate() {
        let style_class = if option.danger { "ez-danger" } else { "" };
        let focus = if option.focused { "ez-focused" } else { "" };
        out.push_str(&format!(
            "<div class='ez-widget' style='left:{:.0}px;top:{by:.0}px;width:{half:.0}px;height:56px'>\
             <div class='ez-btn ez-angled {style_class} {focus}'>{}</div></div>",
            dx + index as f32 * (half + 24.0),
            markup::esc(&option.label)
        ));
    }
    out
}

fn scene_html(scene: &SceneView, enhanced: bool, press: bool, lw: f32, lh: f32) -> String {
    let mut out = background_html(scene);
    for placed in &scene.widgets {
        out.push_str(&widget_html(placed, enhanced, press));
    }
    out.push_str(&markup::hints_html(&scene.hints));
    out.push_str(&modal_html(scene, lw, lh));
    if let Some(transition) = &scene.transition {
        out.push_str(&transition_html(transition));
    }
    out
}
