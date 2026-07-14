//! The explicit screen vocabulary: eleven named states, never booleans.

/// The explicit screen states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Attract,
    Title,
    MainMenu,
    TeamSelect,
    MatchSetup,
    Settings,
    Credits,
    TransitionToGame,
    InGame,
    Paused,
    TransitionToMenu,
}

pub const SCREEN_COUNT: usize = 11;

impl Screen {
    pub fn index(self) -> usize {
        match self {
            Screen::Attract => 0,
            Screen::Title => 1,
            Screen::MainMenu => 2,
            Screen::TeamSelect => 3,
            Screen::MatchSetup => 4,
            Screen::Settings => 5,
            Screen::Credits => 6,
            Screen::TransitionToGame => 7,
            Screen::InGame => 8,
            Screen::Paused => 9,
            Screen::TransitionToMenu => 10,
        }
    }
}
