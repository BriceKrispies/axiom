//! Screen-space value types for the immediate-mode UI surface (SPEC-09).
//! Distinct from the retained [`Rect`](crate::layout_rect) used by panels: this
//! family is **float**, top-left origin, `+y` down — the screen-space HUD drawn
//! *after* the world. Colors are packed integers (the layer owns no float color;
//! the app translates the contract's float `FillStroke` onto these primitives).

/// A logical screen-pixel scalar (top-left origin, `+y` down). A float quantity
/// newtype, so a raw `f32` enters/leaves only through `new`/`get`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiUnit(f32);

impl UiUnit {
    /// Wrap a raw screen-pixel value.
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// The raw screen-pixel value.
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A packed 8-bit-per-channel RGBA color. Integer, so the app maps the contract's
/// float color into this primitive — the interface layer owns no float color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiColor(u32);

impl UiColor {
    /// Wrap a packed `0xRRGGBBAA` value.
    pub const fn new(rgba: u32) -> Self {
        Self(rgba)
    }

    /// The packed `0xRRGGBBAA` value.
    pub const fn rgba(self) -> u32 {
        self.0
    }
}

/// A screen-space rectangle: top-left (`x`, `y`) and size (`w`, `h`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiRect {
    /// Left edge.
    pub x: UiUnit,
    /// Top edge.
    pub y: UiUnit,
    /// Width.
    pub w: UiUnit,
    /// Height.
    pub h: UiUnit,
}

impl UiRect {
    /// Construct a rectangle from its top-left corner and size.
    pub const fn new(x: UiUnit, y: UiUnit, w: UiUnit, h: UiUnit) -> Self {
        Self { x, y, w, h }
    }

    /// Whether a pointer position lies inside the rectangle — left/top inclusive,
    /// right/bottom exclusive. Branchless: two half-open range checks combined
    /// with a bitwise `&`.
    pub fn contains(self, px: UiUnit, py: UiUnit) -> bool {
        (self.x.get()..self.x.get() + self.w.get()).contains(&px.get())
            & (self.y.get()..self.y.get() + self.h.get()).contains(&py.get())
    }
}

/// Fill + stroke style for a screen-space shape (primitive fields; the app maps
/// the contract's `FillStroke` onto this).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiFill {
    /// Interior color.
    pub fill: UiColor,
    /// Border color.
    pub stroke: UiColor,
    /// Border width.
    pub stroke_width: UiUnit,
}

/// Logical screen size, fed in per frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiViewport {
    /// Logical width.
    pub width: UiUnit,
    /// Logical height.
    pub height: UiUnit,
}

/// This-frame pointer state for immediate-mode hit-testing. **Presentation
/// input** — sampled per render frame, never the per-tick sim intent stream
/// (SPEC-05); no value here may reach a fixed update.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointerSnapshot {
    /// Pointer x in screen space.
    pub x: UiUnit,
    /// Pointer y in screen space.
    pub y: UiUnit,
    /// Whether a press began this frame (the activating edge for `button`).
    pub pressed_edge: bool,
}

/// Options for a screen-space text draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiTextOpts {
    /// Baseline x.
    pub x: UiUnit,
    /// Baseline y.
    pub y: UiUnit,
    /// Text color.
    pub color: UiColor,
    /// Font size.
    pub size: UiUnit,
}

/// Options for a screen-space sprite draw (the texture is passed separately as a
/// kernel handle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiSpriteOpts {
    /// Left edge.
    pub x: UiUnit,
    /// Top edge.
    pub y: UiUnit,
    /// Drawn width.
    pub w: UiUnit,
    /// Drawn height.
    pub h: UiUnit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_and_color_round_trip() {
        assert_eq!(UiUnit::new(3.5).get(), 3.5);
        assert_eq!(UiUnit::default().get(), 0.0);
        assert_eq!(UiColor::new(0x1234_5678).rgba(), 0x1234_5678);
    }

    #[test]
    fn contains_is_half_open_on_both_axes() {
        let r = UiRect::new(UiUnit::new(10.0), UiUnit::new(20.0), UiUnit::new(30.0), UiUnit::new(40.0));
        assert!(r.contains(UiUnit::new(15.0), UiUnit::new(25.0)));
        assert!(r.contains(UiUnit::new(10.0), UiUnit::new(20.0)));
        assert!(!r.contains(UiUnit::new(40.0), UiUnit::new(25.0)));
        assert!(!r.contains(UiUnit::new(15.0), UiUnit::new(60.0)));
        assert!(!r.contains(UiUnit::new(5.0), UiUnit::new(25.0)));
        assert!(!r.contains(UiUnit::new(15.0), UiUnit::new(5.0)));
    }
}
