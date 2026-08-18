//! UI primitives: easing curves, math, formatting, pooling — and, on
//! `wasm32`, the DOM realisers everything else is built from.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/util.js:1-215`.
//!
//! ## The split
//!
//! The source mixes two things in one file: DOM construction (`el`, `svg`,
//! `setText`, `setStyle`, `setClass`, `place`) and pure arithmetic (`ease`,
//! `clamp`/`lerp`/`damp`/`spring`, `pad2`/`shortNum`/`metres`/`mmss`/`cardinal`,
//! `Pool`). The port keeps the arithmetic native and fully testable, and moves
//! the DOM half behind `#[cfg(target_arch = "wasm32")]` in [`dom`] — the same
//! split `src/audio/` uses between its recorded [`graph`](crate::audio::graph)
//! and [`web_audio`](crate::audio::web_audio) realiser.
//!
//! [`Pool`] is the one structure that straddles the line: *which* slot is
//! reused (oldest-first, so a burst never starves) is pure bookkeeping over
//! [`PoolSlot`] records, kept here; *what* a slot's node looks like is generic
//! over `N`, so a native test can instantiate `Pool<()>` and a wasm widget can
//! instantiate `Pool<SomeNodeBundle>` without duplicating the reuse policy.
//!
//! Rules the source states and the port preserves:
//!  - No per-frame allocation in the reuse path. `Pool::acquire` never grows.
//!  - No `Math.random()`. Anything random comes from an [`crate::rng::Rng`]
//!    fork passed in (see [`super::markers`]).
//!  - No CSS keyframe animation on gameplay feedback: every animated value is
//!    integrated from `dt` in these pure functions, which is what makes the
//!    capture harness deterministic and lets the whole HUD freeze correctly on
//!    pause.

/// Condensed system stacks — no webfonts, crisp at any size.
///
/// Verified against the capture browser with `node src/ui/preview.mjs --fonts`:
/// "Avenir Next Condensed" (four weights) carries the body text, "DIN
/// Condensed" (very narrow, bold only) carries display numerals, and both
/// degrade through Arial Narrow / Helvetica Neue on machines without them.
pub const FONT_STACK: &str = "\"Avenir Next Condensed\",\"DIN Alternate\",\"Roboto Condensed\",\"Arial Narrow\",\"Helvetica Neue\",Inter,system-ui,-apple-system,sans-serif";

/// Display face: the ammo count, banners, the menu title.
pub const FONT_DISPLAY: &str = "\"DIN Condensed\",\"Avenir Next Condensed\",\"Oswald\",\"Arial Narrow\",\"Helvetica Neue\",Impact,system-ui,sans-serif";

pub const FONT_MONO: &str = "\"SF Mono\",ui-monospace,\"Roboto Mono\",Menlo,monospace";

/* --------------------------------------------------------------- easing --- */

/// `util.js:81-104`'s `ease` object, one function per curve.
pub mod ease {
    use std::f64::consts::PI;

    pub fn linear(t: f64) -> f64 {
        t
    }

    pub fn in_quad(t: f64) -> f64 {
        t * t
    }

    pub fn out_quad(t: f64) -> f64 {
        t * (2.0 - t)
    }

    pub fn out_cubic(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(3)
    }

    pub fn in_out_cubic(t: f64) -> f64 {
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        }
    }

    pub fn out_quint(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(5)
    }

    pub fn out_expo(t: f64) -> f64 {
        if t >= 1.0 {
            1.0
        } else {
            1.0 - 2f64.powf(-10.0 * t)
        }
    }

    pub fn in_out_sine(t: f64) -> f64 {
        -((PI * t).cos() - 1.0) / 2.0
    }

    /// Overshoot then settle — hitmarker / banner punch.
    pub fn out_back(t: f64) -> f64 {
        let c = 1.9;
        let u = t - 1.0;
        1.0 + (c + 1.0) * u * u * u + c * u * u
    }

    /// Damped oscillation, k = number of bounces.
    pub fn out_elastic(t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        1.0 - 2f64.powf(-9.0 * t) * (t * 22.0).cos()
    }

    /// Fast attack, slow release — good for anything that must feel "snappy".
    pub fn punch(t: f64) -> f64 {
        if t < 0.18 {
            out_quint(t / 0.18)
        } else {
            1.0 - in_out_sine((t - 0.18) / 0.82)
        }
    }
}

/* ----------------------------------------------------------------- math --- */

pub fn clamp(v: f64, a: f64, b: f64) -> f64 {
    v.max(a).min(b)
}

pub fn clamp01(v: f64) -> f64 {
    clamp(v, 0.0, 1.0)
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn inv_lerp(a: f64, b: f64, v: f64) -> f64 {
    let denom = b - a;
    // `b - a || 1` in the source: JS `||` falls through on `0` (and `-0`,
    // and `NaN`), so a degenerate range divides by 1 instead of by zero.
    clamp01((v - a) / if denom == 0.0 { 1.0 } else { denom })
}

pub fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Framerate-independent exponential approach. `rate` = 1/e per second.
pub fn damp(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    target + (current - target) * (-rate * dt).exp()
}

/// Critically-damped spring step. Mirrors `spring(current, target, holder,
/// stiffness, damping, dt)` in the source, where `holder` is a `{ v }` object
/// mutated in place; here `holder` is `&mut f64` (the source never reads any
/// other field off it).
pub fn spring(current: f64, target: f64, holder: &mut f64, stiffness: f64, damping: f64, dt: f64) -> f64 {
    let a = (target - current) * stiffness - *holder * damping;
    *holder += a * dt;
    current + *holder * dt
}

pub const TAU: f64 = std::f64::consts::TAU;

/// Shortest signed angular difference, radians.
pub fn angle_delta(a: f64, b: f64) -> f64 {
    let mut d = (b - a) % TAU;
    if d > std::f64::consts::PI {
        d -= TAU;
    }
    if d < -std::f64::consts::PI {
        d += TAU;
    }
    d
}

/* ------------------------------------------------------------- format --- */

fn pad2(n: i64) -> String {
    if n < 10 {
        format!("0{n}")
    } else {
        n.to_string()
    }
}

/// 1834 -> "1.8k", 240 -> "240"
pub fn short_num(n: f64) -> String {
    if n >= 1000.0 {
        format!("{:.1}k", n / 1000.0)
    } else {
        // `n | 0` — ToInt32 truncation toward zero.
        format!("{}", n.trunc() as i64)
    }
}

/// Distance readout: <10m one decimal, else integer.
pub fn metres(d: f64) -> String {
    if d < 10.0 {
        format!("{d:.1}M")
    } else {
        format!("{}M", d.trunc() as i64)
    }
}

pub fn mmss(seconds: f64) -> String {
    let s = (seconds.max(0.0).trunc() as i64).max(0);
    format!("{}:{}", s / 60, pad2(s % 60))
}

const CARDINAL: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

pub fn cardinal(deg: f64) -> &'static str {
    let normalised = ((deg % 360.0) + 360.0) % 360.0;
    let idx = ((normalised / 45.0).round() as i64).rem_euclid(8) as usize;
    CARDINAL[idx]
}

/* ------------------------------------------------------------------ pool --- */

/// One [`Pool`] slot's animation bookkeeping — the source's `{ alive, t, life,
/// i, a, b, c, d, s }` record, minus `i` (redundant with the slot's own index)
/// and `d`/`s`, which no widget outside `minimap.js` reads (`minimap.js` is
/// out of scope for this port — see the crate root docs).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoolSlot {
    pub alive: bool,
    pub t: f64,
    pub life: f64,
    /// Per-widget scratch: spawn scale, drift direction, intensity, ... —
    /// whatever the owning widget's `spawn()` stashed.
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

/// Fixed-size slot pool. Nothing is allocated after construction: `acquire`
/// only ever flips `alive` on an existing slot, oldest-first so a burst never
/// starves. `N` is the node payload a wasm view attaches per slot (a bundle of
/// `web_sys::Element`s); native tests use `Pool<()>` and never touch it.
///
/// Ported from `util.js:168-215`'s `Pool` class.
pub struct Pool<N> {
    slots: Vec<PoolSlot>,
    nodes: Vec<N>,
    next: usize,
}

impl<N> Pool<N> {
    /// `new Pool(count, make, parent)`, split: `nodes` is the already-built
    /// per-slot payload (the source's `make(i)` result), one per slot.
    pub fn new(nodes: Vec<N>) -> Self {
        let slots = vec![PoolSlot::default(); nodes.len()];
        Pool { slots, nodes, next: 0 }
    }

    pub fn count(&self) -> usize {
        self.slots.len()
    }

    pub fn slots(&self) -> &[PoolSlot] {
        &self.slots
    }

    pub fn slot(&self, i: usize) -> PoolSlot {
        self.slots[i]
    }

    pub fn slots_mut(&mut self) -> &mut [PoolSlot] {
        &mut self.slots
    }

    pub fn node(&self, i: usize) -> &N {
        &self.nodes[i]
    }

    pub fn node_mut(&mut self, i: usize) -> &mut N {
        &mut self.nodes[i]
    }

    /// Oldest-first reuse. Returns the acquired slot's index; the caller (a
    /// wasm view) is responsible for the DOM-visible `display: ''` the source
    /// sets inline — that is exactly the DOM-write half this type does not do.
    pub fn acquire(&mut self) -> usize {
        let count = self.count();
        let mut best: Option<usize> = None;
        let mut best_age = f64::NEG_INFINITY;
        for step in 0..count {
            let i = (self.next + step) % count;
            let it = self.slots[i];
            if !it.alive {
                self.next = (i + 1) % count;
                self.slots[i].alive = true;
                self.slots[i].t = 0.0;
                return i;
            }
            let age = it.t / if it.life == 0.0 { 1.0 } else { it.life };
            if age > best_age {
                best_age = age;
                best = Some(i);
            }
        }
        let i = best.expect("count > 0 implies at least one candidate");
        self.slots[i].alive = true;
        self.slots[i].t = 0.0;
        i
    }

    pub fn release(&mut self, i: usize) {
        self.slots[i].alive = false;
    }

    pub fn release_all(&mut self) {
        for i in 0..self.slots.len() {
            self.slots[i].alive = false;
        }
    }
}

/* -------------------------------------------------------------------- dom --- */

/// The DOM binding — `wasm32` only. Every other module in [`super`] computes
/// plain numbers and pre-formatted strings; this is the only place that
/// touches `web_sys`.
#[cfg(target_arch = "wasm32")]
pub mod dom {
    use wasm_bindgen::JsCast;
    use web_sys::{Document, Element};

    fn document() -> Document {
        web_sys::window()
            .expect("no window")
            .document()
            .expect("no document")
    }

    /// `el(tag, cls, parent, text)`.
    pub fn el(tag: &str, class: Option<&str>, parent: Option<&Element>) -> Element {
        let n = document().create_element(tag).expect("create_element");
        if let Some(c) = class {
            n.set_class_name(c);
        }
        if let Some(p) = parent {
            p.append_child(&n).expect("append_child");
        }
        n
    }

    pub fn set_text(node: &Element, text: &str) {
        node.set_text_content(Some(text));
    }

    /// `svg(tag, attrs, parent)`.
    pub fn svg(tag: &str, parent: Option<&Element>) -> Element {
        let n = document()
            .create_element_ns(Some("http://www.w3.org/2000/svg"), tag)
            .expect("create_element_ns");
        if let Some(p) = parent {
            p.append_child(&n).expect("append_child");
        }
        n
    }

    pub fn set_attr(node: &Element, name: &str, value: &str) {
        node.set_attribute(name, value).expect("set_attribute");
    }

    fn html(node: &Element) -> web_sys::HtmlElement {
        node.clone().unchecked_into()
    }

    /// `setStyle(node, prop, value)` — the source's change-only write, kept:
    /// every call already goes through a computed value, and
    /// `CSSStyleDeclaration.setProperty` is not free at 60fps across a whole
    /// HUD, so callers still pass the previous value in and compare before
    /// calling this.
    pub fn set_style(node: &Element, prop: &str, value: &str) {
        html(node)
            .style()
            .set_property(prop, value)
            .expect("set_property");
    }

    pub fn set_display(node: &Element, value: &str) {
        set_style(node, "display", value);
    }

    /// `node.style.cssText = "..."` — a handful of one-off elements
    /// ([`super::super::prompts`]'s hold-progress bar) are styled with a
    /// single inline `style` attribute in the source rather than
    /// `setProperty` calls; `CssStyleDeclaration::set_property` cannot
    /// express that (it sets one named property, and `"cssText"` is not a
    /// real property), so this is `CSSStyleDeclaration.cssText`'s real
    /// setter.
    pub fn set_css_text(node: &Element, value: &str) {
        html(node).style().set_css_text(value);
    }

    pub fn set_class(node: &Element, class: &str, on: bool) {
        let list = node.class_list();
        if on {
            let _ = list.add_1(class);
        } else {
            let _ = list.remove_1(class);
        }
    }

    /// `place(node, transform, opacity)`.
    pub fn place(node: &Element, transform: &str, opacity: Option<f64>) {
        set_style(node, "transform", transform);
        if let Some(o) = opacity {
            let text = if o < 0.001 {
                "0".to_string()
            } else {
                format!("{o:.3}")
            };
            set_style(node, "opacity", &text);
        }
    }

    pub fn remove(node: &Element) {
        node.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_endpoints_are_identity() {
        for f in [
            ease::linear,
            ease::in_quad,
            ease::out_quad,
            ease::out_cubic,
            ease::in_out_cubic,
            ease::out_quint,
            ease::out_expo,
            ease::in_out_sine,
            ease::out_back,
            ease::punch,
        ] {
            assert_eq!(f(0.0), 0.0, "f(0) must be 0");
        }
    }

    #[test]
    fn out_elastic_clamps_domain() {
        assert_eq!(ease::out_elastic(-1.0), 0.0);
        assert_eq!(ease::out_elastic(2.0), 1.0);
    }

    #[test]
    fn clamp_and_lerp() {
        assert_eq!(clamp(5.0, 0.0, 3.0), 3.0);
        assert_eq!(clamp(-5.0, 0.0, 3.0), 0.0);
        assert_eq!(clamp01(1.5), 1.0);
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
    }

    #[test]
    fn inv_lerp_degenerate_range_divides_by_one() {
        assert_eq!(inv_lerp(5.0, 5.0, 5.0), 0.0);
    }

    #[test]
    fn damp_reaches_target_at_infinite_rate() {
        assert!((damp(0.0, 10.0, 1000.0, 1.0) - 10.0).abs() < 1e-6);
        assert_eq!(damp(3.0, 3.0, 5.0, 0.5), 3.0);
    }

    #[test]
    fn spring_accelerates_toward_target() {
        let mut v = 0.0;
        let next = spring(0.0, 10.0, &mut v, 150.0, 15.0, 1.0 / 60.0);
        assert!(next > 0.0);
        assert!(v > 0.0);
    }

    #[test]
    fn angle_delta_shortest_path() {
        assert!((angle_delta(0.0, TAU - 0.1) - (-0.1)).abs() < 1e-9);
        assert!((angle_delta(0.0, 0.1) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn short_num_thousands() {
        assert_eq!(short_num(1834.0), "1.8k");
        assert_eq!(short_num(240.0), "240");
    }

    #[test]
    fn metres_precision_boundary() {
        assert_eq!(metres(9.96), "10.0M");
        assert_eq!(metres(10.0), "10M");
        assert_eq!(metres(4.2), "4.2M");
    }

    #[test]
    fn mmss_formats() {
        assert_eq!(mmss(0.0), "0:00");
        assert_eq!(mmss(-5.0), "0:00");
        assert_eq!(mmss(72.9), "1:12");
    }

    #[test]
    fn cardinal_directions_wrap() {
        assert_eq!(cardinal(0.0), "N");
        assert_eq!(cardinal(44.0), "NE");
        assert_eq!(cardinal(-1.0), "N");
        assert_eq!(cardinal(360.0), "N");
    }

    #[test]
    fn pool_acquire_is_oldest_first_and_never_grows() {
        let mut pool: Pool<()> = Pool::new(vec![(), (), ()]);
        let a = pool.acquire();
        let b = pool.acquire();
        let c = pool.acquire();
        assert_eq!([a, b, c], [0, 1, 2]);
        assert_eq!(pool.count(), 3);

        // all alive; age the first two so acquire() must reuse index 0.
        pool.slots[0].t = 5.0;
        pool.slots[1].t = 1.0;
        pool.slots[2].t = 3.0;
        for i in 0..3 {
            pool.slots[i].life = 1.0;
        }
        let reused = pool.acquire();
        assert_eq!(reused, 0);
        assert_eq!(pool.slot(0).t, 0.0);
    }

    #[test]
    fn pool_release_frees_a_slot_for_reuse() {
        let mut pool: Pool<()> = Pool::new(vec![(), ()]);
        let a = pool.acquire();
        let _b = pool.acquire();
        pool.release(a);
        assert!(!pool.slot(a).alive);
        let reused = pool.acquire();
        assert_eq!(reused, a);
    }

    #[test]
    fn pool_release_all_clears_every_slot() {
        let mut pool: Pool<()> = Pool::new(vec![(), (), ()]);
        pool.acquire();
        pool.acquire();
        pool.release_all();
        assert!(pool.slots().iter().all(|s| !s.alive));
    }
}
