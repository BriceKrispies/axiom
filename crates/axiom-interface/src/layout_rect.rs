//! [`Rect`] — an integer layout rectangle (pixels), and the clamping used to keep
//! a panel on screen. Branchless integer math, no floats.

/// An integer pixel rectangle: top-left `(x, y)` plus `width`/`height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub(crate) fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Rect { x, y, width, height }
    }

    pub(crate) fn position(self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub(crate) fn with_position(self, x: i32, y: i32) -> Self {
        Rect { x, y, ..self }
    }

    pub(crate) fn with_width(self, width: i32) -> Self {
        Rect { width, ..self }
    }

    /// Clamp the top-left into `[0, max_x] × [0, max_y]`. A negative bound (the
    /// panel is larger than the viewport) clamps up to `0`, pinning the corner.
    pub(crate) fn clamped(self, max_x: i32, max_y: i32) -> Self {
        Rect {
            x: self.x.clamp(0, max_x.max(0)),
            y: self.y.clamp(0, max_y.max(0)),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_and_with_position() {
        let r = Rect::new(8, 8, 360, 0);
        assert_eq!(r.position(), (8, 8));
        assert_eq!(r.with_position(20, 30).position(), (20, 30));
    }

    #[test]
    fn with_width_replaces_only_width() {
        let r = Rect::new(8, 8, 360, 0).with_width(460);
        assert_eq!(r.width, 460);
        assert_eq!(r.position(), (8, 8));
    }

    #[test]
    fn clamp_keeps_within_bounds() {
        let r = Rect::new(500, 400, 360, 0);
        assert_eq!(r.clamped(1000, 800).position(), (500, 400));
        assert_eq!(r.with_position(-50, -50).clamped(1000, 800).position(), (0, 0));
        assert_eq!(r.with_position(5000, 5000).clamped(1000, 800).position(), (1000, 800));
    }

    #[test]
    fn clamp_pins_corner_when_bounds_are_negative() {
        let r = Rect::new(500, 500, 360, 0);
        assert_eq!(r.clamped(-100, -100).position(), (0, 0));
    }
}
