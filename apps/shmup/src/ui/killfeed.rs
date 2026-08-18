//! Killfeed, top right. Newest row on top, six visible, 5.6s dwell.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/killfeed.js:1-95`.
//!
//! Rows the local player is involved in get the amber treatment so your own
//! kills are readable at a glance without reading the names.

use super::util::{clamp01, ease, Pool};

#[derive(Debug, Clone)]
pub struct KillEvent {
    pub attacker: String,
    pub victim: String,
    pub headshot: bool,
    pub mine: bool,
    /// `None` means "unknown/local" (the source's default, no colour
    /// override); `Some(true)` = the local player's side, `Some(false)` =
    /// the enemy's.
    pub attacker_friendly: Option<bool>,
}

impl Default for KillEvent {
    fn default() -> Self {
        KillEvent {
            attacker: "UNKNOWN".to_string(),
            victim: "UNKNOWN".to_string(),
            headshot: false,
            mine: false,
            attacker_friendly: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowFrame {
    pub translate_x: f64,
    pub opacity: f64,
}

/// `killfeed.js:31-95`'s `Killfeed` class, minus its DOM handles.
pub struct Killfeed<N> {
    pool: Pool<N>,
    life: f64,
}

impl<N> Killfeed<N> {
    pub fn new(nodes: Vec<N>) -> Self {
        Killfeed { pool: Pool::new(nodes), life: 5.6 }
    }

    pub fn push(&mut self) -> usize {
        let i = self.pool.acquire();
        self.pool.slots_mut()[i].life = self.life;
        i
    }

    pub fn update(&mut self, dt: f64) -> Vec<(usize, RowFrame)> {
        let mut out = Vec::new();
        for i in 0..self.pool.count() {
            let mut slot = self.pool.slot(i);
            if !slot.alive {
                continue;
            }
            slot.t += dt;
            if slot.t >= slot.life {
                self.pool.release(i);
                continue;
            }
            self.pool.slots_mut()[i] = slot;

            let in_t = clamp01(slot.t / 0.16);
            let out_t = clamp01((slot.t - (slot.life - 0.45)) / 0.45);
            let x = (1.0 - ease::out_quint(in_t)) * 26.0;
            let a = ease::out_quad(in_t) * (1.0 - ease::in_quad(out_t));
            out.push((i, RowFrame { translate_x: x, opacity: a }));
        }
        out
    }

    pub fn clear(&mut self) {
        self.pool.release_all();
    }

    pub fn node(&self, i: usize) -> &N {
        self.pool.node(i)
    }

    pub fn count(&self) -> usize {
        self.pool.count()
    }

    pub fn slot(&self, i: usize) -> super::util::PoolSlot {
        self.pool.slot(i)
    }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::{KillEvent, Killfeed, RowFrame};

    fn rifle_icon(parent: &Element) -> Element {
        let s = dom::svg("svg", Some(parent));
        dom::set_attr(&s, "viewBox", "0 0 28 11");
        dom::set_attr(&s, "fill", "rgba(240,246,250,.9)");
        for points in [
            "0.5,4.2 6,4.2 6,8 2.2,8",
            "9,7 12.6,7 11.9,11 9.7,11",
            "13,7 15,7 14.1,10.2 12.5,10.2",
        ] {
            let poly = dom::svg("polygon", Some(&s));
            dom::set_attr(&poly, "points", points);
        }
        for (x, y, w, h) in [(6.0, 3.6, 8.2, 3.4), (14.2, 4.2, 6.4, 2.1), (20.6, 4.6, 6.9, 1.2), (23.6, 2.6, 1.1, 2.0)] {
            let r = dom::svg("rect", Some(&s));
            dom::set_attr(&r, "x", &x.to_string());
            dom::set_attr(&r, "y", &y.to_string());
            dom::set_attr(&r, "width", &w.to_string());
            dom::set_attr(&r, "height", &h.to_string());
        }
        s
    }

    fn skull_icon(parent: &Element) -> Element {
        let s = dom::svg("svg", Some(parent));
        dom::set_attr(&s, "viewBox", "0 0 11 11");
        dom::set_attr(&s, "fill", "rgba(255,194,71,.95)");
        let p = dom::svg("path", Some(&s));
        dom::set_attr(
            &p,
            "d",
            "M5.5.8c2.4 0 4.1 1.7 4.1 4 0 1.5-.7 2.4-1.5 3v1.3H3v-1.3c-.9-.6-1.6-1.5-1.6-3 0-2.3 1.7-4 4.1-4z",
        );
        for (cx, cy) in [(3.9, 4.6), (7.1, 4.6)] {
            let c = dom::svg("circle", Some(&s));
            dom::set_attr(&c, "cx", &cx.to_string());
            dom::set_attr(&c, "cy", &cy.to_string());
            dom::set_attr(&c, "r", "1.15");
            dom::set_attr(&c, "fill", "rgba(10,12,14,.9)");
        }
        let mouth = dom::svg("rect", Some(&s));
        dom::set_attr(&mouth, "x", "5.05");
        dom::set_attr(&mouth, "y", "6.1");
        dom::set_attr(&mouth, "width", "0.9");
        dom::set_attr(&mouth, "height", "1.3");
        dom::set_attr(&mouth, "fill", "rgba(10,12,14,.9)");
        for x in [3.4, 6.2] {
            let tooth = dom::svg("rect", Some(&s));
            dom::set_attr(&tooth, "x", &x.to_string());
            dom::set_attr(&tooth, "y", "9.2");
            dom::set_attr(&tooth, "width", "1.4");
            dom::set_attr(&tooth, "height", "1.5");
        }
        s
    }

    pub struct KfRow {
        row: Element,
        attacker: Element,
        victim: Element,
        headshot: Element,
    }

    fn build_row() -> KfRow {
        let row = dom::el("div", Some("ow-kf-row"), None);
        let attacker = dom::el("span", Some("ow-kf-a"), Some(&row));
        dom::set_text(&attacker, "PLAYER");
        let weapon = dom::el("span", Some("ow-kf-w"), Some(&row));
        let headshot = dom::el("span", Some("ow-kf-hs"), Some(&weapon));
        skull_icon(&headshot);
        rifle_icon(&weapon);
        let victim = dom::el("span", Some("ow-kf-v"), Some(&row));
        dom::set_text(&victim, "ENEMY");
        KfRow { row, attacker, victim, headshot }
    }

    pub struct KillfeedView {
        core: Killfeed<KfRow>,
        root: Element,
    }

    impl KillfeedView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-killfeed"), Some(parent));
            let nodes: Vec<KfRow> = (0..6).map(|_| build_row()).collect();
            KillfeedView { core: Killfeed::new(nodes), root }
        }

        pub fn push(&mut self, e: &KillEvent) {
            let i = self.core.push();
            let node = self.core.node(i);
            // newest on top: re-parent to the front of the killfeed root.
            self.root.prepend_with_node_1(&node.row).expect("prepend");
            dom::set_text(&node.attacker, &e.attacker.to_uppercase());
            dom::set_text(&node.victim, &e.victim.to_uppercase());
            dom::set_display(&node.headshot, if e.headshot { "" } else { "none" });
            dom::set_class(&node.row, "mine", e.mine);
            let (a_colour, v_colour) = match e.attacker_friendly {
                Some(false) => ("var(--enemy)", "var(--friend)"),
                _ => ("", ""),
            };
            dom::set_style(&node.attacker, "color", a_colour);
            dom::set_style(&node.victim, "color", v_colour);
        }

        // Released rows stay in the DOM at their last-painted transform/opacity
        // (opacity 0 by the time release fires) — the source never removes a
        // row element, it only frees the slot for [`Killfeed::push`] to reuse.
        pub fn update(&mut self, dt: f64) {
            for (i, frame) in self.core.update(dt) {
                Self::apply(self.core.node(i), &frame);
            }
        }

        fn apply(node: &KfRow, frame: &RowFrame) {
            dom::set_style(&node.row, "transform", &format!("translateX({:.2}px)", frame.translate_x));
            dom::set_style(&node.row, "opacity", &format!("{:.3}", frame.opacity));
        }

        pub fn clear(&mut self) {
            self.core.clear();
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
    fn row_fades_in_then_holds_then_fades_out() {
        let mut kf: Killfeed<()> = Killfeed::new((0..6).map(|_| ()).collect());
        let i = kf.push();
        let frames = kf.update(0.01);
        let (_, early) = frames.iter().find(|(idx, _)| *idx == i).unwrap();
        assert!(early.opacity < 1.0, "row is still fading in at t=0.01");
        assert!(early.translate_x > 0.0, "row slides in from the right");

        // fully faded in, not yet fading out (life is 5.6s, tail starts at life-0.45).
        let frames = kf.update(2.0);
        let (_, mid) = frames.iter().find(|(idx, _)| *idx == i).unwrap();
        assert!((mid.opacity - 1.0).abs() < 1e-6);
        assert_eq!(mid.translate_x, 0.0);
    }

    #[test]
    fn row_releases_itself_after_its_dwell() {
        let mut kf: Killfeed<()> = Killfeed::new((0..6).map(|_| ()).collect());
        let i = kf.push();
        let frames = kf.update(6.0); // > 5.6s life
        assert!(frames.iter().all(|(idx, _)| *idx != i));
        assert!(!kf.slot(i).alive);
    }

    #[test]
    fn six_slot_pool_reuses_oldest_row_on_a_seventh_push() {
        let mut kf: Killfeed<()> = Killfeed::new((0..6).map(|_| ()).collect());
        for _ in 0..6 {
            kf.push();
        }
        let seventh = kf.push();
        assert!(seventh < 6);
    }
}
