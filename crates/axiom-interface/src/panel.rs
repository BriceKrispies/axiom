//! [`Panel`] — one interface panel: identity, an integer layout rect, visibility,
//! pinning, an in-progress drag, neutral content (header + label/value rows), and
//! its console model. Branchless. Focus lives in [`crate::focus_state`], not here,
//! because it transfers between panels.

use crate::console_model::ConsoleModel;
use crate::layout_rect::Rect;
use crate::panel_id::PanelId;

/// The default inset from the viewport corner and the default panel width.
const DEFAULT_INSET: i32 = 8;
const DEFAULT_WIDTH: i32 = 360;

/// One interface panel.
#[derive(Debug, Clone)]
pub(crate) struct Panel {
    id: PanelId,
    rect: Rect,
    visible: bool,
    pinned: bool,
    /// Pointer-to-top-left offset while dragging; `None` when idle.
    drag_grab: Option<(i32, i32)>,
    header_primary: String,
    header_secondary: String,
    rows: Vec<(String, String)>,
    console: ConsoleModel,
}

impl Panel {
    pub(crate) fn new(id: PanelId) -> Self {
        Panel {
            id,
            rect: Rect::new(DEFAULT_INSET, DEFAULT_INSET, DEFAULT_WIDTH, 0),
            visible: false,
            pinned: false,
            drag_grab: None,
            header_primary: String::new(),
            header_secondary: String::new(),
            rows: Vec::new(),
            console: ConsoleModel::new(),
        }
    }

    pub(crate) fn id(&self) -> PanelId {
        self.id
    }

    // --- visibility / pin ---------------------------------------------------

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn show(&mut self) {
        self.visible = true;
    }

    pub(crate) fn hide(&mut self) {
        self.visible = false;
    }

    /// Showing always works; hiding is suppressed while pinned (branchless
    /// `!visible | pinned`).
    pub(crate) fn toggle(&mut self) {
        self.visible = !self.visible | self.pinned;
    }

    pub(crate) fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub(crate) fn pin(&mut self) {
        self.pinned = true;
        self.visible = true;
    }

    pub(crate) fn unpin(&mut self) {
        self.pinned = false;
    }

    pub(crate) fn toggle_pin(&mut self) {
        let was_pinned = self.pinned;
        self.visible = self.visible | !was_pinned;
        self.pinned = !was_pinned;
    }

    // --- layout / drag ------------------------------------------------------

    pub(crate) fn position(&self) -> (i32, i32) {
        self.rect.position()
    }

    pub(crate) fn rect(&self) -> Rect {
        self.rect
    }

    pub(crate) fn set_width(&mut self, width: i32) {
        self.rect = self.rect.with_width(width);
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.drag_grab.is_some()
    }

    pub(crate) fn drag_begin(&mut self, pointer_x: i32, pointer_y: i32) {
        let (x, y) = self.rect.position();
        self.drag_grab = Some((pointer_x - x, pointer_y - y));
    }

    pub(crate) fn drag_update(&mut self, pointer_x: i32, pointer_y: i32, max_x: i32, max_y: i32) {
        let grab = self.drag_grab;
        grab.map(|(grab_x, grab_y)| {
            self.rect = self
                .rect
                .with_position(pointer_x - grab_x, pointer_y - grab_y)
                .clamped(max_x, max_y);
        });
    }

    pub(crate) fn drag_end(&mut self) {
        self.drag_grab = None;
    }

    // --- content ------------------------------------------------------------

    pub(crate) fn set_header(&mut self, primary: &str, secondary: &str) {
        self.header_primary = primary.to_string();
        self.header_secondary = secondary.to_string();
    }

    pub(crate) fn header_primary(&self) -> &str {
        &self.header_primary
    }

    pub(crate) fn header_secondary(&self) -> &str {
        &self.header_secondary
    }

    pub(crate) fn set_rows(&mut self, rows: &[(String, String)]) {
        self.rows = rows.to_vec();
    }

    pub(crate) fn rows(&self) -> &[(String, String)] {
        &self.rows
    }

    pub(crate) fn console(&self) -> &ConsoleModel {
        &self.console
    }

    pub(crate) fn console_mut(&mut self) -> &mut ConsoleModel {
        &mut self.console
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::HandleId;

    fn panel() -> Panel {
        Panel::new(PanelId::from_handle(HandleId::from_raw(1)))
    }

    #[test]
    fn starts_hidden_unpinned_at_default_rect() {
        let p = panel();
        assert!(!p.is_visible() && !p.is_pinned());
        assert_eq!(p.position(), (8, 8));
        assert_eq!(p.rect().width, 360);
        assert_eq!(p.id().raw(), 1);
    }

    #[test]
    fn visibility_toggle_and_pin() {
        let mut p = panel();
        p.show();
        assert!(p.is_visible());
        p.toggle();
        assert!(!p.is_visible());
        p.toggle();
        assert!(p.is_visible());
        p.pin();
        p.toggle(); // pinned: stays
        assert!(p.is_visible() && p.is_pinned());
        p.unpin();
        p.hide();
        assert!(!p.is_visible());
        p.toggle_pin(); // unpinned + hidden -> pins and shows
        assert!(p.is_pinned() && p.is_visible());
        p.toggle_pin(); // pinned -> unpins, keeps visibility
        assert!(!p.is_pinned() && p.is_visible());
    }

    #[test]
    fn drag_moves_and_clamps() {
        let mut p = panel();
        assert!(!p.is_dragging());
        p.drag_begin(20, 18); // grab offset (12, 10)
        assert!(p.is_dragging());
        p.drag_update(120, 118, 1000, 800);
        assert_eq!(p.position(), (108, 108));
        p.drag_update(-500, -500, 1000, 800);
        assert_eq!(p.position(), (0, 0));
        p.drag_end();
        assert!(!p.is_dragging());
        p.drag_update(300, 300, 1000, 800); // no-op without a grab
        assert_eq!(p.position(), (0, 0));
    }

    #[test]
    fn width_and_content_round_trip() {
        let mut p = panel();
        p.set_width(460);
        assert_eq!(p.rect().width, 460);
        p.set_header("Title", "status");
        assert_eq!(p.header_primary(), "Title");
        assert_eq!(p.header_secondary(), "status");
        p.set_rows(&[("a".to_string(), "1".to_string())]);
        assert_eq!(p.rows(), &[("a".to_string(), "1".to_string())]);
        p.console_mut().record("help");
        assert_eq!(p.console().history_len(), 1);
    }
}
