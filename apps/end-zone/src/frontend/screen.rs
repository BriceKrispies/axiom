//! The explicit screen vocabulary: seven named states, never booleans. There is
//! no attract mode, team select, match setup, or credits — and no play-call
//! screen: the prototype runs ONE concept, and the only choice it ever asks for
//! is made on the field, inside the decision window, not in a menu. The title
//! is the start plate; a first press opens the `Menu` (PLAY / SETTINGS), and
//! PLAY leads straight into gameplay.

/// The explicit screen states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Title,
    Menu,
    InGame,
    Paused,
    Settings,
    Controls,
    GameOver,
}

pub const SCREEN_COUNT: usize = 7;

impl Screen {
    pub fn index(self) -> usize {
        match self {
            Screen::Title => 0,
            Screen::Menu => 1,
            Screen::InGame => 2,
            Screen::Paused => 3,
            Screen::Settings => 4,
            Screen::Controls => 5,
            Screen::GameOver => 6,
        }
    }
}
