//! The per-frame screen-space immediate-mode HUD surface (SPEC-09).
//!
//! Sits beside the retained `InterfaceState`: where panels persist and route
//! clicks by id, the `UiSurface` is rebuilt every frame and its `button` returns
//! activation *this frame*. The only cross-call state is the per-frame
//! [`PointerSnapshot`] installed by [`UiSurface::begin_frame`].

use axiom_kernel::HandleId;

use crate::ui_geometry::{PointerSnapshot, UiFill, UiRect, UiSpriteOpts, UiTextOpts, UiViewport};

/// One accumulated screen-space draw command. Immediate-mode: the whole list is
/// rebuilt each frame, so a consumer (canvas/GPU) repaints it after the world.
#[derive(Debug, Clone, PartialEq)]
pub enum UiDrawItem {
    /// A filled/stroked rectangle.
    Rect {
        /// Screen-space bounds.
        bounds: UiRect,
        /// Fill + stroke style.
        style: UiFill,
    },
    /// A run of screen-space text.
    Text {
        /// The string to draw.
        value: String,
        /// Position, color, size.
        opts: UiTextOpts,
    },
    /// A textured sprite, the texture named by a kernel handle.
    Sprite {
        /// Which texture to sample.
        texture: HandleId,
        /// Position + drawn size.
        opts: UiSpriteOpts,
    },
    /// An immediate-mode button, carrying whether it was activated this frame.
    Button {
        /// Hit-test bounds.
        bounds: UiRect,
        /// Button caption.
        label: String,
        /// Fill + stroke style.
        style: UiFill,
        /// `true` iff the pointer was inside `bounds` on its press edge this frame.
        activated: bool,
    },
}

/// This frame's ordered screen-space draw items (top-left origin, `+y` down).
/// Distinct from the retained integer `InterfaceDrawList`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiDrawList {
    items: Vec<UiDrawItem>,
}

impl UiDrawList {
    /// This frame's accumulated items, in submission order.
    pub fn items(&self) -> &[UiDrawItem] {
        &self.items
    }
}

/// The screen-space immediate-mode HUD surface. **Presentation-class** (§17.5):
/// every output is display-only and must never be read back into a `sim` API.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiSurface {
    viewport: UiViewport,
    pointer: PointerSnapshot,
    list: UiDrawList,
}

impl UiSurface {
    /// An empty surface (zero viewport, neutral pointer).
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a frame: install this frame's viewport and pointer snapshot and
    /// clear last frame's items (immediate-mode — nothing leaks across frames).
    pub fn begin_frame(&mut self, viewport: UiViewport, pointer: PointerSnapshot) {
        self.viewport = viewport;
        self.pointer = pointer;
        self.list.items.clear();
    }

    /// Draw a filled/stroked rectangle.
    pub fn rect(&mut self, bounds: UiRect, style: UiFill) {
        self.list.items.push(UiDrawItem::Rect { bounds, style });
    }

    /// Draw a run of text.
    pub fn text(&mut self, value: &str, opts: UiTextOpts) {
        self.list.items.push(UiDrawItem::Text {
            value: value.to_string(),
            opts,
        });
    }

    /// Draw a textured sprite.
    pub fn sprite(&mut self, texture: HandleId, opts: UiSpriteOpts) {
        self.list.items.push(UiDrawItem::Sprite { texture, opts });
    }

    /// Draw an immediate-mode button and return whether it was activated this
    /// frame — a pure function of `(bounds, this-frame pointer)`: pointer inside
    /// `bounds` on its press edge.
    pub fn button(&mut self, bounds: UiRect, label: &str, style: UiFill) -> bool {
        let activated = bounds.contains(self.pointer.x, self.pointer.y) & self.pointer.pressed_edge;
        self.list.items.push(UiDrawItem::Button {
            bounds,
            label: label.to_string(),
            style,
            activated,
        });
        activated
    }

    /// This frame's logical viewport.
    pub fn viewport(&self) -> UiViewport {
        self.viewport
    }

    /// This frame's accumulated screen-space draw list.
    pub fn draw_list(&self) -> &UiDrawList {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_geometry::UiColor;

    fn u(v: f32) -> crate::ui_geometry::UiUnit {
        crate::ui_geometry::UiUnit::new(v)
    }

    fn fill() -> UiFill {
        UiFill {
            fill: UiColor::new(0xff00_00ff),
            stroke: UiColor::new(0x0000_00ff),
            stroke_width: u(1.0),
        }
    }

    fn viewport() -> UiViewport {
        UiViewport {
            width: u(320.0),
            height: u(240.0),
        }
    }

    fn pointer(x: f32, y: f32, pressed_edge: bool) -> PointerSnapshot {
        PointerSnapshot {
            x: u(x),
            y: u(y),
            pressed_edge,
        }
    }

    #[test]
    fn begin_frame_installs_state_and_clears_last_frame() {
        let mut s = UiSurface::new();
        s.begin_frame(viewport(), pointer(0.0, 0.0, false));
        s.rect(UiRect::new(u(0.0), u(0.0), u(10.0), u(10.0)), fill());
        assert_eq!(s.draw_list().items().len(), 1);
        assert_eq!(s.viewport(), viewport());
        // A new frame resets the surface — last frame's items do not leak.
        s.begin_frame(viewport(), pointer(0.0, 0.0, false));
        assert!(s.draw_list().items().is_empty());
    }

    #[test]
    fn draw_items_accumulate_in_order_with_their_content() {
        let mut s = UiSurface::new();
        s.begin_frame(viewport(), pointer(0.0, 0.0, false));
        let r_bounds = UiRect::new(u(1.0), u(2.0), u(3.0), u(4.0));
        let t_opts = UiTextOpts { x: u(5.0), y: u(6.0), color: UiColor::new(0xffff_ffff), size: u(12.0) };
        let sp_opts = UiSpriteOpts { x: u(8.0), y: u(9.0), w: u(10.0), h: u(11.0) };
        s.rect(r_bounds, fill());
        s.text("hp", t_opts);
        s.sprite(HandleId::from_raw(7), sp_opts);
        // Whole-item equality keeps the assertions branch-free (no `matches!`
        // guard arm to leave uncovered).
        assert_eq!(
            s.draw_list().items(),
            &[
                UiDrawItem::Rect { bounds: r_bounds, style: fill() },
                UiDrawItem::Text { value: "hp".to_string(), opts: t_opts },
                UiDrawItem::Sprite { texture: HandleId::from_raw(7), opts: sp_opts },
            ]
        );
    }

    #[test]
    fn button_activation_truth_table_and_draw_item() {
        let bounds = UiRect::new(u(10.0), u(10.0), u(20.0), u(20.0));
        let f = fill();
        // Pointer inside + press edge => activated, and the draw item records it.
        let mut s = UiSurface::new();
        s.begin_frame(viewport(), pointer(15.0, 15.0, true));
        assert!(s.button(bounds, "ok", f));
        assert_eq!(
            s.draw_list().items(),
            &[UiDrawItem::Button { bounds, label: "ok".to_string(), style: f, activated: true }]
        );
        // Inside but no press edge => not activated.
        s.begin_frame(viewport(), pointer(15.0, 15.0, false));
        assert!(!s.button(bounds, "ok", f));
        // Press edge but pointer outside => not activated.
        s.begin_frame(viewport(), pointer(0.0, 0.0, true));
        assert!(!s.button(bounds, "ok", f));
        // Outside and no edge => not activated.
        s.begin_frame(viewport(), pointer(0.0, 0.0, false));
        assert!(!s.button(bounds, "ok", f));
    }
}
