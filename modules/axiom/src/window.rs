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
    // `None` means use the engine default.
    surface_id: Option<String>,
}

impl Window {
    /// A surface of `width` x `height` physical pixels, clearing to black until
    /// [`Self::with_clear_color`] sets otherwise.
    pub fn new(width: u32, height: u32) -> Self {
        Window {
            width,
            height,
            clear_color: Color::BLACK,
            surface_id: None,
        }
    }

    /// Set the clear colour.
    pub fn with_clear_color(mut self, clear_color: Color) -> Self {
        self.clear_color = clear_color;
        self
    }

    /// Bind the live backend to the presentation-target element with this id
    /// (the web drawing element, on the web).
    pub fn with_surface_id(mut self, surface_id: &str) -> Self {
        self.surface_id = Some(surface_id.to_string());
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

    /// The presentation-target element id the live backend binds to, if set.
    pub fn surface_id(&self) -> Option<&str> {
        self.surface_id.as_deref()
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
        assert_eq!(w.surface_id(), None);
    }

    #[test]
    fn with_surface_id_sets_the_binding_target() {
        let w = Window::new(640, 480).with_surface_id("axiom-cube-surface");
        assert_eq!(w.surface_id(), Some("axiom-cube-surface"));
    }

    #[test]
    fn with_clear_color_overrides() {
        use axiom_kernel::Ratio;
        let bg = || {
            Color::linear_rgb(
                Ratio::new(0.05).expect("authored colour channel is finite"),
                Ratio::new(0.06).expect("authored colour channel is finite"),
                Ratio::new(0.08).expect("authored colour channel is finite"),
            )
        };
        let w = Window::new(320, 240).with_clear_color(bg());
        assert_eq!(w.width(), 320);
        assert_eq!(w.height(), 240);
        assert_eq!(w.clear_color(), bg());
    }
}
