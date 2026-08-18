//! Dynamic four-blade reticle.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/ui/crosshair.js:1-104`.
//!
//! Spread model (all in HUD pixels at k=1):
//! `gap = base + move*MOVE + fire kick + flinch`. The kick is a spring so a
//! burst punches the blades out and they settle back with a little overshoot
//! instead of linearly interpolating — that overshoot is most of what makes
//! firing feel mechanical rather than animated.
//!
//! ADS hides the whole reticle over 70ms (the optic reticle is the weapon's
//! job).
//!
//! [`Crosshair`] is the pure state + `update()` math (every field the source's
//! constructor sets, and the per-frame formulas that turn them into blade
//! transforms). [`view::CrosshairView`] (`wasm32` only) owns the four blade
//! elements and the dot, and just writes [`CrosshairFrame`] into `style`.

use super::util::{clamp, clamp01, damp, ease};

/// Everything [`Crosshair::update`] needs beyond its own state — the source's
/// per-call `s` object (`crosshair.js:50-52`).
#[derive(Debug, Clone, Copy, Default)]
pub struct CrosshairInput {
    pub move_amount: f64,
    pub sprint: bool,
    pub crouch: bool,
    pub airborne: bool,
    pub ads: bool,
    /// `s.baseSpread`; the source defaults this to `5.5` when absent.
    pub base_spread: Option<f64>,
    pub hidden: bool,
}

/// One blade's per-frame transform (`rotate(Ndeg) translateY(-gapPx)
/// scaleY(len)`) plus its opacity — everything [`view`] needs to write, with
/// the `.toFixed()` rounding already applied so the wasm layer only ever
/// formats what the source formats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BladeFrame {
    pub rotation_deg: f64,
    pub gap_px: f64,
    pub scale_y: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrosshairFrame {
    pub blades: [BladeFrame; 4],
    pub dot_scale: f64,
    pub dot_opacity: f64,
    /// `true` once `vis < 0.004` — the source's `display: 'none'` gate.
    pub hidden: bool,
}

/// `crosshair.js:14-104`'s `Crosshair` class, minus its DOM handles.
#[derive(Debug, Clone, Copy)]
pub struct Crosshair {
    pub k: f64,
    kick: f64,
    kick_vel: f64,
    move_spread: f64,
    ads_blend: f64,
    hit_pulse: f64,
    visible: f64,
}

impl Default for Crosshair {
    fn default() -> Self {
        Crosshair {
            k: 1.0,
            kick: 0.0,
            kick_vel: 0.0,
            move_spread: 0.0,
            ads_blend: 0.0,
            hit_pulse: 0.0,
            visible: 1.0,
        }
    }
}

const ROT: [f64; 4] = [0.0, 90.0, 180.0, 270.0];

impl Crosshair {
    pub fn new() -> Self {
        Crosshair::default()
    }

    /// Called on every shot. `amount` scales with weapon recoil.
    pub fn on_fire(&mut self, amount: f64) {
        self.kick_vel += 78.0 * amount;
        self.kick = (self.kick + 1.2 * amount).min(16.0);
    }

    /// Taking damage nudges the reticle — reads as flinch.
    pub fn on_flinch(&mut self, amount: f64) {
        self.kick_vel += 30.0 * amount;
    }

    pub fn on_hit(&mut self) {
        self.hit_pulse = 1.0;
    }

    pub fn set_scale(&mut self, k: f64) {
        self.k = k;
    }

    pub fn update(&mut self, dt: f64, s: CrosshairInput) -> CrosshairFrame {
        // --- spring kick -------------------------------------------------------
        let stiff = 150.0;
        let damp_c = 15.0;
        self.kick_vel += (0.0 - self.kick) * stiff * dt - self.kick_vel * damp_c * dt;
        self.kick += self.kick_vel * dt;
        if self.kick < 0.0 {
            self.kick = 0.0;
            if self.kick_vel < 0.0 {
                self.kick_vel *= 0.4;
            }
        }

        // --- movement / stance bloom ------------------------------------------
        let target = s.move_amount * 7.0
            + if s.sprint { 6.0 } else { 0.0 }
            - if s.crouch { 1.6 } else { 0.0 }
            + if s.airborne { 5.0 } else { 0.0 };
        self.move_spread = damp(self.move_spread, target, 9.0, dt);

        self.ads_blend = damp(self.ads_blend, if s.ads { 1.0 } else { 0.0 }, 16.0, dt);
        self.hit_pulse = (self.hit_pulse - dt * 5.5).max(0.0);

        let base = s.base_spread.unwrap_or(5.5) - self.ads_blend * 2.0;
        let gap = (base + self.move_spread + self.kick) * self.k;
        // blades grow a touch as they spread — keeps the mass of the reticle even
        let len = clamp(1.0 + self.move_spread * 0.035 + self.kick * 0.05, 1.0, 1.7);

        let fade = clamp01(1.0 - self.ads_blend * 1.25) * if s.hidden { 0.0 } else { 1.0 };
        self.visible = damp(self.visible, fade, 22.0, dt);
        let vis = self.visible;

        let bright = 1.0 - 0.25 * self.ads_blend + 0.5 * ease::out_quad(self.hit_pulse);
        let mut blades = [BladeFrame { rotation_deg: 0.0, gap_px: 0.0, scale_y: 0.0, opacity: 0.0 }; 4];
        for (i, rot) in ROT.iter().enumerate() {
            blades[i] = BladeFrame {
                rotation_deg: *rot,
                gap_px: gap,
                scale_y: len,
                opacity: vis * bright.min(1.0),
            };
        }

        let dot_scale = 1.0 + self.hit_pulse * 1.1;
        CrosshairFrame {
            blades,
            dot_scale,
            dot_opacity: vis * (0.85 + 0.15 * self.hit_pulse),
            hidden: vis < 0.004,
        }
    }
}

/// `wasm32`-only DOM binding: builds the four blades + dot, and writes each
/// [`CrosshairFrame`] into `style` with the same `.toFixed()` precision the
/// source uses (`crosshair.js:84-94`).
#[cfg(target_arch = "wasm32")]
pub mod view {
    use web_sys::Element;

    use super::super::util::dom;
    use super::CrosshairFrame;

    pub struct CrosshairView {
        pub root: Element,
        blades: [Element; 4],
        dot: Element,
    }

    impl CrosshairView {
        pub fn new(parent: &Element) -> Self {
            let root = dom::el("div", Some("ow-cross"), Some(parent));
            let blades = [
                dom::el("div", Some("ow-blade"), Some(&root)),
                dom::el("div", Some("ow-blade"), Some(&root)),
                dom::el("div", Some("ow-blade"), Some(&root)),
                dom::el("div", Some("ow-blade"), Some(&root)),
            ];
            let dot = dom::el("div", Some("ow-dot"), Some(&root));
            CrosshairView { root, blades, dot }
        }

        pub fn apply(&self, frame: &CrosshairFrame) {
            for (node, b) in self.blades.iter().zip(frame.blades.iter()) {
                dom::set_style(
                    node,
                    "transform",
                    &format!(
                        "rotate({}deg) translateY({:.2}px) scaleY({:.3})",
                        b.rotation_deg as i64,
                        -b.gap_px,
                        b.scale_y
                    ),
                );
                dom::set_style(node, "opacity", &format!("{:.3}", b.opacity));
            }
            dom::set_style(&self.dot, "transform", &format!("scale({:.3})", frame.dot_scale));
            dom::set_style(&self.dot, "opacity", &format!("{:.3}", frame.dot_opacity));
            dom::set_display(&self.root, if frame.hidden { "none" } else { "" });
        }

        pub fn dispose(&self) {
            dom::remove(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> CrosshairInput {
        CrosshairInput::default()
    }

    #[test]
    fn at_rest_all_blades_share_the_base_spread() {
        let mut c = Crosshair::new();
        // settle the damped channels over a few seconds of steady-state input.
        for _ in 0..600 {
            c.update(1.0 / 60.0, input());
        }
        let frame = c.update(1.0 / 60.0, input());
        for b in frame.blades {
            assert!((b.gap_px - 5.5).abs() < 1e-6, "gap should settle at baseSpread=5.5, got {}", b.gap_px);
            assert_eq!(b.scale_y, 1.0);
        }
        assert_eq!(frame.blades.map(|b| b.rotation_deg), [0.0, 90.0, 180.0, 270.0]);
    }

    #[test]
    fn on_fire_kicks_the_gap_outward_next_frame() {
        let mut c = Crosshair::new();
        c.on_fire(1.0);
        let frame = c.update(1.0 / 60.0, input());
        assert!(frame.blades[0].gap_px > 5.5);
    }

    #[test]
    fn ads_fades_the_reticle_toward_invisible() {
        let mut c = Crosshair::new();
        let mut s = input();
        s.ads = true;
        for _ in 0..600 {
            c.update(1.0 / 60.0, s);
        }
        let frame = c.update(1.0 / 60.0, s);
        assert!(frame.hidden, "fully ADS'd reticle should be hidden");
    }

    #[test]
    fn hidden_flag_forces_zero_visibility_even_without_ads() {
        let mut c = Crosshair::new();
        let mut s = input();
        s.hidden = true;
        for _ in 0..600 {
            c.update(1.0 / 60.0, s);
        }
        let frame = c.update(1.0 / 60.0, s);
        assert!(frame.hidden);
    }

    #[test]
    fn on_hit_pulses_the_dot_and_decays() {
        let mut c = Crosshair::new();
        c.on_hit();
        let frame = c.update(1.0 / 600.0, input());
        assert!(frame.dot_scale > 1.0);
        for _ in 0..600 {
            c.update(1.0 / 60.0, input());
        }
        let settled = c.update(1.0 / 60.0, input());
        assert!((settled.dot_scale - 1.0).abs() < 1e-6);
    }

    #[test]
    fn kick_never_goes_negative_and_damps_reverse_velocity() {
        let mut c = Crosshair::new();
        c.on_fire(0.01);
        // run long enough for the spring to overshoot back through zero; the
        // gap should never dip below the resting spread, since a negative
        // kick is clamped to zero at the site (`crosshair.js:59-62`).
        for _ in 0..300 {
            let frame = c.update(1.0 / 60.0, input());
            assert!(frame.blades[0].gap_px >= 5.5 - 1e-6);
        }
    }
}
