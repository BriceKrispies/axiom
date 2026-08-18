//! Health feedback: the screen-space hurt state *and* the vitals widget.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/health.js:1-178`.
//!
//!  - 0-25% hurt: nothing but a faint edge darkening
//!  - 25-60%: blood vignette blooms in, world desaturates
//!  - 60-100%: heartbeat pulses the vignette, saturation drops hard
//!  - on hit: a 180ms directional-agnostic red flash
//!  - regen: vignette breathes out over ~2s and saturation returns
//!
//! The vignette is two stacked layers pushed through an `feTurbulence`
//! displacement filter ([`super::style::DEFS`]) so its edge is organic; a
//! clean radial gradient is the single most "WebGL demo" thing a hurt overlay
//! can do.
//!
//! The vitals widget lives bottom-left of the safe area — the mirror of the
//! ammo block — because it holds the most important number on the screen.

use super::util::{clamp01, damp, ease, lerp};

#[derive(Debug, Clone, Copy, Default)]
pub struct HealthInput {
    pub health: f64,
    pub max_health: f64,
    pub armour: f64,
    pub max_armour: f64,
    pub regen: bool,
}

/// Everything a wasm view needs to paint one frame's hurt overlay + vitals
/// widget — every value already computed, so [`super::util::dom`] callers
/// only ever format-and-write.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HealthFrame {
    pub blood_opacity: f64,
    pub blood_visible: bool,
    pub blood_scale: f64,
    pub beat_opacity: f64,
    pub beat_visible: bool,
    pub desat_visible: bool,
    pub desat_opacity: f64,
    pub flash_visible: bool,
    pub flash_opacity: f64,
    pub hp_fill_scale: f64,
    pub hp_shown_value: i64,
    pub hp_max_value: i64,
    pub hp_num_scale: f64,
    pub vitals_low: bool,
    pub vitals_crit: bool,
    pub armour_opacity: f64,
    pub armour_visible: bool,
    /// `[fraction; 3]`, one per plate; `None` when the row is not shown.
    pub plates: Option<[f64; 3]>,
}

/// `health.js:27-178`'s `HealthFx` class, minus its DOM handles.
pub struct HealthFx {
    hurt: f64,
    beat_phase: f64,
    beat_energy: f64,
    last_beat: i64,
    regen_t: f64,
    flash_t: f64,
    flash_peak: f64,
    hp_shown: f64,
    last_hp: i64,
    armour_shown: f64,
    /// Fires when a new heartbeat cycle begins, carrying its intensity — the
    /// source's `this.onBeat` hook (`health.js:65`, `102`).
    pub on_beat: Option<Box<dyn FnMut(f64)>>,
}

impl Default for HealthFx {
    fn default() -> Self {
        HealthFx {
            hurt: 0.0,
            beat_phase: 0.0,
            beat_energy: 0.0,
            last_beat: 0,
            regen_t: 1.0,
            flash_t: 1.0,
            flash_peak: 1.0,
            hp_shown: 1.0,
            last_hp: -1,
            armour_shown: 0.0,
            on_beat: None,
        }
    }
}

impl HealthFx {
    pub fn new() -> Self {
        HealthFx::default()
    }

    pub fn on_damage(&mut self, intensity: f64) {
        self.flash_t = 0.0;
        self.flash_peak = 0.35 + 0.65 * clamp01(intensity);
    }

    pub fn on_regen_start(&mut self) {
        self.regen_t = 0.0;
    }

    pub fn update(&mut self, dt: f64, s: HealthInput) -> HealthFrame {
        let max_health = if s.max_health != 0.0 { s.max_health } else { 100.0 };
        let h = clamp01(s.health / max_health);
        let target_hurt = clamp01((0.78 - h) / 0.78).powf(1.3);
        self.hurt = damp(self.hurt, target_hurt, 7.0, dt);
        let hurt = self.hurt;

        // --- heartbeat --------------------------------------------------------
        let beat_intensity = clamp01((0.5 - h) / 0.5);
        if beat_intensity > 0.02 {
            let hz = lerp(1.15, 2.35, beat_intensity);
            self.beat_phase += dt * hz;
            let p = self.beat_phase % 1.0;
            let thump = (-((p / 0.085).powi(2))).exp() + 0.55 * (-(((p - 0.235) / 0.1).powi(2))).exp();
            self.beat_energy = thump * beat_intensity;
            let beat_index = self.beat_phase.floor() as i64;
            if beat_index != self.last_beat {
                self.last_beat = beat_index;
                if let Some(cb) = self.on_beat.as_mut() {
                    cb(beat_intensity);
                }
            }
        } else {
            self.beat_energy = damp(self.beat_energy, 0.0, 6.0, dt);
            self.beat_phase = 0.0;
        }

        // --- regeneration breath ---------------------------------------------
        if self.regen_t < 1.0 {
            self.regen_t = (self.regen_t + dt / 1.8).min(1.0);
        }
        let regen_pulse = if s.regen { 0.12 * (1.0 - ease::out_cubic(self.regen_t)) } else { 0.0 };

        let blood_a = clamp01(hurt * 1.05 + self.beat_energy * 0.16);
        let blood_scale = 1.0 + self.beat_energy * 0.022 + regen_pulse * 0.12;

        let beat_a = clamp01(self.beat_energy * 0.55);

        // backdrop-filter is expensive: only mount the element when it does work
        let desat_a = clamp01(hurt * 0.8);

        if self.flash_t < 1.0 {
            self.flash_t = (self.flash_t + dt / 0.19).min(1.0);
        }
        let flash_visible = self.flash_t < 1.0;
        let flash_opacity = if flash_visible { self.flash_peak * (1.0 - ease::out_quad(self.flash_t)) * 0.8 } else { 0.0 };

        // --- vitals readout ---------------------------------------------------
        let max_h = if s.max_health != 0.0 { s.max_health } else { 100.0 };
        let hp = s.health.max(0.0).min(max_h);
        self.hp_shown = damp(self.hp_shown, h, 16.0, dt);
        let shown_hp = hp.round() as i64;
        self.last_hp = shown_hp;

        // --- armour plates ----------------------------------------------------
        let max_a = if s.max_armour != 0.0 { s.max_armour } else { 150.0 };
        let armour = s.armour.max(0.0);
        self.armour_shown = damp(self.armour_shown, if armour > 0.0 { 1.0 } else { 0.0 }, 10.0, dt);
        let armour_visible = self.armour_shown >= 0.01;
        let plates = armour_visible.then(|| {
            let per = max_a / 3.0;
            let mut out = [0.0; 3];
            for (i, o) in out.iter_mut().enumerate() {
                *o = clamp01((armour - i as f64 * per) / per);
            }
            out
        });

        HealthFrame {
            blood_opacity: blood_a,
            blood_visible: blood_a >= 0.004,
            blood_scale,
            beat_opacity: beat_a,
            beat_visible: beat_a >= 0.004,
            desat_visible: desat_a >= 0.01,
            desat_opacity: desat_a,
            flash_visible,
            flash_opacity,
            hp_fill_scale: clamp01(self.hp_shown),
            hp_shown_value: shown_hp,
            hp_max_value: max_h.round() as i64,
            hp_num_scale: 1.0 + self.beat_energy * 0.05,
            vitals_low: h <= 0.55 && h > 0.28,
            vitals_crit: h <= 0.28,
            armour_opacity: self.armour_shown,
            armour_visible,
            plates,
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::HealthFrame;

    pub struct HealthView {
        blood_wrap: Element,
        beat: Element,
        desat: Element,
        flash: Element,
        vitals: Element,
        hp_val: Element,
        hp_max: Element,
        hp_fill: Element,
        hp_num: Element,
        armour: Element,
        plates: [Element; 3],
    }

    impl HealthView {
        pub fn new(parent: &Element, chrome: &Element) -> Self {
            let blood_wrap = dom::el("div", Some("ow-blood"), Some(parent));
            dom::el("div", Some("ow-blood-a"), Some(&blood_wrap));
            dom::el("div", Some("ow-blood-b"), Some(&blood_wrap));
            let beat = dom::el("div", Some("ow-lowbeat"), Some(parent));
            let desat = dom::el("div", Some("ow-desat"), Some(parent));
            let flash = dom::el("div", Some("ow-hitflash"), Some(parent));

            let vitals = dom::el("div", Some("ow-vitals"), Some(chrome));
            let head = dom::el("div", Some("ow-vt-head"), Some(&vitals));
            let lbl = dom::el("div", Some("ow-vt-lbl"), Some(&head));
            dom::set_text(&lbl, "Health");
            let hp_num = dom::el("div", Some("ow-vt-num"), Some(&head));
            let hp_val = dom::el("span", None, Some(&hp_num));
            dom::set_text(&hp_val, "100");
            let hp_max = dom::el("i", None, Some(&hp_num));
            dom::set_text(&hp_max, "/100");
            let track = dom::el("div", Some("ow-vt-track"), Some(&vitals));
            let hp_fill = dom::el("i", None, Some(&track));
            dom::el("u", None, Some(&track));

            let armour = dom::el("div", Some("ow-armour"), Some(&vitals));
            let alabel = dom::el("div", Some("ow-vt-lbl"), Some(&armour));
            dom::set_text(&alabel, "Armour");
            let plates_row = dom::el("div", Some("ow-arm-plates"), Some(&armour));
            let plates = std::array::from_fn(|_| {
                let plate = dom::el("div", Some("ow-plate"), Some(&plates_row));
                dom::el("i", None, Some(&plate))
            });

            dom::set_style(&blood_wrap, "opacity", "0");
            dom::set_display(&desat, "none");
            dom::set_style(&flash, "opacity", "0");
            dom::set_style(&beat, "opacity", "0");

            HealthView { blood_wrap, beat, desat, flash, vitals, hp_val, hp_max, hp_fill, hp_num, armour, plates }
        }

        pub fn apply(&self, frame: &HealthFrame) {
            dom::set_style(&self.blood_wrap, "opacity", &format!("{:.3}", frame.blood_opacity));
            dom::set_display(&self.blood_wrap, if frame.blood_visible { "" } else { "none" });
            dom::set_style(&self.blood_wrap, "transform", &format!("scale({:.4})", frame.blood_scale));

            dom::set_style(&self.beat, "opacity", &format!("{:.3}", frame.beat_opacity));
            dom::set_display(&self.beat, if frame.beat_visible { "" } else { "none" });

            dom::set_display(&self.desat, if frame.desat_visible { "" } else { "none" });
            dom::set_style(&self.desat, "opacity", &format!("{:.3}", frame.desat_opacity));

            dom::set_display(&self.flash, if frame.flash_visible { "" } else { "none" });
            if frame.flash_visible {
                dom::set_style(&self.flash, "opacity", &format!("{:.3}", frame.flash_opacity));
            }

            dom::set_style(&self.hp_fill, "transform", &format!("scaleX({:.4})", frame.hp_fill_scale));
            dom::set_text(&self.hp_val, &frame.hp_shown_value.to_string());
            dom::set_text(&self.hp_max, &format!("/{}", frame.hp_max_value));
            dom::set_class(&self.vitals, "low", frame.vitals_low);
            dom::set_class(&self.vitals, "crit", frame.vitals_crit);
            dom::set_style(&self.hp_num, "transform", &format!("scale({:.3})", frame.hp_num_scale));

            dom::set_style(&self.armour, "opacity", &format!("{:.3}", frame.armour_opacity));
            dom::set_display(&self.armour, if frame.armour_visible { "" } else { "none" });
            if let Some(plates) = frame.plates {
                for (node, f) in self.plates.iter().zip(plates.iter()) {
                    dom::set_style(node, "transform", &format!("scaleX({f:.3})"));
                    dom::set_style(node, "opacity", if *f > 0.001 { "1" } else { "0" });
                }
            }
        }

        pub fn dispose(&self) {
            dom::remove(&self.blood_wrap);
            dom::remove(&self.beat);
            dom::remove(&self.desat);
            dom::remove(&self.flash);
            dom::remove(&self.vitals);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(health: f64) -> HealthInput {
        HealthInput { health, max_health: 100.0, armour: 0.0, max_armour: 150.0, regen: false }
    }

    #[test]
    fn full_health_has_no_hurt_overlay() {
        let mut fx = HealthFx::new();
        let frame = fx.update(1.0 / 60.0, input(100.0));
        assert!(!frame.blood_visible);
        assert!(!frame.vitals_low);
        assert!(!frame.vitals_crit);
    }

    #[test]
    fn low_health_crosses_into_low_then_crit_thresholds() {
        let mut fx = HealthFx::new();
        let frame = fx.update(1.0 / 60.0, input(50.0)); // h=0.5 -> low (<=0.55, >0.28)
        assert!(frame.vitals_low);
        assert!(!frame.vitals_crit);

        let mut fx = HealthFx::new();
        let frame = fx.update(1.0 / 60.0, input(20.0)); // h=0.2 -> crit
        assert!(frame.vitals_crit);
    }

    #[test]
    fn heartbeat_fires_on_beat_once_per_cycle_below_half_health() {
        let mut fx = HealthFx::new();
        let hits = std::rc::Rc::new(std::cell::RefCell::new(0));
        let hits2 = hits.clone();
        fx.on_beat = Some(Box::new(move |_i| *hits2.borrow_mut() += 1));
        for _ in 0..300 {
            fx.update(1.0 / 60.0, input(10.0)); // deep in heartbeat range
        }
        assert!(*hits.borrow() > 0, "heartbeat callback should fire at low health");
    }

    #[test]
    fn no_heartbeat_above_half_health() {
        let mut fx = HealthFx::new();
        let hits = std::rc::Rc::new(std::cell::RefCell::new(0));
        let hits2 = hits.clone();
        fx.on_beat = Some(Box::new(move |_i| *hits2.borrow_mut() += 1));
        for _ in 0..300 {
            fx.update(1.0 / 60.0, input(90.0));
        }
        assert_eq!(*hits.borrow(), 0);
    }

    #[test]
    fn on_damage_flashes_then_decays_to_hidden() {
        let mut fx = HealthFx::new();
        fx.on_damage(1.0);
        let frame = fx.update(1.0 / 600.0, input(80.0));
        assert!(frame.flash_visible);
        assert!(frame.flash_opacity > 0.0);
        for _ in 0..60 {
            fx.update(1.0 / 60.0, input(80.0));
        }
        let settled = fx.update(1.0 / 60.0, input(80.0));
        assert!(!settled.flash_visible);
    }

    #[test]
    fn armour_row_appears_only_when_armour_present() {
        let mut fx = HealthFx::new();
        let mut s = input(100.0);
        s.armour = 0.0;
        for _ in 0..120 {
            fx.update(1.0 / 60.0, s);
        }
        let frame = fx.update(1.0 / 60.0, s);
        assert!(!frame.armour_visible);

        let mut fx = HealthFx::new();
        s.armour = 75.0;
        for _ in 0..120 {
            fx.update(1.0 / 60.0, s);
        }
        let frame = fx.update(1.0 / 60.0, s);
        assert!(frame.armour_visible);
        let plates = frame.plates.expect("armour row should carry plate fractions");
        // 75/150 max, per=50: plate0 full, plate1 half, plate2 empty.
        assert_eq!(plates[0], 1.0);
        assert!((plates[1] - 0.5).abs() < 1e-9);
        assert_eq!(plates[2], 0.0);
    }

    #[test]
    fn hp_readout_rounds_and_clamps_to_max() {
        let mut fx = HealthFx::new();
        let mut s = input(100.0);
        s.health = 500.0; // over max
        let frame = fx.update(1.0 / 60.0, s);
        assert_eq!(frame.hp_shown_value, 100);
        assert_eq!(frame.hp_max_value, 100);
    }
}
