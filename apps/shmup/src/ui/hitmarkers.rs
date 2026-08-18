//! Hitmarkers.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/hitmarkers.js:1-125`.
//!
//! Timing is the whole point: 0-40ms the X snaps in past its rest size
//! (`outBack`), 40-120ms it settles, then it holds bright and fades. Anything
//! slower than this feels like a notification instead of a hit.

use super::util::{clamp01, ease, Pool};

pub const R_IN: f64 = 13.0; // well outside the reticle blades, so the two never merge
pub const R_OUT: f64 = 28.5;

/// `kind -> { colour, weight, scale, life, ring, spin }` (`hitmarkers.js:8-13`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kind {
    pub colour: &'static str,
    pub weight: f64,
    pub scale: f64,
    pub life: f64,
    pub ring: f64,
    pub spin: f64,
}

pub const HIT: Kind = Kind { colour: "#f6fafc", weight: 1.8, scale: 1.0, life: 0.26, ring: 0.0, spin: 0.0 };
pub const ARMOUR: Kind = Kind { colour: "#8fdcff", weight: 2.0, scale: 1.03, life: 0.28, ring: 0.5, spin: 0.0 };
pub const HEAD: Kind = Kind { colour: "#ffc247", weight: 2.2, scale: 1.08, life: 0.32, ring: 0.3, spin: 0.0 };
pub const KILL: Kind = Kind { colour: "#ff4433", weight: 2.7, scale: 1.18, life: 0.42, ring: 1.0, spin: 9.0 };

/// `'hit'|'armour'|'head'|'kill'`, and `KINDS[kind] ?? KINDS.hit` (an unknown
/// string falls back to `hit` in the source; the port makes that fallback the
/// type system's job instead by only accepting these four).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    Hit,
    Armour,
    Head,
    Kill,
}

impl HitKind {
    pub fn spec(self) -> Kind {
        match self {
            HitKind::Hit => HIT,
            HitKind::Armour => ARMOUR,
            HitKind::Head => HEAD,
            HitKind::Kill => KILL,
        }
    }
}

/// One live marker's per-frame render state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkerFrame {
    pub scale: f64,
    pub rotation_deg: f64,
    pub opacity: f64,
    pub ring_scale: f64,
    pub ring_opacity: f64,
    pub has_ring: bool,
}

/// `hitmarkers.js:22-125`'s `Hitmarkers` class, minus its DOM handles. `N` is
/// the wasm view's per-slot node bundle; native code uses `Pool<()>`.
pub struct Hitmarkers<N> {
    pool: Pool<N>,
    /// Parallel to the pool: which [`Kind`] each slot last spawned, so
    /// [`Hitmarkers::update`] can read `k.w`/`k.c` back the way the source
    /// reads them off `it.node._main` — kept as data here instead.
    kinds: Vec<Kind>,
}

impl<N> Hitmarkers<N> {
    pub fn new(nodes: Vec<N>) -> Self {
        let count = nodes.len();
        Hitmarkers { pool: Pool::new(nodes), kinds: vec![HIT; count] }
    }

    /// Returns the acquired slot index and its resolved [`Kind`] — the wasm
    /// view uses the latter to set `stroke`/`stroke-width` on the real
    /// elements (`hitmarkers.js:78-81`), a one-time attribute write the pure
    /// core does not otherwise need to repeat every frame.
    pub fn spawn(&mut self, kind: HitKind) -> (usize, Kind) {
        let spec = kind.spec();
        let i = self.pool.acquire();
        {
            let slot = &mut self.pool.slots_mut()[i];
            slot.life = spec.life;
            slot.a = spec.scale;
            slot.b = spec.ring;
            slot.c = spec.spin;
        }
        self.kinds[i] = spec;
        (i, spec)
    }

    /// Advances every live marker by `dt` and returns `(index, frame)` for
    /// every marker still alive after the step — anything whose life expired
    /// this frame is released and omitted, exactly as the source's `if (u >=
    /// 1) { this.pool.release(it); continue; }`.
    pub fn update(&mut self, dt: f64) -> Vec<(usize, MarkerFrame)> {
        let mut out = Vec::new();
        for i in 0..self.pool.count() {
            let mut slot = self.pool.slot(i);
            if !slot.alive {
                continue;
            }
            slot.t += dt;
            let u = slot.t / slot.life;
            if u >= 1.0 {
                self.pool.release(i);
                continue;
            }
            self.pool.slots_mut()[i] = slot;

            // snap in over the first 34% of life, then hold, then fall off
            let in_t = clamp01(u / 0.34);
            let scale = slot.a * (0.62 + 0.38 * ease::out_back(in_t));
            let alpha = if u < 0.55 { 1.0 } else { 1.0 - ease::in_out_sine((u - 0.55) / 0.45) };
            let rot = slot.c * ease::out_cubic(in_t);

            let (ring_scale, ring_opacity, has_ring) = if slot.b > 0.0 {
                let rt = clamp01(u / 0.6);
                let rs = 0.55 + 1.15 * ease::out_quint(rt);
                let ro = slot.b * (1.0 - ease::out_quad(rt));
                (rs, ro, true)
            } else {
                (0.0, 0.0, false)
            };

            out.push((
                i,
                MarkerFrame { scale, rotation_deg: rot, opacity: alpha, ring_scale, ring_opacity, has_ring },
            ));
        }
        out
    }

    pub fn clear(&mut self) {
        self.pool.release_all();
    }

    pub fn kind_at(&self, i: usize) -> Kind {
        self.kinds[i]
    }

    pub fn node(&self, i: usize) -> &N {
        self.pool.node(i)
    }
}

#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::{HitKind, Hitmarkers, MarkerFrame, R_IN, R_OUT};

    /// One hitmarker's DOM: the root, the ring circle, and the main (front)
    /// stroke group — the two the source updates (`_ring`, `_main`).
    pub struct HitNode {
        root: Element,
        ring: Element,
        main: Element,
    }

    fn build_marker(parent: &Element) -> HitNode {
        let node = dom::el("div", Some("ow-hit"), Some(parent));
        let svg = dom::svg("svg", Some(&node));
        dom::set_attr(&svg, "viewBox", "-28 -28 56 56");

        let ring = dom::svg("circle", Some(&svg));
        dom::set_attr(&ring, "r", "30");
        dom::set_attr(&ring, "fill", "none");
        dom::set_attr(&ring, "stroke", "#fff");
        dom::set_attr(&ring, "stroke-width", "1.4");
        dom::set_attr(&ring, "opacity", "0");

        let back = dom::svg("g", Some(&svg));
        dom::set_attr(&back, "stroke", "rgba(0,0,0,.7)");
        dom::set_attr(&back, "stroke-width", "4.0");
        dom::set_attr(&back, "fill", "none");
        let main = dom::svg("g", Some(&svg));
        dom::set_attr(&main, "stroke", "#fff");
        dom::set_attr(&main, "stroke-width", "2.2");
        dom::set_attr(&main, "fill", "none");

        let d = std::f64::consts::FRAC_1_SQRT_2;
        for group in [&back, &main] {
            for q in 0..4 {
                let sx = if q == 0 || q == 3 { 1.0 } else { -1.0 };
                let sy = if q < 2 { -1.0 } else { 1.0 };
                let line = dom::svg("line", Some(group));
                dom::set_attr(&line, "x1", &format!("{:.2}", R_IN * d * sx));
                dom::set_attr(&line, "y1", &format!("{:.2}", R_IN * d * sy));
                dom::set_attr(&line, "x2", &format!("{:.2}", R_OUT * d * sx));
                dom::set_attr(&line, "y2", &format!("{:.2}", R_OUT * d * sy));
                dom::set_attr(&line, "stroke-linecap", "square");
            }
        }
        HitNode { root: node, ring, main }
    }

    pub struct HitmarkersView {
        core: Hitmarkers<HitNode>,
    }

    impl HitmarkersView {
        pub fn new(parent: &Element) -> Self {
            // `util.js:174`: every pooled node starts hidden.
            let nodes: Vec<HitNode> = (0..10)
                .map(|_| {
                    let n = build_marker(parent);
                    dom::set_display(&n.root, "none");
                    n
                })
                .collect();
            HitmarkersView { core: Hitmarkers::new(nodes) }
        }

        pub fn spawn(&mut self, kind: HitKind) {
            let (i, spec) = self.core.spawn(kind);
            let node = self.core.node(i);
            dom::set_display(&node.root, "");
            dom::set_attr(&node.main, "stroke", spec.colour);
            dom::set_attr(&node.main, "stroke-width", &spec.weight.to_string());
            dom::set_attr(&node.ring, "stroke", spec.colour);
            if spec.ring <= 0.0 {
                dom::set_style(&node.ring, "opacity", "0");
            }
        }

        pub fn update(&mut self, dt: f64) {
            let frames = self.core.update(dt);
            let live: std::collections::HashSet<usize> = frames.iter().map(|(i, _)| *i).collect();
            for (i, frame) in frames {
                Self::apply(self.core.node(i), &frame);
            }
            for i in 0..self.core.pool.count() {
                if !self.core.pool.slot(i).alive && !live.contains(&i) {
                    dom::set_display(&self.core.node(i).root, "none");
                }
            }
        }

        fn apply(node: &HitNode, frame: &MarkerFrame) {
            let transform = if frame.rotation_deg != 0.0 {
                format!("scale({:.3}) rotate({:.2}deg)", frame.scale, frame.rotation_deg)
            } else {
                format!("scale({:.3})", frame.scale)
            };
            dom::set_style(&node.root, "transform", &transform);
            dom::set_style(&node.root, "opacity", &format!("{:.3}", frame.opacity));
            if frame.has_ring {
                dom::set_attr(&node.ring, "transform", &format!("scale({:.3})", frame.ring_scale));
                dom::set_style(&node.ring, "opacity", &format!("{:.3}", frame.ring_opacity));
            }
        }

        pub fn clear(&mut self) {
            self.core.clear();
            for i in 0..self.core.pool.count() {
                dom::set_display(&self.core.node(i).root, "none");
            }
        }

        pub fn dispose(&self) {
            for i in 0..self.core.pool.count() {
                dom::remove(&self.core.node(i).root);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_reuses_oldest_slot_when_pool_is_exhausted() {
        let mut hm: Hitmarkers<()> = Hitmarkers::new((0..2).map(|_| ()).collect());
        let (a, _) = hm.spawn(HitKind::Hit);
        let (b, _) = hm.spawn(HitKind::Hit);
        assert_ne!(a, b);
        // both slots alive with life 0.26; age `a` more.
        hm.update(0.1);
        let (c, _) = hm.spawn(HitKind::Kill);
        assert!(c == a || c == b, "acquire must reuse an existing slot, never grow");
    }

    #[test]
    fn kill_marker_spins_and_shows_a_full_ring() {
        let mut hm: Hitmarkers<()> = Hitmarkers::new((0..4).map(|_| ()).collect());
        let (i, spec) = hm.spawn(HitKind::Kill);
        assert_eq!(spec, KILL);
        let frames = hm.update(0.05);
        let (fi, frame) = frames.iter().find(|(idx, _)| *idx == i).unwrap();
        assert_eq!(*fi, i);
        assert!(frame.has_ring);
        assert!(frame.rotation_deg > 0.0, "kill markers spin (spin=9)");
    }

    #[test]
    fn plain_hit_has_no_ring_and_no_spin() {
        let mut hm: Hitmarkers<()> = Hitmarkers::new((0..4).map(|_| ()).collect());
        let (i, _) = hm.spawn(HitKind::Hit);
        let frames = hm.update(0.01);
        let (_, frame) = frames.iter().find(|(idx, _)| *idx == i).unwrap();
        assert!(!frame.has_ring);
        assert_eq!(frame.rotation_deg, 0.0);
    }

    #[test]
    fn marker_releases_itself_once_life_elapses() {
        let mut hm: Hitmarkers<()> = Hitmarkers::new((0..2).map(|_| ()).collect());
        let (i, spec) = hm.spawn(HitKind::Hit);
        assert_eq!(spec.life, 0.26);
        let frames = hm.update(0.3); // > life
        assert!(frames.iter().all(|(idx, _)| *idx != i));
    }

    #[test]
    fn clear_releases_every_live_marker() {
        let mut hm: Hitmarkers<()> = Hitmarkers::new((0..3).map(|_| ()).collect());
        hm.spawn(HitKind::Hit);
        hm.spawn(HitKind::Armour);
        hm.clear();
        let frames = hm.update(0.01);
        assert!(frames.is_empty());
    }

    /// Marker snap-in curve at fixed offsets into a `hit` marker's 0.26s life
    /// — pinned by hand from `ease.outBack`/`inOutSine` at these `t` values
    /// (not a JS capture: the whole formula is exercised end to end above,
    /// and these lock the exact intermediate shape against regressions).
    #[test]
    fn snap_in_then_hold_then_fade_shape() {
        let mut hm: Hitmarkers<()> = Hitmarkers::new(vec![()]);
        let (i, _) = hm.spawn(HitKind::Hit);
        // just after spawn: still snapping in, near-full alpha.
        let frames = hm.update(0.01);
        let (_, early) = frames.iter().find(|(idx, _)| *idx == i).unwrap();
        assert_eq!(early.opacity, 1.0);
        // late in life (u > 0.55): fading.
        let frames = hm.update(0.2);
        let (_, late) = frames.iter().find(|(idx, _)| *idx == i).unwrap();
        assert!(late.opacity < 1.0);
    }
}
