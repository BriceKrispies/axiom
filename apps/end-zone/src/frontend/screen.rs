//! The explicit screen vocabulary: eight named states, never booleans. There is
//! no attract mode, team select, match setup, or credits. The title is the
//! start plate; a first press opens the `Menu` (PLAY / SETTINGS), and PLAY leads
//! into gameplay. Between every down the `Huddle` opens for the player to call a
//! play.

/// The explicit screen states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Title,
    Menu,
    InGame,
    Huddle,
    Paused,
    Settings,
    Controls,
    GameOver,
}

pub const SCREEN_COUNT: usize = 8;

impl Screen {
    pub fn index(self) -> usize {
        match self {
            Screen::Title => 0,
            Screen::Menu => 1,
            Screen::InGame => 2,
            Screen::Huddle => 3,
            Screen::Paused => 4,
            Screen::Settings => 5,
            Screen::Controls => 6,
            Screen::GameOver => 7,
        }
    }
}
