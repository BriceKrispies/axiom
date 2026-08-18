//! Heading strip, top centre — and the slim match scoreline under it.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/compass.js:1-118`.
//!
//! Ticks are laid out once across two full revolutions (0-720deg) with left
//! positions written as `calc(Npx * var(--k))`, so a resolution change
//! re-scales the whole strip with zero per-frame work. Only the strip's
//! `translateX` is touched per frame.

use super::util::{clamp, mmss};

pub const SPAN_DEG: f64 = 120.0; // degrees visible across the strip
pub const STRIP_W: f64 = 470.0; // css px at k=1, must match .ow-compass width
pub const PPD: f64 = STRIP_W / SPAN_DEG;

/// `CARD` — the eight cardinal labels keyed by their degree, `compass.js:6`.
pub fn cardinal_label(deg_mod_360: i64) -> Option<&'static str> {
    match deg_mod_360 {
        0 => Some("N"),
        45 => Some("NE"),
        90 => Some("E"),
        135 => Some("SE"),
        180 => Some("S"),
        225 => Some("SW"),
        270 => Some("W"),
        315 => Some("NW"),
        _ => None,
    }
}

/// One tick's static layout — computed once, exactly as the source's
/// construction loop (`compass.js:23-31`); a `wasm` view builds elements from
/// this and never recomputes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickLayout {
    pub angle_deg: i64,
    pub left_px: f64,
    pub major: bool,
    /// `Some((label, is_sub))` when this angle carries a cardinal label.
    pub label: Option<(&'static str, bool)>,
}

/// The full static tick layout, `0..720` step `5` — `compass.js:23-31`.
pub fn tick_layout() -> Vec<TickLayout> {
    (0i64..720)
        .step_by(5)
        .map(|a| {
            let label = cardinal_label(a.rem_euclid(360)).map(|c| (c, c.len() > 1));
            TickLayout { angle_deg: a, left_px: a as f64 * PPD, major: a % 15 == 0, label }
        })
        .collect()
}

/// One objective's compass placement this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectiveTick {
    pub translate_x_px: f64,
    pub opacity: f64,
}

/// `compass.js:16-93`'s `Compass` class, minus its DOM handles and its object
/// pool (kept generic in the wasm view — see [`super::util::Pool`]).
pub struct Compass {
    pub k: f64,
    heading: f64,
}

impl Default for Compass {
    fn default() -> Self {
        Compass { k: 1.0, heading: 0.0 }
    }
}

impl Compass {
    pub fn new() -> Self {
        Compass::default()
    }

    pub fn set_scale(&mut self, k: f64) {
        self.k = k;
    }

    /// Strip `translateX`, in px — `compass.js:53`.
    pub fn strip_offset(&mut self, heading_deg: f64) -> f64 {
        let h = ((heading_deg % 360.0) + 360.0) % 360.0;
        self.heading = h;
        STRIP_W * 0.5 * self.k - (h + 360.0) * PPD * self.k
    }

    /// One objective's screen position on the strip, clamped to the visible
    /// half-width minus an 8px margin (`compass.js:56-76`).
    pub fn objective_tick(&self, bearing_deg: f64) -> ObjectiveTick {
        let k = self.k;
        let mut rel = bearing_deg - self.heading;
        while rel > 180.0 {
            rel -= 360.0;
        }
        while rel < -180.0 {
            rel += 360.0;
        }
        let half = STRIP_W * 0.5 * k;
        let px = clamp(rel * PPD * k, -half + 8.0 * k, half - 8.0 * k);
        let opacity = if rel.abs() > SPAN_DEG * 0.5 { 0.45 } else { 1.0 };
        ObjectiveTick { translate_x_px: px, opacity }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MatchInput {
    pub score_us: i64,
    pub score_them: i64,
    pub time_left: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchFrame {
    pub score_us: i64,
    pub score_them: i64,
    pub mode: String,
    pub clock: String,
}

/// `compass.js:96-117`'s `MatchBar` class, minus its DOM handles.
pub fn match_frame(s: &MatchInput, mode: &str) -> MatchFrame {
    MatchFrame { score_us: s.score_us, score_them: s.score_them, mode: mode.to_string(), clock: mmss(s.time_left) }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::{dom, Pool};
    use super::{tick_layout, Compass, MatchFrame, PPD};

    pub struct CompassView {
        core: Compass,
        root: Element,
        strip: Element,
        obj_pool: Pool<Element>,
    }

    impl CompassView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-compass"), Some(parent));
            let strip = dom::el("div", Some("ow-compass-strip"), Some(&root));
            dom::el("div", Some("ow-compass-base"), Some(&root));
            dom::el("div", Some("ow-compass-caret"), Some(&root));

            for tick in tick_layout() {
                let class = if tick.major { "ow-tick maj" } else { "ow-tick" };
                let t = dom::el("div", Some(class), Some(&strip));
                dom::set_style(&t, "left", &format!("calc({:.2}px * var(--k))", tick.left_px));
                if let Some((label, sub)) = tick.label {
                    let class = if sub { "ow-tick-l sub" } else { "ow-tick-l" };
                    let l = dom::el("div", Some(class), Some(&strip));
                    dom::set_text(&l, label);
                    dom::set_style(&l, "left", &format!("calc({:.2}px * var(--k))", tick.left_px));
                }
            }
            dom::set_style(&strip, "width", &format!("calc({}px * var(--k))", (720.0 * PPD) as i64));

            let obj_pool = Pool::new((0..5).map(|_| dom::el("div", Some("ow-compass-obj"), Some(&root))).collect());
            for i in 0..obj_pool.count() {
                dom::set_display(obj_pool.node(i), "none");
            }

            CompassView { core: Compass::new(), root, strip, obj_pool }
        }

        pub fn set_scale(&mut self, k: f64) {
            self.core.set_scale(k);
        }

        /// `objectives`: `(bearing_deg, label, colour)`.
        pub fn update(&mut self, heading_deg: f64, objectives: &[(f64, &str, &str)]) {
            let x = self.core.strip_offset(heading_deg);
            dom::set_style(&self.strip, "transform", &format!("translateX({x:.2}px)"));

            let n = objectives.len().min(self.obj_pool.count());
            for (i, (bearing, label, colour)) in objectives.iter().take(n).enumerate() {
                let tick = self.core.objective_tick(*bearing);
                let node = self.obj_pool.node(i);
                dom::set_display(node, "");
                dom::set_text(node, label);
                dom::set_style(node, "left", "50%");
                dom::set_style(node, "transform", &format!("translateX(calc(-50% + {:.1}px))", tick.translate_x_px));
                dom::set_style(node, "background", colour);
                dom::set_style(node, "opacity", &format!("{}", tick.opacity));
            }
            for i in n..self.obj_pool.count() {
                dom::set_display(self.obj_pool.node(i), "none");
            }
        }

        pub fn dispose(&self) {
            dom::remove(&self.root);
        }
    }

    pub struct MatchBarView {
        root: Element,
        us: Element,
        mode: Element,
        clock: Element,
        them: Element,
    }

    impl MatchBarView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-match"), Some(parent));
            let us = dom::el("b", Some("us"), Some(&root));
            dom::set_text(&us, "43");
            dom::el("div", Some("sep"), Some(&root));
            let mode = dom::el("div", None, Some(&root));
            dom::set_text(&mode, "TDM");
            let clock = dom::el("div", Some("clock"), Some(&root));
            dom::set_text(&clock, "4:12");
            dom::el("div", Some("sep"), Some(&root));
            let them = dom::el("b", Some("them"), Some(&root));
            dom::set_text(&them, "38");
            MatchBarView { root, us, mode, clock, them }
        }

        pub fn apply(&self, frame: &MatchFrame) {
            dom::set_text(&self.us, &frame.score_us.to_string());
            dom::set_text(&self.them, &frame.score_them.to_string());
            dom::set_text(&self.mode, &frame.mode);
            dom::set_text(&self.clock, &frame.clock);
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
    fn strip_offset_centres_north_when_facing_north() {
        let mut c = Compass::new();
        let x = c.strip_offset(0.0);
        // h=0 -> x = STRIP_W*0.5*k - 360*PPD*k
        let expected = STRIP_W * 0.5 - 360.0 * PPD;
        assert!((x - expected).abs() < 1e-9);
    }

    #[test]
    fn strip_offset_normalises_negative_and_large_headings() {
        let mut c = Compass::new();
        let a = c.strip_offset(-90.0);
        let mut c2 = Compass::new();
        let b = c2.strip_offset(270.0);
        assert!((a - b).abs() < 1e-9);
    }

    #[test]
    fn objective_directly_ahead_sits_at_screen_centre() {
        let mut c = Compass::new();
        c.strip_offset(90.0);
        let tick = c.objective_tick(90.0);
        assert!(tick.translate_x_px.abs() < 1e-9);
        assert_eq!(tick.opacity, 1.0);
    }

    #[test]
    fn objective_off_the_visible_span_dims() {
        let mut c = Compass::new();
        c.strip_offset(0.0);
        let tick = c.objective_tick(179.0); // rel=179 > SPAN_DEG*0.5=60
        assert_eq!(tick.opacity, 0.45);
    }

    #[test]
    fn objective_position_clamps_to_the_strip_half_width() {
        let mut c = Compass::new();
        c.strip_offset(0.0);
        let tick = c.objective_tick(180.0); // maximal relative bearing
        let half = STRIP_W * 0.5 * c.k;
        assert!(tick.translate_x_px <= half - 8.0 * c.k + 1e-9);
        assert!(tick.translate_x_px >= -(half - 8.0 * c.k) - 1e-9);
    }

    #[test]
    fn tick_layout_has_144_ticks_spanning_two_revolutions() {
        let ticks = tick_layout();
        assert_eq!(ticks.len(), 144); // 720/5
        assert!(ticks.iter().any(|t| t.major));
        assert_eq!(ticks[0].label, Some(("N", false)));
    }

    #[test]
    fn cardinal_labels_are_only_defined_on_45_degree_steps() {
        assert_eq!(cardinal_label(0), Some("N"));
        assert_eq!(cardinal_label(45), Some("NE"));
        assert_eq!(cardinal_label(10), None);
    }

    #[test]
    fn match_frame_formats_the_clock_as_mmss() {
        let frame = match_frame(&MatchInput { score_us: 5, score_them: 3, time_left: 72.9 }, "TDM");
        assert_eq!(frame.clock, "1:12");
        assert_eq!(frame.mode, "TDM");
        assert_eq!(frame.score_us, 5);
        assert_eq!(frame.score_them, 3);
    }
}
