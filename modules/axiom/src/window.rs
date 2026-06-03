//! The surface configuration for an [`crate::prelude::App`].

use crate::color::Color;

/// The render surface size and clear colour.
///
/// The umbrella is platform-free: it carries only the physical pixel dimensions
/// (the aspect ratio cameras resolve against) and the clear colour. *Where* the
/// surface lives — a web drawing element, a native window handle — is a
/// windowing-backend concern supplied separately by the platform module, never
/// named here.
#[derive(Debug, Clone)]
pub struct Window {
    width: u32,
    height: u32,
    clear_color: Color,
}

impl Window {
    /// A surface of `width` x `height` physical pixels, clearing to black until
    /// [`Self::with_clear_color`] sets otherwise.
    pub fn new(width: u32, height: u32) -> Self {
        Window {
            width,
            height,
            clear_color: Color::BLACK,
        }
    }

    /// Set the clear colour.
    pub fn with_clear_color(mut self, clear_color: Color) -> Self {
        self.clear_color = clear_color;
        self
    }

    /// The surface width in physical pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The surface height in physical pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The clear colour.
    pub fn clear_color(&self) -> Color {
        self.clear_color
    }
}

impl Default for Window {
    fn default() -> Self {
        Window::new(800, 600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_accessors() {
        let w = Window::default();
        assert_eq!(w.width(), 800);
        assert_eq!(w.height(), 600);
        assert_eq!(w.clear_color(), Color::BLACK);
    }

    #[test]
    fn with_clear_color_overrides() {
        let w = Window::new(320, 240).with_clear_color(Color::linear_rgb(0.05, 0.06, 0.08));
        assert_eq!(w.width(), 320);
        assert_eq!(w.height(), 240);
        assert_eq!(w.clear_color(), Color::linear_rgb(0.05, 0.06, 0.08));
    }
}
