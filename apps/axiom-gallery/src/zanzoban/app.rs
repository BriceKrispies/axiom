//! The top-level app: an edit ⟷ playtest mode machine.
//!
//! [`ZanzobanApp`] owns the [`EditorModel`] and, while playtesting, a
//! [`PlaytestSession`]. It is the orchestration the browser shell drives: paint
//! and validate in edit mode, switch to playtest **only when the level
//! validates**, play, and return to edit mode without losing the edited level.
//! It is pure and browser-free; the wasm `web` arm is a thin adapter over it.

use crate::zanzoban::editor_model::EditorModel;
use crate::zanzoban::level_codec;
use crate::zanzoban::level_definition::LevelDefinition;
use crate::zanzoban::playtest_model::PlaytestSession;
use crate::zanzoban::render_model::RenderModel;

/// Which surface is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Authoring the level.
    Edit,
    /// Playing the validated level.
    Playtest,
}

/// The whole app: the editor, the optional live playtest, and the active mode.
#[derive(Debug)]
pub struct ZanzobanApp {
    editor: EditorModel,
    playtest: Option<PlaytestSession>,
    mode: Mode,
}

impl Default for ZanzobanApp {
    fn default() -> Self {
        ZanzobanApp::new()
    }
}

impl ZanzobanApp {
    /// A fresh app in edit mode, pre-loaded with the built-in Level 001. If the
    /// embedded level somehow fails to parse, it falls back to a blank editor so
    /// construction is always total.
    pub fn new() -> Self {
        let editor = level_codec::from_toml(crate::zanzoban::LEVEL_001_TOML)
            .map(|level| EditorModel::from_level(&level))
            .unwrap_or_default();
        ZanzobanApp {
            editor,
            playtest: None,
            mode: Mode::Edit,
        }
    }

    /// A fresh app in edit mode, pre-loaded with `level`.
    pub fn with_level(level: &LevelDefinition) -> Self {
        ZanzobanApp {
            editor: EditorModel::from_level(level),
            playtest: None,
            mode: Mode::Edit,
        }
    }

    /// The active mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The editor (edit-mode surface).
    pub fn editor(&self) -> &EditorModel {
        &self.editor
    }

    /// The editor, mutably (paint, select, import, etc.).
    pub fn editor_mut(&mut self) -> &mut EditorModel {
        &mut self.editor
    }

    /// The live playtest session, if playtesting.
    pub fn playtest(&self) -> Option<&PlaytestSession> {
        self.playtest.as_ref()
    }

    /// The live playtest session, mutably.
    pub fn playtest_mut(&mut self) -> Option<&mut PlaytestSession> {
        self.playtest.as_mut()
    }

    /// Try to enter playtest. Builds a fresh session from the edited level and
    /// switches mode **only if the level validates**; returns whether it did.
    pub fn enter_playtest(&mut self) -> bool {
        if self.editor.can_playtest() {
            self.playtest = Some(PlaytestSession::new(self.editor.to_level_definition()));
            self.mode = Mode::Playtest;
            true
        } else {
            false
        }
    }

    /// Return to edit mode, keeping the edited level intact. The playtest session
    /// is discarded (a fresh one is built on the next `enter_playtest`).
    pub fn enter_edit(&mut self) {
        self.playtest = None;
        self.mode = Mode::Edit;
    }

    /// The render model for the active mode (the edited grid, or the live game).
    pub fn render_model(&self) -> RenderModel {
        match (self.mode, &self.playtest) {
            (Mode::Playtest, Some(session)) => session.render_model(),
            _ => self.editor.render_model(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zanzoban::tile_kind::TileKind;

    #[test]
    fn opens_in_edit_mode_with_level_001_loaded() {
        let app = ZanzobanApp::new();
        assert_eq!(app.mode(), Mode::Edit);
        // Level 001's entrance is at (1,5).
        assert_eq!(app.editor().tile_at(1, 5), TileKind::Entrance);
        assert!(app.editor().can_playtest());
    }

    #[test]
    fn enter_playtest_requires_a_valid_level() {
        let mut app = ZanzobanApp::new();
        app.editor_mut().select(TileKind::Floor);
        app.editor_mut().paint(1, 5);
        assert!(!app.enter_playtest());
        assert_eq!(app.mode(), Mode::Edit);

        app.editor_mut().select(TileKind::Entrance);
        app.editor_mut().paint(1, 5);
        assert!(app.enter_playtest());
        assert_eq!(app.mode(), Mode::Playtest);
        assert!(app.playtest().is_some());
    }

    #[test]
    fn returning_to_edit_keeps_the_edited_level() {
        let mut app = ZanzobanApp::new();
        app.editor_mut().set_title("My Level");
        assert!(app.enter_playtest());
        app.enter_edit();
        assert_eq!(app.mode(), Mode::Edit);
        assert_eq!(app.editor().title(), "My Level");
        assert!(app.playtest().is_none());
    }
}
