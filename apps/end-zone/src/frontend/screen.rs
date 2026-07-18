//! The explicit screen vocabulary: six named states, never booleans. There is
//! no attract mode, main menu, team select, match setup, or credits — the
//! title leads straight into gameplay.

/// The explicit screen states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Title,
    InGame,
    Paused,
    Settings,
    Controls,
    GameOver,
}

pub const SCREEN_COUNT: usize = 6;

impl Screen {
    pub fn index(self) -> usize {
        match self {
            Screen::Title => 0,
            Screen::InGame => 1,
            Screen::Paused => 2,
            Screen::Settings => 3,
            Screen::Controls => 4,
            Screen::GameOver => 5,
        }
    }
}
