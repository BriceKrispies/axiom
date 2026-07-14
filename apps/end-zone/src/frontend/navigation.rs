//! Deterministic focus + navigation: per-screen focus lists with a stable
//! order, directional movement over a row/column grid, pointer hover focus
//! and activation, focus memory for returning screens, and modal focus
//! confinement. Screens declare entries; they never inspect raw devices.

use axiom_interface::UiRect;

use super::layout::contains;

/// A semantic widget identity, unique within its screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetId(pub u32);

/// One focusable entry: id + hit rect + grid coordinates + enablement.
/// Entries are declared in deterministic order; `row`/`col` drive directional
/// movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusEntry {
    pub id: WidgetId,
    pub rect: UiRect,
    pub row: i16,
    pub col: i16,
    pub enabled: bool,
}

impl FocusEntry {
    pub fn new(id: WidgetId, rect: UiRect, row: i16, col: i16) -> Self {
        FocusEntry {
            id,
            rect,
            row,
            col,
            enabled: true,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// The outcome of a directional move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveOutcome {
    Moved,
    /// No focusable entry in that direction (screens may treat horizontal
    /// misses as value adjustment).
    Edge,
}

/// The focus model over one screen's entries.
#[derive(Debug, Clone, Default)]
pub struct FocusList {
    entries: Vec<FocusEntry>,
    focused: usize,
}

impl FocusList {
    /// Build a list focused on `remembered` when it exists and is enabled,
    /// else the first enabled entry (a screen with no enabled entries keeps
    /// index 0 and reports no focus).
    pub fn new(entries: Vec<FocusEntry>, remembered: Option<WidgetId>) -> Self {
        let mut list = FocusList {
            entries,
            focused: 0,
        };
        let target = remembered
            .and_then(|id| list.index_of(id))
            .filter(|&i| list.entries[i].enabled)
            .or_else(|| list.first_enabled());
        if let Some(index) = target {
            list.focused = index;
        }
        list
    }

    pub fn entries(&self) -> &[FocusEntry] {
        &self.entries
    }

    fn index_of(&self, id: WidgetId) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    fn first_enabled(&self) -> Option<usize> {
        self.entries.iter().position(|e| e.enabled)
    }

    /// The focused widget id, when any entry is focusable.
    pub fn focused(&self) -> Option<WidgetId> {
        self.entries
            .get(self.focused)
            .filter(|e| e.enabled)
            .map(|e| e.id)
    }

    /// Focus a specific enabled widget.
    pub fn focus(&mut self, id: WidgetId) -> bool {
        match self.index_of(id).filter(|&i| self.entries[i].enabled) {
            Some(index) => {
                self.focused = index;
                true
            }
            None => false,
        }
    }

    /// Directional move on the row/column grid: pick the nearest enabled
    /// entry strictly in that direction (primary axis distance first, then
    /// cross-axis distance, then declaration order — fully deterministic).
    pub fn step(&mut self, dx: i32, dy: i32) -> MoveOutcome {
        let Some(current) = self.entries.get(self.focused).copied() else {
            return MoveOutcome::Edge;
        };
        let mut best: Option<(i32, i32, usize)> = None;
        for (index, entry) in self.entries.iter().enumerate() {
            if !entry.enabled || index == self.focused {
                continue;
            }
            let drow = i32::from(entry.row) - i32::from(current.row);
            let dcol = i32::from(entry.col) - i32::from(current.col);
            let (primary, cross) = if dy != 0 {
                (drow * dy.signum(), dcol.abs())
            } else {
                (dcol * dx.signum(), drow.abs())
            };
            if primary <= 0 {
                continue;
            }
            let key = (primary, cross, index);
            let better = best.map(|(bp, bc, bi)| key < (bp, bc, bi)).unwrap_or(true);
            if better {
                best = Some(key);
            }
        }
        match best {
            Some((_, _, index)) => {
                self.focused = index;
                MoveOutcome::Moved
            }
            None => MoveOutcome::Edge,
        }
    }

    /// Hover focus: focus the enabled entry under the pointer, if any.
    pub fn hover(&mut self, x: f32, y: f32) -> Option<WidgetId> {
        let hit = self
            .entries
            .iter()
            .position(|e| e.enabled && contains(e.rect, x, y));
        if let Some(index) = hit {
            self.focused = index;
            return Some(self.entries[index].id);
        }
        None
    }

    /// Pointer activation: the enabled entry under the point (also focuses it).
    pub fn activate_at(&mut self, x: f32, y: f32) -> Option<WidgetId> {
        self.hover(x, y)
    }
}
