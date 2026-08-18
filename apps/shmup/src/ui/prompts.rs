//! Interaction prompt (keycap + verb, optional hold-progress rule) and the
//! kill-confirmation / objective banner.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/prompts.js:1-94`.

use super::util::{clamp01, damp, ease};

#[derive(Debug, Clone)]
pub struct PromptSpec {
    pub key: String,
    pub text: String,
    pub sub: String,
    /// `p.progress` — `None` hides the progress bar entirely.
    pub progress: Option<f64>,
}

impl Default for PromptSpec {
    fn default() -> Self {
        PromptSpec { key: "F".to_string(), text: "INTERACT".to_string(), sub: String::new(), progress: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PromptFrame {
    pub visible: bool,
    pub opacity: f64,
    pub translate_y_px: f64,
    pub bar_visible: bool,
    pub fill_scale: f64,
}

/// `prompts.js:4-54`'s `Prompt` class, minus its DOM handles.
pub struct Prompt {
    shown: f64,
    active: bool,
    progress: f64,
    bar_shown: bool,
}

impl Default for Prompt {
    fn default() -> Self {
        Prompt { shown: 0.0, active: false, progress: 0.0, bar_shown: false }
    }
}

impl Prompt {
    pub fn new() -> Self {
        Prompt::default()
    }

    pub fn set(&mut self, p: &PromptSpec) {
        self.active = true;
        self.progress = p.progress.unwrap_or(0.0);
        self.bar_shown = p.progress.is_some();
    }

    pub fn clear(&mut self) {
        self.active = false;
    }

    pub fn update(&mut self, dt: f64) -> PromptFrame {
        self.shown = damp(self.shown, if self.active { 1.0 } else { 0.0 }, 18.0, dt);
        let vis = self.shown;
        let visible = vis >= 0.005;
        let y = (1.0 - ease::out_cubic(vis)) * 7.0;
        PromptFrame {
            visible,
            opacity: vis,
            translate_y_px: y,
            bar_visible: self.bar_shown,
            fill_scale: clamp01(self.progress),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BannerFrame {
    pub visible: bool,
    pub opacity: f64,
    pub scale: f64,
}

/// `prompts.js:57-94`'s `Banner` class, minus its DOM handles.
pub struct Banner {
    t: f64,
    life: f64,
}

impl Default for Banner {
    fn default() -> Self {
        Banner { t: 1.0, life: 2.1 }
    }
}

impl Banner {
    pub fn new() -> Self {
        Banner::default()
    }

    pub fn show(&mut self, life: f64) {
        self.life = life;
        self.t = 0.0;
    }

    pub fn update(&mut self, dt: f64) -> BannerFrame {
        if self.t >= 1.0 {
            return BannerFrame { visible: false, opacity: 0.0, scale: 1.0 };
        }
        self.t = (self.t + dt / self.life).min(1.0);
        let u = self.t;
        let in_t = clamp01(u / (0.16 / self.life));
        let a = if u > 0.78 { 1.0 - ease::in_quad((u - 0.78) / 0.22) } else { ease::out_quad(in_t) };
        let s = 0.965 + 0.035 * ease::out_back(in_t);
        BannerFrame { visible: true, opacity: a, scale: s }
    }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::{BannerFrame, PromptFrame, PromptSpec};

    pub struct PromptView {
        root: Element,
        key: Element,
        txt: Element,
        sub: Element,
        fill: Element,
        bar: Element,
    }

    impl PromptView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-prompt"), Some(parent));
            let key = dom::el("div", Some("ow-key"), Some(&root));
            dom::set_text(&key, "F");
            let col = dom::el("div", None, Some(&root));
            let txt = dom::el("div", Some("ow-prompt-txt"), Some(&col));
            dom::set_text(&txt, "INTERACT");
            let sub = dom::el("div", Some("ow-prompt-sub"), Some(&col));
            let bar = dom::el("div", None, Some(&col));
            dom::set_css_text(
                &bar,
                "margin-top:calc(var(--u)*1);height:calc(1.5px * var(--k));background:rgba(255,255,255,.16);width:100%",
            );
            let fill = dom::el("i", None, Some(&bar));
            dom::set_css_text(
                &fill,
                "display:block;height:100%;width:100%;background:var(--amber);transform-origin:left;transform:scaleX(0)",
            );
            dom::set_display(&root, "none");
            PromptView { root, key, txt, sub, fill, bar }
        }

        pub fn set(&self, p: &PromptSpec) {
            dom::set_text(&self.key, &p.key);
            dom::set_text(&self.txt, &p.text.to_uppercase());
            dom::set_text(&self.sub, &p.sub.to_uppercase());
            dom::set_display(&self.sub, if p.sub.is_empty() { "none" } else { "" });
            dom::set_display(&self.bar, if p.progress.is_some() { "" } else { "none" });
        }

        pub fn apply(&self, frame: &PromptFrame) {
            dom::set_display(&self.root, if frame.visible { "" } else { "none" });
            if !frame.visible {
                return;
            }
            dom::set_style(&self.root, "opacity", &format!("{:.3}", frame.opacity));
            dom::set_style(
                &self.root,
                "transform",
                &format!("translate(-50%,calc(-50% + {:.2}px))", frame.translate_y_px),
            );
            dom::set_style(&self.fill, "transform", &format!("scaleX({:.3})", frame.fill_scale));
        }

        pub fn dispose(&self) {
            dom::remove(&self.root);
        }
    }

    pub struct BannerView {
        root: Element,
        title: Element,
        sub: Element,
    }

    impl BannerView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-banner"), Some(parent));
            let title = dom::el("div", Some("ow-banner-t"), Some(&root));
            let sub = dom::el("div", Some("ow-banner-s"), Some(&root));
            dom::el("div", Some("ow-banner-rule"), Some(&root));
            dom::set_display(&root, "none");
            BannerView { root, title, sub }
        }

        pub fn show(&self, title: &str, sub: &str) {
            dom::set_text(&self.title, &title.to_uppercase());
            dom::set_text(&self.sub, &sub.to_uppercase());
            dom::set_display(&self.sub, if sub.is_empty() { "none" } else { "" });
        }

        pub fn apply(&self, frame: &BannerFrame) {
            if !frame.visible {
                dom::set_display(&self.root, "none");
                return;
            }
            dom::set_display(&self.root, "");
            dom::set_style(&self.root, "opacity", &format!("{:.3}", frame.opacity));
            dom::set_style(&self.root, "transform", &format!("translate(-50%,-50%) scale({:.4})", frame.scale));
        }

        pub fn dispose(&self) {
            dom::remove(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_settle_reaches_fully_shown() {
        let mut p = Prompt::new();
        p.set(&PromptSpec::default());
        for _ in 0..300 {
            p.update(1.0 / 60.0);
        }
        let frame = p.update(1.0 / 60.0);
        assert!(frame.visible);
        assert!((frame.opacity - 1.0).abs() < 1e-6);
        assert!(frame.translate_y_px.abs() < 1e-6);
    }

    #[test]
    fn clear_then_settle_hides_the_prompt() {
        let mut p = Prompt::new();
        p.set(&PromptSpec::default());
        for _ in 0..60 {
            p.update(1.0 / 60.0);
        }
        p.clear();
        for _ in 0..300 {
            p.update(1.0 / 60.0);
        }
        let frame = p.update(1.0 / 60.0);
        assert!(!frame.visible);
    }

    #[test]
    fn progress_bar_only_shows_when_spec_carries_progress() {
        let mut p = Prompt::new();
        p.set(&PromptSpec { progress: Some(0.5), ..PromptSpec::default() });
        let frame = p.update(1.0 / 60.0);
        assert!(frame.bar_visible);
        assert_eq!(frame.fill_scale, 0.5);

        let mut p2 = Prompt::new();
        p2.set(&PromptSpec::default());
        let frame2 = p2.update(1.0 / 60.0);
        assert!(!frame2.bar_visible);
    }

    #[test]
    fn banner_starts_hidden_and_shows_on_demand() {
        let mut b = Banner::new();
        let frame = b.update(1.0 / 60.0);
        assert!(!frame.visible);

        b.show(2.1);
        let frame = b.update(1.0 / 60.0);
        assert!(frame.visible);
        assert!(frame.opacity < 1.0, "banner punches in, not instant");
    }

    #[test]
    fn banner_holds_near_full_opacity_mid_life_then_fades() {
        let mut b = Banner::new();
        b.show(2.1);
        for _ in 0..(60 * 1) {
            b.update(1.0 / 60.0);
        }
        let mid = b.update(1.0 / 60.0);
        assert!((mid.opacity - 1.0).abs() < 1e-3);

        // Drive to the very end of the life; final update lands exactly at
        // t==1.0, and the *next* call is what the source's `if (t>=1)` gate
        // catches and hides.
        for _ in 0..300 {
            b.update(1.0 / 60.0);
        }
        let done = b.update(1.0 / 60.0);
        assert!(!done.visible);
    }
}
