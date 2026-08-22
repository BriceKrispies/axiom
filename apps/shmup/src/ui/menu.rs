//! Pause / settings menu.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/menu.js:1-199`.
//!
//! Wired straight into `ctx.config`: the quality segments call
//! `config.setQuality`, the sliders write `config.sensitivity` and
//! `config.fov`, and every change is announced on the event bus so
//! render/player can react without importing this module.
//!
//! Events emitted: `ui:pause` `{paused}`, `ui:quality` `{quality}`,
//! `ui:sensitivity` `{value, multiplier}`, `ui:fov` `{value}`, `ui:setting`
//! `{key, value}`.
//!
//! ## What this port can and cannot wire up yet
//!
//! [`Config`] and [`EventBus`] both exist in this crate already, so quality
//! switching, the sensitivity/FOV/invert-look settings, and every emitted
//! event are wired for real, not stubbed. What the source *also* does —
//! push the live FOV into `ctx.camera`, freeze `ctx.time.scale`, disable
//! `ctx.peek('player')`'s controls, and toggle the browser's pointer lock
//! (`menu.js:158-181`) — needs subsystems (`camera`, `player`, `input`) that
//! have not landed in this port yet. Those four effects sit behind the
//! narrow [`MenuHost`] trait instead of a concrete dependency, so
//! [`PauseMenu::show`]/[`PauseMenu::close`] compile and are testable today,
//! and the real camera/player/input bindings implement the trait when they
//! land without this module changing.

use crate::config::{Config, Quality};
use crate::events::EventBus;

/// The four effects [`PauseMenu::show`]/[`PauseMenu::close`] reach for that
/// belong to subsystems not yet ported — see the module docs. Mirrors
/// `markers.js`'s [`super::markers::ScreenProjector`] pattern: a narrow
/// contract a future subsystem binding implements, rather than a concrete
/// dependency this app-tier module cannot yet have.
pub trait MenuHost {
    /// `ctx.time.scale = 0` / restore. Returns the *previous* scale so the
    /// caller can restore it on close, exactly as the source's
    /// `this._prevScale`.
    fn freeze_time(&mut self) -> f64;
    fn set_time_scale(&mut self, scale: f64);
    fn set_player_control_enabled(&mut self, enabled: bool);
    fn exit_pointer_lock(&mut self);
    fn request_pointer_lock(&mut self);
    fn set_camera_fov(&mut self, fov_degrees: f32);
}

/// `PRESETS` (`menu.js:3`) — reuses [`Quality::ALL`], which is already in
/// the same declared order (`low, medium, high, ultra`).
pub const PRESETS: [Quality; 4] = Quality::ALL;

pub const SENS_MIN: f64 = 0.2;
pub const SENS_MAX: f64 = 3.0;
pub const FOV_MIN: f64 = 65.0;
pub const FOV_MAX: f64 = 120.0;

/// `sensitivity` is stored in the config as radians/px; the slider works in
/// a `0.2..3.0` multiplier of the default `0.0022` (`menu.js:41-45`).
pub fn sensitivity_multiplier(sensitivity: f64) -> f64 {
    sensitivity / 0.0022
}

pub fn sensitivity_from_multiplier(multiplier: f64) -> f64 {
    0.0022 * multiplier
}

/// One slider's paint fraction, `0..1` — `menu.js:118-122`'s `t`.
pub fn slider_fraction(value: f64, min: f64, max: f64) -> f64 {
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuFrame {
    pub visible: bool,
    pub pointer_events_auto: bool,
    pub opacity: f64,
}

/// `menu.js:16-199`'s `PauseMenu` class, minus its DOM handles and its
/// direct `ctx` reach. `Config`/`EventBus` are still threaded through
/// exactly as the source's `this.ctx.config`/`this.ctx.events`; the
/// deferred subsystem effects go through [`MenuHost`].
pub struct PauseMenu {
    pub open: bool,
    shown: f64,
    prev_time_scale: f64,
}

impl Default for PauseMenu {
    fn default() -> Self {
        PauseMenu { open: false, shown: 0.0, prev_time_scale: 1.0 }
    }
}

impl PauseMenu {
    pub fn new() -> Self {
        PauseMenu::default()
    }

    pub fn set_quality(&self, cfg: &mut Config, quality: Quality, events: &EventBus) {
        cfg.set_quality(quality);
        events.emit("ui:quality", &quality.name());
    }

    pub fn set_sensitivity_multiplier(&self, cfg: &mut Config, multiplier: f64, events: &EventBus) {
        cfg.sensitivity = sensitivity_from_multiplier(multiplier);
        events.emit("ui:sensitivity", &(cfg.sensitivity, multiplier));
    }

    /// The setting is `f64` (the width `config.js` authors it in); the render
    /// camera behind [`MenuHost::set_camera_fov`] genuinely stores `f32`, so
    /// **that** call is where the narrowing belongs — not the config field.
    pub fn set_fov(&self, cfg: &mut Config, fov: f64, host: Option<&mut dyn MenuHost>, events: &EventBus) {
        cfg.fov = fov;
        if let Some(h) = host {
            h.set_camera_fov(fov as f32);
        }
        events.emit("ui:fov", &fov);
    }

    pub fn set_invert_y(&self, cfg: &mut Config, invert_y: bool, events: &EventBus) {
        cfg.invert_y = invert_y;
        events.emit("ui:setting", &("invertY", invert_y));
    }

    pub fn reset_to_defaults(&self, cfg: &mut Config, events: &EventBus) {
        self.set_sensitivity_multiplier(cfg, 1.0, events);
        self.set_fov(cfg, 80.0, None, events);
        self.set_invert_y(cfg, false, events);
        self.set_quality(cfg, Quality::Ultra, events);
    }

    pub fn toggle(&mut self, host: Option<&mut dyn MenuHost>, events: &EventBus) {
        if self.open {
            self.close(host, events);
        } else {
            self.show(host, events);
        }
    }

    pub fn show(&mut self, host: Option<&mut dyn MenuHost>, events: &EventBus) {
        if self.open {
            return;
        }
        self.open = true;
        if let Some(h) = host {
            h.exit_pointer_lock();
            self.prev_time_scale = h.freeze_time();
            h.set_player_control_enabled(false);
        }
        events.emit("ui:pause", &true);
    }

    pub fn close(&mut self, host: Option<&mut dyn MenuHost>, events: &EventBus) {
        if !self.open {
            return;
        }
        self.open = false;
        if let Some(h) = host {
            h.set_time_scale(self.prev_time_scale);
            h.set_player_control_enabled(true);
            h.request_pointer_lock();
        }
        events.emit("ui:pause", &false);
    }

    /// Driven with unscaled time so the fade still runs while the game is
    /// frozen (`menu.js:184`).
    pub fn update(&mut self, raw_dt: f64) -> MenuFrame {
        self.shown = super::util::damp(self.shown, if self.open { 1.0 } else { 0.0 }, 14.0, raw_dt);
        if self.shown < 0.004 {
            return MenuFrame { visible: false, pointer_events_auto: false, opacity: 0.0 };
        }
        MenuFrame { visible: true, pointer_events_auto: self.open, opacity: super::util::ease::out_quad(self.shown) }
    }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::{slider_fraction, MenuFrame, PRESETS};

    pub struct MenuView {
        root: Element,
        quality_buttons: Vec<Element>,
        invert_buttons: Vec<Element>,
    }

    impl MenuView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-menu"), Some(parent));
            let inner = dom::el("div", Some("ow-menu-inner"), Some(&root));
            let h1 = dom::el("h1", None, Some(&inner));
            dom::set_text(&h1, "PAUSED");
            let sub = dom::el("div", Some("sub"), Some(&inner));
            dom::set_text(&sub, "OVERWATCH — TACTICAL OPERATIONS");
            dom::el("div", Some("rule"), Some(&inner));
            let rows = dom::el("div", None, Some(&inner));

            let q_row = dom::el("div", Some("ow-row"), Some(&rows));
            let q_name = dom::el("div", Some("name"), Some(&q_row));
            dom::set_text(&q_name, "GRAPHICS PRESET");
            let q_seg = dom::el("div", Some("ow-seg"), Some(&q_row));
            let quality_buttons: Vec<Element> = PRESETS
                .iter()
                .map(|p| {
                    let b = dom::el("button", None, Some(&q_seg));
                    dom::set_text(&b, p.name());
                    b
                })
                .collect();

            let sens_row = dom::el("div", Some("ow-row"), Some(&rows));
            let sens_name = dom::el("div", Some("name"), Some(&sens_row));
            dom::set_text(&sens_name, "MOUSE SENSITIVITY");
            dom::el("div", Some("ow-slider"), Some(&sens_row));

            let fov_row = dom::el("div", Some("ow-row"), Some(&rows));
            let fov_name = dom::el("div", Some("name"), Some(&fov_row));
            dom::set_text(&fov_name, "FIELD OF VIEW");
            dom::el("div", Some("ow-slider"), Some(&fov_row));

            let inv_row = dom::el("div", Some("ow-row"), Some(&rows));
            let inv_name = dom::el("div", Some("name"), Some(&inv_row));
            dom::set_text(&inv_name, "INVERT LOOK");
            let inv_seg = dom::el("div", Some("ow-seg"), Some(&inv_row));
            let invert_buttons: Vec<Element> = ["off", "on"]
                .iter()
                .map(|label| {
                    let b = dom::el("button", None, Some(&inv_seg));
                    dom::set_text(&b, label);
                    b
                })
                .collect();

            let btns = dom::el("div", Some("ow-btns"), Some(&inner));
            let resume = dom::el("button", Some("ow-btn primary"), Some(&btns));
            dom::set_text(&resume, "Resume");
            let reset = dom::el("button", Some("ow-btn"), Some(&btns));
            dom::set_text(&reset, "Defaults");
            let hint = dom::el("div", Some("hint"), Some(&inner));
            dom::set_text(&hint, "ESC RESUME \u{b7} WASD MOVE \u{b7} SHIFT SPRINT \u{b7} R RELOAD \u{b7} F USE");

            dom::set_display(&root, "none");
            dom::set_style(&root, "cursor", "default");
            let _ = (resume, reset);
            MenuView { root, quality_buttons, invert_buttons }
        }

        pub fn sync(&self, current_quality_index: usize, invert_y: bool) {
            for (i, b) in self.quality_buttons.iter().enumerate() {
                dom::set_class(b, "on", i == current_quality_index);
            }
            for (i, b) in self.invert_buttons.iter().enumerate() {
                dom::set_class(b, "on", (i == 1) == invert_y);
            }
        }

        /// Paints a slider's fill/knob position — `menu.js:118-121`.
        pub fn paint_slider(track_fill: &Element, knob: &Element, value: f64, min: f64, max: f64) {
            let t = slider_fraction(value, min, max) * 100.0;
            dom::set_style(track_fill, "width", &format!("{t:.2}%"));
            dom::set_style(knob, "left", &format!("{t:.2}%"));
        }

        pub fn apply(&self, frame: &MenuFrame) {
            dom::set_display(&self.root, if frame.visible { "" } else { "none" });
            dom::set_style(&self.root, "pointer-events", if frame.pointer_events_auto { "auto" } else { "none" });
            if frame.visible {
                dom::set_style(&self.root, "opacity", &format!("{:.3}", frame.opacity));
            }
        }

        pub fn dispose(&self) {
            dom::remove(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingHost {
        time_scale: f64,
        control_enabled: bool,
        pointer_locked: bool,
        fov: f32,
    }

    impl Default for RecordingHost {
        fn default() -> Self {
            RecordingHost { time_scale: 1.0, control_enabled: true, pointer_locked: true, fov: 80.0 }
        }
    }

    impl MenuHost for RecordingHost {
        fn freeze_time(&mut self) -> f64 {
            let prev = self.time_scale;
            self.time_scale = 0.0;
            prev
        }
        fn set_time_scale(&mut self, scale: f64) {
            self.time_scale = scale;
        }
        fn set_player_control_enabled(&mut self, enabled: bool) {
            self.control_enabled = enabled;
        }
        fn exit_pointer_lock(&mut self) {
            self.pointer_locked = false;
        }
        fn request_pointer_lock(&mut self) {
            self.pointer_locked = true;
        }
        fn set_camera_fov(&mut self, fov_degrees: f32) {
            self.fov = fov_degrees;
        }
    }

    #[test]
    fn sensitivity_multiplier_round_trips_through_the_default() {
        // Exact now that `Config::sensitivity` is `f64`: nothing on this path
        // narrows, so the round trip is the identity the source's is.
        assert_eq!(sensitivity_multiplier(0.0022), 1.0);
        assert_eq!(sensitivity_from_multiplier(1.0), 0.0022);
    }

    #[test]
    fn slider_fraction_clamps_outside_the_range() {
        assert_eq!(slider_fraction(-10.0, SENS_MIN, SENS_MAX), 0.0);
        assert_eq!(slider_fraction(1000.0, SENS_MIN, SENS_MAX), 1.0);
        assert!((slider_fraction(1.6, 0.2, 3.0) - (1.4 / 2.8)).abs() < 1e-9);
    }

    #[test]
    fn show_freezes_time_and_disables_player_control() {
        let mut menu = PauseMenu::new();
        let events = EventBus::new();
        let mut host = RecordingHost::default();
        host.time_scale = 1.0;
        menu.show(Some(&mut host), &events);
        assert!(menu.open);
        assert_eq!(host.time_scale, 0.0);
        assert!(!host.control_enabled);
        assert!(!host.pointer_locked);
    }

    #[test]
    fn close_restores_the_time_scale_it_captured_on_show() {
        let mut menu = PauseMenu::new();
        let events = EventBus::new();
        let mut host = RecordingHost::default();
        host.time_scale = 0.5; // e.g. already in slow-mo when paused
        menu.show(Some(&mut host), &events);
        menu.close(Some(&mut host), &events);
        assert!(!menu.open);
        assert_eq!(host.time_scale, 0.5);
        assert!(host.control_enabled);
        assert!(host.pointer_locked);
    }

    #[test]
    fn show_and_close_are_idempotent() {
        let mut menu = PauseMenu::new();
        let events = EventBus::new();
        menu.show(None, &events);
        menu.show(None, &events); // no-op, already open
        assert!(menu.open);
        menu.close(None, &events);
        menu.close(None, &events); // no-op, already closed
        assert!(!menu.open);
    }

    #[test]
    fn toggle_flips_open_state() {
        let mut menu = PauseMenu::new();
        let events = EventBus::new();
        menu.toggle(None, &events);
        assert!(menu.open);
        menu.toggle(None, &events);
        assert!(!menu.open);
    }

    #[test]
    fn set_quality_updates_config_and_emits() {
        let menu = PauseMenu::new();
        let events = EventBus::new();
        let mut cfg = Config::default();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let seen2 = seen.clone();
        events.on("ui:quality", move |payload| {
            *seen2.borrow_mut() = payload.downcast_ref::<&str>().map(|s| s.to_string());
            Ok(())
        });
        menu.set_quality(&mut cfg, Quality::Low, &events);
        assert_eq!(cfg.quality, Quality::Low);
        assert_eq!(seen.borrow().as_deref(), Some("low"));
    }

    #[test]
    fn set_fov_pushes_into_the_host_camera_when_present() {
        let menu = PauseMenu::new();
        let events = EventBus::new();
        let mut cfg = Config::default();
        let mut host = RecordingHost::default();
        menu.set_fov(&mut cfg, 100.0, Some(&mut host), &events);
        assert_eq!(cfg.fov, 100.0);
        assert_eq!(host.fov, 100.0);
    }

    #[test]
    fn set_fov_without_a_host_still_updates_config() {
        let menu = PauseMenu::new();
        let events = EventBus::new();
        let mut cfg = Config::default();
        menu.set_fov(&mut cfg, 95.0, None, &events);
        assert_eq!(cfg.fov, 95.0);
    }

    #[test]
    fn reset_to_defaults_restores_every_setting() {
        let menu = PauseMenu::new();
        let events = EventBus::new();
        let mut cfg = Config::default();
        cfg.sensitivity = sensitivity_from_multiplier(2.5);
        cfg.fov = 65.0;
        cfg.invert_y = true;
        cfg.set_quality(Quality::Low);
        menu.reset_to_defaults(&mut cfg, &events);
        assert!((sensitivity_multiplier(cfg.sensitivity) - 1.0).abs() < 1e-6);
        assert_eq!(cfg.fov, 80.0);
        assert!(!cfg.invert_y);
        assert_eq!(cfg.quality, Quality::Ultra);
    }

    #[test]
    fn menu_fades_in_while_open_and_out_while_closed() {
        let mut menu = PauseMenu::new();
        let events = EventBus::new();
        menu.show(None, &events);
        for _ in 0..300 {
            menu.update(1.0 / 60.0);
        }
        let frame = menu.update(1.0 / 60.0);
        assert!(frame.visible);
        assert!(frame.pointer_events_auto);
        assert!((frame.opacity - 1.0).abs() < 1e-3);

        menu.close(None, &events);
        for _ in 0..300 {
            menu.update(1.0 / 60.0);
        }
        let frame = menu.update(1.0 / 60.0);
        assert!(!frame.visible);
    }
}
