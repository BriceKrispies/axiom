//! The DOM presenter: a dumb renderer of the frontend's typed [`SceneView`]
//! (positioned widget markup, background layers, hints, the transition overlay)
//! plus the gameplay HUD built from authoritative run state. It never decides
//! anything; the pure frontend and the run already did.

use axiom_interface::UiColor;
use web_sys::Element;

use crate::frontend::actions::AudioIntent;
use crate::frontend::transitions::{TransitionKind, TransitionView};
use crate::frontend::widgets::{Placed, SceneView, Widget};
use crate::frontend::FrontendFrame;
use crate::presentation::HudView;

use super::markup;
use super::mount_div;
use super::style::MENU_CSS;

fn hex(color: UiColor) -> String {
    format!("#{:06x}", color.rgba() >> 8)
}

/// How many frames the squash-and-snap press animation persists.
const PRESS_FRAMES: u8 = 10;

/// The mounted presenter (menu root + HUD root).
#[derive(Debug)]
pub struct MenuPresenter {
    root: Option<Element>,
    hud: Option<Element>,
    last_html: String,
    last_hud: String,
    last_style: String,
    press_frames: u8,
}

impl MenuPresenter {
    /// Inject the stylesheet and mount the menu + HUD roots.
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
        let hud = mount_div("end-zone-hud", "", None);
        let root = mount_div("end-zone-menu", "", None);
        MenuPresenter {
            root,
            hud,
            last_html: String::new(),
            last_hud: String::new(),
            last_style: String::new(),
            press_frames: 0,
        }
    }

    /// Render one frontend frame (skips DOM writes when nothing changed).
    pub fn render(&mut self, view: &FrontendFrame, css_width: f32, css_height: f32) {
        let Some(root) = self.root.as_ref() else {
            return;
        };
        let theme = &view.theme;
        let (lw, lh) = (css_width, css_height);

        let pressed = view
            .sounds
            .iter()
            .any(|s| matches!(s, AudioIntent::Confirm | AudioIntent::PauseHit));
        if pressed {
            self.press_frames = PRESS_FRAMES;
        } else {
            self.press_frames = self.press_frames.saturating_sub(1);
        }

        let style = format!(
            "width:{lw:.0}px;height:{lh:.0}px;\
             --ez-text:{};--ez-textdim:{};--ez-steel-l:{};--ez-steel-d:{};\
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
        );
        let class = if theme.reduced_motion {
            "ez-reduced"
        } else {
            "ez-anim"
        };
        let combined = format!("{style}|{class}");
        if self.last_style != combined {
            let _ = root.set_attribute("style", &style);
            let _ = root.set_attribute("class", class);
            if let Some(hud) = self.hud.as_ref() {
                let _ = hud.set_attribute("style", &style);
                let _ = hud.set_attribute("class", class);
            }
            self.last_style = combined;
        }

        let html = scene_html(&view.scene, self.press_frames > 0, lw, lh);
        if html != self.last_html {
            root.set_inner_html(&html);
            self.last_html = html;
        }
    }

    /// Render the gameplay HUD from authoritative run state (empty when there
    /// is nothing to show, e.g. on the title or a menu).
    pub fn render_hud(&mut self, hud: Option<HudView>) {
        let Some(root) = self.hud.as_ref() else {
            return;
        };
        let html = hud.map(hud_html).unwrap_or_default();
        if html != self.last_hud {
            root.set_inner_html(&html);
            self.last_hud = html;
        }
    }
}

fn hud_html(hud: HudView) -> String {
    format!(
        "<div class='ez-hud'>\
         <div class='ez-hud-score'>{}</div>\
         <div class='ez-hud-center'>\
           <div class='ez-hud-down'>{}</div>\
           <div class='ez-hud-togain'>{}</div>\
         </div>\
         <div class='ez-hud-heat'>{}</div>\
         </div>",
        markup::esc(&hud.score),
        markup::esc(&hud.down_distance),
        markup::esc(&hud.to_gain),
        markup::esc(&hud.heat),
    )
}

fn widget_html(placed: &Placed, press: bool) -> String {
    let inner = match &placed.widget {
        Widget::Panel(panel) => markup::panel_html(panel),
        Widget::Button(button) => markup::button_html(button, placed.focused),
        Widget::Label(label) => markup::label_html(label),
        Widget::Logo(logo) => markup::logo_html(logo),
        Widget::Setting(row) => markup::setting_row_html(row, placed.focused),
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
    let mut out = format!(
        "<div class='ez-dim' style='background:rgba(5,8,13,{:.3})'></div>",
        bg.dim.clamp(0.0, 1.0)
    );
    out.push_str("<div class='ez-vig'></div>");
    out
}

fn transition_html(transition: &TransitionView) -> String {
    let p = transition.progress.clamp(0.0, 1.0);
    let veil = 1.0 - (2.0 * p - 1.0).abs();
    match transition.kind {
        TransitionKind::Fade => {
            format!("<div class='ez-tr' style='background:#05070b;opacity:{veil:.3}'></div>")
        }
        TransitionKind::Wipe => format!(
            "<div class='ez-tr'><div class='ez-wipeslab' style='left:{:.1}%'></div></div>",
            -70.0 + p * 170.0
        ),
        TransitionKind::ScaleImpact => format!(
            "<div class='ez-tr' style='background:radial-gradient(circle,rgba(255,255,255,{:.3}) 0%,\
             rgba(5,8,13,{:.3}) 78%)'></div>",
            veil * 0.85,
            veil
        ),
    }
}

fn scene_html(scene: &SceneView, press: bool, _lw: f32, _lh: f32) -> String {
    let mut out = background_html(scene);
    for placed in &scene.widgets {
        out.push_str(&widget_html(placed, press));
    }
    out.push_str(&markup::hints_html(&scene.hints));
    if let Some(transition) = &scene.transition {
        out.push_str(&transition_html(transition));
    }
    out
}
