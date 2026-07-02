//! [`LevelBrowserState`] — the typed placeholder state of the Level Browser
//! panel: an ordered list of level rows and an optional selection.
//!
//! Pure value data — the panel simulates nothing. Rows are placeholders until a
//! future integration lists real levels from an opened project.

/// One placeholder row in the Level Browser: a stable id and a display name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LevelEntry {
    /// The stable level id.
    pub id: String,
    /// The human-facing level name.
    pub name: String,
}

impl LevelEntry {
    /// Build a placeholder level row.
    #[must_use]
    pub fn new(id: &str, name: &str) -> Self {
        LevelEntry {
            id: id.to_string(),
            name: name.to_string(),
        }
    }
}

/// The Level Browser panel state: an ordered list of placeholder level rows plus
/// an optional selected index. `Default` is empty (rows are placeholders).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LevelBrowserState {
    levels: Vec<LevelEntry>,
    selected: Option<usize>,
}

impl LevelBrowserState {
    /// Append a level row, preserving insertion order exactly.
    pub fn add_level(&mut self, level: LevelEntry) {
        self.levels.push(level);
    }

    /// The level rows, in insertion order.
    #[must_use]
    pub fn levels(&self) -> &[LevelEntry] {
        &self.levels
    }

    /// The selected row index, if any.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Set the selected row index.
    pub fn select(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }
}
