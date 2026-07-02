//! [`AssetBrowserState`] — the typed placeholder state of the Asset Browser
//! panel: an ordered list of asset rows and an optional selection.
//!
//! Pure value data — the panel simulates nothing. Rows are placeholders until a
//! future integration lists real assets from an opened project.

/// One placeholder row in the Asset Browser: a stable id, a kind label, and a
/// display name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetEntry {
    /// The stable asset id.
    pub id: String,
    /// The asset kind label (e.g. a placeholder like "mesh" or "texture").
    pub kind: String,
    /// The human-facing asset name.
    pub name: String,
}

impl AssetEntry {
    /// Build a placeholder asset row.
    #[must_use]
    pub fn new(id: &str, kind: &str, name: &str) -> Self {
        AssetEntry {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
        }
    }
}

/// The Asset Browser panel state: an ordered list of placeholder asset rows plus
/// an optional selected index. `Default` is empty (rows are placeholders).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetBrowserState {
    assets: Vec<AssetEntry>,
    selected: Option<usize>,
}

impl AssetBrowserState {
    /// Append an asset row, preserving insertion order exactly.
    pub fn add_asset(&mut self, asset: AssetEntry) {
        self.assets.push(asset);
    }

    /// The asset rows, in insertion order.
    #[must_use]
    pub fn assets(&self) -> &[AssetEntry] {
        &self.assets
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
