//! Ammo / weapon readout, bottom right.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/ammo.js:1-199`.
//!
//! ```text
//!              ^ 2   * 1     equipment — its OWN row
//!   [AUTO]        M4A1
//!              28 / 210
//!   pip pip pip pip ...      magazine state, one pip per round
//! ```
//!
//! Layout contract: the panel is a single column of fixed width pinned to the
//! right margin (`--ammo-w` in [`super::style`]) and every row is an explicit
//! grid, so all rows share one left edge. Deliberately understated: three ink
//! levels, no boxes, no icons bigger than the type. The only colour is the
//! low-ammo amber and the empty-mag red.

use super::util::{clamp01, ease};

pub const MAX_PIPS: usize = 30;

/// Tracking / size steps the weapon name falls back through when it
/// overflows (`ammo.js:6-11`) — `(letter-spacing, font-size)`.
pub const NAME_FIT: [(&str, &str); 4] = [
    (".22em", "calc(12.5px * var(--k))"),
    (".14em", "calc(12.5px * var(--k))"),
    (".1em", "calc(11px * var(--k))"),
    (".06em", "calc(9.5px * var(--k))"),
];

#[derive(Debug, Clone)]
pub struct AmmoInput {
    pub ammo: i64,
    pub reserve: i64,
    pub mag_size: i64,
    pub weapon_name: String,
    pub fire_mode: String,
    pub reloading: bool,
    pub reload_progress: f64,
    pub lethal_count: i64,
    pub tactical_count: i64,
    /// `s.time` — used only to pulse the empty-mag prompt.
    pub time: f64,
}

impl Default for AmmoInput {
    fn default() -> Self {
        AmmoInput {
            ammo: 30,
            reserve: 210,
            mag_size: 30,
            weapon_name: "M4A1".to_string(),
            fire_mode: "AUTO".to_string(),
            reloading: false,
            reload_progress: 0.0,
            lethal_count: 2,
            tactical_count: 1,
            time: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReloadPromptText {
    Reloading,
    PressRToReload,
    Hidden,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AmmoFrame {
    pub cur_scale: f64,
    pub low: bool,
    pub empty: bool,
    /// `Some(count)` — how many of the (up to [`MAX_PIPS`]) pip elements should
    /// be shown at all; index `>= count` is `display:none` regardless of fill.
    pub pip_count: usize,
    /// One `(filled, warn)` pair per visible pip.
    pub pips: Vec<(bool, bool)>,
    pub reload_prompt: ReloadPromptText,
    pub reload_prompt_opacity: f64,
    pub reload_bar_visible: bool,
    pub reload_bar_scale: f64,
}

/// `ammo.js:56-199`'s `AmmoPanel` class, minus its DOM handles.
pub struct AmmoPanel {
    punch: f64,
    last_ammo: i64,
    /// `this._lastCount` (`ammo.js:89`, `:148`). The source runs the per-pip
    /// class loop **only when `filled` changes**, so the `warn` amber is a
    /// function of the state at the last *transition*, not of the current
    /// frame. Recomputing every frame is not a harmless optimisation: a reload
    /// that ends with the magazine below 34% settles its final `filled` while
    /// `reloading` is still true (so `warn` is written `false` and cached), and
    /// the frame after — same `filled`, `reloading` now false — the source
    /// skips the loop and the pips stay white. Recomputed, they turn amber.
    last_count: i64,
    /// The `(filled, warn)` pairs that loop last wrote. Deliberately *not*
    /// resized with `pip_count`: the source leaves a newly-revealed pip
    /// carrying the previous weapon's classes when a swap changes `pipCount`
    /// while `filled` happens to stay equal, and the view's `enumerate` over
    /// this vector reproduces that exactly.
    pips: Vec<(bool, bool)>,
}

impl Default for AmmoPanel {
    fn default() -> Self {
        AmmoPanel { punch: 0.0, last_ammo: -1, last_count: -1, pips: Vec::new() }
    }
}

impl AmmoPanel {
    pub fn new() -> Self {
        AmmoPanel::default()
    }

    pub fn update(&mut self, dt: f64, s: &AmmoInput) -> AmmoFrame {
        let ammo = s.ammo.max(0);
        // `Math.max(1, s.magSize | 0 || 30)` (`ammo.js:102`). JS's `||` falls
        // through on **zero only**, so a negative mag size stays negative and
        // is then clamped to 1 (one pip); it does not fall back to 30. The
        // guard is `!= 0`, not `> 0`.
        let mag_size = if s.mag_size != 0 { s.mag_size } else { 30 }.max(1);

        if self.last_ammo != ammo {
            if self.last_ammo >= 0 && ammo < self.last_ammo {
                self.punch = 1.0;
            }
            self.last_ammo = ammo;
        }

        self.punch = (self.punch - dt * 6.5).max(0.0);
        let cur_scale = 1.0 - 0.075 * ease::out_quad(self.punch);

        let frac = ammo as f64 / mag_size as f64;
        let low = ammo > 0 && frac <= 0.34;
        let empty = ammo == 0;

        let reloading = s.reloading;
        let reload_p = clamp01(s.reload_progress);

        // --- magazine pips ------------------------------------------------
        // The strip shows the state of the magazine that is *in the gun*.
        // During a reload that is not the pre-reload count: the old mag
        // leaves (strip empties) and the fresh one seats (strip fills to
        // what the gun will actually hold).
        let pip_count = (mag_size as usize).min(MAX_PIPS);
        let pip_ammo = if reloading {
            let after = mag_size.min(ammo + s.reserve.max(0)) as f64;
            if reload_p < 0.45 {
                ammo as f64 * (1.0 - reload_p / 0.45)
            } else {
                after * ((reload_p - 0.45) / 0.55)
            }
        } else {
            ammo as f64
        };
        let filled = if pip_ammo <= 0.001 {
            0
        } else {
            (((pip_ammo / mag_size as f64) * pip_count as f64).round() as i64).max(1) as usize
        };
        // `if (filled !== this._lastCount) { … }` (`ammo.js:148-155`) — see
        // `AmmoPanel::last_count`.
        if filled as i64 != self.last_count {
            self.last_count = filled as i64;
            let warn_threshold =
                !reloading && pip_count > 0 && (filled as f64 / pip_count as f64) <= 0.34;
            self.pips = (0..pip_count)
                .map(|i| (i < filled, i < filled && warn_threshold))
                .collect();
        }
        let pips = self.pips.clone();

        // --- reload state ---------------------------------------------------
        let reload_prompt = if reloading {
            ReloadPromptText::Reloading
        } else if ammo == 0 {
            ReloadPromptText::PressRToReload
        } else {
            ReloadPromptText::Hidden
        };
        let reload_prompt_opacity = if !reloading && ammo == 0 {
            0.55 + 0.45 * (s.time * 3.8).sin().abs()
        } else {
            1.0
        };

        AmmoFrame {
            cur_scale,
            low,
            empty,
            pip_count,
            pips,
            reload_prompt,
            reload_prompt_opacity,
            reload_bar_visible: reloading,
            reload_bar_scale: reload_p,
        }
    }

    /// `_fitName` (`ammo.js:185-194`) is a DOM layout measurement
    /// (`scrollWidth`/`clientWidth`) — there is no pure equivalent, so it
    /// stays in [`view`], the only place that can ask a real element how wide
    /// its text ran.
    pub fn punch(&self) -> f64 {
        self.punch
    }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::{AmmoFrame, AmmoInput, ReloadPromptText, MAX_PIPS, NAME_FIT};

    fn frag_icon(parent: &Element) -> Element {
        let s = dom::svg("svg", Some(parent));
        dom::set_attr(&s, "viewBox", "0 0 16 20");
        dom::set_attr(&s, "fill", "rgba(255,255,255,.92)");
        let p1 = dom::svg("path", Some(&s));
        dom::set_attr(&p1, "d", "M6.4 0h3.2v2.1h1.5l1.1 2H3.8l1.1-2h1.5z");
        let p2 = dom::svg("path", Some(&s));
        dom::set_attr(&p2, "d", "M8 4.6c3.1 0 5.6 2.9 5.6 7.1S11.1 20 8 20 2.4 15.9 2.4 11.7 4.9 4.6 8 4.6z");
        let g = dom::svg("g", Some(&s));
        dom::set_attr(&g, "stroke", "rgba(0,0,0,.5)");
        dom::set_attr(&g, "stroke-width", "0.9");
        for y in [9.5, 13.0, 16.2] {
            let line = dom::svg("line", Some(&g));
            dom::set_attr(&line, "x1", "3");
            dom::set_attr(&line, "y1", &y.to_string());
            dom::set_attr(&line, "x2", "13");
            dom::set_attr(&line, "y2", &y.to_string());
        }
        let axis = dom::svg("line", Some(&g));
        dom::set_attr(&axis, "x1", "8");
        dom::set_attr(&axis, "y1", "5");
        dom::set_attr(&axis, "x2", "8");
        dom::set_attr(&axis, "y2", "19.6");
        s
    }

    fn flash_icon(parent: &Element) -> Element {
        let s = dom::svg("svg", Some(parent));
        dom::set_attr(&s, "viewBox", "0 0 16 20");
        dom::set_attr(&s, "fill", "rgba(255,255,255,.92)");
        let p1 = dom::svg("path", Some(&s));
        dom::set_attr(&p1, "d", "M6.2 0h3.6v2.4H6.2z");
        let p2 = dom::svg("path", Some(&s));
        dom::set_attr(
            &p2,
            "d",
            "M4.2 3.1h7.6c.5 0 .9.4.9.9v13.4c0 1.4-1.1 2.6-2.6 2.6H5.9c-1.4 0-2.6-1.2-2.6-2.6V4c0-.5.4-.9.9-.9z",
        );
        for (y, h) in [(6.2, 1.2), (9.1, 1.2)] {
            let r = dom::svg("rect", Some(&s));
            dom::set_attr(&r, "x", "4.6");
            dom::set_attr(&r, "y", &y.to_string());
            dom::set_attr(&r, "width", "6.8");
            dom::set_attr(&r, "height", &h.to_string());
            dom::set_attr(&r, "fill", "rgba(0,0,0,.45)");
        }
        s
    }

    pub struct AmmoView {
        root: Element,
        slot_l: Element,
        slot_ln: Element,
        slot_t: Element,
        slot_tn: Element,
        mode: Element,
        name: Element,
        cur: Element,
        res: Element,
        pips: Vec<Element>,
        reload: Element,
        reload_bar: Element,
        reload_fill: Element,
        last_name: Option<String>,
    }

    impl AmmoView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-ammo"), Some(parent));

            let equip = dom::el("div", Some("ow-equip"), Some(&root));
            let slot_l = dom::el("div", Some("ow-slot"), Some(&equip));
            frag_icon(&slot_l);
            let slot_ln = dom::el("span", None, Some(&slot_l));
            dom::set_text(&slot_ln, "2");
            let slot_t = dom::el("div", Some("ow-slot"), Some(&equip));
            flash_icon(&slot_t);
            let slot_tn = dom::el("span", None, Some(&slot_t));
            dom::set_text(&slot_tn, "1");

            let head = dom::el("div", Some("ow-ammo-head"), Some(&root));
            let mode = dom::el("div", Some("ow-ammo-mode"), Some(&head));
            dom::set_text(&mode, "AUTO");
            let name = dom::el("div", Some("ow-ammo-name"), Some(&head));
            dom::set_text(&name, "M4A1");

            let row = dom::el("div", Some("ow-ammo-row"), Some(&root));
            let cur = dom::el("div", Some("ow-ammo-cur"), Some(&row));
            dom::set_text(&cur, "30");
            let sep = dom::el("div", Some("ow-ammo-sep"), Some(&row));
            dom::set_text(&sep, "/");
            let res = dom::el("div", Some("ow-ammo-res"), Some(&row));
            dom::set_text(&res, "210");

            let mag = dom::el("div", Some("ow-mag"), Some(&root));
            let pips: Vec<Element> = (0..MAX_PIPS).map(|_| dom::el("b", None, Some(&mag))).collect();

            let reload = dom::el("div", Some("ow-reload"), Some(&root));
            dom::set_text(&reload, "RELOADING");
            let bar = dom::el("div", Some("ow-reload-bar"), Some(&root));
            let reload_fill = dom::el("i", None, Some(&bar));
            dom::set_display(&reload, "none");
            dom::set_display(&bar, "none");

            AmmoView {
                root,
                slot_l,
                slot_ln,
                slot_t,
                slot_tn,
                mode,
                name,
                cur,
                res,
                pips,
                reload,
                reload_bar: bar,
                reload_fill,
                last_name: None,
            }
        }

        pub fn apply(&mut self, s: &AmmoInput, frame: &AmmoFrame) {
            dom::set_text(&self.cur, &s.ammo.max(0).to_string());
            dom::set_text(&self.res, &s.reserve.max(0).to_string());
            self.fit_name(&s.weapon_name);
            dom::set_text(&self.mode, &s.fire_mode);

            dom::set_style(&self.cur, "transform", &format!("scale({:.3})", frame.cur_scale));
            dom::set_class(&self.root, "ow-ammo-low", frame.low);
            dom::set_class(&self.root, "ow-ammo-empty", frame.empty);

            for (i, node) in self.pips.iter().enumerate() {
                dom::set_display(node, if i < frame.pip_count { "" } else { "none" });
            }
            for (i, (filled, warn)) in frame.pips.iter().enumerate() {
                dom::set_class(&self.pips[i], "off", !filled);
                dom::set_class(&self.pips[i], "warn", *warn);
            }

            let (text, visible) = match frame.reload_prompt {
                ReloadPromptText::Reloading => ("RELOADING", true),
                ReloadPromptText::PressRToReload => ("PRESS R TO RELOAD", true),
                ReloadPromptText::Hidden => ("", false),
            };
            dom::set_display(&self.reload, if visible { "" } else { "none" });
            dom::set_text(&self.reload, text);
            dom::set_style(&self.reload, "opacity", &format!("{:.3}", frame.reload_prompt_opacity));

            dom::set_display(&self.reload_bar, if frame.reload_bar_visible { "" } else { "none" });
            if frame.reload_bar_visible {
                dom::set_style(&self.reload_fill, "transform", &format!("scaleX({:.3})", frame.reload_bar_scale));
            }

            dom::set_text(&self.slot_ln, &s.lethal_count.to_string());
            dom::set_text(&self.slot_tn, &s.tactical_count.to_string());
            dom::set_class(&self.slot_l, "empty", s.lethal_count <= 0);
            dom::set_class(&self.slot_t, "empty", s.tactical_count <= 0);
        }

        /// `_fitName` — measures the weapon-name glyph run against the
        /// column and steps tracking/size down until it fits; measured once
        /// per name change.
        fn fit_name(&mut self, name: &str) {
            if self.last_name.as_deref() == Some(name) {
                return;
            }
            self.last_name = Some(name.to_string());
            dom::set_text(&self.name, name);
            let html: web_sys::HtmlElement = wasm_bindgen::JsCast::unchecked_into(self.name.clone());
            for (spacing, size) in NAME_FIT {
                dom::set_style(&self.name, "letter-spacing", spacing);
                dom::set_style(&self.name, "font-size", size);
                if html.scroll_width() <= html.client_width() + 1 {
                    break;
                }
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

    fn input() -> AmmoInput {
        AmmoInput::default()
    }

    #[test]
    fn full_magazine_is_not_low_or_empty() {
        let mut panel = AmmoPanel::new();
        let frame = panel.update(1.0 / 60.0, &input());
        assert!(!frame.low);
        assert!(!frame.empty);
        assert_eq!(frame.pip_count, 30);
        assert_eq!(frame.pips.iter().filter(|(f, _)| *f).count(), 30);
    }

    #[test]
    fn low_ammo_crosses_the_34_percent_threshold() {
        let mut panel = AmmoPanel::new();
        let mut s = input();
        s.ammo = 10; // 10/30 = 0.333.. <= 0.34
        let frame = panel.update(1.0 / 60.0, &s);
        assert!(frame.low);
        assert!(!frame.empty);
    }

    #[test]
    fn empty_magazine_shows_the_reload_prompt() {
        let mut panel = AmmoPanel::new();
        let mut s = input();
        s.ammo = 0;
        let frame = panel.update(1.0 / 60.0, &s);
        assert!(frame.empty);
        assert_eq!(frame.reload_prompt, ReloadPromptText::PressRToReload);
    }

    #[test]
    fn firing_a_round_punches_the_counter_which_decays() {
        let mut panel = AmmoPanel::new();
        let mut s = input();
        panel.update(1.0 / 60.0, &s); // establishes last_ammo = 30
        s.ammo = 29;
        let frame = panel.update(1.0 / 60.0, &s);
        assert!(frame.cur_scale < 1.0, "a fired round should punch the counter down-scale");
        for _ in 0..60 {
            panel.update(1.0 / 60.0, &s);
        }
        let settled = panel.update(1.0 / 60.0, &s);
        assert!((settled.cur_scale - 1.0).abs() < 1e-6);
    }

    #[test]
    fn reload_pips_empty_then_refill_across_the_progress_split() {
        let mut panel = AmmoPanel::new();
        let mut s = input();
        s.ammo = 30;
        s.reserve = 210;
        s.reloading = true;
        s.reload_progress = 0.2; // < 0.45: mag emptying
        let frame = panel.update(1.0 / 60.0, &s);
        let filled_early = frame.pips.iter().filter(|(f, _)| *f).count();

        s.reload_progress = 0.9; // > 0.45: fresh mag seating
        let frame = panel.update(1.0 / 60.0, &s);
        let filled_late = frame.pips.iter().filter(|(f, _)| *f).count();
        assert!(filled_late > filled_early);
    }

    #[test]
    fn reload_hides_the_press_r_prompt_and_shows_the_bar() {
        let mut panel = AmmoPanel::new();
        let mut s = input();
        s.reloading = true;
        s.reload_progress = 0.5;
        let frame = panel.update(1.0 / 60.0, &s);
        assert_eq!(frame.reload_prompt, ReloadPromptText::Reloading);
        assert!(frame.reload_bar_visible);
        assert_eq!(frame.reload_bar_scale, 0.5);
    }

    #[test]
    fn mag_size_is_capped_at_max_pips() {
        let mut panel = AmmoPanel::new();
        let mut s = input();
        s.mag_size = 100;
        s.ammo = 100;
        let frame = panel.update(1.0 / 60.0, &s);
        assert_eq!(frame.pip_count, MAX_PIPS);
    }
}
