//! Directional damage indicators.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/damage.js:1-100`.
//!
//! A fading arc segment at ~112px radius pointing at the shooter. The
//! fall-off across the arc is built from seven discrete segments on a bell
//! curve rather than an SVG gradient — it reads sharper, matches CoD's
//! stepped look, and costs one path each.
//!
//! The world direction is stored per indicator and the arc is re-oriented
//! every frame, so turning toward the shooter sweeps the arc to the centre.

use super::util::{clamp01, ease, Pool};

pub const SEG: usize = 7;
pub const SEG_STEP: f64 = 8.2; // degrees between segment centres
pub const SEG_ARC: f64 = 8.9; // degrees each segment covers (slight overlap = solid arc)
pub const BELL: [f64; SEG] = [0.1, 0.28, 0.62, 1.0, 0.62, 0.28, 0.1];
pub const R_MAIN: f64 = 112.0;
pub const R_THIN: f64 = 124.0;

/// `pt(deg, r)` — one arc endpoint in SVG-path space, formatted exactly as the
/// source's `"${x} ${y}"` (2 decimals).
pub fn pt(deg: f64, r: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (a.sin() * r, -a.cos() * r)
}

/// `arcPath(cDeg, r)` — the `M x y A r r 0 0 1 x y` path string for one
/// segment centred at `cDeg`.
pub fn arc_path(centre_deg: f64, r: f64) -> String {
    let a0 = centre_deg - SEG_ARC / 2.0;
    let a1 = centre_deg + SEG_ARC / 2.0;
    let (x0, y0) = pt(a0, r);
    let (x1, y1) = pt(a1, r);
    format!("M {x0:.2} {y0:.2} A {r} {r} 0 0 1 {x1:.2} {y1:.2}")
}

/// The seven segment centre angles, offset from 0 — shared by every ring
/// (`main`/`back`/`thin`) at construction time.
pub fn segment_centres() -> [f64; SEG] {
    let mut out = [0.0; SEG];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (i as f64 - (SEG as f64 - 1.0) / 2.0) * SEG_STEP;
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcFrame {
    pub rotation_deg: f64,
    pub scale: f64,
    pub opacity: f64,
}

/// `damage.js:32-100`'s `DamageArcs` class, minus its DOM handles.
pub struct DamageArcs<N> {
    pool: Pool<N>,
    life: f64,
}

impl<N> DamageArcs<N> {
    pub fn new(nodes: Vec<N>) -> Self {
        DamageArcs { pool: Pool::new(nodes), life: 2.0 }
    }

    /// `dx`/`dz`: world XZ direction player -> source (need not be unit).
    /// `intensity`: 0..1, scales opacity and the spawn punch.
    pub fn spawn(&mut self, dx: f64, dz: f64, intensity: f64) -> usize {
        let len = dx.hypot(dz);
        let len = if len == 0.0 { 1.0 } else { len };
        let i = self.pool.acquire();
        let slot = &mut self.pool.slots_mut()[i];
        slot.life = self.life;
        slot.a = dx / len;
        slot.b = dz / len;
        slot.c = clamp01(intensity);
        i
    }

    /// Basis vectors are the camera's right/forward projected to XZ.
    pub fn update(&mut self, dt: f64, rx: f64, rz: f64, fx: f64, fz: f64) -> Vec<(usize, ArcFrame)> {
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

            let deg = (slot.a * rx + slot.b * rz).atan2(slot.a * fx + slot.b * fz).to_degrees();
            // punch in fast, hold, then a long tail — 2s total
            let in_t = clamp01(slot.t / 0.09);
            let scale = 0.92 + 0.08 * ease::out_quint(in_t);
            let hold = clamp01((u - 0.18) / 0.82);
            let opacity = (0.35 + 0.65 * slot.c) * (1.0 - ease::in_quad(hold)) * ease::out_quad(in_t);
            out.push((i, ArcFrame { rotation_deg: deg, scale, opacity }));
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
    use super::{arc_path, segment_centres, ArcFrame, DamageArcs, BELL, R_MAIN, R_THIN};

    fn ring(parent: &Element, stroke: &str, width: f64, radius: f64, opacity_scale: f64) -> Element {
        let g = dom::svg("g", Some(parent));
        dom::set_attr(&g, "fill", "none");
        dom::set_attr(&g, "stroke", stroke);
        dom::set_attr(&g, "stroke-width", &width.to_string());
        for (i, c) in segment_centres().iter().enumerate() {
            let path = dom::svg("path", Some(&g));
            dom::set_attr(&path, "d", &arc_path(*c, radius));
            dom::set_attr(&path, "opacity", &format!("{:.2}", BELL[i] * opacity_scale));
        }
        g
    }

    fn build(parent: &Element) -> Element {
        let node = dom::el("div", Some("ow-dmg"), Some(parent));
        let svg = dom::svg("svg", Some(&node));
        dom::set_attr(&svg, "viewBox", "-170 -170 340 340");
        ring(&svg, "rgba(0,0,0,.5)", 9.5, R_MAIN, 0.9);
        ring(&svg, "#ff3f31", 5.6, R_MAIN, 1.0);
        ring(&svg, "#ff6a52", 1.5, R_THIN, 0.4);
        dom::set_display(&node, "none");
        node
    }

    pub struct DamageArcsView {
        core: DamageArcs<Element>,
    }

    impl DamageArcsView {
        pub fn new(parent: &Element) -> Self {
            let nodes: Vec<Element> = (0..6).map(|_| build(parent)).collect();
            DamageArcsView { core: DamageArcs::new(nodes) }
        }

        pub fn spawn(&mut self, dx: f64, dz: f64, intensity: f64) {
            let i = self.core.spawn(dx, dz, intensity);
            dom::set_display(self.core.node(i), "");
        }

        pub fn update(&mut self, dt: f64, rx: f64, rz: f64, fx: f64, fz: f64) {
            let live: Vec<usize> = self.core.update(dt, rx, rz, fx, fz).into_iter().map(|(i, f)| { Self::apply(self.core.node(i), &f); i }).collect();
            for i in 0..self.core.count() {
                if !self.core.slot(i).alive && !live.contains(&i) {
                    dom::set_display(self.core.node(i), "none");
                }
            }
        }

        fn apply(node: &Element, frame: &ArcFrame) {
            dom::set_style(
                node,
                "transform",
                &format!("rotate({:.2}deg) scale({:.3})", frame.rotation_deg, frame.scale),
            );
            dom::set_style(node, "opacity", &format!("{:.3}", frame.opacity));
        }

        pub fn clear(&mut self) {
            self.core.clear();
            for i in 0..self.core.count() {
                dom::set_display(self.core.node(i), "none");
            }
        }

        pub fn dispose(&self) {
            for i in 0..self.core.count() {
                dom::remove(self.core.node(i));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_matches_hand_computed_trig() {
        let (x, y) = pt(0.0, 100.0);
        assert!((x - 0.0).abs() < 1e-9);
        assert!((y - -100.0).abs() < 1e-9);
        let (x90, y90) = pt(90.0, 100.0);
        assert!((x90 - 100.0).abs() < 1e-9);
        assert!(y90.abs() < 1e-9);
    }

    #[test]
    fn arc_path_has_the_source_svg_shape() {
        let d = arc_path(0.0, 112.0);
        assert!(d.starts_with("M "));
        assert!(d.contains(" A 112 112 0 0 1 "));
    }

    #[test]
    fn segment_centres_are_symmetric_around_zero() {
        let c = segment_centres();
        assert_eq!(c[3], 0.0); // middle of 7
        assert!((c[0] + c[6]).abs() < 1e-9);
    }

    #[test]
    fn spawn_normalises_direction_and_clamps_intensity() {
        let mut arcs: DamageArcs<()> = DamageArcs::new((0..2).map(|_| ()).collect());
        let i = arcs.spawn(3.0, 4.0, 2.0); // len=5, intensity clamps to 1
        let slot = arcs.slot(i);
        assert!((slot.a - 0.6).abs() < 1e-9);
        assert!((slot.b - 0.8).abs() < 1e-9);
        assert_eq!(slot.c, 1.0);
    }

    #[test]
    fn spawn_with_zero_direction_falls_back_to_length_one() {
        let mut arcs: DamageArcs<()> = DamageArcs::new(vec![()]);
        let i = arcs.spawn(0.0, 0.0, 0.5);
        let slot = arcs.slot(i);
        assert_eq!(slot.a, 0.0);
        assert_eq!(slot.b, 0.0);
    }

    #[test]
    fn arc_points_toward_source_via_camera_basis() {
        let mut arcs: DamageArcs<()> = DamageArcs::new(vec![()]);
        // source directly to the player's right (camera-space +X).
        arcs.spawn(1.0, 0.0, 1.0);
        // camera basis: right=(1,0), forward=(0,1) (identity-ish).
        let frames = arcs.update(0.01, 1.0, 0.0, 0.0, 1.0);
        assert_eq!(frames.len(), 1);
        assert!((frames[0].1.rotation_deg - 90.0).abs() < 1e-6);
    }

    #[test]
    fn indicator_expires_after_its_two_second_life() {
        let mut arcs: DamageArcs<()> = DamageArcs::new(vec![()]);
        let i = arcs.spawn(1.0, 0.0, 1.0);
        arcs.update(2.1, 1.0, 0.0, 0.0, 1.0);
        assert!(!arcs.slot(i).alive);
    }
}
