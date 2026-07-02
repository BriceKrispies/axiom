//! [`GameManifestEditorState`] — the typed placeholder state of the Game Manifest
//! Editor panel.
//!
//! The editor edits a game manifest as **typed fields**, not a raw text blob:
//! title, version, entrypoint, and default level. Every field defaults to the
//! explicit placeholder string `"<unset>"` until a future integration loads a
//! real manifest. Setters return an updated state so the shell can thread edits
//! immutably.

/// The explicit placeholder value every unset manifest field carries.
const UNSET: &str = "<unset>";

/// The Game Manifest Editor panel state: the typed, placeholder-populated fields
/// of a game manifest under edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameManifestEditorState {
    title: String,
    version: String,
    entrypoint: String,
    default_level: String,
}

impl Default for GameManifestEditorState {
    /// Every field starts at the placeholder string `"<unset>"`.
    fn default() -> Self {
        GameManifestEditorState {
            title: UNSET.to_string(),
            version: UNSET.to_string(),
            entrypoint: UNSET.to_string(),
            default_level: UNSET.to_string(),
        }
    }
}

impl GameManifestEditorState {
    /// The manifest title (placeholder `"<unset>"` until edited).
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The manifest version (placeholder `"<unset>"` until edited).
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The manifest entrypoint (placeholder `"<unset>"` until edited).
    #[must_use]
    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }

    /// The manifest default level (placeholder `"<unset>"` until edited).
    #[must_use]
    pub fn default_level(&self) -> &str {
        &self.default_level
    }

    /// Return an updated state with the title set.
    #[must_use]
    pub fn with_title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    /// Return an updated state with the version set.
    #[must_use]
    pub fn with_version(mut self, version: &str) -> Self {
        self.version = version.to_string();
        self
    }

    /// Return an updated state with the entrypoint set.
    #[must_use]
    pub fn with_entrypoint(mut self, entrypoint: &str) -> Self {
        self.entrypoint = entrypoint.to_string();
        self
    }

    /// Return an updated state with the default level set.
    #[must_use]
    pub fn with_default_level(mut self, default_level: &str) -> Self {
        self.default_level = default_level.to_string();
        self
    }
}
