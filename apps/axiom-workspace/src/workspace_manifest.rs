//! [`WorkspaceManifest`] — the typed description of an opened workspace.
//!
//! A manifest is the top of the workspace's data: which workspace this is, where
//! its root lives (as an opaque, platform-neutral string — the workspace crate
//! touches no filesystem), and the schema version the on-disk workspace format
//! was written at. It is pure value data with an explicit validation step.

use axiom_kernel::SchemaVersion;

use crate::workspace_api::WorkspaceError;

/// A validated description of the workspace being opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceManifest {
    name: String,
    workspace_root: String,
    schema_version: SchemaVersion,
}

impl WorkspaceManifest {
    /// Validate typed fields into a manifest. `name` and `workspace_root` must be
    /// non-empty after trimming; the schema version is accepted as given.
    pub fn new(
        name: &str,
        workspace_root: &str,
        schema_version: SchemaVersion,
    ) -> Result<Self, WorkspaceError> {
        require_non_empty(name, "workspace.name")?;
        require_non_empty(workspace_root, "workspace.root")?;
        Ok(WorkspaceManifest {
            name: name.trim().to_string(),
            workspace_root: workspace_root.trim().to_string(),
            schema_version,
        })
    }

    /// The workspace name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The opaque workspace root path string.
    #[must_use]
    pub fn workspace_root(&self) -> &str {
        &self.workspace_root
    }

    /// The schema version the workspace format was authored at.
    #[must_use]
    pub fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }
}

/// Reject an empty-after-trim field with a precise [`WorkspaceError`]. Shared by
/// every workspace contract that validates typed text.
pub(crate) fn require_non_empty(value: &str, field: &'static str) -> Result<(), WorkspaceError> {
    match value.trim().is_empty() {
        true => Err(WorkspaceError::MissingField { field }),
        false => Ok(()),
    }
}
