//! [`GameProject`] — the typed descriptor of the project the workspace opens.
//!
//! A game project is *what* the workspace is opening: a stable id, a display
//! name, a version, and the entrypoint the runtime will later be asked to launch.
//! It is a black-box descriptor — the workspace does not know how to run it, only
//! how to name it and hand it to a future runtime integration.

use crate::workspace_api::WorkspaceError;
use crate::workspace_manifest::require_non_empty;

/// A validated descriptor of a game/runtime project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProject {
    id: String,
    name: String,
    version: String,
    entrypoint: String,
}

impl GameProject {
    /// Validate typed fields into a project descriptor. `id`, `name`, `version`,
    /// and `entrypoint` must each be non-empty after trimming.
    pub fn new(
        id: &str,
        name: &str,
        version: &str,
        entrypoint: &str,
    ) -> Result<Self, WorkspaceError> {
        require_non_empty(id, "project.id")?;
        require_non_empty(name, "project.name")?;
        require_non_empty(version, "project.version")?;
        require_non_empty(entrypoint, "project.entrypoint")?;
        Ok(GameProject {
            id: id.trim().to_string(),
            name: name.trim().to_string(),
            version: version.trim().to_string(),
            entrypoint: entrypoint.trim().to_string(),
        })
    }

    /// The stable project id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The human-facing project name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The project version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The entrypoint the runtime is asked to launch.
    #[must_use]
    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }
}
