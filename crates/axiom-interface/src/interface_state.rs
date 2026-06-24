//! [`InterfaceState`] — the interface tree's storage: its panels, the single focus
//! owner, and the monotonic id source. It also assembles a panel's neutral draw
//! list. Branchless; covered through [`crate::InterfaceApi`].

use crate::draw_list::{InterfaceDrawItem, InterfaceDrawList};
use crate::focus_state::FocusState;
use crate::panel::Panel;
use crate::panel_id::PanelId;

/// How many console result lines a panel renders above its input.
pub(crate) const RECENT_RESULTS: usize = 5;

/// The panels, the focus owner, and the next raw id to mint.
#[derive(Debug)]
pub(crate) struct InterfaceState {
    panels: Vec<Panel>,
    focus: FocusState,
    next_raw: u64,
}

impl InterfaceState {
    pub(crate) fn new() -> Self {
        // Start at 1 so minted handles are valid (raw 0 is the kernel's NULL).
        InterfaceState {
            panels: Vec::new(),
            focus: FocusState::new(),
            next_raw: 1,
        }
    }

    /// The next raw id, advancing the counter.
    pub(crate) fn next_raw(&mut self) -> u64 {
        let raw = self.next_raw;
        self.next_raw += 1;
        raw
    }

    pub(crate) fn insert_panel(&mut self, id: PanelId) {
        self.panels.push(Panel::new(id));
    }

    pub(crate) fn panel(&self, id: PanelId) -> Option<&Panel> {
        self.panels.iter().find(|panel| panel.id() == id)
    }

    pub(crate) fn panel_mut(&mut self, id: PanelId) -> Option<&mut Panel> {
        self.panels.iter_mut().find(|panel| panel.id() == id)
    }

    pub(crate) fn focus(&self) -> &FocusState {
        &self.focus
    }

    pub(crate) fn focus_mut(&mut self) -> &mut FocusState {
        &mut self.focus
    }

    /// The deterministic, ordered draw list for one panel — empty if the panel is
    /// hidden or unknown.
    pub(crate) fn draw_list(&self, id: PanelId) -> InterfaceDrawList {
        let focused = self.focus.is_focused(id);
        let items = self
            .panel(id)
            .filter(|panel| panel.is_visible())
            .map(|panel| panel_items(panel, focused))
            .unwrap_or_default();
        InterfaceDrawList::new(items)
    }
}

/// Assemble one visible panel's draw items: background, header, rows, console
/// result lines, then the console input marker — in that fixed order.
fn panel_items(panel: &Panel, focused: bool) -> Vec<InterfaceDrawItem> {
    let rect = panel.rect();
    let mut items = vec![
        InterfaceDrawItem::Panel {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        },
        InterfaceDrawItem::Header {
            primary: panel.header_primary().to_string(),
            secondary: panel.header_secondary().to_string(),
        },
    ];
    panel.rows().iter().for_each(|(label, value)| {
        items.push(InterfaceDrawItem::Row {
            label: label.clone(),
            value: value.clone(),
        });
    });
    panel
        .console()
        .recent_results(RECENT_RESULTS)
        .iter()
        .for_each(|outcome| {
            items.push(InterfaceDrawItem::ConsoleLine {
                ok: outcome.succeeded(),
                text: format!("{}: {}", outcome.command(), outcome.message()),
            });
        });
    items.push(InterfaceDrawItem::ConsoleInput {
        prompt: ">".to_string(),
        focused,
    });
    items
}
