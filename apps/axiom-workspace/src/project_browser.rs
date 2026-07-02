//! [`ProjectBrowserState`] — the typed placeholder state of the Project Browser
//! panel: an ordered list of project rows and an optional selection.
//!
//! This is pure value data — the panel simulates nothing. Rows are placeholders
//! until a future integration lists real projects from an opened workspace.

/// One placeholder row in the Project Browser: a stable id and a display name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectEntry {
    /// The stable project id.
    pub id: String,
    /// The human-facing project name.
    pub name: String,
}

impl ProjectEntry {
    /// Build a placeholder project row.
    #[must_use]
    pub fn new(id: &str, name: &str) -> Self {
        ProjectEntry {
            id: id.to_string(),
            name: name.to_string(),
        }
    }
}

/// The Project Browser panel state: an ordered list of placeholder project rows
/// plus an optional selected index. `Default` is empty (rows are placeholders).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectBrowserState {
    projects: Vec<ProjectEntry>,
    selected: Option<usize>,
}

impl ProjectBrowserState {
    /// Append a project row, preserving insertion order exactly.
    pub fn add_project(&mut self, project: ProjectEntry) {
        self.projects.push(project);
    }

    /// The project rows, in insertion order.
    #[must_use]
    pub fn projects(&self) -> &[ProjectEntry] {
        &self.projects
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
