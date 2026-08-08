//! Where everything sits on the screen, derived from the viewport.
//!
//! Mobile portrait is the case this is designed for, not the case it degrades
//! to. Three bands stack down the screen and each one is sized as a fraction of
//! the viewport, so a 320×568 phone and a 412×915 phone get the same layout at
//! different sizes rather than one of them getting a squeezed version of the
//! other:
//!
//! ```text
//!  ┌───────────────┐
//!  │   the goal    │  ← the whole upper band is the aim pad
//!  │               │
//!  ├───────────────┤
//!  │  sculpt panel │  ← the whole panel is the handle
//!  ├───────────────┤
//!  │ back │  next  │  ← one thumb-sized action row
//!  └───────────────┘
//! ```
//!
//! There is deliberately nothing in a corner and nothing small. The two things
//! the player touches are a band and a panel, both of them enormous, because the
//! trajectory — not a widget — is supposed to be the interface.

use axiom::prelude::Vec2;

use crate::tuning::EditorTuning;

/// An axis-aligned screen rectangle, physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, p: Vec2) -> bool {
        (p.x >= self.x) & (p.x <= self.x + self.w) & (p.y >= self.y) & (p.y <= self.y + self.h)
    }

    pub fn centre(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// Clamp a point into the rectangle — how a drag that wanders off the panel
    /// keeps controlling the curve instead of being dropped.
    pub fn clamp(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            p.x.clamp(self.x, self.x + self.w),
            p.y.clamp(self.y, self.y + self.h),
        )
    }

    /// Where `p` sits inside the rectangle, as `0..1` on each axis.
    pub fn normalized(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            ((p.x - self.x) / self.w.max(1.0)).clamp(0.0, 1.0),
            ((p.y - self.y) / self.h.max(1.0)).clamp(0.0, 1.0),
        )
    }
}

/// The resolved screen layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub viewport: Vec2,
    /// The viewport's short edge — the unit every size is expressed in, so the
    /// interface scales with the device rather than with one axis of it.
    pub short: f32,
    pub portrait: bool,
    /// The band a touch is read as an aim.
    pub aim_pad: Rect,
    /// The sculpt panel. The *whole* rectangle is the handle.
    pub panel: Rect,
    /// The primary action.
    pub action: Rect,
    /// The step-back chip (only drawn when there is somewhere to go back to).
    pub back: Rect,
}

impl Layout {
    /// Resolve the layout for a surface, in physical pixels.
    pub fn resolve(viewport: Vec2, tuning: &EditorTuning) -> Layout {
        let w = viewport.x.max(1.0);
        let h = viewport.y.max(1.0);
        let short = w.min(h);
        let portrait = h >= w;

        // In portrait the panel spans nearly the full width. In landscape the
        // screen is wide and the goal is small in the middle, so the panel takes
        // a centred column instead of stretching into an unusable letterbox.
        let panel_w = [w * 0.52, w * (1.0 - tuning.panel_inset * 2.0)][usize::from(portrait)];
        let panel_x = (w - panel_w) * 0.5;
        let panel_top = h * [0.50, tuning.panel_top][usize::from(portrait)];
        let panel_bottom = h * [0.78, tuning.panel_bottom][usize::from(portrait)];
        let panel = Rect::new(panel_x, panel_top, panel_w, panel_bottom - panel_top);

        let action_h = (h * tuning.action_height).clamp(short * 0.11, short * 0.20);
        let gap = short * 0.022;
        let action_y = (panel.y + panel.h + gap).min(h - action_h - gap);
        // The back chip takes a third of the row; the primary action takes the
        // rest. Both are at least a thumb wide, and neither is in a corner.
        let back_w = panel_w * 0.30;
        let back = Rect::new(panel_x, action_y, back_w, action_h);
        let action = Rect::new(
            panel_x + back_w + gap,
            action_y,
            panel_w - back_w - gap,
            action_h,
        );

        Layout {
            viewport: Vec2::new(w, h),
            short,
            portrait,
            aim_pad: Rect::new(0.0, 0.0, w, (panel.y - gap).max(0.0)),
            panel,
            action,
            back,
        }
    }

    /// The primary action when there is nothing to go back to: it takes the
    /// whole row, because a disabled chip is a thing to be tapped by mistake.
    pub fn wide_action(&self) -> Rect {
        Rect::new(self.back.x, self.action.y, self.panel.w, self.action.h)
    }

    /// A size in short-edge fractions, as pixels.
    pub fn scaled(&self, fraction: f32) -> f32 {
        self.short * fraction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    fn layout(w: f32, h: f32) -> Layout {
        Layout::resolve(Vec2::new(w, h), &Tuning::DEFAULT.editor)
    }

    #[test]
    fn every_band_fits_on_every_phone_and_none_of_them_overlap() {
        for (w, h) in [
            (320.0f32, 568.0f32),
            (360.0, 800.0),
            (390.0, 844.0),
            (412.0, 915.0),
            (1440.0, 900.0),
        ] {
            let l = layout(w, h);
            let bands = [l.aim_pad, l.panel, l.action];
            bands.iter().for_each(|r| {
                assert!(r.x >= -0.5 && r.x + r.w <= w + 0.5, "{w}x{h}: {r:?} off side");
                assert!(r.y >= -0.5 && r.y + r.h <= h + 0.5, "{w}x{h}: {r:?} off end");
                assert!(r.w > 0.0 && r.h > 0.0);
            });
            assert!(l.aim_pad.y + l.aim_pad.h <= l.panel.y, "{w}x{h}: bands overlap");
            assert!(l.panel.y + l.panel.h <= l.action.y, "{w}x{h}: bands overlap");
            assert_eq!(l.back.y, l.action.y);
            assert!(l.back.x + l.back.w < l.action.x, "{w}x{h}: the chips collide");
        }
    }

    #[test]
    fn nothing_the_player_touches_is_small() {
        // A 7 mm thumb target on the smallest supported phone is ~44 CSS px; the
        // panel and the action row are far larger than that on every device.
        for (w, h) in [(320.0f32, 568.0f32), (390.0, 844.0), (412.0, 915.0)] {
            let l = layout(w, h);
            assert!(l.action.h >= 44.0, "{w}x{h}: action row is {}px", l.action.h);
            assert!(l.back.w >= 44.0, "{w}x{h}: back chip is {}px", l.back.w);
            assert!(
                l.panel.h >= l.short * 0.25,
                "{w}x{h}: the panel is only {}px tall",
                l.panel.h
            );
            assert!(l.panel.w >= w * 0.8, "{w}x{h}: the panel is narrow");
            assert!(
                l.aim_pad.h >= h * 0.4,
                "{w}x{h}: the aim pad is only {}px",
                l.aim_pad.h
            );
        }
    }

    #[test]
    fn landscape_keeps_the_panel_a_centred_column() {
        let l = layout(1440.0, 900.0);
        assert!(!l.portrait);
        assert!(l.panel.w < 1440.0 * 0.7, "it does not stretch to a letterbox");
        assert!((l.panel.centre().x - 720.0).abs() < 1.0, "it stays centred");
        assert!(layout(390.0, 844.0).portrait);
    }

    #[test]
    fn a_rectangle_hit_tests_clamps_and_normalizes() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(Vec2::new(60.0, 45.0)));
        assert!(!r.contains(Vec2::new(5.0, 45.0)));
        assert!(!r.contains(Vec2::new(60.0, 500.0)));
        assert_eq!(r.centre(), Vec2::new(60.0, 45.0));
        assert_eq!(r.clamp(Vec2::new(-90.0, 900.0)), Vec2::new(10.0, 70.0));
        assert_eq!(r.normalized(Vec2::new(60.0, 45.0)), Vec2::new(0.5, 0.5));
        assert_eq!(r.normalized(Vec2::new(-99.0, -99.0)), Vec2::ZERO);
        // A degenerate rectangle normalizes without dividing by zero.
        assert!(Rect::new(0.0, 0.0, 0.0, 0.0)
            .normalized(Vec2::ONE)
            .x
            .is_finite());
    }

    #[test]
    fn the_wide_action_spans_the_whole_row() {
        let l = layout(390.0, 844.0);
        let wide = l.wide_action();
        assert_eq!(wide.x, l.back.x);
        assert_eq!(wide.w, l.panel.w);
        assert_eq!(wide.h, l.action.h);
        assert!(l.scaled(0.5) > 0.0);
    }
}
